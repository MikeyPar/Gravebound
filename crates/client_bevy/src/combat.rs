use std::env;

use anyhow::{Result, bail};
use bevy::{log::info, prelude::*, window::PrimaryWindow};
use sim_core::{
    AimDirection, CollisionTarget, CombatAction, EnemyHurtbox, EntityId, HurtboxError,
    MILLI_TILES_PER_TILE, PlayerCombatState, ProjectileCollisionWorld, SimulationVector,
};
use thiserror::Error;

use crate::{
    FixedSimulationSet, FrameSet, LoadedArena,
    arena_view::{render_point_to_simulation, simulation_point_to_render},
    player::{CameraFollow, LocalPlayer, PlayerSimulation},
};

const EVIDENCE_SCENARIO_ENV: &str = "GRAVEBOUND_EVIDENCE_SCENARIO";
const PRIMARY_FIRE_EAST_SCENARIO: &str = "primary_fire_east";
const COLLISION_SHOWCASE_SCENARIO: &str = "collision_showcase";
const FRIENDLY_PROJECTILE_Z: f32 = 6.0;
const AIM_PRESENTATION_Z: f32 = 8.3;
const RETICLE_FALLBACK_DISTANCE_TILES: f32 = 4.0;
const MUZZLE_OFFSET_TILES: f32 = 0.38;
const DEBUG_TARGET_Z: f32 = 5.0;
const CONTACT_EFFECT_SECONDS: f32 = 1.5;

const DEBUG_TARGETS: [(u64, f32, f32, f32); 3] = [
    (10_001, 8.0, 12.0, 0.34),
    (10_002, 13.5, 6.5, 0.42),
    (10_003, 18.0, 12.0, 0.55),
];

/// Replaceable binding for `primary_fire`; simulation code never reads mouse buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Resource)]
pub struct PrimaryFireBindings {
    pub primary: MouseButton,
}

impl Default for PrimaryFireBindings {
    fn default() -> Self {
        Self {
            primary: MouseButton::Left,
        }
    }
}

/// Future menus set this gate rather than pausing authoritative combat time.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Resource)]
pub struct CombatInputGate {
    pub blocked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Resource)]
pub(crate) enum EvidenceScenario {
    None,
    PrimaryFireEast,
    CollisionShowcase,
}

impl EvidenceScenario {
    pub(crate) fn from_environment(screenshot_requested: bool) -> Result<Self> {
        let value = env::var(EVIDENCE_SCENARIO_ENV).ok();
        Self::from_value(value.as_deref(), screenshot_requested)
    }

    fn from_value(value: Option<&str>, screenshot_requested: bool) -> Result<Self> {
        match value {
            None => Ok(Self::None),
            Some(PRIMARY_FIRE_EAST_SCENARIO) if screenshot_requested => Ok(Self::PrimaryFireEast),
            Some(COLLISION_SHOWCASE_SCENARIO) if screenshot_requested => {
                Ok(Self::CollisionShowcase)
            }
            Some(PRIMARY_FIRE_EAST_SCENARIO | COLLISION_SHOWCASE_SCENARIO) => {
                bail!("{EVIDENCE_SCENARIO_ENV} requires GRAVEBOUND_SCREENSHOT_PATH")
            }
            Some(other) => bail!("unknown {EVIDENCE_SCENARIO_ENV} value `{other}`"),
        }
    }
}

#[derive(Debug, Resource)]
pub(crate) struct CombatSimulation(PlayerCombatState);

impl CombatSimulation {
    pub(crate) fn new(state: PlayerCombatState) -> Self {
        Self(state)
    }
}

#[derive(Debug, Resource)]
pub(crate) struct CombatCollisionWorld(ProjectileCollisionWorld);

impl CombatCollisionWorld {
    pub(crate) fn new(world: ProjectileCollisionWorld) -> Self {
        Self(world)
    }
}

