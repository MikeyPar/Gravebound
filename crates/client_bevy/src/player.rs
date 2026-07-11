use bevy::{log::info, prelude::*};
use sim_core::{
    GRAVE_ARBALIST_SPEED_TILES_PER_SECOND, MOVEMENT_RESPONSE_TICKS, MovementAction,
    PLAYER_COLLISION_RADIUS_TILES, PlayerMovementState,
};

use crate::{FixedSimulationSet, FrameSet, LoadedArena, arena_view::simulation_point_to_render};

pub const CAMERA_RESPONSE_SECONDS: f32 = 0.080;
const PLAYER_BODY_SIZE_TILES: f32 = 0.54;
const PLAYER_CORE_SIZE_TILES: f32 = 0.20;
const PLAYER_Z: f32 = 8.0;

/// Replaceable keyboard bindings for the `move` action. Systems never hardcode WASD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Resource)]
pub struct MovementBindings {
    pub up: KeyCode,
    pub left: KeyCode,
    pub down: KeyCode,
    pub right: KeyCode,
}

impl Default for MovementBindings {
    fn default() -> Self {
        Self {
            up: KeyCode::KeyW,
            left: KeyCode::KeyA,
            down: KeyCode::KeyS,
            right: KeyCode::KeyD,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, Resource)]
struct LatestMovementAction(MovementAction);

/// Simulation resource is the only client-side owner of authoritative player state.
#[derive(Debug, Resource)]
pub(crate) struct PlayerSimulation(PlayerMovementState);

impl PlayerSimulation {
    pub(crate) fn new(state: PlayerMovementState) -> Self {
        Self(state)
    }

