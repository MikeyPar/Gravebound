//! Hosted `PostgreSQL` proof for the atomic GB-M03-07 successor writer.
//!
//! Authorities: canonical Production GDD `DTH-020`/`021` and `TECH-021`-`023`, Content
//! Production Spec `CONT-CATALOG-003`, Development Roadmap `GB-M03-07`, and accepted
//! `SPEC-CONFLICT-031`. The fixture commits a real ordinary death before successor recovery; it
//! never manufactures preset or reservation rows.

use persistence::{
    CORE_ITEM_CONTENT_REVISION, PersistenceConfig, PostgresPersistence,
    SUCCESSOR_CONTRACT_VERSION_V1, SuccessorCreateRequestV1, SuccessorCreateTransactionV1,
    WIPEABLE_CORE_NAMESPACE, derive_successor_character_id_v1, derive_successor_receipt_id_v1,
};
use protocol::{AccountErrorCode, CharacterMutationFrame, CharacterMutationPayload, ManifestHash};
use protocol::{SuccessorCreatePayloadV1, WireText};
use server_app::{
    CharacterIdGenerator, IdentityClock, IdentityService, NoopIdentityEventSink,
    PostgresAccountRepository, StarterItemPlan,
};
use sqlx::Row;

#[path = "support/durable_death.rs"]
mod durable_death_fixture;

const SUCCESSOR_MUTATION_ID: [u8; 16] = [71; 16];
const ALTERED_DEATH_ID: [u8; 16] = [72; 16];
const ORDINARY_CREATE_MUTATION_ID: [u8; 16] = [73; 16];
const ORDINARY_CREATE_CHARACTER_ID: [u8; 16] = [74; 16];

#[derive(Debug, Clone, Copy)]
struct FixedIdentityClock;

impl IdentityClock for FixedIdentityClock {
    fn unix_millis(&self) -> u64 {
        10_000
    }
}

#[derive(Debug, Clone, Copy)]
struct ReservedCreateId;

impl CharacterIdGenerator for ReservedCreateId {
    fn next_id(&self) -> [u8; 16] {
        ORDINARY_CREATE_CHARACTER_ID
    }
}

#[derive(Debug, Clone, Copy)]
enum SuccessorFailpoint {
    Character,
    Progression,
    World,
    Life,
    Oath,
    StarterItem,
    Result,
    Receipt,
    Audit,
    Outbox,
    Reservation,
}

impl SuccessorFailpoint {
    const ALL: [Self; 11] = [
        Self::Character,
        Self::Progression,
        Self::World,
        Self::Life,
        Self::Oath,
        Self::StarterItem,
        Self::Result,
        Self::Receipt,
        Self::Audit,
        Self::Outbox,
        Self::Reservation,
    ];

