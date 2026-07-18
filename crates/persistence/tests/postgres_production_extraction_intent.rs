//! Hosted `PostgreSQL` acceptance for durable live extraction-intent replay authority.
//!
//! Authorities:
//! - `Gravebound_Production_GDD_v1_Canonical.md` DTH-011, LOOT-002/060, and
//!   TECH-015/021-023;
//! - `Gravebound_Content_Production_Spec_v1.md` CONT-HUB-001/002, the exact Core Bell
//!   Sepulcher/Sir Caldus route, and CONT-VALID-001;
//! - `Gravebound_Development_Roadmap_v1.md` GB-M03-03/08 and the restart/idempotency gates;
//! - accepted `SPEC-CONFLICT-029-m03-extraction-recall-terminal-authority.md`.

use persistence::{
    DESTRUCTIVE_TEST_OPT_IN_ENV, PRODUCTION_EXTRACTION_CONTRACT_VERSION_V1,
    PRODUCTION_EXTRACTION_INTENT_CONTRACT_VERSION_V1,
    PRODUCTION_EXTRACTION_INTENT_FRAME_SCHEMA_VERSION_V1, PersistenceConfig, PersistenceError,
    PostgresPersistence, ProductionExtractionCommitRequestV1,
    ProductionExtractionCoreRouteRevisionV1, ProductionExtractionExpectedVersionsV1,
    ProductionExtractionIntentAcceptanceTransactionV1, ProductionExtractionIntentAttemptV1,
    StoredWorldFlowRevisionV1, WIPEABLE_CORE_NAMESPACE,
    canonical_production_extraction_frame_payload_hash_v1,
};

const ACCOUNT_ID: [u8; 16] = [181; 16];
const CHARACTER_ID: [u8; 16] = [182; 16];
const MUTATION_ID: [u8; 16] = [183; 16];
const TERMINAL_ID: [u8; 16] = [184; 16];
const EXTRACTION_REQUEST_ID: [u8; 16] = [185; 16];
const EXTRACTION_RECEIPT_ID: [u8; 16] = [186; 16];
const ENCOUNTER_ID: [u8; 16] = [187; 16];
const LINEAGE_ID: [u8; 16] = [188; 16];
const RESTORE_POINT_ID: [u8; 16] = [189; 16];
const EXIT_INSTANCE_ID: [u8; 16] = [190; 16];

async fn disposable_database() -> (PersistenceConfig, PostgresPersistence) {
    assert_eq!(
        std::env::var(DESTRUCTIVE_TEST_OPT_IN_ENV).as_deref(),
        Ok("1"),
        "extraction-intent PostgreSQL evidence requires explicit destructive-test opt-in"
    );
    let config = PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL");
    let persistence = PostgresPersistence::connect(&config).await.unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence.migrate().await.unwrap();
    (config, persistence)
}

async fn reconnect_database(config: &PersistenceConfig) -> PostgresPersistence {
    let persistence = PostgresPersistence::connect(config).await.unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence.readiness().await.unwrap();
    persistence
}

