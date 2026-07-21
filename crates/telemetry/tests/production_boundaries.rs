use std::{
    collections::BTreeSet,
    io,
    sync::{Arc, Mutex},
};

use telemetry::{
    CommittedOutboxEventV1, CommittedTelemetrySource, CrashEventV1, CrashKindV1, CrashSourceV1,
    OnboardingEventV1, PseudonymousAccountId, RedactedTelemetryDocument, StableTelemetryId,
    TelemetryConnectivity, TelemetryContextV1, TelemetryEnvironmentV1, TelemetryEventV1,
    TelemetryExportAcceptance, TelemetryExporter, TelemetryId, TelemetryIngestOutcome,
    TelemetryPipeline, TelemetryPipelineMode, TelemetryPlatformV1, TelemetryWorker,
    TelemetryWorkerOutcome, VersionedTelemetryEnvelopeV1,
};

fn id(byte: u8) -> TelemetryId {
    TelemetryId::new([byte; 16]).expect("nonzero test ID")
}

fn stable(value: &str) -> StableTelemetryId {
    StableTelemetryId::new(value).expect("safe stable ID")
}

fn committed(byte: u8) -> CommittedOutboxEventV1 {
    let context = TelemetryContextV1 {
        pseudonymous_account_id: PseudonymousAccountId::new([0xa5; 32]).unwrap(),
        character_id: None,
        session_id: id(0x20),
        build_id: stable("m03.telemetry.test"),
        content_bundle_version: stable("core.0.3.0"),
        platform: TelemetryPlatformV1::Windows,
        region_id: stable("local-test"),
        environment: TelemetryEnvironmentV1::Test,
        cohort_tags: vec![stable("internal-test")],
    };
    let envelope = VersionedTelemetryEnvelopeV1::new(
        id(byte),
        1_000,
        context,
        TelemetryEventV1::Onboarding(OnboardingEventV1::AccountCreated),
    )
    .unwrap();
    CommittedOutboxEventV1::from_committed_row(id(byte.wrapping_add(0x40)), 1, 1_001, envelope)
        .unwrap()
}

#[test]
fn export_has_the_tel_001_envelope_and_no_secret_or_network_fields() {
    let mut pipeline = TelemetryPipeline::new(
        TelemetryPipelineMode::Enabled,
        TelemetryConnectivity::Online,
        2,
    )
    .unwrap();
    assert_eq!(
        pipeline.ingest_committed(committed(1)),
        TelemetryIngestOutcome::Queued
    );
    let document = pipeline.prepare_redacted_batch(1).unwrap().pop().unwrap();
    let value: serde_json::Value = serde_json::from_str(&document.json).unwrap();
    for required in [
        "event_id",
        "event_name",
        "event_schema_version",
        "occurred_at_utc",
        "pseudonymous_account_id",
        "session_id",
        "build_id",
        "content_bundle_version",
        "platform",
        "region_id",
        "environment",
        "cohort_tags",
    ] {
        assert!(value.get(required).is_some(), "missing {required}");
    }
    let json = document.json.to_ascii_lowercase();
    for forbidden in [
        "account_id",
        "ip_address",
        "socket_address",
        "auth_ticket",
        "access_token",
        "email",
        "platform_id",
        "stack_trace",
        "crash_message",
    ] {
        assert!(!json.contains(&format!("\"{forbidden}\"")));
    }
}

#[test]
fn stable_labels_and_crashes_cannot_carry_raw_sensitive_text() {
    for forbidden in [
        "127.0.0.1",
        "2001:db8::1",
        "player@example.com",
        "c:\\secrets\\token.txt",
        "bearer.secret",
        "token_value",
        "sk_live_key",
        "eyjhbGciOiJIUzI1NiJ9",
    ] {
        assert!(
            StableTelemetryId::new(forbidden).is_err(),
            "accepted {forbidden}"
        );
    }

    let context = TelemetryContextV1 {
        pseudonymous_account_id: PseudonymousAccountId::new([7; 32]).unwrap(),
        character_id: None,
        session_id: id(2),
        build_id: stable("m03.test"),
        content_bundle_version: stable("core.0.3.0"),
        platform: TelemetryPlatformV1::Windows,
        region_id: stable("local-test"),
        environment: TelemetryEnvironmentV1::Test,
        cohort_tags: Vec::new(),
    };
    let zero_signature = TelemetryEventV1::Crash(CrashEventV1 {
        crash_id: id(3),
        source: CrashSourceV1::Client,
        kind: CrashKindV1::Panic,
        signature: [0; 32],
        uptime_millis: 30,
    });
    assert!(VersionedTelemetryEnvelopeV1::new(id(4), 1, context, zero_signature).is_err());
}

#[test]
fn offline_queue_is_bounded_deduplicated_and_acknowledged_only_after_delivery() {
    let mut pipeline = TelemetryPipeline::new(
        TelemetryPipelineMode::Enabled,
        TelemetryConnectivity::Offline,
        2,
    )
    .unwrap();
    let first = committed(1);
    let first_outbox = first.outbox_id();
    assert_eq!(
        pipeline.ingest_committed(first.clone()),
        TelemetryIngestOutcome::Queued
    );
    assert_eq!(
        pipeline.ingest_committed(first),
        TelemetryIngestOutcome::Duplicate
    );
    assert_eq!(
        pipeline.ingest_committed(committed(2)),
        TelemetryIngestOutcome::Queued
    );
    assert_eq!(
        pipeline.ingest_committed(committed(3)),
        TelemetryIngestOutcome::Backpressured
    );
    assert_eq!(pipeline.queued_len(), 2);
    assert!(pipeline.prepare_redacted_batch(2).unwrap().is_empty());

    pipeline.set_connectivity(TelemetryConnectivity::Online);
    assert_eq!(pipeline.prepare_redacted_batch(2).unwrap().len(), 2);
    assert_eq!(pipeline.acknowledge_delivered(&[first_outbox]), 1);
    assert_eq!(pipeline.queued_len(), 1);
}

