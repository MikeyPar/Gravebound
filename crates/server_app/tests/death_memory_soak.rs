//! Explicit release-profile stability evidence for `GB-M03-06E`.
//!
//! Authorities:
//! - canonical GDD `DTH-001`, `DTH-021`, `TECH-022`, `TECH-023`, and `TECH-070`;
//! - Content Production Spec `CONT-ECHO-009` and `CONT-HUB-002`;
//! - Development Roadmap `GB-M03-06`, `GB-M03-13`, and the M03 exit gate.
//!
//! The ordinary CI matrix compiles this test but never spends its 30-minute wall-clock budget.
//! An explicit hosted workflow runs one committed death through sustained authenticated real-QUIC
//! reads, exact terminal replays, reconnect churn, canonical-signature checks, and final
//! server/PostgreSQL teardown inspection. Its combined Linux process RSS is a supplemental
//! subsystem-health ceiling, not target-Windows native-client `TECH-070` certification.

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use persistence::{PersistenceConfig, PostgresPersistence};
use protocol::{
    AuthTicket, ClientHello, Compression, DEATH_VIEW_SCHEMA_VERSION, DeathEchoOutcomeV1,
    DeathViewFrameV1, DeathViewRequestV1, DeathViewResultV1, HandshakeResponse, ManifestHash,
    Platform, WireText,
};
use rustls::pki_types::CertificateDer;
use server_app::{
    BoundCoreIdentityServer, CoreIdentityServerConfig, DurableDeathExecutionService,
    PostgresAccountRepository, SubmitResult, TerminalArbiter, TerminalCandidate,
    durable_death_terminal_candidate,
};
use tokio::sync::oneshot;

#[path = "support/death_measurement.rs"]
mod death_measurement;
#[path = "support/durable_death.rs"]
mod durable_death_fixture;

const REQUIRED_SOAK_DURATION: Duration = Duration::from_mins(30);
const MEMORY_SAMPLE_INTERVAL: Duration = Duration::from_secs(30);
const JOURNEY_PACING: Duration = Duration::from_millis(200);

fn content_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content")
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

fn death_view_frame(sequence: u32, request: DeathViewRequestV1) -> DeathViewFrameV1 {
    DeathViewFrameV1 {
        schema_version: DEATH_VIEW_SCHEMA_VERSION,
        sequence,
        content_revision: durable_death_fixture::death_view_revision(),
        request,
    }
}

fn production_death_view_hello() -> ClientHello {
    let (_, source_report) = sim_content::load_and_validate(&content_root()).unwrap();
    ClientHello {
        protocol_major: protocol::ProtocolVersion::current().major,
        protocol_minor: protocol::ProtocolVersion::current().minor,
        client_build_id: WireText::new(server_app::CORE_IDENTITY_BUILD_ID).unwrap(),
        platform: Platform::WindowsNative,
        supported_compression: vec![Compression::None],
        content_manifest_hash: ManifestHash::new(source_report.package_hash_blake3).unwrap(),
        auth_ticket: AuthTicket::new(durable_death_fixture::AUTH_TICKET.to_vec()).unwrap(),
        locale: WireText::new("en-US").unwrap(),
    }
}

fn client_endpoint(certificate_der: &[u8]) -> quinn::Endpoint {
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(CertificateDer::from(certificate_der.to_vec()))
        .unwrap();
    let config = quinn::ClientConfig::with_root_certificates(Arc::new(roots)).unwrap();
    let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    endpoint.set_default_client_config(config);
    endpoint
}

async fn connect_authenticated(
    endpoint: &quinn::Endpoint,
    address: std::net::SocketAddr,
) -> quinn::Connection {
    let connection = endpoint
        .connect(address, "localhost")
        .unwrap()
        .await
        .unwrap();
    assert!(matches!(
        bot_client::perform_handshake(&connection, production_death_view_hello())
            .await
            .unwrap(),
        HandshakeResponse::Accepted(server)
            if server.feature_flags.iter().any(
                |flag| flag.as_str() == protocol::CORE_DEATH_VIEW_FEATURE_FLAG
            )
    ));
    connection
}

