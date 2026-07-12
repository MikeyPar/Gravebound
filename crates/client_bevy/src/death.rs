use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use bevy::prelude::*;
use sim_core::{
    ArenaGeometry, BellProctorDefinition, EncounterInput, EncounterStep, EnemyLabDefinitions,
    EnemyLabPlayer, EntityId, FieldPickupAccess, LocalDeathCommit, LocalRestartCommit,
    LocalRunLifecycle, PickupOutcome, PlacementChoice, PlayerCombatState, PlayerMovementConfig,
    PlayerMovementState, PlayerVitals, PrototypeInventory, RedTonicDefinition, RedTonicSimulation,
    RewardChoice, RewardOutcome, RunEntityCounts, SimulationVector, Tick, TonicBelt,
};

use crate::{
    FixedSimulationSet, FrameSet,
    combat::{
        CollisionDiagnostics, CombatInputSampler, EvidenceScenario, ProjectilePresentation,
        TransientEffect,
    },
    consumable::{ConsumableDiagnostics, ConsumableInputSampler},
    enemies::{
        EnemyLabRuntime, EnemyPresentationState, HostileProjectilePresentation,
        NormalRewardPresentation,
    },
    player::{LatestMovementAction, PlayerSimulation},
};

const RESTART_KEY: KeyCode = KeyCode::KeyR;
const EVIDENCE_DEAD_FIXED_TICKS: u32 = 3;

#[derive(Debug, Clone, Resource)]
pub(crate) struct LocalRunFactory {
    arena: ArenaGeometry,
    definitions: EnemyLabDefinitions,
    boss_definition: BellProctorDefinition,
    initial_combat: PlayerCombatState,
    red_tonic: RedTonicDefinition,
    stats: LocalRunStats,
    normal_encounter: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalRunStats {
    pub movement_speed_tiles_per_second: f32,
    pub maximum_health: u32,
    pub armor: u32,
    pub resistance_basis_points: i32,
}

impl Default for LocalRunStats {
    fn default() -> Self {
        Self {
            movement_speed_tiles_per_second: sim_core::GRAVE_ARBALIST_SPEED_TILES_PER_SECOND,
            maximum_health: 128,
            armor: 2,
            resistance_basis_points: 0,
        }
    }
}

impl LocalRunFactory {
    pub(crate) const fn new(
        arena: ArenaGeometry,
        definitions: EnemyLabDefinitions,
        boss_definition: BellProctorDefinition,
        initial_combat: PlayerCombatState,
        red_tonic: RedTonicDefinition,
        stats: LocalRunStats,
        normal_encounter: bool,
    ) -> Self {
        Self {
            arena,
            definitions,
            boss_definition,
            initial_combat,
            red_tonic,
            stats,
            normal_encounter,
        }
    }

    pub(crate) fn build_run(
        &self,
        run_ordinal: u32,
        position: SimulationVector,
        current_health: u32,
    ) -> Result<(PlayerSimulation, EnemyLabRuntime)> {
        let movement = PlayerMovementState::new_with_config(
            position,
            PlayerMovementConfig {
                final_speed_tiles_per_second: self.stats.movement_speed_tiles_per_second,
                ..PlayerMovementConfig::default()
            },
            &self.arena,
        )
        .context("failed to construct run-qualified player movement")?;
        let base = u64::from(
            run_ordinal
                .checked_sub(1)
                .ok_or_else(|| anyhow!("run ordinal must be nonzero"))?,
        )
        .checked_mul(100_000)
        .ok_or_else(|| anyhow!("run-qualified player ID overflow"))?;
        let player_id = EntityId::new(
            base.checked_add(10_004)
                .ok_or_else(|| anyhow!("run-qualified player ID overflow"))?,
        )
        .ok_or_else(|| anyhow!("run-qualified player ID must be nonzero"))?;
        let consumables = RedTonicSimulation::new(
            self.red_tonic.clone(),
            PlayerVitals::new(current_health, self.stats.maximum_health)?,
            TonicBelt::first_playable(),
        )?;
        let runtime = EnemyLabRuntime::new_for_run(
            self.definitions.clone(),
            self.arena.clone(),
            EnemyLabPlayer {
                target: sim_core::HostileTargetState {
                    entity_id: player_id,
                    position,
                    target_is_immune: false,
                    resistance_basis_points: self.stats.resistance_basis_points,
                    additional_direct_damage_reductions_basis_points: Vec::new(),
                    armor: self.stats.armor,
                    current_barrier: 0,
                    health_damage_cap_basis_points: None,
                },
                consumables,
                combat: self.initial_combat.clone(),
            },
            self.boss_definition.clone(),
            run_ordinal,
            self.normal_encounter,
        )?;
        Ok((PlayerSimulation::new(movement), runtime))
    }

