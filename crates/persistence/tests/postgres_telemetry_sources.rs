use persistence::{
    M03CrashObservationCommandV1, M03SessionObservationCommandV1, M03SessionObservationV1,
    M03TelemetryPublicationV1, M03TelemetrySessionStartV1, M03TelemetrySourceError,
    M03TelemetrySourceFamilyV1, PersistenceConfig, PostgresPersistence, StoredM03CrashKindV1,
    StoredM03CrashReporterV1, StoredM03CrashSourceV1, StoredM03OnboardingEventV1,
    StoredM03SessionEndReasonV1, StoredM03SessionEventV1, StoredM03TelemetryEnvironmentV1,
    StoredM03TelemetryEventV1, StoredM03TelemetryPlatformV1, StoredM03TelemetrySourceV1,
    WIPEABLE_CORE_NAMESPACE,
};

const ACCOUNT: [u8; 16] = [11; 16];
const CHARACTER: [u8; 16] = [12; 16];
const SESSION: [u8; 16] = [13; 16];
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

fn publication(source: &StoredM03TelemetrySourceV1) -> M03TelemetryPublicationV1 {
    let family = match source.event {
        StoredM03TelemetryEventV1::Onboarding(_) => M03TelemetrySourceFamilyV1::Onboarding,
        StoredM03TelemetryEventV1::Session(_) => M03TelemetrySourceFamilyV1::Session,
        StoredM03TelemetryEventV1::Crash(_) => M03TelemetrySourceFamilyV1::Crash,
    };
    M03TelemetryPublicationV1 {
        family,
        event_id: source.event_id,
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
    verify_link_and_crash_observations(&persistence).await;

    persistence.close().await;
    let restarted = disposable_database().await;
    verify_restart_end_and_publication(&restarted).await;
    restarted.reset_disposable_identity_data().await.unwrap();
    restarted.close().await;
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

async fn write_first_combat_location(persistence: &PostgresPersistence, commit: bool) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
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
    .bind([14_u8; 16].as_slice())
    .bind([15_u8; 16].as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    if commit {
        transaction.commit().await.unwrap();
    } else {
        transaction.rollback().await.unwrap();
    }
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

    let pending = restarted.poll_m03_telemetry_sources_v1(16).await.unwrap();
    assert_eq!(pending.len(), 8);
    let accepted: Vec<_> = pending.iter().map(publication).collect();
    assert_eq!(
        restarted
            .acknowledge_m03_telemetry_sources_v1(&accepted)
            .await
            .unwrap(),
        {
            let mut canonical = accepted.clone();
            canonical.sort_unstable_by_key(|source| (source.event_id, source.family));
            canonical
        }
    );
    assert!(
        restarted
            .poll_m03_telemetry_sources_v1(16)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        restarted
            .acknowledge_m03_telemetry_sources_v1(&accepted[..1])
            .await,
        Err(M03TelemetrySourceError::PublicationConflict)
    ));
}