async fn read_and_assert_complete_death_views(connection: &quinn::Connection, sequence: &mut u32) {
    let next_frame = |sequence: &mut u32, request| {
        let current = *sequence;
        *sequence = sequence.checked_add(1).expect("death-view sequence bound");
        death_view_frame(current, request)
    };
    let (_, latest) = bot_client::perform_death_view(
        connection,
        next_frame(sequence, DeathViewRequestV1::LatestCommitted),
    )
    .await
    .unwrap();
    let (_, summary) = bot_client::perform_death_view(
        connection,
        next_frame(
            sequence,
            DeathViewRequestV1::Summary {
                death_id: durable_death_fixture::DEATH_ID,
                lost_start_ordinal: 0,
                lost_limit: 8,
            },
        ),
    )
    .await
    .unwrap();
    let (_, memorial) = bot_client::perform_death_view(
        connection,
        next_frame(
            sequence,
            DeathViewRequestV1::MemorialPage {
                after: None,
                limit: 8,
            },
        ),
    )
    .await
    .unwrap();
    let (_, trace) = bot_client::perform_death_view(
        connection,
        next_frame(
            sequence,
            DeathViewRequestV1::TracePage {
                death_id: durable_death_fixture::DEATH_ID,
                start_ordinal: 0,
                limit: 8,
            },
        ),
    )
    .await
    .unwrap();

    assert!(matches!(
        latest,
        DeathViewResultV1::Latest {
            death: Some(latest),
            ..
        } if latest.death_id == durable_death_fixture::DEATH_ID
    ));
    assert!(matches!(
        summary,
        DeathViewResultV1::Summary { summary, .. }
            if summary.death_id == durable_death_fixture::DEATH_ID
                && summary.echo_outcome == DeathEchoOutcomeV1::Available
                && summary.lost.len() == 2
    ));
    assert!(matches!(
        memorial,
        DeathViewResultV1::MemorialPage {
            entries,
            next_cursor: None,
            ..
        } if entries.len() == 1
            && entries[0].cursor.death_id == durable_death_fixture::DEATH_ID
    ));
    assert!(matches!(
        trace,
        DeathViewResultV1::TracePage { page, .. }
            if page.death_id == durable_death_fixture::DEATH_ID
                && page.entries.len() == 2
                && page.entries.last().is_some_and(|entry| entry.lethal)
    ));
}

async fn canonical_signature_bytes(persistence: &PostgresPersistence) -> Vec<u8> {
    persistence
        .load_core_death_terminal_signature_v1(
            durable_death_fixture::ACCOUNT_ID,
            durable_death_fixture::CHARACTER_ID,
        )
        .await
        .unwrap()
        .expect("committed death-terminal signature")
        .canonical_bytes()
        .unwrap()
}

