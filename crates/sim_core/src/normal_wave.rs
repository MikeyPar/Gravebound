//! Generalized authoritative owner for First Playable normal-enemy waves.
//!
//! `SIM-010`/`SIM-011` require fixed-tick server ownership and stable ordering. `CONT-FP-002`
//! through `CONT-FP-004` define authored positions, a 900 ms harmless spawn telegraph, the three
//! exact enemy timelines, wave-end hostile cleanup, and normal drops eight ticks after death.

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU64;

use thiserror::Error;

use crate::{
    ActiveEnemyLane, AppliedHostileDamage, ArenaGeometry, AttackCastId, BellReedDefinition,
    BellReedSimulation, ChainSentryDefinition, ChainSentrySimulation, CollisionError, CombatStep,
    DrownedPilgrimDefinition, DrownedPilgrimSimulation, EnemyActor, EnemyActorKind,
    EnemyActorMovement, EnemyEvent, EnemyHealthError, EnemyHealthSimulation, EnemyHealthSnapshot,
    EnemyHealthStep, EnemyHurtbox, EnemyLabPlayer, EnemyRuntimeError, EntityId, EntityIdAllocator,
    HostileDamagePolicy, HostileError, HostileEvent, HostileProjectile,
    HostileProjectileSimulation, HostileStep, LaneGeometry, NormalRewardDropEvent,
    PilgrimTargetInput, SpawnInstanceId, Tick, resolve_lane_contact_with_policy,
};

pub const FIRST_PLAYABLE_SPAWN_TELEGRAPH_TICKS: u32 = 27;
pub const RUN_ENTITY_ID_STRIDE: u64 = 100_000;
pub const NORMAL_WAVE_ENEMY_ID_OFFSET: u64 = 30_000;
pub const NORMAL_WAVE_MAX_SPAWN_ORDINAL: u16 = 9_999;
pub const HOSTILE_PROJECTILE_ID_OFFSET: u64 = 20_000;

/// Derives a stable simulation-owned enemy ID in the run-local `30001..=39999` namespace.
pub fn normal_wave_entity_id(
    instance: SpawnInstanceId,
) -> Result<EntityId, NormalWaveEntityIdError> {
    if instance.run_ordinal == 0 {
        return Err(NormalWaveEntityIdError::ZeroRunOrdinal);
    }
    if instance.spawn_ordinal == 0 || instance.spawn_ordinal > NORMAL_WAVE_MAX_SPAWN_ORDINAL {
        return Err(NormalWaveEntityIdError::SpawnOrdinalOutOfRange(
            instance.spawn_ordinal,
        ));
    }
    let base = run_entity_id_base(instance.run_ordinal)?;
    let value = base
        .checked_add(NORMAL_WAVE_ENEMY_ID_OFFSET)
        .and_then(|value| value.checked_add(u64::from(instance.spawn_ordinal)))
        .ok_or(NormalWaveEntityIdError::Overflow)?;
    EntityId::new(value).ok_or(NormalWaveEntityIdError::Overflow)
}

/// Starts the run-local hostile projectile allocator at `20000`, disjoint from players and waves.
pub fn normal_wave_projectile_allocator(
    run_ordinal: u32,
) -> Result<EntityIdAllocator, NormalWaveEntityIdError> {
    let base = run_entity_id_base(run_ordinal)?;
    let first = base
        .checked_add(HOSTILE_PROJECTILE_ID_OFFSET)
        .and_then(NonZeroU64::new)
        .ok_or(NormalWaveEntityIdError::Overflow)?;
    Ok(EntityIdAllocator::starting_at(first))
}

