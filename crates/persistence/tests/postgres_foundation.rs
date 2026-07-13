use persistence::{
    EXPECTED_SCHEMA_VERSION, PersistenceConfig, PersistenceTransaction, PostgresPersistence,
    StoredCharacter, StoredMutation, WIPEABLE_CORE_NAMESPACE,
};
const ACCOUNT_A: [u8; 16] = [1; 16];
const ACCOUNT_B: [u8; 16] = [2; 16];
const ACCOUNT_ROLLBACK: [u8; 16] = [3; 16];
const CHARACTER_A: [u8; 16] = [11; 16];
const CHARACTER_B: [u8; 16] = [12; 16];
const FOREIGN_CHARACTER: [u8; 16] = [21; 16];

async fn disposable_database() -> PostgresPersistence {
    let config = PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL");
    let persistence = PostgresPersistence::connect(&config).await.unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence.migrate().await.unwrap();
    persistence
}

async fn clear_accounts(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query("DELETE FROM accounts WHERE namespace_id = $1")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .execute(transaction.connection())
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn insert_account(
    transaction: &mut PersistenceTransaction<'_>,
    account_id: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO accounts \
         (namespace_id, account_id, state_version, slot_capacity) VALUES ($1, $2, 1, 2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id)
    .execute(transaction.connection())
    .await?;
    Ok(())
}

async fn insert_character(
    transaction: &mut PersistenceTransaction<'_>,
    account_id: &[u8],
    character_id: &[u8],
    ordinal: i16,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO characters \
         (namespace_id, account_id, character_id, roster_ordinal, class_id, level, \
          oath_id, life_state, security_state) \
         VALUES ($1, $2, $3, $4, 'class.grave_arbalist', 1, NULL, 0, 0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id)
    .bind(character_id)
    .bind(ordinal)
    .execute(transaction.connection())
    .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn migrations_are_idempotent_exact_and_ready() {
    let persistence = disposable_database().await;

    persistence.migrate().await.unwrap();
    let readiness = persistence.readiness().await.unwrap();
    assert_eq!(readiness.schema_version, EXPECTED_SCHEMA_VERSION);
    assert_eq!(readiness.namespace, WIPEABLE_CORE_NAMESPACE);
    assert!(readiness.wipeable);

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = 'public' ORDER BY table_name",
    )
    .fetch_all(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(
        tables,
        [
            "_sqlx_migrations",
            "account_boss_first_clears",
            "account_mutation_results",
            "accounts",
            "character_danger_checkpoints",
            "character_entry_restore_points",
            "character_instance_lineages",
            "character_inventories",
            "character_life_outbox",
            "character_oath_mutation_results",
            "character_progression",
            "character_world_locations",
            "character_world_transfer_results",
            "character_xp_award_results",
            "characters",
            "entry_restore_progression_v1",
            "gravebound_namespaces",
            "item_instances",
            "item_ledger_events",
            "reward_requests",
            "reward_result_entries",
            "starter_initializer_results",
        ]
    );
    for prohibited in ["vault", "memorials", "currency_ledger"] {
        assert!(!tables.iter().any(|table| table == prohibited));
    }
    persistence.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn identity_schema_enforces_transactions_ownership_and_bounds() {
    let persistence = disposable_database().await;
    clear_accounts(&persistence).await;
    insert_valid_fixture(&persistence).await;
    assert_bound_rejections(&persistence).await;
    assert_selected_character_ownership(&persistence).await;
    assert_rollback_and_cascade(&persistence).await;
    persistence.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn typed_identity_store_round_trips_under_one_lock() {
    let persistence = disposable_database().await;
    clear_accounts(&persistence).await;
    let written = persistence
        .transact_identity(ACCOUNT_A, 1, 2, |aggregate| {
            assert_eq!((aggregate.state_version, aggregate.slot_capacity), (1, 2));
            aggregate.state_version = 2;
            aggregate.characters.push(StoredCharacter {
                character_id: CHARACTER_A,
                roster_ordinal: 1,
                class_id: "class.grave_arbalist".to_owned(),
                level: 1,
                oath_id: None,
                life_state: 0,
                security_state: 0,
                character_state_version: 1,
            });
            aggregate.mutations.push(StoredMutation {
                mutation_id: [31; 16],
                payload_hash: [41; 32],
                result_payload: vec![51; 32],
            });
            aggregate.selected_character_id = Some(CHARACTER_A);
            Ok(aggregate.clone())
        })
        .await
        .unwrap();
    let loaded = persistence
        .transact_identity(ACCOUNT_A, 1, 2, |aggregate| Ok(aggregate.clone()))
        .await
        .unwrap();
    assert_eq!(loaded, written);
    assert_eq!(
        persistence
            .identity_character_owner(CHARACTER_A)
            .await
            .unwrap(),
        Some(ACCOUNT_A)
    );
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let initial_location: (i64, i16) = sqlx::query_as(
        "SELECT character_version, location_kind FROM character_world_locations \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .bind(CHARACTER_A.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(initial_location, (1, 0));
    persistence.close().await;
}

async fn insert_valid_fixture(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    insert_account(&mut transaction, &ACCOUNT_A).await.unwrap();
    insert_account(&mut transaction, &ACCOUNT_B).await.unwrap();
    insert_character(&mut transaction, &ACCOUNT_A, &CHARACTER_A, 1)
        .await
        .unwrap();
    insert_character(&mut transaction, &ACCOUNT_A, &CHARACTER_B, 2)
        .await
        .unwrap();
    insert_character(&mut transaction, &ACCOUNT_B, &FOREIGN_CHARACTER, 1)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO account_mutation_results \
         (namespace_id, account_id, mutation_id, payload_hash, result_payload) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .bind([31_u8; 16].as_slice())
    .bind([41_u8; 32].as_slice())
    .bind([51_u8; 32].as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE accounts SET selected_character_id = $1 \
         WHERE namespace_id = $2 AND account_id = $3",
    )
    .bind(CHARACTER_A.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn assert_bound_rejections(persistence: &PostgresPersistence) {
    let mut invalid_account = persistence.begin_transaction().await.unwrap();
    assert!(
        insert_account(&mut invalid_account, &[1_u8; 15])
            .await
            .is_err()
    );
    invalid_account.rollback().await.unwrap();

    let mut duplicate_slot = persistence.begin_transaction().await.unwrap();
    assert!(
        insert_character(&mut duplicate_slot, &ACCOUNT_A, &[13; 16], 2)
            .await
            .is_err()
    );
    duplicate_slot.rollback().await.unwrap();

    let mut invalid_mutation = persistence.begin_transaction().await.unwrap();
    let result = sqlx::query(
        "INSERT INTO account_mutation_results \
         (namespace_id, account_id, mutation_id, payload_hash, result_payload) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .bind([61_u8; 16].as_slice())
    .bind([71_u8; 31].as_slice())
    .bind(Vec::<u8>::new())
    .execute(invalid_mutation.connection())
    .await;
    assert!(result.is_err());
    invalid_mutation.rollback().await.unwrap();
}

async fn assert_selected_character_ownership(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "UPDATE accounts SET selected_character_id = $1 \
         WHERE namespace_id = $2 AND account_id = $3",
    )
    .bind(FOREIGN_CHARACTER.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    assert!(transaction.commit().await.is_err());
}

async fn assert_rollback_and_cascade(persistence: &PostgresPersistence) {
    let mut rolled_back = persistence.begin_transaction().await.unwrap();
    insert_account(&mut rolled_back, &ACCOUNT_ROLLBACK)
        .await
        .unwrap();
    rolled_back.rollback().await.unwrap();

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM accounts WHERE namespace_id = $1 AND account_id = $2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ROLLBACK.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(count, 0);
    sqlx::query("DELETE FROM accounts WHERE namespace_id = $1 AND account_id = $2")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ACCOUNT_A.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
    transaction.commit().await.unwrap();

    let mut verification = persistence.begin_transaction().await.unwrap();
    let characters: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM characters WHERE namespace_id = $1 AND account_id = $2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    let mutations: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM account_mutation_results \
         WHERE namespace_id = $1 AND account_id = $2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    verification.rollback().await.unwrap();
    assert_eq!((characters, mutations), (0, 0));
}
