//! Atomic Sir Caldus combat aggregate.
//!
//! This is the `GB-M03-03E` join point for the three-authority scheduler, scaled health,
//! fixed-point body, shared hostile projectile allocator, canonical player damage, and cleanup.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::{
    AppliedHostileDamage, ArenaGeometry, CoreBossParticipant, CoreBossParticipantLock,
    CoreCaldusBodyError, CoreCaldusBodyEvent, CoreCaldusBodySimulation, CoreCaldusBodyTarget,
    CoreCaldusDamageEvent, CoreCaldusDefeat, CoreCaldusError, CoreCaldusEvent,
    CoreCaldusFriendlyInput, CoreCaldusHealthError, CoreCaldusHealthSimulation, CoreCaldusInput,
    CoreCaldusProjectileRelease, CoreCaldusSimulation, CoreCaldusTargetInput, DamageType,
    EnemyLabPlayer, EntityId, EntityIdAllocator, HostileDamagePolicy, HostileError, HostileEvent,
    HostileProjectile, HostileProjectileSimulation, HostileStep, Tick,
    apply_hostile_contact_transaction_with_policy,
};

pub const CALDUS_CHARGE_CONTACT_DAMAGE: u32 = 48;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCaldusChargeDamageEvent {
    pub tick: Tick,
    pub cast_id: u64,
    pub participant: CoreBossParticipant,
    pub damage: AppliedHostileDamage,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreCaldusEncounterStep {
    pub tick: Tick,
    pub friendly_damage: Vec<CoreCaldusDamageEvent>,
    pub scheduler_events: Vec<CoreCaldusEvent>,
    pub body_events: Vec<CoreCaldusBodyEvent>,
    pub hostile_spawn_events: Vec<HostileEvent>,
    pub charge_damage: Vec<CoreCaldusChargeDamageEvent>,
    pub hostile_step: HostileStep,
    pub defeat: Option<CoreCaldusDefeat>,
    pub cleared_projectiles: Vec<HostileProjectile>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreCaldusEncounterSimulation {
    lock: CoreBossParticipantLock,
    arena: ArenaGeometry,
    scheduler: CoreCaldusSimulation,
    body: CoreCaldusBodySimulation,
    health: CoreCaldusHealthSimulation,
    hostile_projectiles: HostileProjectileSimulation,
    damage_policy: HostileDamagePolicy,
}

impl CoreCaldusEncounterSimulation {
    pub fn new(
        lock: CoreBossParticipantLock,
        arena: ArenaGeometry,
        entity_id: EntityId,
        projectile_ids: EntityIdAllocator,
    ) -> Result<Self, CoreCaldusEncounterError> {
        Ok(Self {
            scheduler: CoreCaldusSimulation::new(lock.clone())?,
            body: CoreCaldusBodySimulation::new(lock.clone())?,
            health: CoreCaldusHealthSimulation::new(lock.clone(), entity_id)?,
            hostile_projectiles: HostileProjectileSimulation::with_allocator(projectile_ids),
            lock,
            arena,
            damage_policy: HostileDamagePolicy::Standard,
        })
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.scheduler.tick()
    }

    #[must_use]
    pub const fn current_health(&self) -> u32 {
        self.health.current_health()
    }

    #[must_use]
    pub const fn maximum_health(&self) -> u32 {
        self.health.maximum_health()
    }

    #[must_use]
    pub const fn body(&self) -> &CoreCaldusBodySimulation {
        &self.body
    }

    #[must_use]
    pub fn hostile_projectiles(&self) -> &[HostileProjectile] {
        self.hostile_projectiles.projectiles()
    }

    pub fn set_damage_policy(&mut self, policy: HostileDamagePolicy) {
        self.damage_policy = policy;
        self.hostile_projectiles.set_damage_policy(policy);
    }

    pub fn step(
        &mut self,
        friendly_inputs: &[CoreCaldusFriendlyInput],
        players: &mut BTreeMap<EntityId, EnemyLabPlayer>,
    ) -> Result<CoreCaldusEncounterStep, CoreCaldusEncounterError> {
        let mut staged = self.clone();
        let mut staged_players = players.clone();
        let step = staged.step_inner(friendly_inputs, &mut staged_players)?;
        *self = staged;
        *players = staged_players;
        Ok(step)
    }

    fn step_inner(
        &mut self,
        friendly_inputs: &[CoreCaldusFriendlyInput],
        players: &mut BTreeMap<EntityId, EnemyLabPlayer>,
    ) -> Result<CoreCaldusEncounterStep, CoreCaldusEncounterError> {
        self.validate_player_map(players, friendly_inputs)?;
        let tick = self.tick();
        let health_step = self
            .health
            .apply_friendly_damage(self.scheduler.state(), friendly_inputs)?;
        let body_targets = self.body_targets(players);
        let scheduler_targets = self.scheduler_targets(&body_targets)?;
        let scheduler_events = self.scheduler.advance(&CoreCaldusInput {
            tick,
            current_health: self.health.current_health(),
            living_targets: scheduler_targets,
        })?;
        let body_events = self
            .body
            .advance(&self.arena, &scheduler_events, &body_targets)?;
        let cleared_projectiles = if scheduler_events
            .iter()
            .any(|event| matches!(event, CoreCaldusEvent::HostilesCleared { .. }))
        {
            self.hostile_projectiles.clear_projectiles()
        } else {
            Vec::new()
        };
        let mut hostile_spawn_events = Vec::new();
        for release in scheduler_releases(&scheduler_events, self.body.simulation_position())
            .into_iter()
            .chain(
                body_events
                    .iter()
                    .filter_map(CoreCaldusBodyEvent::projectile_release),
            )
        {
            hostile_spawn_events.extend(
                self.hostile_projectiles
                    .spawn_from_core_caldus_release(self.health.entity_id(), &release)?,
            );
        }
        let charge_damage = self.apply_charge_contacts(&body_events, players)?;
        let hostile_step = if health_step.defeat.is_some() {
            HostileStep {
                tick,
                events: Vec::new(),
            }
        } else {
            self.hostile_projectiles
                .step_players(&self.arena, players)?
        };
        Ok(CoreCaldusEncounterStep {
            tick,
            friendly_damage: health_step.damage,
            scheduler_events,
            body_events,
            hostile_spawn_events,
            charge_damage,
            hostile_step,
            defeat: health_step.defeat,
            cleared_projectiles,
        })
    }

    fn validate_player_map(
        &self,
        players: &BTreeMap<EntityId, EnemyLabPlayer>,
        friendly_inputs: &[CoreCaldusFriendlyInput],
    ) -> Result<(), CoreCaldusEncounterError> {
        for (id, player) in players {
            if *id != player.target.entity_id
                || !self
                    .lock
                    .participants
                    .iter()
                    .any(|participant| participant.entity_id == *id)
            {
                return Err(CoreCaldusEncounterError::PlayerMapMismatch);
            }
            if !player.target.position.is_finite() {
                return Err(CoreCaldusEncounterError::NonFinitePlayerPosition);
            }
        }
        if friendly_inputs
            .iter()
            .any(|input| !players.contains_key(&input.participant.entity_id))
        {
            return Err(CoreCaldusEncounterError::FriendlyPlayerMissing);
        }
        Ok(())
    }

    fn body_targets(
        &self,
        players: &BTreeMap<EntityId, EnemyLabPlayer>,
    ) -> Vec<CoreCaldusBodyTarget> {
        self.lock
            .participants
            .iter()
            .filter_map(|participant| {
                players
                    .get(&participant.entity_id)
                    .map(|player| CoreCaldusBodyTarget {
                        participant: *participant,
                        position: crate::CoreWorldPosition::new(
                            tiles_to_milli(player.target.position.x),
                            tiles_to_milli(player.target.position.y),
                        ),
                        living: player.consumables.vitals().current_health() > 0,
                        damageable: !player.target.target_is_immune,
                    })
            })
            .collect()
    }

    fn scheduler_targets(
        &self,
        targets: &[CoreCaldusBodyTarget],
    ) -> Result<Vec<CoreCaldusTargetInput>, CoreCaldusEncounterError> {
        let boss = self.body.position();
        targets
            .iter()
            .filter(|target| target.living)
            .map(|target| {
                Ok(CoreCaldusTargetInput {
                    participant: target.participant,
                    position_x_milli_tiles: target.position.x_milli_tiles,
                    position_y_milli_tiles: target.position.y_milli_tiles,
                    squared_distance_to_boss: u64::try_from(
                        boss.squared_distance_to(target.position),
                    )
                    .map_err(|_| CoreCaldusEncounterError::DistanceOverflow)?,
                })
            })
            .collect()
    }

    fn apply_charge_contacts(
        &self,
        events: &[CoreCaldusBodyEvent],
        players: &mut BTreeMap<EntityId, EnemyLabPlayer>,
    ) -> Result<Vec<CoreCaldusChargeDamageEvent>, CoreCaldusEncounterError> {
        let mut output = Vec::new();
        for event in events {
            let CoreCaldusBodyEvent::ChargeMoved {
                tick,
                cast_id,
                contacts,
                ..
            } = event
            else {
                continue;
            };
            for participant in contacts {
                let player = players
                    .get_mut(&participant.entity_id)
                    .ok_or(CoreCaldusEncounterError::PlayerMapMismatch)?;
                let damage = apply_hostile_contact_transaction_with_policy(
                    self.health.entity_id(),
                    CALDUS_CHARGE_CONTACT_DAMAGE,
                    DamageType::Physical,
                    &mut player.target,
                    &mut player.consumables,
                    &mut player.combat,
                    self.damage_policy,
                )?;
                output.push(CoreCaldusChargeDamageEvent {
                    tick: *tick,
                    cast_id: *cast_id,
                    participant: *participant,
                    damage,
                });
            }
        }
        Ok(output)
    }
}

fn scheduler_releases(
    events: &[CoreCaldusEvent],
    origin: crate::SimulationVector,
) -> Vec<CoreCaldusProjectileRelease> {
    events
        .iter()
        .filter_map(|event| match event {
            CoreCaldusEvent::ShieldFired {
                tick,
                cast_id,
                target_x_milli_tiles,
                target_y_milli_tiles,
                ..
            } => Some(CoreCaldusProjectileRelease::ShieldArc {
                tick: *tick,
                cast_id: *cast_id,
                origin,
                target_x_milli_tiles: *target_x_milli_tiles,
                target_y_milli_tiles: *target_y_milli_tiles,
            }),
            CoreCaldusEvent::BellRingFired {
                tick,
                cast_id,
                gap_start_index,
                ..
            } => Some(CoreCaldusProjectileRelease::BellRing {
                tick: *tick,
                cast_id: *cast_id,
                origin,
                gap_start_index: *gap_start_index,
            }),
            _ => None,
        })
        .collect()
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "validated arena positions are bounded to authored milli-tile geometry"
)]
fn tiles_to_milli(value: f32) -> i32 {
    (value * 1_000.0).round() as i32
}

#[derive(Debug, Error)]
pub enum CoreCaldusEncounterError {
    #[error("Caldus encounter player map contains an unknown or mismatched identity")]
    PlayerMapMismatch,
    #[error("Caldus friendly input has no matching authoritative player")]
    FriendlyPlayerMissing,
    #[error("Caldus encounter player position must be finite")]
    NonFinitePlayerPosition,
    #[error("Caldus squared target distance exceeds scheduler input range")]
    DistanceOverflow,
    #[error(transparent)]
    Scheduler(#[from] CoreCaldusError),
    #[error(transparent)]
    Body(#[from] CoreCaldusBodyError),
    #[error(transparent)]
    Health(#[from] CoreCaldusHealthError),
    #[error(transparent)]
    Hostile(#[from] HostileError),
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::*;
    use crate::{
        ArenaAnchor, CollisionTarget, FriendlyProjectileSource, GraveMarkDefinition,
        GraveMarkDefinitionParameters, HostileTargetState, PlayerCombatState, PlayerVitals,
        ProjectileCollision, RawDamageIntent, RawDamageIntentSource, RedTonicDefinition,
        RedTonicSimulation, SlipstepDefinition, SlipstepDefinitionParameters, StillnessDefinition,
        StillnessDefinitionParameters, TilePoint, TonicBelt, WeaponDefinition,
        WeaponDefinitionParameters,
    };

    fn id(value: u64) -> EntityId {
        EntityId::new(value).expect("entity")
    }

    fn participant() -> CoreBossParticipant {
        CoreBossParticipant {
            entity_id: id(10),
            party_slot: 0,
        }
    }

    fn lock() -> CoreBossParticipantLock {
        CoreBossParticipantLock {
            attempt_ordinal: 1,
            participants: vec![participant()],
            maximum_health: 7_200,
        }
    }

    fn arena() -> ArenaGeometry {
        ArenaGeometry {
            id: "arena.boss.caldus_01".to_owned(),
            width_milli_tiles: 18_000,
            height_milli_tiles: 18_000,
            shell_thickness_milli_tiles: 1_000,
            player_spawn: TilePoint::new(2_000, 9_000),
            boss_spawn: TilePoint::new(9_000, 9_000),
            pillars: Vec::new(),
            anchors: vec![ArenaAnchor {
                id: "stage".to_owned(),
                point: TilePoint::new(9_000, 9_000),
            }],
        }
        .validated()
        .expect("arena")
    }

    fn combat_state() -> PlayerCombatState {
        let weapon = WeaponDefinition::new(WeaponDefinitionParameters {
            content_id: "item.prototype.weapon.pine_crossbow".to_owned(),
            raw_damage: 20,
            attack_interval_ticks: 14,
            range_milli_tiles: 9_500,
            projectile_speed_milli_tiles_per_second: 12_000,
            projectile_radius_milli_tiles: 100,
            projectile_count: 1,
            projectile_directions_millionths: vec![(1_000_000, 0)],
            max_projectiles_per_target: 1,
            pierce: 0,
            stops_on_first_enemy: true,
        })
        .expect("weapon");
        let mark = GraveMarkDefinition::new(GraveMarkDefinitionParameters {
            content_id: "ability.arbalist.grave_mark".to_owned(),
            cooldown_ticks: 150,
            global_cooldown_ticks: 5,
            input_buffer_ticks: 3,
            projectile_speed_milli_tiles_per_second: 12_000,
            range_milli_tiles: 11_000,
            projectile_radius_milli_tiles: 120,
            weapon_damage_multiplier_basis_points: 18_000,
            duration_ticks: 120,
            marked_primary_bonus_basis_points: 1_500,
            maximum_marked_targets: 1,
            consumes_on_solid: true,
        })
        .expect("mark");
        let slip = SlipstepDefinition::new(SlipstepDefinitionParameters {
            content_id: "ability.arbalist.slipstep".to_owned(),
            cooldown_ticks: 240,
            global_cooldown_ticks: 5,
            input_buffer_ticks: 3,
            travel_milli_tiles: 2_000,
            travel_ticks: 5,
            direct_damage_reduction_basis_points: 2_500,
            empowered_window_ticks: 45,
            projectile_speed_bonus_basis_points: 3_000,
            pierce_bonus: 1,
            exhaustion_ticks: 45,
        })
        .expect("slip");
        let stillness = StillnessDefinition::new(StillnessDefinitionParameters {
            content_id: "ability.arbalist.stillness".to_owned(),
            activation_ticks: 18,
            movement_threshold_basis_points: 2_000,
            projectile_speed_bonus_basis_points: 1_000,
            primary_damage_bonus_basis_points: 800,
            break_on_damage: true,
            break_on_slipstep: true,
        })
        .expect("stillness");
        PlayerCombatState::new(weapon, mark, slip, stillness).expect("combat")
    }

    fn players() -> BTreeMap<EntityId, EnemyLabPlayer> {
        BTreeMap::from([(
            id(10),
            EnemyLabPlayer {
                target: HostileTargetState {
                    entity_id: id(10),
                    position: crate::SimulationVector::new(14.0, 9.0),
                    target_is_immune: false,
                    resistance_basis_points: 0,
                    additional_direct_damage_reductions_basis_points: Vec::new(),
                    armor: 2,
                    current_barrier: 0,
                    health_damage_cap_basis_points: None,
                },
                consumables: RedTonicSimulation::new(
                    RedTonicDefinition::first_playable(),
                    PlayerVitals::new(128, 128).expect("vitals"),
                    TonicBelt::first_playable(),
                )
                .expect("tonic"),
                combat: combat_state(),
            },
        )])
    }

    fn threshold_damage() -> CoreCaldusFriendlyInput {
        let projectile_id = id(500);
        CoreCaldusFriendlyInput {
            participant: participant(),
            combat: crate::CombatStep {
                tick: Tick(0),
                collisions: vec![ProjectileCollision {
                    tick: Tick(0),
                    projectile_id,
                    source: FriendlyProjectileSource::Primary,
                    target: CollisionTarget::Enemy(id(99)),
                    final_position: crate::SimulationVector::new(9.0, 9.0),
                    distance_travelled_tiles: 1.0,
                    contact_ordinal: 0,
                    empowered_by_slipstep: false,
                    focused_by_stillness: false,
                    projectile_continues: false,
                }],
                raw_damage_intents: vec![RawDamageIntent {
                    tick: Tick(0),
                    projectile_id,
                    source: RawDamageIntentSource::Primary,
                    target: id(99),
                    base_raw_damage: 2_500,
                    multiplier_basis_points: 10_000,
                    resolved_raw_damage: 2_500,
                    contact_ordinal: 0,
                }],
                ..crate::CombatStep::default()
            },
        }
    }

    fn encounter() -> CoreCaldusEncounterSimulation {
        CoreCaldusEncounterSimulation::new(
            lock(),
            arena(),
            id(99),
            EntityIdAllocator::starting_at(NonZeroU64::new(1_000).expect("nonzero")),
        )
        .expect("encounter")
    }

    #[test]
    fn threshold_to_charge_contact_and_realized_stop_ring_is_atomic() {
        let mut encounter = encounter();
        let mut players = players();
        let transition = encounter
            .step(&[threshold_damage()], &mut players)
            .expect("threshold");
        assert!(transition.scheduler_events.iter().any(|event| matches!(
            event,
            CoreCaldusEvent::BreakStarted {
                entering: crate::CoreCaldusPhase::Phase2,
                ..
            }
        )));
        let mut charge_damage = Vec::new();
        let mut stop_ring_projectiles = 0;
        for _ in 1..=167 {
            let step = encounter.step(&[], &mut players).expect("advance");
            charge_damage.extend(step.charge_damage);
            if step
                .body_events
                .iter()
                .any(|event| matches!(event, CoreCaldusBodyEvent::ChargeStopRingReleased { .. }))
            {
                stop_ring_projectiles = step.hostile_spawn_events.len();
            }
        }
        assert_eq!(charge_damage.len(), 1);
        assert_eq!(charge_damage[0].participant, participant());
        assert_eq!(
            charge_damage[0].damage.damage.raw_damage,
            CALDUS_CHARGE_CONTACT_DAMAGE
        );
        assert_eq!(stop_ring_projectiles, 12);
        assert!(players[&id(10)].consumables.vitals().current_health() < 128);
    }

    #[test]
    fn invalid_external_identity_rolls_back_encounter_and_players() {
        let mut encounter = encounter();
        let before_encounter = encounter.clone();
        let mut players = players();
        players.get_mut(&id(10)).expect("player").target.entity_id = id(11);
        let before_players = players.clone();
        assert!(matches!(
            encounter.step(&[], &mut players),
            Err(CoreCaldusEncounterError::PlayerMapMismatch)
        ));
        assert_eq!(encounter, before_encounter);
        assert_eq!(players, before_players);
    }
}
