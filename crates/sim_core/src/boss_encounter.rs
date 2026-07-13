//! Integrated authoritative Bell Proctor combat owner.
//!
//! The exact scheduler owns phase timing. This composite owns player handoff, real health,
//! hostile projectiles and lanes, same-tick cleanup, defeat, and globally comparable ticks.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::{
    AimVector, ArenaGeometry, AttackCastId, BellProctorDefinition, BellProctorSimulation,
    BellProctorStateKind, BossEvent, BossInput, CollisionTarget, CombatStep, DamageError,
    DamageEvent, DamageType, DirectHitParameters, DirectHitRequest, EnemyHurtbox, EnemyLabPlayer,
    EntityId, FriendlyProjectileSource, HostileDamagePolicy, HostileError, HostileEvent,
    HostileProjectile, HostileProjectileSimulation, HostileStep, HurtboxError,
    LaneAttackDefinition, LaneGeometry, RawDamageIntentSource, SimulationVector, Tick,
    resolve_direct_hit, resolve_lane_contact_with_policy,
};

pub const BELL_PROCTOR_ENTITY_ID_OFFSET: u64 = 40_001;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BellProctorDamageEvent {
    pub tick: Tick,
    pub projectile_id: EntityId,
    pub contact_ordinal: u32,
    pub source: RawDamageIntentSource,
    pub base_raw_damage: u32,
    pub authored_multiplier_basis_points: u32,
    pub break_multiplier_basis_points: u32,
    pub resolved_raw_damage: u32,
    pub damage: DamageEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BellProctorDefeat {
    pub tick: Tick,
    pub entity_id: EntityId,
    pub lethal_projectile_id: EntityId,
    pub lethal_contact_ordinal: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BellProctorStatusImmunityEvent {
    pub tick: Tick,
    pub source_trap_id: EntityId,
    pub target: EntityId,
    pub status: BellProctorImmuneStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BellProctorImmuneStatus {
    Frostbind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BellProctorLaneContact {
    pub tick: Tick,
    pub cast_id: AttackCastId,
    pub damage: crate::AppliedHostileDamage,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BellProctorClearedHostiles {
    pub projectiles: Vec<HostileProjectile>,
    pub lane_cast_ids: Vec<AttackCastId>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BellProctorEncounterStep {
    pub tick: Tick,
    pub scheduler_events: Vec<BossEvent>,
    pub hostile_spawn_events: Vec<HostileEvent>,
    pub lane_contacts: Vec<BellProctorLaneContact>,
    pub hostile_step: HostileStep,
    pub friendly_damage: Vec<BellProctorDamageEvent>,
    pub status_immunities: Vec<BellProctorStatusImmunityEvent>,
    pub defeat: Option<BellProctorDefeat>,
    pub cleared_hostiles: Option<BellProctorClearedHostiles>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BellProctorEncounterSnapshot {
    pub tick: Tick,
    pub local_tick: Tick,
    pub entity_id: EntityId,
    pub current_health: u32,
    pub maximum_health: u32,
    pub state: BellProctorStateKind,
    pub active_projectiles: usize,
    pub active_lanes: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct ActiveBossLane {
    cast_id: AttackCastId,
    geometry: LaneGeometry,
    attack: LaneAttackDefinition,
    active_until: Tick,
    contacted_player: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BellProctorEncounterSimulation {
    definition: BellProctorDefinition,
    arena: ArenaGeometry,
    entity_id: EntityId,
    position: SimulationVector,
    scheduler: BellProctorSimulation,
    current_health: u32,
    player: EnemyLabPlayer,
    hostile_projectiles: HostileProjectileSimulation,
    active_lanes: Vec<ActiveBossLane>,
    starts_at: Tick,
    damage_policy: HostileDamagePolicy,
    defeated: bool,
}

impl BellProctorEncounterSimulation {
    pub fn new(
        definition: BellProctorDefinition,
        arena: ArenaGeometry,
        handoff: crate::NormalWaveHandoff,
        run_ordinal: u32,
        starts_at: Tick,
    ) -> Result<Self, BellProctorEncounterError> {
        let parameters = definition.parameters();
        let position = SimulationVector::new(
            milli_to_tiles(parameters.position_x_milli_tiles),
            milli_to_tiles(parameters.position_y_milli_tiles),
        );
        if (
            parameters.position_x_milli_tiles,
            parameters.position_y_milli_tiles,
        ) != (
            arena.boss_spawn.x_milli_tiles,
            arena.boss_spawn.y_milli_tiles,
        ) {
            return Err(BellProctorEncounterError::BossSpawnDrift);
        }
        let entity_id = run_qualified_boss_entity_id(run_ordinal)?;
        if entity_id == handoff.player.target.entity_id {
            return Err(BellProctorEncounterError::BossMatchesPlayer);
        }
        let scheduler = BellProctorSimulation::new(definition.clone());
        Ok(Self {
            current_health: parameters.health,
            definition,
            arena,
            entity_id,
            position,
            scheduler,
            player: handoff.player,
            hostile_projectiles: HostileProjectileSimulation::with_allocator(
                handoff.hostile_projectile_ids,
            ),
            active_lanes: Vec::new(),
            starts_at,
            damage_policy: HostileDamagePolicy::Standard,
            defeated: false,
        })
    }

    #[must_use]
    pub const fn entity_id(&self) -> EntityId {
        self.entity_id
    }

    #[must_use]
    pub const fn player(&self) -> &EnemyLabPlayer {
        &self.player
    }

    pub const fn player_mut(&mut self) -> &mut EnemyLabPlayer {
        &mut self.player
    }

    #[must_use]
    pub const fn position(&self) -> SimulationVector {
        self.position
    }

    #[must_use]
    pub fn snapshot(&self) -> BellProctorEncounterSnapshot {
        BellProctorEncounterSnapshot {
            tick: Tick(self.starts_at.0.saturating_add(self.scheduler.tick().0)),
            local_tick: self.scheduler.tick(),
            entity_id: self.entity_id,
            current_health: self.current_health,
            maximum_health: self.definition.parameters().health,
            state: self.scheduler.state(),
            active_projectiles: self.hostile_projectiles.projectiles().len(),
            active_lanes: self.active_lanes.len(),
        }
    }

    pub fn set_damage_policy(&mut self, policy: HostileDamagePolicy) {
        self.damage_policy = policy;
        self.hostile_projectiles.set_damage_policy(policy);
    }

    pub fn update_player_position(
        &mut self,
        position: SimulationVector,
    ) -> Result<(), BellProctorEncounterError> {
        if !position.is_finite() {
            return Err(BellProctorEncounterError::NonFinitePlayerPosition);
        }
        self.player.target.position = position;
        Ok(())
    }

    pub fn hurtbox(&self) -> Result<Option<EnemyHurtbox>, BellProctorEncounterError> {
        if self.defeated {
            return Ok(None);
        }
        EnemyHurtbox::new(
            self.entity_id,
            self.position,
            milli_to_tiles_u32(self.definition.parameters().hurtbox_radius_milli_tiles),
        )
        .map(Some)
        .map_err(BellProctorEncounterError::Hurtbox)
    }

    #[must_use]
    pub fn hostile_projectiles(&self) -> &[HostileProjectile] {
        self.hostile_projectiles.projectiles()
    }

    #[must_use]
    pub fn active_lane_geometries(&self) -> Vec<LaneGeometry> {
        self.active_lanes.iter().map(|lane| lane.geometry).collect()
    }

    pub fn step(
        &mut self,
        combat: &CombatStep,
    ) -> Result<BellProctorEncounterStep, BellProctorEncounterError> {
        let mut staged = self.clone();
        let step = staged.step_inner(combat)?;
        *self = staged;
        Ok(step)
    }

    #[allow(clippy::too_many_lines)] // The transaction stays linear so damage, cleanup, spawn, lane, and projectile order is reviewable.
    fn step_inner(
        &mut self,
        combat: &CombatStep,
    ) -> Result<BellProctorEncounterStep, BellProctorEncounterError> {
        let local_tick = self.scheduler.tick();
        let global_tick = add_tick(self.starts_at, local_tick)?;
        if combat.tick != global_tick {
            return Err(BellProctorEncounterError::CombatTickMismatch {
                expected: global_tick,
                received: combat.tick,
            });
        }
        let (friendly_damage, defeat) = self.apply_friendly_damage(combat)?;
        let status_immunities = combat
            .nail_traps
            .triggers
            .iter()
            .filter(|trigger| trigger.target_id == self.entity_id)
            .map(|trigger| BellProctorStatusImmunityEvent {
                tick: combat.tick,
                source_trap_id: trigger.trap_id,
                target: trigger.target_id,
                status: BellProctorImmuneStatus::Frostbind,
            })
            .collect();
        let aim = aim_from_positions(self.position, self.player.target.position)?;
        let mut scheduler_events = self.scheduler.advance(BossInput {
            current_health: self.current_health,
            target_aim: aim,
        })?;
        let defeated_now = defeat.is_some();

        let must_clear = scheduler_events
            .iter()
            .any(|event| matches!(event, BossEvent::HostileProjectilesCleared { .. }));
        let cleared_hostiles = must_clear.then(|| BellProctorClearedHostiles {
            projectiles: self.hostile_projectiles.clear_projectiles(),
            lane_cast_ids: self
                .active_lanes
                .drain(..)
                .map(|lane| lane.cast_id)
                .collect(),
        });

        let mut hostile_spawn_events = Vec::new();
        for event in &scheduler_events {
            if matches!(
                event,
                BossEvent::FanFired { .. } | BossEvent::RingFired { .. }
            ) {
                hostile_spawn_events.extend(self.hostile_projectiles.spawn_from_boss_event(
                    self.entity_id,
                    self.position,
                    event,
                )?);
            }
            match event {
                BossEvent::CrossActivated { .. } => {
                    let (cast_id, geometry, attack, active_until) =
                        LaneGeometry::from_boss_activation(event)?;
                    self.active_lanes.push(ActiveBossLane {
                        cast_id,
                        geometry: geometry.with_origin(self.position),
                        attack,
                        active_until,
                        contacted_player: false,
                    });
                    self.active_lanes.sort_by_key(|lane| lane.cast_id);
                }
                BossEvent::CrossExpired { cast_id, .. } => {
                    let cast = AttackCastId::from_ordinal(cast_id.get())
                        .ok_or(BellProctorEncounterError::InvalidCastId)?;
                    self.active_lanes.retain(|lane| lane.cast_id != cast);
                }
                _ => {}
            }
        }

        let mut lane_contacts = Vec::new();
        if !defeated_now {
            for lane in &mut self.active_lanes {
                if lane.contacted_player || local_tick >= lane.active_until {
                    continue;
                }
                if let Some(damage) = resolve_lane_contact_with_policy(
                    self.entity_id,
                    &lane.attack,
                    lane.geometry,
                    &mut self.player.target,
                    &mut self.player.consumables,
                    &mut self.player.combat,
                    self.damage_policy,
                )? {
                    lane.contacted_player = true;
                    lane_contacts.push(BellProctorLaneContact {
                        tick: global_tick,
                        cast_id: lane.cast_id,
                        damage,
                    });
                }
            }
        }

        let mut hostile_step = if defeated_now {
            HostileStep {
                tick: local_tick,
                events: Vec::new(),
            }
        } else {
            self.hostile_projectiles.step(
                &self.arena,
                &mut self.player.target,
                &mut self.player.consumables,
                &mut self.player.combat,
            )?
        };
        shift_hostile_step(&mut hostile_step, self.starts_at)?;
        shift_hostile_events(&mut hostile_spawn_events, self.starts_at)?;
        shift_boss_events(&mut scheduler_events, self.starts_at)?;
        self.defeated |= defeated_now;
        Ok(BellProctorEncounterStep {
            tick: global_tick,
            scheduler_events,
            hostile_spawn_events,
            lane_contacts,
            hostile_step,
            friendly_damage,
            status_immunities,
            defeat,
            cleared_hostiles,
        })
    }

    fn apply_friendly_damage(
        &mut self,
        combat: &CombatStep,
    ) -> Result<(Vec<BellProctorDamageEvent>, Option<BellProctorDefeat>), BellProctorEncounterError>
    {
        validate_boss_intents(combat, self.entity_id)?;
        if self.defeated && !combat.raw_damage_intents.is_empty() {
            return Err(BellProctorEncounterError::DamageAfterDefeat);
        }
        let mut events = Vec::new();
        let mut defeat = None;
        for intent in &combat.raw_damage_intents {
            if self.current_health == 0 {
                break;
            }
            let break_multiplier = self.scheduler.received_damage_multiplier_basis_points();
            let resolved_raw_damage =
                multiply_half_up(intent.resolved_raw_damage, break_multiplier)?;
            let request = DirectHitRequest::new(DirectHitParameters {
                source: intent.projectile_id,
                target: self.entity_id,
                collision_confirmed: true,
                target_is_immune: false,
                raw_damage: resolved_raw_damage,
                damage_type: DamageType::Physical,
                attacker_multiplier_basis_points: combat.attacker_multiplier_basis_points,
                target_resistance_basis_points: 0,
                direct_damage_reductions_basis_points: Vec::new(),
                armor: self.definition.parameters().armor,
                current_barrier: 0,
                health_damage_cap_basis_points: None,
                current_health: self.current_health,
                max_health: self.definition.parameters().health,
            })?;
            let damage = resolve_direct_hit(&request)?;
            self.current_health = damage.health_after;
            events.push(BellProctorDamageEvent {
                tick: combat.tick,
                projectile_id: intent.projectile_id,
                contact_ordinal: intent.contact_ordinal,
                source: intent.source,
                base_raw_damage: intent.base_raw_damage,
                authored_multiplier_basis_points: intent.multiplier_basis_points,
                break_multiplier_basis_points: break_multiplier,
                resolved_raw_damage,
                damage: damage.clone(),
            });
            if damage.lethal {
                defeat = Some(BellProctorDefeat {
                    tick: combat.tick,
                    entity_id: self.entity_id,
                    lethal_projectile_id: intent.projectile_id,
                    lethal_contact_ordinal: intent.contact_ordinal,
                });
            }
        }
        Ok((events, defeat))
    }
}

fn validate_boss_intents(
    combat: &CombatStep,
    boss: EntityId,
) -> Result<(), BellProctorEncounterError> {
    let mut previous = None;
    let mut seen = BTreeSet::new();
    for intent in &combat.raw_damage_intents {
        if intent.tick != combat.tick || intent.target != boss {
            return Err(BellProctorEncounterError::InvalidFriendlyIntent);
        }
        let key = (intent.projectile_id, intent.contact_ordinal);
        if previous.is_some_and(|prior| key < prior) || !seen.insert(key) {
            return Err(BellProctorEncounterError::InvalidFriendlyIntentOrder);
        }
        previous = Some(key);
        if intent.source == RawDamageIntentSource::NailTrap {
            let count = combat
                .nail_traps
                .triggers
                .iter()
                .filter(|trigger| {
                    trigger.tick == intent.tick
                        && trigger.trap_id == intent.projectile_id
                        && trigger.target_id == boss
                        && trigger.snapshot_weapon_raw_damage == intent.base_raw_damage
                        && trigger.raw_damage == intent.resolved_raw_damage
                })
                .count();
            if count != 1 {
                return Err(BellProctorEncounterError::InvalidFriendlyIntent);
            }
            continue;
        }
        let source = match intent.source {
            RawDamageIntentSource::Primary => FriendlyProjectileSource::Primary,
            RawDamageIntentSource::GraveMark => FriendlyProjectileSource::GraveMark,
            RawDamageIntentSource::NailTrap => unreachable!("handled above"),
        };
        let count = combat
            .collisions
            .iter()
            .filter(|collision| {
                collision.tick == intent.tick
                    && collision.projectile_id == intent.projectile_id
                    && collision.source == source
                    && collision.contact_ordinal == intent.contact_ordinal
                    && collision.target == CollisionTarget::Enemy(boss)
            })
            .count();
        if count != 1 {
            return Err(BellProctorEncounterError::InvalidFriendlyIntent);
        }
    }
    Ok(())
}

fn run_qualified_boss_entity_id(run_ordinal: u32) -> Result<EntityId, BellProctorEncounterError> {
    let zero_based = run_ordinal
        .checked_sub(1)
        .ok_or(BellProctorEncounterError::ZeroRunOrdinal)?;
    let value = u64::from(zero_based)
        .checked_mul(crate::RUN_ENTITY_ID_STRIDE)
        .and_then(|base| base.checked_add(BELL_PROCTOR_ENTITY_ID_OFFSET))
        .ok_or(BellProctorEncounterError::EntityIdOverflow)?;
    EntityId::new(value).ok_or(BellProctorEncounterError::EntityIdOverflow)
}

fn multiply_half_up(value: u32, basis_points: u32) -> Result<u32, BellProctorEncounterError> {
    let scaled = u64::from(value)
        .checked_mul(u64::from(basis_points))
        .and_then(|value| value.checked_add(5_000))
        .ok_or(BellProctorEncounterError::ArithmeticOverflow)?;
    u32::try_from(scaled / 10_000).map_err(|_| BellProctorEncounterError::ArithmeticOverflow)
}

fn aim_from_positions(
    origin: SimulationVector,
    target: SimulationVector,
) -> Result<AimVector, BellProctorEncounterError> {
    let delta = target - origin;
    if !delta.is_finite() || delta.length_squared() <= f32::EPSILON {
        return Err(BellProctorEncounterError::InvalidAim);
    }
    Ok(AimVector {
        x: tiles_to_milli(delta.x)?,
        y: tiles_to_milli(delta.y)?,
    })
}

fn shift_boss_events(
    events: &mut [BossEvent],
    starts_at: Tick,
) -> Result<(), BellProctorEncounterError> {
    for event in events {
        match event {
            BossEvent::PhaseStarted { tick, .. }
            | BossEvent::PhaseThresholdCrossed { tick, .. }
            | BossEvent::TimelineCancelled { tick, .. }
            | BossEvent::HostileProjectilesCleared { tick }
            | BossEvent::FanFired { tick, .. }
            | BossEvent::RingFired { tick, .. }
            | BossEvent::CrossExpired { tick, .. }
            | BossEvent::LoopRestarted { tick, .. }
            | BossEvent::SoftEnrageStarted { tick }
            | BossEvent::BossDefeated { tick, .. }
            | BossEvent::BreakEnded { tick, .. } => *tick = add_tick(starts_at, *tick)?,
            BossEvent::BreakStarted { tick, ends_at, .. } => {
                *tick = add_tick(starts_at, *tick)?;
                *ends_at = add_tick(starts_at, *ends_at)?;
            }
            BossEvent::FanTelegraph { tick, fires_at, .. }
            | BossEvent::RingTelegraph { tick, fires_at, .. } => {
                *tick = add_tick(starts_at, *tick)?;
                *fires_at = add_tick(starts_at, *fires_at)?;
            }
            BossEvent::RingPreview {
                tick,
                preview_ends_at,
                fires_at,
                ..
            } => {
                *tick = add_tick(starts_at, *tick)?;
                *preview_ends_at = add_tick(starts_at, *preview_ends_at)?;
                *fires_at = add_tick(starts_at, *fires_at)?;
            }
            BossEvent::CrossTelegraph {
                tick, impacts_at, ..
            } => {
                *tick = add_tick(starts_at, *tick)?;
                *impacts_at = add_tick(starts_at, *impacts_at)?;
            }
            BossEvent::CrossActivated {
                tick, active_until, ..
            } => {
                *tick = add_tick(starts_at, *tick)?;
                *active_until = add_tick(starts_at, *active_until)?;
            }
        }
    }
    Ok(())
}

fn shift_hostile_events(
    events: &mut [HostileEvent],
    starts_at: Tick,
) -> Result<(), BellProctorEncounterError> {
    for event in events {
        let tick = match event {
            HostileEvent::Spawned { tick, .. }
            | HostileEvent::Moved { tick, .. }
            | HostileEvent::Contact { tick, .. }
            | HostileEvent::ProjectileGraceIgnored { tick, .. }
            | HostileEvent::Expired { tick, .. } => tick,
        };
        *tick = add_tick(starts_at, *tick)?;
    }
    Ok(())
}

fn shift_hostile_step(
    step: &mut HostileStep,
    starts_at: Tick,
) -> Result<(), BellProctorEncounterError> {
    step.tick = add_tick(starts_at, step.tick)?;
    shift_hostile_events(&mut step.events, starts_at)
}

fn add_tick(start: Tick, local: Tick) -> Result<Tick, BellProctorEncounterError> {
    start
        .0
        .checked_add(local.0)
        .map(Tick)
        .ok_or(BellProctorEncounterError::TickOverflow)
}

#[allow(clippy::cast_precision_loss)]
fn milli_to_tiles(value: i32) -> f32 {
    value as f32 / 1_000.0
}

#[allow(clippy::cast_precision_loss)]
fn milli_to_tiles_u32(value: u32) -> f32 {
    value as f32 / 1_000.0
}

#[allow(clippy::cast_possible_truncation)]
fn tiles_to_milli(value: f32) -> Result<i32, BellProctorEncounterError> {
    let scaled = value * 1_000.0;
    #[allow(clippy::cast_precision_loss)]
    if !scaled.is_finite() || scaled < i32::MIN as f32 || scaled > i32::MAX as f32 {
        return Err(BellProctorEncounterError::InvalidAim);
    }
    Ok(scaled.round() as i32)
}

#[derive(Debug, Error)]
pub enum BellProctorEncounterError {
    #[error("Bell Proctor spawn differs from the arena boss spawn")]
    BossSpawnDrift,
    #[error("run ordinal must be nonzero")]
    ZeroRunOrdinal,
    #[error("run-qualified Bell Proctor entity ID overflow")]
    EntityIdOverflow,
    #[error("Bell Proctor entity ID must differ from the player")]
    BossMatchesPlayer,
    #[error("player position must remain finite")]
    NonFinitePlayerPosition,
    #[error("boss aim is ambiguous or outside fixed-point range")]
    InvalidAim,
    #[error("combat tick {received} differs from expected boss tick {expected}")]
    CombatTickMismatch { expected: Tick, received: Tick },
    #[error("friendly boss intent lacks exact collision provenance")]
    InvalidFriendlyIntent,
    #[error("friendly boss intents are not unique stable projectile/contact order")]
    InvalidFriendlyIntentOrder,
    #[error("friendly damage cannot target an already defeated Bell Proctor")]
    DamageAfterDefeat,
    #[error("boss cast ID must be nonzero")]
    InvalidCastId,
    #[error("boss arithmetic overflow")]
    ArithmeticOverflow,
    #[error("boss/global tick overflow")]
    TickOverflow,
    #[error(transparent)]
    Boss(#[from] crate::BossRuntimeError),
    #[error(transparent)]
    Hostile(#[from] HostileError),
    #[error(transparent)]
    Damage(#[from] DamageError),
    #[error(transparent)]
    Hurtbox(#[from] HurtboxError),
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::*;
    use crate::{
        ArenaAnchor, EntityIdAllocator, GraveMarkDefinition, GraveMarkDefinitionParameters,
        HostileTargetState, NormalWaveHandoff, PlayerCombatState, PlayerVitals,
        ProjectileCollision, RedTonicDefinition, RedTonicSimulation, SlipstepDefinition,
        SlipstepDefinitionParameters, StillnessDefinition, StillnessDefinitionParameters,
        TilePoint, TileRectangle, TonicBelt, WeaponDefinition, WeaponDefinitionParameters,
    };

    fn id(value: u64) -> EntityId {
        EntityId::new(value).expect("nonzero ID")
    }

    fn arena() -> ArenaGeometry {
        ArenaGeometry {
            id: "arena.prototype.bell_laboratory_01".to_owned(),
            width_milli_tiles: 32_000,
            height_milli_tiles: 24_000,
            shell_thickness_milli_tiles: 1_000,
            player_spawn: TilePoint::new(4_000, 12_000),
            boss_spawn: TilePoint::new(24_000, 12_000),
            pillars: vec![
                TileRectangle::new(10_000, 5_000, 2_000, 3_000),
                TileRectangle::new(10_000, 16_000, 2_000, 3_000),
                TileRectangle::new(20_000, 5_000, 2_000, 3_000),
                TileRectangle::new(20_000, 16_000, 2_000, 3_000),
            ],
            anchors: vec![ArenaAnchor {
                id: "C".to_owned(),
                point: TilePoint::new(16_000, 12_000),
            }],
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

    fn handoff() -> NormalWaveHandoff {
        NormalWaveHandoff {
            player: EnemyLabPlayer {
                target: HostileTargetState {
                    entity_id: id(10_004),
                    position: SimulationVector::new(4.0, 12.0),
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
                combat: combat(),
            },
            hostile_projectile_ids: EntityIdAllocator::starting_at(
                NonZeroU64::new(20_000).expect("nonzero"),
            ),
        }
    }

    fn empty(tick: u64) -> CombatStep {
        CombatStep {
            tick: Tick(tick),
            ..CombatStep::default()
        }
    }

    fn damage_step(tick: u64, boss: EntityId, projectile: u64, raw: u32) -> CombatStep {
        let intent = crate::RawDamageIntent {
            tick: Tick(tick),
            projectile_id: id(projectile),
            source: RawDamageIntentSource::Primary,
            target: boss,
            base_raw_damage: raw,
            multiplier_basis_points: 10_000,
            resolved_raw_damage: raw,
            contact_ordinal: 0,
        };
        CombatStep {
            tick: Tick(tick),
            collisions: vec![ProjectileCollision {
                tick: Tick(tick),
                projectile_id: intent.projectile_id,
                source: FriendlyProjectileSource::Primary,
                target: CollisionTarget::Enemy(boss),
                final_position: SimulationVector::new(24.0, 12.0),
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

    fn nail_trap_damage_step(tick: u64, boss: EntityId, trap: u64) -> CombatStep {
        let trigger = crate::NailTrapTrigger {
            trap_id: id(trap),
            target_id: boss,
            tick: Tick(tick),
            position: SimulationVector::new(24.0, 12.0),
            raw_damage: 18,
            snapshot_weapon_raw_damage: 20,
            frostbind_ticks: 45,
        };
        CombatStep {
            tick: Tick(tick),
            raw_damage_intents: vec![crate::RawDamageIntent {
                tick: Tick(tick),
                projectile_id: id(trap),
                source: RawDamageIntentSource::NailTrap,
                target: boss,
                base_raw_damage: 20,
                multiplier_basis_points: 9_000,
                resolved_raw_damage: 18,
                contact_ordinal: 0,
            }],
            nail_traps: crate::NailTrapStep {
                triggers: vec![trigger],
                ..crate::NailTrapStep::default()
            },
            ..CombatStep::default()
        }
    }

    #[test]
    fn nailkeeper_damages_boss_but_frostbind_emits_immune_feedback() {
        let mut simulation = BellProctorEncounterSimulation::new(
            BellProctorDefinition::first_playable(),
            arena(),
            handoff(),
            1,
            Tick(100),
        )
        .unwrap();
        let boss = simulation.entity_id();
        let step = simulation
            .step(&nail_trap_damage_step(100, boss, 60))
            .unwrap();
        assert_eq!(step.friendly_damage.len(), 1);
        assert!(step.friendly_damage[0].damage.health_damage_applied > 0);
        assert_eq!(
            step.status_immunities,
            vec![BellProctorStatusImmunityEvent {
                tick: Tick(100),
                source_trap_id: id(60),
                target: boss,
                status: BellProctorImmuneStatus::Frostbind,
            }]
        );
    }

    #[test]
    fn cinder_attacker_stage_applies_to_boss_direct_hits() {
        let mut simulation = BellProctorEncounterSimulation::new(
            BellProctorDefinition::first_playable(),
            arena(),
            handoff(),
            1,
            Tick(100),
        )
        .unwrap();
        let boss = simulation.entity_id();
        let mut combat = damage_step(100, boss, 61, 100);
        combat.attacker_multiplier_basis_points = 11_800;
        let step = simulation.step(&combat).unwrap();
        assert_eq!(step.friendly_damage.len(), 1);
        assert_eq!(
            step.friendly_damage[0]
                .damage
                .attacker_multiplier_basis_points,
            11_800
        );
        assert_eq!(step.friendly_damage[0].damage.health_damage_applied, 114);
    }

    #[test]
    fn phase_one_fan_spawns_five_real_projectiles_on_global_tick() {
        let mut simulation = BellProctorEncounterSimulation::new(
            BellProctorDefinition::first_playable(),
            arena(),
            handoff(),
            1,
            Tick(100),
        )
        .expect("boss");
        for tick in 100..112 {
            let step = simulation.step(&empty(tick)).expect("advance");
            assert!(step.hostile_spawn_events.is_empty());
        }
        let fired = simulation.step(&empty(112)).expect("fan fires");
        assert_eq!(fired.tick, Tick(112));
        assert_eq!(fired.hostile_spawn_events.len(), 5);
        assert_eq!(simulation.snapshot().active_projectiles, 5);
        let directions: Vec<_> = fired
            .hostile_spawn_events
            .iter()
            .filter_map(|event| match event {
                HostileEvent::Spawned { projectile, .. } => Some(projectile.direction()),
                _ => None,
            })
            .collect();
        assert_eq!(directions.len(), 5);
        assert!(directions.windows(2).all(|pair| pair[0] != pair[1]));

        let mut ring_spawn_count = 0;
        for tick in 113..=288 {
            let step = simulation.step(&empty(tick)).expect("advance to ring");
            ring_spawn_count += step
                .hostile_spawn_events
                .iter()
                .filter(|event| {
                    matches!(
                        event,
                        HostileEvent::Spawned { projectile, .. }
                            if projectile.pattern_id() == crate::BELL_PROCTOR_RING_ID
                    )
                })
                .count();
        }
        assert_eq!(ring_spawn_count, 12);
    }

    #[test]
    fn phase_two_cross_uses_real_lane_once_and_debug_policy_preserves_health() {
        let mut simulation = BellProctorEncounterSimulation::new(
            BellProctorDefinition::first_playable(),
            arena(),
            handoff(),
            1,
            Tick(0),
        )
        .expect("boss");
        simulation.set_damage_policy(HostileDamagePolicy::DebugInvulnerable);
        let boss = simulation.entity_id();
        simulation
            .step(&damage_step(0, boss, 800, 1_000))
            .expect("enter phase two break");
        let health_before = simulation.player().consumables.vitals().current_health();
        let mut cross_contacts = Vec::new();
        let mut cross_activations = 0;
        for tick in 1..=327 {
            let step = simulation.step(&empty(tick)).expect("phase two");
            cross_activations += step
                .scheduler_events
                .iter()
                .filter(|event| matches!(event, BossEvent::CrossActivated { .. }))
                .count();
            cross_contacts.extend(step.lane_contacts);
        }
        assert_eq!(cross_activations, 1);
        assert_eq!(cross_contacts.len(), 1);
        assert!(cross_contacts[0].damage.debug_invulnerable);
        assert_eq!(cross_contacts[0].damage.health_application.applied, 0);
        assert_eq!(
            simulation.player().consumables.vitals().current_health(),
            health_before
        );
    }

    #[test]
    fn threshold_clears_real_hostiles_and_break_multiplier_applies() {
        let mut simulation = BellProctorEncounterSimulation::new(
            BellProctorDefinition::first_playable(),
            arena(),
            handoff(),
            1,
            Tick(0),
        )
        .expect("boss");
        for tick in 0..=12 {
            simulation.step(&empty(tick)).expect("fan setup");
        }
        assert_eq!(simulation.snapshot().active_projectiles, 5);
        let boss = simulation.entity_id();
        let threshold = simulation
            .step(&damage_step(13, boss, 900, 1_000))
            .expect("threshold");
        assert_eq!(
            simulation.snapshot().state,
            BellProctorStateKind::Break {
                entering: crate::BellProctorPhase::Phase2
            }
        );
        assert_eq!(
            threshold
                .cleared_hostiles
                .expect("cleanup")
                .projectiles
                .len(),
            5
        );
        assert_eq!(simulation.snapshot().active_projectiles, 0);

        let during_break = simulation
            .step(&damage_step(14, boss, 901, 100))
            .expect("break damage");
        assert_eq!(
            during_break.friendly_damage[0].break_multiplier_basis_points,
            12_000
        );
        assert_eq!(during_break.friendly_damage[0].resolved_raw_damage, 120);
        assert_eq!(
            during_break.friendly_damage[0].damage.health_damage_applied,
            116
        );
    }

    #[test]
    fn lethal_friendly_damage_clears_and_defeats_once_without_hostile_step() {
        let mut simulation = BellProctorEncounterSimulation::new(
            BellProctorDefinition::first_playable(),
            arena(),
            handoff(),
            2,
            Tick(500),
        )
        .expect("boss");
        let boss = simulation.entity_id();
        assert_eq!(boss.get(), 140_001);
        let lethal = simulation
            .step(&damage_step(500, boss, 999, 4_000))
            .expect("lethal");
        assert!(lethal.defeat.is_some());
        assert!(matches!(
            lethal.scheduler_events.as_slice(),
            [
                BossEvent::HostileProjectilesCleared { tick: Tick(500) },
                BossEvent::BossDefeated {
                    tick: Tick(500),
                    ..
                }
            ]
        ));
        assert!(lethal.hostile_step.events.is_empty());
        assert!(simulation.hurtbox().expect("hurtbox").is_none());
        assert!(simulation.step(&damage_step(501, boss, 1_000, 20)).is_err());
    }
}
