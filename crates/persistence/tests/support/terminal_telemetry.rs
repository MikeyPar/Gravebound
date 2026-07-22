//! Shared hosted proof helpers for GB-M03-09 terminal telemetry.
//!
//! The three authorities are the canonical Production GDD (`TECH-021..023`, `TECH-123`,
//! `TEL-001..005`), Content Production Spec (Core stable IDs and terminal boundaries), and
//! Development Roadmap (`ADR-005`, `GB-M03-06..09`).

use persistence::{
    M03TelemetrySessionStartV1, PostgresM03TelemetryOutboxAdapter, PostgresPersistence,
    StoredM03TelemetryEnvironmentV1, StoredM03TelemetryPlatformV1, TelemetryPseudonymizationKeyV1,
    WIPEABLE_CORE_NAMESPACE,
};
use sqlx::Row;
use telemetry::{CommittedTelemetrySource, TelemetryId};

const APP_AUTHORED_SKEWED_START_MILLIS: u64 = 1_750_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "each integration-test binary exercises only its owning terminal family"
)]
pub enum TerminalFamily {
    Death,
    Extraction,
    Recall,
    Successor,
}

impl TerminalFamily {
    const fn select_sql(self) -> &'static str {
        match self {
            Self::Death => {
                "SELECT event_id,origin_session_id,published_at IS NOT NULL AS published \
                 FROM death_outbox_events \
                 WHERE namespace_id=$1 AND event_type='death_committed' \
                 ORDER BY created_at DESC,event_id DESC LIMIT 1"
            }
            Self::Extraction => {
                "SELECT event_id,origin_session_id,published_at IS NOT NULL AS published \
                 FROM extraction_terminal_outbox_events_v1 \
                 WHERE namespace_id=$1 AND event_type='extraction_committed' \
                 ORDER BY created_at DESC,event_id DESC LIMIT 1"
            }
            Self::Recall => {
                "SELECT event_id,origin_session_id,published_at IS NOT NULL AS published \
                 FROM recall_terminal_outbox_events_v1 \
                 WHERE namespace_id=$1 \
                   AND event_type IN ('emergency_recall_committed','disconnect_recovery_committed') \
                 ORDER BY created_at DESC,event_id DESC LIMIT 1"
            }
            Self::Successor => {
                "SELECT event_id,origin_session_id,published_at IS NOT NULL AS published \
                 FROM successor_mutation_outbox_events_v1 \
                 WHERE namespace_id=$1 AND event_type=1 \
                 ORDER BY created_at DESC,event_id DESC LIMIT 1"
            }
        }
    }

    const fn mutate_origin_sql(self) -> &'static str {
        match self {
            Self::Death => {
                "UPDATE death_outbox_events SET origin_session_id=$1,origin_account_id=$2 \
                 WHERE namespace_id=$3 AND event_id=$4"
            }
            Self::Extraction => {
                "UPDATE extraction_terminal_outbox_events_v1 SET origin_session_id=$1 \
                 WHERE namespace_id=$2 AND event_id=$3"
            }
            Self::Recall => {
                "UPDATE recall_terminal_outbox_events_v1 SET origin_session_id=$1 \
                 WHERE namespace_id=$2 AND event_id=$3"
            }
            Self::Successor => {
                "UPDATE successor_mutation_outbox_events_v1 SET origin_session_id=$1 \
                 WHERE namespace_id=$2 AND event_id=$3"
            }
        }
    }

    const fn republish_sql(self) -> &'static str {
        match self {
            Self::Death => {
                "UPDATE death_outbox_events SET published_at=clock_timestamp() \
                 WHERE namespace_id=$1 AND event_id=$2"
            }
            Self::Extraction => {
                "UPDATE extraction_terminal_outbox_events_v1 SET published_at=clock_timestamp() \
                 WHERE namespace_id=$1 AND event_id=$2"
            }
            Self::Recall => {
                "UPDATE recall_terminal_outbox_events_v1 SET published_at=clock_timestamp() \
                 WHERE namespace_id=$1 AND event_id=$2"
            }
            Self::Successor => {
                "UPDATE successor_mutation_outbox_events_v1 SET published_at=clock_timestamp() \
                 WHERE namespace_id=$1 AND event_id=$2"
            }
        }
    }
}

pub async fn start_skewed_session(
    persistence: &PostgresPersistence,
    account_id: [u8; 16],
    session_id: [u8; 16],
) {
    persistence
        .begin_m03_telemetry_session_v1(&M03TelemetrySessionStartV1 {
            session_id,
            account_id,
            build_id: "m03-terminal-origin-hosted-test".into(),
            content_bundle_version: "core-dev".into(),
            platform: StoredM03TelemetryPlatformV1::Windows,
            region_id: "local".into(),
            environment: StoredM03TelemetryEnvironmentV1::Test,
            cohort_tags: vec!["cohort.private".into(), "staff".into()],
            // Deliberately unrelated to the database wall clock. Origin attribution must use the
            // session outbox's PostgreSQL-authored created_at boundary instead.
            started_at_utc_millis: APP_AUTHORED_SKEWED_START_MILLIS,
        })
        .await
        .unwrap();
}