    const fn install_sql(self) -> &'static str {
        match self {
            Self::Character => {
                "CREATE TRIGGER successor_test_failpoint BEFORE INSERT ON characters \
                 FOR EACH ROW EXECUTE FUNCTION fail_successor_test_write_v1()"
            }
            Self::Progression => {
                "CREATE TRIGGER successor_test_failpoint BEFORE INSERT ON character_progression \
                 FOR EACH ROW EXECUTE FUNCTION fail_successor_test_write_v1()"
            }
            Self::World => {
                "CREATE TRIGGER successor_test_failpoint BEFORE INSERT ON character_world_locations \
                 FOR EACH ROW EXECUTE FUNCTION fail_successor_test_write_v1()"
            }
            Self::Life => {
                "CREATE TRIGGER successor_test_failpoint BEFORE INSERT ON character_life_metrics \
                 FOR EACH ROW EXECUTE FUNCTION fail_successor_test_write_v1()"
            }
            Self::Oath => {
                "CREATE TRIGGER successor_test_failpoint BEFORE INSERT ON character_oath_bargain_state \
                 FOR EACH ROW EXECUTE FUNCTION fail_successor_test_write_v1()"
            }
            Self::StarterItem => {
                "CREATE TRIGGER successor_test_failpoint BEFORE INSERT ON item_instances \
                 FOR EACH ROW EXECUTE FUNCTION fail_successor_test_write_v1()"
            }
            Self::Result => {
                "CREATE TRIGGER successor_test_failpoint BEFORE INSERT ON successor_mutation_results_v1 \
                 FOR EACH ROW EXECUTE FUNCTION fail_successor_test_write_v1()"
            }
            Self::Receipt => {
                "CREATE TRIGGER successor_test_failpoint BEFORE INSERT ON successor_creation_receipts_v1 \
                 FOR EACH ROW EXECUTE FUNCTION fail_successor_test_write_v1()"
            }
            Self::Audit => {
                "CREATE TRIGGER successor_test_failpoint BEFORE INSERT ON successor_mutation_audit_events_v1 \
                 FOR EACH ROW EXECUTE FUNCTION fail_successor_test_write_v1()"
            }
            Self::Outbox => {
                "CREATE TRIGGER successor_test_failpoint BEFORE INSERT ON successor_mutation_outbox_events_v1 \
                 FOR EACH ROW EXECUTE FUNCTION fail_successor_test_write_v1()"
            }
            Self::Reservation => {
                "CREATE TRIGGER successor_test_failpoint BEFORE UPDATE OF reservation_state \
                 ON successor_roster_reservations_v1 \
                 FOR EACH ROW EXECUTE FUNCTION fail_successor_test_write_v1()"
            }
        }
    }

    const fn drop_sql(self) -> &'static str {
        match self {
            Self::Character => "DROP TRIGGER successor_test_failpoint ON characters",
            Self::Progression => "DROP TRIGGER successor_test_failpoint ON character_progression",
            Self::World => "DROP TRIGGER successor_test_failpoint ON character_world_locations",
            Self::Life => "DROP TRIGGER successor_test_failpoint ON character_life_metrics",
            Self::Oath => "DROP TRIGGER successor_test_failpoint ON character_oath_bargain_state",
            Self::StarterItem => "DROP TRIGGER successor_test_failpoint ON item_instances",
            Self::Result => {
                "DROP TRIGGER successor_test_failpoint ON successor_mutation_results_v1"
            }
            Self::Receipt => {
                "DROP TRIGGER successor_test_failpoint ON successor_creation_receipts_v1"
            }
            Self::Audit => {
                "DROP TRIGGER successor_test_failpoint ON successor_mutation_audit_events_v1"
            }
            Self::Outbox => {
                "DROP TRIGGER successor_test_failpoint ON successor_mutation_outbox_events_v1"
            }
            Self::Reservation => {
                "DROP TRIGGER successor_test_failpoint ON successor_roster_reservations_v1"
            }
        }
    }
}

async fn disposable_database() -> PostgresPersistence {
    let config = PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL");
    let persistence = PostgresPersistence::connect(&config).await.unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence.migrate().await.unwrap();
    persistence.reset_disposable_identity_data().await.unwrap();
    persistence
}

async fn reconnect_database() -> PostgresPersistence {
    let config = PersistenceConfig::from_test_environment().unwrap();
    let persistence = PostgresPersistence::connect(&config).await.unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence
}

fn successor_request(
    account_id: [u8; 16],
    death_id: [u8; 16],
    mutation_id: [u8; 16],
) -> SuccessorCreateRequestV1 {
    let successor_id = derive_successor_character_id_v1(account_id, death_id, mutation_id);
    let receipt_id =
        derive_successor_receipt_id_v1(account_id, death_id, mutation_id, successor_id);
    let payload = SuccessorCreatePayloadV1 {
        death_id,
        content_revision: WireText::new(CORE_ITEM_CONTENT_REVISION).unwrap(),
    };
    let starter = StarterItemPlan::for_character(successor_id).unwrap();
    let request = SuccessorCreateRequestV1 {
        contract_version: SUCCESSOR_CONTRACT_VERSION_V1,
        namespace_id: WIPEABLE_CORE_NAMESPACE.to_owned(),
        account_id,
        mutation_id,
        death_id,
        successor_id,
        receipt_id,
        canonical_request_hash: payload.canonical_hash(),
        content_revision: CORE_ITEM_CONTENT_REVISION.to_owned(),
        starter_request_hash: starter.request_hash,
        starter_result_hash: starter.result_hash,
        starter_items: starter.items,
    };
    request.validate().unwrap();
    assert_eq!(
        request.expected_request_hash().unwrap(),
        payload.canonical_hash()
    );
    request
}

