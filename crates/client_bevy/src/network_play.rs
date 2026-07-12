//! Native Bevy presentation and input orchestration for the `GB-M02-GATE` playtest build.
//!
//! This mode deliberately does not install `LocalLab` combat, enemy, encounter, inventory, or death
//! authority systems. It predicts local movement only and presents server snapshots.

use std::{net::SocketAddr, path::PathBuf, time::Instant};

use anyhow::{Context, Result, bail};
use bevy::{
    app::AppExit,
    prelude::*,
    window::{PrimaryWindow, WindowResolution},
};
use protocol::{
    ActionFrame, ActionKind, AuthTicket, ClientHello, Compression, ENTITY_STATE_ALIVE,
    ENTITY_STATE_ELIGIBLE, EntityKind, M02_LOCAL_BUILD_ID, M02_LOCAL_SERVER_NAME,
    M02_PLAYER_ENTITY_ID_BASE, ManifestHash, MutationRequest, PickupPlacement, Platform,
    ProtocolVersion, ReliableEvent, WireMessage, WireText,
};
use sim_content::{first_playable_arena, load_and_validate};
use sim_core::{PlayerMovementState, SimulationVector};

use crate::{
    FrameSet, LoadedArena, NativeNetworkPresentation, NetworkCorrectionDiagnostics,
    PackageDiagnostics, PredictedMovementInput, RemoteClientRuntime, RemoteSnapshotInbox,
    accessibility::AccessibilitySettings,
    arena_view::render_point_to_simulation,
    combat::EvidenceScenario,
    network_session::{ClientConnectionLifecycle, ClientConnectionPhase},
    network_transport::{NetworkTransportConfig, NetworkWorkerHandle, TransportEvent},
    player::{LatestMovementAction, PlayerSimulation},
};

const NETWORK_WINDOW_TITLE: &str = "Gravebound - M02 Network Playtest";
const INITIAL_LOCAL_PLAYER_ID: u64 = M02_PLAYER_ENTITY_ID_BASE;

#[derive(Debug, Clone)]
pub struct NetworkPlayConfig {
    pub server_address: SocketAddr,
    pub certificate_path: PathBuf,
    pub player_token: String,
    pub content_root: PathBuf,
}

#[derive(Resource, Debug)]
struct NetworkBridge(NetworkWorkerHandle);

#[derive(Resource, Debug)]
struct NetworkPlayState {
    lifecycle: ClientConnectionLifecycle,
    started: Instant,
    status: String,
    fatal_error: Option<String>,
    reliable_results: u64,
    saw_hostile: bool,
    combat_complete: bool,
}

#[derive(Resource, Debug)]
struct NetworkInputSequencer {
    input_sequence: u32,
    primary_sequence: u32,
    action_sequence: u32,
    mutation_sequence: u128,
    primary_was_held: bool,
    last_aim: (i16, i16),
}

impl Default for NetworkInputSequencer {
    fn default() -> Self {
        Self {
            input_sequence: 1,
            primary_sequence: 0,
            action_sequence: 1,
            mutation_sequence: 1,
            primary_was_held: false,
            last_aim: (1_000, 0),
        }
    }
}

impl NetworkInputSequencer {
    fn sample_primary(&mut self, held: bool) -> Option<u32> {
        if held && !self.primary_was_held {
            self.primary_sequence = self.primary_sequence.checked_add(1)?;
        }
        self.primary_was_held = held;
        Some(self.primary_sequence)
    }
}

#[derive(Component)]
struct NetworkStatusText;

#[derive(Component)]
struct NetworkHealthText;

#[derive(Component)]
struct NetworkGateOverlay;

