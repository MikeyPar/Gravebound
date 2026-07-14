//! Dedicated Sepulcher Knight movement and immutable attack authority.
//!
//! `SPEC-CONFLICT-019` fixes charge tick ownership, swept contact, solid truncation, and the
//! target-opposite ring pair. Health, player damage, projectile allocation, and room lifecycle
//! remain in their existing shared owners.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::{
    ArenaGeometry, AttackCastId, CollisionError, CollisionTarget, CoreEnemyDefinition,
    CoreEnemyKitError, CoreEnemyKitEvent, CoreEnemyKitKind, CoreEnemyKitScheduler,
    CoreEnemyLocomotionDefinition, CoreSelectedTarget, CoreTargetCandidate,
    CoreTargetSelectionError, CoreWorldPosition, EntityId, ProjectileCollisionWorld,
    SimulationVector, SolidColliderId, Tick, select_core_target,
};

const TICKS_PER_SECOND: i64 = 30;
const CHARGE_PATTERN_INDEX: usize = 0;
const STOP_RING_PATTERN_INDEX: usize = 1;
const SHIELD_FAN_PATTERN_INDEX: usize = 2;
const CHARGE_LENGTH_MILLI_TILES: i64 = 5_000;
const CHARGE_TICKS: u32 = 17;
const PLAYER_HURTBOX_RADIUS_TILES: f32 = 0.25;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreKnightAttackLock {
    cast_id: AttackCastId,
    pattern_index: usize,
    pattern_id: String,
    origin: CoreWorldPosition,
    target: CoreSelectedTarget,
    telegraph_started_at: Tick,
    resolves_at: Tick,
}

impl CoreKnightAttackLock {
    #[must_use]
    pub const fn cast_id(&self) -> AttackCastId {
        self.cast_id
    }

    #[must_use]
    pub const fn pattern_index(&self) -> usize {
        self.pattern_index
    }

    #[must_use]
    pub fn pattern_id(&self) -> &str {
        &self.pattern_id
    }

    #[must_use]
    pub const fn origin(&self) -> CoreWorldPosition {
        self.origin
    }

    #[must_use]
    pub const fn target(&self) -> CoreSelectedTarget {
        self.target
    }

    #[must_use]
    pub const fn telegraph_started_at(&self) -> Tick {
        self.telegraph_started_at
    }

