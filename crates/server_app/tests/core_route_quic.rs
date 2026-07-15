use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

#[path = "support/durable_death.rs"]
mod durable_death_fixture;

use persistence::{
    CaldusExtractionCommit, CaldusExtractionRequest, PersistenceConfig, PostgresPersistence,
    StoredExtractionAuthority, StoredWorldFlowRevisionV1, WIPEABLE_CORE_NAMESPACE,
};
use protocol::{
    AuthTicket, CharacterLocation, ClientHello, Compression, DEATH_VIEW_SCHEMA_VERSION,
    DeathViewContentRevisionV1, DeathViewFrameV1, DeathViewRequestV1, DeathViewResultV1,
    HandshakeResponse, ManifestHash, Platform, ProtocolVersion, SafeArrival, WireText,
    WorldFlowContentRevisionV1, WorldFlowFrame, WorldFlowRequest, WorldFlowResult,
    WorldTransferCommand, WorldTransferMutation, WorldTransferPayload, WorldTransferResultCode,
};
use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::pki_types::PrivatePkcs8KeyDer;
use server_app::{
    AccountId, AdmissionState, AuthenticatedAccount, AuthenticatedNamespace,
    AuthenticationDecision, CaldusVictoryOwnerCommand, CharacterIdGenerator, CoreBargainAuthority,
    CoreOathSelectionAuthority, CoreSafeInventoryAuthority, CoreTerminalCoordinator,
    CoreTerminalEvaluation, CoreTerminalProducer, CoreTerminalTickSeal, DeathViewService,
    DisabledDeathViewRepository, DisabledProgressionQueryRepository,
    DisposableCoreJourneyWorldFlow, DurableDeathExecutionService, HandshakePolicy, IdentityClock,
    IdentityService, InMemoryAccountRepository, NoopIdentityEventSink,
    PostgresCaldusHallTransferCoordinator, PostgresCaldusVictoryCoordinator,
    PostgresDangerEntryAshWalletProviderV3, PostgresDangerEntryInventoryProviderV3,
    PostgresDangerEntryLifeMetricsProviderV3, PostgresDangerEntryOathBargainProviderV3,
    PostgresDeathViewRepository, PostgresDormantWorldFlowCoordinator,
    PostgresProgressionAwardService, PostgresProgressionRestoreProvider, PostgresRewardService,
    PreparedTerminal, ProgressionQueryService, SecretRewardEpoch, SubmitResult, TerminalArbiter,
    TerminalCandidate, WorldFlowIdGenerator, durable_death_terminal_candidate,
    recover_committed_death_arbiter, serve_core_reliable, serve_handshake,
};
use sim_core::{
    CoreBossParticipant, CoreBossParticipantLock, CoreCaldusAntiCheatState,
    CoreCaldusDefeatPresence, CoreCaldusEligibilityEvidence, CoreCaldusRecallState,
    CoreCaldusSessionState, CoreCaldusVictoryIdentities, EntityId,
};

const ACCOUNT_ID: [u8; 16] = [211; 16];
const CHARACTER_ID: [u8; 16] = [212; 16];
const TRANSFER_ID: [u8; 16] = [213; 16];
const LINEAGE_ID: [u8; 16] = [214; 16];
const RESTORE_ID: [u8; 16] = [215; 16];
const EXTRACTION_RECEIPT_ID: [u8; 16] = [217; 16];
const HALL_ID: &str = "hub.lantern_halls_01";
const WORLD_ID: &str = "world.core_microrealm_01";

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