pub(crate) fn first_playable_debug_hurtboxes() -> Result<Vec<EnemyHurtbox>, HurtboxError> {
    DEBUG_TARGETS
        .into_iter()
        .map(|(id, x, y, radius)| {
            EnemyHurtbox::new(
                EntityId::new(id).expect("debug target IDs are nonzero"),
                SimulationVector::new(x, y),
                radius,
            )
        })
        .collect()
}

#[derive(Debug, Default, Resource)]
struct CombatInputSampler {
    latest: CombatAction,
    suppressed_until_release: bool,
}

#[derive(Debug, Default, Resource)]
struct AimPresentation {
    cursor_render_world: Option<Vec2>,
}

#[derive(Debug, Component)]
struct CrossbowPresentation;

#[derive(Debug, Component)]
struct AimGuide;

#[derive(Debug, Component)]
struct AimReticle;

#[derive(Debug, Component)]
struct ProjectilePresentation(EntityId);

#[derive(Debug, Component)]
struct DebugTargetPresentation;

#[derive(Debug, Component)]
struct CombatDiagnostics;

#[derive(Debug, Default, Resource)]
pub(crate) struct CollisionDiagnostics {
    enemy_hits: u64,
    solid_blocks: u64,
    last_target: Option<CollisionTarget>,
}

impl CollisionDiagnostics {
    pub(crate) const fn showcase_ready(&self) -> bool {
        self.enemy_hits > 0 && self.solid_blocks > 0
    }
}

