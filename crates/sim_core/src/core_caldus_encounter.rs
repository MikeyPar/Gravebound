//! Atomic Sir Caldus combat aggregate.
//!
//! This is the `GB-M03-03E` join point for the three-authority scheduler, scaled health,
//! fixed-point body, shared hostile projectile allocator, canonical player damage, and cleanup.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::{
    AppliedHostileDamage, ArenaGeometry, CALDUS_COLLISION_RADIUS_TILES, CollisionError,
    CoreBossParticipant, CoreBossParticipantLock, CoreCaldusBodyError, CoreCaldusBodyEvent,
    CoreCaldusBodySimulation, CoreCaldusBodyTarget, CoreCaldusChargeAxis, CoreCaldusDamageEvent,
    CoreCaldusDefeat, CoreCaldusError, CoreCaldusEvent, CoreCaldusFriendlyInput,
    CoreCaldusHealthError, CoreCaldusHealthSimulation, CoreCaldusInput,
    CoreCaldusProjectileRelease, CoreCaldusSimulation, CoreCaldusTargetInput, DamageType,
    EnemyLabPlayer, EntityId, EntityIdAllocator, HostileDamagePolicy, HostileError, HostileEvent,
    HostileProjectile, HostileProjectileSimulation, HostileStep, PLAYER_COLLISION_RADIUS_TILES,
    ProjectileCollisionWorld, SimulationVector, Tick,
    apply_hostile_contact_transaction_with_policy,
};

pub const CALDUS_CHARGE_CONTACT_DAMAGE: u32 = 48;
pub const CORE_CALDUS_ENTITY_ID_OFFSET: u64 = 40_002;

