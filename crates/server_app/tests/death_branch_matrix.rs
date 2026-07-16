//! Measured `GB-M03-06E` death/Echo eligibility and availability matrix.
//!
//! Authorities:
//! - `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-001`, `DTH-021`, `ECH-001`,
//!   `ECH-002`, and `TECH-022`;
//! - `Gravebound_Content_Production_Spec_v1.md`: `CONT-ECHO-009` and `CONT-HUB-002`;
//! - `Gravebound_Development_Roadmap_v1.md`: `GB-M03-06`, `GB-M03-13`, and the M03
//!   atomic-death gate.
//!
//! Every target death enters through the production trace, builder, terminal writer, authenticated
//! real-QUIC read route, client summary reducer, exact replay, canonical signature, and teardown
//! seams. The existing-Available prestate is created by a real predecessor death; no Echo row or
//! transition is manufactured directly. This matrix measures model readiness, not the separate
//! native Bevy render/focus frame required for final `DTH-021` presentation evidence.

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use client_bevy::{
    DeathSummaryAction, DeathUiActivity, DeathUiSnapshot, DeathViewClientModel, TerminalDeathPhase,
};
use persistence::{PersistenceConfig, PostgresPersistence, WIPEABLE_CORE_NAMESPACE};
use protocol::{
    AuthTicket, ClientHello, Compression, DEATH_VIEW_SCHEMA_VERSION, DeathEchoOutcomeV1,
    DeathViewFrameV1, DeathViewRequestV1, DeathViewResultV1, HandshakeResponse, ManifestHash,
    Platform, WireText,
};
use rustls::pki_types::CertificateDer;
use server_app::{
    BoundCoreIdentityServer, CoreIdentityServerConfig, CoreIdentityServerReport,
    DurableDeathExecutionService, PostgresAccountRepository, SubmitResult, TerminalArbiter,
    TerminalCandidate, durable_death_terminal_candidate,
};
use tokio::sync::oneshot;

#[path = "support/death_measurement.rs"]
mod death_measurement;
#[path = "support/durable_death.rs"]
mod durable_death_fixture;

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