    #[must_use]
    pub const fn resolves_at(&self) -> Tick {
        self.resolves_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreKnightEvent {
    TelegraphStarted {
        lock: CoreKnightAttackLock,
        first_use: bool,
    },
    ChargeStarted {
        tick: Tick,
        lock: CoreKnightAttackLock,
    },
    ChargeMoved {
        tick: Tick,
        cast_id: AttackCastId,
        segment_index: u8,
        from: CoreWorldPosition,
        to: CoreWorldPosition,
        blocked_by: Option<SolidColliderId>,
        contacts: Vec<EntityId>,
    },
    StopRingReleased {
        tick: Tick,
        lock: CoreKnightAttackLock,
        origin: CoreWorldPosition,
        emitted_indices: [u8; 8],
        omitted_indices: [u8; 2],
    },
    ShieldFanReleased {
        tick: Tick,
        lock: CoreKnightAttackLock,
    },
    TargetlessReset {
        tick: Tick,
        restored_position: CoreWorldPosition,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreKnightStep {
    pub tick: Tick,
    pub selected_target: Option<CoreSelectedTarget>,
    pub from: CoreWorldPosition,
    pub to: CoreWorldPosition,
    pub positioned_for_attack: bool,
    pub kit_events: Vec<CoreEnemyKitEvent>,
    pub events: Vec<CoreKnightEvent>,
}

#[derive(Debug, Clone)]
struct ActiveCharge {
    lock: CoreKnightAttackLock,
    nominal_endpoint: CoreWorldPosition,
    next_segment: u32,
    blocked_by: Option<SolidColliderId>,
    contacted_players: BTreeSet<EntityId>,
}

#[derive(Debug, Clone)]
struct CompletedCharge {
    lock: CoreKnightAttackLock,
    endpoint: CoreWorldPosition,
}

/// Owns one Knight's fixed-point body and exact multi-pattern schedule.
#[derive(Debug, Clone)]
pub struct CoreKnightSimulation {
    definition: CoreEnemyDefinition,
    scheduler: CoreEnemyKitScheduler,
    entity_id: EntityId,
    authored_spawn: CoreWorldPosition,
    acquire_home: CoreWorldPosition,
    position: CoreWorldPosition,
    tick: Tick,
    next_cast_ordinal: u64,
    pending_lock: Option<CoreKnightAttackLock>,
    active_charge: Option<ActiveCharge>,
    completed_charge: Option<CompletedCharge>,
    pursue_x_remainder: i64,
    pursue_y_remainder: i64,
    targetless_ticks: u32,
}

impl CoreKnightSimulation {
    pub fn new(
        definition: CoreEnemyDefinition,
        entity_id: EntityId,
        authored_spawn: CoreWorldPosition,
    ) -> Result<Self, CoreKnightError> {
        let scheduler = CoreEnemyKitScheduler::new(definition.clone())?;
        if scheduler.kind() != CoreEnemyKitKind::SepulcherKnight
            || !matches!(
                definition.locomotion(),
                CoreEnemyLocomotionDefinition::PursueStopChargeHome {
                    movement_speed_milli_tiles_per_second: 2_400,
                    stop_distance_milli_tiles: 3_500,
                }
            )
            || definition.parameters().collision_radius_milli_tiles != 550
            || definition.parameters().hurtbox_radius_milli_tiles != 480
        {
            return Err(CoreKnightError::DefinitionDrift);
        }
        Ok(Self {
            definition,
            scheduler,
            entity_id,
            authored_spawn,
            acquire_home: authored_spawn,
            position: authored_spawn,
            tick: Tick(0),
            next_cast_ordinal: 1,
            pending_lock: None,
            active_charge: None,
            completed_charge: None,
            pursue_x_remainder: 0,
            pursue_y_remainder: 0,
            targetless_ticks: 0,
        })
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    #[must_use]
    pub const fn entity_id(&self) -> EntityId {
        self.entity_id
    }

    #[must_use]
    pub const fn position(&self) -> CoreWorldPosition {
        self.position
    }

    #[must_use]
    pub const fn authored_spawn(&self) -> CoreWorldPosition {
        self.authored_spawn
    }

    #[must_use]
    pub const fn acquire_home(&self) -> CoreWorldPosition {
        self.acquire_home
    }

    #[must_use]
    pub const fn definition(&self) -> &CoreEnemyDefinition {
        &self.definition
    }

    pub fn advance(
        &mut self,
        arena: &ArenaGeometry,
        candidates: &[CoreTargetCandidate],
        attacks_enabled: bool,
    ) -> Result<CoreKnightStep, CoreKnightError> {
        let mut staged = self.clone();
        let step = staged.advance_inner(arena, candidates, attacks_enabled)?;
        *self = staged;
        Ok(step)
    }

    /// Restores the authored spawn and first-use schedule without rewinding time or identities.
    pub fn reset(&mut self) -> Result<(), CoreKnightError> {
        self.scheduler.reset()?;
        self.acquire_home = self.authored_spawn;
        self.position = self.authored_spawn;
        self.pending_lock = None;
        self.active_charge = None;
        self.completed_charge = None;
        self.pursue_x_remainder = 0;
        self.pursue_y_remainder = 0;
        self.targetless_ticks = 0;
        Ok(())
    }

    fn advance_inner(
        &mut self,
        arena: &ArenaGeometry,
        candidates: &[CoreTargetCandidate],
        attacks_enabled: bool,
    ) -> Result<CoreKnightStep, CoreKnightError> {
        if self.scheduler.tick() != self.tick {
            return Err(CoreKnightError::SchedulerTickMismatch);
        }
        let from = self.position;
        let selected_target = select_core_target(
            self.position,
            self.definition.parameters().aggro_radius_milli_tiles,
            candidates,
        )?;
        if selected_target.is_some() {
            self.targetless_ticks = 0;
        } else {
            self.targetless_ticks = self
                .targetless_ticks
                .checked_add(1)
                .ok_or(CoreKnightError::TickOverflow)?;
        }

        let mut positioned_for_attack = false;
        if self.active_charge.is_none() && self.pending_lock.is_none() {
            positioned_for_attack = self.pursue(arena, selected_target)?;
        }
        let kit_events = self
            .scheduler
            .advance(attacks_enabled && positioned_for_attack && selected_target.is_some())?;
        let mut events = Vec::with_capacity(kit_events.len() + 1);
        for event in &kit_events {
            self.consume_kit_event(selected_target, event, &mut events)?;
        }
        if self.active_charge.is_some() {
            let movement = self.advance_charge_segment(arena, candidates)?;
            events.push(movement);
        }

        if self.targetless_ticks >= self.definition.no_target_reset_ticks() {
            self.reset()?;
            events.push(CoreKnightEvent::TargetlessReset {
                tick: self.tick,
                restored_position: self.position,
            });
        }
        let step = CoreKnightStep {
            tick: self.tick,
            selected_target,
            from,
            to: self.position,
            positioned_for_attack,
            kit_events,
            events,
        };
        self.tick = self
            .tick
            .checked_next()
            .ok_or(CoreKnightError::TickOverflow)?;
        Ok(step)
    }

    fn consume_kit_event(
        &mut self,
        selected_target: Option<CoreSelectedTarget>,
        event: &CoreEnemyKitEvent,
        output: &mut Vec<CoreKnightEvent>,
    ) -> Result<(), CoreKnightError> {
        match event {
            CoreEnemyKitEvent::TelegraphDue {
                tick,
                pattern_index,
                warning_ticks,
                first_use,
            } => {
                self.require_tick(*tick)?;
                if self.pending_lock.is_some() {
                    return Err(CoreKnightError::OverlappingTelegraph);
                }
                let target = selected_target.ok_or(CoreKnightError::MissingTarget)?;
                let lock = self.new_lock(*pattern_index, target, *tick, *warning_ticks)?;
                self.pending_lock = Some(lock.clone());
                output.push(CoreKnightEvent::TelegraphStarted {
                    lock,
                    first_use: *first_use,
                });
            }
            CoreEnemyKitEvent::KnightChargeDue {
                tick,
                pattern_index,
                charge_ticks,
            } => {
                if *charge_ticks != CHARGE_TICKS {
                    return Err(CoreKnightError::DefinitionDrift);
                }
                let lock = self.take_lock(*tick, *pattern_index)?;
                let nominal_endpoint = charge_endpoint(lock.origin, lock.target.position)?;
                self.active_charge = Some(ActiveCharge {
                    lock: lock.clone(),
                    nominal_endpoint,
                    next_segment: 0,
                    blocked_by: None,
                    contacted_players: BTreeSet::new(),
                });
                output.push(CoreKnightEvent::ChargeStarted { tick: *tick, lock });
            }
            CoreEnemyKitEvent::KnightStopRingDue {
                tick,
                pattern_index,
            } => {
                self.require_tick(*tick)?;
                if *pattern_index != STOP_RING_PATTERN_INDEX || self.active_charge.is_some() {
                    return Err(CoreKnightError::ChargeBoundaryMismatch);
                }
                let completed = self
                    .completed_charge
                    .take()
                    .ok_or(CoreKnightError::MissingCompletedCharge)?;
                let (emitted_indices, omitted_indices) = ring_indices(&completed)?;
                output.push(CoreKnightEvent::StopRingReleased {
                    tick: *tick,
                    lock: completed.lock,
                    origin: completed.endpoint,
                    emitted_indices,
                    omitted_indices,
                });
            }
            CoreEnemyKitEvent::KnightShieldFanDue {
                tick,
                pattern_index,
            } => {
                let lock = self.take_lock(*tick, *pattern_index)?;
                output.push(CoreKnightEvent::ShieldFanReleased { tick: *tick, lock });
            }
            CoreEnemyKitEvent::MireChargeDue { .. }
            | CoreEnemyKitEvent::MireRetreatDue { .. }
            | CoreEnemyKitEvent::AcolyteFanDue { .. }
            | CoreEnemyKitEvent::RotorStarted { .. }
            | CoreEnemyKitEvent::RotorVolleyDue { .. }
            | CoreEnemyKitEvent::RotorRecoveryStarted { .. }
            | CoreEnemyKitEvent::RecoveryWarningDue { .. }
            | CoreEnemyKitEvent::DirectionalGapPreviewDue { .. }
            | CoreEnemyKitEvent::AbbotRecoveryRingDue { .. } => {
                return Err(CoreKnightError::DefinitionDrift);
            }
        }
        Ok(())
    }

    fn new_lock(
        &mut self,
        pattern_index: usize,
        target: CoreSelectedTarget,
        telegraph_started_at: Tick,
        warning_ticks: u32,
    ) -> Result<CoreKnightAttackLock, CoreKnightError> {
        if !matches!(
            pattern_index,
            CHARGE_PATTERN_INDEX | SHIELD_FAN_PATTERN_INDEX
        ) {
            return Err(CoreKnightError::InvalidPatternIndex);
        }
        let pattern = self
            .definition
            .parameters()
            .patterns
            .get(pattern_index)
            .ok_or(CoreKnightError::InvalidPatternIndex)?;
        let cast_id = AttackCastId::from_ordinal(self.next_cast_ordinal)
            .ok_or(CoreKnightError::CastIdOverflow)?;
        self.next_cast_ordinal = self
            .next_cast_ordinal
            .checked_add(1)
            .ok_or(CoreKnightError::CastIdOverflow)?;
        let resolves_at = add_ticks(telegraph_started_at, warning_ticks)?;
        Ok(CoreKnightAttackLock {
            cast_id,
            pattern_index,
            pattern_id: pattern.parameters().id.clone(),
            origin: self.position,
            target,
            telegraph_started_at,
            resolves_at,
        })
    }

    fn take_lock(
        &mut self,
        tick: Tick,
        pattern_index: usize,
    ) -> Result<CoreKnightAttackLock, CoreKnightError> {
        self.require_tick(tick)?;
        let lock = self
            .pending_lock
            .take()
            .ok_or(CoreKnightError::ReleaseWithoutTelegraph)?;
        if lock.pattern_index != pattern_index || lock.resolves_at != tick {
            return Err(CoreKnightError::ReleaseBoundaryMismatch);
        }
        Ok(lock)
    }

    fn pursue(
        &mut self,
        arena: &ArenaGeometry,
        target: Option<CoreSelectedTarget>,
    ) -> Result<bool, CoreKnightError> {
        let Some(target) = target else {
            return Ok(false);
        };
        let delta = position_delta(self.position, target.position);
        let distance = i64::try_from(integer_sqrt(squared_length(delta)?))
            .map_err(|_| CoreKnightError::ArithmeticOverflow)?;
        if distance <= 3_500 {
            return Ok(true);
        }
        let maximum_step = distance - 3_500;
        let (dx, dy) = normalized_tick_step(
            delta,
            2_400,
            maximum_step,
            &mut self.pursue_x_remainder,
            &mut self.pursue_y_remainder,
        )?;
        let desired = add_position(self.position, dx, dy)?;
        let (position, _) = sweep_to(
            arena,
            self.position,
            desired,
            self.definition.parameters().collision_radius_milli_tiles,
        )?;
        self.position = position;
        Ok(integer_sqrt(squared_length(position_delta(
            self.position,
            target.position,
        ))?) <= 3_500)
    }

    fn advance_charge_segment(
        &mut self,
        arena: &ArenaGeometry,
        candidates: &[CoreTargetCandidate],
    ) -> Result<CoreKnightEvent, CoreKnightError> {
        let mut charge = self
            .active_charge
            .take()
            .ok_or(CoreKnightError::MissingActiveCharge)?;
        if charge.next_segment >= CHARGE_TICKS {
            return Err(CoreKnightError::ChargeBoundaryMismatch);
        }
        let from = self.position;
        let desired = interpolate_charge(
            charge.lock.origin,
            charge.nominal_endpoint,
            charge.next_segment + 1,
        )?;
        let (to, blocked) = if charge.blocked_by.is_some() {
            (from, charge.blocked_by)
        } else {
            sweep_to(
                arena,
                from,
                desired,
                self.definition.parameters().collision_radius_milli_tiles,
            )?
        };
        charge.blocked_by = charge.blocked_by.or(blocked);
        let contacts = charge_contacts(
            arena,
            from,
            to,
            candidates,
            self.definition.parameters().collision_radius_milli_tiles,
            &mut charge.contacted_players,
        )?;
        self.position = to;
        let segment_index =
            u8::try_from(charge.next_segment).map_err(|_| CoreKnightError::DefinitionDrift)?;
        charge.next_segment += 1;
        let cast_id = charge.lock.cast_id;
        if charge.next_segment == CHARGE_TICKS {
            self.acquire_home = self.position;
            self.completed_charge = Some(CompletedCharge {
                lock: charge.lock,
                endpoint: self.position,
            });
        } else {
            self.active_charge = Some(charge);
        }
        Ok(CoreKnightEvent::ChargeMoved {
            tick: self.tick,
            cast_id,
            segment_index,
            from,
            to,
            blocked_by: blocked,
            contacts,
        })
    }

    fn require_tick(&self, tick: Tick) -> Result<(), CoreKnightError> {
        if tick == self.tick {
            Ok(())
        } else {
            Err(CoreKnightError::KitEventTickMismatch)
        }
    }
}

fn charge_endpoint(
    origin: CoreWorldPosition,
    target: CoreWorldPosition,
) -> Result<CoreWorldPosition, CoreKnightError> {
    let delta = position_delta(origin, target);
    let length = i64::try_from(integer_sqrt(squared_length(delta)?))
        .map_err(|_| CoreKnightError::ArithmeticOverflow)?;
    if length == 0 {
        return Err(CoreKnightError::CoincidentTarget);
    }
    add_position(
        origin,
        divide_round_nearest(delta.0 * CHARGE_LENGTH_MILLI_TILES, length)?,
        divide_round_nearest(delta.1 * CHARGE_LENGTH_MILLI_TILES, length)?,
    )
}

fn interpolate_charge(
    origin: CoreWorldPosition,
    endpoint: CoreWorldPosition,
    completed_segments: u32,
) -> Result<CoreWorldPosition, CoreKnightError> {
    let delta = position_delta(origin, endpoint);
    add_position(
        origin,
        divide_round_nearest(
            delta.0 * i64::from(completed_segments),
            i64::from(CHARGE_TICKS),
        )?,
        divide_round_nearest(
            delta.1 * i64::from(completed_segments),
            i64::from(CHARGE_TICKS),
        )?,
    )
}

fn ring_indices(completed: &CompletedCharge) -> Result<([u8; 8], [u8; 2]), CoreKnightError> {
    // Pair midpoints at 18° + 36°i, clockwise in northwest-origin coordinates, scaled 1e6.
    const MIDPOINTS: [(i64, i64); 10] = [
        (951_057, 309_017),
        (587_785, 809_017),
        (0, 1_000_000),
        (-587_785, 809_017),
        (-951_057, 309_017),
        (-951_057, -309_017),
        (-587_785, -809_017),
        (0, -1_000_000),
        (587_785, -809_017),
        (951_057, -309_017),
    ];
    let mut desired = position_delta(completed.lock.target.position, completed.endpoint);
    if desired == (0, 0) {
        desired = position_delta(completed.lock.origin, completed.endpoint);
    }
    if desired == (0, 0) {
        return Err(CoreKnightError::CoincidentTarget);
    }
    let omitted_start = MIDPOINTS
        .iter()
        .enumerate()
        .max_by_key(|(index, basis)| {
            let dot = desired.0 * basis.0 + desired.1 * basis.1;
            (dot, std::cmp::Reverse(*index))
        })
        .map(|(index, _)| u8::try_from(index).expect("ten ring pairs fit u8"))
        .ok_or(CoreKnightError::DefinitionDrift)?;
    let omitted = [omitted_start, (omitted_start + 1) % 10];
    let emitted = (0_u8..10)
        .filter(|index| !omitted.contains(index))
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| CoreKnightError::DefinitionDrift)?;
    Ok((emitted, omitted))
}

fn charge_contacts(
    arena: &ArenaGeometry,
    from: CoreWorldPosition,
    to: CoreWorldPosition,
    candidates: &[CoreTargetCandidate],
    radius_milli_tiles: u32,
    already_contacted: &mut BTreeSet<EntityId>,
) -> Result<Vec<EntityId>, CoreKnightError> {
    let start = world_to_vector(from);
    let displacement = world_to_vector(to) - start;
    let mut contacts = Vec::new();
    let mut ordered = candidates.to_vec();
    ordered.sort_by_key(|candidate| candidate.entity_id);
    for candidate in ordered {
        if !candidate.living
            || !candidate.damageable
            || already_contacted.contains(&candidate.entity_id)
        {
            continue;
        }
        let player = crate::EnemyHurtbox::new(
            candidate.entity_id,
            world_to_vector(candidate.position),
            PLAYER_HURTBOX_RADIUS_TILES,
        )?;
        let world = ProjectileCollisionWorld::new(arena, vec![player])?;
        if world
            .sweep_circle(start, displacement, milli_radius(radius_milli_tiles))?
            .is_some_and(|hit| hit.target == CollisionTarget::Enemy(candidate.entity_id))
        {
            already_contacted.insert(candidate.entity_id);
            contacts.push(candidate.entity_id);
        }
    }
    Ok(contacts)
}

fn sweep_to(
    arena: &ArenaGeometry,
    from: CoreWorldPosition,
    desired: CoreWorldPosition,
    radius_milli_tiles: u32,
) -> Result<(CoreWorldPosition, Option<SolidColliderId>), CoreKnightError> {
    let start = world_to_vector(from);
    let displacement = world_to_vector(desired) - start;
    let world = ProjectileCollisionWorld::new(arena, Vec::new())?;
    let hit = world.sweep_solids(start, displacement, milli_radius(radius_milli_tiles))?;
    let fraction = hit.map_or(1.0, |contact| contact.fraction);
    let delta = position_delta(from, desired);
    let mut dx = scale_by_fraction(delta.0, fraction)?;
    let mut dy = scale_by_fraction(delta.1, fraction)?;
    if hit.is_some() && (dx != 0 || dy != 0) {
        dx = dx.saturating_sub(delta.0.signum());
        dy = dy.saturating_sub(delta.1.signum());
    }
    let position = add_position(from, dx, dy)?;
    Ok((
        position,
        hit.and_then(|contact| match contact.target {
            CollisionTarget::Solid(id) => Some(id),
            CollisionTarget::Enemy(_) => None,
        }),
    ))
}

fn normalized_tick_step(
    direction: (i64, i64),
    speed: u32,
    maximum_step: i64,
    remainder_x: &mut i64,
    remainder_y: &mut i64,
) -> Result<(i64, i64), CoreKnightError> {
    let length = i64::try_from(integer_sqrt(squared_length(direction)?))
        .map_err(|_| CoreKnightError::ArithmeticOverflow)?;
    if length == 0 {
        return Ok((0, 0));
    }
    let denominator = length
        .checked_mul(TICKS_PER_SECOND)
        .ok_or(CoreKnightError::ArithmeticOverflow)?;
    *remainder_x = remainder_x
        .checked_add(
            direction
                .0
                .checked_mul(i64::from(speed))
                .ok_or(CoreKnightError::ArithmeticOverflow)?,
        )
        .ok_or(CoreKnightError::ArithmeticOverflow)?;
    *remainder_y = remainder_y
        .checked_add(
            direction
                .1
                .checked_mul(i64::from(speed))
                .ok_or(CoreKnightError::ArithmeticOverflow)?,
        )
        .ok_or(CoreKnightError::ArithmeticOverflow)?;
    let mut dx = *remainder_x / denominator;
    let mut dy = *remainder_y / denominator;
    *remainder_x %= denominator;
    *remainder_y %= denominator;
    let planned = i64::try_from(integer_sqrt(squared_length((dx, dy))?))
        .map_err(|_| CoreKnightError::ArithmeticOverflow)?;
    if planned > maximum_step && planned > 0 {
        dx = divide_round_nearest(dx * maximum_step, planned)?;
        dy = divide_round_nearest(dy * maximum_step, planned)?;
    }
    Ok((dx, dy))
}

fn position_delta(from: CoreWorldPosition, to: CoreWorldPosition) -> (i64, i64) {
    (
        i64::from(to.x_milli_tiles) - i64::from(from.x_milli_tiles),
        i64::from(to.y_milli_tiles) - i64::from(from.y_milli_tiles),
    )
}

fn add_position(
    position: CoreWorldPosition,
    dx: i64,
    dy: i64,
) -> Result<CoreWorldPosition, CoreKnightError> {
    Ok(CoreWorldPosition::new(
        i32::try_from(
            i64::from(position.x_milli_tiles)
                .checked_add(dx)
                .ok_or(CoreKnightError::ArithmeticOverflow)?,
        )
        .map_err(|_| CoreKnightError::ArithmeticOverflow)?,
        i32::try_from(
            i64::from(position.y_milli_tiles)
                .checked_add(dy)
                .ok_or(CoreKnightError::ArithmeticOverflow)?,
        )
        .map_err(|_| CoreKnightError::ArithmeticOverflow)?,
    ))
}

fn squared_length(vector: (i64, i64)) -> Result<u64, CoreKnightError> {
    vector
        .0
        .unsigned_abs()
        .checked_mul(vector.0.unsigned_abs())
        .and_then(|x| {
            vector
                .1
                .unsigned_abs()
                .checked_mul(vector.1.unsigned_abs())
                .and_then(|y| x.checked_add(y))
        })
        .ok_or(CoreKnightError::ArithmeticOverflow)
}

fn integer_sqrt(value: u64) -> u64 {
    if value < 2 {
        return value;
    }
    let mut x = value;
    let mut next = x.midpoint(value / x);
    while next < x {
        x = next;
        next = x.midpoint(value / x);
    }
    x
}

fn divide_round_nearest(numerator: i64, denominator: i64) -> Result<i64, CoreKnightError> {
    if denominator <= 0 {
        return Err(CoreKnightError::ArithmeticOverflow);
    }
    let half = denominator / 2;
    numerator
        .checked_add(half * numerator.signum())
        .map(|value| value / denominator)
        .ok_or(CoreKnightError::ArithmeticOverflow)
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "authored room displacements are tightly bounded and collision returns f32 fractions"
)]
fn scale_by_fraction(value: i64, fraction: f32) -> Result<i64, CoreKnightError> {
    let scaled = value as f64 * f64::from(fraction);
    if !scaled.is_finite() || scaled < i64::MIN as f64 || scaled > i64::MAX as f64 {
        return Err(CoreKnightError::ArithmeticOverflow);
    }
    Ok(scaled.round() as i64)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "authored room positions are tightly bounded milli-tiles"
)]
fn world_to_vector(position: CoreWorldPosition) -> SimulationVector {
    SimulationVector::new(
        position.x_milli_tiles as f32 / 1_000.0,
        position.y_milli_tiles as f32 / 1_000.0,
    )
}

#[allow(
    clippy::cast_precision_loss,
    reason = "authored collision radii are tightly bounded milli-tiles"
)]
fn milli_radius(radius_milli_tiles: u32) -> f32 {
    radius_milli_tiles as f32 / 1_000.0
}

