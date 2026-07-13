use persistence::{PersistenceConfig, PostgresPersistence, WIPEABLE_CORE_NAMESPACE};
use server_app::{
    AccountId, AshReasonCode, AshWalletMutationFrame, AshWalletMutationKind,
    AshWalletMutationPayload, AshWalletResultCode, AuthenticatedAccount, AuthenticatedNamespace,
    IdentityClock, PostgresAshWalletService,
};
use sqlx::Row;

const ACCOUNT_ID: [u8; 16] = [81; 16];
const CAP_ACCOUNT_ID: [u8; 16] = [82; 16];
const CONTENT_VERSION: &str =
    "core-dev.blake3.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Debug, Clone, Copy)]
struct FixedClock;

impl IdentityClock for FixedClock {
    fn unix_millis(&self) -> u64 {
        10_000
    }
}

fn authenticated(account_id: [u8; 16]) -> AuthenticatedAccount {
    AuthenticatedAccount {
        account_id: AccountId::new(account_id).unwrap(),
        namespace: AuthenticatedNamespace::WipeableTest,
    }
}

fn command(
    mutation: u8,
    expected_wallet_version: u64,
    reason: AshReasonCode,
    kind: AshWalletMutationKind,
    amount: u32,
) -> AshWalletMutationFrame {
    let payload = AshWalletMutationPayload {
        kind,
        reason,
        amount,
        source_id: format!("fixture.source.{mutation}"),
        content_version: CONTENT_VERSION.into(),
    };
    AshWalletMutationFrame {
        mutation_id: [mutation; 16],
        expected_wallet_version,
        payload_hash: payload.canonical_hash(),
        issued_at_unix_millis: 9_999,
        payload,
    }
}

async fn disposable_database() -> PostgresPersistence {
    let config = PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL");
    let persistence = PostgresPersistence::connect(&config).await.unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence.migrate().await.unwrap();
    persistence
}