    pub(crate) fn state(&self) -> PlayerMovementState {
        self.0
    }
}

#[derive(Debug, Component)]
pub(crate) struct LocalPlayer;

#[derive(Debug, Component)]
pub(crate) struct MovementDiagnostics;

/// Presentation-only camera state. This component never crosses into `sim_core`.
#[derive(Debug, Default, Component)]
pub(crate) struct CameraFollow {
    velocity: Vec2,
    initialized: bool,
}

pub(crate) fn configure(app: &mut App) {
    app.insert_resource(MovementBindings::default())
        .insert_resource(LatestMovementAction::default())
        .add_systems(Startup, spawn_player)
        .add_systems(
            FixedUpdate,
            simulate_player.in_set(FixedSimulationSet::Movement),
        )
        .add_systems(
            Update,
            (
                sample_movement_action.in_set(FrameSet::InputSample),
                follow_player_camera.in_set(FrameSet::CameraFollow),
                update_movement_diagnostics.in_set(FrameSet::Presentation),
            ),
        );
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn spawn_player(
    mut commands: Commands,
    arena: Res<LoadedArena>,
    simulation: Res<PlayerSimulation>,
) {
    let position = simulation_point_to_render(simulation.0.position(), &arena.0);
    commands
        .spawn((
            Name::new("Grave Arbalist"),
            LocalPlayer,
            Sprite::from_color(
                Color::srgb_u8(211, 241, 224),
                Vec2::splat(PLAYER_BODY_SIZE_TILES),
            ),
            Transform::from_xyz(position.x, position.y, PLAYER_Z)
                .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
        ))
        .with_child((
            Name::new("Grave Arbalist core"),
            Sprite::from_color(
                Color::srgb_u8(42, 167, 148),
                Vec2::splat(PLAYER_CORE_SIZE_TILES),
            ),
            Transform::from_xyz(0.0, 0.0, 0.1),
        ));

    info!(
        feature_id = "GB-M01-01B",
        spawn_x = simulation.0.position().x,
        spawn_y = simulation.0.position().y,
        speed_tiles_per_second = GRAVE_ARBALIST_SPEED_TILES_PER_SECOND,
        collision_radius_tiles = PLAYER_COLLISION_RADIUS_TILES,
        response_ticks = MOVEMENT_RESPONSE_TICKS,
        "fixed-step Grave Arbalist movement initialized"
    );
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn sample_movement_action(
    keyboard: Res<ButtonInput<KeyCode>>,
    bindings: Res<MovementBindings>,
    mut latest: ResMut<LatestMovementAction>,
) {
    latest.0 = movement_action_from_keyboard(&keyboard, *bindings);
}

fn movement_action_from_keyboard(
    keyboard: &ButtonInput<KeyCode>,
    bindings: MovementBindings,
) -> MovementAction {
    let horizontal =
        i8::from(keyboard.pressed(bindings.right)) - i8::from(keyboard.pressed(bindings.left));
    let vertical =
        i8::from(keyboard.pressed(bindings.down)) - i8::from(keyboard.pressed(bindings.up));
    MovementAction::new(horizontal, vertical)
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn simulate_player(
    mut simulation: ResMut<PlayerSimulation>,
    latest: Res<LatestMovementAction>,
    arena: Res<LoadedArena>,
    mut player: Single<&mut Transform, With<LocalPlayer>>,
) {
    simulation
        .0
        .step(latest.0, &arena.0)
        .expect("validated movement state must remain legal");
    let render_position = simulation_point_to_render(simulation.0.position(), &arena.0);
    player.translation.x = render_position.x;
    player.translation.y = render_position.y;
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn follow_player_camera(
    time: Res<Time>,
    player: Single<&Transform, (With<LocalPlayer>, Without<CameraFollow>)>,
    mut camera: Single<(&mut Transform, &mut CameraFollow), Without<LocalPlayer>>,
) {
    let target = player.translation.truncate();
    if !camera.1.initialized {
        camera.0.translation.x = target.x;
        camera.0.translation.y = target.y;
        camera.1.velocity = Vec2::ZERO;
        camera.1.initialized = true;
        return;
    }
    let current = camera.0.translation.truncate();
    let (next, velocity) = critically_damped_step(
        current,
        camera.1.velocity,
        target,
        time.delta_secs().clamp(0.0, 0.25),
    );
    camera.0.translation.x = next.x;
    camera.0.translation.y = next.y;
    camera.1.velocity = velocity;
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn update_movement_diagnostics(
    simulation: Res<PlayerSimulation>,
    mut diagnostics: Single<&mut Text, With<MovementDiagnostics>>,
) {
    if !simulation.is_changed() {
        return;
    }
    let state = simulation.state();
    diagnostics.0 = format!(
        "MOVE: WASD (REBIND-READY)  |  POS {:>6.2}, {:>6.2}  |  SPEED {:>4.2} / {:.1} TILES/S  |  RADIUS {:.2}",
        state.position().x,
        state.position().y,
        state.velocity().length(),
        GRAVE_ARBALIST_SPEED_TILES_PER_SECOND,
        PLAYER_COLLISION_RADIUS_TILES
    );
}

/// Exact critically damped stationary-target solution from ADR-002.
#[must_use]
pub fn critically_damped_step(
    current: Vec2,
    velocity: Vec2,
    target: Vec2,
    delta_seconds: f32,
) -> (Vec2, Vec2) {
    if !current.is_finite()
        || !velocity.is_finite()
        || !target.is_finite()
        || !delta_seconds.is_finite()
        || delta_seconds < 0.0
    {
        return (target, Vec2::ZERO);
    }
    let omega = 2.0 / CAMERA_RESPONSE_SECONDS;
    let displacement = current - target;
    let j = velocity + displacement * omega;
    let decay = (-omega * delta_seconds).exp();
    let next_displacement = (displacement + j * delta_seconds) * decay;
    let next_velocity = (velocity - j * (omega * delta_seconds)) * decay;
    (target + next_displacement, next_velocity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{ArenaGeometry, TilePoint};

    #[test]
    fn defaults_are_wasd_and_replaceable_without_simulation_changes() {
        let defaults = MovementBindings::default();
        assert_eq!(defaults.up, KeyCode::KeyW);
        assert_eq!(defaults.left, KeyCode::KeyA);
        assert_eq!(defaults.down, KeyCode::KeyS);
        assert_eq!(defaults.right, KeyCode::KeyD);
        let rebound = MovementBindings {
            up: KeyCode::ArrowUp,
            left: KeyCode::ArrowLeft,
            down: KeyCode::ArrowDown,
            right: KeyCode::ArrowRight,
        };
        assert_ne!(defaults, rebound);

        let mut keyboard = ButtonInput::default();
        keyboard.press(KeyCode::KeyW);
        keyboard.press(KeyCode::KeyD);
        assert_eq!(
            movement_action_from_keyboard(&keyboard, defaults).normalized_vector(),
            MovementAction::new(1, -1).normalized_vector()
        );
        assert_eq!(
            movement_action_from_keyboard(&keyboard, rebound),
            MovementAction::default()
        );
        keyboard.clear();
        keyboard.press(KeyCode::ArrowLeft);
        assert_eq!(
            movement_action_from_keyboard(&keyboard, rebound),
            MovementAction::new(-1, 0)
        );
    }

    #[test]
    fn opposing_bound_keys_cancel_per_axis() {
        let bindings = MovementBindings::default();
        let mut keyboard = ButtonInput::default();
        keyboard.press(KeyCode::KeyA);
        keyboard.press(KeyCode::KeyD);
        keyboard.press(KeyCode::KeyW);
        assert_eq!(
            movement_action_from_keyboard(&keyboard, bindings),
            MovementAction::new(0, -1)
        );
    }

    #[test]
    fn camera_spring_converges_without_overshoot_at_multiple_frame_rates() {
        fn run(delta: f32, frames: usize) -> Vec2 {
            let target = Vec2::new(4.0, -3.0);
            let mut position = Vec2::ZERO;
            let mut velocity = Vec2::ZERO;
            for _ in 0..frames {
                (position, velocity) = critically_damped_step(position, velocity, target, delta);
                assert!(position.x <= target.x && position.y >= target.y);
            }
            position
        }
        let at_60_hz = run(1.0 / 60.0, 60);
        let at_120_hz = run(1.0 / 120.0, 120);
        assert!((at_60_hz - at_120_hz).length() < 1.0e-4);
        assert!((at_60_hz - Vec2::new(4.0, -3.0)).length() < 1.0e-4);
    }

    #[test]
    fn camera_math_cannot_mutate_authoritative_state() {
        let arena = ArenaGeometry {
            id: "arena.camera_test".to_owned(),
            width_milli_tiles: 32_000,
            height_milli_tiles: 24_000,
            shell_thickness_milli_tiles: 1_000,
            player_spawn: TilePoint::new(4_000, 12_000),
            boss_spawn: TilePoint::new(24_000, 12_000),
            pillars: vec![],
            anchors: vec![],
        }
        .validated()
        .expect("arena");
        let state = PlayerMovementState::at_arena_spawn(&arena).expect("player");
        let before = state;
        let _ = critically_damped_step(Vec2::ZERO, Vec2::ZERO, Vec2::ONE, 1.0 / 60.0);
        assert_eq!(state, before);
    }

    #[test]
    fn invalid_camera_state_snaps_safely() {
        let target = Vec2::new(2.0, 3.0);
        let (position, velocity) =
            critically_damped_step(Vec2::new(f32::NAN, 0.0), Vec2::ZERO, target, 1.0 / 60.0);
        assert_eq!(position, target);
        assert_eq!(velocity, Vec2::ZERO);
    }
}