    pub(crate) fn build_fresh_run(
        &self,
        run_ordinal: u32,
    ) -> Result<(PlayerSimulation, EnemyLabRuntime)> {
        let spawn = PlayerMovementState::at_arena_spawn(&self.arena)
            .context("failed to resolve the fresh-run player spawn")?
            .position();
        self.build_run(run_ordinal, spawn, self.stats.maximum_health)
    }
}

#[derive(Debug, Resource)]
pub(crate) struct LocalDeathRuntime {
    lifecycle: LocalRunLifecycle,
    last_death: Option<LocalDeathCommit>,
    last_restart: Option<LocalRestartCommit>,
    dead_fixed_ticks: u32,
    frozen_ticks_before_restart: u32,
    restart_duration: Option<Duration>,
    removed_visuals: u32,
}

impl LocalDeathRuntime {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            lifecycle: LocalRunLifecycle::first_playable()?,
            last_death: None,
            last_restart: None,
            dead_fixed_ticks: 0,
            frozen_ticks_before_restart: 0,
            restart_duration: None,
            removed_visuals: 0,
        })
    }

    pub(crate) fn evidence_ready(&self) -> bool {
        self.last_death.is_some()
            && self.last_restart.is_some()
            && self.lifecycle.is_alive()
            && self
                .restart_duration
                .is_some_and(|duration| duration.as_secs_f64() < 3.0)
            && self.frozen_ticks_before_restart >= EVIDENCE_DEAD_FIXED_TICKS
    }

    pub(crate) fn death_evidence_ready(&self) -> bool {
        self.last_death.is_some()
            && self.last_restart.is_none()
            && !self.lifecycle.is_alive()
            && self.dead_fixed_ticks >= EVIDENCE_DEAD_FIXED_TICKS
    }

    pub(crate) const fn inventory(&self) -> &PrototypeInventory {
        self.lifecycle.inventory()
    }

    pub(crate) const fn encounter(&self) -> &sim_core::BellLaboratoryEncounter {
        self.lifecycle.encounter()
    }

    pub(crate) const fn phase(&self) -> &sim_core::LocalRunPhase {
        self.lifecycle.phase()
    }

    pub(crate) const fn last_restart(&self) -> Option<&LocalRestartCommit> {
        self.last_restart.as_ref()
    }

    pub(crate) fn advance_encounter(&mut self, input: EncounterInput) -> Result<EncounterStep> {
        Ok(self.lifecycle.advance_encounter(input)?)
    }

    pub(crate) fn restart_after_victory(&mut self) -> Result<sim_core::LocalVictoryRestartCommit> {
        Ok(self.lifecycle.restart_after_victory()?)
    }

    pub(crate) fn apply_field_pickup(
        &mut self,
        pickup: &mut sim_core::FieldPickup,
        choice: PlacementChoice,
        player_position: SimulationVector,
        access: FieldPickupAccess,
        now: Tick,
    ) -> Result<PickupOutcome> {
        Ok(self.lifecycle.inventory_mut()?.apply_field_pickup(
            pickup,
            choice,
            player_position,
            access,
            now,
        )?)
    }

    pub(crate) fn apply_reward_choice(
        &mut self,
        reward: &mut sim_core::FieldPickup,
        choice: RewardChoice,
        now: Tick,
    ) -> Result<RewardOutcome> {
        Ok(self
            .lifecycle
            .inventory_mut()?
            .apply_reward_choice(reward, choice, now)?)
    }
}

#[derive(Debug, Component)]
struct LocalDeathDiagnostics;

pub(crate) fn configure(app: &mut App) -> Result<()> {
    app.insert_resource(LocalDeathRuntime::new()?)
        .add_systems(Startup, spawn_death_diagnostics)
        .add_systems(
            FixedUpdate,
            advance_local_death.in_set(FixedSimulationSet::Death),
        )
        .add_systems(
            Update,
            update_death_diagnostics.in_set(FrameSet::Presentation),
        );
    Ok(())
}