async fn reset_fixture(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    for account_id in [ACCOUNT_ID, CAP_ACCOUNT_ID] {
        sqlx::query("DELETE FROM accounts WHERE namespace_id = $1 AND account_id = $2")
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(account_id.as_slice())
            .execute(transaction.connection())
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO accounts (namespace_id, account_id, state_version, slot_capacity) \
             VALUES ($1, $2, 1, 2)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO ash_wallets (namespace_id, account_id, balance, wallet_version) \
         VALUES ($1, $2, 99999, 1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(CAP_ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn assert_durable_ledger(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let ledger = sqlx::query(
        "SELECT COUNT(*) AS event_count, COALESCE(SUM(delta), 0) AS net_delta, \
                MIN(wallet_version) AS first_version, MAX(wallet_version) AS last_version \
         FROM currency_ledger_events WHERE namespace_id = $1 AND account_id = $2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(ledger.get::<i64, _>("event_count"), 3);
    assert_eq!(ledger.get::<i64, _>("net_delta"), 10);
    assert_eq!(ledger.get::<Option<i64>, _>("first_version"), Some(2));
    assert_eq!(ledger.get::<Option<i64>, _>("last_version"), Some(4));

    let results: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ash_mutation_results \
         WHERE namespace_id = $1 AND account_id = $2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(results, 5);

    let accepted_rows = sqlx::query(
        "SELECT reason_code, source_id, content_version, before_balance, delta, after_balance \
         FROM currency_ledger_events WHERE namespace_id = $1 AND account_id = $2 \
         ORDER BY wallet_version",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .fetch_all(transaction.connection())
    .await
    .unwrap();
    assert_eq!(
        accepted_rows[0].get::<&str, _>("reason_code"),
        "drowned_reliquary_boss"
    );
    assert_eq!(
        accepted_rows[0].get::<&str, _>("source_id"),
        "fixture.source.1"
    );
    assert_eq!(
        accepted_rows[0].get::<&str, _>("content_version"),
        CONTENT_VERSION
    );
    assert_eq!(
        (
            accepted_rows[0].get::<i32, _>("before_balance"),
            accepted_rows[0].get::<i32, _>("delta"),
            accepted_rows[0].get::<i32, _>("after_balance"),
        ),
        (0, 40, 40)
    );
    transaction.rollback().await.unwrap();
}

async fn race_same_wallet_version(
    service: &PostgresAshWalletService<FixedClock>,
    account: AuthenticatedAccount,
) {
    let first = command(
        2,
        2,
        AshReasonCode::MinorRealmEvent,
        AshWalletMutationKind::Earn,
        10,
    );
    let second = command(
        3,
        2,
        AshReasonCode::MinorRealmEvent,
        AshWalletMutationKind::Earn,
        10,
    );
    let (a, b) = tokio::join!(
        service.mutate(account, &first),
        service.mutate(account, &second)
    );
    let codes = [a.code, b.code];
    for (code, expected_count) in [
        (AshWalletResultCode::Accepted, 1),
        (AshWalletResultCode::StateVersionMismatch, 1),
    ] {
        assert_eq!(
            codes.iter().filter(|value| **value == code).count(),
            expected_count
        );
    }
}

async fn assert_rejected_boundaries(
    service: &PostgresAshWalletService<FixedClock>,
    account: AuthenticatedAccount,
) {
    let insufficient = command(
        5,
        4,
        AshReasonCode::BargainPurge,
        AshWalletMutationKind::Spend,
        50,
    );
    let rejected = service.mutate(account, &insufficient).await;
    assert_eq!(rejected.code, AshWalletResultCode::InsufficientBalance);
    assert_eq!(
        rejected
            .projection
            .map(|value| (value.balance, value.wallet_version)),
        Some((10, 4))
    );

    let capped = command(
        6,
        1,
        AshReasonCode::MinorRealmEvent,
        AshWalletMutationKind::Earn,
        10,
    );
    let cap_result = service.mutate(authenticated(CAP_ACCOUNT_ID), &capped).await;
    assert_eq!(cap_result.code, AshWalletResultCode::CapExceeded);
    assert_eq!(
        cap_result
            .projection
            .map(|value| (value.balance, value.wallet_version)),
        Some((99_999, 1))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn ash_wallet_is_replay_safe_versioned_capped_and_durable() {
    let persistence = disposable_database().await;
    reset_fixture(&persistence).await;
    let service = PostgresAshWalletService::new(persistence.clone(), FixedClock);
    let account = authenticated(ACCOUNT_ID);

    let initial = service.view(account).await.unwrap();
    assert_eq!(
        (initial.balance, initial.wallet_version, initial.cap),
        (0, 1, 99_999)
    );

    let earn = command(
        1,
        1,
        AshReasonCode::DrownedReliquaryBoss,
        AshWalletMutationKind::Earn,
        40,
    );
    let first = service.mutate(account, &earn).await;
    assert_eq!(first.code, AshWalletResultCode::Accepted);
    assert_eq!(
        first
            .projection
            .map(|value| (value.balance, value.wallet_version)),
        Some((40, 2))
    );
    assert_eq!(service.mutate(account, &earn).await, first);

    let mut conflict = earn.clone();
    conflict.payload.source_id = "fixture.source.conflict".into();
    conflict.payload_hash = conflict.payload.canonical_hash();
    assert_eq!(
        service.mutate(account, &conflict).await.code,
        AshWalletResultCode::IdempotencyConflict
    );

    race_same_wallet_version(&service, account).await;

    let spend = command(
        4,
        3,
        AshReasonCode::OathChange,
        AshWalletMutationKind::Spend,
        40,
    );
    let spend_result = service.mutate(account, &spend).await;
    assert_eq!(spend_result.code, AshWalletResultCode::Accepted);
    assert_eq!(
        spend_result
            .projection
            .map(|value| (value.balance, value.wallet_version)),
        Some((10, 4))
    );
    assert_rejected_boundaries(&service, account).await;

    assert_durable_ledger(&persistence).await;
    persistence.close().await;

    let restarted = disposable_database().await;
    let restarted_service = PostgresAshWalletService::new(restarted.clone(), FixedClock);
    let durable = restarted_service.view(account).await.unwrap();
    assert_eq!((durable.balance, durable.wallet_version), (10, 4));
    assert_eq!(restarted_service.mutate(account, &earn).await, first);
    assert_durable_ledger(&restarted).await;
    restarted.close().await;
}