async fn exact_terminal_replay(
    persistence: &PostgresPersistence,
    candidate: &TerminalCandidate,
    death: &server_app::PreparedDurableDeathCommit,
) {
    let mut arbiter = TerminalArbiter::new(candidate.binding());
    assert!(matches!(
        arbiter.submit(candidate.clone()),
        SubmitResult::Accepted { .. }
    ));
    let prepared = arbiter
        .prepare(death.request().plan.event.death_tick)
        .unwrap();
    let replay = DurableDeathExecutionService::new(persistence.clone())
        .execute_prepared(&mut arbiter, &prepared, death)
        .await
        .unwrap();
    assert!(replay.transaction.is_replay());
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// This explicit release test is intentionally separate from `core_route_quic`: ordinary hosted
/// `PostgreSQL` CI must not silently inherit a 30-minute wall-clock gate. The report is accepted only
/// when the complete stored projections remain stable under sustained authenticated QUIC reads,
/// exact mutation replay, reconnect churn, and zero-residue shutdown.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit release-profile 30-minute GB-M03-06E death persistence soak"]
#[allow(
    clippy::too_many_lines,
    reason = "one uninterrupted wall-clock soak preserves measurement and teardown ordering"
)]
async fn death_persistence_real_quic_thirty_minute_soak() {
    let persistence = disposable_database().await;
    durable_death_fixture::seed_danger_root(&persistence).await;
    let death = durable_death_fixture::prepare_death(persistence.clone()).await;
    let candidate = durable_death_terminal_candidate(&death).unwrap();
    let mut initial_arbiter = TerminalArbiter::new(candidate.binding());
    assert!(matches!(
        initial_arbiter.submit(candidate.clone()),
        SubmitResult::Accepted { .. }
    ));
    let initial_prepared = initial_arbiter
        .prepare(death.request().plan.event.death_tick)
        .unwrap();
    let committed = DurableDeathExecutionService::new(persistence.clone())
        .execute_prepared(&mut initial_arbiter, &initial_prepared, &death)
        .await
        .unwrap();
    assert!(!committed.transaction.is_replay());
    durable_death_fixture::assert_committed_graph(&persistence).await;
    let expected_signature = canonical_signature_bytes(&persistence).await;

    let presentation = sim_content::load_core_development_death_view(&content_root()).unwrap();
    let hashes = presentation.hashes();
    let server = BoundCoreIdentityServer::bind_persistent(
        &CoreIdentityServerConfig {
            bind_address: "127.0.0.1:0".parse().unwrap(),
            content_root: content_root(),
        },
        PostgresAccountRepository::new(persistence.clone()),
    )
    .unwrap();
    let address = server.local_address();
    let endpoint = client_endpoint(server.certificate_der());
    let (shutdown_send, shutdown_receive) = oneshot::channel::<()>();
    let server_task = tokio::spawn(server.serve_until(async {
        let _ = shutdown_receive.await;
    }));

    let mut memory = death_measurement::ProcessMemorySampler::new().unwrap();
    let started = Instant::now();
    let mut samples = vec![death_measurement::ResidentMemorySampleV1 {
        elapsed_ms: 0,
        resident_bytes: memory.resident_bytes().unwrap(),
    }];
    let mut next_memory_sample = MEMORY_SAMPLE_INTERVAL;
    let mut connection = None;
    let mut sequence = 1_u32;
    let mut query_journeys = 0_u64;
    let mut death_view_queries = 0_u64;
    let mut connection_generations = 0_u64;
    let mut exact_replays = 0_u64;
    let mut canonical_signature_checks = 0_u64;

    while started.elapsed() < REQUIRED_SOAK_DURATION {
        if connection.is_none() {
            connection = Some(connect_authenticated(&endpoint, address).await);
            connection_generations = connection_generations.checked_add(1).unwrap();
            sequence = 1;
        }
        read_and_assert_complete_death_views(connection.as_ref().unwrap(), &mut sequence).await;
        query_journeys = query_journeys.checked_add(1).unwrap();
        death_view_queries = death_view_queries
            .checked_add(death_measurement::DEATH_VIEW_QUERIES_PER_SOAK_JOURNEY)
            .unwrap();

        if query_journeys.is_multiple_of(death_measurement::DEATH_SOAK_RECONNECT_INTERVAL_JOURNEYS)
        {
            exact_terminal_replay(&persistence, &candidate, &death).await;
            exact_replays = exact_replays.checked_add(1).unwrap();
            assert_eq!(
                canonical_signature_bytes(&persistence).await,
                expected_signature
            );
            canonical_signature_checks = canonical_signature_checks.checked_add(1).unwrap();

            connection
                .take()
                .unwrap()
                .close(0_u32.into(), b"death soak reconnect");
            endpoint.wait_idle().await;
        }

        let elapsed = started.elapsed();
        if elapsed >= next_memory_sample {
            samples.push(death_measurement::ResidentMemorySampleV1 {
                elapsed_ms: elapsed_millis(started),
                resident_bytes: memory.resident_bytes().unwrap(),
            });
            while next_memory_sample <= elapsed {
                next_memory_sample = next_memory_sample.saturating_add(MEMORY_SAMPLE_INTERVAL);
            }
        }
        tokio::time::sleep(JOURNEY_PACING).await;
    }

    let measured_duration_ms = elapsed_millis(started);
    if samples
        .last()
        .is_none_or(|sample| sample.elapsed_ms < measured_duration_ms)
    {
        samples.push(death_measurement::ResidentMemorySampleV1 {
            elapsed_ms: measured_duration_ms,
            resident_bytes: memory.resident_bytes().unwrap(),
        });
    }
    if let Some(connection) = connection {
        connection.close(0_u32.into(), b"death soak complete");
    }
    endpoint.wait_idle().await;
    shutdown_send.send(()).unwrap();
    let server_report = server_task.await.unwrap().unwrap();

    let canonical_signature_unchanged =
        canonical_signature_bytes(&persistence).await == expected_signature;
    let final_database_residue =
        death_measurement::PostgresResidueSnapshotV1::capture(&persistence)
            .await
            .unwrap();
    durable_death_fixture::assert_committed_graph(&persistence).await;
    let evidence = death_measurement::DeathMemorySoakEvidenceV1::compile(
        death_measurement::DeathMemorySoakInputV1 {
            build_id: server_app::CORE_IDENTITY_BUILD_ID.to_owned(),
            death_view_records_blake3: hashes.records_blake3.clone(),
            death_view_assets_blake3: hashes.assets_blake3.clone(),
            death_view_localization_blake3: hashes.localization_blake3.clone(),
            measured_duration_ms,
            query_journeys,
            death_view_queries,
            connection_generations,
            exact_replays,
            canonical_signature_checks,
            resident_memory_samples: samples,
            canonical_signature_unchanged,
            final_database_residue,
            runtime_residue: death_measurement::DeathRuntimeResidueV1 {
                accepted_connections: server_report.accepted_connections,
                rejected_connections: server_report.rejected_connections,
                combat_sessions_admitted: server_report.combat_sessions_admitted,
                completed_connection_tasks: server_report.completed_connection_tasks,
                failed_connection_tasks: server_report.failed_connection_tasks,
                remaining_connection_tasks: server_report.remaining_connection_tasks,
                remaining_open_connections: server_report.remaining_open_connections,
                zero_residue: server_report.zero_residue,
                persistence_enabled: server_report.persistence_enabled,
            },
        },
    )
    .unwrap();
    assert!(evidence.accepted, "death memory soak failed: {evidence:#?}");
    println!(
        "GB_M03_06E_DEATH_MEMORY_SOAK_EVIDENCE={}",
        serde_json::to_string(&evidence).unwrap()
    );
    persistence.close().await;
}
