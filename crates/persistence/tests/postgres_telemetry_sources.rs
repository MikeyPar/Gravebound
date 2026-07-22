use persistence::{
    CORE_ITEM_CONTENT_REVISION, CORE_WORLD_ASSETS_BLAKE3, CORE_WORLD_LOCALIZATION_BLAKE3,
    CORE_WORLD_RECORDS_BLAKE3, M03CrashObservationCommandV1, M03SessionObservationCommandV1,
    M03SessionObservationV1, M03TelemetryOutboxError, M03TelemetrySessionStartV1,
    M03TelemetrySourceError, PersistenceConfig, PersistenceError,
    PostgresM03TelemetryDomainAdapter, PostgresPersistence, RewardTransaction,
    StoredM03CrashKindV1, StoredM03CrashReporterV1, StoredM03CrashSourceV1, StoredM03LootActionV1,
    StoredM03OnboardingEventV1, StoredM03SessionEndReasonV1, StoredM03SessionEventV1,
    StoredM03TelemetryEnvironmentV1, StoredM03TelemetryEventV1, StoredM03TelemetryPlatformV1,
    StoredRewardCommit, StoredRewardEntry, StoredRewardItem, StoredRewardRequest,
    TelemetryPseudonymizationKeyV1, WIPEABLE_CORE_NAMESPACE,
};
use telemetry::{
    CommittedTelemetrySource, TelemetryConnectivity, TelemetryIngestOutcome, TelemetryPipeline,
    TelemetryPipelineMode,
};

const ACCOUNT: [u8; 16] = [11; 16];
const CHARACTER: [u8; 16] = [12; 16];
const SESSION: [u8; 16] = [13; 16];
const LINEAGE: [u8; 16] = [14; 16];
const RESTORE_POINT: [u8; 16] = [15; 16];
const STARTED_AT: u64 = 1_750_000_000_000;

async fn disposable_database() -> PostgresPersistence {
    let config = PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL");
    let persistence = PostgresPersistence::connect(&config).await.unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence.migrate().await.unwrap();
    persistence
}

fn start_command() -> M03TelemetrySessionStartV1 {
    M03TelemetrySessionStartV1 {
        session_id: SESSION,
        account_id: ACCOUNT,
        build_id: "m03-core-dev-telemetry-1".into(),
        content_bundle_version: "core-dev".into(),
        platform: StoredM03TelemetryPlatformV1::Windows,
        region_id: "local".into(),
        environment: StoredM03TelemetryEnvironmentV1::Test,
        cohort_tags: vec!["cohort.private".into(), "staff".into()],
        started_at_utc_millis: STARTED_AT,
    }
}

