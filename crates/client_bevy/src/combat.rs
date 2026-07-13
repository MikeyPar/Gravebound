use std::env;

use anyhow::{Result, bail};
use bevy::{
    log::{info, warn},
    prelude::*,
    window::PrimaryWindow,
};
use sim_core::{
    AimDirection, CollisionTarget, CombatAction, EnemyHurtbox, EntityId, FriendlyProjectileSource,
    HurtboxError, MILLI_TILES_PER_TILE, PlayerCombatState, ProjectileCollisionWorld,
    RawDamageIntentSource, SimulationVector,
};
use thiserror::Error;

use crate::{
    FixedSimulationSet, FrameSet, LoadedArena,
    arena_view::{render_point_to_simulation, simulation_point_to_render},
    enemies::EnemyLabRuntime,
    player::{CameraFollow, LatestMovementAction, LocalPlayer, PlayerSimulation},
};

const EVIDENCE_SCENARIO_ENV: &str = "GRAVEBOUND_EVIDENCE_SCENARIO";
const PRIMARY_FIRE_EAST_SCENARIO: &str = "primary_fire_east";
const COLLISION_SHOWCASE_SCENARIO: &str = "collision_showcase";
const GRAVE_MARK_SHOWCASE_SCENARIO: &str = "grave_mark_showcase";
const SLIPSTEP_SHOWCASE_SCENARIO: &str = "slipstep_showcase";
const STILLNESS_SHOWCASE_SCENARIO: &str = "stillness_showcase";
const RED_TONIC_SHOWCASE_SCENARIO: &str = "red_tonic_showcase";
const ENEMY_SHOWCASE_SCENARIO: &str = "enemy_showcase";
const ENEMY_DEATH_SHOWCASE_SCENARIO: &str = "enemy_death_showcase";
const DAMAGE_LETHAL_SHOWCASE_SCENARIO: &str = "damage_lethal_showcase";
const DAMAGE_GRACE_SHOWCASE_SCENARIO: &str = "damage_grace_showcase";
const DEATH_RESTART_SHOWCASE_SCENARIO: &str = "death_restart_showcase";
const DEATH_RECAP_SHOWCASE_SCENARIO: &str = "death_recap_showcase";
const INVENTORY_SHOWCASE_SCENARIO: &str = "inventory_showcase";
const ITEM_CATALOG_SHOWCASE_SCENARIO: &str = "item_catalog_showcase";
const DEBUG_OVERLAY_SHOWCASE_SCENARIO: &str = "debug_overlay_showcase";
const DEBUG_TOOLS_SHOWCASE_SCENARIO: &str = "debug_tools_showcase";
const BOSS_SHOWCASE_SCENARIO: &str = "boss_showcase";
const BOSS_COMPLETION_SHOWCASE_SCENARIO: &str = "boss_completion_showcase";
const STRESS_FULL_SCENARIO: &str = "stress_full";
const STRESS_REDUCED_SCENARIO: &str = "stress_reduced";
pub(crate) const FRIENDLY_PROJECTILE_Z: f32 = 6.0;
const AIM_PRESENTATION_Z: f32 = 8.3;
const RETICLE_FALLBACK_DISTANCE_TILES: f32 = 4.0;
const MUZZLE_OFFSET_TILES: f32 = 0.38;
const DEBUG_TARGET_Z: f32 = 5.0;
const CONTACT_EFFECT_SECONDS: f32 = 1.5;