#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::type_complexity // Bevy query filters express run-visual ownership at the type boundary.
)]
fn advance_local_death(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    scenario: Res<EvidenceScenario>,
    factory: Res<LocalRunFactory>,
    mut death: ResMut<LocalDeathRuntime>,
    mut runtime: ResMut<EnemyLabRuntime>,
    mut player: ResMut<PlayerSimulation>,
    mut combat_input: ResMut<CombatInputSampler>,
    mut collision_diagnostics: ResMut<CollisionDiagnostics>,
    mut consumable_input: ResMut<ConsumableInputSampler>,
    mut consumable_diagnostics: ResMut<ConsumableDiagnostics>,
    mut enemy_presentation: ResMut<EnemyPresentationState>,
    mut movement: ResMut<LatestMovementAction>,
    run_visuals: Query<
        (
            Entity,
            Option<&HostileProjectilePresentation>,
            Option<&ProjectilePresentation>,
            Option<&NormalRewardPresentation>,
            Option<&TransientEffect>,
        ),
        Or<(
            With<HostileProjectilePresentation>,
            With<ProjectilePresentation>,
            With<NormalRewardPresentation>,
            With<TransientEffect>,
        )>,
    >,
) {
    if death.lifecycle.is_alive() {
        let Some(observation) = runtime.pending_death().cloned() else {
            return;
        };
        let mut staged_runtime = runtime.clone();
        staged_runtime.take_pending_death();
        let cleanup = staged_runtime.clear_for_local_death();
        let mut reward_entities = 0_u32;
        let mut transient_effects = 0_u32;
        let mut visual_count = 0_u32;
        for (_, _, _, reward, transient) in &run_visuals {
            visual_count = visual_count.saturating_add(1);
            reward_entities = reward_entities.saturating_add(u32::from(reward.is_some()));
            transient_effects = transient_effects.saturating_add(u32::from(transient.is_some()));
        }
        let entities = RunEntityCounts {
            enemies: cleanup.enemies,
            hostile_projectiles: cleanup.hostile_projectiles,
            hostile_hazards: cleanup.hostile_hazards,
            friendly_projectiles: cleanup.friendly_projectiles,
            field_pickups: 0,
            reward_entities,
            transient_effects,
        };
        let commit = death
            .lifecycle
            .observe_damage(observation, entities)
            .expect("validated lethal LocalLab transaction must commit")
            .expect("pending death observation is lethal");
        *runtime = staged_runtime;
        for (entity, _, _, _, _) in &run_visuals {
            commands.entity(entity).despawn();
        }
        death.last_death = Some(commit);
        death.last_restart = None;
        death.dead_fixed_ticks = 0;
        death.restart_duration = None;
        death.removed_visuals = visual_count;
        return;
    }

    death.dead_fixed_ticks = death.dead_fixed_ticks.saturating_add(1);
    let automatic_evidence_restart = *scenario == EvidenceScenario::DeathRestartShowcase
        && death.dead_fixed_ticks >= EVIDENCE_DEAD_FIXED_TICKS;
    if !keyboard.just_pressed(RESTART_KEY) && !automatic_evidence_restart {
        return;
    }

    let restart_started = Instant::now();
    let mut staged_lifecycle = death.lifecycle.clone();
    let requested_at = staged_lifecycle.encounter().tick();
    let restart = staged_lifecycle
        .restart(requested_at)
        .expect("dead LocalLab run must accept one explicit restart");
    let (fresh_player, fresh_runtime) = factory
        .build_fresh_run(restart.new_run_ordinal)
        .expect("validated fresh LocalLab run must reconstruct");
    let elapsed = restart_started.elapsed();
    assert!(
        elapsed.as_secs_f64() < 3.0,
        "LocalLab restart exceeded three seconds"
    );
    *player = fresh_player;
    *runtime = fresh_runtime;
    death.lifecycle = staged_lifecycle;
    death.last_restart = Some(restart);
    death.frozen_ticks_before_restart = death.dead_fixed_ticks;
    death.restart_duration = Some(elapsed);
    *combat_input = CombatInputSampler::default();
    *collision_diagnostics = CollisionDiagnostics::default();
    *consumable_input = ConsumableInputSampler::default();
    *consumable_diagnostics = ConsumableDiagnostics::default();
    *enemy_presentation = EnemyPresentationState::default();
    *movement = LatestMovementAction::default();
}