async fn commit_primary_death(persistence: &PostgresPersistence) {
    durable_death_fixture::seed_danger_root(persistence).await;
    let death = durable_death_fixture::prepare_death(persistence.clone()).await;
    let committed = persistence
        .transact_durable_death(death.request(), death.content(), death.promotion())
        .await
        .unwrap();
    assert!(!committed.is_replay());
}

async fn assert_reserved_ordinary_create_is_typed(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let account_version: i64 = sqlx::query_scalar(
        "SELECT state_version FROM accounts WHERE namespace_id=$1 AND account_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(durable_death_fixture::ACCOUNT_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    let service = IdentityService::new(
        PostgresAccountRepository::new(persistence.clone()),
        FixedIdentityClock,
        ReservedCreateId,
        NoopIdentityEventSink,
        ManifestHash::new("a".repeat(64)).unwrap(),
    );
    let payload = CharacterMutationPayload::Create {
        class_id: WireText::new(protocol::GRAVE_ARBALIST_CLASS_ID).unwrap(),
    };
    let frame = CharacterMutationFrame {
        mutation_id: ORDINARY_CREATE_MUTATION_ID,
        expected_account_version: u64::try_from(account_version).unwrap(),
        payload_hash: payload.canonical_hash(),
        issued_at_unix_millis: 9_000,
        payload,
    };
    let result = service
        .mutate(Some(durable_death_fixture::authenticated_account()), &frame)
        .await;
    assert!(!result.accepted);
    assert_eq!(
        result.error,
        Some(AccountErrorCode::SuccessorResolutionRequired)
    );
    assert!(result.snapshot.is_none());

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let residue: (i64, i64) = sqlx::query_as(
        "SELECT \
           (SELECT count(*) FROM characters WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3), \
           (SELECT count(*) FROM account_mutation_results WHERE namespace_id=$1 AND account_id=$2 AND mutation_id=$4)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(durable_death_fixture::ACCOUNT_ID.as_slice())
    .bind(ORDINARY_CREATE_CHARACTER_ID.as_slice())
    .bind(ORDINARY_CREATE_MUTATION_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(residue, (0, 0));
    transaction.rollback().await.unwrap();
}

async fn install_failpoint(persistence: &PostgresPersistence, failpoint: SuccessorFailpoint) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "CREATE OR REPLACE FUNCTION fail_successor_test_write_v1() \
         RETURNS TRIGGER LANGUAGE plpgsql AS $$ \
         BEGIN RAISE EXCEPTION 'injected successor rollback'; END $$",
    )
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(failpoint.install_sql())
        .execute(transaction.connection())
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn remove_failpoint(persistence: &PostgresPersistence, failpoint: SuccessorFailpoint) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(failpoint.drop_sql())
        .execute(transaction.connection())
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn remove_failpoint_function(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query("DROP FUNCTION fail_successor_test_write_v1()")
        .execute(transaction.connection())
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn assert_successor_rollback_pristine(
    persistence: &PostgresPersistence,
    request: &SuccessorCreateRequestV1,
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let connection = transaction.connection();
    let account: (i64, Option<Vec<u8>>) = sqlx::query_as(
        "SELECT state_version,selected_character_id FROM accounts \
         WHERE namespace_id=$1 AND account_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert!(account.0 > 0);
    assert!(account.1.is_none());
    let counts: (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT \
          (SELECT count(*) FROM characters WHERE namespace_id=$1 AND character_id=$3), \
          (SELECT count(*) FROM character_progression WHERE namespace_id=$1 AND character_id=$3), \
          (SELECT count(*) FROM character_world_locations WHERE namespace_id=$1 AND character_id=$3), \
          (SELECT count(*) FROM character_life_metrics WHERE namespace_id=$1 AND character_id=$3), \
          (SELECT count(*) FROM character_oath_bargain_state WHERE namespace_id=$1 AND character_id=$3), \
          (SELECT count(*) FROM item_instances WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3), \
          (SELECT count(*) FROM starter_initializer_results WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3), \
          (SELECT count(*) FROM successor_mutation_results_v1 WHERE namespace_id=$1 AND account_id=$2), \
          (SELECT count(*) FROM successor_creation_receipts_v1 WHERE namespace_id=$1 AND account_id=$2), \
          (SELECT count(*) FROM successor_mutation_audit_events_v1 WHERE namespace_id=$1 AND account_id=$2), \
          (SELECT count(*) FROM successor_mutation_conflict_audits_v1 WHERE namespace_id=$1 AND account_id=$2), \
          (SELECT count(*) FROM successor_mutation_outbox_events_v1 WHERE namespace_id=$1 AND account_id=$2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.successor_id.as_slice())
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(counts, (0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0));
    let reservation_state: i16 = sqlx::query_scalar(
        "SELECT reservation_state FROM successor_roster_reservations_v1 \
         WHERE namespace_id=$1 AND account_id=$2 AND death_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.death_id.as_slice())
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(reservation_state, 0);
    transaction.rollback().await.unwrap();
}

#[allow(
    clippy::too_many_lines,
    reason = "the normalized successor graph remains contiguous for hosted authority review"
)]
async fn assert_complete_successor_graph(
    persistence: &PostgresPersistence,
    expected: &persistence::StoredSuccessorResultV1,
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let connection = transaction.connection();
    let graph_counts: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT \
           (SELECT count(*) FROM successor_mutation_results_v1 \
            WHERE namespace_id=$1 AND account_id=$2), \
           (SELECT count(*) FROM successor_creation_receipts_v1 \
            WHERE namespace_id=$1 AND account_id=$2), \
           (SELECT count(*) FROM successor_mutation_audit_events_v1 \
            WHERE namespace_id=$1 AND account_id=$2), \
           (SELECT count(*) FROM successor_mutation_outbox_events_v1 \
            WHERE namespace_id=$1 AND account_id=$2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(expected.account_id.as_slice())
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(graph_counts, (1, 1, 1, 1));
    let reservation: (i16, Vec<u8>, Vec<u8>, Vec<u8>) = sqlx::query_as(
        "SELECT reservation_state,consumed_mutation_id,consumed_successor_id,consumed_receipt_id \
         FROM successor_roster_reservations_v1 \
         WHERE namespace_id=$1 AND account_id=$2 AND death_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(expected.account_id.as_slice())
    .bind(expected.death_id.as_slice())
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(reservation.0, 1);
    assert_eq!(reservation.1, expected.mutation_id);
    assert_eq!(reservation.2, expected.successor_id);
    assert_eq!(reservation.3, expected.receipt_id);

    let account: (i64, Vec<u8>) = sqlx::query_as(
        "SELECT state_version,selected_character_id FROM accounts \
         WHERE namespace_id=$1 AND account_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(expected.account_id.as_slice())
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(u64::try_from(account.0).unwrap(), expected.versions.account);
    assert_eq!(account.1, expected.successor_id);

    let aggregate: (i16, String, i32, i64, i16, i16, i64, i64, i64) = sqlx::query_as(
        "SELECT character.roster_ordinal,character.class_id,character.level, \
                character.character_state_version,progression.level,progression.current_health, \
                progression.progression_version,world.character_version,inventory.inventory_version \
         FROM characters AS character \
         JOIN character_progression AS progression USING (namespace_id,account_id,character_id) \
         JOIN character_world_locations AS world USING (namespace_id,account_id,character_id) \
         JOIN character_inventories AS inventory USING (namespace_id,account_id,character_id) \
         WHERE character.namespace_id=$1 AND character.account_id=$2 \
           AND character.character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(expected.account_id.as_slice())
    .bind(expected.successor_id.as_slice())
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(
        aggregate,
        (1, "class.grave_arbalist".into(), 1, 1, 1, 120, 1, 1, 2)
    );

    let item_rows = sqlx::query(
        "SELECT item_uid,template_id,location_kind,slot_index,item_version,provenance_kind \
         FROM item_instances WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 \
         ORDER BY roll_index,unit_ordinal,item_uid",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(expected.account_id.as_slice())
    .bind(expected.successor_id.as_slice())
    .fetch_all(&mut *connection)
    .await
    .unwrap();
    assert_eq!(item_rows.len(), 4);
    let expected_uids = expected.starter_items.ordered_uids();
    for (index, row) in item_rows.iter().enumerate() {
        assert_eq!(
            row.try_get::<Vec<u8>, _>("item_uid").unwrap(),
            expected_uids[index]
        );
        assert_eq!(row.try_get::<i64, _>("item_version").unwrap(), 1);
    }
    assert_eq!(
        item_rows[0].try_get::<String, _>("template_id").unwrap(),
        "item.weapon.crossbow.pine_crossbow"
    );
    assert_eq!(
        (
            item_rows[0].try_get::<i16, _>("location_kind").unwrap(),
            item_rows[0].try_get::<i16, _>("slot_index").unwrap(),
            item_rows[0].try_get::<i16, _>("provenance_kind").unwrap()
        ),
        (0, 0, 0)
    );
    assert_eq!(
        item_rows[1].try_get::<String, _>("template_id").unwrap(),
        "item.relic.arbalist.cracked_mark_lens"
    );
    assert_eq!(
        (
            item_rows[1].try_get::<i16, _>("location_kind").unwrap(),
            item_rows[1].try_get::<i16, _>("slot_index").unwrap(),
            item_rows[1].try_get::<i16, _>("provenance_kind").unwrap()
        ),
        (0, 1, 0)
    );
    for row in &item_rows[2..] {
        assert_eq!(
            row.try_get::<String, _>("template_id").unwrap(),
            "consumable.red_tonic"
        );
        assert_eq!(
            (
                row.try_get::<i16, _>("location_kind").unwrap(),
                row.try_get::<i16, _>("slot_index").unwrap(),
                row.try_get::<i16, _>("provenance_kind").unwrap()
            ),
            (1, 0, 4)
        );
    }
    let active_bargains: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM character_active_bargains \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(expected.account_id.as_slice())
    .bind(expected.successor_id.as_slice())
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(active_bargains, 0);
    transaction.rollback().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn successor_creation_is_atomic_concurrent_replayable_and_restart_safe() {
    let persistence = disposable_database().await;
    commit_primary_death(&persistence).await;
    assert_reserved_ordinary_create_is_typed(&persistence).await;
    let request = successor_request(
        durable_death_fixture::ACCOUNT_ID,
        durable_death_fixture::PRIMARY_IDENTITY.death_id,
        SUCCESSOR_MUTATION_ID,
    );

    for failpoint in SuccessorFailpoint::ALL {
        install_failpoint(&persistence, failpoint).await;
        assert!(matches!(
            persistence.create_successor_v1(&request).await,
            Err(persistence::PersistenceError::Database(_))
        ));
        remove_failpoint(&persistence, failpoint).await;
        assert_successor_rollback_pristine(&persistence, &request).await;
    }
    remove_failpoint_function(&persistence).await;

    let first_persistence = persistence.clone();
    let second_persistence = persistence.clone();
    let first_request = request.clone();
    let second_request = request.clone();
    let (first, second) = tokio::join!(
        async move { first_persistence.create_successor_v1(&first_request).await },
        async move {
            second_persistence
                .create_successor_v1(&second_request)
                .await
        },
    );
    let first = first.unwrap();
    let second = second.unwrap();
    let (fresh, replay) = match (first, second) {
        (
            SuccessorCreateTransactionV1::Fresh(fresh),
            SuccessorCreateTransactionV1::Replayed(replay),
        )
        | (
            SuccessorCreateTransactionV1::Replayed(replay),
            SuccessorCreateTransactionV1::Fresh(fresh),
        ) => (fresh, replay),
        other => panic!("expected one fresh successor and one replay, got {other:?}"),
    };
    assert_eq!(fresh, replay);
    assert_complete_successor_graph(&persistence, &fresh).await;

    persistence.close().await;
    let persistence = reconnect_database().await;
    let restart_replay = persistence.create_successor_v1(&request).await.unwrap();
    assert!(
        matches!(restart_replay, SuccessorCreateTransactionV1::Replayed(ref stored) if stored == &fresh)
    );

    let altered = successor_request(
        durable_death_fixture::ACCOUNT_ID,
        ALTERED_DEATH_ID,
        SUCCESSOR_MUTATION_ID,
    );
    assert!(matches!(
        persistence.create_successor_v1(&altered).await.unwrap(),
        SuccessorCreateTransactionV1::Conflict {
            stored_mutation_id: SUCCESSOR_MUTATION_ID,
            stored_death_id,
        } if stored_death_id == fresh.death_id
    ));
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let conflict_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM successor_mutation_conflict_audits_v1 \
         WHERE namespace_id=$1 AND account_id=$2 AND mutation_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(durable_death_fixture::ACCOUNT_ID.as_slice())
    .bind(SUCCESSOR_MUTATION_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(conflict_count, 1);
    transaction.rollback().await.unwrap();
    assert_complete_successor_graph(&persistence, &fresh).await;
    persistence.close().await;
}