/// Derives Sir Caldus's stable run-qualified identity in the boss namespace. The adjacent
/// `40_001` slot belongs to the Bell Proctor and normal enemies occupy `30_001..=39_999`.
pub fn core_caldus_entity_id(run_ordinal: u32) -> Result<EntityId, CoreCaldusEncounterError> {
    let zero_based = run_ordinal
        .checked_sub(1)
        .ok_or(CoreCaldusEncounterError::ZeroRunOrdinal)?;
    let value = u64::from(zero_based)
        .checked_mul(crate::RUN_ENTITY_ID_STRIDE)
        .and_then(|base| base.checked_add(CORE_CALDUS_ENTITY_ID_OFFSET))
        .ok_or(CoreCaldusEncounterError::EntityIdOverflow)?;
    EntityId::new(value).ok_or(CoreCaldusEncounterError::EntityIdOverflow)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCaldusChargeDamageEvent {
    pub tick: Tick,
    pub cast_id: u64,
    pub participant: CoreBossParticipant,
    pub damage: AppliedHostileDamage,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoreCaldusPlayerSeparationEvent {
    pub tick: Tick,
    pub participant: CoreBossParticipant,
    pub boss_position: SimulationVector,
    pub from: SimulationVector,
    pub to: SimulationVector,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreCaldusEncounterStep {
    pub tick: Tick,
    pub friendly_damage: Vec<CoreCaldusDamageEvent>,
    pub scheduler_events: Vec<CoreCaldusEvent>,
    pub body_events: Vec<CoreCaldusBodyEvent>,
    pub hostile_spawn_events: Vec<HostileEvent>,
    pub player_separations: Vec<CoreCaldusPlayerSeparationEvent>,
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
        Self::new_at_tick(lock, arena, entity_id, projectile_ids, Tick(0))
    }

    pub fn new_at_tick(
        lock: CoreBossParticipantLock,
        arena: ArenaGeometry,
        entity_id: EntityId,
        projectile_ids: EntityIdAllocator,
        start_tick: Tick,
    ) -> Result<Self, CoreCaldusEncounterError> {
        Ok(Self {
            scheduler: CoreCaldusSimulation::new_at_tick(lock.clone(), start_tick)?,
            body: CoreCaldusBodySimulation::new_at_tick(lock.clone(), start_tick)?,
            health: CoreCaldusHealthSimulation::new_at_tick(lock.clone(), entity_id, start_tick)?,
            hostile_projectiles: HostileProjectileSimulation::with_allocator_at_tick(
                projectile_ids,
                start_tick,
            ),
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
    pub const fn arena(&self) -> &ArenaGeometry {
        &self.arena
    }

    #[must_use]
    pub const fn state(&self) -> &crate::CoreCaldusState {
        self.scheduler.state()
    }

    pub fn hurtbox(&self) -> Result<Option<crate::EnemyHurtbox>, CoreCaldusEncounterError> {
        self.health
            .hurtbox(self.body.simulation_position())
            .map_err(Into::into)
    }

    pub fn body_collider(&self) -> Result<crate::EnemyBodyCollider, CoreCaldusEncounterError> {
        crate::EnemyBodyCollider::new(
            self.health.entity_id(),
            self.body.simulation_position(),
            crate::CALDUS_COLLISION_RADIUS_TILES,
        )
        .map_err(CoreCaldusHealthError::from)
        .map_err(Into::into)
    }

    #[must_use]
    pub fn hostile_projectiles(&self) -> &[HostileProjectile] {
        self.hostile_projectiles.projectiles()
    }

    pub fn set_damage_policy(&mut self, policy: HostileDamagePolicy) {
        self.damage_policy = policy;
        self.hostile_projectiles.set_damage_policy(policy);
    }

    /// Clears attempt-local hostiles and returns the same monotonic projectile allocator for a
    /// DNG-006 reset. Consuming the encounter prevents a caller from retaining defeated or
    /// abandoned boss authority beside the recovered allocator.
    #[must_use]
    pub fn into_cleared_projectile_allocator(mut self) -> EntityIdAllocator {
        self.hostile_projectiles.clear_projectiles();
        self.hostile_projectiles.into_allocator()
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
        let player_separations = self.resolve_charge_overlaps(&body_events, players)?;
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
            player_separations,
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

    fn resolve_charge_overlaps(
        &self,
        events: &[CoreCaldusBodyEvent],
        players: &mut BTreeMap<EntityId, EnemyLabPlayer>,
    ) -> Result<Vec<CoreCaldusPlayerSeparationEvent>, CoreCaldusEncounterError> {
        let Some((tick, charge_axis)) = events.iter().find_map(|event| match event {
            CoreCaldusBodyEvent::ChargeMoved { tick, axis, .. } => Some((*tick, *axis)),
            _ => None,
        }) else {
            return Ok(Vec::new());
        };
        let boss_position = self.body.simulation_position();
        let static_world = ProjectileCollisionWorld::new(&self.arena, Vec::new())?;
        let combined_radius = CALDUS_COLLISION_RADIUS_TILES + PLAYER_COLLISION_RADIUS_TILES;
        let combined_squared = combined_radius * combined_radius;
        let reverse_axis = reverse_charge_axis(charge_axis);
        let cardinal_directions = clockwise_cardinals_from(reverse_axis);
        let mut participants = self.lock.participants.clone();
        participants.sort_by_key(|participant| (participant.party_slot, participant.entity_id));
        let mut output = Vec::new();
        for participant in participants {
            let player = players
                .get_mut(&participant.entity_id)
                .ok_or(CoreCaldusEncounterError::PlayerMapMismatch)?;
            if player.consumables.vitals().current_health() == 0 {
                continue;
            }
            let from = player.target.position;
            let delta = from - boss_position;
            let distance_squared = delta.length_squared();
            if distance_squared >= combined_squared {
                continue;
            }
            let radial = if distance_squared > f32::EPSILON {
                delta * (combined_radius / distance_squared.sqrt())
            } else {
                reverse_axis * combined_radius
            };
            let candidates = std::iter::once(boss_position + radial).chain(
                cardinal_directions
                    .into_iter()
                    .map(|direction| boss_position + direction * combined_radius),
            );
            let mut legal = None;
            for candidate in candidates {
                if static_world.is_circle_clear(candidate, PLAYER_COLLISION_RADIUS_TILES)? {
                    legal = Some(candidate);
                    break;
                }
            }
            let to =
                legal.ok_or(CoreCaldusEncounterError::NoLegalPlayerSeparation { participant })?;
            player.target.position = to;
            output.push(CoreCaldusPlayerSeparationEvent {
                tick,
                participant,
                boss_position,
                from,
                to,
            });
        }
        Ok(output)
    }
}

const fn reverse_charge_axis(axis: CoreCaldusChargeAxis) -> SimulationVector {
    match axis {
        CoreCaldusChargeAxis::East => SimulationVector::new(-1.0, 0.0),
        CoreCaldusChargeAxis::South => SimulationVector::new(0.0, -1.0),
        CoreCaldusChargeAxis::West => SimulationVector::new(1.0, 0.0),
        CoreCaldusChargeAxis::North => SimulationVector::new(0.0, 1.0),
    }
}

fn clockwise_cardinals_from(first: SimulationVector) -> [SimulationVector; 4] {
    const CARDINALS: [SimulationVector; 4] = [
        SimulationVector::new(1.0, 0.0),
        SimulationVector::new(0.0, 1.0),
        SimulationVector::new(-1.0, 0.0),
        SimulationVector::new(0.0, -1.0),
    ];
    let start = CARDINALS
        .iter()
        .position(|direction| *direction == first)
        .unwrap_or(2);
    std::array::from_fn(|offset| CARDINALS[(start + offset) % CARDINALS.len()])
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
    #[error("Caldus encounter run ordinal must be nonzero")]
    ZeroRunOrdinal,
    #[error("Caldus encounter entity identity overflowed")]
    EntityIdOverflow,
    #[error("Caldus encounter player map contains an unknown or mismatched identity")]
    PlayerMapMismatch,
    #[error("Caldus friendly input has no matching authoritative player")]
    FriendlyPlayerMissing,
    #[error("Caldus encounter player position must be finite")]
    NonFinitePlayerPosition,
    #[error("Caldus squared target distance exceeds scheduler input range")]
    DistanceOverflow,
    #[error("Caldus charge could not legally separate participant {participant:?}")]
    NoLegalPlayerSeparation { participant: CoreBossParticipant },
    #[error(transparent)]
    Collision(#[from] CollisionError),
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

    #[test]
    fn caldus_identity_is_stable_run_qualified_and_disjoint() {
        assert_eq!(core_caldus_entity_id(1).expect("first").get(), 40_002);
        assert_eq!(core_caldus_entity_id(2).expect("second").get(), 140_002);
        assert_ne!(
            core_caldus_entity_id(1).expect("Caldus"),
            crate::normal_wave_entity_id(crate::SpawnInstanceId {
                run_ordinal: 1,
                spawn_ordinal: 9_999,
            })
            .expect("normal enemy")
        );
        assert!(matches!(
            core_caldus_entity_id(0),
            Err(CoreCaldusEncounterError::ZeroRunOrdinal)
        ));
    }

    #[test]
    fn reset_consumes_encounter_and_preserves_monotonic_projectile_identity() {
        let allocator = encounter().into_cleared_projectile_allocator();
        assert_eq!(allocator.peek(), id(1_000));
    }

    #[test]
    fn physical_body_and_damage_hurtbox_keep_distinct_authored_radii() {
        let encounter = encounter();
        assert!(
            (encounter.body_collider().expect("body").radius_tiles()
                - crate::CALDUS_COLLISION_RADIUS_TILES)
                .abs()
                < f32::EPSILON
        );
        assert!(
            (encounter
                .hurtbox()
                .expect("hurtbox")
                .expect("living boss")
                .radius_tiles()
                - crate::CALDUS_HURTBOX_RADIUS_TILES)
                .abs()
                < f32::EPSILON
        );
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

    fn friendly_damage(tick: u64, projectile: u64, raw_damage: u32) -> CoreCaldusFriendlyInput {
        let projectile_id = id(projectile);
        CoreCaldusFriendlyInput {
            participant: participant(),
            combat: crate::CombatStep {
                tick: Tick(tick),
                collisions: vec![ProjectileCollision {
                    tick: Tick(tick),
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
                    tick: Tick(tick),
                    projectile_id,
                    source: RawDamageIntentSource::Primary,
                    target: id(99),
                    base_raw_damage: raw_damage,
                    multiplier_basis_points: 10_000,
                    resolved_raw_damage: raw_damage,
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
    fn inherited_authoritative_tick_starts_every_caldus_owner_without_rewind() {
        let start_tick = Tick(4_123);
        let mut encounter = CoreCaldusEncounterSimulation::new_at_tick(
            lock(),
            arena(),
            id(99),
            EntityIdAllocator::starting_at(NonZeroU64::new(1_000).expect("nonzero")),
            start_tick,
        )
        .expect("inherited-tick encounter");
        assert_eq!(encounter.tick(), start_tick);
        assert_eq!(encounter.body().tick(), start_tick);

        let step = encounter
            .step(&[friendly_damage(start_tick.0, 500, 20)], &mut players())
            .expect("first inherited-tick frame");
        assert_eq!(step.tick, start_tick);
        assert_eq!(step.hostile_step.tick, start_tick);
        assert!(
            step.friendly_damage
                .iter()
                .all(|damage| damage.tick == start_tick)
        );
        assert_eq!(encounter.tick(), Tick(start_tick.0 + 1));
        assert_eq!(encounter.body().tick(), Tick(start_tick.0 + 1));
    }

    #[test]
    fn threshold_to_charge_contact_and_realized_stop_ring_is_atomic() {
        let mut encounter = encounter();
        let mut players = players();
        let transition = encounter
            .step(&[friendly_damage(0, 500, 2_500)], &mut players)
            .expect("threshold");
        assert!(transition.scheduler_events.iter().any(|event| matches!(
            event,
            CoreCaldusEvent::BreakStarted {
                entering: crate::CoreCaldusPhase::Phase2,
                ..
            }
        )));
        let mut charge_damage = Vec::new();
        let mut player_separations = Vec::new();
        let mut stop_ring_projectiles = 0;
        for _ in 1..=167 {
            let step = encounter.step(&[], &mut players).expect("advance");
            charge_damage.extend(step.charge_damage);
            player_separations.extend(step.player_separations);
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
        assert!(!player_separations.is_empty());
        assert!(player_separations.iter().all(|event| {
            ((event.to - event.boss_position).length()
                - (CALDUS_COLLISION_RADIUS_TILES + PLAYER_COLLISION_RADIUS_TILES))
                .abs()
                < 1.0e-5
        }));
    }

    #[test]
    fn blocked_radial_separation_uses_reverse_charge_axis() {
        let mut blocked_arena = arena();
        blocked_arena.pillars = vec![crate::TileRectangle::new(10_200, 8_900, 100, 200)];
        let encounter = CoreCaldusEncounterSimulation::new(
            lock(),
            blocked_arena,
            id(99),
            EntityIdAllocator::starting_at(NonZeroU64::new(1_000).expect("nonzero")),
        )
        .expect("encounter");
        let mut players = players();
        players.get_mut(&id(10)).expect("player").target.position = SimulationVector::new(9.5, 9.0);
        let events = [CoreCaldusBodyEvent::ChargeMoved {
            tick: Tick(0),
            cast_id: 7,
            segment_index: 0,
            from: crate::CoreWorldPosition::new(8_000, 9_000),
            to: crate::CoreWorldPosition::new(9_000, 9_000),
            axis: CoreCaldusChargeAxis::East,
            blocked_by: None,
            contacts: vec![participant()],
        }];
        let separated = encounter
            .resolve_charge_overlaps(&events, &mut players)
            .expect("fallback separation");
        assert_eq!(separated.len(), 1);
        assert_eq!(separated[0].to, SimulationVector::new(8.0, 9.0));
        assert_eq!(players[&id(10)].target.position, separated[0].to);
    }

    #[test]
    fn unavailable_separation_fails_without_mutating_players() {
        let mut blocked_arena = arena();
        blocked_arena.pillars = vec![
            crate::TileRectangle::new(10_200, 8_900, 100, 200),
            crate::TileRectangle::new(8_900, 10_200, 200, 100),
            crate::TileRectangle::new(7_700, 8_900, 100, 200),
            crate::TileRectangle::new(8_900, 7_700, 200, 100),
        ];
        let encounter = CoreCaldusEncounterSimulation::new(
            lock(),
            blocked_arena,
            id(99),
            EntityIdAllocator::starting_at(NonZeroU64::new(1_000).expect("nonzero")),
        )
        .expect("encounter");
        let mut players = players();
        players.get_mut(&id(10)).expect("player").target.position = SimulationVector::new(9.0, 9.0);
        let before = players.clone();
        let events = [CoreCaldusBodyEvent::ChargeMoved {
            tick: Tick(0),
            cast_id: 7,
            segment_index: 0,
            from: crate::CoreWorldPosition::new(8_000, 9_000),
            to: crate::CoreWorldPosition::new(9_000, 9_000),
            axis: CoreCaldusChargeAxis::East,
            blocked_by: None,
            contacts: vec![participant()],
        }];
        assert!(matches!(
            encounter.resolve_charge_overlaps(&events, &mut players),
            Err(CoreCaldusEncounterError::NoLegalPlayerSeparation { .. })
        ));
        assert_eq!(players, before);
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

    fn complete_solo_run() -> (Tick, Vec<(Tick, crate::CoreCaldusPhase)>, blake3::Hash) {
        let mut encounter = encounter();
        encounter.set_damage_policy(HostileDamagePolicy::DebugInvulnerable);
        let mut players = players();
        let mut phase_starts = Vec::new();
        let mut trace = blake3::Hasher::new();
        for tick in 0_u64..=5_400 {
            let input = [1_800_u64, 3_600, 5_400]
                .contains(&tick)
                .then(|| friendly_damage(tick, 600 + tick / 1_800, 2_700));
            let step = encounter
                .step(input.as_slice(), &mut players)
                .unwrap_or_else(|error| panic!("complete run tick {tick}: {error}"));
            trace.update(format!("{step:?}\n").as_bytes());
            phase_starts.extend(
                step.scheduler_events
                    .iter()
                    .filter_map(|event| match event {
                        CoreCaldusEvent::PhaseStarted { tick, phase } => Some((*tick, *phase)),
                        _ => None,
                    }),
            );
            if let Some(defeat) = step.defeat {
                assert_eq!(encounter.current_health(), 0);
                assert!(encounter.hostile_projectiles().is_empty());
                return (defeat.tick, phase_starts, trace.finalize());
            }
        }
        panic!("script did not defeat Caldus")
    }

    #[test]
    fn complete_solo_combat_is_180_seconds_and_byte_identical_on_replay() {
        let first = complete_solo_run();
        let second = complete_solo_run();
        assert_eq!(first, second);
        assert_eq!(first.0, Tick(5_400));
        assert_eq!(first.0.0 / 30, 180);
        assert_eq!(
            first.1,
            [
                (Tick(1_920), crate::CoreCaldusPhase::Phase2),
                (Tick(3_720), crate::CoreCaldusPhase::Phase3),
            ]
        );
    }
}