async fn seed_character(persistence: &PostgresPersistence) {
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
        "INSERT INTO characters (namespace_id,account_id,character_id,roster_ordinal,class_id,
         level,oath_id,life_state,security_state,character_state_version)
         VALUES ($1,$2,$3,1,'class.grave_arbalist',1,NULL,0,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE accounts SET selected_character_id=$1
         WHERE namespace_id=$2 AND account_id=$3",
    )
    .bind(CHARACTER_ID.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_world_locations (namespace_id,account_id,character_id,
         character_version,location_kind,location_content_id,safe_arrival_kind)
         VALUES ($1,$2,$3,1,0,NULL,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_progression (namespace_id,account_id,character_id,total_xp,level,
         current_health,progression_version) VALUES ($1,$2,$3,0,1,120,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_life_metrics \
         (namespace_id,account_id,character_id,lifetime_ticks,permadeath_combat_ticks, \
          life_metrics_version) VALUES ($1,$2,$3,0,0,1) \
          ON CONFLICT (namespace_id,account_id,character_id) DO NOTHING",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_inventories
         (namespace_id,account_id,character_id,inventory_version) VALUES ($1,$2,$3,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_oath_bargain_state
         (namespace_id,account_id,character_id,earned_bargain_slots,oath_bargain_version)
         VALUES ($1,$2,$3,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ash_wallets (namespace_id,account_id,balance,wallet_version)
         VALUES ($1,$2,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

fn revision() -> WorldFlowContentRevisionV1 {
    let world = sim_content::load_core_development_world_flow(&content_root()).unwrap();
    WorldFlowContentRevisionV1 {
        records_blake3: ManifestHash::new(world.hashes().records_blake3.clone()).unwrap(),
        assets_blake3: ManifestHash::new(world.hashes().assets_blake3.clone()).unwrap(),
        localization_blake3: ManifestHash::new(world.hashes().localization_blake3.clone()).unwrap(),
    }
}

fn death_view_frame(sequence: u32, request: DeathViewRequestV1) -> DeathViewFrameV1 {
    DeathViewFrameV1 {
        schema_version: DEATH_VIEW_SCHEMA_VERSION,
        sequence,
        content_revision: durable_death_fixture::death_view_revision(),
        request,
    }
}

fn disposable_world_flow(
    persistence: PostgresPersistence,
) -> impl server_app::CoreWorldFlowAuthority {
    let progression_content =
        sim_content::load_core_development_progression(&content_root()).unwrap();
    let route = PostgresDormantWorldFlowCoordinator::new(
        persistence.clone(),
        FixedAuthority,
        FixedAuthority,
        revision(),
        PostgresProgressionRestoreProvider::new(&progression_content).unwrap(),
        PostgresDangerEntryInventoryProviderV3,
        PostgresDangerEntryOathBargainProviderV3,
        PostgresDangerEntryLifeMetricsProviderV3,
        PostgresDangerEntryAshWalletProviderV3,
    );
    let extraction =
        PostgresCaldusHallTransferCoordinator::new(persistence, FixedAuthority, revision());
    DisposableCoreJourneyWorldFlow::new(route, extraction)
}

fn disabled_progression() -> ProgressionQueryService<DisabledProgressionQueryRepository> {
    let content = sim_content::load_core_development_progression(&content_root()).unwrap();
    ProgressionQueryService::new(DisabledProgressionQueryRepository, &content).unwrap()
}

#[derive(Debug, Clone, Copy)]
struct FixedAuthority;

impl IdentityClock for FixedAuthority {
    fn unix_millis(&self) -> u64 {
        10_000
    }
}

impl CharacterIdGenerator for FixedAuthority {
    fn next_id(&self) -> [u8; 16] {
        [221; 16]
    }
}

impl WorldFlowIdGenerator for FixedAuthority {
    fn next_transfer_id(&self) -> [u8; 16] {
        TRANSFER_ID
    }

    fn next_lineage_id(&self) -> [u8; 16] {
        LINEAGE_ID
    }

    fn next_restore_point_id(&self) -> [u8; 16] {
        RESTORE_ID
    }
}

fn route_frame(
    sequence: u32,
    mutation_id: [u8; 16],
    version: u64,
    command: WorldTransferCommand,
) -> WorldFlowFrame {
    let payload = WorldTransferPayload {
        content_revision: revision(),
        command,
    };
    WorldFlowFrame {
        sequence,
        request: WorldFlowRequest::Transfer(WorldTransferMutation {
            mutation_id,
            character_id: CHARACTER_ID,
            expected_character_version: version,
            issued_at_unix_millis: 9_000,
            payload_hash: payload.canonical_hash(),
            payload,
        }),
    }
}

async fn commit_caldus_fixture(persistence: &PostgresPersistence) -> ([u8; 16], [u8; 16]) {
    let participant = CoreBossParticipant {
        entity_id: EntityId::new(1).unwrap(),
        party_slot: 0,
    };
    let lock = CoreBossParticipantLock {
        attempt_ordinal: 1,
        participants: vec![participant],
        maximum_health: 7_200,
    };
    let progression_content =
        sim_content::load_core_development_progression(&content_root()).unwrap();
    let oath_bargain = sim_content::load_core_development_oaths_bargains(&content_root()).unwrap();
    let rewards = PostgresRewardService::load(
        persistence.clone(),
        &content_root(),
        SecretRewardEpoch::new("core-route-caldus-v1", [0x5a; 32]).unwrap(),
    )
    .unwrap();
    let progression = PostgresProgressionAwardService::new(
        persistence.clone(),
        &progression_content,
        &oath_bargain,
    )
    .unwrap();
    let victory = PostgresCaldusVictoryCoordinator::new(persistence.clone(), rewards, progression);
    victory
        .commit(
            LINEAGE_ID,
            &lock,
            5_400,
            9_000,
            &[CaldusVictoryOwnerCommand {
                participant,
                authenticated: AuthenticatedAccount {
                    account_id: AccountId::new(ACCOUNT_ID).unwrap(),
                    namespace: AuthenticatedNamespace::WipeableTest,
                },
                character_id: CHARACTER_ID,
                expected_progression_version: 1,
                progression_content_revision: ManifestHash::new(
                    progression_content.hashes().records_blake3.clone(),
                )
                .unwrap(),
                eligibility: CoreCaldusEligibilityEvidence {
                    participant,
                    presence_ticks: 5_400,
                    direct_damage: 100,
                    effective_healing_to_others: 0,
                    damage_prevented_on_others: 0,
                    objective_credits: 0,
                    longest_inactivity_ticks: 0,
                    defeat_presence: CoreCaldusDefeatPresence::AliveAndPresent,
                    recall_state: CoreCaldusRecallState::Stayed,
                    session_state: CoreCaldusSessionState::Valid,
                    anti_cheat_state: CoreCaldusAntiCheatState::Valid,
                },
            }],
        )
        .await
        .unwrap();
    let identities = CoreCaldusVictoryIdentities::derive(LINEAGE_ID, &lock).unwrap();
    let extraction = identities.extraction_for(participant).unwrap();
    let revision = revision();
    persistence
        .request_caldus_extraction(&CaldusExtractionRequest {
            account_id: ACCOUNT_ID,
            character_id: CHARACTER_ID,
            extraction_request_id: extraction.request_id.bytes(),
            encounter_id: identities.encounter_id.bytes(),
            instance_lineage_id: LINEAGE_ID,
            entry_restore_point_id: RESTORE_ID,
            exit_instance_id: identities.exit_instance_id.bytes(),
            attempt_ordinal: 1,
            party_slot: 0,
            participant_entity_id: 1,
            expected_character_version: 3,
            content_revision: StoredWorldFlowRevisionV1 {
                records_blake3: revision.records_blake3.as_str().to_owned(),
                assets_blake3: revision.assets_blake3.as_str().to_owned(),
                localization_blake3: revision.localization_blake3.as_str().to_owned(),
            },
        })
        .await
        .unwrap();
    persistence
        .commit_caldus_extraction(CaldusExtractionCommit {
            extraction_request_id: extraction.request_id.bytes(),
            extraction_receipt_id: EXTRACTION_RECEIPT_ID,
            authority: StoredExtractionAuthority::WipeableTestEvidence,
        })
        .await
        .unwrap();
    (extraction.request_id.bytes(), EXTRACTION_RECEIPT_ID)
}

fn endpoints() -> (quinn::Endpoint, quinn::Endpoint, std::net::SocketAddr) {
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
    let certificate = cert.der().clone();
    let private_key = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
    let server_config =
        quinn::ServerConfig::with_single_cert(vec![certificate.clone()], private_key.into())
            .unwrap();
    let server = quinn::Endpoint::server(server_config, "127.0.0.1:0".parse().unwrap()).unwrap();
    let address = server.local_addr().unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate).unwrap();
    let config = quinn::ClientConfig::with_root_certificates(Arc::new(roots)).unwrap();
    let mut client = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    client.set_default_client_config(config);
    (server, client, address)
}

fn policy() -> HandshakePolicy {
    HandshakePolicy {
        required_protocol: ProtocolVersion::current(),
        required_client_build: WireText::new("m03-core-route-journey-1").unwrap(),
        required_manifest_hash: ManifestHash::new("a".repeat(64)).unwrap(),
        content_bundle_version: WireText::new("core-dev").unwrap(),
        region_id: WireText::new("loopback").unwrap(),
        feature_flags: vec![WireText::new("core_world_flow_integration").unwrap()],
        admission: AdmissionState::Available,
    }
}

fn death_view_policy() -> HandshakePolicy {
    let mut policy = policy();
    policy
        .feature_flags
        .push(WireText::new(protocol::CORE_DEATH_VIEW_FEATURE_FLAG).unwrap());
    policy
}

fn hello() -> ClientHello {
    ClientHello {
        protocol_major: ProtocolVersion::current().major,
        protocol_minor: ProtocolVersion::current().minor,
        client_build_id: WireText::new("m03-core-route-journey-1").unwrap(),
        platform: Platform::WindowsNative,
        supported_compression: vec![Compression::None],
        content_manifest_hash: ManifestHash::new("a".repeat(64)).unwrap(),
        auth_ticket: AuthTicket::new(b"disposable-core-route".to_vec()).unwrap(),
        locale: WireText::new("en-US").unwrap(),
    }
}

fn assert_accepted(result: &WorldFlowResult, version: u64, location: &str) {
    assert!(matches!(
        result,
        WorldFlowResult::Transfer {
            accepted: true,
            code: WorldTransferResultCode::Accepted,
            snapshot: Some(snapshot),
            ..
        } if snapshot.character_version == version && match &snapshot.location {
            CharacterLocation::Safe { location_id, arrival: SafeArrival::HallDefault }
            | CharacterLocation::Danger { location_id, .. } => location_id.as_str() == location,
            CharacterLocation::Safe { .. } | CharacterLocation::CharacterSelect { .. } => false,
        }
    ));
}

#[allow(
    clippy::too_many_lines,
    reason = "the first authenticated QUIC session and deliberate response loss stay contiguous"
)]
async fn run_lost_death_summary_session(persistence: &PostgresPersistence) -> DeathViewResultV1 {
    let identity = IdentityService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        FixedAuthority,
        NoopIdentityEventSink,
        ManifestHash::new("a".repeat(64)).unwrap(),
    );
    let world_flow = disposable_world_flow(persistence.clone());
    let progression = disabled_progression();
    let death_views = DeathViewService::new(
        PostgresDeathViewRepository::new(
            persistence.clone(),
            durable_death_fixture::death_view_revision(),
        ),
        durable_death_fixture::death_view_revision(),
    );
    let oath = CoreOathSelectionAuthority::<FixedAuthority>::disabled();
    let bargain = CoreBargainAuthority::<FixedAuthority>::disabled();
    let safe_inventory = CoreSafeInventoryAuthority::disabled();
    let authenticated = durable_death_fixture::authenticated_account();
    let (server_endpoint, client_endpoint, address) = endpoints();
    let connecting = client_endpoint.connect(address, "localhost").unwrap();
    let incoming = server_endpoint.accept().await.unwrap();
    let (client, server) = tokio::join!(connecting, incoming);
    let client = client.unwrap();
    let server = server.unwrap();

    let server_session = async {
        serve_handshake(
            &server,
            &death_view_policy(),
            AuthenticationDecision::Accepted,
            WireText::new("committed-death-loss-session").unwrap(),
        )
        .await
        .unwrap();
        serve_core_reliable(
            &server,
            &identity,
            &world_flow,
            &progression,
            &death_views,
            &oath,
            &bargain,
            &safe_inventory,
            authenticated,
            1,
            20_000,
        )
        .await
        .unwrap();
        // The client deliberately sends STOP_SENDING for this response. The read has already
        // resolved from committed PostgreSQL state, so either a completed write or transport loss
        // is acceptable and neither can change domain state.
        let _lost = serve_core_reliable(
            &server,
            &identity,
            &world_flow,
            &progression,
            &death_views,
            &oath,
            &bargain,
            &safe_inventory,
            authenticated,
            2,
            20_000,
        )
        .await;
    };
    let client_session = async {
        assert!(matches!(
            bot_client::perform_handshake(&client, hello())
                .await
                .unwrap(),
            HandshakeResponse::Accepted(server)
                if server.feature_flags.iter().any(
                    |flag| flag.as_str() == protocol::CORE_DEATH_VIEW_FEATURE_FLAG
                )
        ));
        let (_, latest) = bot_client::perform_death_view(
            &client,
            death_view_frame(1, DeathViewRequestV1::LatestCommitted),
        )
        .await
        .unwrap();
        bot_client::submit_death_view_without_response(
            &client,
            death_view_frame(
                2,
                DeathViewRequestV1::Summary {
                    death_id: durable_death_fixture::DEATH_ID,
                    lost_start_ordinal: 0,
                    lost_limit: 8,
                },
            ),
        )
        .await
        .unwrap();
        latest
    };
    let ((), latest) = tokio::join!(server_session, client_session);
    drop(client);
    client_endpoint.wait_idle().await;
    server_endpoint.close(0_u32.into(), b"lost death response");
    server_endpoint.wait_idle().await;
    latest
}

#[allow(
    clippy::too_many_lines,
    reason = "the restarted authenticated QUIC projection sequence stays contiguous for audit"
)]
async fn run_restarted_death_read_session(
    persistence: &PostgresPersistence,
) -> (
    DeathViewResultV1,
    DeathViewResultV1,
    DeathViewResultV1,
    DeathViewResultV1,
) {
    let identity = IdentityService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        FixedAuthority,
        NoopIdentityEventSink,
        ManifestHash::new("a".repeat(64)).unwrap(),
    );
    let world_flow = disposable_world_flow(persistence.clone());
    let progression = disabled_progression();
    let death_views = DeathViewService::new(
        PostgresDeathViewRepository::new(
            persistence.clone(),
            durable_death_fixture::death_view_revision(),
        ),
        durable_death_fixture::death_view_revision(),
    );
    let oath = CoreOathSelectionAuthority::<FixedAuthority>::disabled();
    let bargain = CoreBargainAuthority::<FixedAuthority>::disabled();
    let safe_inventory = CoreSafeInventoryAuthority::disabled();
    let authenticated = durable_death_fixture::authenticated_account();
    let (server_endpoint, client_endpoint, address) = endpoints();
    let connecting = client_endpoint.connect(address, "localhost").unwrap();
    let incoming = server_endpoint.accept().await.unwrap();
    let (client, server) = tokio::join!(connecting, incoming);
    let client = client.unwrap();
    let server = server.unwrap();

    let server_session = async {
        serve_handshake(
            &server,
            &death_view_policy(),
            AuthenticationDecision::Accepted,
            WireText::new("committed-death-restart-session").unwrap(),
        )
        .await
        .unwrap();
        for response_sequence in 1..=4 {
            serve_core_reliable(
                &server,
                &identity,
                &world_flow,
                &progression,
                &death_views,
                &oath,
                &bargain,
                &safe_inventory,
                authenticated,
                response_sequence,
                20_000,
            )
            .await
            .unwrap();
        }
    };
    let client_session = async {
        assert!(matches!(
            bot_client::perform_handshake(&client, hello())
                .await
                .unwrap(),
            HandshakeResponse::Accepted(server)
                if server.feature_flags.iter().any(
                    |flag| flag.as_str() == protocol::CORE_DEATH_VIEW_FEATURE_FLAG
                )
        ));
        let (_, latest) = bot_client::perform_death_view(
            &client,
            death_view_frame(1, DeathViewRequestV1::LatestCommitted),
        )
        .await
        .unwrap();
        let (_, summary) = bot_client::perform_death_view(
            &client,
            death_view_frame(
                2,
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
            &client,
            death_view_frame(
                3,
                DeathViewRequestV1::MemorialPage {
                    after: None,
                    limit: 8,
                },
            ),
        )
        .await
        .unwrap();
        let (_, trace) = bot_client::perform_death_view(
            &client,
            death_view_frame(
                4,
                DeathViewRequestV1::TracePage {
                    death_id: durable_death_fixture::DEATH_ID,
                    start_ordinal: 0,
                    limit: 8,
                },
            ),
        )
        .await
        .unwrap();
        (latest, summary, memorial, trace)
    };
    let ((), views) = tokio::join!(server_session, client_session);
    drop(client);
    client_endpoint.wait_idle().await;
    server_endpoint.close(0_u32.into(), b"restart death reads complete");
    server_endpoint.wait_idle().await;
    views
}