#[allow(clippy::needless_pass_by_value)]
fn spawn_death_diagnostics(mut commands: Commands) {
    commands.spawn((
        Name::new("Local death transaction diagnostics"),
        LocalDeathDiagnostics,
        Text::new("LOCAL RUN 1 | ALIVE | DEATH TRANSACTION ARMED"),
        TextFont::from_font_size(13.0),
        TextColor(Color::srgb_u8(234, 222, 205)),
        Node {
            position_type: PositionType::Absolute,
            left: px(14),
            top: px(252),
            border: UiRect::all(px(1)),
            padding: UiRect::all(px(7)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(12, 10, 14, 226)),
        BorderColor::all(Color::srgba_u8(193, 137, 93, 210)),
    ));
}

#[allow(clippy::needless_pass_by_value)]
fn update_death_diagnostics(
    death: Res<LocalDeathRuntime>,
    runtime: Res<EnemyLabRuntime>,
    mut text: Single<&mut Text, With<LocalDeathDiagnostics>>,
) {
    let Some(commit) = &death.last_death else {
        text.0 = format!(
            "LOCAL RUN {} | ALIVE | DEATH TRANSACTION ARMED",
            death.lifecycle.encounter().run_ordinal()
        );
        return;
    };
    let trace_span = commit
        .trace
        .first()
        .zip(commit.trace.last())
        .map_or(0, |(first, last)| last.tick.0.saturating_sub(first.tick.0));
    if let (Some(restart), Some(duration)) = (&death.last_restart, death.restart_duration) {
        text.0 = format!(
            "RUN {} FRESH | CONTROL READY | DEFAULT SEED {:08X}\nDEATH {} FROZE {}T | RESTART {:.2}MS < 3000MS\nCLEANED {} LOGICAL / {} VISUAL | TRACE {} EVENTS / {}T\nSTARTER EQUIP {} | TONICS {} | OLD RUN IDS RETIRED",
            restart.new_run_ordinal,
            restart.seed,
            commit.cause.death_id.get(),
            death.frozen_ticks_before_restart,
            duration.as_secs_f64() * 1_000.0,
            commit.cleared_entity_total,
            death.removed_visuals,
            commit.trace.len(),
            trace_span,
            restart.equipped_starter_items,
            restart.starting_tonics,
        );
    } else {
        let lethal = &commit.cause.lethal;
        let timeline = commit
            .trace
            .iter()
            .rev()
            .take(5)
            .rev()
            .map(|entry| {
                format!(
                    "T{:03} {} {} -> {} HP",
                    entry.tick.0,
                    attack_label(&entry.pattern_id),
                    entry.final_damage,
                    entry.health_after
                )
            })
            .collect::<Vec<_>>()
            .join(" | ");
        text.0 = format!(
            "LOCAL DEATH RECAP | RUN {} FROZEN | DEATH {}\nHERO LOCAL PROTOTYPE | GRAVE ARBALIST | LEVEL N/A | LIFETIME {}.{}S\nKILLER {} | ATTACK {} | DAMAGE {} {} | SOURCE ({:.1},{:.1})\nLAST {}/5 DAMAGE: {}\nNETWORK LOCAL / NONPERSISTENT | LATER ACTIONS REJECTED\nLOST {} STACKS + {} TONICS + {} RUN ENTITIES | PRESERVED NONE | CREATED NONE\n[R] RUN AGAIN (PRIMARY) | CONTROL LOCKED | HOSTILES CLEARED {}",
            death.lifecycle.encounter().run_ordinal(),
            commit.cause.death_id.get(),
            lethal.tick.0 / u64::from(sim_core::TICKS_PER_SECOND),
            (lethal.tick.0 % u64::from(sim_core::TICKS_PER_SECOND)) * 10
                / u64::from(sim_core::TICKS_PER_SECOND),
            killer_label(lethal.source),
            attack_label(&lethal.pattern_id),
            lethal.final_damage,
            damage_type_label(lethal.damage_type),
            lethal.source_position.x,
            lethal.source_position.y,
            commit.trace.len().min(5),
            timeline,
            commit.inventory_cleanup.removed_stacks.len(),
            commit.inventory_cleanup.cleared_belt_tonics,
            commit.cleared_entity_total,
            !runtime.is_active(),
        );
    }
}

fn killer_label(source: EntityId) -> &'static str {
    match source.get() % 100_000 {
        10_001 => "DROWNED PILGRIM",
        10_002 => "BELL REED",
        10_003 => "CHAIN SENTRY",
        _ => "UNKNOWN AUTHORITY",
    }
}

fn attack_label(pattern_id: &str) -> &'static str {
    if pattern_id.contains("pilgrim") {
        "PILGRIM FAN"
    } else if pattern_id.contains("reed") {
        "BELL REED RING"
    } else if pattern_id.contains("sentry") {
        "SENTRY LANES"
    } else {
        "UNKNOWN PATTERN"
    }
}

const fn damage_type_label(damage_type: sim_core::DamageType) -> &'static str {
    match damage_type {
        sim_core::DamageType::Physical => "PHYSICAL",
        sim_core::DamageType::Veil => "VEIL",
    }
}