#[test]
fn disabled_pipeline_never_retains_or_exports_events() {
    let mut pipeline = TelemetryPipeline::new(
        TelemetryPipelineMode::Disabled,
        TelemetryConnectivity::Online,
        4,
    )
    .unwrap();
    assert_eq!(
        pipeline.ingest_committed(committed(1)),
        TelemetryIngestOutcome::Disabled
    );
    assert_eq!(pipeline.queued_len(), 0);
    assert!(pipeline.prepare_redacted_batch(1).unwrap().is_empty());
}

#[derive(Debug, Default)]
struct DurableTestState {
    rows: Vec<CommittedOutboxEventV1>,
    published: BTreeSet<TelemetryId>,
    poll_calls: usize,
    acknowledge_calls: usize,
}

#[derive(Debug, Clone)]
struct TestSource(Arc<Mutex<DurableTestState>>);

impl CommittedTelemetrySource for TestSource {
    type Error = io::Error;

    async fn poll_unpublished(
        &mut self,
        limit: usize,
    ) -> Result<Vec<CommittedOutboxEventV1>, Self::Error> {
        let mut state = self.0.lock().unwrap();
        state.poll_calls += 1;
        Ok(state
            .rows
            .iter()
            .filter(|row| !state.published.contains(&row.outbox_id()))
            .take(limit)
            .cloned()
            .collect())
    }

    async fn acknowledge_published(
        &mut self,
        accepted: &[TelemetryId],
    ) -> Result<Vec<TelemetryId>, Self::Error> {
        let mut state = self.0.lock().unwrap();
        state.acknowledge_calls += 1;
        let known = state
            .rows
            .iter()
            .map(CommittedOutboxEventV1::outbox_id)
            .collect::<BTreeSet<_>>();
        let mut published = accepted
            .iter()
            .copied()
            .filter(|event_id| known.contains(event_id) && state.published.insert(*event_id))
            .collect::<Vec<_>>();
        published.sort_unstable();
        Ok(published)
    }
}

#[derive(Debug)]
struct TestExporter {
    fail: bool,
    calls: usize,
}

impl TelemetryExporter for TestExporter {
    type Error = io::Error;

    async fn export(
        &mut self,
        documents: &[RedactedTelemetryDocument],
    ) -> Result<TelemetryExportAcceptance, Self::Error> {
        self.calls += 1;
        if self.fail {
            return Err(io::Error::other("injected response failure"));
        }
        TelemetryExportAcceptance::new(
            documents
                .iter()
                .map(|document| document.outbox_id)
                .collect(),
        )
        .map_err(io::Error::other)
    }
}

#[tokio::test]
async fn exporter_response_failure_never_publishes_and_restart_repolls_exact_row() {
    let state = Arc::new(Mutex::new(DurableTestState {
        rows: vec![committed(9)],
        ..DurableTestState::default()
    }));
    let mut first_source = TestSource(Arc::clone(&state));
    let mut failed_exporter = TestExporter {
        fail: true,
        calls: 0,
    };
    let mut first_worker = TelemetryWorker::enabled(4, TelemetryConnectivity::Online).unwrap();
    assert!(
        first_worker
            .run_once(&mut first_source, &mut failed_exporter, 4)
            .await
            .is_err()
    );
    assert_eq!(first_worker.queued_len(), 1);
    assert!(state.lock().unwrap().published.is_empty());
    assert_eq!(state.lock().unwrap().acknowledge_calls, 0);

    drop(first_worker);
    let mut restarted_source = TestSource(Arc::clone(&state));
    let mut successful_exporter = TestExporter {
        fail: false,
        calls: 0,
    };
    let mut restarted_worker = TelemetryWorker::enabled(4, TelemetryConnectivity::Online).unwrap();
    assert_eq!(
        restarted_worker
            .run_once(&mut restarted_source, &mut successful_exporter, 4)
            .await
            .unwrap(),
        TelemetryWorkerOutcome::Exported {
            newly_queued: 1,
            offered: 1,
            published: 1,
        }
    );
    assert_eq!(restarted_worker.queued_len(), 0);
    assert_eq!(state.lock().unwrap().published.len(), 1);
}

#[tokio::test]
async fn default_worker_is_disabled_and_never_touches_source_or_exporter() {
    let state = Arc::new(Mutex::new(DurableTestState {
        rows: vec![committed(10)],
        ..DurableTestState::default()
    }));
    let mut source = TestSource(Arc::clone(&state));
    let mut exporter = TestExporter {
        fail: false,
        calls: 0,
    };
    let mut worker = TelemetryWorker::new(4).unwrap();
    assert_eq!(
        worker
            .run_once(&mut source, &mut exporter, 4)
            .await
            .unwrap(),
        TelemetryWorkerOutcome::Disabled
    );
    assert_eq!(state.lock().unwrap().poll_calls, 0);
    assert_eq!(state.lock().unwrap().acknowledge_calls, 0);
    assert_eq!(exporter.calls, 0);
}