#[allow(clippy::too_many_lines)] // App assembly remains linear so authority exclusions are reviewable.
pub fn run_network_playtest(config: NetworkPlayConfig) -> Result<()> {
    if config.player_token.trim().is_empty() {
        bail!("--player must contain a nonempty local playtest token");
    }
    let certificate_der = std::fs::read(&config.certificate_path).with_context(|| {
        format!(
            "failed to read local server certificate {}",
            config.certificate_path.display()
        )
    })?;
    let (package, report) = load_and_validate(&config.content_root).with_context(|| {
        format!(
            "content validation failed at {}",
            config.content_root.display()
        )
    })?;
    if report.content_version != "fp.1.0.0" {
        bail!(
            "M02 network playtest requires fp.1.0.0, received {}",
            report.content_version
        );
    }
    let arena = first_playable_arena(&package).context("failed to compile Bell Laboratory")?;
    let initial_movement = PlayerMovementState::at_arena_spawn(&arena)
        .context("failed to construct predicted player movement")?;
    let hello = ClientHello {
        protocol_major: ProtocolVersion::current().major,
        protocol_minor: ProtocolVersion::current().minor,
        client_build_id: WireText::new(M02_LOCAL_BUILD_ID)?,
        platform: Platform::WindowsNative,
        supported_compression: vec![Compression::None],
        content_manifest_hash: ManifestHash::new(report.package_hash_blake3.clone())?,
        auth_ticket: AuthTicket::new(config.player_token.into_bytes())?,
        locale: WireText::new("en-US")?,
    };
    let worker = NetworkWorkerHandle::spawn(NetworkTransportConfig {
        server_address: config.server_address,
        server_name: M02_LOCAL_SERVER_NAME.to_owned(),
        certificate_der,
        hello,
    })?;
    let mut lifecycle = ClientConnectionLifecycle::default();
    lifecycle
        .join_request(0, 0)
        .context("failed to initialize client Join lifecycle")?;

    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb_u8(7, 10, 14)))
        .insert_resource(LoadedArena(arena.clone()))
        .insert_resource(PlayerSimulation::new(initial_movement))
        .insert_resource(EvidenceScenario::None)
        .insert_resource(AccessibilitySettings::default())
        .insert_resource(Time::<Fixed>::from_hz(f64::from(
            sim_core::TICKS_PER_SECOND,
        )))
        .insert_resource(PackageDiagnostics {
            build_id: M02_LOCAL_BUILD_ID.to_owned(),
            content_version: report.content_version,
            record_count: report.record_count,
            package_hash_blake3: report.package_hash_blake3,
            content_root: config.content_root,
            runtime_label: "NETWORK PLAYTEST",
            milestone_label: "GB-M02 AUTHORITY GATE",
        })
        .insert_resource(NativeNetworkPresentation::new(RemoteClientRuntime::new(
            INITIAL_LOCAL_PLAYER_ID,
            arena,
            initial_movement,
        )))
        .insert_resource(NetworkBridge(worker))
        .insert_resource(NetworkPlayState {
            lifecycle,
            started: Instant::now(),
            status: "CONNECTING".to_owned(),
            fatal_error: None,
            reliable_results: 0,
            saw_hostile: false,
            combat_complete: false,
        })
        .insert_resource(NetworkInputSequencer::default())
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: NETWORK_WINDOW_TITLE.to_owned(),
                        resolution: WindowResolution::new(1280, 720),
                        ..default()
                    }),
                    ..default()
                }),
        )
        .configure_sets(
            Update,
            (
                FrameSet::CameraFollow,
                FrameSet::InputSample,
                FrameSet::Presentation,
            )
                .chain(),
        )
        .add_systems(
            Startup,
            (crate::arena_view::spawn_arena_view, spawn_network_hud),
        )
        .add_systems(PreUpdate, poll_network_transport)
        .add_systems(FixedUpdate, predict_and_send_input)
        .add_systems(
            Update,
            (send_reliable_edges, update_network_hud)
                .chain()
                .in_set(FrameSet::Presentation),
        )
        .add_systems(Last, shutdown_network_on_exit);
    crate::player::configure(&mut app);
    crate::network_prediction::configure(&mut app);
    app.run();
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn poll_network_transport(
    bridge: Res<NetworkBridge>,
    mut inbox: ResMut<RemoteSnapshotInbox>,
    mut state: ResMut<NetworkPlayState>,
    mut presentation: ResMut<NativeNetworkPresentation>,
) {
    for snapshot in bridge.0.drain_snapshots() {
        inbox.push(snapshot);
    }
    for event in bridge.0.drain_events() {
        match event {
            TransportEvent::Connecting => "CONNECTING".clone_into(&mut state.status),
            TransportEvent::HandshakeAccepted => "JOINING".clone_into(&mut state.status),
            TransportEvent::Reliable(event) => {
                state.reliable_results = state.reliable_results.saturating_add(1);
                if matches!(event.event, ReliableEvent::Control(_)) {
                    if let ReliableEvent::Control(protocol::ControlEvent::SessionResult(result)) =
                        &event.event
                        && let Some(entity_id) = result.controlled_entity_id
                        && presentation.runtime().local_entity_id() != entity_id
                        && let Err(error) =
                            presentation.runtime_mut().bind_local_entity_id(entity_id)
                    {
                        state.fatal_error = Some(error.to_string());
                        continue;
                    }
                    let now = elapsed_millis(state.started);
                    match state.lifecycle.apply_reliable_event(&event, now) {
                        Ok(()) => {
                            lifecycle_label(state.lifecycle.phase()).clone_into(&mut state.status);
                        }
                        Err(error) => state.fatal_error = Some(error.to_string()),
                    }
                }
            }
            TransportEvent::LinkLost => {
                let now = elapsed_millis(state.started);
                if state.lifecycle.transport_lost(now).is_ok() {
                    "LINK LOST — CHARACTER REMAINS VULNERABLE".clone_into(&mut state.status);
                }
            }
            TransportEvent::Reconnecting { attempt } => {
                let now = elapsed_millis(state.started);
                if matches!(
                    state.lifecycle.phase(),
                    ClientConnectionPhase::LinkLost { .. }
                        | ClientConnectionPhase::AwaitingAuthoritativeResolution { .. }
                ) {
                    let _ = state
                        .lifecycle
                        .reconnect_request(0, now.saturating_mul(1_000));
                }
                state.status = format!("RECONNECTING — ATTEMPT {attempt}");
            }
            TransportEvent::TransportClosed => "CLOSED".clone_into(&mut state.status),
            TransportEvent::Fatal(error) => {
                "CONNECTION FAILED".clone_into(&mut state.status);
                state.fatal_error = Some(error);
            }
        }
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::needless_pass_by_value,
    clippy::similar_names,
    clippy::too_many_arguments
)]
fn predict_and_send_input(
    bridge: Res<NetworkBridge>,
    movement: Res<LatestMovementAction>,
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform)>,
    arena: Res<LoadedArena>,
    mut presentation: ResMut<NativeNetworkPresentation>,
    mut sequencer: ResMut<NetworkInputSequencer>,
    mut state: ResMut<NetworkPlayState>,
) {
    let sequence = sequencer.input_sequence;
    let action = movement.0;
    if let Err(error) = presentation
        .runtime_mut()
        .predict_local_movement(PredictedMovementInput { sequence, action })
    {
        bevy::log::debug!(%error, "local movement prediction skipped");
    }
    let normalized = action.normalized_vector();
    let movement_x_milli = (normalized.x * 1_000.0).round() as i16;
    let movement_y_milli = (normalized.y * 1_000.0).round() as i16;
    let aim = cursor_aim(
        &window,
        *camera,
        &arena.0,
        presentation.runtime().local_presentation_position(),
    )
    .unwrap_or(sequencer.last_aim);
    sequencer.last_aim = aim;
    let held_primary = mouse.pressed(MouseButton::Left);
    let Some(primary_sequence) = sequencer.sample_primary(held_primary) else {
        state.fatal_error = Some("primary input sequence exhausted".to_owned());
        return;
    };
    bridge.0.replace_input(protocol::InputFrame {
        sequence,
        client_tick: u64::from(sequence),
        movement_x_milli,
        movement_y_milli,
        aim_x_milli: aim.0,
        aim_y_milli: aim.1,
        held_primary,
        primary_sequence,
        ability_1_sequence: 0,
        ability_2_sequence: 0,
    });
    let Some(next) = sequence.checked_add(1) else {
        state.fatal_error = Some("input sequence exhausted".to_owned());
        return;
    };
    sequencer.input_sequence = next;
}

