//! Complete renderer-independent Choir Abbot combat fixture.

use sim_core::{
    ArenaGeometry, CombatStep, CoreAbbotEvent, CoreAbbotSimulation, CoreAbbotStep,
    CoreEnemyDefinition, EnemyHealthActor, EnemyHealthSimulation, EnemyHealthSnapshot,
    EnemyHealthStep, EnemyLabPlayer, EntityId, EntityIdAllocator, HostileDamagePolicy,
    HostileEvent, HostileProjectile, HostileProjectileSimulation, HostileStep, Tick,
};
use thiserror::Error;

use crate::CoreDevelopmentEncounterRooms;

const INTRODUCTION_TICKS: u64 = 90;
const QUIET_TICKS: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreAbbotFixturePhase {
    Active,
    Quiet,
    Cleared,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreAbbotRewardHandoff {
    pub actor_id: EntityId,
    pub participant_id: EntityId,
    pub death_tick: Tick,
    pub reward_due_tick: Tick,
    pub reward_profile_id: String,
    pub xp_profile_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreAbbotFixtureStep {
    pub tick: Tick,
    pub phase_after: CoreAbbotFixturePhase,
    pub health: EnemyHealthStep,
    pub abbot: Option<CoreAbbotStep>,
    pub hostile_spawn_events: Vec<HostileEvent>,
    pub hostile_step: HostileStep,
    pub cleared_projectiles: Vec<HostileProjectile>,
    pub reward_handoff: Option<CoreAbbotRewardHandoff>,
}

#[derive(Debug, Clone)]
pub struct CoreAbbotFixtureSimulation {
    definition: CoreEnemyDefinition,
    arena: ArenaGeometry,
    actor_id: EntityId,
    spawn: sim_core::CoreWorldPosition,
    player: EnemyLabPlayer,
    health: EnemyHealthSimulation,
    abbot: CoreAbbotSimulation,
    hostile: HostileProjectileSimulation,
    damage_policy: HostileDamagePolicy,
    phase: CoreAbbotFixturePhase,
    tick: Tick,
    attacks_enabled_at: Tick,
    quiet_ends_at: Option<Tick>,
}

impl CoreAbbotFixtureSimulation {
    pub fn new(
        content: &CoreDevelopmentEncounterRooms,
        actor_id: EntityId,
        player: EnemyLabPlayer,
        projectile_ids: EntityIdAllocator,
    ) -> Result<Self, CoreAbbotFixtureError> {
        let definition =
            super::core_fixed_room_encounter::authored_definition(content, "miniboss.choir_abbot")?;
        if definition.parameters().maximum_health != 1_900
            || definition.parameters().armor != 6
            || definition.parameters().reward_profile_id != "reward.miniboss_t1"
            || definition.parameters().xp_profile_id != "xp.miniboss_t1"
        {
            return Err(CoreAbbotFixtureError::DefinitionDrift);
        }
        let room = content
            .compile_room_definitions()?
            .into_iter()
            .find(|room| room.id == "room.bell.choir_01")
            .ok_or(CoreAbbotFixtureError::DefinitionDrift)?
            .rotated(0)?;
        let arena = super::core_fixed_room_encounter::combat_arena(&room)?;
        if arena.width_milli_tiles != 19_000
            || arena.height_milli_tiles != 15_000
            || arena.pillars.len() != 4
        {
            return Err(CoreAbbotFixtureError::DefinitionDrift);
        }
        let spawn = sim_core::CoreWorldPosition::new(9_500, 7_500);
        let health = build_health(actor_id, &definition, spawn, Tick(0))?;
        let abbot = CoreAbbotSimulation::new(definition.clone(), actor_id, spawn)?;
        Ok(Self {
            definition,
            arena,
            actor_id,
            spawn,
            player,
            health,
            abbot,
            hostile: HostileProjectileSimulation::with_allocator(projectile_ids),
            damage_policy: HostileDamagePolicy::Standard,
            phase: CoreAbbotFixturePhase::Active,
            tick: Tick(0),
            attacks_enabled_at: Tick(INTRODUCTION_TICKS),
            quiet_ends_at: None,
        })
    }

    #[must_use]
    pub const fn phase(&self) -> CoreAbbotFixturePhase {
        self.phase
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    #[must_use]
    pub fn snapshot(&self) -> EnemyHealthSnapshot {
        self.health
            .snapshots()
            .into_iter()
            .next()
            .expect("Abbot fixture always owns one health record")
    }

    pub fn set_damage_policy(&mut self, policy: HostileDamagePolicy) {
        self.damage_policy = policy;
        self.hostile.set_damage_policy(policy);
    }

    pub fn step(
        &mut self,
        combat: &CombatStep,
    ) -> Result<CoreAbbotFixtureStep, CoreAbbotFixtureError> {
        let mut staged = self.clone();
        let step = staged.step_inner(combat)?;
        *self = staged;
        Ok(step)
    }

    /// Restores the authored fixture state at the current tick without rewinding projectile IDs.
    pub fn reset(&mut self) -> Result<Vec<HostileProjectile>, CoreAbbotFixtureError> {
        let cleared = self.hostile.clear_projectiles();
        self.health = build_health(self.actor_id, &self.definition, self.spawn, self.tick)?;
        self.abbot.reset()?;
        self.attacks_enabled_at = add_ticks(self.tick, INTRODUCTION_TICKS)?;
        self.phase = CoreAbbotFixturePhase::Active;
        self.quiet_ends_at = None;
        Ok(cleared)
    }

    fn step_inner(
        &mut self,
        combat: &CombatStep,
    ) -> Result<CoreAbbotFixtureStep, CoreAbbotFixtureError> {
        if combat.tick != self.tick {
            return Err(CoreAbbotFixtureError::TickMismatch);
        }
        if self.phase == CoreAbbotFixturePhase::Quiet
            && self.quiet_ends_at.is_some_and(|due| self.tick >= due)
        {
            self.phase = CoreAbbotFixturePhase::Cleared;
            self.quiet_ends_at = None;
        }
        let mut health = EnemyHealthStep {
            tick: self.tick,
            ..EnemyHealthStep::default()
        };
        let mut abbot_step = None;
        let mut hostile_spawn_events = Vec::new();
        let mut cleared_projectiles = Vec::new();
        let mut reward_handoff = None;
        let hostile_step;
        if self.phase == CoreAbbotFixturePhase::Active {
            health = self.health.apply_combat_step(combat)?;
            if let Some(death) = health.death_events.first() {
                reward_handoff = Some(CoreAbbotRewardHandoff {
                    actor_id: self.actor_id,
                    participant_id: self.player.target.entity_id,
                    death_tick: death.tick,
                    reward_due_tick: death.reward_due_tick,
                    reward_profile_id: self.definition.parameters().reward_profile_id.clone(),
                    xp_profile_id: self.definition.parameters().xp_profile_id.clone(),
                });
                self.phase = CoreAbbotFixturePhase::Quiet;
                self.quiet_ends_at = Some(add_ticks(self.tick, QUIET_TICKS)?);
                cleared_projectiles = self.hostile.clear_projectiles();
                hostile_step = HostileStep {
                    tick: self.hostile.tick(),
                    events: Vec::new(),
                };
            } else {
                let candidates =
                    super::core_fixed_room_encounter::player_target_candidates(&self.player)?;
                let step = self
                    .abbot
                    .advance(&candidates, self.tick >= self.attacks_enabled_at)?;
                for event in &step.events {
                    if matches!(
                        event,
                        CoreAbbotEvent::RotorVolleyReleased { .. }
                            | CoreAbbotEvent::RecoveryRingReleased { .. }
                    ) {
                        hostile_spawn_events.extend(self.hostile.spawn_from_core_abbot_event(
                            self.actor_id,
                            &self.definition,
                            event,
                        )?);
                    }
                }
                hostile_step = self.hostile.step(
                    &self.arena,
                    &mut self.player.target,
                    &mut self.player.consumables,
                    &mut self.player.combat,
                )?;
                abbot_step = Some(step);
            }
        } else {
            if !combat.raw_damage_intents.is_empty() || !combat.collisions.is_empty() {
                return Err(CoreAbbotFixtureError::DamageAfterDefeat);
            }
            hostile_step = HostileStep {
                tick: self.hostile.tick(),
                events: Vec::new(),
            };
        }
        let step = CoreAbbotFixtureStep {
            tick: self.tick,
            phase_after: self.phase,
            health,
            abbot: abbot_step,
            hostile_spawn_events,
            hostile_step,
            cleared_projectiles,
            reward_handoff,
        };
        self.tick = self
            .tick
            .checked_next()
            .ok_or(CoreAbbotFixtureError::TickOverflow)?;
        Ok(step)
    }
}

fn build_health(
    actor_id: EntityId,
    definition: &CoreEnemyDefinition,
    spawn: sim_core::CoreWorldPosition,
    spawned_at: Tick,
) -> Result<EnemyHealthSimulation, CoreAbbotFixtureError> {
    Ok(EnemyHealthSimulation::new(vec![
        EnemyHealthActor::core_authored(
            actor_id,
            definition,
            super::core_fixed_room_encounter::core_position_vector(spawn),
            spawned_at,
        )?,
    ])?)
}

fn add_ticks(tick: Tick, amount: u64) -> Result<Tick, CoreAbbotFixtureError> {
    tick.0
        .checked_add(amount)
        .map(Tick)
        .ok_or(CoreAbbotFixtureError::TickOverflow)
}

#[derive(Debug, Error)]
pub enum CoreAbbotFixtureError {
    #[error("Choir Abbot fixture content drifted from its exact Core contract")]
    DefinitionDrift,
    #[error("Choir Abbot fixture input tick diverged from authority")]
    TickMismatch,
    #[error("Choir Abbot fixture received damage after defeat")]
    DamageAfterDefeat,
    #[error("Choir Abbot fixture tick arithmetic overflowed")]
    TickOverflow,
    #[error(transparent)]
    Content(#[from] anyhow::Error),
    #[error(transparent)]
    Room(#[from] sim_core::DungeonRoomError),
    #[error(transparent)]
    FixedRoom(#[from] crate::CoreFixedRoomEncounterError),
    #[error(transparent)]
    Abbot(#[from] sim_core::CoreAbbotError),
    #[error(transparent)]
    Health(#[from] sim_core::EnemyHealthError),
    #[error(transparent)]
    Hostile(#[from] sim_core::HostileError),
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroU64, path::Path};

    use super::*;
    use crate::load_core_development_encounter_rooms;
    use sim_core::{
        CollisionTarget, CoreWorldPosition, FriendlyProjectileSource, HostileTargetState,
        PlayerVitals, ProjectileCollision, RawDamageIntent, RawDamageIntentSource,
        RedTonicSimulation, SimulationVector, TonicBelt,
    };

    fn content_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn fixture() -> CoreAbbotFixtureSimulation {
        let root = content_root();
        let content = load_core_development_encounter_rooms(&root).expect("Core content");
        let (source, _) = crate::load_and_validate(&root).expect("FP source");
        let combat = crate::first_playable_authority_combat_test(&source).expect("combat fixture");
        let definitions = combat.definitions;
        let player = EnemyLabPlayer {
            target: HostileTargetState {
                entity_id: EntityId::new(900).expect("player ID"),
                position: SimulationVector::new(3.0, 7.5),
                target_is_immune: false,
                resistance_basis_points: definitions.resistance_basis_points,
                additional_direct_damage_reductions_basis_points: Vec::new(),
                armor: definitions.starting_armor,
                current_barrier: 0,
                health_damage_cap_basis_points: None,
            },
            consumables: RedTonicSimulation::new(
                definitions.red_tonic,
                PlayerVitals::new(definitions.maximum_health, definitions.maximum_health)
                    .expect("vitals"),
                TonicBelt::first_playable(),
            )
            .expect("tonic"),
            combat: definitions.combat,
        };
        let mut fixture = CoreAbbotFixtureSimulation::new(
            &content,
            EntityId::new(70_000).expect("Abbot ID"),
            player,
            EntityIdAllocator::starting_at(NonZeroU64::new(90_000).expect("projectile allocator")),
        )
        .expect("Abbot fixture");
        fixture.set_damage_policy(HostileDamagePolicy::DebugInvulnerable);
        fixture
    }

    fn empty_step(tick: u64) -> CombatStep {
        CombatStep {
            tick: Tick(tick),
            ..CombatStep::default()
        }
    }

    fn lethal(actor_id: EntityId, tick: u64) -> CombatStep {
        let projectile_id = EntityId::new(99_500).expect("friendly projectile");
        CombatStep {
            tick: Tick(tick),
            collisions: vec![ProjectileCollision {
                tick: Tick(tick),
                projectile_id,
                source: FriendlyProjectileSource::Primary,
                target: CollisionTarget::Enemy(actor_id),
                final_position: SimulationVector::new(9.5, 7.5),
                distance_travelled_tiles: 1.0,
                contact_ordinal: 0,
                empowered_by_slipstep: false,
                focused_by_stillness: false,
                projectile_continues: false,
            }],
            raw_damage_intents: vec![RawDamageIntent {
                tick: Tick(tick),
                projectile_id,
                source: RawDamageIntentSource::Primary,
                target: actor_id,
                base_raw_damage: 10_000,
                multiplier_basis_points: 10_000,
                resolved_raw_damage: 10_000,
                contact_ordinal: 0,
            }],
            ..CombatStep::default()
        }
    }

    fn trace() -> Vec<(u64, String, u64)> {
        let mut fixture = fixture();
        let actor_id = fixture.snapshot().actor_id;
        let mut trace = Vec::new();
        for tick in 0..=1_110 {
            let combat = if tick == 1_050 {
                lethal(actor_id, tick)
            } else {
                empty_step(tick)
            };
            let step = fixture.step(&combat).expect("fixture step");
            if let Some(abbot) = step.abbot {
                for event in abbot.events {
                    let name = match event {
                        CoreAbbotEvent::RotorTelegraphStarted { .. } => "rotor_telegraph",
                        CoreAbbotEvent::RotorStarted { .. } => "rotor_start",
                        CoreAbbotEvent::RotorVolleyReleased { .. } => "rotor_volley",
                        CoreAbbotEvent::RecoveryStarted { .. } => "recovery",
                        CoreAbbotEvent::RecoveryOriginWarningStarted { .. } => "origin_warning",
                        CoreAbbotEvent::DirectionalGapPreviewStarted { .. } => "gap_preview",
                        CoreAbbotEvent::RecoveryRingReleased { .. } => "ring",
                        CoreAbbotEvent::TargetlessReset { .. } => "reset",
                    };
                    trace.push((tick, name.to_owned(), 0));
                }
            }
            for event in step.hostile_spawn_events {
                if let HostileEvent::Spawned { projectile, .. } = event {
                    trace.push((
                        tick,
                        projectile.pattern_id().to_owned(),
                        projectile.id().get(),
                    ));
                }
            }
            if tick == 1_050 {
                let reward = step.reward_handoff.expect("reward handoff");
                assert_eq!(reward.reward_profile_id, "reward.miniboss_t1");
                assert_eq!(reward.xp_profile_id, "xp.miniboss_t1");
                assert_eq!(step.phase_after, CoreAbbotFixturePhase::Quiet);
            } else {
                assert!(step.reward_handoff.is_none());
            }
        }
        assert_eq!(fixture.phase(), CoreAbbotFixturePhase::Cleared);
        trace
    }

    #[test]
    fn complete_35_second_abbot_trace_replays_exact_schedule_payloads_and_quiet() {
        let first = trace();
        assert_eq!(first, trace());
        assert!(first.iter().all(|(tick, name, _)| {
            *tick >= 90 || !matches!(name.as_str(), "rotor_start" | "rotor_volley" | "ring")
        }));
        for (tick, name) in [
            (90, "rotor_telegraph"),
            (110, "rotor_start"),
            (121, "rotor_volley"),
            (215, "recovery"),
            (215, "origin_warning"),
            (270, "gap_preview"),
            (275, "rotor_telegraph"),
            (290, "ring"),
            (290, "rotor_start"),
        ] {
            assert!(first.iter().any(|entry| entry.0 == tick && entry.1 == name));
        }
        assert_eq!(
            first
                .iter()
                .filter(|entry| entry.0 == 290 && entry.1 == "miniboss.choir_abbot.recovery_ring")
                .count(),
            12
        );
        assert_eq!(
            first
                .iter()
                .filter(|entry| entry.0 == 121 && entry.1 == "miniboss.choir_abbot.rotor")
                .count(),
            2
        );
    }

    #[test]
    fn reset_cancels_live_rotor_restores_health_and_restarts_introduction() {
        let mut fixture = fixture();
        for tick in 0..=121 {
            fixture.step(&empty_step(tick)).expect("pre-reset step");
        }
        let cleared = fixture.reset().expect("reset");
        assert_eq!(cleared.len(), 2);
        let snapshot = fixture.snapshot();
        assert_eq!(snapshot.current_health, 1_900);
        assert_eq!(snapshot.damageable_at, Tick(152));
        for tick in 122..212 {
            let step = fixture.step(&empty_step(tick)).expect("reintro");
            assert!(step.reward_handoff.is_none());
            assert!(step.hostile_spawn_events.is_empty());
        }
        let telegraph = fixture.step(&empty_step(212)).expect("restart boundary");
        assert!(telegraph.abbot.expect("Abbot").events.iter().any(|event| {
            matches!(
                event,
                CoreAbbotEvent::RotorTelegraphStarted {
                    first_use: true,
                    ..
                }
            )
        }));
    }

    #[test]
    fn rotor_phase_and_final_preview_lock_are_immutable() {
        let mut fixture = fixture();
        let mut phases = Vec::new();
        let mut preview_target = None;
        let mut ring = None;
        for tick in 0..=290 {
            if tick == 271 {
                fixture.player.target.position = SimulationVector::new(16.0, 7.5);
            }
            let step = fixture.step(&empty_step(tick)).expect("fixture step");
            if let Some(abbot) = step.abbot {
                for event in abbot.events {
                    match event {
                        CoreAbbotEvent::RotorVolleyReleased {
                            cycle_index: 0,
                            phase_milli_degrees,
                            ..
                        } => phases.push(phase_milli_degrees),
                        CoreAbbotEvent::DirectionalGapPreviewStarted { lock } => {
                            preview_target = Some(lock.target().position);
                        }
                        CoreAbbotEvent::RecoveryRingReleased {
                            lock,
                            emitted_indices,
                            omitted_indices,
                            ..
                        } => {
                            ring = Some((lock.target().position, emitted_indices, omitted_indices));
                        }
                        _ => {}
                    }
                }
            }
        }
        assert_eq!(
            phases,
            [
                0, 12_250, 24_500, 36_750, 49_000, 61_250, 73_500, 85_750, 98_000, 110_250,
            ]
        );
        assert_eq!(preview_target, Some(CoreWorldPosition::new(3_000, 7_500)));
        let (ring_target, emitted, omitted) = ring.expect("recovery ring");
        assert_eq!(ring_target, preview_target.expect("preview target"));
        assert_eq!(omitted, [6, 7, 8, 9]);
        assert_eq!(emitted, [0, 1, 2, 3, 4, 5, 10, 11, 12, 13, 14, 15]);
    }
}