#[tokio::test]
#[ignore = "requires explicitly opted-in disposable PostgreSQL"]
async fn telemetry_sources_are_transactional_replay_safe_restart_safe_and_one_way() {
    let persistence = disposable_database().await;
    persistence.reset_disposable_identity_data().await.unwrap();
    let start = start_command();
    verify_session_start_replay(&persistence, &start).await;
    insert_account_and_character(&persistence).await;
    verify_onboarding_context(&persistence, &start).await;
    verify_first_combat_projection_is_transactional(&persistence).await;
    verify_loot_origin_is_atomic_replay_safe_and_immutable(&persistence).await;
    verify_link_and_crash_observations(&persistence).await;

    persistence.close().await;
    let restarted = disposable_database().await;
    verify_restart_end_and_publication(&restarted).await;
    restarted.reset_disposable_identity_data().await.unwrap();
    restarted.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly opted-in disposable PostgreSQL"]
async fn item_writes_succeed_without_a_telemetry_session_or_loot_sidecar() {
    let persistence = disposable_database().await;
    persistence.reset_disposable_identity_data().await.unwrap();
    insert_account_and_character(&persistence).await;
    let request = reward_request();
    let commit = reward_commit();
    assert!(matches!(
        persistence
            .transact_reward(request, |_| Ok(((), commit)))
            .await
            .unwrap(),
        RewardTransaction::Fresh { .. }
    ));
    let mut inspection = persistence.begin_transaction().await.unwrap();
    let ledger_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM item_ledger_events WHERE namespace_id=$1")
            .bind(WIPEABLE_CORE_NAMESPACE)
            .fetch_one(inspection.connection())
            .await
            .unwrap();
    let telemetry_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM item_ledger_telemetry_outbox_v1 WHERE namespace_id=$1",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .fetch_one(inspection.connection())
    .await
    .unwrap();
    assert_eq!(ledger_count, 1);
    assert_eq!(telemetry_count, 0);
    inspection.rollback().await.unwrap();
    persistence.reset_disposable_identity_data().await.unwrap();
    persistence.close().await;
}

async fn verify_session_start_replay(
    persistence: &PostgresPersistence,
    start: &M03TelemetrySessionStartV1,
) {
    let started = persistence
        .begin_m03_telemetry_session_v1(start)
        .await
        .unwrap();
    assert!(matches!(
        started.event,
        StoredM03TelemetryEventV1::Session(StoredM03SessionEventV1::Started)
    ));
    assert_eq!(
        persistence
            .begin_m03_telemetry_session_v1(start)
            .await
            .unwrap(),
        started
    );
    let mut changed_start = start.clone();
    changed_start.build_id = "m03-core-dev-changed".into();
    assert!(matches!(
        persistence
            .begin_m03_telemetry_session_v1(&changed_start)
            .await,
        Err(M03TelemetrySourceError::IdempotencyConflict)
    ));
}

async fn insert_account_and_character(persistence: &PostgresPersistence) {
    let mut owning_transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO accounts (namespace_id,account_id,state_version,slot_capacity)
         VALUES ($1,$2,1,2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT.as_slice())
    .execute(owning_transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO characters
         (namespace_id,account_id,character_id,roster_ordinal,class_id,level,
          oath_id,life_state,security_state)
         VALUES ($1,$2,$3,1,'class.grave_arbalist',1,NULL,0,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT.as_slice())
    .bind(CHARACTER.as_slice())
    .execute(owning_transaction.connection())
    .await
    .unwrap();
    for statement in [
        "INSERT INTO character_progression
         (namespace_id,account_id,character_id,total_xp,level,current_health,progression_version)
         VALUES ($1,$2,$3,0,1,120,1)",
        "INSERT INTO character_inventories
         (namespace_id,account_id,character_id,inventory_version)
         VALUES ($1,$2,$3,1)",
        "INSERT INTO character_oath_bargain_state
         (namespace_id,account_id,character_id,earned_bargain_slots,oath_bargain_version)
         VALUES ($1,$2,$3,0,1)",
    ] {
        sqlx::query(statement)
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(ACCOUNT.as_slice())
            .bind(CHARACTER.as_slice())
            .execute(owning_transaction.connection())
            .await
            .unwrap();
    }
    sqlx::query(
        "INSERT INTO ash_wallets (namespace_id,account_id,balance,wallet_version)
         VALUES ($1,$2,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT.as_slice())
    .execute(owning_transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_world_locations
         (namespace_id,account_id,character_id,character_version,location_kind,
          location_content_id,safe_arrival_kind)
         VALUES ($1,$2,$3,1,1,'hub.lantern_halls_01',0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT.as_slice())
    .bind(CHARACTER.as_slice())
    .execute(owning_transaction.connection())
    .await
    .unwrap();
    owning_transaction.commit().await.unwrap();
}

async fn verify_onboarding_context(
    persistence: &PostgresPersistence,
    start: &M03TelemetrySessionStartV1,
) {
    let initial = persistence.poll_m03_telemetry_sources_v1(16).await.unwrap();
    assert_eq!(initial.len(), 3);
    assert!(initial.iter().all(|source| {
        source.context.account_id == ACCOUNT
            && source.context.session_id == SESSION
            && source.context.build_id == start.build_id
            && source.context.content_bundle_version == start.content_bundle_version
            && source.context.cohort_tags == start.cohort_tags
    }));
    assert!(initial.iter().any(|source| matches!(
        source.event,
        StoredM03TelemetryEventV1::Onboarding(StoredM03OnboardingEventV1::AccountCreated)
    )));
    assert!(initial.iter().any(|source| matches!(
        source.event,
        StoredM03TelemetryEventV1::Onboarding(
            StoredM03OnboardingEventV1::CharacterCreated { ref class_id }
        ) if class_id == "class.grave_arbalist"
    )));
}

async fn verify_first_combat_projection_is_transactional(persistence: &PostgresPersistence) {
    write_first_combat_location(persistence, false).await;
    let after_rollback = persistence.poll_m03_telemetry_sources_v1(16).await.unwrap();
    assert_eq!(after_rollback.len(), 3);
    assert!(!after_rollback.iter().any(|source| matches!(
        source.event,
        StoredM03TelemetryEventV1::Onboarding(
            StoredM03OnboardingEventV1::CharacterEnteredCombat { .. }
        )
    )));

    write_first_combat_location(persistence, true).await;
    let after_commit = persistence.poll_m03_telemetry_sources_v1(16).await.unwrap();
    assert_eq!(after_commit.len(), 4);
    assert!(after_commit.iter().any(|source| matches!(
        source.event,
        StoredM03TelemetryEventV1::Onboarding(
            StoredM03OnboardingEventV1::CharacterEnteredCombat {
                ref class_id,
                ref source_content_id,
            }
        ) if class_id == "class.grave_arbalist"
            && source_content_id == "world.core_microrealm_01"
    )));
}

#[allow(
    clippy::too_many_lines,
    reason = "the component-complete first-combat root stays contiguous for atomic rollback audit"
)]
async fn write_first_combat_location(persistence: &PostgresPersistence, commit: bool) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO character_instance_lineages
         (namespace_id,account_id,character_id,lineage_id,content_id,layout_id,
          lineage_state,records_blake3,assets_blake3,localization_blake3)
         VALUES ($1,$2,$3,$4,'world.core_microrealm_01',
          'layout.core_private_life_01',0,$5,$6,$7)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT.as_slice())
    .bind(CHARACTER.as_slice())
    .bind(LINEAGE.as_slice())
    .bind(CORE_WORLD_RECORDS_BLAKE3)
    .bind(CORE_WORLD_ASSETS_BLAKE3)
    .bind(CORE_WORLD_LOCALIZATION_BLAKE3)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO entry_restore_progression_v3
         (namespace_id,account_id,character_id,restore_point_id,level,total_xp,
          current_health,progression_version,component_digest)
         VALUES ($1,$2,$3,$4,1,0,120,1,$5)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT.as_slice())
    .bind(CHARACTER.as_slice())
    .bind(RESTORE_POINT.as_slice())
    .bind([18_u8; 32].as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO entry_restore_progression_v1
         (namespace_id,account_id,character_id,restore_point_id,level,total_xp,
          current_health,progression_version)
         VALUES ($1,$2,$3,$4,1,0,120,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT.as_slice())
    .bind(CHARACTER.as_slice())
    .bind(RESTORE_POINT.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    persistence::stage_danger_entry_inventory_restore_v3(
        &mut transaction,
        ACCOUNT,
        CHARACTER,
        RESTORE_POINT,
        [17_u8; 16],
        0,
    )
    .await
    .unwrap();
    persistence::stage_danger_entry_oath_bargain_restore_v3(
        &mut transaction,
        ACCOUNT,
        CHARACTER,
        RESTORE_POINT,
    )
    .await
    .unwrap();
    persistence::stage_danger_entry_life_metrics_restore_v3(
        &mut transaction,
        ACCOUNT,
        CHARACTER,
        RESTORE_POINT,
    )
    .await
    .unwrap();
    persistence::stage_danger_entry_ash_wallet_restore_v3(
        &mut transaction,
        ACCOUNT,
        CHARACTER,
        RESTORE_POINT,
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_entry_restore_points
         (namespace_id,account_id,character_id,restore_point_id,lineage_id,
          source_location_id,restore_location_id,snapshot_contract_version,
          account_version,character_version,progression_version,inventory_version,
          oath_bargain_version,life_metrics_version,ash_wallet_version,component_mask,
          composite_digest,restore_state,records_blake3,assets_blake3,localization_blake3)
         VALUES ($1,$2,$3,$4,$5,'hub.lantern_halls_01','hub.lantern_halls_01',
          3,1,1,1,1,1,1,1,31,$6,0,$7,$8,$9)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT.as_slice())
    .bind(CHARACTER.as_slice())
    .bind(RESTORE_POINT.as_slice())
    .bind(LINEAGE.as_slice())
    .bind([16_u8; 32].as_slice())
    .bind(CORE_WORLD_RECORDS_BLAKE3)
    .bind(CORE_WORLD_ASSETS_BLAKE3)
    .bind(CORE_WORLD_LOCALIZATION_BLAKE3)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_world_locations
         SET character_version=2,location_kind=2,
             location_content_id='world.core_microrealm_01',safe_arrival_kind=NULL,
             instance_lineage_id=$4,entry_restore_point_id=$5,
             updated_at=transaction_timestamp()
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT.as_slice())
    .bind(CHARACTER.as_slice())
    .bind(LINEAGE.as_slice())
    .bind(RESTORE_POINT.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    if commit {
        transaction.commit().await.unwrap();
    } else {
        transaction.rollback().await.unwrap();
    }
}

fn reward_request() -> StoredRewardRequest {
    StoredRewardRequest {
        reward_request_id: [71; 16],
        account_id: ACCOUNT,
        character_id: CHARACTER,
        source_instance_id: [72; 16],
        reward_table_id: "reward.normal_outer".into(),
        content_revision: CORE_ITEM_CONTENT_REVISION.into(),
        epoch_id: "core".into(),
        canonical_request_hash: [73; 32],
    }
}

fn reward_commit() -> StoredRewardCommit {
    StoredRewardCommit {
        plan_hash: [74; 32],
        result_hash: [75; 32],
        audit_digest: [76; 32],
        entries: vec![StoredRewardEntry {
            roll_index: 0,
            template_id: "item.charm.ember_tooth.t1".into(),
            item_kind: 0,
            quantity: 1,
            item_level: Some(1),
            rarity: Some(0),
        }],
        items: vec![StoredRewardItem {
            item_uid: [77; 16],
            ledger_event_id: [78; 16],
            roll_index: 0,
            unit_ordinal: 0,
            template_id: "item.charm.ember_tooth.t1".into(),
            item_kind: 0,
            item_level: Some(1),
            rarity: Some(0),
            location_kind: 2,
            slot_index: Some(0),
            instance_id: None,
            pickup_id: None,
            expires_at_tick: None,
            provenance_kind: 1,
            salvage_band: 0,
            salvage_value: 0,
        }],
    }
}

async fn verify_loot_origin_is_atomic_replay_safe_and_immutable(persistence: &PostgresPersistence) {
    let request = reward_request();
    let commit = reward_commit();
    assert!(matches!(
        persistence
            .transact_reward(request.clone(), |_| Ok(((), commit.clone())))
            .await
            .unwrap(),
        RewardTransaction::Fresh { .. }
    ));
    assert!(matches!(
        persistence
            .transact_reward(
                request.clone(),
                |_| -> Result<((), StoredRewardCommit), PersistenceError> {
                    panic!("exact replay must not replan")
                },
            )
            .await
            .unwrap(),
        RewardTransaction::Replay(_)
    ));
    let mut changed = request;
    changed.reward_table_id = "reward.elite_outer".into();
    assert!(matches!(
        persistence
            .transact_reward(
                changed,
                |_| -> Result<((), StoredRewardCommit), PersistenceError> {
                    panic!("changed replay must fail before planning")
                },
            )
            .await,
        Err(PersistenceError::ItemIdempotencyConflict)
    ));

    let pending = persistence.poll_m03_telemetry_sources_v1(16).await.unwrap();
    let loot = pending.iter().find(|source| {
        matches!(
            source.event,
            StoredM03TelemetryEventV1::Loot(ref event)
                if event.action == StoredM03LootActionV1::Created
                    && event.item_uid == [77; 16]
                    && event.template_id == "item.charm.ember_tooth.t1"
                    && event.source_content_id == "reward.normal_outer"
                    && event.item_version == 1
        )
    });
    let Some(loot) = loot else {
        panic!(
            "committed reward item must project one immutable loot source; {}",
            loot_projection_diagnostic(persistence).await
        );
    };
    assert_eq!(loot.context.session_id, SESSION);
    assert_eq!(loot.context.build_id, "m03-core-dev-telemetry-1");
    assert_eq!(loot.context.content_bundle_version, "core-dev");

    let mut mutation = persistence.begin_transaction().await.unwrap();
    let update = sqlx::query(
        "UPDATE item_ledger_telemetry_outbox_v1 SET template_id='item.invalid' \
         WHERE namespace_id=$1 AND event_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(loot.event_id.as_slice())
    .execute(mutation.connection())
    .await;
    assert!(update.is_err());
    mutation.rollback().await.unwrap();
}

async fn loot_projection_diagnostic(persistence: &PostgresPersistence) -> String {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let eligible_sessions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM core_telemetry_sessions_v1 AS session
         JOIN item_ledger_events AS ledger
           ON ledger.namespace_id=session.namespace_id
          AND ledger.account_id=session.account_id
         WHERE ledger.namespace_id=$1 AND ledger.ledger_event_id=$2
           AND session.started_at <= ledger.committed_at
           AND (session.ended_at IS NULL OR session.ended_at >= ledger.committed_at)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind([78_u8; 16].as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let immutable_sources: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM item_ledger_events AS ledger
         JOIN item_instances AS item
           ON item.namespace_id=ledger.namespace_id AND item.item_uid=ledger.item_uid
         JOIN reward_requests AS reward
           ON reward.namespace_id=item.namespace_id
          AND reward.reward_request_id=item.creation_request_id
         WHERE ledger.namespace_id=$1 AND ledger.ledger_event_id=$2
           AND item.creation_kind=1",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind([78_u8; 16].as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let manual_projection = sqlx::query(
        "INSERT INTO item_ledger_telemetry_outbox_v1 (
             namespace_id,event_id,ledger_event_id,account_id,character_id,
             session_id,loot_action,item_uid,template_id,source_content_id,
             item_version,occurred_at
         )
         SELECT ledger.namespace_id,
                derive_m03_loot_telemetry_event_id_v1(0,ledger.ledger_event_id),
                ledger.ledger_event_id,ledger.account_id,ledger.character_id,
                session.session_id,0,item.item_uid,item.template_id,
                reward.reward_table_id,ledger.post_item_version,ledger.committed_at
         FROM item_ledger_events AS ledger
         JOIN item_instances AS item
           ON item.namespace_id=ledger.namespace_id AND item.item_uid=ledger.item_uid
         JOIN reward_requests AS reward
           ON reward.namespace_id=item.namespace_id
          AND reward.reward_request_id=item.creation_request_id
         JOIN core_telemetry_sessions_v1 AS session
           ON session.namespace_id=ledger.namespace_id
          AND session.account_id=ledger.account_id
          AND session.started_at <= ledger.committed_at
          AND (session.ended_at IS NULL OR session.ended_at >= ledger.committed_at)
         WHERE ledger.namespace_id=$1 AND ledger.ledger_event_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind([78_u8; 16].as_slice())
    .execute(transaction.connection())
    .await;
    let diagnostic = format!(
        "eligible_sessions={eligible_sessions}, immutable_sources={immutable_sources}, manual_projection={manual_projection:?}"
    );
    transaction.rollback().await.unwrap();
    diagnostic
}

async fn verify_link_and_crash_observations(persistence: &PostgresPersistence) {
    let disconnect = M03SessionObservationCommandV1 {
        session_id: SESSION,
        account_id: ACCOUNT,
        observation_id: [21; 16],
        occurred_at_utc_millis: STARTED_AT + 1_000,
        observation: M03SessionObservationV1::Disconnected,
    };
    let disconnected = persistence
        .record_m03_session_observation_v1(&disconnect)
        .await
        .unwrap();
    assert_eq!(
        persistence
            .record_m03_session_observation_v1(&disconnect)
            .await
            .unwrap(),
        disconnected
    );
    let invalid_second_disconnect = M03SessionObservationCommandV1 {
        observation_id: [22; 16],
        occurred_at_utc_millis: STARTED_AT + 1_100,
        ..disconnect.clone()
    };
    assert!(matches!(
        persistence
            .record_m03_session_observation_v1(&invalid_second_disconnect)
            .await,
        Err(M03TelemetrySourceError::InvalidTransition)
    ));
    let reconnected = persistence
        .record_m03_session_observation_v1(&M03SessionObservationCommandV1 {
            session_id: SESSION,
            account_id: ACCOUNT,
            observation_id: [23; 16],
            occurred_at_utc_millis: STARTED_AT + 1_500,
            observation: M03SessionObservationV1::Reconnected,
        })
        .await
        .unwrap();
    assert!(matches!(
        reconnected.event,
        StoredM03TelemetryEventV1::Session(StoredM03SessionEventV1::Reconnected {
            link_lost_millis: 500
        })
    ));

    let crash = M03CrashObservationCommandV1 {
        crash_id: [31; 16],
        account_id: ACCOUNT,
        character_id: Some(CHARACTER),
        session_id: SESSION,
        source: StoredM03CrashSourceV1::Client,
        kind: StoredM03CrashKindV1::Panic,
        reporter: StoredM03CrashReporterV1::AuthenticatedClient,
        signature: [41; 32],
        uptime_millis: 1_700,
        occurred_at_utc_millis: STARTED_AT + 1_700,
    };
    let crashed = persistence
        .record_m03_crash_observation_v1(&crash)
        .await
        .unwrap();
    assert_eq!(
        persistence
            .record_m03_crash_observation_v1(&crash)
            .await
            .unwrap(),
        crashed
    );
    let mut changed_crash = crash.clone();
    changed_crash.signature[0] ^= 1;
    assert!(matches!(
        persistence
            .record_m03_crash_observation_v1(&changed_crash)
            .await,
        Err(M03TelemetrySourceError::IdempotencyConflict)
    ));
}

async fn verify_restart_end_and_publication(restarted: &PostgresPersistence) {
    assert_eq!(
        restarted
            .load_open_m03_telemetry_session_v1(ACCOUNT)
            .await
            .unwrap()
            .unwrap()
            .session_id,
        SESSION
    );
    let ended = restarted
        .record_m03_session_observation_v1(&M03SessionObservationCommandV1 {
            session_id: SESSION,
            account_id: ACCOUNT,
            observation_id: [24; 16],
            occurred_at_utc_millis: STARTED_AT + 2_000,
            observation: M03SessionObservationV1::Ended(StoredM03SessionEndReasonV1::CleanExit),
        })
        .await
        .unwrap();
    assert!(matches!(
        ended.event,
        StoredM03TelemetryEventV1::Session(StoredM03SessionEventV1::Ended {
            duration_millis: 2_000,
            reason: StoredM03SessionEndReasonV1::CleanExit
        })
    ));
    assert!(
        restarted
            .load_open_m03_telemetry_session_v1(ACCOUNT)
            .await
            .unwrap()
            .is_none()
    );

    verify_adapter_projection_and_publication(restarted).await;
}

async fn verify_adapter_projection_and_publication(restarted: &PostgresPersistence) {
    let mut adapter = PostgresM03TelemetryDomainAdapter::new(
        restarted.clone(),
        TelemetryPseudonymizationKeyV1::new([51; 32]).unwrap(),
    );
    let pending = adapter.poll_unpublished(16).await.unwrap();
    assert_eq!(pending.len(), 9);
    let mut names = pending
        .iter()
        .map(|source| source.envelope().event_name())
        .collect::<Vec<_>>();
    names.sort_unstable();
    assert_eq!(
        names,
        vec![
            "account_created",
            "character_created",
            "character_entered_combat",
            "client_crash",
            "disconnect",
            "item_created",
            "reconnect",
            "session_ended",
            "session_started",
        ]
    );
    let mut pipeline = TelemetryPipeline::new(
        TelemetryPipelineMode::Enabled,
        TelemetryConnectivity::Online,
        16,
    )
    .unwrap();
    for source in pending.iter().cloned() {
        assert_eq!(
            pipeline.ingest_committed(source),
            TelemetryIngestOutcome::Queued
        );
    }
    let documents = pipeline.prepare_redacted_batch(16).unwrap();
    assert_eq!(documents.len(), 9);
    assert!(documents.iter().all(|document| {
        document.json.contains("m03-core-dev-telemetry-1")
            && document.json.contains("\"platform\":\"windows\"")
            && document.json.contains("\"environment\":\"test\"")
            && !document.json.contains(&"0b".repeat(16))
            && !document.json.contains("auth_ticket")
    }));

    let first = [pending[0].outbox_id()];
    assert_eq!(adapter.acknowledge_published(&first).await.unwrap(), first);
    assert!(matches!(
        adapter.acknowledge_published(&first).await,
        Err(M03TelemetryOutboxError::UnknownAcknowledgement)
    ));
    assert_eq!(
        restarted
            .poll_m03_telemetry_sources_v1(16)
            .await
            .unwrap()
            .len(),
        8
    );
    let remaining = pending[1..]
        .iter()
        .map(telemetry::CommittedOutboxEventV1::outbox_id)
        .collect::<Vec<_>>();
    let mut canonical_remaining = remaining.clone();
    canonical_remaining.sort_unstable();
    assert_eq!(
        adapter.acknowledge_published(&remaining).await.unwrap(),
        canonical_remaining
    );
    assert!(
        restarted
            .poll_m03_telemetry_sources_v1(16)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        adapter.acknowledge_published(&[remaining[0]]).await,
        Err(M03TelemetryOutboxError::UnknownAcknowledgement)
    ));
}
