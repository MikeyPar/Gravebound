//! Focused `PostgreSQL` contract proof for GB-M03-09 terminal telemetry origins.
//!
//! Authorities:
//! - `Gravebound_Production_GDD_v1_Canonical.md` TECH-021..023, TECH-123,
//!   and TEL-001..005;
//! - `Gravebound_Content_Production_Spec_v1.md` CONT-CATALOG-003,
//!   CONT-ROOM-007, and CONT-BOSS-001;
//! - `Gravebound_Development_Roadmap_v1.md` ADR-005 and GB-M03-06..09.

use persistence::{
    M03SessionObservationCommandV1, M03SessionObservationV1, M03TelemetrySessionStartV1,
    PersistenceConfig, PostgresPersistence, StoredM03SessionEndReasonV1,
    StoredM03TelemetryEnvironmentV1, StoredM03TelemetryPlatformV1, WIPEABLE_CORE_NAMESPACE,
};
use sqlx::Row;

const ACCOUNT_ID: [u8; 16] = [0x31; 16];
const OTHER_ACCOUNT_ID: [u8; 16] = [0x32; 16];
const FIRST_SESSION_ID: [u8; 16] = [0x41; 16];
const SECOND_SESSION_ID: [u8; 16] = [0x42; 16];
const OVERLAPPING_SESSION_ID: [u8; 16] = [0x43; 16];
const STARTED_AT_MILLIS: u64 = 1_750_000_000_000;
const BOUNDARY_MILLIS: u64 = STARTED_AT_MILLIS + 10_000;

fn persistence_config() -> PersistenceConfig {
    PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL")
}

async fn disposable_database() -> PostgresPersistence {
    let persistence = PostgresPersistence::connect(&persistence_config())
        .await
        .unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence.migrate().await.unwrap();
    persistence.reset_disposable_identity_data().await.unwrap();
    persistence
}

fn session_start(session_id: [u8; 16], started_at_utc_millis: u64) -> M03TelemetrySessionStartV1 {
    M03TelemetrySessionStartV1 {
        session_id,
        account_id: ACCOUNT_ID,
        build_id: "m03-terminal-origin-test".into(),
        content_bundle_version: "core-dev".into(),
        platform: StoredM03TelemetryPlatformV1::Windows,
        region_id: "local".into(),
        environment: StoredM03TelemetryEnvironmentV1::Test,
        cohort_tags: vec!["cohort.private".into(), "staff".into()],
        started_at_utc_millis,
    }
}

async fn resolve_origin(
    persistence: &PostgresPersistence,
    account_id: [u8; 16],
    occurred_at_micros: i64,
) -> Option<[u8; 16]> {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let row = sqlx::query(
        "SELECT resolve_m03_terminal_telemetry_session_v1(
             $1,$2,to_timestamp($3::double precision/1000000.0)
         ) AS session_id",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(occurred_at_micros)
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let session_id: Option<Vec<u8>> = row.try_get("session_id").unwrap();
    transaction.rollback().await.unwrap();
    session_id.map(|value| value.try_into().unwrap())
}

