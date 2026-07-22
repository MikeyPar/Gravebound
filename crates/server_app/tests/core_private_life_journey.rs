use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use client_bevy::{CorePrivateRouteClientModel, CorePrivateSceneReadiness, CoreSceneReadiness};
use persistence::{PersistenceConfig, PostgresPersistence};
use protocol::{
    AccountBootstrapFrame, AccountBootstrapRequest, AccountBootstrapResult, AuthTicket,
    CharacterLocation, CharacterMutationFrame, CharacterMutationPayload, ClientHello, Compression,
    CorePrivateRouteContentRevisionV1, CorePrivateRoutePhaseV1, CorePrivateRouteSceneV1,
    EntityKind, HandshakeResponse, ManifestHash, Platform, ProtocolVersion, ReliableEvent,
    SafeArrival, WireText, WorldFlowContentRevisionV1, WorldFlowFrame, WorldFlowRequest,
    WorldFlowResult, WorldTransferCommand, WorldTransferMutation, WorldTransferPayload,
    WorldTransferResultCode,
};
use server_app::{
    BoundCorePrivateLifeServer, CORE_IDENTITY_BUILD_ID, CoreIdentityServerConfig,
    CoreIdentityServerReport, LOCAL_SERVER_NAME, LocalServerRuntimeError, SecretRewardEpoch,
};
use tokio::sync::oneshot;

const OPERATION_TIMEOUT: Duration = Duration::from_secs(10);
const HALL_CONTENT_ID: &str = "hub.lantern_halls_01";

fn content_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
}

fn current_unix_millis() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must follow the Unix epoch")
            .as_millis(),
    )
    .expect("current Unix milliseconds must fit in u64")
}

fn manifest(content_root: &Path) -> ManifestHash {
    let (_, report) = sim_content::load_and_validate(content_root).unwrap();
    ManifestHash::new(report.package_hash_blake3).unwrap()
}

fn world_flow_revision(content_root: &Path) -> WorldFlowContentRevisionV1 {
    let content = sim_content::load_core_development_world_flow(content_root).unwrap();
    WorldFlowContentRevisionV1 {
        records_blake3: ManifestHash::new(content.hashes().records_blake3.clone()).unwrap(),
        assets_blake3: ManifestHash::new(content.hashes().assets_blake3.clone()).unwrap(),
        localization_blake3: ManifestHash::new(content.hashes().localization_blake3.clone())
            .unwrap(),
    }
}

fn route_revision(content_root: &Path) -> CorePrivateRouteContentRevisionV1 {
    let content = sim_content::load_core_private_life_content(content_root).unwrap();
    CorePrivateRouteContentRevisionV1 {
        records_blake3: ManifestHash::new(content.revision().records_blake3.clone()).unwrap(),
        assets_blake3: ManifestHash::new(content.revision().assets_blake3.clone()).unwrap(),
        localization_blake3: ManifestHash::new(content.revision().localization_blake3.clone())
            .unwrap(),
    }
}

fn client_endpoint(certificate_der: &[u8]) -> quinn::Endpoint {
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(rustls::pki_types::CertificateDer::from(
            certificate_der.to_vec(),
        ))
        .unwrap();
    let config = quinn::ClientConfig::with_root_certificates(Arc::new(roots)).unwrap();
    let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    endpoint.set_default_client_config(config);
    endpoint
}

fn hello(content_root: &Path, ticket: Vec<u8>) -> ClientHello {
    ClientHello {
        protocol_major: ProtocolVersion::current().major,
        protocol_minor: ProtocolVersion::current().minor,
        client_build_id: WireText::new(CORE_IDENTITY_BUILD_ID).unwrap(),
        platform: Platform::WindowsNative,
        supported_compression: vec![Compression::None],
        content_manifest_hash: manifest(content_root),
        auth_ticket: AuthTicket::new(ticket).unwrap(),
        locale: WireText::new("en-US").unwrap(),
    }
}