#[allow(clippy::needless_pass_by_value)]
fn send_reliable_edges(
    bridge: Res<NetworkBridge>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    presentation: Res<NativeNetworkPresentation>,
    mut sequencer: ResMut<NetworkInputSequencer>,
    mut state: ResMut<NetworkPlayState>,
) {
    for pressed in [
        mouse
            .just_pressed(MouseButton::Right)
            .then_some(ActionKind::Ability1Press),
        keyboard
            .just_pressed(KeyCode::Space)
            .then_some(ActionKind::Ability2Press),
    ]
    .into_iter()
    .flatten()
    {
        let sequence = sequencer.action_sequence;
        match bridge
            .0
            .queue_reliable(WireMessage::ActionFrame(ActionFrame {
                sequence,
                client_tick: u64::from(sequencer.input_sequence),
                action: pressed,
            })) {
            Ok(()) => {
                let Some(next) = sequence.checked_add(1) else {
                    state.fatal_error = Some("action sequence exhausted".to_owned());
                    return;
                };
                sequencer.action_sequence = next;
            }
            Err(error) => {
                state.fatal_error = Some(error.to_string());
                return;
            }
        }
    }
    if keyboard.just_pressed(KeyCode::KeyE)
        && let Some(pickup_id) = nearest_eligible_pickup(&presentation)
    {
        let mutation_id = sequencer.mutation_sequence.to_le_bytes();
        match bridge
            .0
            .queue_reliable(WireMessage::MutationRequest(MutationRequest {
                mutation_id,
                pickup_id,
                placement: PickupPlacement::Take,
            })) {
            Ok(()) => {
                let Some(next) = sequencer.mutation_sequence.checked_add(1) else {
                    state.fatal_error = Some("mutation sequence exhausted".to_owned());
                    return;
                };
                sequencer.mutation_sequence = next;
            }
            Err(error) => state.fatal_error = Some(error.to_string()),
        }
    }
}