fn add_ticks(tick: Tick, amount: u32) -> Result<Tick, CoreKnightError> {
    tick.0
        .checked_add(u64::from(amount))
        .map(Tick)
        .ok_or(CoreKnightError::TickOverflow)
}

#[derive(Debug, Error)]
pub enum CoreKnightError {
    #[error("Sepulcher Knight definition drifted from its exact authored contract")]
    DefinitionDrift,
    #[error("Knight scheduler tick diverged from its body owner")]
    SchedulerTickMismatch,
    #[error("Knight scheduler emitted an event at the wrong tick")]
    KitEventTickMismatch,
    #[error("Knight target-relative mechanic requires a legal target")]
    MissingTarget,
    #[error("Knight target is coincident with the charge origin")]
    CoincidentTarget,
    #[error("Knight started an overlapping telegraph")]
    OverlappingTelegraph,
    #[error("Knight released without an immutable telegraph lock")]
    ReleaseWithoutTelegraph,
    #[error("Knight release did not match its warning boundary")]
    ReleaseBoundaryMismatch,
    #[error("Knight charge end did not match its parent ring boundary")]
    ChargeBoundaryMismatch,
    #[error("Knight charge movement has no active cast")]
    MissingActiveCharge,
    #[error("Knight stop ring has no completed parent charge")]
    MissingCompletedCharge,
    #[error("Knight pattern index is invalid")]
    InvalidPatternIndex,
    #[error("Knight cast identity overflowed")]
    CastIdOverflow,
    #[error("Knight tick arithmetic overflowed")]
    TickOverflow,
    #[error("Knight fixed-point arithmetic overflowed")]
    ArithmeticOverflow,
    #[error(transparent)]
    Kit(#[from] CoreEnemyKitError),
    #[error(transparent)]
    Target(#[from] CoreTargetSelectionError),
    #[error(transparent)]
    Collision(#[from] CollisionError),
    #[error(transparent)]
    Hurtbox(#[from] crate::HurtboxError),
}