async fn latest_terminal(
    persistence: &PostgresPersistence,
    family: TerminalFamily,
) -> ([u8; 16], Option<[u8; 16]>, bool) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let row = sqlx::query(family.select_sql())
        .bind(WIPEABLE_CORE_NAMESPACE)
        .fetch_one(transaction.connection())
        .await
        .unwrap();
    let event_id: Vec<u8> = row.try_get("event_id").unwrap();
    let origin_session_id: Option<Vec<u8>> = row.try_get("origin_session_id").unwrap();
    let published: bool = row.try_get("published").unwrap();
    transaction.rollback().await.unwrap();
    (
        event_id.try_into().unwrap(),
        origin_session_id.map(|value| value.try_into().unwrap()),
        published,
    )
}

pub async fn assert_unbound_terminal(persistence: &PostgresPersistence, family: TerminalFamily) {
    let (_, origin_session_id, published) = latest_terminal(persistence, family).await;
    assert_eq!(
        origin_session_id, None,
        "a missing session must fail open without guessing a telemetry origin"
    );
    assert!(!published, "unbound telemetry must not enter the publisher");
}

pub async fn assert_bound_immutable_restart_poll_ack(
    persistence: &PostgresPersistence,
    family: TerminalFamily,
    expected_session_id: [u8; 16],
    expected_event_name: &str,
) {
    let (event_id, origin_session_id, published) = latest_terminal(persistence, family).await;
    assert_eq!(origin_session_id, Some(expected_session_id));
    assert!(!published);

    let altered_session_id = [0xFD; 16];
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let origin_mutation = match family {
        TerminalFamily::Death => {
            sqlx::query(family.mutate_origin_sql())
                .bind(altered_session_id.as_slice())
                .bind([0xFC; 16].as_slice())
                .bind(WIPEABLE_CORE_NAMESPACE)
                .bind(event_id.as_slice())
                .execute(transaction.connection())
                .await
        }
        _ => {
            sqlx::query(family.mutate_origin_sql())
                .bind(altered_session_id.as_slice())
                .bind(WIPEABLE_CORE_NAMESPACE)
                .bind(event_id.as_slice())
                .execute(transaction.connection())
                .await
        }
    };
    let origin_error = origin_mutation.expect_err("terminal origin must be immutable");
    let origin_message = origin_error
        .as_database_error()
        .map(sqlx::error::DatabaseError::message)
        .unwrap_or_default();
    assert!(
        origin_message.contains("terminal telemetry origin is immutable"),
        "origin guard must reject before a foreign-key side effect: {origin_error}"
    );
    transaction.rollback().await.unwrap();

    let key = TelemetryPseudonymizationKeyV1::new([0xA5; 32]).unwrap();
    let mut first_adapter =
        PostgresM03TelemetryOutboxAdapter::from_persistence(persistence.clone(), key);
    let first_poll = first_adapter.poll_unpublished(256).await.unwrap();
    let target = TelemetryId::new(event_id).unwrap();
    assert!(first_poll.iter().any(|event| {
        event.outbox_id() == target && event.envelope().event_name() == expected_event_name
    }));

    // Dropping the process-local adapter without acknowledgement simulates a publisher restart.
    drop(first_adapter);
    let key = TelemetryPseudonymizationKeyV1::new([0xA5; 32]).unwrap();
    let mut restarted_adapter =
        PostgresM03TelemetryOutboxAdapter::from_persistence(persistence.clone(), key);
    let restart_poll = restarted_adapter.poll_unpublished(256).await.unwrap();
    assert!(restart_poll.iter().any(|event| event.outbox_id() == target));
    let all_polled = restart_poll
        .iter()
        .map(telemetry::CommittedOutboxEventV1::outbox_id)
        .collect::<Vec<_>>();
    let acknowledged = restarted_adapter
        .acknowledge_published(&all_polled)
        .await
        .unwrap();
    assert!(acknowledged.contains(&target));
    assert!(
        restarted_adapter
            .poll_unpublished(256)
            .await
            .unwrap()
            .is_empty()
    );

    let (_, durable_origin, published) = latest_terminal(persistence, family).await;
    assert_eq!(durable_origin, Some(expected_session_id));
    assert!(published);

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let republish = sqlx::query(family.republish_sql())
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(event_id.as_slice())
        .execute(transaction.connection())
        .await;
    let republish_error = republish.expect_err("published_at may advance exactly once");
    let republish_message = republish_error
        .as_database_error()
        .map(sqlx::error::DatabaseError::message)
        .unwrap_or_default();
    assert!(
        republish_message.contains("permits only first publication"),
        "publish-only guard must be the rejecting authority: {republish_error}"
    );
    transaction.rollback().await.unwrap();
}