fn assert_normal_route_capabilities(server_hello: &protocol::ServerHello) {
    let actual = server_hello
        .feature_flags
        .iter()
        .map(WireText::as_str)
        .collect::<BTreeSet<_>>();
    for required in [
        protocol::CORE_TEST_IDENTITY_FEATURE_FLAG,
        protocol::CORE_WORLD_FLOW_FEATURE_FLAG,
        protocol::CORE_SAFE_INVENTORY_FEATURE_FLAG,
        protocol::CORE_DEATH_VIEW_FEATURE_FLAG,
        protocol::CORE_EXTRACTION_TERMINAL_FEATURE_FLAG,
        protocol::CORE_RECALL_TERMINAL_FEATURE_FLAG,
        protocol::CORE_RESOLUTION_HOLD_FEATURE_FLAG,
        protocol::CORE_SUCCESSOR_FEATURE_FLAG,
        protocol::HALL_INTERACTION_FEATURE_FLAG,
        protocol::CORE_CONSUMABLE_FEATURE_FLAG,
        protocol::SAFE_STORAGE_FEATURE_FLAG,
        protocol::CORE_COMBAT_PRESENTATION_FEATURE_FLAG,
    ] {
        assert!(
            actual.contains(required),
            "missing production capability {required}"
        );
    }
}

fn assert_clean_hall_shutdown(report: CoreIdentityServerReport) {
    assert_eq!(report.accepted_connections, 1);
    assert_eq!(report.rejected_connections, 0);
    assert_eq!(report.combat_sessions_admitted, 0);
    assert_eq!(report.completed_connection_tasks, 1);
    assert_eq!(report.failed_connection_tasks, 0);
    assert_eq!(report.remaining_connection_tasks, 0);
    assert_eq!(report.remaining_open_connections, 0);
    assert!(report.zero_residue);
    assert!(report.persistence_enabled);
}

type ServerTask =
    tokio::task::JoinHandle<Result<CoreIdentityServerReport, LocalServerRuntimeError>>;