#[derive(Debug, Component)]
struct TransientEffect {
    remaining_seconds: f32,
    total_seconds: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
enum PrimarySequenceError {
    #[error("primary press sequence exhausted u32")]
    Exhausted,
}

pub(crate) fn configure(app: &mut App) {
    app.insert_resource(PrimaryFireBindings::default())
        .insert_resource(CombatInputGate::default())
        .insert_resource(CombatInputSampler::default())
        .insert_resource(AimPresentation::default())
        .insert_resource(CollisionDiagnostics::default())
        .add_systems(Startup, spawn_combat_presentation)
        .add_systems(
            FixedUpdate,
            simulate_combat.in_set(FixedSimulationSet::Combat),
        )
        .add_systems(
            Update,
            (
                sample_combat_input.in_set(FrameSet::InputSample),
                (
                    update_aim_presentation,
                    update_combat_diagnostics,
                    update_transient_effects,
                    draw_collision_debug,
                )
                    .in_set(FrameSet::Presentation),
            ),
        );
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn spawn_combat_presentation(
    mut commands: Commands,
    arena: Res<LoadedArena>,
    player: Res<PlayerSimulation>,
    combat: Res<CombatSimulation>,
    collision_world: Res<CombatCollisionWorld>,
) {
    let player_render = simulation_point_to_render(player.state().position(), &arena.0);
    let east = Vec2::X;
    commands.spawn((
        Name::new("Pine Crossbow"),
        CrossbowPresentation,
        Sprite::from_color(Color::srgb_u8(173, 141, 79), Vec2::new(0.58, 0.12)),
        Transform::from_xyz(
            player_render.x + east.x * 0.28,
            player_render.y + east.y * 0.28,
            AIM_PRESENTATION_Z,
        ),
    ));
    for hurtbox in collision_world.0.enemies() {
        let render = simulation_point_to_render(hurtbox.center(), &arena.0);
        commands
            .spawn((
                Name::new(format!("Debug enemy target {}", hurtbox.id())),
                DebugTargetPresentation,
                Sprite::from_color(
                    Color::srgba_u8(42, 167, 148, 95),
                    Vec2::splat(hurtbox.radius_tiles() * 1.4),
                ),
                Transform::from_xyz(render.x, render.y, DEBUG_TARGET_Z)
                    .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
            ))
            .with_children(|parent| {
                parent.spawn((
                    Sprite::from_color(
                        Color::srgb_u8(211, 241, 224),
                        Vec2::new(hurtbox.radius_tiles() * 1.4, 0.035),
                    ),
                    Transform::from_xyz(0.0, 0.0, 0.1)
                        .with_rotation(Quat::from_rotation_z(-std::f32::consts::FRAC_PI_4)),
                ));
            });
    }
    commands.spawn((
        Name::new("Aim guide"),
        AimGuide,
        Sprite::from_color(Color::srgba_u8(211, 241, 224, 65), Vec2::new(1.0, 0.025)),
        Transform::from_xyz(
            player_render.x + 0.75,
            player_render.y,
            AIM_PRESENTATION_Z - 0.2,
        ),
    ));
    let reticle_position = player_render + east * RETICLE_FALLBACK_DISTANCE_TILES;
    commands
        .spawn((
            Name::new("Aim reticle"),
            AimReticle,
            Transform::from_xyz(reticle_position.x, reticle_position.y, AIM_PRESENTATION_Z),
            Visibility::default(),
        ))
        .with_children(|parent| {
            parent.spawn((
                Sprite::from_color(Color::srgb_u8(211, 241, 224), Vec2::new(0.34, 0.045)),
                Transform::default(),
            ));
            parent.spawn((
                Sprite::from_color(Color::srgb_u8(211, 241, 224), Vec2::new(0.045, 0.34)),
                Transform::default(),
            ));
        });
    commands.spawn((
        Name::new("Combat diagnostics"),
        CombatDiagnostics,
        Text::new("PRIMARY: LMB (REBIND-READY)  |  PINE CROSSBOW"),
        TextFont::from_font_size(13.0),
        TextColor(Color::srgb_u8(231, 210, 157)),
        Node {
            position_type: PositionType::Absolute,
            top: px(145),
            left: px(14),
            border: UiRect::all(px(1)),
            padding: UiRect::all(px(7)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(8, 12, 16, 220)),
        BorderColor::all(Color::srgba_u8(173, 141, 79, 180)),
    ));

    let weapon = combat.0.weapon();
    info!(
        feature_id = "GB-M01-02B",
        weapon_id = weapon.content_id(),
        damage = weapon.raw_damage(),
        interval_ticks = weapon.attack_interval_ticks(),
        lifetime_ticks = weapon.projectile_lifetime_ticks(),
        range_tiles = weapon.range_tiles(),
        speed_tiles_per_second = weapon.projectile_speed_tiles_per_second(),
        radius_tiles = weapon.projectile_radius_tiles(),
        debug_hurtboxes = collision_world.0.enemies().len(),
        "Pine Crossbow collision presentation initialized"
    );
}

#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
fn sample_combat_input(
    mouse: Res<ButtonInput<MouseButton>>,
    bindings: Res<PrimaryFireBindings>,
    gate: Res<CombatInputGate>,
    scenario: Res<EvidenceScenario>,
    arena: Res<LoadedArena>,
    player: Res<PlayerSimulation>,
    combat: Res<CombatSimulation>,
    camera: Single<(&Camera, &GlobalTransform), With<CameraFollow>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut sampler: ResMut<CombatInputSampler>,
    mut presentation: ResMut<AimPresentation>,
) {
    if matches!(
        *scenario,
        EvidenceScenario::PrimaryFireEast | EvidenceScenario::CollisionShowcase
    ) {
        let showcase_west = *scenario == EvidenceScenario::CollisionShowcase
            && (combat.0.tick().0 < 14 || combat.0.tick().0 >= 28);
        let aim = if showcase_west {
            AimDirection::new(SimulationVector::new(-1.0, 0.0)).expect("west aim")
        } else {
            AimDirection::east()
        };
        sampler.latest.aim = aim;
        sampler.latest.primary_held = true;
        if sampler.latest.primary_press_sequence == 0 {
            sampler.latest.primary_press_sequence = 1;
        }
        let target = player.state().position() + aim.vector() * RETICLE_FALLBACK_DISTANCE_TILES;
        presentation.cursor_render_world = Some(simulation_point_to_render(target, &arena.0));
        return;
    }

    if let Some(cursor_viewport) = window.cursor_position()
        && let Ok(cursor_render) = camera.0.viewport_to_world_2d(camera.1, cursor_viewport)
    {
        let cursor_simulation = render_point_to_simulation(cursor_render, &arena.0);
        if let Ok(aim) = aim_from_simulation_points(player.state().position(), cursor_simulation) {
            sampler.latest.aim = aim;
            presentation.cursor_render_world = Some(cursor_render);
        }
    }
    sample_primary_button(&mut sampler, mouse.pressed(bindings.primary), gate.blocked)
        .expect("primary sequence space must not exhaust during LocalLab");
}

fn sample_primary_button(
    sampler: &mut CombatInputSampler,
    physically_pressed: bool,
    blocked: bool,
) -> Result<(), PrimarySequenceError> {
    if blocked {
        sampler.latest.primary_held = false;
        sampler.suppressed_until_release |= physically_pressed;
        return Ok(());
    }
    if sampler.suppressed_until_release {
        sampler.latest.primary_held = false;
        if !physically_pressed {
            sampler.suppressed_until_release = false;
        }
        return Ok(());
    }
    if physically_pressed && !sampler.latest.primary_held {
        sampler.latest.primary_press_sequence = sampler
            .latest
            .primary_press_sequence
            .checked_add(1)
            .ok_or(PrimarySequenceError::Exhausted)?;
    }
    sampler.latest.primary_held = physically_pressed;
    Ok(())
}

fn aim_from_simulation_points(
    player: SimulationVector,
    target: SimulationVector,
) -> Result<AimDirection, sim_core::AimDirectionError> {
    AimDirection::new(target - player)
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)] // Bevy system parameters are wrapper values.
fn simulate_combat(
    mut commands: Commands,
    mut combat: ResMut<CombatSimulation>,
    collision_world: Res<CombatCollisionWorld>,
    input: Res<CombatInputSampler>,
    player: Res<PlayerSimulation>,
    arena: Res<LoadedArena>,
    mut visuals: Query<(Entity, &ProjectilePresentation, &mut Transform)>,
    mut collision_diagnostics: ResMut<CollisionDiagnostics>,
) {
    let step = combat
        .0
        .step(input.latest, player.state().position(), &collision_world.0)
        .expect("validated LocalLab combat input must remain legal");
    for collision in &step.collisions {
        if let Some((entity, _, _)) = visuals
            .iter_mut()
            .find(|(_, visual, _)| visual.0 == collision.projectile_id)
        {
            commands.entity(entity).despawn();
        }
        let (name, color, size) = match collision.target {
            CollisionTarget::Solid(_) => {
                collision_diagnostics.solid_blocks =
                    collision_diagnostics.solid_blocks.saturating_add(1);
                ("Solid block", Color::srgb_u8(240, 184, 92), 0.36)
            }
            CollisionTarget::Enemy(_) => {
                collision_diagnostics.enemy_hits =
                    collision_diagnostics.enemy_hits.saturating_add(1);
                ("Enemy hit", Color::srgb_u8(82, 211, 178), 0.48)
            }
        };
        collision_diagnostics.last_target = Some(collision.target);
        let position = simulation_point_to_render(collision.final_position, &arena.0);
        spawn_contact_transient(&mut commands, name, position, color, size, collision.target);
        info!(
            feature_id = "GB-M01-02B",
            tick = collision.tick.0,
            projectile_id = collision.projectile_id.get(),
            target = %collision.target,
            position_x = collision.final_position.x,
            position_y = collision.final_position.y,
            distance_tiles = collision.distance_travelled_tiles,
            "primary bolt collision"
        );
    }
    for expiration in &step.expirations {
        if let Some((entity, _, _)) = visuals
            .iter_mut()
            .find(|(_, visual, _)| visual.0 == expiration.projectile_id)
        {
            commands.entity(entity).despawn();
        }
        let position = simulation_point_to_render(expiration.final_position, &arena.0);
        spawn_transient(
            &mut commands,
            "Range expiry",
            position,
            Color::srgba_u8(211, 241, 224, 150),
            0.32,
            0.12,
        );
    }
    for shot in &step.shots {
        spawn_projectile(&mut commands, &shot.projectile, &arena.0);
        let direction = simulation_direction_to_render(shot.projectile.direction().vector());
        let origin = simulation_point_to_render(shot.projectile.origin(), &arena.0);
        spawn_transient(
            &mut commands,
            "Muzzle flash",
            origin + direction * MUZZLE_OFFSET_TILES,
            Color::srgba_u8(240, 213, 139, 220),
            0.24,
            0.07,
        );
        info!(
            feature_id = "GB-M01-02B",
            tick = shot.tick.0,
            press_sequence = shot.press_sequence,
            projectile_id = shot.projectile.id().get(),
            origin_x = shot.projectile.origin().x,
            origin_y = shot.projectile.origin().y,
            direction_x = shot.projectile.direction().vector().x,
            direction_y = shot.projectile.direction().vector().y,
            "primary bolt fired"
        );
    }
    for (_, visual, mut transform) in &mut visuals {
        if let Some(projectile) = combat
            .0
            .projectiles()
            .iter()
            .find(|projectile| projectile.id() == visual.0)
        {
            *transform = projectile_transform(projectile, &arena.0);
        }
    }
}

fn spawn_contact_transient(
    commands: &mut Commands,
    name: &str,
    position: Vec2,
    color: Color,
    size: f32,
    target: CollisionTarget,
) {
    let entity = commands
        .spawn((
            Name::new(name.to_owned()),
            TransientEffect {
                remaining_seconds: CONTACT_EFFECT_SECONDS,
                total_seconds: CONTACT_EFFECT_SECONDS,
            },
            Transform::from_xyz(position.x, position.y, FRIENDLY_PROJECTILE_Z + 0.4),
            Visibility::default(),
        ))
        .id();
    commands
        .entity(entity)
        .with_children(|parent| match target {
            CollisionTarget::Solid(_) => {
                for rotation in [0.0, std::f32::consts::FRAC_PI_2] {
                    parent.spawn((
                        Sprite::from_color(color, Vec2::new(size, 0.07)),
                        Transform::from_rotation(Quat::from_rotation_z(rotation)),
                    ));
                }
            }
            CollisionTarget::Enemy(_) => {
                for rotation in [std::f32::consts::FRAC_PI_4, -std::f32::consts::FRAC_PI_4] {
                    parent.spawn((
                        Sprite::from_color(color, Vec2::new(size, 0.07)),
                        Transform::from_rotation(Quat::from_rotation_z(rotation)),
                    ));
                }
            }
        });
}

fn spawn_projectile(
    commands: &mut Commands,
    projectile: &sim_core::FriendlyProjectile,
    arena: &sim_core::ArenaGeometry,
) {
    commands
        .spawn((
            Name::new(format!("Pine bolt {}", projectile.id())),
            ProjectilePresentation(projectile.id()),
            Sprite::from_color(
                Color::srgb_u8(231, 224, 199),
                Vec2::new(0.34, projectile.radius_tiles() * 2.0),
            ),
            projectile_transform(projectile, arena),
        ))
        .with_child((
            Name::new("Pine bolt core"),
            Sprite::from_color(Color::srgb_u8(173, 141, 79), Vec2::new(0.18, 0.035)),
            Transform::from_xyz(0.03, 0.0, 0.1),
        ));
}

fn projectile_transform(
    projectile: &sim_core::FriendlyProjectile,
    arena: &sim_core::ArenaGeometry,
) -> Transform {
    let position = simulation_point_to_render(projectile.position(), arena);
    let direction = simulation_direction_to_render(projectile.direction().vector());
    Transform::from_xyz(position.x, position.y, FRIENDLY_PROJECTILE_Z)
        .with_rotation(Quat::from_rotation_z(direction.y.atan2(direction.x)))
}

fn spawn_transient(
    commands: &mut Commands,
    name: &str,
    position: Vec2,
    color: Color,
    size: f32,
    duration: f32,
) {
    commands.spawn((
        Name::new(name.to_owned()),
        TransientEffect {
            remaining_seconds: duration,
            total_seconds: duration,
        },
        Sprite::from_color(color, Vec2::splat(size)),
        Transform::from_xyz(position.x, position.y, FRIENDLY_PROJECTILE_Z + 0.2)
            .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
    ));
}

#[allow(
    clippy::needless_pass_by_value,
    clippy::type_complexity // Disjoint Bevy filters prove mutable query compatibility.
)]
fn update_aim_presentation(
    input: Res<CombatInputSampler>,
    aim_target: Res<AimPresentation>,
    player: Single<&Transform, (With<LocalPlayer>, Without<CrossbowPresentation>)>,
    mut crossbow: Single<
        &mut Transform,
        (
            With<CrossbowPresentation>,
            Without<LocalPlayer>,
            Without<AimGuide>,
            Without<AimReticle>,
        ),
    >,
    mut guide: Single<
        &mut Transform,
        (
            With<AimGuide>,
            Without<LocalPlayer>,
            Without<CrossbowPresentation>,
            Without<AimReticle>,
        ),
    >,
    mut reticle: Single<
        &mut Transform,
        (
            With<AimReticle>,
            Without<LocalPlayer>,
            Without<CrossbowPresentation>,
            Without<AimGuide>,
        ),
    >,
) {
    let direction = simulation_direction_to_render(input.latest.aim.vector());
    let angle = direction.y.atan2(direction.x);
    let player_position = player.translation.truncate();
    crossbow.translation.x = player_position.x + direction.x * 0.28;
    crossbow.translation.y = player_position.y + direction.y * 0.28;
    crossbow.rotation = Quat::from_rotation_z(angle);
    guide.translation.x = player_position.x + direction.x * 0.75;
    guide.translation.y = player_position.y + direction.y * 0.75;
    guide.rotation = Quat::from_rotation_z(angle);
    let target = aim_target
        .cursor_render_world
        .unwrap_or(player_position + direction * RETICLE_FALLBACK_DISTANCE_TILES);
    reticle.translation.x = target.x;
    reticle.translation.y = target.y;
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn update_combat_diagnostics(
    combat: Res<CombatSimulation>,
    input: Res<CombatInputSampler>,
    gate: Res<CombatInputGate>,
    collision_diagnostics: Res<CollisionDiagnostics>,
    mut diagnostics: Single<&mut Text, With<CombatDiagnostics>>,
) {
    if !combat.is_changed()
        && !input.is_changed()
        && !gate.is_changed()
        && !collision_diagnostics.is_changed()
    {
        return;
    }
    let state = &combat.0;
    let status = if gate.blocked {
        "BLOCKED".to_owned()
    } else if state.interval_remaining_ticks() == 0 {
        "READY".to_owned()
    } else {
        format!("CD {}T", state.interval_remaining_ticks())
    };
    let aim = input.latest.aim.vector();
    let angle = aim.y.atan2(aim.x).to_degrees().rem_euclid(360.0);
    let last = collision_diagnostics
        .last_target
        .map_or_else(|| "NONE".to_owned(), |target| target.to_string());
    diagnostics.0 = format!(
        "PRIMARY: LMB  |  PINE CROSSBOW 20 DMG  14T  9.5 RNG  12 SPD  |  {status}  |  AIM {angle:>5.1} DEG  |  BOLTS {}\nCOLLISION ACTIVE  |  ENEMY HITS {}  |  SOLID BLOCKS {}  |  LAST {last}  |  DAMAGE DEFERRED",
        state.projectiles().len(),
        collision_diagnostics.enemy_hits,
        collision_diagnostics.solid_blocks
    );
}

#[allow(clippy::cast_precision_loss, clippy::needless_pass_by_value)]
fn draw_collision_debug(
    mut gizmos: Gizmos,
    arena: Res<LoadedArena>,
    collision_world: Res<CombatCollisionWorld>,
    combat: Res<CombatSimulation>,
    targets: Query<&DebugTargetPresentation>,
) {
    let weapon_radius = combat.0.weapon().projectile_radius_tiles();
    let width = arena.0.width_milli_tiles as f32 / MILLI_TILES_PER_TILE as f32;
    let height = arena.0.height_milli_tiles as f32 / MILLI_TILES_PER_TILE as f32;
    gizmos.rect_2d(
        Isometry2d::from_translation(Vec2::ZERO),
        Vec2::new(width - weapon_radius * 2.0, height - weapon_radius * 2.0),
        Color::srgba_u8(240, 184, 92, 180),
    );
    for pillar in &arena.0.pillars {
        let width = pillar.width_milli_tiles as f32 / MILLI_TILES_PER_TILE as f32;
        let height = pillar.height_milli_tiles as f32 / MILLI_TILES_PER_TILE as f32;
        let center = SimulationVector::new(
            (pillar.x_milli_tiles + pillar.width_milli_tiles / 2) as f32
                / MILLI_TILES_PER_TILE as f32,
            (pillar.y_milli_tiles + pillar.height_milli_tiles / 2) as f32
                / MILLI_TILES_PER_TILE as f32,
        );
        gizmos
            .rounded_rect_2d(
                Isometry2d::from_translation(simulation_point_to_render(center, &arena.0)),
                Vec2::new(width + weapon_radius * 2.0, height + weapon_radius * 2.0),
                Color::srgba_u8(240, 184, 92, 200),
            )
            .corner_radius(weapon_radius);
    }
    for hurtbox in collision_world.0.enemies() {
        gizmos
            .circle_2d(
                Isometry2d::from_translation(simulation_point_to_render(
                    hurtbox.center(),
                    &arena.0,
                )),
                hurtbox.radius_tiles(),
                Color::srgb_u8(82, 211, 178),
            )
            .resolution(32);
    }
    for projectile in combat.0.projectiles() {
        gizmos
            .circle_2d(
                Isometry2d::from_translation(simulation_point_to_render(
                    projectile.position(),
                    &arena.0,
                )),
                projectile.radius_tiles(),
                Color::srgb_u8(231, 224, 199),
            )
            .resolution(16);
    }
    debug_assert_eq!(targets.iter().count(), collision_world.0.enemies().len());
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn update_transient_effects(
    time: Res<Time>,
    mut commands: Commands,
    mut effects: Query<(Entity, &mut TransientEffect, &mut Transform)>,
) {
    for (entity, mut effect, mut transform) in &mut effects {
        effect.remaining_seconds -= time.delta_secs();
        if effect.remaining_seconds <= 0.0 {
            commands.entity(entity).despawn();
        } else {
            let scale = (effect.remaining_seconds / effect.total_seconds).clamp(0.0, 1.0);
            transform.scale = Vec3::splat(scale);
        }
    }
}

#[must_use]
fn simulation_direction_to_render(direction: SimulationVector) -> Vec2 {
    Vec2::new(direction.x, -direction.y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{ArenaGeometry, PlayerMovementState, TilePoint};

    fn arena() -> ArenaGeometry {
        ArenaGeometry {
            id: "arena.combat_client_test".to_owned(),
            width_milli_tiles: 32_000,
            height_milli_tiles: 24_000,
            shell_thickness_milli_tiles: 1_000,
            player_spawn: TilePoint::new(4_000, 12_000),
            boss_spawn: TilePoint::new(24_000, 12_000),
            pillars: vec![],
            anchors: vec![],
        }
        .validated()
        .expect("arena")
    }

    #[test]
    fn primary_binding_defaults_to_left_mouse_and_is_replaceable() {
        let defaults = PrimaryFireBindings::default();
        assert_eq!(defaults.primary, MouseButton::Left);
        assert_ne!(
            defaults,
            PrimaryFireBindings {
                primary: MouseButton::Right
            }
        );
    }

    #[test]
    fn button_sampler_sequences_once_per_physical_press() {
        let mut sampler = CombatInputSampler::default();
        sample_primary_button(&mut sampler, true, false).expect("press");
        assert_eq!(sampler.latest.primary_press_sequence, 1);
        assert!(sampler.latest.primary_held);
        sample_primary_button(&mut sampler, true, false).expect("hold");
        assert_eq!(sampler.latest.primary_press_sequence, 1);
        sample_primary_button(&mut sampler, false, false).expect("release");
        sample_primary_button(&mut sampler, true, false).expect("second press");
        assert_eq!(sampler.latest.primary_press_sequence, 2);
    }

    #[test]
    fn blocked_press_requires_release_before_rearming() {
        let mut sampler = CombatInputSampler::default();
        sample_primary_button(&mut sampler, true, true).expect("blocked press");
        assert!(!sampler.latest.primary_held);
        assert_eq!(sampler.latest.primary_press_sequence, 0);
        sample_primary_button(&mut sampler, true, false).expect("still suppressed");
        assert!(!sampler.latest.primary_held);
        sample_primary_button(&mut sampler, false, false).expect("rearm");
        sample_primary_button(&mut sampler, true, false).expect("fresh press");
        assert_eq!(sampler.latest.primary_press_sequence, 1);
    }

    #[test]
    fn sequence_overflow_fails_without_wrapping() {
        let mut sampler = CombatInputSampler::default();
        sampler.latest.primary_press_sequence = u32::MAX;
        assert_eq!(
            sample_primary_button(&mut sampler, true, false),
            Err(PrimarySequenceError::Exhausted)
        );
        assert_eq!(sampler.latest.primary_press_sequence, u32::MAX);
        assert!(!sampler.latest.primary_held);
    }

    #[test]
    fn cursor_world_target_maps_to_northwest_aim() {
        let arena = arena();
        let player = SimulationVector::new(4.0, 12.0);
        let target = render_point_to_simulation(Vec2::new(-8.0, -3.0), &arena);
        assert_eq!(target, SimulationVector::new(8.0, 15.0));
        let aim = aim_from_simulation_points(player, target).expect("aim");
        assert!((aim.vector().x - 0.8).abs() < 1.0e-6);
        assert!((aim.vector().y - 0.6).abs() < 1.0e-6);
        assert_eq!(
            simulation_direction_to_render(aim.vector()),
            Vec2::new(0.8, -0.6)
        );
    }

    #[test]
    fn coincident_cursor_preserves_last_aim_by_rejecting_update() {
        let player = SimulationVector::new(4.0, 12.0);
        assert!(aim_from_simulation_points(player, player).is_err());
    }

    #[test]
    fn evidence_scenario_is_strict_and_screenshot_only() {
        assert_eq!(
            EvidenceScenario::from_value(None, false).expect("none"),
            EvidenceScenario::None
        );
        assert!(EvidenceScenario::from_value(Some(PRIMARY_FIRE_EAST_SCENARIO), false).is_err());
        assert_eq!(
            EvidenceScenario::from_value(Some(PRIMARY_FIRE_EAST_SCENARIO), true).expect("scenario"),
            EvidenceScenario::PrimaryFireEast
        );
        assert_eq!(
            EvidenceScenario::from_value(Some(COLLISION_SHOWCASE_SCENARIO), true)
                .expect("collision scenario"),
            EvidenceScenario::CollisionShowcase
        );
        assert!(EvidenceScenario::from_value(Some("unknown"), true).is_err());
    }

    #[test]
    fn aim_calculation_cannot_mutate_movement_state() {
        let arena = arena();
        let movement = PlayerMovementState::at_arena_spawn(&arena).expect("movement");
        let before = movement;
        let _ = aim_from_simulation_points(movement.position(), SimulationVector::new(20.0, 10.0));
        assert_eq!(movement, before);
    }
}