#[allow(
    clippy::too_many_lines,
    reason = "the end-to-end authority sequence stays contiguous for route-bypass auditing"
)]
async fn run_reliable_core_journey(persistence: &PostgresPersistence) -> Duration {
    let progression_content =
        sim_content::load_core_development_progression(&content_root()).unwrap();
    let route = PostgresDormantWorldFlowCoordinator::new(
        persistence.clone(),
        FixedAuthority,
        FixedAuthority,
        revision(),
        PostgresProgressionRestoreProvider::new(&progression_content).unwrap(),
        PostgresDangerEntryInventoryProviderV3,
        PostgresDangerEntryOathBargainProviderV3,
        PostgresDangerEntryLifeMetricsProviderV3,
        PostgresDangerEntryAshWalletProviderV3,
    );
    let extraction =
        PostgresCaldusHallTransferCoordinator::new(persistence.clone(), FixedAuthority, revision());
    let world_flow = DisposableCoreJourneyWorldFlow::new(route, extraction);
    let identity = IdentityService::new(
        InMemoryAccountRepository::default(),
        FixedAuthority,
        FixedAuthority,
        NoopIdentityEventSink,
        ManifestHash::new("a".repeat(64)).unwrap(),
    );
    let progression =
        ProgressionQueryService::new(DisabledProgressionQueryRepository, &progression_content)
            .unwrap();
    let route_revision = revision();
    let death_views = DeathViewService::new(
        DisabledDeathViewRepository,
        DeathViewContentRevisionV1 {
            records_blake3: route_revision.records_blake3,
            assets_blake3: route_revision.assets_blake3,
            localization_blake3: route_revision.localization_blake3,
        },
    );
    let oath = CoreOathSelectionAuthority::<FixedAuthority>::disabled();
    let bargain = CoreBargainAuthority::<FixedAuthority>::disabled();
    let safe_inventory = CoreSafeInventoryAuthority::disabled();
    let authenticated = AuthenticatedAccount {
        account_id: AccountId::new(ACCOUNT_ID).unwrap(),
        namespace: AuthenticatedNamespace::WipeableTest,
    };
    let (server_endpoint, client_endpoint, address) = endpoints();
    let login_started = Instant::now();
    let connecting = client_endpoint.connect(address, "localhost").unwrap();
    let incoming = server_endpoint.accept().await.unwrap();
    let (client, server) = tokio::join!(connecting, incoming);
    let client = client.unwrap();
    let server = server.unwrap();

    let server_journey = async {
        serve_handshake(
            &server,
            &policy(),
            AuthenticationDecision::Accepted,
            WireText::new("core-route-session").unwrap(),
        )
        .await
        .unwrap();
        for response_sequence in 1..=6 {
            serve_core_reliable(
                &server,
                &identity,
                &world_flow,
                &progression,
                &death_views,
                &oath,
                &bargain,
                &safe_inventory,
                authenticated,
                response_sequence,
                0,
            )
            .await
            .unwrap();
        }
    };
    let client_journey = async {
        assert!(matches!(
            bot_client::perform_handshake(&client, hello()).await.unwrap(),
            HandshakeResponse::Accepted(server) if server.feature_flags.iter().any(
                |flag| flag.as_str() == "core_world_flow_integration"
            )
        ));
        let hall_request = route_frame(
            1,
            [224; 16],
            1,
            WorldTransferCommand::EnterHallFromCharacterSelect,
        );
        let _discarded_committed_response =
            bot_client::perform_world_flow(&client, hall_request.clone())
                .await
                .unwrap();
        let (_, hall) = bot_client::perform_world_flow(
            &client,
            WorldFlowFrame {
                sequence: 2,
                ..hall_request
            },
        )
        .await
        .unwrap();
        assert_accepted(&hall, 2, HALL_ID);
        let login_to_control = login_started.elapsed();
        let mut mismatched_danger = route_frame(
            3,
            [225; 16],
            2,
            WorldTransferCommand::UsePortal {
                portal_id: WireText::new("station.realm_gate").unwrap(),
            },
        );
        let WorldFlowRequest::Transfer(mutation) = &mut mismatched_danger.request else {
            unreachable!();
        };
        mutation.payload.content_revision.assets_blake3 =
            ManifestHash::new("f".repeat(64)).unwrap();
        mutation.payload_hash = mutation.payload.canonical_hash();
        let (_, mismatch) = bot_client::perform_world_flow(&client, mismatched_danger)
            .await
            .unwrap();
        assert!(matches!(
            mismatch,
            WorldFlowResult::Transfer {
                accepted: false,
                code: WorldTransferResultCode::ContentMismatch,
                ..
            }
        ));
        let (_, danger) = bot_client::perform_world_flow(
            &client,
            route_frame(
                4,
                [226; 16],
                2,
                WorldTransferCommand::UsePortal {
                    portal_id: WireText::new("station.realm_gate").unwrap(),
                },
            ),
        )
        .await
        .unwrap();
        assert_accepted(&danger, 3, WORLD_ID);
        let (extraction_request_id, extraction_receipt_id) =
            commit_caldus_fixture(persistence).await;
        let extraction_request = route_frame(
            5,
            [227; 16],
            3,
            WorldTransferCommand::UseCommittedExtraction {
                portal_id: WireText::new("portal.exit.dungeon.bell_sepulcher").unwrap(),
                extraction_request_id,
                extraction_receipt_id,
            },
        );
        let (_, hall_return) = bot_client::perform_world_flow(&client, extraction_request.clone())
            .await
            .unwrap();
        assert_accepted(&hall_return, 4, HALL_ID);
        let (_, extraction_replay) = bot_client::perform_world_flow(
            &client,
            WorldFlowFrame {
                sequence: 6,
                ..extraction_request
            },
        )
        .await
        .unwrap();
        assert_accepted(&extraction_replay, 4, HALL_ID);
        login_to_control
    };
    let ((), login_to_control) = tokio::join!(server_journey, client_journey);

    assert!(matches!(
        persistence.world_location(ACCOUNT_ID, CHARACTER_ID).await.unwrap(),
        Some(persistence::StoredWorldLocation::Safe {
            character_version: 4,
            location_content_id,
            arrival: persistence::StoredSafeArrival::HallDefault,
        }) if location_content_id == HALL_ID
    ));
    drop(client);
    client_endpoint.wait_idle().await;
    server_endpoint.close(0_u32.into(), b"journey complete");
    server_endpoint.wait_idle().await;
    login_to_control
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn reliable_quic_traverses_disposable_core_route_and_committed_extraction() {
    let persistence = disposable_database().await;
    seed_character(&persistence).await;
    let login_to_control = Box::pin(run_reliable_core_journey(&persistence)).await;
    assert!(login_to_control < Duration::from_secs(30));
    persistence.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn reliable_quic_completes_25_scripted_core_journeys_below_login_budget() {
    let persistence = disposable_database().await;
    let mut login_to_control = Vec::with_capacity(25);
    for _ in 0..25 {
        persistence.reset_disposable_identity_data().await.unwrap();
        seed_character(&persistence).await;
        let elapsed = Box::pin(run_reliable_core_journey(&persistence)).await;
        assert!(elapsed < Duration::from_secs(30));
        login_to_control.push(elapsed);
    }
    login_to_control.sort_unstable();
    let median = login_to_control[login_to_control.len() / 2];
    assert!(median < Duration::from_secs(30));
    println!(
        "GB-M03-03F 25-journey login-to-control: median={}us p95={}us max={}us",
        median.as_micros(),
        login_to_control[23].as_micros(),
        login_to_control[24].as_micros()
    );
    persistence.close().await;
}

fn seal_complete_same_tick_terminal_set(
    lethal: &TerminalCandidate,
) -> (CoreTerminalCoordinator, PreparedTerminal) {
    let tick = lethal.observed_tick();
    let version = lethal.expected_state_version();
    let mut coordinator = CoreTerminalCoordinator::new(
        durable_death_fixture::authenticated_account(),
        lethal.binding(),
    )
    .unwrap();
    // Submit every competing terminal result in reverse priority order. GB-M03-08 will replace
    // the opaque non-death candidates with their extraction/Recall repository plans; this already
    // proves that their shared production barrier cannot outrank the sealed lethal plan.
    for producer in CoreTerminalProducer::ALL.into_iter().rev() {
        let competing = if producer == CoreTerminalProducer::LethalHealth {
            lethal.clone()
        } else {
            let discriminator = 60_u8 + producer.terminal_kind().stable_code();
            TerminalCandidate::from_server_plan(
                lethal.binding(),
                [discriminator; 16],
                [discriminator + 10; 16],
                [discriminator + 20; 32],
                [discriminator + 30; 32],
                version,
                tick,
                producer.terminal_kind(),
            )
            .unwrap()
        };
        coordinator
            .evaluate(CoreTerminalEvaluation::candidate(
                producer,
                lethal.binding(),
                tick,
                version,
                competing,
            ))
            .unwrap();
    }
    let CoreTerminalTickSeal::Prepared(prepared) =
        coordinator.seal_authoritative_tick(tick, version).unwrap()
    else {
        panic!("same-tick terminal set must produce a winner")
    };
    assert_eq!(prepared.winner(), lethal);
    (coordinator, prepared)
}

/// GDD `DTH-001`/`DTH-020` and `TECH-020`-`023`, Content Spec `CONT-ECHO-009` and
/// `CONT-HUB-002`, and Roadmap `GB-M03-02`/`06`/`13` jointly require a committed lethal result to
/// survive lost delivery and process restart without duplicate domain records. The lethal input in
/// this proof is exclusively server-authored; real QUIC carries only authenticated historical
/// reads, so normal player death admission remains disabled.
#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn committed_death_survives_response_loss_and_full_process_restart_over_real_quic() {
    let persistence = disposable_database().await;
    durable_death_fixture::seed_danger_root(&persistence).await;
    let death = durable_death_fixture::prepare_death(persistence.clone()).await;
    let candidate = durable_death_terminal_candidate(&death).unwrap();
    let (mut coordinator, prepared) = seal_complete_same_tick_terminal_set(&candidate);
    let committed = DurableDeathExecutionService::new(persistence.clone())
        .execute_coordinated(&mut coordinator, &prepared, &death)
        .await
        .unwrap();
    assert!(!committed.transaction.is_replay());
    let expected_result = committed.transaction.result().clone();
    let expected_receipt = coordinator.committed_receipt().unwrap().clone();
    durable_death_fixture::assert_committed_graph(&persistence).await;

    // Capture one projection, abandon the following summary response, and then discard every
    // process-local authority that knew the commit acknowledgement.
    let latest_before_restart = run_lost_death_summary_session(&persistence).await;
    drop(committed);
    drop(coordinator);
    persistence.close().await;

    let config = PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL");
    let restarted = PostgresPersistence::connect(&config).await.unwrap();
    restarted.verify_disposable_test_database().await.unwrap();
    restarted.migrate().await.unwrap();

    let mut recovered = recover_committed_death_arbiter(
        &restarted,
        durable_death_fixture::ACCOUNT_ID,
        durable_death_fixture::CHARACTER_ID,
    )
    .await
    .unwrap()
    .expect("the committed terminal must reconstruct after process restart");
    assert_eq!(recovered.committed_receipt(), Some(&expected_receipt));
    assert!(matches!(
        recovered.submit(candidate.clone()),
        SubmitResult::ReplayedCommitted { receipt } if receipt == expected_receipt
    ));

    // A server that lost all in-memory acknowledgement may retry the exact sealed winner. The
    // durable writer returns its original result and the rebuilt arbiter publishes identical bytes.
    let mut replay_arbiter = TerminalArbiter::new(candidate.binding());
    assert!(matches!(
        replay_arbiter.submit(candidate),
        SubmitResult::Accepted { .. }
    ));
    let replay_prepared = replay_arbiter
        .prepare(death.request().plan.event.death_tick)
        .unwrap();
    let replay = DurableDeathExecutionService::new(restarted.clone())
        .execute_prepared(&mut replay_arbiter, &replay_prepared, &death)
        .await
        .unwrap();
    assert!(replay.transaction.is_replay());
    assert_eq!(replay.transaction.result(), &expected_result);
    assert_eq!(replay_arbiter.committed_receipt(), Some(&expected_receipt));

    let (latest, summary, memorial, trace) = run_restarted_death_read_session(&restarted).await;
    assert_eq!(latest, latest_before_restart);
    assert!(matches!(
        latest,
        DeathViewResultV1::Latest {
            death: Some(ref latest),
            ..
        } if latest.death_id == durable_death_fixture::DEATH_ID
    ));
    assert!(matches!(
        summary,
        DeathViewResultV1::Summary { ref summary, .. }
            if summary.death_id == durable_death_fixture::DEATH_ID
                && summary.echo_outcome == protocol::DeathEchoOutcomeV1::Available
                && summary.lost.len() == 2
    ));
    assert!(matches!(
        memorial,
        DeathViewResultV1::MemorialPage { ref entries, next_cursor: None, .. }
            if entries.len() == 1
                && entries[0].cursor.death_id == durable_death_fixture::DEATH_ID
    ));
    assert!(matches!(
        trace,
        DeathViewResultV1::TracePage { ref page, .. }
            if page.death_id == durable_death_fixture::DEATH_ID
                && page.entries.len() == 2
                && page.entries.last().is_some_and(|entry| entry.lethal)
    ));
    durable_death_fixture::assert_committed_graph(&restarted).await;
    restarted.close().await;
}