async fn prepare_selected_character(persistence: &PostgresPersistence) -> u64 {
    persistence.reset_disposable_identity_data().await.unwrap();
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO accounts (namespace_id,account_id,state_version,slot_capacity)
         VALUES ($1,$2,1,2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO characters
         (namespace_id,account_id,character_id,roster_ordinal,class_id,level,
          oath_id,life_state,security_state,character_state_version)
         VALUES ($1,$2,$3,1,'class.grave_arbalist',10,NULL,0,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE accounts SET selected_character_id=$3
         WHERE namespace_id=$1 AND account_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    persistence
        .allocate_private_route_generation_v1(ACCOUNT_ID, CHARACTER_ID)
        .await
        .unwrap()
        .actor_generation
}

fn world_flow_revision() -> StoredWorldFlowRevisionV1 {
    StoredWorldFlowRevisionV1 {
        records_blake3: "a".repeat(64),
        assets_blake3: "b".repeat(64),
        localization_blake3: "c".repeat(64),
    }
}

fn commit_request() -> ProductionExtractionCommitRequestV1 {
    ProductionExtractionCommitRequestV1 {
        contract_version: PRODUCTION_EXTRACTION_CONTRACT_VERSION_V1,
        namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
        account_id: ACCOUNT_ID,
        character_id: CHARACTER_ID,
        mutation_id: MUTATION_ID,
        terminal_id: TERMINAL_ID,
        extraction_request_id: EXTRACTION_REQUEST_ID,
        extraction_receipt_id: EXTRACTION_RECEIPT_ID,
        encounter_id: ENCOUNTER_ID,
        instance_lineage_id: LINEAGE_ID,
        entry_restore_point_id: RESTORE_POINT_ID,
        exit_instance_id: EXIT_INSTANCE_ID,
        expected_versions: ProductionExtractionExpectedVersionsV1 {
            account: 1,
            character: 2,
            world: 2,
            inventory: 3,
            life_metrics: 4,
        },
        content_revision: world_flow_revision(),
        issued_at_unix_ms: 1,
        observed_tick: 30_000,
    }
}

fn attempt(actor_generation: u64) -> ProductionExtractionIntentAttemptV1 {
    let request = commit_request();
    ProductionExtractionIntentAttemptV1 {
        contract_version: PRODUCTION_EXTRACTION_INTENT_CONTRACT_VERSION_V1,
        namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
        authenticated_account_id: request.account_id,
        attempted_character_id: request.character_id,
        attempted_mutation_id: request.mutation_id,
        attempted_frame_schema_version: PRODUCTION_EXTRACTION_INTENT_FRAME_SCHEMA_VERSION_V1,
        attempted_frame_payload_hash: canonical_production_extraction_frame_payload_hash_v1(
            &request,
        )
        .unwrap(),
        extraction_request_id: request.extraction_request_id,
        extraction_receipt_id: request.extraction_receipt_id,
        terminal_id: request.terminal_id,
        actor_generation,
        accepted_pre_route_state_version: 40,
        accepted_post_route_state_version: 41,
        core_route_revision: ProductionExtractionCoreRouteRevisionV1 {
            records_blake3: "d".repeat(64),
            assets_blake3: "e".repeat(64),
            localization_blake3: "f".repeat(64),
        },
        world_flow_revision: request.content_revision.clone(),
        issued_at_unix_ms: request.issued_at_unix_ms,
        observed_tick: request.observed_tick,
        commit_request: request,
    }
}

fn refresh_frame_payload(attempt: &mut ProductionExtractionIntentAttemptV1) {
    attempt.attempted_frame_payload_hash =
        canonical_production_extraction_frame_payload_hash_v1(&attempt.commit_request).unwrap();
}

async fn expect_conflict(
    persistence: &PostgresPersistence,
    attempt: &ProductionExtractionIntentAttemptV1,
) -> [u8; 16] {
    attempt.validate().unwrap();
    match persistence
        .accept_production_extraction_intent_v1(attempt)
        .await
        .unwrap()
    {
        ProductionExtractionIntentAcceptanceTransactionV1::Conflict {
            extraction_request_id,
            conflict_audit_id,
            stored_attempt_hash,
            attempted_attempt_hash,
        } => {
            assert_eq!(extraction_request_id, EXTRACTION_REQUEST_ID);
            assert_ne!(conflict_audit_id, [0; 16]);
            assert_ne!(stored_attempt_hash, attempted_attempt_hash);
            conflict_audit_id
        }
        other => panic!("expected durable conflict, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires guarded TEST_DATABASE_URL PostgreSQL"]
#[allow(
    clippy::too_many_lines,
    reason = "one hosted journey must preserve acceptance, every altered replay axis, restart, and corruption ordering"
)]
async fn extraction_intent_replay_conflicts_restart_and_corruption_fail_closed() {
    let (config, persistence) = disposable_database().await;
    let actor_generation = prepare_selected_character(&persistence).await;
    assert_eq!(actor_generation, 1);
    let baseline = attempt(actor_generation);

    let fresh = persistence
        .accept_production_extraction_intent_v1(&baseline)
        .await
        .unwrap();
    let accepted = match fresh {
        ProductionExtractionIntentAcceptanceTransactionV1::Fresh(accepted) => accepted,
        other => panic!("expected fresh acceptance, got {other:?}"),
    };
    assert_eq!(accepted.attempt, baseline);
    assert_eq!(
        accepted.canonical_attempt_hash,
        baseline.canonical_hash().unwrap()
    );
    assert_eq!(
        persistence
            .load_production_extraction_intent_acceptance_v1(EXTRACTION_REQUEST_ID)
            .await
            .unwrap(),
        Some(accepted.clone())
    );

    let replay = persistence
        .accept_production_extraction_intent_v1(&baseline)
        .await
        .unwrap();
    assert!(replay.is_replay());
    assert_eq!(replay.acceptance(), Some(&accepted));

    persistence.close().await;
    let restarted = reconnect_database(&config).await;
    let replay = restarted
        .accept_production_extraction_intent_v1(&baseline)
        .await
        .unwrap();
    assert!(replay.is_replay());
    assert_eq!(replay.acceptance(), Some(&accepted));

    let mut changed_mutation = baseline.clone();
    changed_mutation.attempted_mutation_id = [201; 16];
    changed_mutation.commit_request.mutation_id = [201; 16];
    let mutation_conflict = expect_conflict(&restarted, &changed_mutation).await;
    assert_eq!(
        expect_conflict(&restarted, &changed_mutation).await,
        mutation_conflict,
        "the same changed material must reuse one durable audit identity"
    );

    let mut changed_aggregate_versions = baseline.clone();
    changed_aggregate_versions
        .commit_request
        .expected_versions
        .inventory += 1;
    refresh_frame_payload(&mut changed_aggregate_versions);
    expect_conflict(&restarted, &changed_aggregate_versions).await;

    let mut changed_route_versions = baseline.clone();
    changed_route_versions.accepted_pre_route_state_version = 41;
    changed_route_versions.accepted_post_route_state_version = 42;
    expect_conflict(&restarted, &changed_route_versions).await;

    let mut changed_broad_content = baseline.clone();
    changed_broad_content.core_route_revision.records_blake3 = "1".repeat(64);
    expect_conflict(&restarted, &changed_broad_content).await;

    let mut changed_narrow_content = baseline.clone();
    changed_narrow_content.world_flow_revision.records_blake3 = "2".repeat(64);
    changed_narrow_content
        .commit_request
        .content_revision
        .records_blake3 = "2".repeat(64);
    refresh_frame_payload(&mut changed_narrow_content);
    expect_conflict(&restarted, &changed_narrow_content).await;

    let mut changed_character = baseline.clone();
    changed_character.attempted_character_id = [202; 16];
    changed_character.commit_request.character_id = [202; 16];
    expect_conflict(&restarted, &changed_character).await;

    let mut changed_generation = baseline.clone();
    changed_generation.actor_generation += 1;
    expect_conflict(&restarted, &changed_generation).await;

    let mut inspection = restarted.begin_transaction().await.unwrap();
    let conflict_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM production_extraction_intent_conflict_audits_v1
         WHERE namespace_id=$1 AND extraction_request_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(EXTRACTION_REQUEST_ID.as_slice())
    .fetch_one(inspection.connection())
    .await
    .unwrap();
    let distinct_conflict_hashes: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT attempted_attempt_hash)
         FROM production_extraction_intent_conflict_audits_v1
         WHERE namespace_id=$1 AND extraction_request_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(EXTRACTION_REQUEST_ID.as_slice())
    .fetch_one(inspection.connection())
    .await
    .unwrap();
    inspection.rollback().await.unwrap();
    assert_eq!(conflict_count, 7);
    assert_eq!(distinct_conflict_hashes, conflict_count);

    restarted.close().await;
    let restarted = reconnect_database(&config).await;
    assert!(
        restarted
            .accept_production_extraction_intent_v1(&baseline)
            .await
            .unwrap()
            .is_replay()
    );

    let mut corruption = restarted.begin_transaction().await.unwrap();
    sqlx::query(
        "ALTER TABLE production_extraction_intent_acceptances_v1
         DISABLE TRIGGER production_extraction_intent_acceptance_immutable_v1",
    )
    .execute(corruption.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE production_extraction_intent_acceptances_v1
         SET attempt_payload=set_byte(attempt_payload,0,2)
         WHERE namespace_id=$1 AND extraction_request_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(EXTRACTION_REQUEST_ID.as_slice())
    .execute(corruption.connection())
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE production_extraction_intent_acceptances_v1
         ENABLE TRIGGER production_extraction_intent_acceptance_immutable_v1",
    )
    .execute(corruption.connection())
    .await
    .unwrap();
    corruption.commit().await.unwrap();

    assert!(matches!(
        restarted
            .load_production_extraction_intent_acceptance_v1(EXTRACTION_REQUEST_ID)
            .await,
        Err(PersistenceError::CorruptStoredProductionExtractionIntent)
    ));
    assert!(matches!(
        restarted
            .accept_production_extraction_intent_v1(&baseline)
            .await,
        Err(PersistenceError::CorruptStoredProductionExtractionIntent)
    ));

    let mut cleanup = restarted.begin_transaction().await.unwrap();
    sqlx::query("DELETE FROM accounts WHERE namespace_id=$1 AND account_id=$2")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ACCOUNT_ID.as_slice())
        .execute(cleanup.connection())
        .await
        .unwrap();
    cleanup.commit().await.unwrap();
    restarted.close().await;
}
