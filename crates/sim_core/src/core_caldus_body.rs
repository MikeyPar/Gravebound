//! Fixed-point Sir Caldus body authority.
//!
//! The canonical GDD `ENC-010`, content spec `CONT-BOSS-001`/`002`, roadmap `GB-M03-03`,
//! and approved `SPEC-CONFLICT-022` define this seam. Pattern scheduling remains in
//! `core_caldus`; this module alone owns charge displacement, swept contact, solid truncation,
//! the realized Stop Ring origin/gap, and the two-tiles-per-second center return.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::{
    ArenaGeometry, CollisionError, CollisionTarget, CoreBossParticipant, CoreBossParticipantLock,
    CoreCaldusEvent, CoreCaldusProjectileRelease, CoreWorldPosition, EnemyHurtbox, EntityId,
    HurtboxError, PLAYER_HURTBOX_RADIUS_TILES, ProjectileCollisionWorld, SimulationVector,
    SolidColliderId, Tick,
};

const TICKS_PER_SECOND: i64 = 30;
const CALDUS_COLLISION_RADIUS_MILLI_TILES: u32 = 700;
const CHARGE_LENGTH_MILLI_TILES: i64 = 6_500;
const CHARGE_SEGMENTS: u8 = 17;
const CENTER_RETURN_SPEED_MILLI_TILES_PER_SECOND: i64 = 2_000;
const CENTER_STOP_RADIUS_MILLI_TILES: u64 = 250;
const ARENA_CENTER: CoreWorldPosition = CoreWorldPosition::new(9_000, 9_000);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreCaldusBodyTarget {
    pub participant: CoreBossParticipant,
    pub position: CoreWorldPosition,
    pub living: bool,
    pub damageable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreCaldusBodyEvent {
    ChargeLocked {
        tick: Tick,
        cast_id: u64,
        origin: CoreWorldPosition,
        target: CoreBossParticipant,
        target_position: CoreWorldPosition,
        nominal_endpoint: CoreWorldPosition,
    },
    ChargeMoved {
        tick: Tick,
        cast_id: u64,
        segment_index: u8,
        from: CoreWorldPosition,
        to: CoreWorldPosition,
        blocked_by: Option<SolidColliderId>,
        contacts: Vec<CoreBossParticipant>,
    },
    ChargeStopRingReleased {
        tick: Tick,
        cast_id: u64,
        origin: CoreWorldPosition,
        omitted_start_index: u8,
    },
    CenterReturnMoved {
        tick: Tick,
        from: CoreWorldPosition,
        to: CoreWorldPosition,
        blocked_by: Option<SolidColliderId>,
    },
}

impl CoreCaldusBodyEvent {
    #[must_use]
    pub fn projectile_release(&self) -> Option<CoreCaldusProjectileRelease> {
        match self {
            Self::ChargeStopRingReleased {
                tick,
                cast_id,
                origin,
                omitted_start_index,
            } => Some(CoreCaldusProjectileRelease::ChargeStopRing {
                tick: *tick,
                cast_id: *cast_id,
                origin: world_to_vector(*origin),
                omitted_start_index: *omitted_start_index,
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LockedCharge {
    cast_id: u64,
    origin: CoreWorldPosition,
    target: CoreBossParticipant,
    target_position: CoreWorldPosition,
    nominal_endpoint: CoreWorldPosition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveCharge {
    lock: LockedCharge,
    next_segment: u8,
    blocked_by: Option<SolidColliderId>,
    contacted: BTreeSet<EntityId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletedCharge {
    cast_id: u64,
    endpoint: CoreWorldPosition,
    omitted_start_index: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCaldusBodySimulation {
    lock: CoreBossParticipantLock,
    tick: Tick,
    position: CoreWorldPosition,
    locked_charge: Option<LockedCharge>,
    active_charge: Option<ActiveCharge>,
    completed_charge: Option<CompletedCharge>,
    return_x_remainder: i64,
    return_y_remainder: i64,
}

impl CoreCaldusBodySimulation {
    pub fn new(lock: CoreBossParticipantLock) -> Result<Self, CoreCaldusBodyError> {
        if lock.participants.is_empty() {
            return Err(CoreCaldusBodyError::EmptyParticipantLock);
        }
        Ok(Self {
            lock,
            tick: Tick(0),
            position: ARENA_CENTER,
            locked_charge: None,
            active_charge: None,
            completed_charge: None,
            return_x_remainder: 0,
            return_y_remainder: 0,
        })
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    #[must_use]
    pub const fn position(&self) -> CoreWorldPosition {
        self.position
    }

    #[must_use]
    pub fn simulation_position(&self) -> SimulationVector {
        world_to_vector(self.position)
    }

    pub fn advance(
        &mut self,
        arena: &ArenaGeometry,
        scheduler_events: &[CoreCaldusEvent],
        targets: &[CoreCaldusBodyTarget],
    ) -> Result<Vec<CoreCaldusBodyEvent>, CoreCaldusBodyError> {
        let mut staged = self.clone();
        let events = staged.advance_inner(arena, scheduler_events, targets)?;
        *self = staged;
        Ok(events)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "scheduler event ordering and clone-stage-commit mutations remain explicit"
    )]
    fn advance_inner(
        &mut self,
        arena: &ArenaGeometry,
        scheduler_events: &[CoreCaldusEvent],
        targets: &[CoreCaldusBodyTarget],
    ) -> Result<Vec<CoreCaldusBodyEvent>, CoreCaldusBodyError> {
        validate_targets(&self.lock, targets)?;
        if scheduler_events
            .iter()
            .filter_map(body_authority_tick)
            .any(|tick| tick != self.tick)
        {
            return Err(CoreCaldusBodyError::SchedulerTickMismatch);
        }
        let mut output = Vec::new();
        let mut suppress_return = false;
        for event in scheduler_events {
            match event {
                CoreCaldusEvent::ChargeDirectionLocked {
                    cast_id,
                    target,
                    target_x_milli_tiles,
                    target_y_milli_tiles,
                    ..
                } => {
                    if self.locked_charge.is_some()
                        || self.active_charge.is_some()
                        || self.completed_charge.is_some()
                    {
                        return Err(CoreCaldusBodyError::OverlappingCharge);
                    }
                    let target_position =
                        CoreWorldPosition::new(*target_x_milli_tiles, *target_y_milli_tiles);
                    let nominal_endpoint = charge_endpoint(self.position, target_position)?;
                    let lock = LockedCharge {
                        cast_id: *cast_id,
                        origin: self.position,
                        target: *target,
                        target_position,
                        nominal_endpoint,
                    };
                    output.push(CoreCaldusBodyEvent::ChargeLocked {
                        tick: self.tick,
                        cast_id: *cast_id,
                        origin: self.position,
                        target: *target,
                        target_position,
                        nominal_endpoint,
                    });
                    self.locked_charge = Some(lock);
                    suppress_return = true;
                }
                CoreCaldusEvent::ChargeMovementStarted { cast_id, .. } => {
                    let lock = self
                        .locked_charge
                        .take()
                        .ok_or(CoreCaldusBodyError::MissingLockedCharge)?;
                    if lock.cast_id != *cast_id {
                        return Err(CoreCaldusBodyError::ChargeCastMismatch);
                    }
                    self.active_charge = Some(ActiveCharge {
                        lock,
                        next_segment: 0,
                        blocked_by: None,
                        contacted: BTreeSet::new(),
                    });
                    suppress_return = true;
                }
                CoreCaldusEvent::ChargeEnded { cast_id, .. } => {
                    let completed = self
                        .completed_charge
                        .take()
                        .ok_or(CoreCaldusBodyError::MissingCompletedCharge)?;
                    if completed.cast_id != *cast_id {
                        return Err(CoreCaldusBodyError::ChargeCastMismatch);
                    }
                    output.push(CoreCaldusBodyEvent::ChargeStopRingReleased {
                        tick: self.tick,
                        cast_id: *cast_id,
                        origin: completed.endpoint,
                        omitted_start_index: completed.omitted_start_index,
                    });
                    suppress_return = true;
                }
                CoreCaldusEvent::PhaseTimelineCancelled { .. }
                | CoreCaldusEvent::BossDefeated { .. } => {
                    self.locked_charge = None;
                    self.active_charge = None;
                    self.completed_charge = None;
                    if matches!(event, CoreCaldusEvent::BossDefeated { .. }) {
                        suppress_return = true;
                    }
                }
                _ => {}
            }
        }
        if self.active_charge.is_some() {
            output.push(self.advance_charge_segment(arena, targets)?);
            suppress_return = true;
        }
        if self.locked_charge.is_some() || self.completed_charge.is_some() {
            suppress_return = true;
        }
        if !suppress_return {
            self.advance_center_return(arena, &mut output)?;
        }
        self.tick = Tick(
            self.tick
                .0
                .checked_add(1)
                .ok_or(CoreCaldusBodyError::ArithmeticOverflow)?,
        );
        Ok(output)
    }

    fn advance_charge_segment(
        &mut self,
        arena: &ArenaGeometry,
        targets: &[CoreCaldusBodyTarget],
    ) -> Result<CoreCaldusBodyEvent, CoreCaldusBodyError> {
        let mut charge = self
            .active_charge
            .take()
            .ok_or(CoreCaldusBodyError::MissingActiveCharge)?;
        let from = self.position;
        let desired = interpolate_charge(
            charge.lock.origin,
            charge.lock.nominal_endpoint,
            charge.next_segment + 1,
        )?;
        let (to, blocked_by) = if charge.blocked_by.is_some() {
            (from, charge.blocked_by)
        } else {
            sweep_to(arena, from, desired)?
        };
        charge.blocked_by = charge.blocked_by.or(blocked_by);
        let contacts = charge_contacts(arena, from, to, targets, &mut charge.contacted)?;
        self.position = to;
        let event = CoreCaldusBodyEvent::ChargeMoved {
            tick: self.tick,
            cast_id: charge.lock.cast_id,
            segment_index: charge.next_segment,
            from,
            to,
            blocked_by,
            contacts,
        };
        charge.next_segment = charge
            .next_segment
            .checked_add(1)
            .ok_or(CoreCaldusBodyError::ArithmeticOverflow)?;
        if charge.next_segment == CHARGE_SEGMENTS {
            self.completed_charge = Some(CompletedCharge {
                cast_id: charge.lock.cast_id,
                endpoint: self.position,
                omitted_start_index: opposite_gap_start(charge.lock.origin, self.position)?,
            });
        } else {
            self.active_charge = Some(charge);
        }
        Ok(event)
    }

    fn advance_center_return(
        &mut self,
        arena: &ArenaGeometry,
        output: &mut Vec<CoreCaldusBodyEvent>,
    ) -> Result<(), CoreCaldusBodyError> {
        let delta = position_delta(self.position, ARENA_CENTER);
        let distance = integer_sqrt(squared_length(delta)?);
        if distance <= CENTER_STOP_RADIUS_MILLI_TILES {
            return Ok(());
        }
        let maximum_step = i64::try_from(distance - CENTER_STOP_RADIUS_MILLI_TILES)
            .map_err(|_| CoreCaldusBodyError::ArithmeticOverflow)?;
        let (dx, dy) = normalized_tick_step(
            delta,
            maximum_step,
            &mut self.return_x_remainder,
            &mut self.return_y_remainder,
        )?;
        let desired = add_position(self.position, dx, dy)?;
        let from = self.position;
        let (to, blocked_by) = sweep_to(arena, from, desired)?;
        self.position = to;
        output.push(CoreCaldusBodyEvent::CenterReturnMoved {
            tick: self.tick,
            from,
            to,
            blocked_by,
        });
        Ok(())
    }
}

fn validate_targets(
    lock: &CoreBossParticipantLock,
    targets: &[CoreCaldusBodyTarget],
) -> Result<(), CoreCaldusBodyError> {
    let mut slots = BTreeSet::new();
    let mut entities = BTreeSet::new();
    for target in targets {
        if !lock.participants.contains(&target.participant) {
            return Err(CoreCaldusBodyError::TargetOutsideLock);
        }
        if !slots.insert(target.participant.party_slot)
            || !entities.insert(target.participant.entity_id)
        {
            return Err(CoreCaldusBodyError::DuplicateTarget);
        }
    }
    Ok(())
}

fn charge_endpoint(
    origin: CoreWorldPosition,
    target: CoreWorldPosition,
) -> Result<CoreWorldPosition, CoreCaldusBodyError> {
    let (dx, dy) = position_delta(origin, target);
    if dx == 0 && dy == 0 {
        return Err(CoreCaldusBodyError::CoincidentChargeTarget);
    }
    let (step_x, step_y, limit) = if dx.unsigned_abs() >= dy.unsigned_abs() {
        if dx >= 0 {
            (CHARGE_LENGTH_MILLI_TILES, 0, 17_000)
        } else {
            (-CHARGE_LENGTH_MILLI_TILES, 0, 1_000)
        }
    } else if dy >= 0 {
        (0, CHARGE_LENGTH_MILLI_TILES, 17_000)
    } else {
        (0, -CHARGE_LENGTH_MILLI_TILES, 1_000)
    };
    let candidate = add_position(origin, step_x, step_y)?;
    Ok(if step_x != 0 {
        CoreWorldPosition::new(
            if step_x > 0 {
                candidate.x_milli_tiles.min(limit)
            } else {
                candidate.x_milli_tiles.max(limit)
            },
            origin.y_milli_tiles,
        )
    } else {
        CoreWorldPosition::new(
            origin.x_milli_tiles,
            if step_y > 0 {
                candidate.y_milli_tiles.min(limit)
            } else {
                candidate.y_milli_tiles.max(limit)
            },
        )
    })
}

fn interpolate_charge(
    origin: CoreWorldPosition,
    endpoint: CoreWorldPosition,
    completed_segments: u8,
) -> Result<CoreWorldPosition, CoreCaldusBodyError> {
    let (dx, dy) = position_delta(origin, endpoint);
    add_position(
        origin,
        divide_round_nearest(
            dx * i64::from(completed_segments),
            i64::from(CHARGE_SEGMENTS),
        )?,
        divide_round_nearest(
            dy * i64::from(completed_segments),
            i64::from(CHARGE_SEGMENTS),
        )?,
    )
}

fn opposite_gap_start(
    origin: CoreWorldPosition,
    endpoint: CoreWorldPosition,
) -> Result<u8, CoreCaldusBodyError> {
    // Adjacent-pair midpoints are 12.857° + 25.714°i clockwise; values are scaled by 1e6.
    const MIDPOINTS: [(i64, i64); 14] = [
        (974_928, 222_521),
        (781_831, 623_490),
        (433_884, 900_969),
        (0, 1_000_000),
        (-433_884, 900_969),
        (-781_831, 623_490),
        (-974_928, 222_521),
        (-974_928, -222_521),
        (-781_831, -623_490),
        (-433_884, -900_969),
        (0, -1_000_000),
        (433_884, -900_969),
        (781_831, -623_490),
        (974_928, -222_521),
    ];
    let direction = position_delta(origin, endpoint);
    if direction == (0, 0) {
        return Err(CoreCaldusBodyError::CoincidentChargeTarget);
    }
    MIDPOINTS
        .iter()
        .enumerate()
        .min_by_key(|(index, basis)| {
            let dot = direction.0 * basis.0 + direction.1 * basis.1;
            (dot, *index)
        })
        .map(|(index, _)| u8::try_from(index).expect("fourteen indices fit u8"))
        .ok_or(CoreCaldusBodyError::ArithmeticOverflow)
}

fn charge_contacts(
    arena: &ArenaGeometry,
    from: CoreWorldPosition,
    to: CoreWorldPosition,
    targets: &[CoreCaldusBodyTarget],
    contacted: &mut BTreeSet<EntityId>,
) -> Result<Vec<CoreBossParticipant>, CoreCaldusBodyError> {
    let start = world_to_vector(from);
    let displacement = world_to_vector(to) - start;
    let mut hits = Vec::new();
    for target in targets {
        if !target.living || !target.damageable || contacted.contains(&target.participant.entity_id)
        {
            continue;
        }
        let hurtbox = EnemyHurtbox::new(
            target.participant.entity_id,
            world_to_vector(target.position),
            PLAYER_HURTBOX_RADIUS_TILES,
        )?;
        let world = ProjectileCollisionWorld::new(arena, vec![hurtbox])?;
        if let Some(hit) = world.sweep_circle(
            start,
            displacement,
            milli_radius(CALDUS_COLLISION_RADIUS_MILLI_TILES),
        )? && hit.target == CollisionTarget::Enemy(target.participant.entity_id)
        {
            hits.push((hit.fraction, target.participant));
        }
    }
    hits.sort_by(|left, right| {
        left.0
            .total_cmp(&right.0)
            .then_with(|| left.1.party_slot.cmp(&right.1.party_slot))
            .then_with(|| left.1.entity_id.cmp(&right.1.entity_id))
    });
    for (_, participant) in &hits {
        contacted.insert(participant.entity_id);
    }
    Ok(hits
        .into_iter()
        .map(|(_, participant)| participant)
        .collect())
}

fn sweep_to(
    arena: &ArenaGeometry,
    from: CoreWorldPosition,
    desired: CoreWorldPosition,
) -> Result<(CoreWorldPosition, Option<SolidColliderId>), CoreCaldusBodyError> {
    let start = world_to_vector(from);
    let displacement = world_to_vector(desired) - start;
    let world = ProjectileCollisionWorld::new(arena, Vec::new())?;
    let hit = world.sweep_solids(
        start,
        displacement,
        milli_radius(CALDUS_COLLISION_RADIUS_MILLI_TILES),
    )?;
    let fraction = hit.map_or(1.0, |contact| contact.fraction);
    let delta = position_delta(from, desired);
    let mut dx = scale_by_fraction(delta.0, fraction)?;
    let mut dy = scale_by_fraction(delta.1, fraction)?;
    if hit.is_some() && (dx != 0 || dy != 0) {
        dx = dx.saturating_sub(delta.0.signum());
        dy = dy.saturating_sub(delta.1.signum());
    }
    Ok((
        add_position(from, dx, dy)?,
        hit.and_then(|contact| match contact.target {
            CollisionTarget::Solid(id) => Some(id),
            CollisionTarget::Enemy(_) => None,
        }),
    ))
}

fn normalized_tick_step(
    direction: (i64, i64),
    maximum_step: i64,
    remainder_x: &mut i64,
    remainder_y: &mut i64,
) -> Result<(i64, i64), CoreCaldusBodyError> {
    let length = i64::try_from(integer_sqrt(squared_length(direction)?))
        .map_err(|_| CoreCaldusBodyError::ArithmeticOverflow)?;
    if length == 0 {
        return Ok((0, 0));
    }
    let denominator = length
        .checked_mul(TICKS_PER_SECOND)
        .ok_or(CoreCaldusBodyError::ArithmeticOverflow)?;
    *remainder_x = remainder_x
        .checked_add(direction.0 * CENTER_RETURN_SPEED_MILLI_TILES_PER_SECOND)
        .ok_or(CoreCaldusBodyError::ArithmeticOverflow)?;
    *remainder_y = remainder_y
        .checked_add(direction.1 * CENTER_RETURN_SPEED_MILLI_TILES_PER_SECOND)
        .ok_or(CoreCaldusBodyError::ArithmeticOverflow)?;
    let mut dx = *remainder_x / denominator;
    let mut dy = *remainder_y / denominator;
    *remainder_x %= denominator;
    *remainder_y %= denominator;
    let step_length = i64::try_from(integer_sqrt(squared_length((dx, dy))?))
        .map_err(|_| CoreCaldusBodyError::ArithmeticOverflow)?;
    if step_length > maximum_step && step_length > 0 {
        dx = divide_round_nearest(dx * maximum_step, step_length)?;
        dy = divide_round_nearest(dy * maximum_step, step_length)?;
    }
    Ok((dx, dy))
}

fn body_authority_tick(event: &CoreCaldusEvent) -> Option<Tick> {
    match event {
        CoreCaldusEvent::PhaseTimelineCancelled { tick, .. }
        | CoreCaldusEvent::ChargeDirectionLocked { tick, .. }
        | CoreCaldusEvent::ChargeMovementStarted { tick, .. }
        | CoreCaldusEvent::ChargeEnded { tick, .. }
        | CoreCaldusEvent::BossDefeated { tick } => Some(*tick),
        _ => None,
    }
}

const fn position_delta(from: CoreWorldPosition, to: CoreWorldPosition) -> (i64, i64) {
    (
        to.x_milli_tiles as i64 - from.x_milli_tiles as i64,
        to.y_milli_tiles as i64 - from.y_milli_tiles as i64,
    )
}

fn add_position(
    position: CoreWorldPosition,
    dx: i64,
    dy: i64,
) -> Result<CoreWorldPosition, CoreCaldusBodyError> {
    Ok(CoreWorldPosition::new(
        i32::try_from(i64::from(position.x_milli_tiles) + dx)
            .map_err(|_| CoreCaldusBodyError::ArithmeticOverflow)?,
        i32::try_from(i64::from(position.y_milli_tiles) + dy)
            .map_err(|_| CoreCaldusBodyError::ArithmeticOverflow)?,
    ))
}

fn squared_length(value: (i64, i64)) -> Result<u128, CoreCaldusBodyError> {
    let x = i128::from(value.0);
    let y = i128::from(value.1);
    x.checked_mul(x)
        .and_then(|x_squared| {
            y.checked_mul(y)
                .and_then(|y_squared| x_squared.checked_add(y_squared))
        })
        .map(i128::unsigned_abs)
        .ok_or(CoreCaldusBodyError::ArithmeticOverflow)
}

fn integer_sqrt(value: u128) -> u64 {
    if value == 0 {
        return 0;
    }
    let mut low = 1_u128;
    let mut high = value.min(u128::from(u64::MAX));
    while low <= high {
        let middle = low + (high - low) / 2;
        if middle <= value / middle {
            low = middle + 1;
        } else {
            high = middle - 1;
        }
    }
    u64::try_from(high).expect("bounded by u64 maximum")
}

fn divide_round_nearest(numerator: i64, denominator: i64) -> Result<i64, CoreCaldusBodyError> {
    if denominator <= 0 {
        return Err(CoreCaldusBodyError::ArithmeticOverflow);
    }
    let half = denominator / 2;
    numerator
        .checked_add(numerator.signum() * half)
        .ok_or(CoreCaldusBodyError::ArithmeticOverflow)
        .map(|value| value / denominator)
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "authored arena displacements are tightly bounded and collision returns f32 fractions"
)]
fn scale_by_fraction(value: i64, fraction: f32) -> Result<i64, CoreCaldusBodyError> {
    let scaled = value as f64 * f64::from(fraction);
    if !scaled.is_finite() || scaled < i64::MIN as f64 || scaled > i64::MAX as f64 {
        return Err(CoreCaldusBodyError::ArithmeticOverflow);
    }
    Ok(scaled.round() as i64)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "authored arena positions are tightly bounded milli-tiles"
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

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CoreCaldusBodyError {
    #[error("Caldus body requires a nonempty participant lock")]
    EmptyParticipantLock,
    #[error("Caldus body target is outside the immutable participant lock")]
    TargetOutsideLock,
    #[error("Caldus body input repeats a participant slot or entity")]
    DuplicateTarget,
    #[error("Caldus body received an event for another authoritative tick")]
    SchedulerTickMismatch,
    #[error("Caldus charge target is coincident with the body")]
    CoincidentChargeTarget,
    #[error("Caldus charge overlapped another charge lifecycle")]
    OverlappingCharge,
    #[error("Caldus charge movement started without a direction lock")]
    MissingLockedCharge,
    #[error("Caldus charge movement has no active cast")]
    MissingActiveCharge,
    #[error("Caldus charge ended before all movement segments completed")]
    MissingCompletedCharge,
    #[error("Caldus charge event cast identity changed")]
    ChargeCastMismatch,
    #[error("Caldus body arithmetic overflowed")]
    ArithmeticOverflow,
    #[error(transparent)]
    Collision(#[from] CollisionError),
    #[error(transparent)]
    Hurtbox(#[from] HurtboxError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArenaAnchor, TilePoint, TileRectangle};

    fn participant(entity: u64, slot: u8) -> CoreBossParticipant {
        CoreBossParticipant {
            entity_id: EntityId::new(entity).expect("entity"),
            party_slot: slot,
        }
    }

    fn lock() -> CoreBossParticipantLock {
        CoreBossParticipantLock {
            attempt_ordinal: 1,
            participants: vec![participant(10, 1), participant(20, 0)],
            maximum_health: 12_384,
        }
    }

    fn arena(pillars: Vec<TileRectangle>) -> ArenaGeometry {
        ArenaGeometry {
            id: "arena.boss.caldus_01".to_owned(),
            width_milli_tiles: 18_000,
            height_milli_tiles: 18_000,
            shell_thickness_milli_tiles: 1_000,
            player_spawn: TilePoint::new(2_000, 9_000),
            boss_spawn: TilePoint::new(9_000, 9_000),
            pillars,
            anchors: vec![ArenaAnchor {
                id: "stage".to_owned(),
                point: TilePoint::new(9_000, 9_000),
            }],
        }
        .validated()
        .expect("arena")
    }

    fn targets() -> Vec<CoreCaldusBodyTarget> {
        vec![
            CoreCaldusBodyTarget {
                participant: participant(10, 1),
                position: CoreWorldPosition::new(11_000, 9_000),
                living: true,
                damageable: true,
            },
            CoreCaldusBodyTarget {
                participant: participant(20, 0),
                position: CoreWorldPosition::new(10_000, 9_000),
                living: true,
                damageable: true,
            },
        ]
    }

    fn scheduler_events(tick: u64) -> Vec<CoreCaldusEvent> {
        match tick {
            21 => vec![CoreCaldusEvent::ChargeDirectionLocked {
                tick: Tick(tick),
                cast_id: 7,
                target: participant(10, 1),
                target_x_milli_tiles: 14_000,
                target_y_milli_tiles: 9_000,
            }],
            30 => vec![CoreCaldusEvent::ChargeMovementStarted {
                tick: Tick(tick),
                cast_id: 7,
            }],
            47 => vec![
                CoreCaldusEvent::ChargeEnded {
                    tick: Tick(tick),
                    cast_id: 7,
                },
                CoreCaldusEvent::ChargeStopRingFired {
                    tick: Tick(tick),
                    cast_id: 7,
                },
            ],
            _ => Vec::new(),
        }
    }

    #[test]
    fn charge_has_seventeen_segments_one_ordered_contact_each_and_realized_ring() {
        let mut body = CoreCaldusBodySimulation::new(lock()).expect("body");
        let mut all = Vec::new();
        for tick in 0..=47 {
            all.extend(
                body.advance(&arena(Vec::new()), &scheduler_events(tick), &targets())
                    .expect("advance"),
            );
        }
        let moved = all
            .iter()
            .filter(|event| matches!(event, CoreCaldusBodyEvent::ChargeMoved { .. }))
            .count();
        assert_eq!(moved, 17);
        let contacts = all
            .iter()
            .filter_map(|event| match event {
                CoreCaldusBodyEvent::ChargeMoved { contacts, .. } => Some(contacts),
                _ => None,
            })
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(contacts, [participant(20, 0), participant(10, 1)]);
        assert_eq!(body.position(), CoreWorldPosition::new(15_500, 9_000));
        let release = all
            .iter()
            .find_map(CoreCaldusBodyEvent::projectile_release)
            .expect("stop ring");
        assert!(matches!(
            release,
            CoreCaldusProjectileRelease::ChargeStopRing {
                tick: Tick(47),
                origin,
                omitted_start_index: 6,
                ..
            } if origin == SimulationVector::new(15.5, 9.0)
        ));
    }

    #[test]
    fn solid_truncates_charge_and_body_returns_toward_center_after_ring() {
        let mut body = CoreCaldusBodySimulation::new(lock()).expect("body");
        let arena = arena(vec![TileRectangle::new(12_000, 8_000, 1_000, 2_000)]);
        let mut blocked = None;
        for tick in 0..=47 {
            let events = body
                .advance(&arena, &scheduler_events(tick), &targets())
                .expect("advance");
            blocked = blocked.or_else(|| {
                events.iter().find_map(|event| match event {
                    CoreCaldusBodyEvent::ChargeMoved {
                        blocked_by: Some(collider),
                        to,
                        ..
                    } => Some((*collider, *to)),
                    _ => None,
                })
            });
        }
        let (collider, endpoint) = blocked.expect("solid truncation");
        assert_eq!(collider, SolidColliderId::Pillar(0));
        assert!(endpoint.x_milli_tiles < 11_300);
        let before_return = body.position();
        let returned = body.advance(&arena, &[], &targets()).expect("return");
        assert!(matches!(
            returned.as_slice(),
            [CoreCaldusBodyEvent::CenterReturnMoved { from, to, .. }]
                if *from == before_return && to.x_milli_tiles < from.x_milli_tiles
        ));
    }

    #[test]
    fn invalid_tick_and_overlap_inputs_roll_back() {
        let mut body = CoreCaldusBodySimulation::new(lock()).expect("body");
        let before = body.clone();
        let error = body
            .advance(
                &arena(Vec::new()),
                &[CoreCaldusEvent::ChargeMovementStarted {
                    tick: Tick(1),
                    cast_id: 7,
                }],
                &targets(),
            )
            .expect_err("wrong tick");
        assert_eq!(error, CoreCaldusBodyError::SchedulerTickMismatch);
        assert_eq!(body, before);
    }
}
