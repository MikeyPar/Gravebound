//! Transactional three-role Combat Laboratory coordinator.

use thiserror::Error;

use crate::{
    AppliedHostileDamage, ArenaGeometry, AttackCastId, BellReedDefinition, BellReedSimulation,
    ChainSentryDefinition, ChainSentrySimulation, CollisionError, DrownedPilgrimDefinition,
    DrownedPilgrimSimulation, EnemyActor, EnemyActorKind, EnemyActorMovement, EnemyEvent,
    EnemyHurtbox, EnemyRuntimeError, EntityId, EntityIdAllocator, HostileDamagePolicy,
    HostileError, HostileEvent, HostileProjectile, HostileProjectileSimulation, HostileStep,
    HostileTargetState, HurtboxError, LaneAttackDefinition, LaneGeometry,
    PLAYER_HURTBOX_RADIUS_TILES, PilgrimTargetInput, PlayerCombatState, ProjectileCollisionWorld,
    RedTonicSimulation, SimulationVector, Tick, resolve_lane_contact_with_policy,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnemyLabDefinitions {
    pub drowned_pilgrim: DrownedPilgrimDefinition,
    pub bell_reed: BellReedDefinition,
    pub chain_sentry: ChainSentryDefinition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyLabActorIds {
    pub drowned_pilgrim: EntityId,
    pub bell_reed: EntityId,
    pub chain_sentry: EntityId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyLabActorPositions {
    pub drowned_pilgrim_milli_tiles: (i32, i32),
    pub bell_reed_milli_tiles: (i32, i32),
    pub chain_sentry_milli_tiles: (i32, i32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnemyLabPlayer {
    pub target: HostileTargetState,
    pub consumables: RedTonicSimulation,
    pub combat: PlayerCombatState,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnemyActorGroup {
    drowned_pilgrim: EnemyActor,
    bell_reed: EnemyActor,
    chain_sentry: EnemyActor,
}

impl EnemyActorGroup {
    #[must_use]
    pub const fn drowned_pilgrim(&self) -> &EnemyActor {
        &self.drowned_pilgrim
    }

    #[must_use]
    pub const fn bell_reed(&self) -> &EnemyActor {
        &self.bell_reed
    }

    #[must_use]
    pub const fn chain_sentry(&self) -> &EnemyActor {
        &self.chain_sentry
    }

    #[must_use]
    pub fn actor(&self, kind: EnemyActorKind) -> &EnemyActor {
        match kind {
            EnemyActorKind::DrownedPilgrim => &self.drowned_pilgrim,
            EnemyActorKind::BellReed => &self.bell_reed,
            EnemyActorKind::ChainSentry => &self.chain_sentry,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActiveEnemyLane {
    pub source_entity_id: EntityId,
    pub cast_id: AttackCastId,
    pub geometry: LaneGeometry,
    pub attack: LaneAttackDefinition,
    pub active_until: Tick,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ShowcaseAttackState {
    #[default]
    Unseen,
    EventObserved,
    DamageObserved,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EnemyShowcaseReadiness {
    fan: ShowcaseAttackState,
    ring: ShowcaseAttackState,
    lane: ShowcaseAttackState,
}

impl EnemyShowcaseReadiness {
    #[must_use]
    pub const fn is_ready(self) -> bool {
        matches!(self.fan, ShowcaseAttackState::DamageObserved)
            && matches!(self.ring, ShowcaseAttackState::DamageObserved)
            && matches!(self.lane, ShowcaseAttackState::DamageObserved)
    }

    #[must_use]
    pub const fn fan_fired(self) -> bool {
        !matches!(self.fan, ShowcaseAttackState::Unseen)
    }

    #[must_use]
    pub const fn ring_fired(self) -> bool {
        !matches!(self.ring, ShowcaseAttackState::Unseen)
    }

    #[must_use]
    pub const fn lane_activated(self) -> bool {
        !matches!(self.lane, ShowcaseAttackState::Unseen)
    }

    #[must_use]
    pub const fn fan_damaged_player(self) -> bool {
        matches!(self.fan, ShowcaseAttackState::DamageObserved)
    }

    #[must_use]
    pub const fn ring_damaged_player(self) -> bool {
        matches!(self.ring, ShowcaseAttackState::DamageObserved)
    }

    #[must_use]
    pub const fn lane_damaged_player(self) -> bool {
        matches!(self.lane, ShowcaseAttackState::DamageObserved)
    }

    fn observe_fan_event(&mut self) {
        self.fan = self.fan.max(ShowcaseAttackState::EventObserved);
    }

    fn observe_ring_event(&mut self) {
        self.ring = self.ring.max(ShowcaseAttackState::EventObserved);
    }

    fn observe_lane_event(&mut self) {
        self.lane = self.lane.max(ShowcaseAttackState::EventObserved);
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnemyLabTargetSnapshot {
    pub tick: Tick,
    pub entity_id: EntityId,
    pub position: SimulationVector,
    pub health: u32,
    pub barrier: u32,
    pub alive: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnemyTimelineEvent {
    pub source_entity_id: EntityId,
    pub source_kind: EnemyActorKind,
    pub event: EnemyEvent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnemyLaneEvent {
    Activated {
        source_entity_id: EntityId,
        cast_id: AttackCastId,
        active_until: Tick,
    },
    Contact {
        source_entity_id: EntityId,
        pattern_id: &'static str,
        cast_id: AttackCastId,
        player_entity_id: EntityId,
        damage: Box<AppliedHostileDamage>,
    },
    Expired {
        source_entity_id: EntityId,
        cast_id: AttackCastId,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnemyLabStep {
    pub tick: Tick,
    pub target_snapshot: EnemyLabTargetSnapshot,
    pub enemy_events: Vec<EnemyTimelineEvent>,
    pub actor_movements: Vec<EnemyActorMovement>,
    pub hostile_spawn_events: Vec<HostileEvent>,
    pub lane_events: Vec<EnemyLaneEvent>,
    pub hostile_step: HostileStep,
    pub health_after: u32,
    pub readiness_after: EnemyShowcaseReadiness,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClearedEnemyHostiles {
    pub projectiles: Vec<HostileProjectile>,
    pub lane: Option<ActiveEnemyLane>,
}

/// Authoritative owner of the three-role local semantic combat showcase.
#[derive(Debug, Clone, PartialEq)]
pub struct EnemyLab {
    arena: ArenaGeometry,
    actors: EnemyActorGroup,
    drowned_pilgrim: DrownedPilgrimSimulation,
    bell_reed: BellReedSimulation,
    chain_sentry: ChainSentrySimulation,
    hostile_projectiles: HostileProjectileSimulation,
    active_lane: Option<ActiveEnemyLane>,
    player: EnemyLabPlayer,
    readiness: EnemyShowcaseReadiness,
    tick: Tick,
    damage_policy: HostileDamagePolicy,
}

impl EnemyLab {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        definitions: EnemyLabDefinitions,
        arena: ArenaGeometry,
        actor_ids: EnemyLabActorIds,
        actor_positions: EnemyLabActorPositions,
        player: EnemyLabPlayer,
        hostile_projectile_ids: EntityIdAllocator,
    ) -> Result<Self, EnemyLabError> {
        validate_distinct_ids(actor_ids, player.target.entity_id)?;
        if !player.target.position.is_finite() {
            return Err(EnemyLabError::NonFiniteTargetPosition);
        }
        let pilgrim = EnemyActor::new(
            actor_ids.drowned_pilgrim,
            EnemyActorKind::DrownedPilgrim,
            actor_positions.drowned_pilgrim_milli_tiles.0,
            actor_positions.drowned_pilgrim_milli_tiles.1,
            definitions
                .drowned_pilgrim
                .parameters()
                .hurtbox_radius_milli_tiles,
        )?;
        let reed = EnemyActor::new(
            actor_ids.bell_reed,
            EnemyActorKind::BellReed,
            actor_positions.bell_reed_milli_tiles.0,
            actor_positions.bell_reed_milli_tiles.1,
            definitions
                .bell_reed
                .parameters()
                .hurtbox_radius_milli_tiles,
        )?;
        let sentry = EnemyActor::new(
            actor_ids.chain_sentry,
            EnemyActorKind::ChainSentry,
            actor_positions.chain_sentry_milli_tiles.0,
            actor_positions.chain_sentry_milli_tiles.1,
            definitions
                .chain_sentry
                .parameters()
                .hurtbox_radius_milli_tiles,
        )?;
        validate_spawn_geometry(
            &arena,
            &[
                (
                    &pilgrim,
                    definitions
                        .drowned_pilgrim
                        .parameters()
                        .hurtbox_radius_milli_tiles,
                ),
                (
                    &reed,
                    definitions
                        .bell_reed
                        .parameters()
                        .hurtbox_radius_milli_tiles,
                ),
                (
                    &sentry,
                    definitions
                        .chain_sentry
                        .parameters()
                        .hurtbox_radius_milli_tiles,
                ),
            ],
            &player.target,
        )?;
        Ok(Self {
            arena,
            actors: EnemyActorGroup {
                drowned_pilgrim: pilgrim,
                bell_reed: reed,
                chain_sentry: sentry,
            },
            drowned_pilgrim: DrownedPilgrimSimulation::new(definitions.drowned_pilgrim),
            bell_reed: BellReedSimulation::new(definitions.bell_reed),
            chain_sentry: ChainSentrySimulation::new(definitions.chain_sentry),
            hostile_projectiles: HostileProjectileSimulation::with_allocator(
                hostile_projectile_ids,
            ),
            active_lane: None,
            player,
            readiness: EnemyShowcaseReadiness::default(),
            tick: Tick(0),
            damage_policy: HostileDamagePolicy::Standard,
        })
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    #[must_use]
    pub const fn arena(&self) -> &ArenaGeometry {
        &self.arena
    }

    #[must_use]
    pub const fn actors(&self) -> &EnemyActorGroup {
        &self.actors
    }

    #[must_use]
    pub const fn player(&self) -> &EnemyLabPlayer {
        &self.player
    }

    /// Mutable seam for ordered player combat/consumable systems using this sole authoritative state.
    pub const fn player_mut(&mut self) -> &mut EnemyLabPlayer {
        &mut self.player
    }

    pub fn set_damage_policy(&mut self, policy: HostileDamagePolicy) {
        self.damage_policy = policy;
        self.hostile_projectiles.set_damage_policy(policy);
    }

    #[must_use]
    pub fn hostile_projectiles(&self) -> &[HostileProjectile] {
        self.hostile_projectiles.projectiles()
    }

    #[must_use]
    pub const fn active_lane(&self) -> Option<&ActiveEnemyLane> {
        self.active_lane.as_ref()
    }

    #[must_use]
    pub const fn readiness(&self) -> EnemyShowcaseReadiness {
        self.readiness
    }

    pub fn update_target_position(
        &mut self,
        position: SimulationVector,
    ) -> Result<(), EnemyLabError> {
        if !position.is_finite() {
            return Err(EnemyLabError::NonFiniteTargetPosition);
        }
        validate_target_position(&self.arena, self.player.target.entity_id, position)?;
        self.player.target.position = position;
        Ok(())
    }

    /// Clears active projectile/lane threats without rewinding hostile time or projectile IDs.
    pub fn clear_hostiles(&mut self) -> ClearedEnemyHostiles {
        ClearedEnemyHostiles {
            projectiles: self.hostile_projectiles.clear_projectiles(),
            lane: self.active_lane.take(),
        }
    }

    /// Advances the entire enemy laboratory as one clone-then-commit transaction.
    pub fn step(&mut self) -> Result<EnemyLabStep, EnemyLabError> {
        let mut next = self.clone();
        let result = next.step_inner()?;
        *self = next;
        Ok(result)
    }

    fn step_inner(&mut self) -> Result<EnemyLabStep, EnemyLabError> {
        self.validate_tick_alignment()?;
        let snapshot = self.target_snapshot();
        let pilgrim_input = if snapshot.alive {
            self.actors
                .drowned_pilgrim
                .target_input(snapshot.position)?
        } else {
            PilgrimTargetInput::ABSENT
        };

        // Ordering contract: snapshot -> all timelines -> actor movement -> authorized threats.
        let pilgrim_events = self.drowned_pilgrim.advance(pilgrim_input)?;
        let reed_events = self.bell_reed.advance()?;
        let sentry_events = self.chain_sentry.advance()?;
        let mut enemy_events = Vec::new();
        let mut actor_movements = Vec::new();
        for event in &pilgrim_events {
            if let Some(movement) = self
                .actors
                .drowned_pilgrim
                .apply_event(&self.arena, event)?
            {
                actor_movements.push(movement);
            }
            enemy_events.push(EnemyTimelineEvent {
                source_entity_id: self.actors.drowned_pilgrim.entity_id(),
                source_kind: EnemyActorKind::DrownedPilgrim,
                event: event.clone(),
            });
        }
        append_timeline_events(
            &mut enemy_events,
            self.actors.bell_reed.entity_id(),
            EnemyActorKind::BellReed,
            &reed_events,
        );
        append_timeline_events(
            &mut enemy_events,
            self.actors.chain_sentry.entity_id(),
            EnemyActorKind::ChainSentry,
            &sentry_events,
        );

        let mut hostile_spawn_events = Vec::new();
        for event in &pilgrim_events {
            if matches!(event, EnemyEvent::FanFired { .. }) {
                let spawned = self.hostile_projectiles.spawn_from_enemy_event(
                    self.actors.drowned_pilgrim.entity_id(),
                    self.actors.drowned_pilgrim.position(),
                    event,
                )?;
                if !spawned.is_empty() {
                    self.readiness.observe_fan_event();
                }
                hostile_spawn_events.extend(spawned);
            }
        }
        for event in &reed_events {
            if matches!(event, EnemyEvent::RingFired { .. }) {
                let spawned = self.hostile_projectiles.spawn_from_enemy_event(
                    self.actors.bell_reed.entity_id(),
                    self.actors.bell_reed.position(),
                    event,
                )?;
                if !spawned.is_empty() {
                    self.readiness.observe_ring_event();
                }
                hostile_spawn_events.extend(spawned);
            }
        }

        let mut lane_events = self.process_lane_timeline(&sentry_events)?;
        self.resolve_active_lane(snapshot, &mut lane_events)?;

        // Hostile sweeps and their damage/Focused break happen after all same-tick spawns/lanes.
        let hostile_step = self.hostile_projectiles.step(
            &self.arena,
            &mut self.player.target,
            &mut self.player.consumables,
            &mut self.player.combat,
        )?;
        self.observe_projectile_damage(&hostile_step);
        let result = EnemyLabStep {
            tick: self.tick,
            target_snapshot: snapshot,
            enemy_events,
            actor_movements,
            hostile_spawn_events,
            lane_events,
            hostile_step,
            health_after: self.player.consumables.vitals().current_health(),
            readiness_after: self.readiness,
        };
        self.tick = self
            .tick
            .checked_next()
            .ok_or(EnemyLabError::TickOverflow)?;
        Ok(result)
    }

    fn process_lane_timeline(
        &mut self,
        sentry_events: &[EnemyEvent],
    ) -> Result<Vec<EnemyLaneEvent>, EnemyLabError> {
        let mut events = Vec::new();
        for event in sentry_events {
            match event {
                EnemyEvent::LanesActivated {
                    cast_id,
                    active_until,
                    ..
                } => {
                    let (resolved_cast, geometry, attack) = LaneGeometry::from_activation(event)?;
                    if resolved_cast != *cast_id {
                        return Err(EnemyLabError::LaneCastMismatch);
                    }
                    self.active_lane = Some(ActiveEnemyLane {
                        source_entity_id: self.actors.chain_sentry.entity_id(),
                        cast_id: *cast_id,
                        geometry: geometry.with_origin(self.actors.chain_sentry.position()),
                        attack,
                        active_until: *active_until,
                    });
                    self.readiness.observe_lane_event();
                    events.push(EnemyLaneEvent::Activated {
                        source_entity_id: self.actors.chain_sentry.entity_id(),
                        cast_id: *cast_id,
                        active_until: *active_until,
                    });
                }
                EnemyEvent::LanesExpired { cast_id } => {
                    if let Some(lane) = self.active_lane.take() {
                        if lane.cast_id != *cast_id {
                            return Err(EnemyLabError::LaneCastMismatch);
                        }
                        events.push(EnemyLaneEvent::Expired {
                            source_entity_id: lane.source_entity_id,
                            cast_id: *cast_id,
                        });
                    }
                }
                _ => {}
            }
        }
        Ok(events)
    }

    fn resolve_active_lane(
        &mut self,
        snapshot: EnemyLabTargetSnapshot,
        events: &mut Vec<EnemyLaneEvent>,
    ) -> Result<(), EnemyLabError> {
        let Some(lane) = self.active_lane.clone() else {
            return Ok(());
        };
        if !snapshot.alive || !lane.geometry.contacts_player(snapshot.position) {
            return Ok(());
        }
        if !self
            .chain_sentry
            .register_player_contact(lane.cast_id, snapshot.entity_id.get())?
        {
            return Ok(());
        }
        let damage = resolve_lane_contact_with_policy(
            lane.source_entity_id,
            &lane.attack,
            lane.geometry,
            &mut self.player.target,
            &mut self.player.consumables,
            &mut self.player.combat,
            self.damage_policy,
        )?
        .ok_or(EnemyLabError::LaneGeometryDisagreed)?;
        if damage.health_application.applied > 0 {
            self.readiness.lane = ShowcaseAttackState::DamageObserved;
        }
        events.push(EnemyLaneEvent::Contact {
            source_entity_id: lane.source_entity_id,
            pattern_id: lane.attack.pattern_id,
            cast_id: lane.cast_id,
            player_entity_id: snapshot.entity_id,
            damage: Box::new(damage),
        });
        Ok(())
    }

    fn observe_projectile_damage(&mut self, step: &HostileStep) {
        for event in &step.events {
            let HostileEvent::Contact {
                source_entity_id,
                health_application: Some(application),
                ..
            } = event
            else {
                continue;
            };
            if application.applied == 0 {
                continue;
            }
            if *source_entity_id == self.actors.drowned_pilgrim.entity_id() {
                self.readiness.fan = ShowcaseAttackState::DamageObserved;
            } else if *source_entity_id == self.actors.bell_reed.entity_id() {
                self.readiness.ring = ShowcaseAttackState::DamageObserved;
            }
        }
    }

    fn target_snapshot(&self) -> EnemyLabTargetSnapshot {
        let vitals = self.player.consumables.vitals();
        EnemyLabTargetSnapshot {
            tick: self.tick,
            entity_id: self.player.target.entity_id,
            position: self.player.target.position,
            health: vitals.current_health(),
            barrier: self.player.target.current_barrier,
            alive: vitals.current_health() > 0,
        }
    }

    fn validate_tick_alignment(&self) -> Result<(), EnemyLabError> {
        let ticks = [
            self.drowned_pilgrim.tick(),
            self.bell_reed.tick(),
            self.chain_sentry.tick(),
            self.hostile_projectiles.tick(),
        ];
        if ticks.iter().any(|tick| *tick != self.tick) {
            return Err(EnemyLabError::TickDesynchronized {
                coordinator: self.tick,
                actors: ticks,
            });
        }
        Ok(())
    }
}

fn append_timeline_events(
    destination: &mut Vec<EnemyTimelineEvent>,
    source_entity_id: EntityId,
    source_kind: EnemyActorKind,
    events: &[EnemyEvent],
) {
    destination.extend(events.iter().cloned().map(|event| EnemyTimelineEvent {
        source_entity_id,
        source_kind,
        event,
    }));
}

fn validate_distinct_ids(ids: EnemyLabActorIds, player: EntityId) -> Result<(), EnemyLabError> {
    let mut values = [ids.drowned_pilgrim, ids.bell_reed, ids.chain_sentry, player];
    values.sort_unstable();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(EnemyLabError::DuplicateEntityId);
    }
    Ok(())
}

fn validate_spawn_geometry(
    arena: &ArenaGeometry,
    actors: &[(&EnemyActor, u32)],
    target: &HostileTargetState,
) -> Result<(), EnemyLabError> {
    let hurtboxes = actors
        .iter()
        .map(|(actor, radius)| {
            EnemyHurtbox::new(actor.entity_id(), actor.position(), milli_to_tiles(*radius))
        })
        .collect::<Result<Vec<_>, _>>()?;
    ProjectileCollisionWorld::new(arena, hurtboxes)?;
    validate_target_position(arena, target.entity_id, target.position)
}

fn validate_target_position(
    arena: &ArenaGeometry,
    entity_id: EntityId,
    position: SimulationVector,
) -> Result<(), EnemyLabError> {
    let target = EnemyHurtbox::new(entity_id, position, PLAYER_HURTBOX_RADIUS_TILES)?;
    ProjectileCollisionWorld::new(arena, vec![target])?;
    Ok(())
}

#[allow(clippy::cast_precision_loss)]
fn milli_to_tiles(value: u32) -> f32 {
    value as f32 / 1_000.0
}

#[derive(Debug, Error)]
pub enum EnemyLabError {
    #[error("enemy laboratory actor/player entity IDs must be distinct")]
    DuplicateEntityId,
    #[error("hostile target position must be finite")]
    NonFiniteTargetPosition,
    #[error("enemy laboratory tick overflowed u64")]
    TickOverflow,
    #[error("enemy laboratory clocks desynchronized: coordinator {coordinator}, actors {actors:?}")]
    TickDesynchronized {
        coordinator: Tick,
        actors: [Tick; 4],
    },
    #[error("lane activation and geometry cast IDs disagreed")]
    LaneCastMismatch,
    #[error("lane contact predicate and resolver disagreed")]
    LaneGeometryDisagreed,
    #[error("enemy timeline failed: {0}")]
    Enemy(#[from] EnemyRuntimeError),
    #[error("hostile integration failed: {0}")]
    Hostile(#[from] HostileError),
    #[error("actor/target hurtbox failed: {0}")]
    Hurtbox(#[from] HurtboxError),
    #[error("actor/target geometry failed: {0}")]
    Collision(#[from] CollisionError),
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::*;
    use crate::{
        GraveMarkDefinition, GraveMarkDefinitionParameters, SlipstepDefinition,
        SlipstepDefinitionParameters, StillnessDefinition, StillnessDefinitionParameters,
        TilePoint, WeaponDefinition, WeaponDefinitionParameters,
    };

    fn id(value: u64) -> EntityId {
        EntityId::new(value).expect("nonzero ID")
    }

    fn arena() -> ArenaGeometry {
        ArenaGeometry {
            id: "arena.test.enemy_lab".to_owned(),
            width_milli_tiles: 32_000,
            height_milli_tiles: 24_000,
            shell_thickness_milli_tiles: 1_000,
            player_spawn: TilePoint::new(10_000, 12_000),
            boss_spawn: TilePoint::new(28_000, 12_000),
            pillars: Vec::new(),
            anchors: Vec::new(),
        }
        .validated()
        .expect("arena")
    }

    fn combat() -> PlayerCombatState {
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
        let slipstep = SlipstepDefinition::new(SlipstepDefinitionParameters {
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
        .expect("slipstep");
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
        PlayerCombatState::new(weapon, mark, slipstep, stillness).expect("combat")
    }

    fn player(position: SimulationVector, health: u32) -> EnemyLabPlayer {
        EnemyLabPlayer {
            target: HostileTargetState {
                entity_id: id(90),
                position,
                target_is_immune: false,
                resistance_basis_points: 0,
                additional_direct_damage_reductions_basis_points: Vec::new(),
                armor: 0,
                current_barrier: 0,
                health_damage_cap_basis_points: None,
            },
            consumables: RedTonicSimulation::first_playable(
                crate::PlayerVitals::new(health, health).expect("vitals"),
            )
            .expect("tonic state"),
            combat: combat(),
        }
    }

    fn lab_with_allocator(
        target_position: SimulationVector,
        health: u32,
        allocator: EntityIdAllocator,
    ) -> EnemyLab {
        EnemyLab::new(
            EnemyLabDefinitions {
                drowned_pilgrim: DrownedPilgrimDefinition::first_playable(),
                bell_reed: BellReedDefinition::first_playable(),
                chain_sentry: ChainSentryDefinition::first_playable(),
            },
            arena(),
            EnemyLabActorIds {
                drowned_pilgrim: id(10),
                bell_reed: id(20),
                chain_sentry: id(30),
            },
            EnemyLabActorPositions {
                drowned_pilgrim_milli_tiles: (8_000, 12_000),
                bell_reed_milli_tiles: (16_000, 12_000),
                chain_sentry_milli_tiles: (24_000, 12_000),
            },
            player(target_position, health),
            allocator,
        )
        .expect("enemy lab")
    }

    fn lab() -> EnemyLab {
        lab_with_allocator(
            SimulationVector::new(10.0, 12.0),
            1_000,
            EntityIdAllocator::starting_at(NonZeroU64::new(1_000).expect("allocator")),
        )
    }

    #[test]
    fn constructor_uses_supplied_definitions_ids_positions_and_rejects_duplicates() {
        let simulation = lab();
        assert_eq!(simulation.actors().drowned_pilgrim().entity_id(), id(10));
        assert_eq!(
            simulation.actors().bell_reed().position_milli_tiles(),
            (16_000, 12_000)
        );
        assert_eq!(simulation.player().target.entity_id, id(90));

        let result = EnemyLab::new(
            EnemyLabDefinitions {
                drowned_pilgrim: DrownedPilgrimDefinition::first_playable(),
                bell_reed: BellReedDefinition::first_playable(),
                chain_sentry: ChainSentryDefinition::first_playable(),
            },
            arena(),
            EnemyLabActorIds {
                drowned_pilgrim: id(10),
                bell_reed: id(10),
                chain_sentry: id(30),
            },
            EnemyLabActorPositions {
                drowned_pilgrim_milli_tiles: (8_000, 12_000),
                bell_reed_milli_tiles: (16_000, 12_000),
                chain_sentry_milli_tiles: (24_000, 12_000),
            },
            player(SimulationVector::new(10.0, 12.0), 100),
            EntityIdAllocator::default(),
        );
        assert!(matches!(result, Err(EnemyLabError::DuplicateEntityId)));
    }

    #[test]
    fn target_update_is_validated_and_nonfinite_failure_is_nonmutating() {
        let mut simulation = lab();
        simulation
            .update_target_position(SimulationVector::new(11.0, 12.0))
            .expect("valid update");
        let before = simulation.clone();
        assert!(matches!(
            simulation.update_target_position(SimulationVector::new(f32::NAN, 12.0)),
            Err(EnemyLabError::NonFiniteTargetPosition)
        ));
        assert_eq!(simulation, before);
    }

    #[test]
    fn target_snapshot_precedes_timelines_movement_spawns_lanes_and_sweeps() {
        let mut simulation = lab();
        let mut fan_step = None;
        for _ in 0..37 {
            let step = simulation.step().expect("step");
            if step
                .enemy_events
                .iter()
                .any(|event| matches!(event.event, EnemyEvent::FanFired { .. }))
            {
                fan_step = Some(step);
            }
        }
        let step = fan_step.expect("fan step");
        assert_eq!(step.tick, Tick(36));
        assert_eq!(step.target_snapshot.tick, Tick(36));
        assert_eq!(step.hostile_spawn_events.len(), 3);
        assert!(
            step.hostile_step
                .events
                .iter()
                .any(|event| matches!(event, HostileEvent::Moved { .. }))
        );
    }

    #[test]
    fn real_fan_ring_lane_events_and_damage_drive_showcase_readiness() {
        let mut simulation = lab();
        let mut became_ready_at = None;
        for _ in 0..180 {
            let step = simulation.step().expect("step");
            if step.readiness_after.is_ready() {
                became_ready_at = Some(step.tick);
                break;
            }
        }
        assert!(
            became_ready_at.is_some(),
            "all three attacks must deal damage"
        );
        assert!(simulation.readiness().is_ready());
        assert!(simulation.player().consumables.vitals().current_health() < 1_000);
    }

    #[test]
    fn lane_contact_occurs_once_for_the_active_cast() {
        let mut simulation = lab();
        let mut contacts = Vec::new();
        for _ in 0..90 {
            let step = simulation.step().expect("step");
            contacts.extend(
                step.lane_events
                    .into_iter()
                    .filter_map(|event| match event {
                        EnemyLaneEvent::Contact { cast_id, .. } => Some(cast_id),
                        _ => None,
                    }),
            );
        }
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0], AttackCastId::FIRST);
    }

    #[test]
    fn dead_target_receives_no_more_damage_while_timelines_continue() {
        let mut simulation = lab();
        let maximum = simulation.player.consumables.vitals().maximum_health();
        simulation.player.consumables.apply_damage(maximum);
        for _ in 0..120 {
            let step = simulation.step().expect("dead-target step");
            assert_eq!(step.health_after, 0);
            assert!(
                step.lane_events
                    .iter()
                    .all(|event| { !matches!(event, EnemyLaneEvent::Contact { .. }) })
            );
            assert!(step.hostile_step.events.iter().all(|event| match event {
                HostileEvent::Contact {
                    health_application, ..
                } => health_application.is_none(),
                _ => true,
            }));
        }
        assert!(!simulation.readiness().fan_fired());
        assert!(simulation.readiness().ring_fired());
        assert!(!simulation.readiness().fan_damaged_player());
    }

    #[test]
    fn projectile_id_exhaustion_rolls_back_every_coordinated_state() {
        let mut simulation = lab_with_allocator(
            SimulationVector::new(10.0, 12.0),
            1_000,
            EntityIdAllocator::starting_at(NonZeroU64::new(u64::MAX).expect("allocator")),
        );
        for _ in 0..36 {
            simulation.step().expect("pre-fan step");
        }
        let before = simulation.clone();
        assert!(matches!(
            simulation.step(),
            Err(EnemyLabError::Hostile(HostileError::ProjectileIdOverflow))
        ));
        assert_eq!(simulation, before);
    }

    #[test]
    fn clear_hostiles_preserves_time_and_identity_seam() {
        let mut simulation = lab();
        for _ in 0..37 {
            simulation.step().expect("step");
        }
        let tick = simulation.tick();
        let cleared = simulation.clear_hostiles();
        assert_eq!(cleared.projectiles.len(), 3);
        assert!(simulation.hostile_projectiles().is_empty());
        assert_eq!(simulation.tick(), tick);
        simulation.step().expect("continues after clear");
    }

    #[test]
    fn clearing_an_active_lane_allows_its_timeline_expiry_to_pass_cleanly() {
        let mut simulation = lab();
        for _ in 0..73 {
            simulation.step().expect("through lane activation");
        }
        assert!(simulation.active_lane().is_some());
        let cleared = simulation.clear_hostiles();
        assert!(cleared.lane.is_some());
        for _ in 0..20 {
            simulation.step().expect("post-clear lane timeline");
        }
        assert!(simulation.active_lane().is_none());
    }

    #[test]
    fn fixed_replay_is_deterministic() {
        fn replay() -> (Vec<EnemyLabStep>, EnemyLab) {
            let mut simulation = lab();
            let mut trace = Vec::new();
            for _ in 0..180 {
                trace.push(simulation.step().expect("step"));
            }
            (trace, simulation)
        }
        assert_eq!(replay(), replay());
    }

    #[test]
    fn mutable_player_seam_persists_authoritative_combat_and_consumable_changes() {
        let mut simulation = lab();
        let health_before = simulation.player().consumables.vitals().current_health();
        simulation.player_mut().target.resistance_basis_points = 1_500;
        simulation.player_mut().consumables.apply_damage(17);
        assert_eq!(simulation.player().target.resistance_basis_points, 1_500);
        assert_eq!(
            simulation.player().consumables.vitals().current_health(),
            health_before - 17
        );
        simulation
            .step()
            .expect("coordinator consumes mutated player state");
        assert_eq!(simulation.player().target.resistance_basis_points, 1_500);
    }
}
