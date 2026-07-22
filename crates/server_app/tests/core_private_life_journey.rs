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
    EntityKind, EntitySnapshot, HALL_INTERACTION_SCHEMA_VERSION, HallInteractionFrameV1,
    HallInteractionIntentV1, HallInteractionResultCodeV1, HallStationV1, HandshakeResponse,
    InputFrame, ManifestHash, Platform, ProtocolVersion, ReliableEvent, SafeArrival, WireMessage,
    WireText, WorldFlowContentRevisionV1, WorldFlowFrame, WorldFlowRequest, WorldFlowResult,
    WorldTransferCommand, WorldTransferMutation, WorldTransferPayload, WorldTransferResultCode,
};
use server_app::{
    BoundCorePrivateLifeServer, CORE_IDENTITY_BUILD_ID, CoreIdentityServerConfig,
    CoreIdentityServerReport, LOCAL_SERVER_NAME, LocalServerRuntimeError, SecretRewardEpoch,
};
use tokio::sync::oneshot;

const OPERATION_TIMEOUT: Duration = Duration::from_secs(10);
const MOVEMENT_TIMEOUT: Duration = Duration::from_secs(15);
const HALL_CONTENT_ID: &str = "hub.lantern_halls_01";
const MICROREALM_CONTENT_ID: &str = "world.core_microrealm_01";

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

fn assert_clean_microrealm_shutdown(report: CoreIdentityServerReport) {
    assert_eq!(report.accepted_connections, 1);
    assert_eq!(report.rejected_connections, 0);
    assert_eq!(report.combat_sessions_admitted, 1);
    assert_eq!(report.completed_connection_tasks, 1);
    assert_eq!(report.failed_connection_tasks, 0);
    assert_eq!(report.remaining_connection_tasks, 0);
    assert_eq!(report.remaining_open_connections, 0);
    assert!(report.zero_residue);
    assert!(report.persistence_enabled);
}

fn input(sequence: u32, horizontal_milli: i16, vertical_milli: i16) -> InputFrame {
    InputFrame {
        sequence,
        client_tick: u64::from(sequence),
        movement_x_milli: horizontal_milli,
        movement_y_milli: vertical_milli,
        aim_x_milli: 1,
        aim_y_milli: 0,
        held_primary: false,
        primary_sequence: 0,
        ability_1_sequence: 0,
        ability_2_sequence: 0,
    }
}

async fn next_complete_snapshot(
    connection: &quinn::Connection,
    assembler: &mut bot_client::BotSnapshotAssembler,
) -> bot_client::BotSnapshot {
    loop {
        let chunk = bot_client::receive_snapshot_datagram(connection)
            .await
            .unwrap();
        if let Some(snapshot) = assembler.ingest(chunk).unwrap() {
            return snapshot;
        }
    }
}

