use persistence::{
    DESTRUCTIVE_TEST_OPT_IN_ENV, PersistenceConfig, PersistenceError, PostgresPersistence,
    WIPEABLE_CORE_NAMESPACE,
};

const ACCOUNT_ID: [u8; 16] = [171; 16];
const CHARACTER_ID: [u8; 16] = [172; 16];
const FOREIGN_CHARACTER_ID: [u8; 16] = [173; 16];

async fn disposable_database() -> (PersistenceConfig, PostgresPersistence) {
    assert_eq!(
        std::env::var(DESTRUCTIVE_TEST_OPT_IN_ENV).as_deref(),
        Ok("1"),
        "private-route PostgreSQL evidence requires explicit destructive-test opt-in"
    );
    let config = PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL");
    let persistence = PostgresPersistence::connect(&config).await.unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence.migrate().await.unwrap();
    (config, persistence)
}

async fn prepare_selected_character(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query("DELETE FROM accounts WHERE namespace_id=$1 AND account_id=$2")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ACCOUNT_ID.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO accounts (namespace_id,account_id,state_version,slot_capacity) \
         VALUES ($1,$2,1,2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO characters \
         (namespace_id,account_id,character_id,roster_ordinal,class_id,level,oath_id,life_state,security_state) \
         VALUES ($1,$2,$3,1,'class.grave_arbalist',1,NULL,0,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE accounts SET selected_character_id=$3,updated_at=transaction_timestamp() \
         WHERE namespace_id=$1 AND account_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires guarded TEST_DATABASE_URL PostgreSQL"]
async fn actor_generations_survive_restart_serialize_concurrency_and_reject_mutation() {
    let (config, persistence) = disposable_database().await;
    prepare_selected_character(&persistence).await;

    assert!(matches!(
        persistence
            .allocate_private_route_generation_v1(ACCOUNT_ID, FOREIGN_CHARACTER_ID)
            .await,
        Err(PersistenceError::PrivateRouteCharacterUnavailable)
    ));

    let mut tasks = Vec::new();
    for _ in 0..8 {
        let contender = persistence.clone();
        tasks.push(tokio::spawn(async move {
            contender
                .allocate_private_route_generation_v1(ACCOUNT_ID, CHARACTER_ID)
                .await
                .expect("concurrent allocation")
                .actor_generation
        }));
    }
    let mut generations = Vec::new();
    for task in tasks {
        generations.push(task.await.expect("allocation task"));
    }
    generations.sort_unstable();
    assert_eq!(generations, (1_u64..=8).collect::<Vec<_>>());

    let restarted = PostgresPersistence::connect(&config).await.unwrap();
    restarted.readiness().await.unwrap();
    let ninth = restarted
        .allocate_private_route_generation_v1(ACCOUNT_ID, CHARACTER_ID)
        .await
        .expect("post-restart allocation");
    assert_eq!(ninth.actor_generation, 9);

    let mut inspection = restarted.begin_transaction().await.unwrap();
    let head: i64 = sqlx::query_scalar(
        "SELECT last_generation FROM character_private_route_generation_heads_v1 \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .fetch_one(inspection.connection())
    .await
    .unwrap();
    let allocations: Vec<i64> = sqlx::query_scalar(
        "SELECT actor_generation FROM private_route_generation_allocations_v1 \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 \
         ORDER BY actor_generation",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .fetch_all(inspection.connection())
    .await
    .unwrap();
    inspection.rollback().await.unwrap();
    assert_eq!(head, 9);
    assert_eq!(allocations, (1_i64..=9).collect::<Vec<_>>());

    let mut update = restarted.begin_transaction().await.unwrap();
    assert!(
        sqlx::query(
            "UPDATE private_route_generation_allocations_v1 SET actor_generation=10 \
             WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND actor_generation=9",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ACCOUNT_ID.as_slice())
        .bind(CHARACTER_ID.as_slice())
        .execute(update.connection())
        .await
        .is_err()
    );
    update.rollback().await.unwrap();

    let mut delete = restarted.begin_transaction().await.unwrap();
    assert!(
        sqlx::query(
            "DELETE FROM private_route_generation_allocations_v1 \
             WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND actor_generation=9",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ACCOUNT_ID.as_slice())
        .bind(CHARACTER_ID.as_slice())
        .execute(delete.connection())
        .await
        .is_err()
    );
    delete.rollback().await.unwrap();

    let mut cleanup = restarted.begin_transaction().await.unwrap();
    sqlx::query("DELETE FROM accounts WHERE namespace_id=$1 AND account_id=$2")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ACCOUNT_ID.as_slice())
        .execute(cleanup.connection())
        .await
        .unwrap();
    cleanup.commit().await.unwrap();
    persistence.close().await;
    restarted.close().await;
}