const DEBUG_TARGETS: [(u64, f32, f32, f32); 3] = [
    (10_001, 8.0, 12.0, 0.34),
    (10_002, 10.0, 12.0, 0.42),
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

/// Replaceable binding for `ability_1`; simulation code never reads mouse buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Resource)]
pub struct AbilityOneBindings {
    pub primary: MouseButton,
}

impl Default for AbilityOneBindings {
    fn default() -> Self {
        Self {
            primary: MouseButton::Right,
        }
    }
}

/// Replaceable keyboard/gamepad bindings for `ability_2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Resource)]
pub struct AbilityTwoBindings {
    pub keyboard: KeyCode,
    pub gamepad: GamepadButton,
}

impl Default for AbilityTwoBindings {
    fn default() -> Self {
        Self {
            keyboard: KeyCode::Space,
            gamepad: GamepadButton::LeftTrigger,
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
    GraveMarkShowcase,
    SlipstepShowcase,
    StillnessShowcase,
    RedTonicShowcase,
    EnemyShowcase,
    EnemyDeathShowcase,
    DamageLethalShowcase,
    DamageGraceShowcase,
    DeathRestartShowcase,
    DeathRecapShowcase,
    InventoryShowcase,
    ItemCatalogShowcase,
    DebugOverlayShowcase,
    DebugToolsShowcase,
    BossShowcase,
    BossCompletionShowcase,
    StressFull,
    StressReduced,
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
            Some(GRAVE_MARK_SHOWCASE_SCENARIO) if screenshot_requested => {
                Ok(Self::GraveMarkShowcase)
            }
            Some(SLIPSTEP_SHOWCASE_SCENARIO) if screenshot_requested => Ok(Self::SlipstepShowcase),
            Some(STILLNESS_SHOWCASE_SCENARIO) if screenshot_requested => {
                Ok(Self::StillnessShowcase)
            }
            Some(RED_TONIC_SHOWCASE_SCENARIO) if screenshot_requested => Ok(Self::RedTonicShowcase),
            Some(ENEMY_SHOWCASE_SCENARIO) if screenshot_requested => Ok(Self::EnemyShowcase),
            Some(ENEMY_DEATH_SHOWCASE_SCENARIO) if screenshot_requested => {
                Ok(Self::EnemyDeathShowcase)
            }
            Some(DAMAGE_LETHAL_SHOWCASE_SCENARIO) if screenshot_requested => {
                Ok(Self::DamageLethalShowcase)
            }
            Some(DAMAGE_GRACE_SHOWCASE_SCENARIO) if screenshot_requested => {
                Ok(Self::DamageGraceShowcase)
            }
            Some(DEATH_RESTART_SHOWCASE_SCENARIO) if screenshot_requested => {
                Ok(Self::DeathRestartShowcase)
            }
            Some(DEATH_RECAP_SHOWCASE_SCENARIO) if screenshot_requested => {
                Ok(Self::DeathRecapShowcase)
            }
            Some(INVENTORY_SHOWCASE_SCENARIO) if screenshot_requested => {
                Ok(Self::InventoryShowcase)
            }
            Some(ITEM_CATALOG_SHOWCASE_SCENARIO) if screenshot_requested => {
                Ok(Self::ItemCatalogShowcase)
            }
            Some(DEBUG_OVERLAY_SHOWCASE_SCENARIO) if screenshot_requested => {
                Ok(Self::DebugOverlayShowcase)
            }
            Some(DEBUG_TOOLS_SHOWCASE_SCENARIO) if screenshot_requested => {
                Ok(Self::DebugToolsShowcase)
            }
            Some(BOSS_SHOWCASE_SCENARIO) if screenshot_requested => Ok(Self::BossShowcase),
            Some(BOSS_COMPLETION_SHOWCASE_SCENARIO) if screenshot_requested => {
                Ok(Self::BossCompletionShowcase)
            }
            Some(STRESS_FULL_SCENARIO) if screenshot_requested => Ok(Self::StressFull),
            Some(STRESS_REDUCED_SCENARIO) if screenshot_requested => Ok(Self::StressReduced),
            Some(
                PRIMARY_FIRE_EAST_SCENARIO
                | COLLISION_SHOWCASE_SCENARIO
                | GRAVE_MARK_SHOWCASE_SCENARIO
                | SLIPSTEP_SHOWCASE_SCENARIO
                | STILLNESS_SHOWCASE_SCENARIO
                | RED_TONIC_SHOWCASE_SCENARIO
                | ENEMY_SHOWCASE_SCENARIO
                | ENEMY_DEATH_SHOWCASE_SCENARIO
                | DAMAGE_LETHAL_SHOWCASE_SCENARIO
                | DAMAGE_GRACE_SHOWCASE_SCENARIO
                | DEATH_RESTART_SHOWCASE_SCENARIO
                | DEATH_RECAP_SHOWCASE_SCENARIO
                | INVENTORY_SHOWCASE_SCENARIO
                | ITEM_CATALOG_SHOWCASE_SCENARIO
                | DEBUG_OVERLAY_SHOWCASE_SCENARIO
                | DEBUG_TOOLS_SHOWCASE_SCENARIO
                | BOSS_SHOWCASE_SCENARIO
                | BOSS_COMPLETION_SHOWCASE_SCENARIO
                | STRESS_FULL_SCENARIO
                | STRESS_REDUCED_SCENARIO,
            ) => {
                bail!("{EVIDENCE_SCENARIO_ENV} requires GRAVEBOUND_SCREENSHOT_PATH")
            }
            Some(other) => bail!("unknown {EVIDENCE_SCENARIO_ENV} value `{other}`"),
        }
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
pub(crate) struct CombatInputSampler {
    latest: CombatAction,
    primary: SequencedButtonState,
    ability_1: SequencedButtonState,
    ability_2: SequencedButtonState,
}

impl CombatInputSampler {
    pub(crate) const fn player_fired(&self) -> bool {
        self.latest.primary_held
    }
}

#[derive(Debug, Default)]
struct SequencedButtonState {
    was_pressed: bool,
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
pub(crate) struct ProjectilePresentation(EntityId);

#[derive(Debug, Component)]
struct DebugTargetPresentation;

#[derive(Debug, Component)]
struct CombatDiagnostics;

#[derive(Debug, Default, Resource)]
pub(crate) struct CollisionDiagnostics {
    enemy_hits: u64,
    solid_blocks: u64,
    last_target: Option<CollisionTarget>,
    grave_mark_hits: u64,
    marked_primary_intents: u64,
    last_raw_intent: Option<u32>,
    slipstep_casts: u64,
    empowered_shots: u64,
    piercing_contacts: u64,
    focused_gains: u64,
    focused_shots: u64,
    focused_raw_intents: u64,
    later_actions_rejected: u64,
}

impl CollisionDiagnostics {
    pub(crate) const fn showcase_ready(&self) -> bool {
        self.enemy_hits > 0 && self.solid_blocks > 0
    }

    pub(crate) const fn grave_mark_showcase_ready(&self) -> bool {
        self.grave_mark_hits > 0 && self.marked_primary_intents > 0
    }

    pub(crate) const fn slipstep_showcase_ready(&self) -> bool {
        self.slipstep_casts > 0 && self.empowered_shots > 0 && self.piercing_contacts >= 2
    }
    pub(crate) const fn stillness_showcase_ready(&self) -> bool {
        self.focused_gains > 0 && self.focused_shots > 0
    }

    pub(crate) const fn later_action_rejected(&self) -> bool {
        self.later_actions_rejected > 0
    }

    pub(crate) const fn item_showcase_ready(&self) -> bool {
        self.focused_shots >= 3 && self.focused_raw_intents > 0
    }

    pub(crate) const fn focused_raw_intents(&self) -> u64 {
        self.focused_raw_intents
    }
}

#[derive(Debug, Component)]
pub(crate) struct TransientEffect {
    remaining_seconds: f32,
    total_seconds: f32,
}

#[derive(Debug, Component)]
struct NailTrapPresentation(sim_core::EntityId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
enum PrimarySequenceError {
    #[error("primary press sequence exhausted u32")]
    Exhausted,
}

pub(crate) fn configure(app: &mut App) {
    app.insert_resource(PrimaryFireBindings::default())
        .insert_resource(AbilityOneBindings::default())
        .insert_resource(AbilityTwoBindings::default())
        .insert_resource(CombatInputGate::default())
        .insert_resource(CombatInputSampler::default())
        .insert_resource(AimPresentation::default())
        .insert_resource(crate::oath_feedback::OathAudioCue::start())
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
    runtime: Res<EnemyLabRuntime>,
    collision_world: Res<CombatCollisionWorld>,
    scenario: Res<EvidenceScenario>,
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
    if !matches!(
        *scenario,
        EvidenceScenario::EnemyShowcase
            | EvidenceScenario::EnemyDeathShowcase
            | EvidenceScenario::DamageLethalShowcase
            | EvidenceScenario::DamageGraceShowcase
            | EvidenceScenario::DeathRestartShowcase
            | EvidenceScenario::DeathRecapShowcase
            | EvidenceScenario::DebugOverlayShowcase
            | EvidenceScenario::DebugToolsShowcase
            | EvidenceScenario::BossShowcase
            | EvidenceScenario::BossCompletionShowcase
            | EvidenceScenario::StressFull
            | EvidenceScenario::StressReduced
    ) {
        spawn_debug_targets(&mut commands, &arena.0, &collision_world.0);
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
        Text::new("PRIMARY: LMB  |  ABILITY 1: RMB  |  PINE CROSSBOW + GRAVE MARK"),
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

    let weapon = runtime.combat().weapon();
    let grave_mark = runtime.combat().grave_mark_definition();
    info!(
        feature_id = "GB-M01-02C",
        weapon_id = weapon.content_id(),
        damage = weapon.raw_damage(),
        interval_ticks = weapon.attack_interval_ticks(),
        lifetime_ticks = weapon.projectile_lifetime_ticks(),
        range_tiles = weapon.range_tiles(),
        speed_tiles_per_second = weapon.projectile_speed_tiles_per_second(),
        radius_tiles = weapon.projectile_radius_tiles(),
        debug_hurtboxes = collision_world.0.enemies().len(),
        grave_mark_id = grave_mark.content_id(),
        grave_mark_cooldown_ticks = grave_mark.cooldown_ticks(),
        grave_mark_range_tiles = grave_mark.range_tiles(),
        grave_mark_radius_tiles = grave_mark.projectile_radius_tiles(),
        grave_mark_duration_ticks = grave_mark.duration_ticks(),
        "Grave Arbalist combat presentation initialized"
    );
}

fn spawn_debug_targets(
    commands: &mut Commands,
    arena: &sim_core::ArenaGeometry,
    collision_world: &ProjectileCollisionWorld,
) {
    for hurtbox in collision_world.enemies() {
        let render = simulation_point_to_render(hurtbox.center(), arena);
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
}

#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
fn sample_combat_input(
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    bindings: Res<PrimaryFireBindings>,
    ability_bindings: Res<AbilityOneBindings>,
    ability_two_bindings: Res<AbilityTwoBindings>,
    gate: Res<CombatInputGate>,
    scenario: Res<EvidenceScenario>,
    arena: Res<LoadedArena>,
    player: Res<PlayerSimulation>,
    runtime: Res<EnemyLabRuntime>,
    camera: Single<(&Camera, &GlobalTransform), With<CameraFollow>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut sampler: ResMut<CombatInputSampler>,
    mut presentation: ResMut<AimPresentation>,
) {
    if matches!(
        *scenario,
        EvidenceScenario::PrimaryFireEast
            | EvidenceScenario::CollisionShowcase
            | EvidenceScenario::GraveMarkShowcase
            | EvidenceScenario::SlipstepShowcase
            | EvidenceScenario::StillnessShowcase
            | EvidenceScenario::EnemyDeathShowcase
            | EvidenceScenario::DamageLethalShowcase
            | EvidenceScenario::DamageGraceShowcase
            | EvidenceScenario::DeathRestartShowcase
            | EvidenceScenario::DeathRecapShowcase
            | EvidenceScenario::ItemCatalogShowcase
    ) {
        let showcase_west = *scenario == EvidenceScenario::CollisionShowcase
            && (runtime.combat().tick().0 < 14 || runtime.combat().tick().0 >= 28);
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
        if *scenario == EvidenceScenario::GraveMarkShowcase
            && sampler.latest.ability_1_press_sequence == 0
        {
            sampler.latest.ability_1_press_sequence = 1;
        }
        if *scenario == EvidenceScenario::SlipstepShowcase
            && sampler.latest.ability_2_press_sequence == 0
        {
            sampler.latest.ability_2_press_sequence = 1;
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
    sample_ability_one_button(
        &mut sampler,
        mouse.pressed(ability_bindings.primary),
        gate.blocked,
    )
    .expect("ability-1 sequence space must not exhaust during LocalLab");
    let gamepad_pressed = gamepads
        .iter()
        .any(|gamepad| gamepad.pressed(ability_two_bindings.gamepad));
    sample_ability_two_button(
        &mut sampler,
        keyboard.pressed(ability_two_bindings.keyboard) || gamepad_pressed,
        gate.blocked,
    )
    .expect("ability-2 sequence space must not exhaust during LocalLab");
}

fn sample_primary_button(
    sampler: &mut CombatInputSampler,
    physically_pressed: bool,
    blocked: bool,
) -> Result<(), PrimarySequenceError> {
    if blocked {
        sampler.latest.primary_held = false;
        sampler.primary.suppressed_until_release |= physically_pressed;
        sampler.primary.was_pressed = physically_pressed;
        return Ok(());
    }
    if sampler.primary.suppressed_until_release {
        sampler.latest.primary_held = false;
        if !physically_pressed {
            sampler.primary.suppressed_until_release = false;
        }
        sampler.primary.was_pressed = physically_pressed;
        return Ok(());
    }
    if physically_pressed && !sampler.primary.was_pressed {
        sampler.latest.primary_press_sequence = sampler
            .latest
            .primary_press_sequence
            .checked_add(1)
            .ok_or(PrimarySequenceError::Exhausted)?;
    }
    sampler.latest.primary_held = physically_pressed;
    sampler.primary.was_pressed = physically_pressed;
    Ok(())
}

fn sample_ability_one_button(
    sampler: &mut CombatInputSampler,
    physically_pressed: bool,
    blocked: bool,
) -> Result<(), PrimarySequenceError> {
    if blocked {
        sampler.ability_1.suppressed_until_release |= physically_pressed;
        sampler.ability_1.was_pressed = physically_pressed;
        return Ok(());
    }
    if sampler.ability_1.suppressed_until_release {
        if !physically_pressed {
            sampler.ability_1.suppressed_until_release = false;
        }
        sampler.ability_1.was_pressed = physically_pressed;
        return Ok(());
    }
    if physically_pressed && !sampler.ability_1.was_pressed {
        sampler.latest.ability_1_press_sequence = sampler
            .latest
            .ability_1_press_sequence
            .checked_add(1)
            .ok_or(PrimarySequenceError::Exhausted)?;
    }
    sampler.ability_1.was_pressed = physically_pressed;
    Ok(())
}

fn sample_ability_two_button(
    sampler: &mut CombatInputSampler,
    physically_pressed: bool,
    blocked: bool,
) -> Result<(), PrimarySequenceError> {
    if blocked {
        sampler.ability_2.suppressed_until_release |= physically_pressed;
        sampler.ability_2.was_pressed = physically_pressed;
        return Ok(());
    }
    if sampler.ability_2.suppressed_until_release {
        if !physically_pressed {
            sampler.ability_2.suppressed_until_release = false;
        }
        sampler.ability_2.was_pressed = physically_pressed;
        return Ok(());
    }
    if physically_pressed && !sampler.ability_2.was_pressed {
        sampler.latest.ability_2_press_sequence = sampler
            .latest
            .ability_2_press_sequence
            .checked_add(1)
            .ok_or(PrimarySequenceError::Exhausted)?;
    }
    sampler.ability_2.was_pressed = physically_pressed;
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
    mut runtime: ResMut<EnemyLabRuntime>,
    collision_world: Res<CombatCollisionWorld>,
    input: Res<CombatInputSampler>,
    mut player: ResMut<PlayerSimulation>,
    arena: Res<LoadedArena>,
    movement: Res<LatestMovementAction>,
    scenario: Res<EvidenceScenario>,
    accessibility: Res<crate::accessibility::AccessibilitySettings>,
    oath_audio: Res<crate::oath_feedback::OathAudioCue>,
    mut player_transform: Single<&mut Transform, With<LocalPlayer>>,
    mut visuals: Query<(Entity, &ProjectilePresentation, &mut Transform), Without<LocalPlayer>>,
    trap_visuals: Query<(Entity, &NailTrapPresentation)>,
    mut collision_diagnostics: ResMut<CollisionDiagnostics>,
) {
    if !runtime.player_can_act() {
        if input.latest.primary_held
            || input.latest.ability_1_press_sequence > 0
            || input.latest.ability_2_press_sequence > 0
        {
            collision_diagnostics.later_actions_rejected = collision_diagnostics
                .later_actions_rejected
                .saturating_add(1);
        }
        return;
    }
    let mut action = input.latest;
    action.movement =
        if *scenario == EvidenceScenario::SlipstepShowcase && runtime.combat().tick().0 == 0 {
            sim_core::MovementAction::new(1, 0)
        } else {
            movement.0
        };
    let dynamic_collision_world = if matches!(
        *scenario,
        EvidenceScenario::None
            | EvidenceScenario::EnemyShowcase
            | EvidenceScenario::EnemyDeathShowcase
            | EvidenceScenario::DamageLethalShowcase
            | EvidenceScenario::DamageGraceShowcase
            | EvidenceScenario::DeathRestartShowcase
            | EvidenceScenario::DeathRecapShowcase
            | EvidenceScenario::DebugOverlayShowcase
            | EvidenceScenario::DebugToolsShowcase
            | EvidenceScenario::BossShowcase
            | EvidenceScenario::BossCompletionShowcase
    ) {
        Some(
            ProjectileCollisionWorld::new(
                &arena.0,
                runtime
                    .alive_hurtboxes()
                    .expect("enemy health hurtboxes remain valid"),
            )
            .expect("dynamic LocalLab collision world remains valid"),
        )
    } else {
        None
    };
    let active_collision_world = dynamic_collision_world
        .as_ref()
        .unwrap_or(&collision_world.0);
    let step = runtime
        .combat_mut()
        .step_with_movement(player.state_mut(), action, &arena.0, active_collision_world)
        .expect("validated LocalLab combat input must remain legal");
    if dynamic_collision_world.is_some() {
        runtime
            .apply_friendly_combat(&step)
            .expect("friendly damage provenance remains valid");
    }
    let render_position = simulation_point_to_render(player.state().position(), &arena.0);
    player_transform.translation.x = render_position.x;
    player_transform.translation.y = render_position.y;
    present_collision_events(
        &mut commands,
        &step,
        &arena.0,
        &mut visuals,
        &mut collision_diagnostics,
    );
    present_expirations(&mut commands, &step, &arena.0, &mut visuals);
    present_shots(&mut commands, &step, &arena.0);
    present_slipstep(&mut commands, &step, &arena.0, &mut collision_diagnostics);
    present_stillness(&mut commands, &step, &arena.0, &mut collision_diagnostics);
    present_nail_traps(
        &mut commands,
        &step,
        runtime.combat(),
        &arena.0,
        &trap_visuals,
        *accessibility,
        &oath_audio,
    );
    log_grave_mark_events(&step);
    log_slipstep_events(&step);
    log_stillness_events(&step);
    sync_projectile_visuals(runtime.combat(), &arena.0, &mut visuals);
}

fn present_nail_traps(
    commands: &mut Commands,
    step: &sim_core::CombatStep,
    combat: &PlayerCombatState,
    arena: &sim_core::ArenaGeometry,
    visuals: &Query<(Entity, &NailTrapPresentation)>,
    accessibility: crate::accessibility::AccessibilitySettings,
    oath_audio: &crate::oath_feedback::OathAudioCue,
) {
    for removal in &step.nail_traps.removals {
        despawn_nail_trap(commands, removal.trap_id, visuals);
    }
    for trap_id in &step.nail_traps.armed {
        if !oath_audio.play(crate::oath_feedback::OathAudioCueKind::TrapArmed) {
            warn!(
                feature_id = "GB-M03-05C",
                "Nailkeeper arm cue was unavailable"
            );
        }
        despawn_nail_trap(commands, *trap_id, visuals);
        if let Some(trap) = combat
            .nail_traps()
            .traps()
            .iter()
            .find(|trap| trap.id() == *trap_id)
        {
            spawn_nail_trap(commands, trap, arena, true, accessibility);
        }
    }
    for trap_id in &step.nail_traps.spawned {
        if let Some(trap) = combat
            .nail_traps()
            .traps()
            .iter()
            .find(|trap| trap.id() == *trap_id)
        {
            spawn_nail_trap(commands, trap, arena, false, accessibility);
        }
    }
    for trigger in &step.nail_traps.triggers {
        if !oath_audio.play(crate::oath_feedback::OathAudioCueKind::TrapTriggered) {
            warn!(
                feature_id = "GB-M03-05C",
                "Nailkeeper trigger cue was unavailable"
            );
        }
        spawn_transient(
            commands,
            "Nailkeeper Frostbind burst",
            simulation_point_to_render(trigger.position, arena),
            Color::srgba_u8(154, 238, 247, 230),
            0.92,
            0.34,
        );
        info!(
            feature_id = "GB-M03-05C",
            tick = trigger.tick.0,
            trap_id = trigger.trap_id.get(),
            target_id = trigger.target_id.get(),
            raw_damage = trigger.raw_damage,
            frostbind_ticks = trigger.frostbind_ticks,
            "Nailkeeper trap triggered"
        );
    }
}

fn despawn_nail_trap(
    commands: &mut Commands,
    trap_id: sim_core::EntityId,
    visuals: &Query<(Entity, &NailTrapPresentation)>,
) {
    if let Some((entity, _)) = visuals.iter().find(|(_, visual)| visual.0 == trap_id) {
        commands.entity(entity).despawn();
    }
}

fn spawn_nail_trap(
    commands: &mut Commands,
    trap: &sim_core::NailTrap,
    arena: &sim_core::ArenaGeometry,
    armed: bool,
    accessibility: crate::accessibility::AccessibilitySettings,
) {
    let plan = nail_trap_visual_plan(armed, accessibility.reduced_motion);
    let position = simulation_point_to_render(trap.position(), arena);
    let color = if plan.armed {
        Color::srgba_u8(106, 221, 235, 225)
    } else {
        Color::srgba_u8(173, 141, 79, 155)
    };
    let entity = commands
        .spawn((
            Name::new(if armed {
                "Armed nail trap"
            } else {
                "Arming nail trap"
            }),
            NailTrapPresentation(trap.id()),
            Transform::from_xyz(position.x, position.y, FRIENDLY_PROJECTILE_Z - 0.1),
            Visibility::default(),
        ))
        .id();
    commands.entity(entity).with_children(|parent| {
        for index in 0..plan.segment_count {
            let angle = f32::from(index) * std::f32::consts::TAU / f32::from(plan.segment_count);
            let offset =
                Vec2::new(angle.cos(), angle.sin()) * sim_core::NAILKEEPER_TRAP_RADIUS_TILES;
            parent.spawn((
                Sprite::from_color(color, Vec2::new(plan.segment_width, plan.segment_length)),
                Transform::from_xyz(offset.x, offset.y, 0.0)
                    .with_rotation(Quat::from_rotation_z(angle)),
            ));
        }
        let marker_rotations: &[f32] = if plan.armed {
            &[0.0, std::f32::consts::FRAC_PI_2]
        } else {
            &[std::f32::consts::FRAC_PI_4]
        };
        for rotation in marker_rotations {
            parent.spawn((
                Sprite::from_color(color, Vec2::new(0.34, 0.08)),
                Transform::from_rotation(Quat::from_rotation_z(*rotation)),
            ));
        }
    });
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct NailTrapVisualPlan {
    armed: bool,
    segment_count: u8,
    segment_width: f32,
    segment_length: f32,
}

fn nail_trap_visual_plan(armed: bool, reduced_motion: bool) -> NailTrapVisualPlan {
    NailTrapVisualPlan {
        armed,
        segment_count: if reduced_motion { 8 } else { 12 },
        segment_width: if reduced_motion { 0.10 } else { 0.07 },
        segment_length: if armed { 0.30 } else { 0.20 },
    }
}

fn present_collision_events(
    commands: &mut Commands,
    step: &sim_core::CombatStep,
    arena: &sim_core::ArenaGeometry,
    visuals: &mut Query<(Entity, &ProjectilePresentation, &mut Transform), Without<LocalPlayer>>,
    diagnostics: &mut CollisionDiagnostics,
) {
    for collision in &step.collisions {
        if !collision.projectile_continues
            && let Some((entity, _, _)) = visuals
                .iter_mut()
                .find(|(_, visual, _)| visual.0 == collision.projectile_id)
        {
            commands.entity(entity).despawn();
        }
        let raw_intent = step.raw_damage_intents.iter().find(|intent| {
            intent.projectile_id == collision.projectile_id
                && intent.contact_ordinal == collision.contact_ordinal
        });
        if collision.empowered_by_slipstep && matches!(collision.target, CollisionTarget::Enemy(_))
        {
            diagnostics.piercing_contacts = diagnostics.piercing_contacts.saturating_add(1);
        }
        let marked_primary = raw_intent.is_some_and(|intent| {
            intent.source == RawDamageIntentSource::Primary
                && intent.multiplier_basis_points > sim_core::BASIS_POINTS_PER_ONE
        });
        let (name, color, size) = match (collision.source, collision.target, marked_primary) {
            (FriendlyProjectileSource::GraveMark, CollisionTarget::Enemy(_), _) => {
                diagnostics.grave_mark_hits = diagnostics.grave_mark_hits.saturating_add(1);
                ("Grave Mark applied", Color::srgb_u8(191, 139, 241), 0.58)
            }
            (FriendlyProjectileSource::Primary, CollisionTarget::Enemy(_), true) => {
                diagnostics.marked_primary_intents =
                    diagnostics.marked_primary_intents.saturating_add(1);
                ("Marked primary intent", Color::srgb_u8(240, 213, 139), 0.54)
            }
            (_, CollisionTarget::Solid(_), _) => {
                diagnostics.solid_blocks = diagnostics.solid_blocks.saturating_add(1);
                ("Solid block", Color::srgb_u8(240, 184, 92), 0.36)
            }
            (_, CollisionTarget::Enemy(_), _) => {
                diagnostics.enemy_hits = diagnostics.enemy_hits.saturating_add(1);
                ("Enemy hit", Color::srgb_u8(82, 211, 178), 0.48)
            }
        };
        if let Some(intent) = raw_intent {
            diagnostics.last_raw_intent = Some(intent.resolved_raw_damage);
            if collision.focused_by_stillness && intent.resolved_raw_damage == 13 {
                diagnostics.focused_raw_intents = diagnostics.focused_raw_intents.saturating_add(1);
            }
        }
        diagnostics.last_target = Some(collision.target);
        let position = simulation_point_to_render(collision.final_position, arena);
        spawn_contact_transient(commands, name, position, color, size, collision.target);
        let feature_id = if collision.empowered_by_slipstep {
            "GB-M01-02D"
        } else if collision.source == FriendlyProjectileSource::GraveMark {
            "GB-M01-02C"
        } else {
            "GB-M01-02B"
        };
        info!(
            feature_id,
            tick = collision.tick.0,
            projectile_id = collision.projectile_id.get(),
            projectile_source = ?collision.source,
            target = %collision.target,
            position_x = collision.final_position.x,
            position_y = collision.final_position.y,
            distance_tiles = collision.distance_travelled_tiles,
            contact_ordinal = collision.contact_ordinal,
            projectile_continues = collision.projectile_continues,
            "friendly projectile collision"
        );
    }
}

fn present_expirations(
    commands: &mut Commands,
    step: &sim_core::CombatStep,
    arena: &sim_core::ArenaGeometry,
    visuals: &mut Query<(Entity, &ProjectilePresentation, &mut Transform), Without<LocalPlayer>>,
) {
    for expiration in &step.expirations {
        if let Some((entity, _, _)) = visuals
            .iter_mut()
            .find(|(_, visual, _)| visual.0 == expiration.projectile_id)
        {
            commands.entity(entity).despawn();
        }
        let position = simulation_point_to_render(expiration.final_position, arena);
        spawn_transient(
            commands,
            "Range expiry",
            position,
            match expiration.source {
                FriendlyProjectileSource::Primary => Color::srgba_u8(211, 241, 224, 150),
                FriendlyProjectileSource::BellDebtRepeat => Color::srgba_u8(245, 183, 79, 190),
                FriendlyProjectileSource::GraveMark => Color::srgba_u8(191, 139, 241, 180),
            },
            0.32,
            0.12,
        );
    }
}

fn present_shots(
    commands: &mut Commands,
    step: &sim_core::CombatStep,
    arena: &sim_core::ArenaGeometry,
) {
    for shot in &step.shots {
        spawn_projectile(commands, &shot.projectile, arena);
        let direction = simulation_direction_to_render(shot.projectile.direction().vector());
        let origin = simulation_point_to_render(shot.projectile.origin(), arena);
        let muzzle_color = match shot.projectile.source() {
            FriendlyProjectileSource::Primary => Color::srgba_u8(240, 213, 139, 220),
            FriendlyProjectileSource::BellDebtRepeat => Color::srgba_u8(245, 183, 79, 250),
            FriendlyProjectileSource::GraveMark => Color::srgba_u8(191, 139, 241, 240),
        };
        spawn_transient(
            commands,
            "Muzzle flash",
            origin + direction * MUZZLE_OFFSET_TILES,
            muzzle_color,
            0.24,
            0.07,
        );
        info!(
            feature_id = "GB-M01-02C",
            tick = shot.tick.0,
            press_sequence = shot.press_sequence,
            projectile_id = shot.projectile.id().get(),
            projectile_source = ?shot.projectile.source(),
            origin_x = shot.projectile.origin().x,
            origin_y = shot.projectile.origin().y,
            direction_x = shot.projectile.direction().vector().x,
            direction_y = shot.projectile.direction().vector().y,
            "friendly projectile fired"
        );
    }
}

fn present_slipstep(
    commands: &mut Commands,
    step: &sim_core::CombatStep,
    arena: &sim_core::ArenaGeometry,
    diagnostics: &mut CollisionDiagnostics,
) {
    for input in &step.slipstep_inputs {
        if input.result == sim_core::SlipstepInputResult::Began {
            diagnostics.slipstep_casts = diagnostics.slipstep_casts.saturating_add(1);
        }
    }
    for shot in &step.shots {
        if shot.projectile.empowered_by_slipstep() {
            diagnostics.empowered_shots = diagnostics.empowered_shots.saturating_add(1);
            spawn_transient(
                commands,
                "Slipstep empowered muzzle",
                simulation_point_to_render(shot.projectile.origin(), arena),
                Color::srgba_u8(91, 220, 235, 220),
                0.62,
                0.32,
            );
        }
    }
    for transition in &step.slipstep_transitions {
        if matches!(
            transition.kind,
            sim_core::SlipstepTransitionKind::Travelled
                | sim_core::SlipstepTransitionKind::Collided
                | sim_core::SlipstepTransitionKind::Completed
        ) {
            spawn_transient(
                commands,
                "Slipstep afterimage",
                simulation_point_to_render(transition.position, arena),
                Color::srgba_u8(91, 220, 235, 150),
                0.48,
                3.0,
            );
        }
    }
}

fn present_stillness(
    commands: &mut Commands,
    step: &sim_core::CombatStep,
    arena: &sim_core::ArenaGeometry,
    diagnostics: &mut CollisionDiagnostics,
) {
    for transition in &step.focused_transitions {
        if transition.kind == sim_core::FocusedTransitionKind::Gained {
            diagnostics.focused_gains = diagnostics.focused_gains.saturating_add(1);
            if let Some(shot) = step.shots.first() {
                spawn_transient(
                    commands,
                    "Focused gained",
                    simulation_point_to_render(shot.projectile.origin(), arena),
                    Color::srgba_u8(240, 213, 139, 210),
                    0.82,
                    2.0,
                );
            }
        }
    }
    let focused_shots = step
        .shots
        .iter()
        .filter(|shot| shot.projectile.focused_by_stillness())
        .fold(0_u64, |count, _| count.saturating_add(1));
    diagnostics.focused_shots = diagnostics.focused_shots.saturating_add(focused_shots);
}

fn log_grave_mark_events(step: &sim_core::CombatStep) {
    for transition in &step.mark_transitions {
        info!(
            feature_id = "GB-M01-02C",
            tick = transition.tick.0,
            transition = ?transition.kind,
            target = transition.target.get(),
            previous_target = transition.previous_target.map(EntityId::get),
            source_projectile_id = transition.source_projectile_id.get(),
            remaining_ticks = transition.remaining_ticks,
            "Grave Mark state changed"
        );
    }
    for input_event in &step.grave_mark_inputs {
        info!(
            feature_id = "GB-M01-02C",
            tick = input_event.tick.0,
            press_sequence = input_event.press_sequence,
            result = ?input_event.result,
            "Grave Mark input resolved"
        );
    }
}

fn log_slipstep_events(step: &sim_core::CombatStep) {
    for input in &step.slipstep_inputs {
        info!(
            feature_id = "GB-M01-02D",
            tick = input.tick.0,
            press_sequence = input.press_sequence,
            result = ?input.result,
            "Slipstep input resolved"
        );
    }
    for transition in &step.slipstep_transitions {
        info!(
            feature_id = "GB-M01-02D",
            tick = transition.tick.0,
            transition = ?transition.kind,
            press_sequence = transition.press_sequence,
            position_x = transition.position.x,
            position_y = transition.position.y,
            travelled_tiles = transition.travelled_tiles,
            remaining_travel_ticks = transition.remaining_travel_ticks,
            solid = ?transition.solid,
            direct_damage_reduction_basis_points = step.direct_damage_reduction_basis_points,
            "Slipstep state changed"
        );
    }
}

fn log_stillness_events(step: &sim_core::CombatStep) {
    for transition in &step.focused_transitions {
        info!(
            feature_id = "GB-M01-02E",
            tick = transition.tick.0,
            transition = ?transition.kind,
            stillness_ticks = transition.stillness_ticks,
            "Stillness state changed"
        );
    }
}

fn sync_projectile_visuals(
    combat: &PlayerCombatState,
    arena: &sim_core::ArenaGeometry,
    visuals: &mut Query<(Entity, &ProjectilePresentation, &mut Transform), Without<LocalPlayer>>,
) {
    for (_, visual, mut transform) in visuals.iter_mut() {
        if let Some(projectile) = combat
            .projectiles()
            .iter()
            .find(|projectile| projectile.id() == visual.0)
        {
            *transform = projectile_transform(projectile, arena);
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
    let (name, outer_color, core_color, length) = match projectile.source() {
        FriendlyProjectileSource::Primary => (
            if projectile.empowered_by_slipstep() {
                "Slipstep bolt"
            } else if projectile.focused_by_stillness() {
                "Focused bolt"
            } else {
                "Pine bolt"
            },
            if projectile.empowered_by_slipstep() {
                Color::srgb_u8(119, 239, 245)
            } else if projectile.focused_by_stillness() {
                Color::srgb_u8(244, 221, 142)
            } else {
                Color::srgb_u8(231, 224, 199)
            },
            if projectile.empowered_by_slipstep() {
                Color::srgb_u8(191, 139, 241)
            } else if projectile.focused_by_stillness() {
                Color::srgb_u8(82, 211, 178)
            } else {
                Color::srgb_u8(173, 141, 79)
            },
            if projectile.empowered_by_slipstep() {
                0.46
            } else if projectile.focused_by_stillness() {
                0.42
            } else {
                0.34
            },
        ),
        FriendlyProjectileSource::BellDebtRepeat => (
            "Bell echo bolt",
            Color::srgb_u8(245, 183, 79),
            Color::srgb_u8(255, 239, 178),
            0.28,
        ),
        FriendlyProjectileSource::GraveMark => (
            "Grave Mark bolt",
            Color::srgb_u8(211, 176, 245),
            Color::srgb_u8(82, 211, 178),
            0.46,
        ),
    };
    commands
        .spawn((
            Name::new(format!("{name} {}", projectile.id())),
            ProjectilePresentation(projectile.id()),
            Sprite::from_color(
                outer_color,
                Vec2::new(length, projectile.radius_tiles() * 2.0),
            ),
            projectile_transform(projectile, arena),
        ))
        .with_child((
            Name::new(format!("{name} core")),
            Sprite::from_color(core_color, Vec2::new(length * 0.55, 0.035)),
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

#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_lines // One snapshot keeps cross-system combat diagnostics coherent.
)]
fn update_combat_diagnostics(
    runtime: Res<EnemyLabRuntime>,
    input: Res<CombatInputSampler>,
    gate: Res<CombatInputGate>,
    collision_diagnostics: Res<CollisionDiagnostics>,
    scenario: Res<EvidenceScenario>,
    mut diagnostics: Single<&mut Text, With<CombatDiagnostics>>,
) {
    if !runtime.is_changed()
        && !input.is_changed()
        && !gate.is_changed()
        && !collision_diagnostics.is_changed()
        && !scenario.is_changed()
    {
        return;
    }
    let state = runtime.combat();
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
    let mark_status = if gate.blocked {
        "MARK BLOCKED".to_owned()
    } else if let Some(sequence) = state.pending_grave_mark_sequence() {
        format!("MARK BUFFER {sequence}")
    } else if state.grave_mark_cooldown_remaining_ticks() == 0
        && state.global_cooldown_remaining_ticks() == 0
    {
        "MARK READY".to_owned()
    } else {
        format!(
            "MARK CD {}T / GCD {}T",
            state.grave_mark_cooldown_remaining_ticks(),
            state.global_cooldown_remaining_ticks()
        )
    };
    let active_mark = state.active_grave_mark().map_or_else(
        || "NONE".to_owned(),
        |mark| format!("{} / {}T", mark.target(), mark.remaining_ticks()),
    );
    let last_intent = collision_diagnostics
        .last_raw_intent
        .map_or_else(|| "NONE".to_owned(), |value| value.to_string());
    let slip_status = if gate.blocked {
        "SLIP BLOCKED".to_owned()
    } else if state.slipstep_remaining_travel_ticks() > 0 {
        format!(
            "SLIPPING {}T / DR 25%",
            state.slipstep_remaining_travel_ticks()
        )
    } else if state.slipstep_cooldown_remaining_ticks() == 0 {
        "SLIP READY".to_owned()
    } else {
        format!("SLIP CD {}T", state.slipstep_cooldown_remaining_ticks())
    };
    let focus_status = if state.focused() {
        format!(
            "FOCUSED +{}% SPD / +{}% DMG",
            state
                .stillness_definition()
                .projectile_speed_bonus_basis_points()
                / 100,
            state
                .stillness_definition()
                .primary_damage_bonus_basis_points()
                / 100,
        )
    } else {
        format!(
            "FOCUSING {}/{}T",
            state.stillness_ticks(),
            state.stillness_definition().activation_ticks()
        )
    };
    let health_boundary = match *scenario {
        EvidenceScenario::RedTonicShowcase => "TONIC VITALS ACTIVE",
        EvidenceScenario::EnemyShowcase
        | EvidenceScenario::EnemyDeathShowcase
        | EvidenceScenario::DamageLethalShowcase
        | EvidenceScenario::DamageGraceShowcase
        | EvidenceScenario::DeathRestartShowcase => "HOSTILE VITALS ACTIVE",
        EvidenceScenario::DeathRecapShowcase => "LOCAL DEATH FROZEN",
        EvidenceScenario::ItemCatalogShowcase => "ITEM LOADOUT ACTIVE",
        _ => "HEALTH UNCHANGED",
    };
    let weapon = state.weapon();
    diagnostics.0 = format!(
        "PRIMARY: LMB  |  {} {} / {}T / {:.1} RNG / {:.1} SPD / x{}  |  {status}  |  AIM {angle:>5.1} DEG  |  BOLTS {}\nABILITY 1: RMB  |  GRAVE MARK 36 INTENT / 11 RNG / R0.12 / 120T  |  {mark_status}  |  ACTIVE {active_mark}\nABILITY 2: SPACE/LB  |  SLIP 2.0 / 5T / DR25%  |  {slip_status}  |  EXHAUST {}T  |  EMPOWER {}T  |  CASTS {} / SHOTS {} / PIERCE HITS {}\nPASSIVE: STILLNESS  |  {focus_status}  |  GAINS {} / FOCUSED SHOTS {}\nCOLLISION ACTIVE  |  MARK HITS {}  |  +15% PRIMARY INTENTS {}  |  SOLID BLOCKS {}  |  LAST {last}  |  RAW {last_intent}  |  {health_boundary}",
        weapon_label(weapon.content_id()),
        weapon.raw_damage(),
        weapon.attack_interval_ticks(),
        weapon.range_tiles(),
        weapon.projectile_speed_tiles_per_second(),
        weapon.projectile_count(),
        state.projectiles().len(),
        state.exhaustion_remaining_ticks(),
        state.empowered_primary_remaining_ticks(),
        collision_diagnostics.slipstep_casts,
        collision_diagnostics.empowered_shots,
        collision_diagnostics.piercing_contacts,
        collision_diagnostics.focused_gains,
        collision_diagnostics.focused_shots,
        collision_diagnostics.grave_mark_hits,
        collision_diagnostics.marked_primary_intents,
        collision_diagnostics.solid_blocks
    );
}

fn weapon_label(content_id: &str) -> &'static str {
    match content_id {
        "item.prototype.weapon.pine_crossbow" => "PINE",
        "item.prototype.weapon.grave_repeater" => "REPEATER",
        "item.prototype.weapon.longbolt_crossbow" => "LONGBOLT",
        "item.prototype.weapon.scatterbow" => "SCATTER",
        _ => "UNKNOWN",
    }
}

#[allow(
    clippy::cast_precision_loss,
    clippy::needless_pass_by_value,
    clippy::too_many_lines // One pass keeps every collision primitive on the same snapshot.
)]
fn draw_collision_debug(
    mut gizmos: Gizmos,
    arena: Res<LoadedArena>,
    collision_world: Res<CombatCollisionWorld>,
    runtime: Res<EnemyLabRuntime>,
    targets: Query<&DebugTargetPresentation>,
    scenario: Res<EvidenceScenario>,
    debug: Res<crate::debug_overlay::DebugOverlayState>,
) {
    if !debug.visible() && *scenario == EvidenceScenario::None {
        return;
    }
    let weapon_radius = runtime.combat().weapon().projectile_radius_tiles();
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
    let debug_hurtboxes = if runtime.normal_mode() {
        runtime
            .alive_hurtboxes()
            .expect("normal-wave debug hurtboxes remain valid")
    } else {
        collision_world.0.enemies().to_vec()
    };
    for hurtbox in debug_hurtboxes.iter().filter(|_| {
        !matches!(
            *scenario,
            EvidenceScenario::EnemyShowcase
                | EvidenceScenario::EnemyDeathShowcase
                | EvidenceScenario::DamageLethalShowcase
                | EvidenceScenario::DamageGraceShowcase
                | EvidenceScenario::DeathRestartShowcase
                | EvidenceScenario::DeathRecapShowcase
        )
    }) {
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
    for projectile in runtime.combat().projectiles() {
        let color = match projectile.source() {
            FriendlyProjectileSource::Primary => Color::srgb_u8(231, 224, 199),
            FriendlyProjectileSource::BellDebtRepeat => Color::srgb_u8(245, 183, 79),
            FriendlyProjectileSource::GraveMark => Color::srgb_u8(211, 176, 245),
        };
        gizmos
            .circle_2d(
                Isometry2d::from_translation(simulation_point_to_render(
                    projectile.position(),
                    &arena.0,
                )),
                projectile.radius_tiles(),
                color,
            )
            .resolution(16);
    }
    if let Some(mark) = runtime.combat().active_grave_mark()
        && let Some(target) = collision_world
            .0
            .enemies()
            .iter()
            .find(|hurtbox| hurtbox.id() == mark.target())
    {
        let center = simulation_point_to_render(target.center(), &arena.0);
        gizmos
            .circle_2d(
                Isometry2d::from_translation(center),
                target.radius_tiles() + 0.12,
                Color::srgb_u8(211, 176, 245),
            )
            .resolution(32);
        gizmos
            .circle_2d(
                Isometry2d::from_translation(center),
                target.radius_tiles() + 0.20,
                Color::srgb_u8(82, 211, 178),
            )
            .resolution(32);
    }
    let expected_debug_targets = if runtime.normal_mode()
        || matches!(
            *scenario,
            EvidenceScenario::EnemyShowcase
                | EvidenceScenario::EnemyDeathShowcase
                | EvidenceScenario::DamageLethalShowcase
                | EvidenceScenario::DamageGraceShowcase
                | EvidenceScenario::DeathRestartShowcase
                | EvidenceScenario::DeathRecapShowcase
        ) {
        0
    } else {
        collision_world.0.enemies().len()
    };
    debug_assert_eq!(targets.iter().count(), expected_debug_targets);
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn update_transient_effects(
    time: Res<Time>,
    accessibility: Res<crate::accessibility::AccessibilitySettings>,
    mut commands: Commands,
    mut effects: Query<(Entity, &mut TransientEffect, &mut Transform)>,
) {
    for (entity, mut effect, mut transform) in &mut effects {
        effect.remaining_seconds -= time.delta_secs();
        if effect.remaining_seconds <= 0.0 {
            commands.entity(entity).despawn();
        } else {
            let scale = if accessibility.reduced_motion {
                1.0
            } else {
                (effect.remaining_seconds / effect.total_seconds).clamp(0.0, 1.0)
            };
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
    fn ability_one_binding_defaults_to_right_mouse_and_is_replaceable() {
        let defaults = AbilityOneBindings::default();
        assert_eq!(defaults.primary, MouseButton::Right);
        assert_ne!(
            defaults,
            AbilityOneBindings {
                primary: MouseButton::Middle
            }
        );
    }

    #[test]
    fn ability_two_defaults_to_space_and_left_bumper_and_is_replaceable() {
        let defaults = AbilityTwoBindings::default();
        assert_eq!(defaults.keyboard, KeyCode::Space);
        assert_eq!(defaults.gamepad, GamepadButton::LeftTrigger);
        assert_ne!(
            defaults,
            AbilityTwoBindings {
                keyboard: KeyCode::ShiftLeft,
                gamepad: GamepadButton::RightTrigger,
            }
        );
    }

    #[test]
    fn reduced_motion_traps_preserve_radius_shape_and_armed_distinction() {
        let arming = nail_trap_visual_plan(false, true);
        let armed = nail_trap_visual_plan(true, true);
        assert_eq!(arming.segment_count, 8);
        assert_eq!(armed.segment_count, 8);
        assert!((arming.segment_width - 0.10).abs() < f32::EPSILON);
        assert!((armed.segment_width - 0.10).abs() < f32::EPSILON);
        assert!(armed.segment_length > arming.segment_length);
        assert!(!arming.armed);
        assert!(armed.armed);
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
    fn ability_sampler_sequences_once_and_blocked_press_cannot_leak() {
        let mut sampler = CombatInputSampler::default();
        sample_ability_one_button(&mut sampler, true, false).expect("press");
        assert_eq!(sampler.latest.ability_1_press_sequence, 1);
        sample_ability_one_button(&mut sampler, true, false).expect("hold");
        assert_eq!(sampler.latest.ability_1_press_sequence, 1);
        sample_ability_one_button(&mut sampler, false, false).expect("release");
        sample_ability_one_button(&mut sampler, true, true).expect("blocked press");
        assert_eq!(sampler.latest.ability_1_press_sequence, 1);
        sample_ability_one_button(&mut sampler, true, false).expect("suppressed");
        assert_eq!(sampler.latest.ability_1_press_sequence, 1);
        sample_ability_one_button(&mut sampler, false, false).expect("rearm");
        sample_ability_one_button(&mut sampler, true, false).expect("fresh press");
        assert_eq!(sampler.latest.ability_1_press_sequence, 2);
    }

    #[test]
    fn ability_two_sampler_sequences_once_and_blocked_press_cannot_leak() {
        let mut sampler = CombatInputSampler::default();
        sample_ability_two_button(&mut sampler, true, false).expect("press");
        assert_eq!(sampler.latest.ability_2_press_sequence, 1);
        sample_ability_two_button(&mut sampler, true, false).expect("hold");
        assert_eq!(sampler.latest.ability_2_press_sequence, 1);
        sample_ability_two_button(&mut sampler, false, false).expect("release");
        sample_ability_two_button(&mut sampler, true, true).expect("blocked press");
        sample_ability_two_button(&mut sampler, true, false).expect("suppressed");
        assert_eq!(sampler.latest.ability_2_press_sequence, 1);
        sample_ability_two_button(&mut sampler, false, false).expect("rearm");
        sample_ability_two_button(&mut sampler, true, false).expect("fresh press");
        assert_eq!(sampler.latest.ability_2_press_sequence, 2);
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

        let mut sampler = CombatInputSampler::default();
        sampler.latest.ability_1_press_sequence = u32::MAX;
        assert_eq!(
            sample_ability_one_button(&mut sampler, true, false),
            Err(PrimarySequenceError::Exhausted)
        );
        assert_eq!(sampler.latest.ability_1_press_sequence, u32::MAX);
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
    #[allow(clippy::too_many_lines)]
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
        assert_eq!(
            EvidenceScenario::from_value(Some(GRAVE_MARK_SHOWCASE_SCENARIO), true)
                .expect("Grave Mark scenario"),
            EvidenceScenario::GraveMarkShowcase
        );
        assert_eq!(
            EvidenceScenario::from_value(Some(SLIPSTEP_SHOWCASE_SCENARIO), true)
                .expect("Slipstep scenario"),
            EvidenceScenario::SlipstepShowcase
        );
        assert_eq!(
            EvidenceScenario::from_value(Some(STILLNESS_SHOWCASE_SCENARIO), true)
                .expect("Stillness scenario"),
            EvidenceScenario::StillnessShowcase
        );
        assert!(EvidenceScenario::from_value(Some("unknown"), true).is_err());
        assert_eq!(
            EvidenceScenario::from_value(Some(RED_TONIC_SHOWCASE_SCENARIO), true)
                .expect("scenario"),
            EvidenceScenario::RedTonicShowcase
        );
        assert_eq!(
            EvidenceScenario::from_value(Some(ENEMY_SHOWCASE_SCENARIO), true).expect("scenario"),
            EvidenceScenario::EnemyShowcase
        );
        assert_eq!(
            EvidenceScenario::from_value(Some(ENEMY_DEATH_SHOWCASE_SCENARIO), true)
                .expect("scenario"),
            EvidenceScenario::EnemyDeathShowcase
        );
        assert_eq!(
            EvidenceScenario::from_value(Some(DAMAGE_LETHAL_SHOWCASE_SCENARIO), true)
                .expect("scenario"),
            EvidenceScenario::DamageLethalShowcase
        );
        assert_eq!(
            EvidenceScenario::from_value(Some(DAMAGE_GRACE_SHOWCASE_SCENARIO), true)
                .expect("scenario"),
            EvidenceScenario::DamageGraceShowcase
        );
        assert_eq!(
            EvidenceScenario::from_value(Some(DEATH_RESTART_SHOWCASE_SCENARIO), true)
                .expect("scenario"),
            EvidenceScenario::DeathRestartShowcase
        );
        assert_eq!(
            EvidenceScenario::from_value(Some(DEATH_RECAP_SHOWCASE_SCENARIO), true)
                .expect("scenario"),
            EvidenceScenario::DeathRecapShowcase
        );
        assert_eq!(
            EvidenceScenario::from_value(Some(INVENTORY_SHOWCASE_SCENARIO), true)
                .expect("scenario"),
            EvidenceScenario::InventoryShowcase
        );
        assert_eq!(
            EvidenceScenario::from_value(Some(ITEM_CATALOG_SHOWCASE_SCENARIO), true)
                .expect("scenario"),
            EvidenceScenario::ItemCatalogShowcase
        );
        assert_eq!(
            EvidenceScenario::from_value(Some(DEBUG_OVERLAY_SHOWCASE_SCENARIO), true)
                .expect("scenario"),
            EvidenceScenario::DebugOverlayShowcase
        );
        assert_eq!(
            EvidenceScenario::from_value(Some(DEBUG_TOOLS_SHOWCASE_SCENARIO), true)
                .expect("scenario"),
            EvidenceScenario::DebugToolsShowcase
        );
        assert_eq!(
            EvidenceScenario::from_value(Some(BOSS_SHOWCASE_SCENARIO), true).expect("scenario"),
            EvidenceScenario::BossShowcase
        );
        assert_eq!(
            EvidenceScenario::from_value(Some(BOSS_COMPLETION_SHOWCASE_SCENARIO), true)
                .expect("scenario"),
            EvidenceScenario::BossCompletionShowcase
        );
        assert_eq!(
            EvidenceScenario::from_value(Some(STRESS_FULL_SCENARIO), true).expect("scenario"),
            EvidenceScenario::StressFull
        );
        assert_eq!(
            EvidenceScenario::from_value(Some(STRESS_REDUCED_SCENARIO), true).expect("scenario"),
            EvidenceScenario::StressReduced
        );
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