async fn drive_hall_until<Reached>(
    connection: &quinn::Connection,
    assembler: &mut bot_client::BotSnapshotAssembler,
    input_sequence: &mut u32,
    movement: (i16, i16),
    reached: Reached,
) -> EntitySnapshot
where
    Reached: Fn(&EntitySnapshot) -> bool,
{
    *input_sequence = input_sequence.checked_add(1).unwrap();
    bot_client::send_input_datagram(connection, input(*input_sequence, movement.0, movement.1))
        .unwrap();
    tokio::time::timeout(MOVEMENT_TIMEOUT, async {
        loop {
            let snapshot = next_complete_snapshot(connection, assembler).await;
            let player = snapshot
                .entities
                .iter()
                .find(|entity| entity.kind == EntityKind::Player)
                .expect("Hall snapshot must retain its authoritative player");
            if reached(player) {
                break;
            }
        }
        *input_sequence = input_sequence.checked_add(1).unwrap();
        bot_client::send_input_datagram(connection, input(*input_sequence, 0, 0)).unwrap();
        loop {
            let snapshot = next_complete_snapshot(connection, assembler).await;
            if snapshot.acknowledged_input_sequence >= *input_sequence {
                return snapshot
                    .entities
                    .into_iter()
                    .find(|entity| entity.kind == EntityKind::Player)
                    .expect("stopped Hall snapshot must retain its authoritative player");
            }
        }
    })
    .await
    .expect("authoritative Hall traversal timed out")
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
async fn production_root_admits_a_fresh_character_to_controllable_microrealm_and_cleans_up() {
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
    let hall_snapshot = tokio::time::timeout(
        OPERATION_TIMEOUT,
        next_complete_snapshot(&connection, &mut assembler),
    )
    .await
    .expect("authoritative Hall gameplay snapshot timed out");
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
                content_revision: world_revision.clone(),
            },
            scene: CorePrivateRouteSceneV1::LanternHalls,
            room: None,
            instance_lineage_id: None,
            actor_generation: route_state.actor_generation,
            route_state_version: route_state.state_version,
        })
        .unwrap();
    assert!(route_model.can_accept_gameplay_input());

    // The direct north line is obstructed by the authored central Hall fixture. Drive the
    // authoritative player around its west side, recenter above it, then approach the gate.
    let mut input_sequence = 0;
    let west = drive_hall_until(
        &connection,
        &mut assembler,
        &mut input_sequence,
        (-1_000, 0),
        |player| player.x_milli_tiles <= 28_500,
    )
    .await;
    assert!(west.y_milli_tiles > 26_300);
    let north_of_fixture = drive_hall_until(
        &connection,
        &mut assembler,
        &mut input_sequence,
        (0, -1_000),
        |player| player.y_milli_tiles <= 21_500,
    )
    .await;
    assert!(north_of_fixture.x_milli_tiles < 28_700);
    let recentered = drive_hall_until(
        &connection,
        &mut assembler,
        &mut input_sequence,
        (1_000, 0),
        |player| player.x_milli_tiles >= 32_000,
    )
    .await;
    assert!(recentered.y_milli_tiles < 21_700);
    let at_gate = drive_hall_until(
        &connection,
        &mut assembler,
        &mut input_sequence,
        (0, -1_000),
        |player| player.y_milli_tiles <= 4_200,
    )
    .await;
    let gate_offset = (
        i64::from(at_gate.x_milli_tiles - 32_000),
        i64::from(at_gate.y_milli_tiles - 3_000),
    );
    assert!(gate_offset.0 * gate_offset.0 + gate_offset.1 * gate_offset.1 <= 1_500_i64.pow(2));

    let gate_response = tokio::time::timeout(
        OPERATION_TIMEOUT,
        bot_client::perform_reliable_gameplay(
            &connection,
            WireMessage::HallInteractionFrame(HallInteractionFrameV1 {
                schema_version: HALL_INTERACTION_SCHEMA_VERSION,
                sequence: 1,
                intent: HallInteractionIntentV1::BeginHold,
            }),
        ),
    )
    .await
    .expect("Realm Gate interaction timed out")
    .unwrap();
    assert!(matches!(
        gate_response.event,
        ReliableEvent::HallInteractionResult(result)
            if result.code == HallInteractionResultCodeV1::Opened
                && result.station == Some(HallStationV1::RealmGate)
    ));

    let microrealm_payload = WorldTransferPayload {
        content_revision: world_revision.clone(),
        command: WorldTransferCommand::UsePortal {
            portal_id: WireText::new(HallStationV1::RealmGate.content_id()).unwrap(),
        },
    };
    let (_, microrealm_transfer) = tokio::time::timeout(
        OPERATION_TIMEOUT,
        bot_client::perform_world_flow(
            &connection,
            WorldFlowFrame {
                sequence: 3,
                request: WorldFlowRequest::Transfer(WorldTransferMutation {
                    mutation_id: [0x34; 16],
                    character_id,
                    expected_character_version: hall_location.character_version,
                    issued_at_unix_millis: current_unix_millis(),
                    payload_hash: microrealm_payload.canonical_hash(),
                    payload: microrealm_payload,
                }),
            },
        ),
    )
    .await
    .expect("Core microrealm transfer timed out")
    .unwrap();
    let WorldFlowResult::Transfer {
        accepted: true,
        code: WorldTransferResultCode::Accepted,
        snapshot: Some(microrealm_location),
        transfer_id: Some(_),
        ..
    } = microrealm_transfer
    else {
        panic!("an opened in-range Realm Gate must admit the production Core microrealm");
    };
    let CharacterLocation::Danger {
        location_id,
        instance_lineage_id,
        entry_restore_point_id,
    } = &microrealm_location.location
    else {
        panic!("Realm Gate admission must publish a durable danger location");
    };
    assert_eq!(location_id.as_str(), MICROREALM_CONTENT_ID);
    assert_ne!(*instance_lineage_id, [0; 16]);
    assert_ne!(*entry_restore_point_id, [0; 16]);

    route_model.begin_committed_transfer_refresh().unwrap();
    route_model
        .apply_location(microrealm_location.clone())
        .unwrap();
    let microrealm_route_frame = tokio::time::timeout(OPERATION_TIMEOUT, async {
        loop {
            let frame = bot_client::receive_server_reliable(&connection)
                .await
                .unwrap();
            if matches!(
                &frame.event,
                ReliableEvent::CorePrivateRouteState(state)
                    if frame.server_tick > 0
                        && state.scene == CorePrivateRouteSceneV1::CoreMicrorealm
            ) {
                break frame;
            }
        }
    })
    .await
    .expect("live Core microrealm route authority timed out");
    let ReliableEvent::CorePrivateRouteState(microrealm_route) = &microrealm_route_frame.event
    else {
        unreachable!("filtered reliable event is the Core microrealm route");
    };
    assert_eq!(microrealm_route.character_id, character_id);
    assert_eq!(
        microrealm_route.character_version,
        microrealm_location.character_version
    );
    assert_eq!(
        microrealm_route.instance_lineage_id,
        Some(*instance_lineage_id)
    );
    assert_eq!(
        microrealm_route.scene,
        CorePrivateRouteSceneV1::CoreMicrorealm
    );
    assert!(
        microrealm_route
            .readiness
            .accepts_gameplay_input
            .is_available()
    );

    let microrealm_snapshot = tokio::time::timeout(OPERATION_TIMEOUT, async {
        loop {
            let snapshot = next_complete_snapshot(&connection, &mut assembler).await;
            let in_microrealm = snapshot.entities.iter().any(|entity| {
                entity.kind == EntityKind::Player
                    && entity.x_milli_tiles < 15_000
                    && entity.y_milli_tiles > 35_000
            });
            if in_microrealm
                && snapshot.server_tick == microrealm_route_frame.server_tick
                && snapshot.state_version == microrealm_route.state_version
            {
                break snapshot;
            }
        }
    })
    .await
    .expect("matching Core microrealm gameplay snapshot timed out");
    let microrealm_players = microrealm_snapshot
        .entities
        .iter()
        .filter(|entity| entity.kind == EntityKind::Player)
        .collect::<Vec<_>>();
    assert_eq!(microrealm_players.len(), 1);
    assert!(microrealm_players[0].current_health > 0);

    route_model.apply_reliable(&microrealm_route_frame).unwrap();
    route_model
        .apply_scene_readiness(CorePrivateSceneReadiness {
            base: CoreSceneReadiness {
                location_id: WireText::new(MICROREALM_CONTENT_ID).unwrap(),
                character_version: microrealm_location.character_version,
                content_revision: world_revision,
            },
            scene: CorePrivateRouteSceneV1::CoreMicrorealm,
            room: None,
            instance_lineage_id: Some(*instance_lineage_id),
            actor_generation: microrealm_route.actor_generation,
            route_state_version: microrealm_route.state_version,
        })
        .unwrap();
    assert!(route_model.can_accept_gameplay_input());

    connection.close(0_u32.into(), b"production-root microrealm proof complete");
    client_endpoint.close(0_u32.into(), b"production-root microrealm proof complete");
    tokio::time::timeout(OPERATION_TIMEOUT, client_endpoint.wait_idle())
        .await
        .expect("client endpoint cleanup timed out");
    shutdown_send.send(()).unwrap();
    let report = tokio::time::timeout(OPERATION_TIMEOUT, server_task)
        .await
        .expect("production-root server shutdown timed out")
        .unwrap()
        .unwrap();
    assert_clean_microrealm_shutdown(report);

    let cleanup = PostgresPersistence::connect(&config).await.unwrap();
    cleanup.verify_disposable_test_database().await.unwrap();
    cleanup.reset_disposable_identity_data().await.unwrap();
    let mut verification = cleanup.begin_transaction().await.unwrap();
    let remaining_gameplay_roots: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM accounts) + (SELECT count(*) FROM caldus_victory_exits)",
    )
    .fetch_one(verification.connection())
    .await
    .unwrap();
    verification.rollback().await.unwrap();
    assert_eq!(remaining_gameplay_roots, 0);
    cleanup.close().await;
}