fn death_view_frame(
    sequence: u32,
    request: DeathViewRequestV1,
    revision: protocol::DeathViewContentRevisionV1,
) -> DeathViewFrameV1 {
    DeathViewFrameV1 {
        schema_version: DEATH_VIEW_SCHEMA_VERSION,
        sequence,
        content_revision: revision,
        request,
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

fn micros(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX)
}

async fn canonical_signature_bytes(
    persistence: &PostgresPersistence,
    character_id: [u8; 16],
) -> Vec<u8> {
    persistence
        .load_core_death_terminal_signature_v1(durable_death_fixture::ACCOUNT_ID, character_id)
        .await
        .unwrap()
        .expect("committed death-terminal signature")
        .canonical_bytes()
        .unwrap()
}

async fn commit_prepared(
    persistence: &PostgresPersistence,
    death: &server_app::PreparedDurableDeathCommit,
) -> (
    TerminalCandidate,
    persistence::StoredCommittedDeathResultV1,
    Duration,
) {
    let candidate = durable_death_terminal_candidate(death).unwrap();
    let mut arbiter = TerminalArbiter::new(candidate.binding());
    assert!(matches!(
        arbiter.submit(candidate.clone()),
        SubmitResult::Accepted { .. }
    ));
    let prepared = arbiter
        .prepare(death.request().plan.event.death_tick)
        .unwrap();
    let started = Instant::now();
    let committed = DurableDeathExecutionService::new(persistence.clone())
        .execute_prepared(&mut arbiter, &prepared, death)
        .await
        .unwrap();
    let elapsed = started.elapsed();
    assert!(!committed.transaction.is_replay());
    (candidate, committed.transaction.result().clone(), elapsed)
}

async fn exact_replay(
    persistence: &PostgresPersistence,
    candidate: &TerminalCandidate,
    death: &server_app::PreparedDurableDeathCommit,
    expected: &persistence::StoredCommittedDeathResultV1,
) -> Duration {
    let mut arbiter = TerminalArbiter::new(candidate.binding());
    assert!(matches!(
        arbiter.submit(candidate.clone()),
        SubmitResult::Accepted { .. }
    ));
    let prepared = arbiter
        .prepare(death.request().plan.event.death_tick)
        .unwrap();
    let started = Instant::now();
    let replay = DurableDeathExecutionService::new(persistence.clone())
        .execute_prepared(&mut arbiter, &prepared, death)
        .await
        .unwrap();
    let elapsed = started.elapsed();
    assert!(replay.transaction.is_replay());
    assert_eq!(replay.transaction.result(), expected);
    elapsed
}

fn expected_wire_outcome(
    expected: death_measurement::DeathBranchEchoOutcomeV1,
) -> DeathEchoOutcomeV1 {
    match expected {
        death_measurement::DeathBranchEchoOutcomeV1::NotEligible => DeathEchoOutcomeV1::NotEligible,
        death_measurement::DeathBranchEchoOutcomeV1::Dormant => DeathEchoOutcomeV1::Dormant,
        death_measurement::DeathBranchEchoOutcomeV1::Available => DeathEchoOutcomeV1::Available,
    }
}

async fn read_target_death(
    connection: &quinn::Connection,
    presentation: &sim_content::CoreDevelopmentDeathView,
    scenario: &durable_death_fixture::DurableDeathScenarioV1,
    expected: death_measurement::DeathBranchEchoOutcomeV1,
    committed: Instant,
) -> (Duration, Duration, Duration) {
    let mut model = DeathViewClientModel::new(presentation.clone()).unwrap();
    let latest_request = model
        .begin_committed_death_lookup(scenario.identity.character_id)
        .unwrap();
    let latest_started = Instant::now();
    let (_, latest) = bot_client::perform_death_view(connection, latest_request)
        .await
        .unwrap();
    let latest_round_trip = latest_started.elapsed();
    let latest_outcome = model.handle_result(&latest).unwrap();
    let summary_request = latest_outcome.follow_up.unwrap();
    let summary_started = Instant::now();
    let (_, summary) = bot_client::perform_death_view(connection, summary_request)
        .await
        .unwrap();
    let summary_round_trip = summary_started.elapsed();
    assert!(matches!(
        summary,
        DeathViewResultV1::Summary { ref summary, .. }
            if summary.death_id == scenario.identity.death_id
                && summary.echo_outcome == expected_wire_outcome(expected)
    ));
    model.handle_result(&summary).unwrap();
    assert_eq!(model.terminal().phase(), TerminalDeathPhase::SummaryReady);
    assert!(
        model
            .terminal()
            .action_state(DeathSummaryAction::InspectTrace)
            .is_enabled()
    );
    let snapshot = DeathUiSnapshot::terminal(&model).unwrap();
    assert!(snapshot.summary.is_some());
    assert_eq!(snapshot.activity, DeathUiActivity::Idle);
    let post_commit_to_client_model_ready = committed.elapsed();
    assert!(post_commit_to_client_model_ready < Duration::from_secs(2));

    let revision = durable_death_fixture::death_view_revision();
    let (_, memorial) = bot_client::perform_death_view(
        connection,
        death_view_frame(
            3,
            DeathViewRequestV1::MemorialPage {
                after: None,
                limit: 8,
            },
            revision.clone(),
        ),
    )
    .await
    .unwrap();
    assert!(matches!(
        memorial,
        DeathViewResultV1::MemorialPage { ref entries, .. }
            if entries.iter().any(|entry| entry.cursor.death_id == scenario.identity.death_id)
    ));
    let (_, trace) = bot_client::perform_death_view(
        connection,
        death_view_frame(
            4,
            DeathViewRequestV1::TracePage {
                death_id: scenario.identity.death_id,
                start_ordinal: 0,
                limit: 8,
            },
            revision,
        ),
    )
    .await
    .unwrap();
    assert!(matches!(
        trace,
        DeathViewResultV1::TracePage { ref page, .. }
            if page.death_id == scenario.identity.death_id
                && page.entries.len() == 2
                && page.entries.last().is_some_and(|entry| entry.lethal)
    ));
    (
        latest_round_trip,
        summary_round_trip,
        post_commit_to_client_model_ready,
    )
}

async fn assert_reconnected_latest(
    connection: &quinn::Connection,
    scenario: &durable_death_fixture::DurableDeathScenarioV1,
) {
    let (_, latest) = bot_client::perform_death_view(
        connection,
        death_view_frame(
            1,
            DeathViewRequestV1::LatestCommitted,
            durable_death_fixture::death_view_revision(),
        ),
    )
    .await
    .unwrap();
    assert!(matches!(
        latest,
        DeathViewResultV1::Latest {
            death: Some(ref latest),
            ..
        } if latest.death_id == scenario.identity.death_id
    ));
}

async fn branch_counts(
    persistence: &PostgresPersistence,
    scenario: &durable_death_fixture::DurableDeathScenarioV1,
) -> (u32, u32, u32, u32, u32) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let row: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT \
         (SELECT count(*) FROM echo_records WHERE namespace_id=$1 AND death_id=$2), \
         (SELECT count(*) FROM echo_state_transitions AS transition \
          JOIN echo_records AS echo USING (namespace_id,echo_id) \
          WHERE transition.namespace_id=$1 AND echo.death_id=$2), \
         (SELECT count(*) FROM death_outbox_events WHERE namespace_id=$1 AND death_id=$2), \
         (SELECT count(*) FROM echo_records WHERE namespace_id=$1 AND account_id=$3 AND state=1), \
         (SELECT count(*) FROM echo_records WHERE namespace_id=$1 AND account_id=$3 AND state=0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(scenario.identity.death_id.as_slice())
    .bind(durable_death_fixture::ACCOUNT_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    (
        u32::try_from(row.0).unwrap(),
        u32::try_from(row.1).unwrap(),
        u32::try_from(row.2).unwrap(),
        u32::try_from(row.3).unwrap(),
        u32::try_from(row.4).unwrap(),
    )
}

fn assert_shutdown(report: CoreIdentityServerReport) -> death_measurement::DeathRuntimeResidueV1 {
    assert_eq!(report.accepted_connections, 2);
    assert_eq!(report.rejected_connections, 0);
    assert_eq!(report.combat_sessions_admitted, 0);
    assert_eq!(report.completed_connection_tasks, 2);
    assert_eq!(report.failed_connection_tasks, 0);
    assert_eq!(report.remaining_connection_tasks, 0);
    assert_eq!(report.remaining_open_connections, 0);
    assert!(report.zero_residue);
    assert!(report.persistence_enabled);
    death_measurement::DeathRuntimeResidueV1 {
        accepted_connections: report.accepted_connections,
        rejected_connections: report.rejected_connections,
        combat_sessions_admitted: report.combat_sessions_admitted,
        completed_connection_tasks: report.completed_connection_tasks,
        failed_connection_tasks: report.failed_connection_tasks,
        remaining_connection_tasks: report.remaining_connection_tasks,
        remaining_open_connections: report.remaining_open_connections,
        zero_residue: report.zero_residue,
        persistence_enabled: report.persistence_enabled,
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one target branch preserves commit, native projection, reconnect, replay, and cleanup"
)]
async fn run_branch(
    persistence: &PostgresPersistence,
    presentation: &sim_content::CoreDevelopmentDeathView,
    branch: death_measurement::DeathBranchKindV1,
    scenario: &durable_death_fixture::DurableDeathScenarioV1,
    expected: death_measurement::DeathBranchEchoOutcomeV1,
) -> death_measurement::DeathBranchSampleV1 {
    durable_death_fixture::seed_danger_root_for(persistence, scenario).await;
    let death = durable_death_fixture::prepare_death_for(persistence.clone(), scenario).await;
    assert_eq!(
        death.request().plan.echo.is_some(),
        expected != death_measurement::DeathBranchEchoOutcomeV1::NotEligible
    );

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
    let first_connection = connect_authenticated(&endpoint, address).await;

    let (candidate, expected_result, terminal_commit) = commit_prepared(persistence, &death).await;
    let committed = Instant::now();
    let (latest_round_trip, summary_round_trip, post_commit_to_client_model_ready) =
        read_target_death(
            &first_connection,
            presentation,
            scenario,
            expected,
            committed,
        )
        .await;

    let signature_started = Instant::now();
    let signature = canonical_signature_bytes(persistence, scenario.identity.character_id).await;
    let canonical_signature_query = signature_started.elapsed();
    first_connection.close(0_u32.into(), b"branch reconnect");
    endpoint.wait_idle().await;
    let second_connection = connect_authenticated(&endpoint, address).await;
    assert_reconnected_latest(&second_connection, scenario).await;
    assert_eq!(
        canonical_signature_bytes(persistence, scenario.identity.character_id).await,
        signature
    );

    let exact_replay = exact_replay(persistence, &candidate, &death, &expected_result).await;
    let canonical_signature_unchanged =
        canonical_signature_bytes(persistence, scenario.identity.character_id).await == signature;
    let (
        target_echo_records,
        target_echo_transitions,
        target_outbox_events,
        account_available_echoes,
        account_dormant_echoes,
    ) = branch_counts(persistence, scenario).await;

    second_connection.close(0_u32.into(), b"branch complete");
    endpoint.wait_idle().await;
    shutdown_send.send(()).unwrap();
    let runtime_residue = assert_shutdown(server_task.await.unwrap().unwrap());
    let database_residue = death_measurement::PostgresResidueSnapshotV1::capture(persistence)
        .await
        .unwrap();
    assert!(database_residue.is_zero());

    death_measurement::DeathBranchSampleV1 {
        branch,
        echo_outcome: expected,
        terminal_commit_micros: micros(terminal_commit),
        exact_replay_micros: micros(exact_replay),
        canonical_signature_query_micros: micros(canonical_signature_query),
        latest_round_trip_micros: micros(latest_round_trip),
        summary_round_trip_micros: micros(summary_round_trip),
        post_commit_to_client_model_ready_micros: micros(post_commit_to_client_model_ready),
        target_echo_records,
        target_echo_transitions,
        target_outbox_events,
        account_available_echoes,
        account_dormant_echoes,
        canonical_signature_unchanged,
        database_residue,
        runtime_residue,
    }
}

fn ineligible_scenario(
    branch: death_measurement::DeathBranchKindV1,
) -> durable_death_fixture::DurableDeathScenarioV1 {
    let mut scenario = durable_death_fixture::DurableDeathScenarioV1::primary_eligible();
    scenario.echo_availability = durable_death_fixture::FixtureEchoAvailabilityV1::None;
    match branch {
        death_measurement::DeathBranchKindV1::LevelBelowTen => scenario.level = 9,
        death_measurement::DeathBranchKindV1::CombatBelowThreshold => {
            scenario.permadeath_combat_ticks = 17_999;
        }
        death_measurement::DeathBranchKindV1::MissingQualifyingDeed => {
            scenario.boss_deed = false;
        }
        death_measurement::DeathBranchKindV1::VerifiedServerIncident => {
            scenario.provenance = server_app::DeathProvenance::VerifiedServerIncident;
        }
        death_measurement::DeathBranchKindV1::EligibleSelfPromotion
        | death_measurement::DeathBranchKindV1::EligibleExistingAvailable => {
            panic!("eligible branch requested from ineligible scenario builder")
        }
    }
    scenario
}

/// GDD `ECH-001`/`DTH-021`, Content `CONT-ECHO-009`, and Roadmap `GB-M03-06`/`13`
/// require the exact level, combat-time, deed, provenance, self-promotion, and existing-Available
/// outcomes to pass through one durable transaction and player-visible stored projection.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn durable_death_echo_branch_matrix_over_real_quic_and_postgresql() {
    let persistence = disposable_database().await;
    let presentation = sim_content::load_core_development_death_view(&content_root()).unwrap();
    let mut samples = Vec::with_capacity(6);

    for branch in [
        death_measurement::DeathBranchKindV1::LevelBelowTen,
        death_measurement::DeathBranchKindV1::CombatBelowThreshold,
        death_measurement::DeathBranchKindV1::MissingQualifyingDeed,
        death_measurement::DeathBranchKindV1::VerifiedServerIncident,
    ] {
        persistence.reset_disposable_identity_data().await.unwrap();
        let scenario = ineligible_scenario(branch);
        samples.push(
            run_branch(
                &persistence,
                &presentation,
                branch,
                &scenario,
                death_measurement::DeathBranchEchoOutcomeV1::NotEligible,
            )
            .await,
        );
    }

    persistence.reset_disposable_identity_data().await.unwrap();
    let self_promotion = durable_death_fixture::DurableDeathScenarioV1::primary_eligible();
    samples.push(
        run_branch(
            &persistence,
            &presentation,
            death_measurement::DeathBranchKindV1::EligibleSelfPromotion,
            &self_promotion,
            death_measurement::DeathBranchEchoOutcomeV1::Available,
        )
        .await,
    );

    persistence.reset_disposable_identity_data().await.unwrap();
    durable_death_fixture::seed_danger_root_for(&persistence, &self_promotion).await;
    let predecessor =
        durable_death_fixture::prepare_death_for(persistence.clone(), &self_promotion).await;
    let (_, predecessor_result, _) = commit_prepared(&persistence, &predecessor).await;
    assert_eq!(
        predecessor_result.echo_outcome,
        persistence::DurableEchoOutcomeV1::Available
    );
    let existing_available =
        durable_death_fixture::DurableDeathScenarioV1::secondary_with_existing_available(
            self_promotion.identity.echo_id,
        );
    samples.push(
        run_branch(
            &persistence,
            &presentation,
            death_measurement::DeathBranchKindV1::EligibleExistingAvailable,
            &existing_available,
            death_measurement::DeathBranchEchoOutcomeV1::Dormant,
        )
        .await,
    );

    let hashes = presentation.hashes();
    let evidence = death_measurement::DeathBranchMatrixEvidenceV1::compile(
        samples,
        server_app::CORE_IDENTITY_BUILD_ID,
        hashes.records_blake3.clone(),
        hashes.assets_blake3.clone(),
        hashes.localization_blake3.clone(),
    )
    .unwrap();
    assert!(
        evidence.accepted,
        "death branch matrix failed: {evidence:#?}"
    );
    println!(
        "GB_M03_06E_DEATH_BRANCH_MATRIX_EVIDENCE={}",
        serde_json::to_string(&evidence).unwrap()
    );
    persistence.close().await;
}