async fn session_boundary_micros(
    persistence: &PostgresPersistence,
    session_id: [u8; 16],
    event_kind: i16,
) -> i64 {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let value = sqlx::query_scalar(
        "SELECT floor(extract(epoch FROM created_at)*1000000)::bigint
         FROM session_outbox_events_v1
         WHERE namespace_id=$1 AND session_id=$2 AND event_kind=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(session_id.as_slice())
    .bind(event_kind)
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    value
}

async fn insert_overlapping_closed_session(persistence: &PostgresPersistence, overlap_micros: i64) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO core_telemetry_sessions_v1
         (namespace_id,session_id,account_id,build_id,content_bundle_version,
          platform,region_id,environment,cohort_tags,started_at,ended_at,end_reason)
         VALUES ($1,$2,$3,'m03-overlap-test','core-dev',0,'local',1,
                 ARRAY['cohort.private'],to_timestamp($4::double precision/1000.0),
                 to_timestamp($5::double precision/1000.0),2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(OVERLAPPING_SESSION_ID.as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(i64::try_from(STARTED_AT_MILLIS).unwrap())
    .bind(i64::try_from(BOUNDARY_MILLIS).unwrap())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO session_outbox_events_v1
         (namespace_id,event_id,source_id,account_id,session_id,event_sequence,event_kind,
          occurred_at,created_at)
         VALUES ($1,$2,$3,$4,$3,1,0,to_timestamp($5::double precision/1000.0),
                 to_timestamp($6::double precision/1000000.0))",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind([0x61_u8; 16].as_slice())
    .bind(OVERLAPPING_SESSION_ID.as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(i64::try_from(STARTED_AT_MILLIS).unwrap())
    .bind(overlap_micros - 1)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO session_outbox_events_v1
         (namespace_id,event_id,source_id,account_id,session_id,event_sequence,event_kind,
          duration_millis,end_reason,occurred_at,created_at)
         VALUES ($1,$2,$3,$4,$5,2,1,$6,2,
                 to_timestamp($7::double precision/1000.0),
                 to_timestamp($8::double precision/1000000.0))",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind([0x62_u8; 16].as_slice())
    .bind([0x63_u8; 16].as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(OVERLAPPING_SESSION_ID.as_slice())
    .bind(i64::try_from(BOUNDARY_MILLIS - STARTED_AT_MILLIS).unwrap())
    .bind(i64::try_from(BOUNDARY_MILLIS).unwrap())
    .bind(overlap_micros + 1)
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

#[tokio::test]
#[ignore = "requires dedicated disposable PostgreSQL via TEST_DATABASE_URL"]
async fn schema_73_uses_one_database_clock_and_fails_open_on_ambiguous_origin() {
    let persistence = disposable_database().await;

    persistence
        .begin_m03_telemetry_session_v1(&session_start(FIRST_SESSION_ID, STARTED_AT_MILLIS))
        .await
        .unwrap();
    let first_started = session_boundary_micros(&persistence, FIRST_SESSION_ID, 0).await;
    assert_eq!(
        resolve_origin(&persistence, ACCOUNT_ID, first_started).await,
        Some(FIRST_SESSION_ID)
    );
    assert_eq!(
        resolve_origin(&persistence, OTHER_ACCOUNT_ID, first_started).await,
        None,
        "missing context must remain unbound instead of borrowing another account's session"
    );
    assert_eq!(
        resolve_origin(
            &persistence,
            ACCOUNT_ID,
            i64::try_from(STARTED_AT_MILLIS * 1_000).unwrap(),
        )
        .await,
        None,
        "application-authored skewed time must not define the terminal origin interval"
    );

    insert_overlapping_closed_session(&persistence, first_started).await;
    assert_eq!(
        resolve_origin(&persistence, ACCOUNT_ID, first_started).await,
        None,
        "two database-clock intervals are ambiguous and must fail open"
    );

    persistence
        .record_m03_session_observation_v1(&M03SessionObservationCommandV1 {
            session_id: FIRST_SESSION_ID,
            account_id: ACCOUNT_ID,
            observation_id: [0x51; 16],
            occurred_at_utc_millis: BOUNDARY_MILLIS,
            observation: M03SessionObservationV1::Ended(
                StoredM03SessionEndReasonV1::TransportClosed,
            ),
        })
        .await
        .unwrap();
    persistence
        .begin_m03_telemetry_session_v1(&session_start(SECOND_SESSION_ID, BOUNDARY_MILLIS))
        .await
        .unwrap();

    let first_ended = session_boundary_micros(&persistence, FIRST_SESSION_ID, 1).await;
    let second_started = session_boundary_micros(&persistence, SECOND_SESSION_ID, 0).await;
    assert!(first_ended <= second_started);
    assert_eq!(
        resolve_origin(&persistence, ACCOUNT_ID, first_ended).await,
        (first_ended == second_started).then_some(SECOND_SESSION_ID),
        "the old interval is half-open and a database-clock handoff never guesses"
    );
    assert_eq!(
        resolve_origin(&persistence, ACCOUNT_ID, second_started).await,
        Some(SECOND_SESSION_ID)
    );
    persistence.close().await;
}