fn run_entity_id_base(run_ordinal: u32) -> Result<u64, NormalWaveEntityIdError> {
    let zero_based = run_ordinal
        .checked_sub(1)
        .ok_or(NormalWaveEntityIdError::ZeroRunOrdinal)?;
    u64::from(zero_based)
        .checked_mul(RUN_ENTITY_ID_STRIDE)
        .ok_or(NormalWaveEntityIdError::Overflow)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NormalWaveEntityIdError {
    #[error("run ordinal must be nonzero")]
    ZeroRunOrdinal,
    #[error("spawn ordinal {0} is outside the reserved 1..=9999 wave-enemy namespace")]
    SpawnOrdinalOutOfRange(u16),
    #[error("run-qualified entity ID overflow")]
    Overflow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalWaveDefinitions {
    pub drowned_pilgrim: DrownedPilgrimDefinition,
    pub bell_reed: BellReedDefinition,
    pub chain_sentry: ChainSentryDefinition,
}

impl NormalWaveDefinitions {
    #[must_use]
    pub fn first_playable() -> Self {
        Self {
            drowned_pilgrim: DrownedPilgrimDefinition::first_playable(),
            bell_reed: BellReedDefinition::first_playable(),
            chain_sentry: ChainSentryDefinition::first_playable(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NormalWaveEnemyKind {
    DrownedPilgrim,
    BellReed,
    ChainSentry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NormalWaveSpawn {
    pub instance_id: SpawnInstanceId,
    pub kind: NormalWaveEnemyKind,
    pub position_milli_tiles: (i32, i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalWavePhase {
    DormantTelegraph { activates_at: Tick },
    Active,
    Cleared { cleared_at: Tick },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalWaveTimelineEvent {
    pub instance_id: SpawnInstanceId,
    pub entity_id: EntityId,
    pub kind: NormalWaveEnemyKind,
    pub event: EnemyEvent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalWaveLaneEvent {
    Activated {
        instance_id: SpawnInstanceId,
        source_entity_id: EntityId,
        cast_id: AttackCastId,
        active_until: Tick,
    },
    Contact {
        instance_id: SpawnInstanceId,
        source_entity_id: EntityId,
        pattern_id: &'static str,
        cast_id: AttackCastId,
        player_entity_id: EntityId,
        damage: Box<AppliedHostileDamage>,
    },
    Expired {
        instance_id: SpawnInstanceId,
        source_entity_id: EntityId,
        cast_id: AttackCastId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormalWaveDefeat {
    pub instance_id: SpawnInstanceId,
    pub entity_id: EntityId,
    pub death_tick: Tick,
    pub reward_due_tick: Tick,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalWaveDrop {
    pub instance_id: SpawnInstanceId,
    pub event: NormalRewardDropEvent,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalWaveClearedHostiles {
    pub projectiles: Vec<HostileProjectile>,
    pub lanes: Vec<ActiveEnemyLane>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalWaveHandoff {
    pub player: EnemyLabPlayer,
    pub hostile_projectile_ids: EntityIdAllocator,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalWaveResetHandoff {
    pub participant: NormalWaveHandoff,
    pub cleared_hostiles: NormalWaveClearedHostiles,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalWaveStep {
    pub tick: Tick,
    pub phase_after: NormalWavePhase,
    pub activated: bool,
    pub timeline_events: Vec<NormalWaveTimelineEvent>,
    pub actor_movements: Vec<EnemyActorMovement>,
    pub hostile_spawn_events: Vec<HostileEvent>,
    pub lane_events: Vec<NormalWaveLaneEvent>,
    pub hostile_step: HostileStep,
    pub enemy_health_step: EnemyHealthStep,
    pub defeats: Vec<NormalWaveDefeat>,
    pub drops: Vec<NormalWaveDrop>,
    pub cleared_hostiles: Option<NormalWaveClearedHostiles>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalWaveInstanceSnapshot {
    pub instance_id: SpawnInstanceId,
    pub entity_id: EntityId,
    pub kind: NormalWaveEnemyKind,
    pub position_milli_tiles: (i32, i32),
    pub health: EnemyHealthSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NormalEnemyTimeline {
    DrownedPilgrim(DrownedPilgrimSimulation),
    BellReed(BellReedSimulation),
    ChainSentry(ChainSentrySimulation),
}

impl NormalEnemyTimeline {
    const fn tick(&self) -> Tick {
        match self {
            Self::DrownedPilgrim(simulation) => simulation.tick(),
            Self::BellReed(simulation) => simulation.tick(),
            Self::ChainSentry(simulation) => simulation.tick(),
        }
    }

    fn advance(
        &mut self,
        pilgrim_input: PilgrimTargetInput,
    ) -> Result<Vec<EnemyEvent>, EnemyRuntimeError> {
        match self {
            Self::DrownedPilgrim(simulation) => simulation.advance(pilgrim_input),
            Self::BellReed(simulation) => simulation.advance(),
            Self::ChainSentry(simulation) => simulation.advance(),
        }
    }

    fn register_lane_contact(
        &mut self,
        cast_id: AttackCastId,
        player_id: u64,
    ) -> Result<bool, EnemyRuntimeError> {
        let Self::ChainSentry(simulation) = self else {
            return Err(EnemyRuntimeError::LaneCastNotActive);
        };
        simulation.register_player_contact(cast_id, player_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalWaveActor {
    spawn: NormalWaveSpawn,
    entity_id: EntityId,
    actor: EnemyActor,
    timeline: NormalEnemyTimeline,
}

/// Owns an arbitrary, duplicate-safe collection of the three First Playable normal enemies.
#[derive(Debug, Clone, PartialEq)]
pub struct NormalWaveSimulation {
    arena: ArenaGeometry,
    actors: Vec<NormalWaveActor>,
    instance_by_entity: BTreeMap<EntityId, SpawnInstanceId>,
    health: EnemyHealthSimulation,
    hostile_projectiles: HostileProjectileSimulation,
    active_lanes: Vec<(SpawnInstanceId, ActiveEnemyLane)>,
    players: BTreeMap<EntityId, EnemyLabPlayer>,
    phase: NormalWavePhase,
    starts_at: Tick,
    activation_tick: Tick,
    tick: Tick,
    damage_policy: HostileDamagePolicy,
}

impl NormalWaveSimulation {
    #[allow(clippy::needless_pass_by_value)] // Public construction takes ownership of validated wave definitions at the boundary.
    pub fn new(
        definitions: NormalWaveDefinitions,
        arena: ArenaGeometry,
        mut spawns: Vec<NormalWaveSpawn>,
        player: EnemyLabPlayer,
        hostile_projectile_ids: EntityIdAllocator,
        starts_at: Tick,
    ) -> Result<Self, NormalWaveError> {
        if spawns.is_empty() {
            return Err(NormalWaveError::EmptyWave);
        }
        validate_telegraph_ticks(&definitions)?;
        spawns.sort_by_key(|spawn| spawn.instance_id);
        validate_spawn_identities(&spawns, player.target.entity_id)?;

        let mut actors = Vec::with_capacity(spawns.len());
        let mut health_actors = Vec::with_capacity(spawns.len());
        let mut instance_by_entity = BTreeMap::new();
        for spawn in spawns {
            let entity_id = normal_wave_entity_id(spawn.instance_id)?;
            let (actor, timeline, health_actor) = build_actor(&definitions, spawn, entity_id)?;
            instance_by_entity.insert(entity_id, spawn.instance_id);
            health_actors.push(health_actor);
            actors.push(NormalWaveActor {
                spawn,
                entity_id,
                actor,
                timeline,
            });
        }
        validate_authored_geometry(&arena, &actors)?;
        let activation_tick = add_global_ticks(starts_at, FIRST_PLAYABLE_SPAWN_TELEGRAPH_TICKS)?;
        Ok(Self {
            arena,
            actors,
            instance_by_entity,
            health: EnemyHealthSimulation::new(health_actors)?,
            hostile_projectiles: HostileProjectileSimulation::with_allocator(
                hostile_projectile_ids,
            ),
            active_lanes: Vec::new(),
            players: BTreeMap::from([(player.target.entity_id, player)]),
            phase: NormalWavePhase::DormantTelegraph {
                activates_at: activation_tick,
            },
            starts_at,
            activation_tick,
            tick: starts_at,
            damage_policy: HostileDamagePolicy::Standard,
        })
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    #[must_use]
    pub const fn phase(&self) -> NormalWavePhase {
        self.phase
    }

    #[must_use]
    pub const fn starts_at(&self) -> Tick {
        self.starts_at
    }

    #[must_use]
    pub fn player(&self) -> &EnemyLabPlayer {
        match self.players.first_key_value() {
            Some((_, player)) => player,
            None => unreachable!(),
        }
    }

    pub fn player_mut(&mut self) -> &mut EnemyLabPlayer {
        self.players
            .first_entry()
            .expect("normal wave always owns at least one player")
            .into_mut()
    }

    #[must_use]
    pub const fn players(&self) -> &BTreeMap<EntityId, EnemyLabPlayer> {
        &self.players
    }

    pub fn players_mut(&mut self) -> &mut BTreeMap<EntityId, EnemyLabPlayer> {
        &mut self.players
    }

    /// Adds a player before the wave advances. Shared authority locks its roster at first step.
    pub fn add_player(&mut self, player: EnemyLabPlayer) -> Result<(), NormalWaveError> {
        if self.tick != self.starts_at {
            return Err(NormalWaveError::RosterLocked);
        }
        let player_id = player.target.entity_id;
        if self.players.contains_key(&player_id) {
            return Err(NormalWaveError::DuplicatePlayer(player_id));
        }
        if !player.target.position.is_finite() {
            return Err(NormalWaveError::InvalidPlayerPosition(player_id));
        }
        self.players.insert(player_id, player);
        Ok(())
    }

    pub fn set_damage_policy(&mut self, policy: HostileDamagePolicy) {
        self.damage_policy = policy;
        self.hostile_projectiles.set_damage_policy(policy);
    }

    /// Transfers persistent player and monotonic hostile identity state to the next authored wave.
    /// Handoff is legal only after all hostile authority has been cleared.
    pub fn into_handoff(self) -> Result<NormalWaveHandoff, NormalWaveError> {
        if !matches!(self.phase, NormalWavePhase::Cleared { .. })
            || !self.hostile_projectiles.projectiles().is_empty()
            || !self.active_lanes.is_empty()
        {
            return Err(NormalWaveError::HandoffBeforeClear);
        }
        if self.players.len() != 1 {
            return Err(NormalWaveError::SharedHandoffUnsupported);
        }
        Ok(NormalWaveHandoff {
            player: self
                .players
                .into_values()
                .next()
                .ok_or(NormalWaveError::MissingPlayer)?,
            hostile_projectile_ids: self.hostile_projectiles.into_allocator(),
        })
    }

    /// Cancels an uncleared wave for an owning encounter reset while preserving participant and
    /// monotonic projectile authority. No reward or defeat event is synthesized.
    pub fn into_reset_handoff(mut self) -> Result<NormalWaveResetHandoff, NormalWaveError> {
        if self.players.len() != 1 {
            return Err(NormalWaveError::SharedHandoffUnsupported);
        }
        let cleared_hostiles = self.clear_hostiles();
        Ok(NormalWaveResetHandoff {
            participant: NormalWaveHandoff {
                player: self
                    .players
                    .into_values()
                    .next()
                    .ok_or(NormalWaveError::MissingPlayer)?,
                hostile_projectile_ids: self.hostile_projectiles.into_allocator(),
            },
            cleared_hostiles,
        })
    }

    #[must_use]
    pub fn hostile_projectiles(&self) -> &[HostileProjectile] {
        self.hostile_projectiles.projectiles()
    }

    /// Clears every hostile projectile/hazard when authoritative player death freezes the run.
    pub fn clear_hostiles_for_player_death(&mut self) -> NormalWaveClearedHostiles {
        self.clear_hostiles()
    }

    #[must_use]
    pub fn active_lanes(&self) -> Vec<(SpawnInstanceId, &ActiveEnemyLane)> {
        self.active_lanes
            .iter()
            .map(|(instance, lane)| (*instance, lane))
            .collect()
    }

    #[must_use]
    pub fn entity_id_for(&self, instance_id: SpawnInstanceId) -> Option<EntityId> {
        self.actors
            .binary_search_by_key(&instance_id, |actor| actor.spawn.instance_id)
            .ok()
            .map(|index| self.actors[index].entity_id)
    }

    #[must_use]
    pub fn instance_id_for(&self, entity_id: EntityId) -> Option<SpawnInstanceId> {
        self.instance_by_entity.get(&entity_id).copied()
    }

    pub fn alive_hurtboxes(&self) -> Result<Vec<EnemyHurtbox>, NormalWaveError> {
        if self.tick < self.activation_tick || matches!(self.phase, NormalWavePhase::Cleared { .. })
        {
            return Ok(Vec::new());
        }
        self.health.alive_hurtboxes().map_err(Into::into)
    }

    #[must_use]
    pub fn snapshots(&self) -> Vec<NormalWaveInstanceSnapshot> {
        let health = self
            .health
            .snapshots()
            .into_iter()
            .map(|snapshot| (snapshot.actor_id, snapshot))
            .collect::<BTreeMap<_, _>>();
        self.actors
            .iter()
            .map(|actor| NormalWaveInstanceSnapshot {
                instance_id: actor.spawn.instance_id,
                entity_id: actor.entity_id,
                kind: actor.spawn.kind,
                position_milli_tiles: actor.actor.position_milli_tiles(),
                health: health[&actor.entity_id],
            })
            .collect()
    }

    /// Fixed order: activation -> friendly damage/death -> living AI/movement -> hostile lanes and
    /// projectiles -> due drops -> wave-end hostile cleanup. The whole tick commits atomically.
    pub fn step(&mut self, combat_step: &CombatStep) -> Result<NormalWaveStep, NormalWaveError> {
        self.step_with_external_hostiles(combat_step, 0)
    }

    /// Advances an immutable cohort while a mixed encounter owns additional required hostiles.
    /// External health cannot mutate this wave; the count only delays terminal cleanup/clear.
    pub fn step_with_external_hostiles(
        &mut self,
        combat_step: &CombatStep,
        external_hostiles_remaining: u16,
    ) -> Result<NormalWaveStep, NormalWaveError> {
        let player_id = self.player().target.entity_id;
        self.step_players_with_external_hostiles(
            &BTreeMap::from([(player_id, combat_step.clone())]),
            external_hostiles_remaining,
        )
    }

    /// Advances a shared wave from the frozen per-player combat results. Network arrival order is
    /// erased by player-map order and global projectile/contact provenance sorting.
    pub fn step_players(
        &mut self,
        combat_steps: &BTreeMap<EntityId, CombatStep>,
    ) -> Result<NormalWaveStep, NormalWaveError> {
        self.step_players_with_external_hostiles(combat_steps, 0)
    }

    pub fn step_players_with_external_hostiles(
        &mut self,
        combat_steps: &BTreeMap<EntityId, CombatStep>,
        external_hostiles_remaining: u16,
    ) -> Result<NormalWaveStep, NormalWaveError> {
        let mut next = self.clone();
        let result = next.step_players_inner(combat_steps, external_hostiles_remaining)?;
        *self = next;
        Ok(result)
    }

    /// Inserts one Core-authored immutable release into this wave's shared hostile allocator.
    /// Returned events are shifted into the wave's global tick domain.
    pub fn spawn_from_core_normal_event(
        &mut self,
        source_entity_id: EntityId,
        definition: &crate::CoreEnemyDefinition,
        event: &crate::CoreNormalAttackEvent,
    ) -> Result<Vec<HostileEvent>, NormalWaveError> {
        let mut next = self.clone();
        let mut events = next.hostile_projectiles.spawn_from_core_normal_event(
            source_entity_id,
            definition,
            event,
        )?;
        for event in &mut events {
            shift_hostile_event(event, next.starts_at)?;
        }
        *self = next;
        Ok(events)
    }

    fn step_players_inner(
        &mut self,
        combat_steps: &BTreeMap<EntityId, CombatStep>,
        external_hostiles_remaining: u16,
    ) -> Result<NormalWaveStep, NormalWaveError> {
        if combat_steps.keys().any(|id| !self.players.contains_key(id)) {
            return Err(NormalWaveError::UnknownCombatPlayer);
        }
        let mut combat_step = CombatStep {
            tick: self.tick,
            ..CombatStep::default()
        };
        for step in combat_steps.values() {
            if step.tick != self.tick {
                return Err(NormalWaveError::CombatTickMismatch {
                    expected: self.tick,
                    received: step.tick,
                });
            }
            combat_step
                .collisions
                .extend(step.collisions.iter().copied());
            combat_step
                .raw_damage_intents
                .extend(step.raw_damage_intents.iter().copied());
        }
        combat_step
            .collisions
            .sort_by_key(|collision| (collision.projectile_id, collision.contact_ordinal));
        combat_step
            .raw_damage_intents
            .sort_by_key(|intent| (intent.projectile_id, intent.contact_ordinal));
        self.validate_alignment(&combat_step)?;
        let activated = self.activate_if_due();
        let enemy_health_step = self.health.apply_combat_step(&combat_step)?;
        let defeats = self.map_defeats(&enemy_health_step)?;
        let alive = self.alive_entity_ids();
        let just_cleared = alive.is_empty()
            && external_hostiles_remaining == 0
            && !matches!(self.phase, NormalWavePhase::Cleared { .. });

        let mut timeline_events = Vec::new();
        let mut actor_movements = Vec::new();
        let mut hostile_spawn_events = Vec::new();
        let mut lane_events = Vec::new();
        if !just_cleared && !matches!(self.phase, NormalWavePhase::Cleared { .. }) {
            self.advance_living_timelines(
                &alive,
                &mut timeline_events,
                &mut actor_movements,
                &mut hostile_spawn_events,
                &mut lane_events,
            )?;
            self.resolve_active_lanes(&mut lane_events)?;
        }

        let cleared_hostiles = just_cleared.then(|| self.clear_hostiles());
        if just_cleared {
            self.phase = NormalWavePhase::Cleared {
                cleared_at: self.tick,
            };
        }
        let mut hostile_step = self
            .hostile_projectiles
            .step_players(&self.arena, &mut self.players)?;
        shift_hostile_step(&mut hostile_step, self.starts_at)?;
        let drops = self
            .health
            .collect_due_drops(self.tick)?
            .into_iter()
            .map(|event| {
                let instance_id = self.instance_for_entity(event.actor_id)?;
                Ok(NormalWaveDrop { instance_id, event })
            })
            .collect::<Result<Vec<_>, NormalWaveError>>()?;
        let result = NormalWaveStep {
            tick: self.tick,
            phase_after: self.phase,
            activated,
            timeline_events,
            actor_movements,
            hostile_spawn_events,
            lane_events,
            hostile_step,
            enemy_health_step,
            defeats,
            drops,
            cleared_hostiles,
        };
        self.tick = self
            .tick
            .checked_next()
            .ok_or(NormalWaveError::TickOverflow)?;
        Ok(result)
    }

    fn validate_alignment(&self, combat_step: &CombatStep) -> Result<(), NormalWaveError> {
        if combat_step.tick != self.tick {
            return Err(NormalWaveError::CombatTickMismatch {
                expected: self.tick,
                received: combat_step.tick,
            });
        }
        if self.tick < self.activation_tick && !combat_step.raw_damage_intents.is_empty() {
            return Err(NormalWaveError::DamageDuringSpawnTelegraph);
        }
        if self.hostile_projectiles.tick() != self.local_tick()? {
            return Err(NormalWaveError::HostileTickMismatch);
        }
        Ok(())
    }

    fn activate_if_due(&mut self) -> bool {
        if matches!(self.phase, NormalWavePhase::DormantTelegraph { .. })
            && self.tick >= self.activation_tick
        {
            self.phase = NormalWavePhase::Active;
            true
        } else {
            false
        }
    }

    fn local_tick(&self) -> Result<Tick, NormalWaveError> {
        self.tick
            .0
            .checked_sub(self.starts_at.0)
            .map(Tick)
            .ok_or(NormalWaveError::TickBeforeWaveStart)
    }

    fn alive_entity_ids(&self) -> BTreeSet<EntityId> {
        self.health
            .snapshots()
            .into_iter()
            .filter(|snapshot| snapshot.alive)
            .map(|snapshot| snapshot.actor_id)
            .collect()
    }

    fn map_defeats(
        &self,
        step: &EnemyHealthStep,
    ) -> Result<Vec<NormalWaveDefeat>, NormalWaveError> {
        step.death_events
            .iter()
            .map(|event| {
                Ok(NormalWaveDefeat {
                    instance_id: self.instance_for_entity(event.actor_id)?,
                    entity_id: event.actor_id,
                    death_tick: event.tick,
                    reward_due_tick: event.reward_due_tick,
                })
            })
            .collect()
    }

    fn advance_living_timelines(
        &mut self,
        alive: &BTreeSet<EntityId>,
        timeline_events: &mut Vec<NormalWaveTimelineEvent>,
        actor_movements: &mut Vec<EnemyActorMovement>,
        hostile_spawn_events: &mut Vec<HostileEvent>,
        lane_events: &mut Vec<NormalWaveLaneEvent>,
    ) -> Result<(), NormalWaveError> {
        let local_tick = self.local_tick()?;
        for actor in &mut self.actors {
            if !alive.contains(&actor.entity_id) {
                continue;
            }
            if actor.timeline.tick() != local_tick {
                return Err(NormalWaveError::TimelineTickMismatch {
                    instance_id: actor.spawn.instance_id,
                });
            }
            let target = self
                .players
                .values()
                .filter(|player| {
                    player.consumables.vitals().current_health() > 0
                        && !player.target.target_is_immune
                })
                .min_by(|left, right| {
                    let left_delta = left.target.position - actor.actor.position();
                    let right_delta = right.target.position - actor.actor.position();
                    left_delta
                        .length_squared()
                        .total_cmp(&right_delta.length_squared())
                        .then_with(|| left.target.entity_id.cmp(&right.target.entity_id))
                });
            let input = if actor.spawn.kind == NormalWaveEnemyKind::DrownedPilgrim {
                target.map_or(Ok(PilgrimTargetInput::ABSENT), |player| {
                    actor.actor.target_input(player.target.position)
                })?
            } else {
                PilgrimTargetInput::ABSENT
            };
            let events = actor.timeline.advance(input)?;
            if self.tick < self.activation_tick && events.iter().any(authorizes_attack) {
                return Err(NormalWaveError::AttackDuringSpawnTelegraph {
                    instance_id: actor.spawn.instance_id,
                });
            }
            for event in events {
                if let Some(movement) = actor.actor.apply_event(&self.arena, &event)? {
                    self.health
                        .update_actor_position(actor.entity_id, movement.to)?;
                    actor_movements.push(movement);
                }
                if matches!(
                    event,
                    EnemyEvent::FanFired { .. } | EnemyEvent::RingFired { .. }
                ) {
                    let mut spawned = self.hostile_projectiles.spawn_from_enemy_event(
                        actor.entity_id,
                        actor.actor.position(),
                        &event,
                    )?;
                    for event in &mut spawned {
                        shift_hostile_event(event, self.starts_at)?;
                    }
                    hostile_spawn_events.extend(spawned);
                }
                process_lane_event(
                    actor,
                    &event,
                    &mut self.active_lanes,
                    lane_events,
                    self.starts_at,
                )?;
                timeline_events.push(NormalWaveTimelineEvent {
                    instance_id: actor.spawn.instance_id,
                    entity_id: actor.entity_id,
                    kind: actor.spawn.kind,
                    event: shift_enemy_event(event, self.starts_at)?,
                });
            }
        }
        Ok(())
    }

    fn resolve_active_lanes(
        &mut self,
        events: &mut Vec<NormalWaveLaneEvent>,
    ) -> Result<(), NormalWaveError> {
        for lane_index in 0..self.active_lanes.len() {
            let (instance_id, lane) = self.active_lanes[lane_index].clone();
            let actor_index = self
                .actors
                .binary_search_by_key(&instance_id, |actor| actor.spawn.instance_id)
                .map_err(|_| NormalWaveError::UnknownInstance(instance_id))?;
            let player_ids = self
                .players
                .iter()
                .filter(|(_, player)| {
                    player.consumables.vitals().current_health() > 0
                        && !player.target.target_is_immune
                        && lane.geometry.contacts_player(player.target.position)
                })
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            for player_id in player_ids {
                let registered = self.actors[actor_index]
                    .timeline
                    .register_lane_contact(lane.cast_id, player_id.get())?;
                if !registered {
                    continue;
                }
                let player = self
                    .players
                    .get_mut(&player_id)
                    .ok_or(NormalWaveError::UnknownCombatPlayer)?;
                let damage = resolve_lane_contact_with_policy(
                    lane.source_entity_id,
                    &lane.attack,
                    lane.geometry,
                    &mut player.target,
                    &mut player.consumables,
                    &mut player.combat,
                    self.damage_policy,
                )?
                .ok_or(NormalWaveError::LaneGeometryDisagreed)?;
                events.push(NormalWaveLaneEvent::Contact {
                    instance_id,
                    source_entity_id: lane.source_entity_id,
                    pattern_id: lane.attack.pattern_id,
                    cast_id: lane.cast_id,
                    player_entity_id: player_id,
                    damage: Box::new(damage),
                });
            }
        }
        Ok(())
    }

    fn clear_hostiles(&mut self) -> NormalWaveClearedHostiles {
        let projectiles = self.hostile_projectiles.clear_projectiles();
        self.active_lanes
            .sort_by_key(|(instance, lane)| (*instance, lane.cast_id));
        let lanes = self.active_lanes.drain(..).map(|(_, lane)| lane).collect();
        NormalWaveClearedHostiles { projectiles, lanes }
    }

    fn instance_for_entity(&self, entity_id: EntityId) -> Result<SpawnInstanceId, NormalWaveError> {
        self.instance_by_entity
            .get(&entity_id)
            .copied()
            .ok_or(NormalWaveError::UnknownEntity(entity_id))
    }
}

fn process_lane_event(
    actor: &mut NormalWaveActor,
    event: &EnemyEvent,
    active_lanes: &mut Vec<(SpawnInstanceId, ActiveEnemyLane)>,
    lane_events: &mut Vec<NormalWaveLaneEvent>,
    starts_at: Tick,
) -> Result<(), NormalWaveError> {
    match event {
        EnemyEvent::LanesActivated {
            cast_id,
            active_until,
            ..
        } => {
            let (resolved_cast, geometry, attack) = LaneGeometry::from_activation(event)?;
            if resolved_cast != *cast_id {
                return Err(NormalWaveError::LaneCastMismatch);
            }
            let global_active_until = add_global_tick(starts_at, *active_until)?;
            let lane = ActiveEnemyLane {
                source_entity_id: actor.entity_id,
                cast_id: *cast_id,
                geometry: geometry.with_origin(actor.actor.position()),
                attack,
                active_until: global_active_until,
            };
            active_lanes.push((actor.spawn.instance_id, lane));
            active_lanes.sort_by_key(|(instance, lane)| (*instance, lane.cast_id));
            lane_events.push(NormalWaveLaneEvent::Activated {
                instance_id: actor.spawn.instance_id,
                source_entity_id: actor.entity_id,
                cast_id: *cast_id,
                active_until: global_active_until,
            });
        }
        EnemyEvent::LanesExpired { cast_id } => {
            let index = active_lanes
                .iter()
                .position(|(instance, lane)| {
                    *instance == actor.spawn.instance_id && lane.cast_id == *cast_id
                })
                .ok_or(NormalWaveError::MissingActiveLane {
                    instance_id: actor.spawn.instance_id,
                    cast_id: *cast_id,
                })?;
            active_lanes.remove(index);
            lane_events.push(NormalWaveLaneEvent::Expired {
                instance_id: actor.spawn.instance_id,
                source_entity_id: actor.entity_id,
                cast_id: *cast_id,
            });
        }
        _ => {}
    }
    Ok(())
}

fn authorizes_attack(event: &EnemyEvent) -> bool {
    matches!(
        event,
        EnemyEvent::FanFired { .. }
            | EnemyEvent::RingFired { .. }
            | EnemyEvent::LanesActivated { .. }
    )
}

fn add_global_ticks(start: Tick, ticks: u32) -> Result<Tick, NormalWaveError> {
    start
        .0
        .checked_add(u64::from(ticks))
        .map(Tick)
        .ok_or(NormalWaveError::TickOverflow)
}

fn add_global_tick(start: Tick, local: Tick) -> Result<Tick, NormalWaveError> {
    start
        .0
        .checked_add(local.0)
        .map(Tick)
        .ok_or(NormalWaveError::TickOverflow)
}

fn shift_enemy_event(
    mut event: EnemyEvent,
    starts_at: Tick,
) -> Result<EnemyEvent, NormalWaveError> {
    match &mut event {
        EnemyEvent::SpawnTelegraph { ends_at, .. } => {
            *ends_at = add_global_tick(starts_at, *ends_at)?;
        }
        EnemyEvent::AimLocked { fires_at, .. } | EnemyEvent::RingTelegraph { fires_at, .. } => {
            *fires_at = add_global_tick(starts_at, *fires_at)?;
        }
        EnemyEvent::LaneTelegraph { impacts_at, .. } => {
            *impacts_at = add_global_tick(starts_at, *impacts_at)?;
        }
        EnemyEvent::LanesActivated { active_until, .. } => {
            *active_until = add_global_tick(starts_at, *active_until)?;
        }
        EnemyEvent::StateChanged { .. }
        | EnemyEvent::ApproachIntent { .. }
        | EnemyEvent::FanFired { .. }
        | EnemyEvent::RingFired { .. }
        | EnemyEvent::LanesExpired { .. } => {}
    }
    Ok(event)
}

fn shift_hostile_step(step: &mut HostileStep, starts_at: Tick) -> Result<(), NormalWaveError> {
    step.tick = add_global_tick(starts_at, step.tick)?;
    for event in &mut step.events {
        shift_hostile_event(event, starts_at)?;
    }
    Ok(())
}

fn shift_hostile_event(event: &mut HostileEvent, starts_at: Tick) -> Result<(), NormalWaveError> {
    let tick = match event {
        HostileEvent::Spawned { tick, .. }
        | HostileEvent::Moved { tick, .. }
        | HostileEvent::Contact { tick, .. }
        | HostileEvent::ProjectileGraceIgnored { tick, .. }
        | HostileEvent::Expired { tick, .. } => tick,
    };
    *tick = add_global_tick(starts_at, *tick)?;
    Ok(())
}

fn validate_telegraph_ticks(definitions: &NormalWaveDefinitions) -> Result<(), NormalWaveError> {
    let ticks = [
        definitions
            .drowned_pilgrim
            .parameters()
            .spawn_telegraph_ticks,
        definitions.bell_reed.parameters().spawn_telegraph_ticks,
        definitions.chain_sentry.parameters().spawn_telegraph_ticks,
    ];
    if ticks
        .iter()
        .any(|ticks| *ticks != FIRST_PLAYABLE_SPAWN_TELEGRAPH_TICKS)
    {
        return Err(NormalWaveError::SpawnTelegraphDefinitionDrift);
    }
    Ok(())
}

fn validate_spawn_identities(
    spawns: &[NormalWaveSpawn],
    player_id: EntityId,
) -> Result<(), NormalWaveError> {
    let mut instances = BTreeSet::new();
    let mut entities = BTreeSet::new();
    for spawn in spawns {
        if !instances.insert(spawn.instance_id) {
            return Err(NormalWaveError::DuplicateInstance(spawn.instance_id));
        }
        let entity_id = normal_wave_entity_id(spawn.instance_id)?;
        if !entities.insert(entity_id) {
            return Err(NormalWaveError::DuplicateEntity(entity_id));
        }
        if entity_id == player_id {
            return Err(NormalWaveError::EntityMatchesPlayer(entity_id));
        }
    }
    Ok(())
}

#[allow(clippy::cast_precision_loss)] // Authored arena coordinates are tightly bounded milli-tiles.
fn build_actor(
    definitions: &NormalWaveDefinitions,
    spawn: NormalWaveSpawn,
    entity_id: EntityId,
) -> Result<(EnemyActor, NormalEnemyTimeline, crate::EnemyHealthActor), NormalWaveError> {
    let position = crate::SimulationVector::new(
        spawn.position_milli_tiles.0 as f32 / 1_000.0,
        spawn.position_milli_tiles.1 as f32 / 1_000.0,
    );
    let (actor_kind, radius, timeline, health) = match spawn.kind {
        NormalWaveEnemyKind::DrownedPilgrim => (
            EnemyActorKind::DrownedPilgrim,
            definitions
                .drowned_pilgrim
                .parameters()
                .hurtbox_radius_milli_tiles,
            NormalEnemyTimeline::DrownedPilgrim(DrownedPilgrimSimulation::new(
                definitions.drowned_pilgrim.clone(),
            )),
            crate::EnemyHealthActor::drowned_pilgrim(
                entity_id,
                &definitions.drowned_pilgrim,
                position,
            ),
        ),
        NormalWaveEnemyKind::BellReed => (
            EnemyActorKind::BellReed,
            definitions
                .bell_reed
                .parameters()
                .hurtbox_radius_milli_tiles,
            NormalEnemyTimeline::BellReed(BellReedSimulation::new(definitions.bell_reed.clone())),
            crate::EnemyHealthActor::bell_reed(entity_id, &definitions.bell_reed, position),
        ),
        NormalWaveEnemyKind::ChainSentry => (
            EnemyActorKind::ChainSentry,
            definitions
                .chain_sentry
                .parameters()
                .hurtbox_radius_milli_tiles,
            NormalEnemyTimeline::ChainSentry(ChainSentrySimulation::new(
                definitions.chain_sentry.clone(),
            )),
            crate::EnemyHealthActor::chain_sentry(entity_id, &definitions.chain_sentry, position),
        ),
    };
    Ok((
        EnemyActor::new(
            entity_id,
            actor_kind,
            spawn.position_milli_tiles.0,
            spawn.position_milli_tiles.1,
            radius,
        )?,
        timeline,
        health,
    ))
}

fn validate_authored_geometry(
    arena: &ArenaGeometry,
    actors: &[NormalWaveActor],
) -> Result<(), NormalWaveError> {
    let hurtboxes = actors
        .iter()
        .map(|actor| {
            let radius = match actor.spawn.kind {
                NormalWaveEnemyKind::DrownedPilgrim => 0.34,
                NormalWaveEnemyKind::BellReed => 0.42,
                NormalWaveEnemyKind::ChainSentry => 0.55,
            };
            EnemyHurtbox::new(actor.entity_id, actor.actor.position(), radius)
        })
        .collect::<Result<Vec<_>, _>>()?;
    crate::ProjectileCollisionWorld::new(arena, hurtboxes)?;
    Ok(())
}

#[derive(Debug, Error)]
pub enum NormalWaveError {
    #[error("normal wave must contain at least one enemy")]
    EmptyWave,
    #[error("normal wave must retain at least one player")]
    MissingPlayer,
    #[error("duplicate normal-wave player {0}")]
    DuplicatePlayer(EntityId),
    #[error("combat step or lane contact referenced a player outside the wave roster")]
    UnknownCombatPlayer,
    #[error("player {0} position is non-finite")]
    InvalidPlayerPosition(EntityId),
    #[error("normal-wave roster is locked after the first step")]
    RosterLocked,
    #[error("single-player handoff cannot consume a shared player roster")]
    SharedHandoffUnsupported,
    #[error("First Playable spawn telegraph must remain exactly 27 ticks")]
    SpawnTelegraphDefinitionDrift,
    #[error("duplicate spawn instance {0:?}")]
    DuplicateInstance(SpawnInstanceId),
    #[error("duplicate enemy entity {0}")]
    DuplicateEntity(EntityId),
    #[error("enemy entity {0} equals the player entity")]
    EntityMatchesPlayer(EntityId),
    #[error("combat tick mismatch: expected {expected:?}, received {received:?}")]
    CombatTickMismatch { expected: Tick, received: Tick },
    #[error("friendly damage is forbidden during the harmless spawn telegraph")]
    DamageDuringSpawnTelegraph,
    #[error("enemy {instance_id:?} authorized an attack during the spawn telegraph")]
    AttackDuringSpawnTelegraph { instance_id: SpawnInstanceId },
    #[error("enemy {instance_id:?} timeline tick diverged from the wave")]
    TimelineTickMismatch { instance_id: SpawnInstanceId },
    #[error("hostile projectile tick diverged from the wave")]
    HostileTickMismatch,
    #[error("unknown spawn instance {0:?}")]
    UnknownInstance(SpawnInstanceId),
    #[error("unknown wave entity {0}")]
    UnknownEntity(EntityId),
    #[error("lane activation cast mismatch")]
    LaneCastMismatch,
    #[error("lane geometry and contact resolver disagreed")]
    LaneGeometryDisagreed,
    #[error("missing active lane {cast_id:?} for {instance_id:?}")]
    MissingActiveLane {
        instance_id: SpawnInstanceId,
        cast_id: AttackCastId,
    },
    #[error("normal wave tick overflow")]
    TickOverflow,
    #[error("normal wave tick preceded its authored global start")]
    TickBeforeWaveStart,
    #[error("normal wave handoff requires a cleared phase with zero hostile authority")]
    HandoffBeforeClear,
    #[error(transparent)]
    EntityIdentity(#[from] NormalWaveEntityIdError),
    #[error(transparent)]
    Hostile(#[from] HostileError),
    #[error(transparent)]
    EnemyRuntime(#[from] EnemyRuntimeError),
    #[error(transparent)]
    EnemyHealth(#[from] EnemyHealthError),
    #[error(transparent)]
    Collision(#[from] CollisionError),
    #[error(transparent)]
    Hurtbox(#[from] crate::HurtboxError),
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::*;
    use crate::{
        CollisionTarget, FriendlyProjectileSource, GraveMarkDefinition,
        GraveMarkDefinitionParameters, HostileTargetState, PlayerCombatState, PlayerVitals,
        ProjectileCollision, RawDamageIntent, RawDamageIntentSource, RedTonicDefinition,
        RedTonicSimulation, SimulationVector, SlipstepDefinition, SlipstepDefinitionParameters,
        StillnessDefinition, StillnessDefinitionParameters, TilePoint, TonicBelt, WeaponDefinition,
        WeaponDefinitionParameters,
    };

    fn id(value: u64) -> EntityId {
        EntityId::new(value).expect("nonzero ID")
    }

    fn instance(ordinal: u16) -> SpawnInstanceId {
        SpawnInstanceId {
            run_ordinal: 7,
            spawn_ordinal: ordinal,
        }
    }

    fn arena() -> ArenaGeometry {
        ArenaGeometry {
            id: "arena.test.normal_wave".to_owned(),
            width_milli_tiles: 32_000,
            height_milli_tiles: 24_000,
            shell_thickness_milli_tiles: 1_000,
            player_spawn: TilePoint::new(16_000, 12_000),
            boss_spawn: TilePoint::new(24_000, 12_000),
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

    fn player() -> EnemyLabPlayer {
        EnemyLabPlayer {
            target: HostileTargetState {
                entity_id: id(900),
                position: SimulationVector::new(16.0, 12.0),
                target_is_immune: false,
                resistance_basis_points: 0,
                additional_direct_damage_reductions_basis_points: Vec::new(),
                armor: 2,
                current_barrier: 0,
                health_damage_cap_basis_points: None,
            },
            consumables: RedTonicSimulation::new(
                RedTonicDefinition::first_playable(),
                PlayerVitals::new(1_000, 1_000).expect("vitals"),
                TonicBelt::first_playable(),
            )
            .expect("tonic"),
            combat: combat(),
        }
    }

    fn player_at(entity_id: u64, position: SimulationVector) -> EnemyLabPlayer {
        let mut player = player();
        player.target.entity_id = id(entity_id);
        player.target.position = position;
        player
    }

    fn spawn(
        ordinal: u16,
        _legacy_entity: u64,
        kind: NormalWaveEnemyKind,
        position: (i32, i32),
    ) -> NormalWaveSpawn {
        NormalWaveSpawn {
            instance_id: instance(ordinal),
            kind,
            position_milli_tiles: position,
        }
    }

    fn wave_id(ordinal: u16) -> EntityId {
        normal_wave_entity_id(instance(ordinal)).expect("wave entity")
    }

    fn simulation(spawns: Vec<NormalWaveSpawn>) -> NormalWaveSimulation {
        simulation_at(spawns, Tick(0))
    }

    fn simulation_at(spawns: Vec<NormalWaveSpawn>, starts_at: Tick) -> NormalWaveSimulation {
        NormalWaveSimulation::new(
            NormalWaveDefinitions::first_playable(),
            arena(),
            spawns,
            player(),
            EntityIdAllocator::starting_at(NonZeroU64::new(10_000).expect("nonzero")),
            starts_at,
        )
        .expect("wave")
    }

    fn empty_step(tick: u64) -> CombatStep {
        CombatStep {
            tick: Tick(tick),
            ..CombatStep::default()
        }
    }

    fn lethal_step(tick: u64, target: EntityId) -> CombatStep {
        let intent = RawDamageIntent {
            tick: Tick(tick),
            projectile_id: id(50_000 + tick),
            source: RawDamageIntentSource::Primary,
            target,
            base_raw_damage: 100,
            multiplier_basis_points: 10_000,
            resolved_raw_damage: 100,
            contact_ordinal: 0,
        };
        CombatStep {
            tick: Tick(tick),
            collisions: vec![ProjectileCollision {
                tick: intent.tick,
                projectile_id: intent.projectile_id,
                source: FriendlyProjectileSource::Primary,
                target: CollisionTarget::Enemy(target),
                final_position: SimulationVector::new(8.0, 3.0),
                distance_travelled_tiles: 1.0,
                contact_ordinal: 0,
                empowered_by_slipstep: false,
                focused_by_stillness: false,
                projectile_continues: false,
            }],
            raw_damage_intents: vec![intent],
            ..CombatStep::default()
        }
    }

    fn lethal_all_step(tick: u64, mut targets: Vec<EntityId>) -> CombatStep {
        targets.sort_unstable();
        let mut step = CombatStep {
            tick: Tick(tick),
            ..CombatStep::default()
        };
        for (index, target) in targets.into_iter().enumerate() {
            let projectile_id =
                id(50_000 + tick * 32 + u64::try_from(index).expect("bounded test index"));
            step.collisions.push(ProjectileCollision {
                tick: step.tick,
                projectile_id,
                source: FriendlyProjectileSource::Primary,
                target: CollisionTarget::Enemy(target),
                final_position: SimulationVector::new(8.0, 3.0),
                distance_travelled_tiles: 1.0,
                contact_ordinal: 0,
                empowered_by_slipstep: false,
                focused_by_stillness: false,
                projectile_continues: false,
            });
            step.raw_damage_intents.push(RawDamageIntent {
                tick: step.tick,
                projectile_id,
                source: RawDamageIntentSource::Primary,
                target,
                base_raw_damage: 1_000,
                multiplier_basis_points: 10_000,
                resolved_raw_damage: 1_000,
                contact_ordinal: 0,
            });
        }
        step
    }

    fn damage_step(tick: u64, projectile_id: u64, target: EntityId, raw_damage: u32) -> CombatStep {
        let intent = RawDamageIntent {
            tick: Tick(tick),
            projectile_id: id(projectile_id),
            source: RawDamageIntentSource::Primary,
            target,
            base_raw_damage: raw_damage,
            multiplier_basis_points: 10_000,
            resolved_raw_damage: raw_damage,
            contact_ordinal: 0,
        };
        CombatStep {
            tick: Tick(tick),
            collisions: vec![ProjectileCollision {
                tick: intent.tick,
                projectile_id: intent.projectile_id,
                source: FriendlyProjectileSource::Primary,
                target: CollisionTarget::Enemy(target),
                final_position: SimulationVector::new(8.0, 3.0),
                distance_travelled_tiles: 1.0,
                contact_ordinal: 0,
                empowered_by_slipstep: false,
                focused_by_stillness: false,
                projectile_continues: false,
            }],
            raw_damage_intents: vec![intent],
            ..CombatStep::default()
        }
    }

    #[test]
    fn mixed_completion_hold_defers_terminal_cleanup_until_external_hostiles_clear() {
        let target = wave_id(1);
        let mut wave = simulation(vec![spawn(
            1,
            0,
            NormalWaveEnemyKind::DrownedPilgrim,
            (8_000, 3_000),
        )]);
        for tick in 0..27 {
            wave.step_with_external_hostiles(&empty_step(tick), 1)
                .expect("warning tick");
        }
        let held = wave
            .step_with_external_hostiles(&lethal_step(27, target), 1)
            .expect("held clear");
        assert_eq!(held.phase_after, NormalWavePhase::Active);
        assert!(held.cleared_hostiles.is_none());
        assert!(!wave.snapshots()[0].health.alive);

        let cleared = wave
            .step_with_external_hostiles(&empty_step(28), 0)
            .expect("external clear");
        assert_eq!(
            cleared.phase_after,
            NormalWavePhase::Cleared {
                cleared_at: Tick(28)
            }
        );
        assert!(cleared.cleared_hostiles.is_some());
    }

    #[test]
    fn shared_players_damage_one_enemy_in_global_projectile_order() {
        let mut wave = simulation(vec![spawn(
            1,
            101,
            NormalWaveEnemyKind::DrownedPilgrim,
            (8_000, 3_000),
        )]);
        wave.add_player(player_at(901, SimulationVector::new(15.0, 12.0)))
            .unwrap();
        for tick in 0..u64::from(FIRST_PLAYABLE_SPAWN_TELEGRAPH_TICKS) {
            wave.step_players(&BTreeMap::from([
                (id(900), empty_step(tick)),
                (id(901), empty_step(tick)),
            ]))
            .unwrap();
        }
        let step = wave
            .step_players(&BTreeMap::from([
                (id(900), damage_step(27, 60_002, wave_id(1), 20)),
                (id(901), damage_step(27, 60_001, wave_id(1), 20)),
            ]))
            .unwrap();
        assert_eq!(step.enemy_health_step.damage_events.len(), 2);
        assert_eq!(
            step.enemy_health_step
                .damage_events
                .iter()
                .map(|event| event.projectile_id.get())
                .collect::<Vec<_>>(),
            vec![60_001, 60_002]
        );
        assert_eq!(wave.snapshots()[0].health.current_health, 45);
    }

    #[test]
    fn active_lane_contacts_each_shared_player_once_per_cast() {
        let mut wave = simulation(vec![spawn(
            1,
            101,
            NormalWaveEnemyKind::ChainSentry,
            (16_000, 12_000),
        )]);
        wave.add_player(player_at(901, SimulationVector::new(16.0, 12.0)))
            .unwrap();
        let mut contacts = Vec::new();
        for tick in 0..240 {
            let step = wave
                .step_players(&BTreeMap::from([
                    (id(900), empty_step(tick)),
                    (id(901), empty_step(tick)),
                ]))
                .unwrap();
            contacts.extend(
                step.lane_events
                    .into_iter()
                    .filter_map(|event| match event {
                        NormalWaveLaneEvent::Contact {
                            cast_id,
                            player_entity_id,
                            ..
                        } => Some((cast_id, player_entity_id)),
                        _ => None,
                    }),
            );
            if contacts.len() >= 2 {
                break;
            }
        }
        assert_eq!(contacts.len(), 2);
        assert_eq!(contacts[0].0, contacts[1].0);
        assert_eq!(
            contacts
                .iter()
                .map(|(_, player)| *player)
                .collect::<Vec<_>>(),
            vec![id(900), id(901)]
        );
    }

    #[test]
    fn arbitrary_duplicates_keep_stable_instance_mapping_and_harmless_telegraph() {
        let mut wave = simulation(vec![
            spawn(5, 105, NormalWaveEnemyKind::BellReed, (24_000, 3_000)),
            spawn(1, 101, NormalWaveEnemyKind::DrownedPilgrim, (8_000, 3_000)),
            spawn(4, 104, NormalWaveEnemyKind::BellReed, (8_000, 21_000)),
            spawn(
                2,
                102,
                NormalWaveEnemyKind::DrownedPilgrim,
                (24_000, 21_000),
            ),
            spawn(3, 103, NormalWaveEnemyKind::ChainSentry, (16_000, 12_000)),
        ]);
        assert_eq!(
            wave.snapshots()
                .iter()
                .map(|snapshot| snapshot.instance_id.spawn_ordinal)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5]
        );
        assert_eq!(wave.entity_id_for(instance(4)), Some(wave_id(4)));
        assert_eq!(wave.instance_id_for(wave_id(2)), Some(instance(2)));

        for tick in 0..u64::from(FIRST_PLAYABLE_SPAWN_TELEGRAPH_TICKS) {
            assert!(wave.alive_hurtboxes().expect("hurtboxes").is_empty());
            let step = wave.step(&empty_step(tick)).expect("dormant step");
            assert!(step.hostile_spawn_events.is_empty());
            assert!(step.lane_events.is_empty());
            assert!(
                !step
                    .timeline_events
                    .iter()
                    .any(|event| authorizes_attack(&event.event))
            );
        }
        assert_eq!(
            wave.alive_hurtboxes().expect("activated hurtboxes").len(),
            5
        );
        let activation = wave.step(&empty_step(27)).expect("activation");
        assert!(activation.activated);
        assert_eq!(activation.phase_after, NormalWavePhase::Active);
    }

    #[test]
    fn duplicate_timeline_sources_share_hostile_resolution_without_identity_loss() {
        let mut wave = simulation(vec![
            spawn(
                1,
                101,
                NormalWaveEnemyKind::DrownedPilgrim,
                (12_000, 10_000),
            ),
            spawn(
                2,
                102,
                NormalWaveEnemyKind::DrownedPilgrim,
                (20_000, 10_000),
            ),
            spawn(3, 103, NormalWaveEnemyKind::BellReed, (8_000, 18_000)),
            spawn(4, 104, NormalWaveEnemyKind::ChainSentry, (24_000, 18_000)),
        ]);
        let mut fan_sources = BTreeSet::new();
        let mut saw_ring = false;
        let mut saw_lane = false;
        for tick in 0..100 {
            let step = wave.step(&empty_step(tick)).expect("timeline");
            for event in &step.timeline_events {
                match event.event {
                    EnemyEvent::FanFired { .. } => {
                        fan_sources.insert((event.instance_id, event.entity_id));
                    }
                    EnemyEvent::RingFired { .. } => saw_ring = true,
                    EnemyEvent::LanesActivated { .. } => saw_lane = true,
                    _ => {}
                }
            }
        }
        assert_eq!(
            fan_sources,
            BTreeSet::from([(instance(1), wave_id(1)), (instance(2), wave_id(2)),])
        );
        assert!(saw_ring);
        assert!(saw_lane);
    }

    #[test]
    fn lethal_tick_reports_stable_defeat_clears_hostiles_and_drops_at_exact_plus_eight() {
        let mut wave = simulation_at(
            vec![spawn(
                1,
                101,
                NormalWaveEnemyKind::DrownedPilgrim,
                (8_000, 3_000),
            )],
            Tick(45),
        );
        assert_eq!(wave.starts_at(), Tick(45));
        assert_eq!(
            wave.phase(),
            NormalWavePhase::DormantTelegraph {
                activates_at: Tick(72)
            }
        );
        let first = wave.step(&empty_step(45)).expect("global first step");
        assert!(first.timeline_events.iter().any(|event| matches!(
            event.event,
            EnemyEvent::SpawnTelegraph {
                ends_at: Tick(72),
                ..
            }
        )));
        assert_eq!(first.hostile_step.tick, Tick(45));
        for tick in 46..72 {
            wave.step(&empty_step(tick)).expect("telegraph");
        }
        let lethal = wave.step(&lethal_step(72, wave_id(1))).expect("lethal");
        assert_eq!(
            lethal.defeats,
            vec![NormalWaveDefeat {
                instance_id: instance(1),
                entity_id: wave_id(1),
                death_tick: Tick(72),
                reward_due_tick: Tick(80),
            }]
        );
        assert_eq!(
            lethal.phase_after,
            NormalWavePhase::Cleared {
                cleared_at: Tick(72)
            }
        );
        assert!(lethal.cleared_hostiles.is_some());
        assert!(wave.alive_hurtboxes().expect("dead hurtboxes").is_empty());
        for tick in 73..80 {
            assert!(
                wave.step(&empty_step(tick))
                    .expect("drop wait")
                    .drops
                    .is_empty()
            );
        }
        let due = wave.step(&empty_step(80)).expect("due drop");
        assert_eq!(due.drops.len(), 1);
        assert_eq!(due.drops[0].instance_id, instance(1));
        assert_eq!(due.drops[0].event.death_tick, Tick(72));
        assert_eq!(due.drops[0].event.due_tick, Tick(80));
    }

    #[test]
    fn invalid_identity_and_dormant_damage_fail_transactionally() {
        let duplicate = NormalWaveSimulation::new(
            NormalWaveDefinitions::first_playable(),
            arena(),
            vec![
                spawn(1, 101, NormalWaveEnemyKind::DrownedPilgrim, (8_000, 3_000)),
                spawn(1, 102, NormalWaveEnemyKind::BellReed, (24_000, 3_000)),
            ],
            player(),
            EntityIdAllocator::default(),
            Tick(0),
        );
        assert!(
            matches!(duplicate, Err(NormalWaveError::DuplicateInstance(value)) if value == instance(1))
        );

        let mut wave = simulation(vec![spawn(
            1,
            101,
            NormalWaveEnemyKind::DrownedPilgrim,
            (8_000, 3_000),
        )]);
        assert!(matches!(
            wave.step(&lethal_step(0, wave_id(1))),
            Err(NormalWaveError::DamageDuringSpawnTelegraph)
        ));
        assert_eq!(wave.tick(), Tick(0));
        assert_eq!(wave.snapshots()[0].health.current_health, 85);
    }

    #[test]
    fn run_qualified_identity_namespaces_are_stable_disjoint_and_checked() {
        let first = SpawnInstanceId {
            run_ordinal: 1,
            spawn_ordinal: 1,
        };
        let later = SpawnInstanceId {
            run_ordinal: 2,
            spawn_ordinal: 1,
        };
        assert_eq!(normal_wave_entity_id(first).expect("first").get(), 30_001);
        assert_eq!(normal_wave_entity_id(first), normal_wave_entity_id(first));
        assert_eq!(normal_wave_entity_id(later).expect("later").get(), 130_001);
        assert_ne!(normal_wave_entity_id(first), normal_wave_entity_id(later));
        assert_eq!(
            normal_wave_projectile_allocator(1)
                .expect("allocator")
                .peek()
                .get(),
            20_000
        );
        assert_eq!(
            normal_wave_entity_id(SpawnInstanceId {
                run_ordinal: 0,
                spawn_ordinal: 1,
            }),
            Err(NormalWaveEntityIdError::ZeroRunOrdinal)
        );
        assert_eq!(
            normal_wave_entity_id(SpawnInstanceId {
                run_ordinal: 1,
                spawn_ordinal: 0,
            }),
            Err(NormalWaveEntityIdError::SpawnOrdinalOutOfRange(0))
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)] // One journey fixture intentionally keeps all three rosters and the boss handoff visible.
    fn exact_four_six_six_journey_uses_real_damage_and_preserves_handoff() {
        let rosters = [
            vec![
                spawn(1, 0, NormalWaveEnemyKind::DrownedPilgrim, (8_000, 3_000)),
                spawn(2, 0, NormalWaveEnemyKind::DrownedPilgrim, (24_000, 3_000)),
                spawn(3, 0, NormalWaveEnemyKind::DrownedPilgrim, (8_000, 21_000)),
                spawn(4, 0, NormalWaveEnemyKind::DrownedPilgrim, (24_000, 21_000)),
            ],
            vec![
                spawn(5, 0, NormalWaveEnemyKind::BellReed, (16_000, 3_000)),
                spawn(6, 0, NormalWaveEnemyKind::BellReed, (16_000, 21_000)),
                spawn(7, 0, NormalWaveEnemyKind::DrownedPilgrim, (3_000, 8_000)),
                spawn(8, 0, NormalWaveEnemyKind::DrownedPilgrim, (3_000, 16_000)),
                spawn(9, 0, NormalWaveEnemyKind::DrownedPilgrim, (29_000, 8_000)),
                spawn(10, 0, NormalWaveEnemyKind::DrownedPilgrim, (29_000, 16_000)),
            ],
            vec![
                spawn(11, 0, NormalWaveEnemyKind::ChainSentry, (16_000, 12_000)),
                spawn(12, 0, NormalWaveEnemyKind::BellReed, (8_000, 6_000)),
                spawn(13, 0, NormalWaveEnemyKind::BellReed, (8_000, 18_000)),
                spawn(14, 0, NormalWaveEnemyKind::DrownedPilgrim, (29_000, 8_000)),
                spawn(15, 0, NormalWaveEnemyKind::DrownedPilgrim, (29_000, 16_000)),
                spawn(16, 0, NormalWaveEnemyKind::DrownedPilgrim, (24_000, 3_000)),
            ],
        ];
        assert_eq!(rosters.each_ref().map(Vec::len), [4, 6, 6]);
        let mut handoff = NormalWaveHandoff {
            player: player(),
            hostile_projectile_ids: normal_wave_projectile_allocator(7).expect("allocator"),
        };
        let mut starts_at = 45_u64;
        let mut all_defeats = Vec::new();
        for roster in rosters {
            let mut wave = NormalWaveSimulation::new(
                NormalWaveDefinitions::first_playable(),
                arena(),
                roster,
                handoff.player,
                handoff.hostile_projectile_ids,
                Tick(starts_at),
            )
            .expect("authored wave");
            for tick in starts_at..starts_at + 27 {
                let step = wave.step(&empty_step(tick)).expect("telegraph");
                assert!(step.defeats.is_empty());
                assert!(step.hostile_spawn_events.is_empty());
            }
            let targets = wave
                .snapshots()
                .into_iter()
                .map(|snapshot| snapshot.entity_id)
                .collect::<Vec<_>>();
            let lethal_tick = starts_at + 27;
            let clear = wave
                .step(&lethal_all_step(lethal_tick, targets))
                .expect("real damage clear");
            assert_eq!(
                clear.defeats.len(),
                clear.enemy_health_step.death_events.len()
            );
            assert!(clear.cleared_hostiles.is_some());
            assert!(
                clear
                    .defeats
                    .windows(2)
                    .all(|pair| pair[0].instance_id < pair[1].instance_id)
            );
            all_defeats.extend(clear.defeats.iter().map(|defeat| defeat.instance_id));
            for tick in lethal_tick + 1..=lethal_tick + 8 {
                wave.step(&empty_step(tick)).expect("drop delay");
            }
            handoff = wave.into_handoff().expect("cleared handoff");
            starts_at = lethal_tick + 8 + 45;
        }
        assert_eq!(all_defeats.len(), 16);
        assert_eq!(handoff.hostile_projectile_ids.peek().get(), 620_000);
        assert_eq!(handoff.player.consumables.vitals().current_health(), 1_000);

        let boss_starts_at = starts_at + 60;
        let mut boss = crate::BellProctorEncounterSimulation::new(
            crate::BellProctorDefinition::first_playable(),
            arena(),
            handoff,
            7,
            Tick(boss_starts_at),
        )
        .expect("Wave 3 handoff starts Bell Proctor");
        let boss_id = boss.entity_id();
        let first = boss
            .step(&lethal_all_step(boss_starts_at, vec![boss_id]))
            .expect("first boss damage");
        assert!(first.defeat.is_none());
        let second = boss
            .step(&lethal_all_step(boss_starts_at + 1, vec![boss_id]))
            .expect("break-amplified boss damage");
        assert_eq!(
            second.friendly_damage[0].break_multiplier_basis_points,
            12_000
        );
        let lethal = boss
            .step(&lethal_all_step(boss_starts_at + 2, vec![boss_id]))
            .expect("real boss defeat");
        assert!(lethal.defeat.is_some());
        assert!(lethal.cleared_hostiles.is_some());
        assert_eq!(boss.snapshot().state, crate::BellProctorStateKind::Defeated);
    }

    #[test]
    fn duplicate_wave_timeline_replay_is_bit_identical_at_global_ticks() {
        fn replay() -> (Vec<NormalWaveStep>, NormalWaveSimulation) {
            let mut wave = simulation_at(
                vec![
                    spawn(1, 0, NormalWaveEnemyKind::DrownedPilgrim, (12_000, 10_000)),
                    spawn(2, 0, NormalWaveEnemyKind::DrownedPilgrim, (20_000, 10_000)),
                    spawn(3, 0, NormalWaveEnemyKind::BellReed, (8_000, 18_000)),
                    spawn(4, 0, NormalWaveEnemyKind::ChainSentry, (24_000, 18_000)),
                ],
                Tick(45),
            );
            let steps = (45..145)
                .map(|tick| wave.step(&empty_step(tick)).expect("replay step"))
                .collect();
            (steps, wave)
        }
        assert_eq!(replay(), replay());
    }
}