fn start_server(
    persistence: PostgresPersistence,
    content_root: &Path,
) -> (
    std::net::SocketAddr,
    rustls::pki_types::CertificateDer<'static>,
    oneshot::Sender<()>,
    ServerTask,
) {
    let server = BoundCorePrivateLifeServer::bind_persistent(
        &CoreIdentityServerConfig {
            bind_address: "127.0.0.1:0".parse().unwrap(),
            content_root: content_root.to_path_buf(),
        },
        persistence,
        SecretRewardEpoch::new("m03-production-route-harness", [0xa7; 32]).unwrap(),
    )
    .unwrap();
    let address = server.local_address();
    let certificate = rustls::pki_types::CertificateDer::from(server.certificate_der().to_vec());
    let (shutdown_send, shutdown_receive) = oneshot::channel();
    let task = tokio::spawn(server.serve_until(async {
        let _ = shutdown_receive.await;
    }));
    (address, certificate, shutdown_send, task)
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
#[allow(
    clippy::too_many_lines,
    reason = "the production-root route proof stays contiguous so no direct state-writing seam can be hidden"
)]
async fn production_root_admits_a_fresh_character_to_controllable_hall_and_cleans_up() {
    let config = PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL");
    let persistence = PostgresPersistence::connect(&config).await.unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence.migrate().await.unwrap();
    persistence.reset_disposable_identity_data().await.unwrap();

    let content_root = content_root();
    let world_revision = world_flow_revision(&content_root);
    let local_route_revision = route_revision(&content_root);
    let (address, certificate, shutdown_send, server_task) =
        start_server(persistence, &content_root);
    let client_endpoint = client_endpoint(certificate.as_ref());
    let connection = tokio::time::timeout(
        OPERATION_TIMEOUT,
        client_endpoint.connect(address, LOCAL_SERVER_NAME).unwrap(),
    )
    .await
    .expect("production-root QUIC connection timed out")
    .unwrap();

    let ticket = format!("m03-production-root-hall-{}", current_unix_millis()).into_bytes();
    let HandshakeResponse::Accepted(server_hello) = tokio::time::timeout(
        OPERATION_TIMEOUT,
        bot_client::perform_handshake(&connection, hello(&content_root, ticket)),
    )
    .await
    .expect("production-root handshake timed out")
    .unwrap() else {
        panic!("production root must admit the matching client");
    };
    server_hello.validate().unwrap();
    assert_normal_route_capabilities(&server_hello);

    let (_, bootstrap) = tokio::time::timeout(
        OPERATION_TIMEOUT,
        bot_client::perform_account_bootstrap(
            &connection,
            AccountBootstrapFrame {
                sequence: 1,
                request: AccountBootstrapRequest::Bootstrap,
                content_manifest_hash: manifest(&content_root),
            },
        ),
    )
    .await
    .expect("account bootstrap timed out")
    .unwrap();
    let AccountBootstrapResult::Snapshot(empty_account) = bootstrap else {
        panic!("a new authenticated account must bootstrap through the normal route");
    };
    assert_eq!(empty_account.account_version, 1);
    assert!(empty_account.characters.is_empty());
    assert_eq!(empty_account.selected_character_id, None);

    let create_payload = CharacterMutationPayload::Create {
        class_id: WireText::new(protocol::GRAVE_ARBALIST_CLASS_ID).unwrap(),
    };
    let (_, created) = tokio::time::timeout(
        OPERATION_TIMEOUT,
        bot_client::perform_character_mutation(
            &connection,
            CharacterMutationFrame {
                mutation_id: [0x31; 16],
                expected_account_version: empty_account.account_version,
                payload_hash: create_payload.canonical_hash(),
                issued_at_unix_millis: current_unix_millis(),
                payload: create_payload,
            },
        ),
    )
    .await
    .expect("character creation timed out")
    .unwrap();
    assert!(created.accepted);
    let created_account = created
        .snapshot
        .expect("accepted creation returns its snapshot");
    assert_eq!(created_account.characters.len(), 1);
    let character_id = created_account.characters[0].character_id;

    let select_payload = CharacterMutationPayload::Select { character_id };
    let (_, selected) = tokio::time::timeout(
        OPERATION_TIMEOUT,
        bot_client::perform_character_mutation(
            &connection,
            CharacterMutationFrame {
                mutation_id: [0x32; 16],
                expected_account_version: created_account.account_version,
                payload_hash: select_payload.canonical_hash(),
                issued_at_unix_millis: current_unix_millis(),
                payload: select_payload,
            },
        ),
    )
    .await
    .expect("character selection timed out")
    .unwrap();
    assert!(selected.accepted);
    assert_eq!(
        selected
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.selected_character_id),
        Some(character_id)
    );

    let (_, location) = tokio::time::timeout(
        OPERATION_TIMEOUT,
        bot_client::perform_world_flow(
            &connection,
            WorldFlowFrame {
                sequence: 1,
                request: WorldFlowRequest::Location {
                    character_id,
                    content_revision: world_revision.clone(),
                },
            },
        ),
    )
    .await
    .expect("Character Select location query timed out")
    .unwrap();
    let WorldFlowResult::Location {
        snapshot: character_select,
        ..
    } = location
    else {
        panic!("fresh selected character must have a durable Character Select location");
    };
    assert!(matches!(
        character_select.location,
        CharacterLocation::CharacterSelect { .. }
    ));

    let hall_payload = WorldTransferPayload {
        content_revision: world_revision.clone(),
        command: WorldTransferCommand::EnterHallFromCharacterSelect,
    };
    let (_, hall_transfer) = tokio::time::timeout(
        OPERATION_TIMEOUT,
        bot_client::perform_world_flow(
            &connection,
            WorldFlowFrame {
                sequence: 2,
                request: WorldFlowRequest::Transfer(WorldTransferMutation {
                    mutation_id: [0x33; 16],
                    character_id,
                    expected_character_version: character_select.character_version,
                    issued_at_unix_millis: current_unix_millis(),
                    payload_hash: hall_payload.canonical_hash(),
                    payload: hall_payload,
                }),
            },
        ),
    )
    .await
    .expect("Hall transfer timed out")
    .unwrap();
    let WorldFlowResult::Transfer {
        accepted: true,
        code: WorldTransferResultCode::Accepted,
        snapshot: Some(hall_location),
        transfer_id: Some(_),
        ..
    } = hall_transfer
    else {
        panic!("production root must commit the normal Character Select to Hall transfer");
    };
    assert!(matches!(
        &hall_location.location,
        CharacterLocation::Safe {
            location_id,
            arrival: SafeArrival::HallDefault,
        } if location_id.as_str() == HALL_CONTENT_ID
    ));

    let route_frame = tokio::time::timeout(
        OPERATION_TIMEOUT,
        bot_client::receive_server_reliable(&connection),
    )
    .await
    .expect("authoritative Hall route publication timed out")
    .unwrap();
    let ReliableEvent::CorePrivateRouteState(route_state) = &route_frame.event else {
        panic!("Hall transfer must be followed by its authoritative route state");
    };
    assert_eq!(route_state.character_id, character_id);
    assert_eq!(
        route_state.character_version,
        hall_location.character_version
    );
    assert_eq!(route_state.content_revision, local_route_revision);
    assert_eq!(route_state.scene, CorePrivateRouteSceneV1::LanternHalls);
    assert_eq!(route_state.phase, CorePrivateRoutePhaseV1::Hall);
    assert!(route_state.readiness.accepts_gameplay_input.is_available());

    let mut assembler = bot_client::BotSnapshotAssembler::default();
    let hall_snapshot = loop {
        let chunk = tokio::time::timeout(
            OPERATION_TIMEOUT,
            bot_client::receive_snapshot_datagram(&connection),
        )
        .await
        .expect("authoritative Hall gameplay snapshot timed out")
        .unwrap();
        if let Some(snapshot) = assembler.ingest(chunk).unwrap() {
            break snapshot;
        }
    };
    let players = hall_snapshot
        .entities
        .iter()
        .filter(|entity| entity.kind == EntityKind::Player)
        .collect::<Vec<_>>();
    assert_eq!(players.len(), 1);
    assert!(players[0].current_health > 0);
    assert_eq!(players[0].current_health, players[0].maximum_health);

    let mut route_model = CorePrivateRouteClientModel::new(
        character_id,
        world_revision.clone(),
        local_route_revision,
    )
    .unwrap();
    assert!(route_model.accept_server_hello(&server_hello).unwrap());
    route_model.apply_location(hall_location.clone()).unwrap();
    route_model.apply_reliable(&route_frame).unwrap();
    route_model
        .apply_scene_readiness(CorePrivateSceneReadiness {
            base: CoreSceneReadiness {
                location_id: WireText::new(HALL_CONTENT_ID).unwrap(),
                character_version: hall_location.character_version,
                content_revision: world_revision,
            },
            scene: CorePrivateRouteSceneV1::LanternHalls,
            room: None,
            instance_lineage_id: None,
            actor_generation: route_state.actor_generation,
            route_state_version: route_state.state_version,
        })
        .unwrap();
    assert!(route_model.can_accept_gameplay_input());

    connection.close(0_u32.into(), b"production-root Hall proof complete");
    client_endpoint.close(0_u32.into(), b"production-root Hall proof complete");
    tokio::time::timeout(OPERATION_TIMEOUT, client_endpoint.wait_idle())
        .await
        .expect("client endpoint cleanup timed out");
    shutdown_send.send(()).unwrap();
    let report = tokio::time::timeout(OPERATION_TIMEOUT, server_task)
        .await
        .expect("production-root server shutdown timed out")
        .unwrap()
        .unwrap();
    assert_clean_hall_shutdown(report);

    let cleanup = PostgresPersistence::connect(&config).await.unwrap();
    cleanup.verify_disposable_test_database().await.unwrap();
    cleanup.reset_disposable_identity_data().await.unwrap();
    cleanup.close().await;
}
