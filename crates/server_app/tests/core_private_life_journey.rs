use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use client_bevy::{CorePrivateRouteClientModel, CorePrivateSceneReadiness, CoreSceneReadiness};
use persistence::{
    PersistenceConfig, PostgresPersistence, StoredM03OnboardingEventV1,
    StoredM03SessionEndReasonV1, StoredM03SessionEventV1, StoredM03TelemetryEnvironmentV1,
    StoredM03TelemetryEventV1, StoredM03TelemetryPlatformV1, StoredM03TelemetrySourceV1,
};
use protocol::{
    AccountBootstrapFrame, AccountBootstrapRequest, AccountBootstrapResult, ActionFrame,
    ActionKind, ActionResultCode, AuthTicket, BargainContentRevisionV1, BargainResultCode,
    BargainViewFrame, CharacterLocation, CharacterMutationFrame, CharacterMutationPayload,
    ClientHello, Compression, CorePrivateRouteContentRevisionV1, CorePrivateRoutePhaseV1,
    CorePrivateRouteRoomV1, CorePrivateRouteSceneV1, CorePrivateRouteStateV1, EntityKind,
    EntitySnapshot, HALL_INTERACTION_SCHEMA_VERSION, HallInteractionFrameV1,
    HallInteractionIntentV1, HallInteractionResultCodeV1, HallStationV1, HandshakeResponse,
    InputFrame, ManifestHash, Platform, ProtocolVersion, ReliableEvent, ReliableEventFrame,
    SafeArrival, WireMessage, WireText, WorldFlowContentRevisionV1, WorldFlowFrame,
    WorldFlowRequest, WorldFlowResult, WorldTransferCommand, WorldTransferMutation,
    WorldTransferPayload, WorldTransferResultCode,
};
use server_app::{
    BoundCorePrivateLifeServer, CORE_IDENTITY_BUILD_ID, CoreIdentityServerConfig,
    CoreIdentityServerReport, LOCAL_SERVER_NAME, LocalServerRuntimeError, SecretRewardEpoch,
};
use tokio::sync::{oneshot, watch};

const OPERATION_TIMEOUT: Duration = Duration::from_secs(10);
const MOVEMENT_TIMEOUT: Duration = Duration::from_secs(15);
const COMBAT_TIMEOUT: Duration = Duration::from_mins(3);
const BOSS_TIMEOUT: Duration = Duration::from_mins(5);
const HALL_CONTENT_ID: &str = "hub.lantern_halls_01";
const MICROREALM_CONTENT_ID: &str = "world.core_microrealm_01";
const BELL_DUNGEON_CONTENT_ID: &str = "dungeon.bell_sepulcher";
const BELL_DUNGEON_PORTAL_ID: &str = "portal.dungeon.bell_sepulcher";
const TELEMETRY_ENVIRONMENT_VARIABLE: &str = "GRAVEBOUND_TELEMETRY_ENVIRONMENT";
const TELEMETRY_REGION_VARIABLE: &str = "GRAVEBOUND_TELEMETRY_REGION_ID";
const TELEMETRY_TEST_REGION: &str = "local-playtest";

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