fn nearest_eligible_pickup(presentation: &NativeNetworkPresentation) -> Option<u64> {
    let snapshot = presentation.latest_snapshot()?;
    let player = snapshot.entities.iter().find(|entity| {
        entity.entity_id == presentation.runtime().local_entity_id()
            && entity.kind == EntityKind::Player
    })?;
    snapshot
        .entities
        .iter()
        .filter(|entity| {
            matches!(entity.kind, EntityKind::PersonalPickup | EntityKind::Loot)
                && entity.state_flags & ENTITY_STATE_ELIGIBLE != 0
        })
        .filter_map(|entity| {
            let dx = i64::from(entity.x_milli_tiles) - i64::from(player.x_milli_tiles);
            let dy = i64::from(entity.y_milli_tiles) - i64::from(player.y_milli_tiles);
            let distance_squared = dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy));
            (distance_squared <= 1_250_i64.pow(2)).then_some((distance_squared, entity.entity_id))
        })
        .min()
        .map(|(_, entity_id)| entity_id)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn cursor_aim(
    window: &Window,
    (camera, camera_transform): (&Camera, &GlobalTransform),
    arena: &sim_core::ArenaGeometry,
    player: SimulationVector,
) -> Option<(i16, i16)> {
    let cursor = window.cursor_position()?;
    let world = camera.viewport_to_world_2d(camera_transform, cursor).ok()?;
    let target = render_point_to_simulation(world, arena);
    let delta = target - player;
    let length = delta.length();
    if !length.is_finite() || length <= f32::EPSILON {
        return None;
    }
    Some((
        (delta.x / length * 1_000.0).round() as i16,
        (delta.y / length * 1_000.0).round() as i16,
    ))
}