fn bargain_revision(content_root: &Path) -> BargainContentRevisionV1 {
    let content = sim_content::load_core_development_oaths_bargains(content_root).unwrap();
    BargainContentRevisionV1 {
        records_blake3: ManifestHash::new(content.hashes().records_blake3.clone()).unwrap(),
        assets_blake3: ManifestHash::new(content.hashes().assets_blake3.clone()).unwrap(),
        localization_blake3: ManifestHash::new(content.hashes().localization_blake3.clone())
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

async fn wait_for_clean_exit_telemetry(
    persistence: &PostgresPersistence,
) -> Vec<StoredM03TelemetrySourceV1> {
    tokio::time::timeout(OPERATION_TIMEOUT, async {
        loop {
            let sources = persistence.poll_m03_telemetry_sources_v1(16).await.unwrap();
            if sources.iter().any(|source| {
                matches!(
                    source.event,
                    StoredM03TelemetryEventV1::Session(StoredM03SessionEventV1::Ended {
                        reason: StoredM03SessionEndReasonV1::CleanExit,
                        ..
                    })
                )
            }) {
                return sources;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("native clean-exit telemetry timed out")
}

fn assert_production_route_telemetry(
    sources: &[StoredM03TelemetrySourceV1],
    character_id: [u8; 16],
) -> ([u8; 16], [u8; 16]) {
    assert_eq!(sources.len(), 5);
    let account_id = sources[0].context.account_id;
    let session_id = sources[0].context.session_id;
    assert_ne!(account_id, [0; 16]);
    assert_eq!(session_id[6] >> 4, 7);
    assert_eq!(session_id[8] >> 6, 2);
    assert!(sources.iter().all(|source| {
        source.context.account_id == account_id
            && source.context.session_id == session_id
            && source.context.build_id == CORE_IDENTITY_BUILD_ID
            && source.context.content_bundle_version == protocol::M03_CORE_DEV_CONTENT_TARGET
            && source.context.platform == StoredM03TelemetryPlatformV1::Windows
            && source.context.region_id == TELEMETRY_TEST_REGION
            && source.context.environment == StoredM03TelemetryEnvironmentV1::Test
            && source.context.cohort_tags == ["cohort.private"]
            && source.event_id != [0; 16]
    }));
    assert_eq!(
        sources
            .iter()
            .filter(|source| {
                matches!(
                    source.event,
                    StoredM03TelemetryEventV1::Session(StoredM03SessionEventV1::Started)
                )
            })
            .count(),
        1
    );
    assert_eq!(
        sources
            .iter()
            .filter(|source| {
                matches!(
                    source.event,
                    StoredM03TelemetryEventV1::Session(StoredM03SessionEventV1::Ended {
                        reason: StoredM03SessionEndReasonV1::CleanExit,
                        ..
                    })
                )
            })
            .count(),
        1
    );
    assert_eq!(
        sources
            .iter()
            .filter(|source| {
                matches!(
                    source.event,
                    StoredM03TelemetryEventV1::Onboarding(
                        StoredM03OnboardingEventV1::AccountCreated
                    )
                )
            })
            .count(),
        1
    );
    assert_eq!(
        sources
            .iter()
            .filter(|source| {
                source.context.character_id == Some(character_id)
                    && matches!(
                        source.event,
                        StoredM03TelemetryEventV1::Onboarding(
                            StoredM03OnboardingEventV1::CharacterCreated { ref class_id }
                        ) if class_id == "class.grave_arbalist"
                    )
            })
            .count(),
        1
    );
    assert_eq!(
        sources
            .iter()
            .filter(|source| {
                source.context.character_id == Some(character_id)
                    && matches!(
                        source.event,
                        StoredM03TelemetryEventV1::Onboarding(
                            StoredM03OnboardingEventV1::CharacterEnteredCombat {
                                ref class_id,
                                ref source_content_id,
                            }
                        ) if class_id == "class.grave_arbalist"
                            && source_content_id == MICROREALM_CONTENT_ID
                    )
            })
            .count(),
        1
    );
    assert!(
        sources
            .iter()
            .all(|source| { !matches!(source.event, StoredM03TelemetryEventV1::Crash(_)) })
    );
    (account_id, session_id)
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

fn combat_input(sequence: u32, player: &EntitySnapshot, target: &EntitySnapshot) -> InputFrame {
    let delta_x = i64::from(target.x_milli_tiles - player.x_milli_tiles);
    let delta_y = i64::from(target.y_milli_tiles - player.y_milli_tiles);
    let longest_axis = delta_x.abs().max(delta_y.abs()).max(1);
    let horizontal_aim = i16::try_from(delta_x * 1_000 / longest_axis).unwrap();
    let vertical_aim = i16::try_from(delta_y * 1_000 / longest_axis).unwrap();
    let distance_squared = delta_x * delta_x + delta_y * delta_y;

    // Close distance until the starter weapon can connect, then strafe around the target. The
    // periodic direction reversal prevents a deterministic bot from pinning itself against the
    // authored shell while still exercising ordinary movement and hostile avoidance.
    let (mut horizontal_motion, mut vertical_motion) = if distance_squared > 6_000_i64.pow(2) {
        (horizontal_aim, vertical_aim)
    } else if (sequence / 90).is_multiple_of(2) {
        (-vertical_aim, horizontal_aim)
    } else {
        (vertical_aim, -horizontal_aim)
    };
    if player.x_milli_tiles < 2_000 {
        horizontal_motion = 1_000;
    } else if player.x_milli_tiles > 46_000 {
        horizontal_motion = -1_000;
    }
    if player.y_milli_tiles < 2_000 {
        vertical_motion = 1_000;
    } else if player.y_milli_tiles > 46_000 {
        vertical_motion = -1_000;
    }

    InputFrame {
        sequence,
        client_tick: u64::from(sequence),
        movement_x_milli: horizontal_motion,
        movement_y_milli: vertical_motion,
        aim_x_milli: horizontal_aim,
        aim_y_milli: vertical_aim,
        held_primary: true,
        primary_sequence: 1,
        ability_1_sequence: 0,
        ability_2_sequence: 0,
    }
}

fn fixed_room_bounds(room: CorePrivateRouteRoomV1) -> (i64, i64) {
    match room {
        CorePrivateRouteRoomV1::BellCrossB1 => (17_000, 17_000),
        // B2 uses the exact clockwise-rotated 15x21 Nave template.
        CorePrivateRouteRoomV1::BellNaveB2 => (21_000, 15_000),
        CorePrivateRouteRoomV1::BellKnightB3 => (19_000, 15_000),
        CorePrivateRouteRoomV1::BellBridgeB5 => (23_000, 11_000),
        CorePrivateRouteRoomV1::CaldusArenaB6 => (18_000, 18_000),
        CorePrivateRouteRoomV1::BellVestibuleB0 | CorePrivateRouteRoomV1::BellRestB4 => {
            unreachable!("safe rooms never run combat")
        }
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "the deterministic public-input policy keeps pursuit, projectile avoidance, and authored-shell safety auditable together"
)]
fn fixed_dungeon_combat_input(
    sequence: u32,
    player: &EntitySnapshot,
    target: &EntitySnapshot,
    entities: &[EntitySnapshot],
    room: CorePrivateRouteRoomV1,
) -> InputFrame {
    let delta_x = i64::from(target.x_milli_tiles - player.x_milli_tiles);
    let delta_y = i64::from(target.y_milli_tiles - player.y_milli_tiles);
    let longest_aim_axis = delta_x.abs().max(delta_y.abs()).max(1);
    let horizontal_aim = i16::try_from(delta_x * 1_000 / longest_aim_axis).unwrap();
    let vertical_aim = i16::try_from(delta_y * 1_000 / longest_aim_axis).unwrap();
    let distance_squared = delta_x * delta_x + delta_y * delta_y;
    let is_boss = target.kind == EntityKind::Boss;
    let preferred_distance = if is_boss { 7_000_i64 } else { 5_500_i64 };
    let distance_tolerance = if is_boss { 1_000_i64 } else { 750_i64 };

    // Maintain the starter Crossbow's legal range while moving perpendicular to aimed attacks.
    // A slow deterministic direction reversal prevents wall-locking without reading attack IDs.
    let (mut movement_x, mut movement_y) =
        if distance_squared > (preferred_distance + distance_tolerance).pow(2) {
            (delta_x, delta_y)
        } else if distance_squared < (preferred_distance - distance_tolerance).pow(2) {
            (-delta_x, -delta_y)
        } else if (sequence / 180).is_multiple_of(2) {
            (-delta_y, delta_x)
        } else {
            (delta_y, -delta_x)
        };

    // Snapshots expose only authoritative positions and velocities. Predict each hostile shot
    // 350 ms forward and steer toward the aggregate local gap; no pattern, hit, or outcome is
    // authored by the harness. The ordinary server collision and movement caps remain final.
    for projectile in entities
        .iter()
        .filter(|entity| entity.kind == EntityKind::HostileProjectile)
    {
        let projected_x = i64::from(projectile.x_milli_tiles)
            + i64::from(projectile.velocity_x_milli_tiles_per_second) * 350 / 1_000;
        let projected_y = i64::from(projectile.y_milli_tiles)
            + i64::from(projectile.velocity_y_milli_tiles_per_second) * 350 / 1_000;
        let away_x = i64::from(player.x_milli_tiles) - projected_x;
        let away_y = i64::from(player.y_milli_tiles) - projected_y;
        let distance = away_x.abs().max(away_y.abs());
        if distance < 3_500 {
            let weight = 3_500 - distance;
            let divisor = distance.max(250);
            movement_x += away_x * weight / divisor * 3;
            movement_y += away_y * weight / divisor * 3;
        }
    }

    // Keep the driver off authored shells and bridge water even when projectile repulsion and
    // target pursuit cancel each other. The authoritative collision world still owns legality.
    let (width, height) = fixed_room_bounds(room);
    let player_x = i64::from(player.x_milli_tiles);
    let player_y = i64::from(player.y_milli_tiles);
    if player_x < 1_500 {
        movement_x += 4_000;
    } else if player_x > width - 1_500 {
        movement_x -= 4_000;
    }
    if player_y < 1_500 {
        movement_y += 4_000;
    } else if player_y > height - 1_500 {
        movement_y -= 4_000;
    }

    let longest_motion_axis = movement_x.abs().max(movement_y.abs()).max(1);
    let movement_x = (movement_x * 1_000 / longest_motion_axis).clamp(-1_000, 1_000);
    let movement_y = (movement_y * 1_000 / longest_motion_axis).clamp(-1_000, 1_000);

    InputFrame {
        sequence,
        client_tick: u64::from(sequence),
        movement_x_milli: i16::try_from(movement_x).unwrap(),
        movement_y_milli: i16::try_from(movement_y).unwrap(),
        aim_x_milli: horizontal_aim,
        aim_y_milli: vertical_aim,
        held_primary: true,
        primary_sequence: 1,
        ability_1_sequence: 0,
        ability_2_sequence: 0,
    }
}

fn nearest_hostile<'a>(
    player: &EntitySnapshot,
    entities: &'a [EntitySnapshot],
) -> Option<&'a EntitySnapshot> {
    entities
        .iter()
        .filter(|entity| matches!(entity.kind, EntityKind::Enemy | EntityKind::Boss))
        .min_by_key(|entity| {
            let delta_x = i64::from(entity.x_milli_tiles - player.x_milli_tiles);
            let delta_y = i64::from(entity.y_milli_tiles - player.y_milli_tiles);
            delta_x * delta_x + delta_y * delta_y
        })
}

fn spawn_route_event_pump(
    connection: quinn::Connection,
) -> (
    watch::Receiver<Option<ReliableEventFrame>>,
    tokio::task::JoinHandle<()>,
) {
    let (route_send, route_receive) = watch::channel(None);
    let task = tokio::spawn(async move {
        while let Ok(frame) = bot_client::receive_server_reliable(&connection).await {
            if matches!(frame.event, ReliableEvent::CorePrivateRouteState(_))
                && route_send.send(Some(frame)).is_err()
            {
                break;
            }
        }
    });
    (route_receive, task)
}

fn matching_route<Matches>(
    route_receive: &watch::Receiver<Option<ReliableEventFrame>>,
    matches: &Matches,
) -> Option<ReliableEventFrame>
where
    Matches: Fn(&CorePrivateRouteStateV1) -> bool,
{
    route_receive.borrow().as_ref().and_then(|frame| {
        let ReliableEvent::CorePrivateRouteState(route) = &frame.event else {
            unreachable!("the route event pump publishes only route projections");
        };
        matches(route).then(|| frame.clone())
    })
}

async fn wait_for_route<Matches>(
    route_receive: &mut watch::Receiver<Option<ReliableEventFrame>>,
    matches: Matches,
    timeout_message: &'static str,
) -> ReliableEventFrame
where
    Matches: Fn(&CorePrivateRouteStateV1) -> bool,
{
    tokio::time::timeout(OPERATION_TIMEOUT, async {
        loop {
            if let Some(frame) = matching_route(route_receive, &matches) {
                return frame;
            }
            route_receive
                .changed()
                .await
                .expect("route event pump must remain attached");
        }
    })
    .await
    .expect(timeout_message)
}

#[derive(Debug, Default)]
struct CombatAbilityCadence {
    last_grave_mark_tick: u64,
    last_slipstep_tick: u64,
}

async fn press_combat_ability(
    connection: &quinn::Connection,
    action_sequence: &mut u32,
    server_tick: u64,
    action: ActionKind,
) {
    *action_sequence = action_sequence.checked_add(1).unwrap();
    let response = bot_client::perform_reliable_gameplay(
        connection,
        WireMessage::ActionFrame(ActionFrame {
            sequence: *action_sequence,
            client_tick: server_tick,
            action,
        }),
    )
    .await
    .unwrap();
    assert!(matches!(
        response.event,
        ReliableEvent::ActionResult {
            action_sequence: accepted_sequence,
            code: ActionResultCode::Accepted,
        } if accepted_sequence == *action_sequence
    ));
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "the bounded production journey keeps all public transport, input, route, and cadence authority explicit"
)]
async fn drive_fixed_dungeon_combat_until<Reached>(
    connection: &quinn::Connection,
    assembler: &mut bot_client::BotSnapshotAssembler,
    route_receive: &mut watch::Receiver<Option<ReliableEventFrame>>,
    input_sequence: &mut u32,
    action_sequence: &mut u32,
    ability_cadence: &mut CombatAbilityCadence,
    room: CorePrivateRouteRoomV1,
    timeout: Duration,
    reached: Reached,
    timeout_message: &'static str,
) -> ReliableEventFrame
where
    Reached: Fn(&CorePrivateRouteStateV1) -> bool,
{
    tokio::time::timeout(timeout, async {
        loop {
            if let Some(frame) = matching_route(route_receive, &reached) {
                return frame;
            }

            tokio::select! {
                changed = route_receive.changed() => {
                    changed.expect("route event pump must remain attached");
                    if let Some(frame) = route_receive.borrow().as_ref() {
                        let ReliableEvent::CorePrivateRouteState(route) = &frame.event else {
                            unreachable!("the route event pump publishes only route projections");
                        };
                        assert_ne!(
                            route.phase,
                            CorePrivateRoutePhaseV1::TerminalPending,
                            "ordinary fixed-route combat reached a terminal outcome before its authored boundary"
                        );
                    }
                }
                chunk = bot_client::receive_snapshot_datagram(connection) => {
                    let Some(snapshot) = assembler.ingest(chunk.unwrap()).unwrap() else {
                        continue;
                    };
                    let player = snapshot
                        .entities
                        .iter()
                        .find(|entity| entity.kind == EntityKind::Player)
                        .expect("fixed-dungeon snapshot must retain its authoritative player");
                    assert!(
                        player.current_health > 0,
                        "ordinary input must reach the requested fixed-route boundary alive"
                    );
                    let Some(target) = nearest_hostile(player, &snapshot.entities) else {
                        *input_sequence = input_sequence.checked_add(1).unwrap();
                        bot_client::send_input_datagram(
                            connection,
                            InputFrame {
                                primary_sequence: 1,
                                ..input(*input_sequence, 0, 0)
                            },
                        )
                        .unwrap();
                        continue;
                    };

                    *input_sequence = input_sequence.checked_add(1).unwrap();
                    bot_client::send_input_datagram(
                        connection,
                        fixed_dungeon_combat_input(
                            *input_sequence,
                            player,
                            target,
                            &snapshot.entities,
                            room,
                        ),
                    )
                    .unwrap();

                    if snapshot.server_tick.saturating_sub(ability_cadence.last_grave_mark_tick)
                        >= 150
                    {
                        press_combat_ability(
                            connection,
                            action_sequence,
                            snapshot.server_tick,
                            ActionKind::Ability1Press,
                        )
                        .await;
                        ability_cadence.last_grave_mark_tick = snapshot.server_tick;
                    }
                    if snapshot.server_tick.saturating_sub(ability_cadence.last_slipstep_tick)
                        >= 240
                    {
                        press_combat_ability(
                            connection,
                            action_sequence,
                            snapshot.server_tick,
                            ActionKind::Ability2Press,
                        )
                        .await;
                        ability_cadence.last_slipstep_tick = snapshot.server_tick;
                    }
                }
            }
        }
    })
    .await
    .expect(timeout_message)
}

async fn interact_and_wait_for_route<Reached>(
    connection: &quinn::Connection,
    route_receive: &mut watch::Receiver<Option<ReliableEventFrame>>,
    action_sequence: &mut u32,
    client_tick: u64,
    reached: Reached,
    timeout_message: &'static str,
) -> ReliableEventFrame
where
    Reached: Fn(&CorePrivateRouteStateV1) -> bool,
{
    *action_sequence = action_sequence.checked_add(1).unwrap();
    let response = bot_client::perform_reliable_gameplay(
        connection,
        WireMessage::ActionFrame(ActionFrame {
            sequence: *action_sequence,
            client_tick,
            action: ActionKind::Interact,
        }),
    )
    .await
    .unwrap();
    assert!(matches!(
        response.event,
        ReliableEvent::ActionResult {
            action_sequence: accepted_sequence,
            code: ActionResultCode::Accepted,
        } if accepted_sequence == *action_sequence
    ));
    wait_for_route(route_receive, reached, timeout_message).await
}

async fn drive_microrealm_until_cleared(
    connection: &quinn::Connection,
    assembler: &mut bot_client::BotSnapshotAssembler,
    route_receive: &mut watch::Receiver<Option<ReliableEventFrame>>,
    input_sequence: &mut u32,
) -> ReliableEventFrame {
    let cleared = |route: &CorePrivateRouteStateV1| {
        route.scene == CorePrivateRouteSceneV1::CoreMicrorealm
            && route.phase == CorePrivateRoutePhaseV1::MicrorealmCleared
            && route.readiness.bell_portal_available.is_available()
    };
    tokio::time::timeout(COMBAT_TIMEOUT, async {
        loop {
            if let Some(frame) = matching_route(route_receive, &cleared) {
                *input_sequence = input_sequence.checked_add(1).unwrap();
                bot_client::send_input_datagram(
                    connection,
                    InputFrame {
                        primary_sequence: 1,
                        ..input(*input_sequence, 0, 0)
                    },
                )
                .unwrap();
                return frame;
            }

            tokio::select! {
                changed = route_receive.changed() => {
                    changed.expect("route event pump must remain attached");
                }
                chunk = bot_client::receive_snapshot_datagram(connection) => {
                    let Some(snapshot) = assembler.ingest(chunk.unwrap()).unwrap() else {
                        continue;
                    };
                    let player = snapshot
                        .entities
                        .iter()
                        .find(|entity| entity.kind == EntityKind::Player)
                        .expect("microrealm snapshot must retain its authoritative player");
                    assert!(player.current_health > 0, "ordinary combat must reach the Bell portal alive");
                    let Some(target) = nearest_hostile(player, &snapshot.entities) else {
                        continue;
                    };
                    *input_sequence = input_sequence.checked_add(1).unwrap();
                    bot_client::send_input_datagram(
                        connection,
                        combat_input(*input_sequence, player, target),
                    )
                    .unwrap();
                }
            }
        }
    })
    .await
    .expect("ordinary public-input microrealm clear timed out")
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

async fn drive_player_until<Reached>(
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
                .expect("gameplay snapshot must retain its authoritative player");
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
                    .expect("stopped gameplay snapshot must retain its authoritative player");
            }
        }
    })
    .await
    .expect("authoritative player traversal timed out")
}

async fn drive_player_to_waypoint(
    connection: &quinn::Connection,
    assembler: &mut bot_client::BotSnapshotAssembler,
    input_sequence: &mut u32,
    waypoint: (i32, i32),
) -> EntitySnapshot {
    tokio::time::timeout(MOVEMENT_TIMEOUT, async {
        loop {
            let snapshot = next_complete_snapshot(connection, assembler).await;
            let player = snapshot
                .entities
                .iter()
                .find(|entity| entity.kind == EntityKind::Player)
                .expect("gameplay snapshot must retain its authoritative player");
            assert!(player.current_health > 0);
            let delta_x = waypoint.0 - player.x_milli_tiles;
            let delta_y = waypoint.1 - player.y_milli_tiles;
            if i64::from(delta_x).pow(2) + i64::from(delta_y).pow(2) <= 900_i64.pow(2) {
                *input_sequence = input_sequence.checked_add(1).unwrap();
                bot_client::send_input_datagram(
                    connection,
                    InputFrame {
                        primary_sequence: 1,
                        ..input(*input_sequence, 0, 0)
                    },
                )
                .unwrap();
                return player.clone();
            }
            let longest_axis = delta_x.abs().max(delta_y.abs()).max(1);
            let horizontal_motion = i16::try_from(delta_x * 1_000 / longest_axis).unwrap();
            let vertical_motion = i16::try_from(delta_y * 1_000 / longest_axis).unwrap();
            *input_sequence = input_sequence.checked_add(1).unwrap();
            bot_client::send_input_datagram(
                connection,
                InputFrame {
                    movement_x_milli: horizontal_motion,
                    movement_y_milli: vertical_motion,
                    primary_sequence: 1,
                    ..input(*input_sequence, 0, 0)
                },
            )
            .unwrap();
        }
    })
    .await
    .expect("authoritative waypoint traversal timed out")
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
async fn production_root_reaches_caldus_exit_ready_and_cleans_up() {
    assert_eq!(
        std::env::var(TELEMETRY_ENVIRONMENT_VARIABLE).as_deref(),
        Ok("test"),
        "the hosted route command must opt into test-attributed telemetry"
    );
    assert_eq!(
        std::env::var(TELEMETRY_REGION_VARIABLE).as_deref(),
        Ok(TELEMETRY_TEST_REGION),
        "the hosted route command must bind one explicit telemetry region"
    );
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
    let west = drive_player_until(
        &connection,
        &mut assembler,
        &mut input_sequence,
        (-1_000, 0),
        |player| player.x_milli_tiles <= 28_500,
    )
    .await;
    assert!(west.y_milli_tiles > 26_300);
    let north_of_fixture = drive_player_until(
        &connection,
        &mut assembler,
        &mut input_sequence,
        (0, -1_000),
        |player| player.y_milli_tiles <= 21_500,
    )
    .await;
    assert!(north_of_fixture.x_milli_tiles < 28_700);
    let recentered = drive_player_until(
        &connection,
        &mut assembler,
        &mut input_sequence,
        (1_000, 0),
        |player| player.x_milli_tiles >= 32_000,
    )
    .await;
    assert!(recentered.y_milli_tiles < 21_700);
    let at_gate = drive_player_until(
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
                content_revision: world_revision.clone(),
            },
            scene: CorePrivateRouteSceneV1::CoreMicrorealm,
            room: None,
            instance_lineage_id: Some(*instance_lineage_id),
            actor_generation: microrealm_route.actor_generation,
            route_state_version: microrealm_route.state_version,
        })
        .unwrap();
    assert!(route_model.can_accept_gameplay_input());

    // From this point onward one task exclusively owns server-initiated reliable streams. Direct
    // request/response frames continue to use their bidirectional streams, so route transitions
    // cannot be lost when snapshot traffic is busy or a response is in flight.
    let (mut route_receive, route_pump) = spawn_route_event_pump(connection.clone());
    let cleared_route_frame = drive_microrealm_until_cleared(
        &connection,
        &mut assembler,
        &mut route_receive,
        &mut input_sequence,
    )
    .await;
    let ReliableEvent::CorePrivateRouteState(cleared_route) = &cleared_route_frame.event else {
        unreachable!("the combat driver returns a filtered route projection");
    };
    assert_eq!(cleared_route.character_id, character_id);
    assert_eq!(
        cleared_route.content_revision,
        microrealm_route.content_revision
    );
    assert_eq!(
        cleared_route.instance_lineage_id,
        Some(*instance_lineage_id)
    );
    assert!(cleared_route.readiness.microrealm_cleared.is_available());

    // Follow the authored road through Lantern Fork and its east bend. The portal authority uses
    // the live server position, so a durable transfer cannot be substituted for this traversal.
    drive_player_to_waypoint(
        &connection,
        &mut assembler,
        &mut input_sequence,
        (24_500, 24_500),
    )
    .await;
    drive_player_to_waypoint(
        &connection,
        &mut assembler,
        &mut input_sequence,
        (40_500, 24_500),
    )
    .await;
    let at_bell_portal = drive_player_to_waypoint(
        &connection,
        &mut assembler,
        &mut input_sequence,
        (40_500, 8_500),
    )
    .await;
    let portal_offset = (
        i64::from(at_bell_portal.x_milli_tiles - 40_500),
        i64::from(at_bell_portal.y_milli_tiles - 8_500),
    );
    assert!(
        portal_offset.0 * portal_offset.0 + portal_offset.1 * portal_offset.1 <= 900_i64.pow(2)
    );

    let bell_payload = WorldTransferPayload {
        content_revision: world_revision.clone(),
        command: WorldTransferCommand::UsePortal {
            portal_id: WireText::new(BELL_DUNGEON_PORTAL_ID).unwrap(),
        },
    };
    let (_, bell_transfer) = tokio::time::timeout(
        OPERATION_TIMEOUT,
        bot_client::perform_world_flow(
            &connection,
            WorldFlowFrame {
                sequence: 4,
                request: WorldFlowRequest::Transfer(WorldTransferMutation {
                    mutation_id: [0x35; 16],
                    character_id,
                    expected_character_version: cleared_route.character_version,
                    issued_at_unix_millis: current_unix_millis(),
                    payload_hash: bell_payload.canonical_hash(),
                    payload: bell_payload,
                }),
            },
        ),
    )
    .await
    .expect("Bell Sepulcher transfer timed out")
    .unwrap();
    let WorldFlowResult::Transfer {
        accepted: true,
        code: WorldTransferResultCode::Accepted,
        snapshot: Some(bell_location),
        transfer_id: Some(_),
        ..
    } = bell_transfer
    else {
        panic!("the live cleared Bell portal must commit its ordinary dungeon transfer");
    };
    let CharacterLocation::Danger {
        location_id,
        instance_lineage_id: bell_lineage_id,
        entry_restore_point_id: bell_restore_point_id,
    } = &bell_location.location
    else {
        panic!("the Bell Sepulcher must remain inside the durable danger lineage");
    };
    assert_eq!(location_id.as_str(), BELL_DUNGEON_CONTENT_ID);
    assert_eq!(bell_lineage_id, instance_lineage_id);
    assert_eq!(bell_restore_point_id, entry_restore_point_id);

    let b0_route_frame = wait_for_route(
        &mut route_receive,
        |route| {
            route.scene == CorePrivateRouteSceneV1::BellSepulcher
                && route.room == Some(CorePrivateRouteRoomV1::BellVestibuleB0)
                && route.phase == CorePrivateRoutePhaseV1::DungeonVestibule
        },
        "Bell B0 route authority timed out",
    )
    .await;
    let ReliableEvent::CorePrivateRouteState(b0_route) = &b0_route_frame.event else {
        unreachable!("the route waiter returns a filtered route projection");
    };
    assert_eq!(b0_route.character_version, bell_location.character_version);
    assert!(b0_route.readiness.room_exit_available.is_available());

    let b0_snapshot = tokio::time::timeout(OPERATION_TIMEOUT, async {
        loop {
            let snapshot = next_complete_snapshot(&connection, &mut assembler).await;
            let Some(player) = snapshot
                .entities
                .iter()
                .find(|entity| entity.kind == EntityKind::Player)
            else {
                continue;
            };
            if player.x_milli_tiles <= 13_000 && player.y_milli_tiles <= 11_000 {
                break snapshot;
            }
        }
    })
    .await
    .expect("Bell B0 gameplay snapshot timed out");
    assert_eq!(
        b0_snapshot
            .entities
            .iter()
            .filter(|entity| matches!(entity.kind, EntityKind::Enemy | EntityKind::Boss))
            .count(),
        0,
        "the authored B0 vestibule is safe and contains no hostile"
    );

    let enter_b1 = tokio::time::timeout(
        OPERATION_TIMEOUT,
        bot_client::perform_reliable_gameplay(
            &connection,
            WireMessage::ActionFrame(ActionFrame {
                sequence: 1,
                client_tick: b0_route_frame.server_tick,
                action: ActionKind::Interact,
            }),
        ),
    )
    .await
    .expect("public B0 exit interaction timed out")
    .unwrap();
    assert!(matches!(
        enter_b1.event,
        ReliableEvent::ActionResult {
            action_sequence: 1,
            code: ActionResultCode::Accepted,
        }
    ));

    let b1_active_frame = wait_for_route(
        &mut route_receive,
        |route| {
            route.scene == CorePrivateRouteSceneV1::BellSepulcher
                && route.room == Some(CorePrivateRouteRoomV1::BellCrossB1)
                && route.phase == CorePrivateRoutePhaseV1::RoomActive
        },
        "Bell B1 active authority timed out",
    )
    .await;
    let ReliableEvent::CorePrivateRouteState(b1_active) = &b1_active_frame.event else {
        unreachable!("the route waiter returns a filtered route projection");
    };
    assert_eq!(b1_active.instance_lineage_id, Some(*instance_lineage_id));
    assert!(b1_active.readiness.accepts_gameplay_input.is_available());

    let b1_snapshot = tokio::time::timeout(OPERATION_TIMEOUT, async {
        loop {
            let snapshot = next_complete_snapshot(&connection, &mut assembler).await;
            let hostile_count = snapshot
                .entities
                .iter()
                .filter(|entity| entity.kind == EntityKind::Enemy)
                .count();
            if hostile_count == 8 {
                break snapshot;
            }
        }
    })
    .await
    .expect("authored Bell B1 roster timed out");
    assert_eq!(
        b1_snapshot
            .entities
            .iter()
            .filter(|entity| entity.kind == EntityKind::Player)
            .count(),
        1
    );
    assert!(
        b1_snapshot
            .entities
            .iter()
            .all(|entity| entity.kind != EntityKind::Boss)
    );

    let mut action_sequence = 1;
    let mut ability_cadence = CombatAbilityCadence::default();
    let b1_cleared_frame = drive_fixed_dungeon_combat_until(
        &connection,
        &mut assembler,
        &mut route_receive,
        &mut input_sequence,
        &mut action_sequence,
        &mut ability_cadence,
        CorePrivateRouteRoomV1::BellCrossB1,
        COMBAT_TIMEOUT,
        |route| {
            route.room == Some(CorePrivateRouteRoomV1::BellCrossB1)
                && route.phase == CorePrivateRoutePhaseV1::RoomCleared
        },
        "ordinary Bell B1 clear timed out",
    )
    .await;
    let ReliableEvent::CorePrivateRouteState(b1_cleared) = &b1_cleared_frame.event else {
        unreachable!("the combat driver returns a filtered route projection");
    };
    assert!(b1_cleared.readiness.room_exit_available.is_available());

    let b2_active_frame = interact_and_wait_for_route(
        &connection,
        &mut route_receive,
        &mut action_sequence,
        b1_cleared_frame.server_tick,
        |route| {
            route.room == Some(CorePrivateRouteRoomV1::BellNaveB2)
                && route.phase == CorePrivateRoutePhaseV1::RoomActive
        },
        "ordinary Bell B2 activation timed out",
    )
    .await;
    let ReliableEvent::CorePrivateRouteState(b2_active) = &b2_active_frame.event else {
        unreachable!("the route waiter returns a filtered route projection");
    };
    assert_eq!(b2_active.instance_lineage_id, Some(*instance_lineage_id));
    let b2_cleared_frame = drive_fixed_dungeon_combat_until(
        &connection,
        &mut assembler,
        &mut route_receive,
        &mut input_sequence,
        &mut action_sequence,
        &mut ability_cadence,
        CorePrivateRouteRoomV1::BellNaveB2,
        COMBAT_TIMEOUT,
        |route| {
            route.room == Some(CorePrivateRouteRoomV1::BellNaveB2)
                && route.phase == CorePrivateRoutePhaseV1::RoomCleared
        },
        "ordinary Bell B2 clear timed out",
    )
    .await;

    let b3_active_frame = interact_and_wait_for_route(
        &connection,
        &mut route_receive,
        &mut action_sequence,
        b2_cleared_frame.server_tick,
        |route| {
            route.room == Some(CorePrivateRouteRoomV1::BellKnightB3)
                && route.phase == CorePrivateRoutePhaseV1::RoomActive
        },
        "ordinary Bell B3 activation timed out",
    )
    .await;
    let ReliableEvent::CorePrivateRouteState(b3_active) = &b3_active_frame.event else {
        unreachable!("the route waiter returns a filtered route projection");
    };
    assert_eq!(b3_active.instance_lineage_id, Some(*instance_lineage_id));
    let b3_cleared_frame = drive_fixed_dungeon_combat_until(
        &connection,
        &mut assembler,
        &mut route_receive,
        &mut input_sequence,
        &mut action_sequence,
        &mut ability_cadence,
        CorePrivateRouteRoomV1::BellKnightB3,
        COMBAT_TIMEOUT,
        |route| {
            route.room == Some(CorePrivateRouteRoomV1::BellKnightB3)
                && route.phase == CorePrivateRoutePhaseV1::RoomCleared
        },
        "ordinary Bell B3 reward-and-clear timed out",
    )
    .await;

    let b4_rest_frame = interact_and_wait_for_route(
        &connection,
        &mut route_receive,
        &mut action_sequence,
        b3_cleared_frame.server_tick,
        |route| {
            route.room == Some(CorePrivateRouteRoomV1::BellRestB4)
                && route.phase == CorePrivateRoutePhaseV1::Rest
        },
        "ordinary Bell B4 rest entry timed out",
    )
    .await;
    let ReliableEvent::CorePrivateRouteState(b4_rest) = &b4_rest_frame.event else {
        unreachable!("the route waiter returns a filtered route projection");
    };
    assert!(b4_rest.readiness.room_exit_available.is_available());

    // The exact ordinary life has not reached the temporary Core level-5 milestone. B3 therefore
    // commits its item/XP terminal with an authoritative no-offer result, and the public B4 view
    // must report that result rather than allowing the harness to invent or skip a Bargain.
    let (_, bargain_view) = bot_client::perform_bargain_view(
        &connection,
        BargainViewFrame {
            sequence: 1,
            character_id,
            content_revision: bargain_revision(&content_root),
        },
    )
    .await
    .unwrap();
    assert_eq!(bargain_view.code, BargainResultCode::NoOffer);
    let bargain_projection = bargain_view
        .projection
        .expect("authoritative no-offer view retains the character life projection");
    assert_eq!(bargain_projection.character_id, character_id);
    assert_eq!(bargain_projection.earned_bargain_slots, 0);
    assert!(bargain_projection.active_bargain_ids.is_empty());
    assert!(bargain_projection.offer.is_none());

    let b5_active_frame = interact_and_wait_for_route(
        &connection,
        &mut route_receive,
        &mut action_sequence,
        b4_rest_frame.server_tick,
        |route| {
            route.room == Some(CorePrivateRouteRoomV1::BellBridgeB5)
                && route.phase == CorePrivateRoutePhaseV1::RoomActive
        },
        "ordinary Bell B5 activation timed out",
    )
    .await;
    let ReliableEvent::CorePrivateRouteState(b5_active) = &b5_active_frame.event else {
        unreachable!("the route waiter returns a filtered route projection");
    };
    assert_eq!(b5_active.instance_lineage_id, Some(*instance_lineage_id));
    let b5_cleared_frame = drive_fixed_dungeon_combat_until(
        &connection,
        &mut assembler,
        &mut route_receive,
        &mut input_sequence,
        &mut action_sequence,
        &mut ability_cadence,
        CorePrivateRouteRoomV1::BellBridgeB5,
        COMBAT_TIMEOUT,
        |route| {
            route.room == Some(CorePrivateRouteRoomV1::BellBridgeB5)
                && route.phase == CorePrivateRoutePhaseV1::RoomCleared
        },
        "ordinary Bell B5 clear timed out",
    )
    .await;

    let b6_frame = interact_and_wait_for_route(
        &connection,
        &mut route_receive,
        &mut action_sequence,
        b5_cleared_frame.server_tick,
        |route| {
            route.room == Some(CorePrivateRouteRoomV1::CaldusArenaB6)
                && matches!(
                    route.phase,
                    CorePrivateRoutePhaseV1::BossStaging
                        | CorePrivateRoutePhaseV1::BossReadyCountdown
                        | CorePrivateRoutePhaseV1::BossIntroduction
                        | CorePrivateRoutePhaseV1::BossPhaseOne
                )
        },
        "ordinary Sir Caldus staging timed out",
    )
    .await;
    let ReliableEvent::CorePrivateRouteState(b6_route) = &b6_frame.event else {
        unreachable!("the route waiter returns a filtered route projection");
    };
    assert_eq!(b6_route.instance_lineage_id, Some(*instance_lineage_id));

    let exit_ready_frame = drive_fixed_dungeon_combat_until(
        &connection,
        &mut assembler,
        &mut route_receive,
        &mut input_sequence,
        &mut action_sequence,
        &mut ability_cadence,
        CorePrivateRouteRoomV1::CaldusArenaB6,
        BOSS_TIMEOUT,
        |route| {
            route.room == Some(CorePrivateRouteRoomV1::CaldusArenaB6)
                && route.phase == CorePrivateRoutePhaseV1::BossExitReady
        },
        "ordinary Sir Caldus defeat, durable reward, and stable exit timed out",
    )
    .await;
    let ReliableEvent::CorePrivateRouteState(exit_ready) = &exit_ready_frame.event else {
        unreachable!("the combat driver returns a filtered route projection");
    };
    assert_eq!(exit_ready.character_id, character_id);
    assert_eq!(exit_ready.instance_lineage_id, Some(*instance_lineage_id));
    assert!(exit_ready.readiness.boss_encounter_ready.is_available());
    assert!(exit_ready.readiness.extraction_available.is_available());

    connection.close(0_u32.into(), b"native client shutdown");
    client_endpoint.close(0_u32.into(), b"native client shutdown");
    tokio::time::timeout(OPERATION_TIMEOUT, client_endpoint.wait_idle())
        .await
        .expect("client endpoint cleanup timed out");
    tokio::time::timeout(OPERATION_TIMEOUT, route_pump)
        .await
        .expect("route event pump cleanup timed out")
        .unwrap();
    let cleanup = PostgresPersistence::connect(&config).await.unwrap();
    cleanup.verify_disposable_test_database().await.unwrap();
    let sources = wait_for_clean_exit_telemetry(&cleanup).await;
    let (telemetry_account_id, _) = assert_production_route_telemetry(&sources, character_id);
    assert!(
        cleanup
            .load_open_m03_telemetry_session_v1(telemetry_account_id)
            .await
            .unwrap()
            .is_none()
    );
    shutdown_send.send(()).unwrap();
    let report = tokio::time::timeout(OPERATION_TIMEOUT, server_task)
        .await
        .expect("production-root server shutdown timed out")
        .unwrap()
        .unwrap();
    assert_clean_microrealm_shutdown(report);

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