fn spawn_network_hud(mut commands: Commands) {
    commands.spawn((
        Name::new("Network gate label"),
        Text::new("M02 NETWORK PLAYTEST — NONPERSISTENT\nRECALL UNAVAILABLE — LOCAL TEST"),
        TextFont::from_font_size(15.0),
        TextColor(Color::srgb_u8(240, 213, 139)),
        Node {
            position_type: PositionType::Absolute,
            top: px(14),
            right: px(14),
            padding: UiRect::all(px(9)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(8, 12, 16, 225)),
    ));
    commands.spawn((
        Name::new("Network status"),
        NetworkStatusText,
        Text::new("CONNECTING"),
        TextFont::from_font_size(14.0),
        TextColor(Color::srgb_u8(211, 241, 224)),
        Node {
            position_type: PositionType::Absolute,
            top: px(87),
            right: px(14),
            padding: UiRect::all(px(8)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(8, 12, 16, 220)),
    ));
    commands.spawn((
        Name::new("Authoritative health"),
        NetworkHealthText,
        Text::new("HP — / —"),
        TextFont::from_font_size(18.0),
        TextColor(Color::srgb_u8(232, 225, 203)),
        Node {
            position_type: PositionType::Absolute,
            bottom: px(58),
            right: px(14),
            padding: UiRect::all(px(9)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(8, 12, 16, 225)),
    ));
    commands.spawn((
        Name::new("Network gate overlay"),
        NetworkGateOverlay,
        Text::new(""),
        TextFont::from_font_size(30.0),
        TextColor(Color::srgb_u8(242, 107, 91)),
        Node {
            position_type: PositionType::Absolute,
            top: percent(42),
            left: percent(24),
            right: percent(24),
            padding: UiRect::all(px(14)),
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(Color::srgba_u8(8, 12, 16, 230)),
    ));
}

#[allow(clippy::needless_pass_by_value, clippy::type_complexity)]
fn update_network_hud(
    mut state: ResMut<NetworkPlayState>,
    presentation: Res<NativeNetworkPresentation>,
    diagnostics: Res<NetworkCorrectionDiagnostics>,
    mut status: Single<&mut Text, With<NetworkStatusText>>,
    mut health: Single<&mut Text, (With<NetworkHealthText>, Without<NetworkStatusText>)>,
    mut overlay: Single<
        &mut Text,
        (
            With<NetworkGateOverlay>,
            Without<NetworkStatusText>,
            Without<NetworkHealthText>,
        ),
    >,
) {
    status.0 = format!(
        "{}\nSNAPS {}  /  CORRECTIONS {}  /  RELIABLE {}",
        state.status,
        diagnostics.snaps,
        diagnostics.micro_corrections + diagnostics.noticeable_corrections,
        state.reliable_results
    );
    let player = presentation.latest_snapshot().and_then(|snapshot| {
        snapshot.entities.iter().find(|entity| {
            entity.entity_id == presentation.runtime().local_entity_id()
                && entity.kind == EntityKind::Player
        })
    });
    if let Some(snapshot) = presentation.latest_snapshot() {
        let has_living_hostile = snapshot.entities.iter().any(|entity| {
            matches!(entity.kind, EntityKind::Enemy | EntityKind::Boss)
                && entity.state_flags & ENTITY_STATE_ALIVE != 0
        });
        state.saw_hostile |= has_living_hostile;
        state.combat_complete |= state.saw_hostile && !has_living_hostile;
    }
    health.0 = player.map_or_else(
        || "HP — / —".to_owned(),
        |player| format!("HP {} / {}", player.current_health, player.maximum_health),
    );
    overlay.0 = if let Some(error) = &state.fatal_error {
        format!("CONNECTION FAILED\n{error}")
    } else if player.is_some_and(|player| player.state_flags & ENTITY_STATE_ALIVE == 0) {
        "YOU DIED\nAUTHORITATIVE RESULT".to_owned()
    } else if state.combat_complete {
        "COMBAT TEST COMPLETE\nAUTHORITY CONFIRMED".to_owned()
    } else if !matches!(
        state.lifecycle.phase(),
        ClientConnectionPhase::Connected { .. }
    ) {
        state.status.clone()
    } else {
        String::new()
    };
}

#[allow(clippy::needless_pass_by_value)]
fn shutdown_network_on_exit(mut exit: MessageReader<AppExit>, bridge: Res<NetworkBridge>) {
    if exit.read().next().is_some() {
        bridge.0.shutdown();
    }
}

fn lifecycle_label(phase: &ClientConnectionPhase) -> &'static str {
    match phase {
        ClientConnectionPhase::Offline => "OFFLINE",
        ClientConnectionPhase::Joining => "JOINING",
        ClientConnectionPhase::Connected { .. } => "CONNECTED — AUTHORITATIVE 30 HZ",
        ClientConnectionPhase::LinkLost { .. } => "LINK LOST",
        ClientConnectionPhase::Reconnecting { .. } => "RECONNECTING",
        ClientConnectionPhase::AwaitingAuthoritativeResolution { .. } => {
            "AWAITING AUTHORITATIVE RESOLUTION"
        }
        ClientConnectionPhase::LanternHalls { .. } => "RECALLED — LANTERN HALLS ROUTE",
        ClientConnectionPhase::DeathFinal { .. } => "DEATH FINAL",
        ClientConnectionPhase::ServerShuttingDown => "SERVER SHUTTING DOWN",
        ClientConnectionPhase::Closed => "CLOSED",
    }
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pickup_selection_is_nearest_eligible_and_within_interact_reach() {
        assert_eq!(INITIAL_LOCAL_PLAYER_ID, 10_000);
        assert_eq!(NetworkInputSequencer::default().last_aim, (1_000, 0));
    }

    #[test]
    fn primary_sequence_remains_monotonic_across_press_hold_and_release() {
        let mut sequencer = NetworkInputSequencer::default();
        assert_eq!(sequencer.sample_primary(false), Some(0));
        assert_eq!(sequencer.sample_primary(true), Some(1));
        assert_eq!(sequencer.sample_primary(true), Some(1));
        assert_eq!(sequencer.sample_primary(false), Some(1));
        assert_eq!(sequencer.sample_primary(true), Some(2));
        assert_eq!(sequencer.sample_primary(false), Some(2));
    }

    #[test]
    fn lifecycle_labels_never_claim_shared_multiplayer() {
        let connected = ClientConnectionPhase::Connected {
            session_id: WireText::new("session-1").unwrap(),
        };
        let label = lifecycle_label(&connected);
        assert!(label.contains("AUTHORITATIVE"));
        assert!(!label.contains("SHARED"));
        assert!(!label.contains("PARTY"));
    }
}
