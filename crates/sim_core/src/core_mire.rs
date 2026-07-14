//! Collision-aware Mire Leech body owner built over the shared normal attack lock scheduler.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::{
    ArenaGeometry, AttackCastId, CollisionError, CollisionTarget, CoreEnemyDefinition,
    CoreEnemyLocomotionDefinition, CoreNormalAttackError, CoreNormalAttackEvent,
    CoreNormalAttackKind, CoreNormalAttackLock, CoreNormalAttackSimulation, CoreSelectedTarget,
    CoreTargetCandidate, CoreTargetSelectionError, CoreWorldPosition, EnemyHurtbox, EntityId,
    HurtboxError, ProjectileCollisionWorld, SimulationVector, SolidColliderId, Tick,
    select_core_target,
};

const PLAYER_RADIUS_TILES: f32 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreMireMovementPhase {
    Approach,
    Charge,
    Retreat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreMireEvent {
    Movement {
        tick: Tick,
        phase: CoreMireMovementPhase,
        from: CoreWorldPosition,
        to: CoreWorldPosition,
        blocked_by: Option<SolidColliderId>,
    },
    ChargeContact {
        tick: Tick,
        cast_id: AttackCastId,
        target: EntityId,
    },
    TargetlessReset {
        tick: Tick,
        restored_position: CoreWorldPosition,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreMireStep {
    pub tick: Tick,
    pub selected_target: Option<CoreSelectedTarget>,
    pub positioned_for_attack: bool,
    pub attack_events: Vec<CoreNormalAttackEvent>,
    pub events: Vec<CoreMireEvent>,
}

#[derive(Debug, Clone)]
struct ChargeMotion {
    lock: CoreNormalAttackLock,
    endpoint: CoreWorldPosition,
    next_segment: u32,
    blocked_by: Option<SolidColliderId>,
    contacted: BTreeSet<EntityId>,
}

#[derive(Debug, Clone)]
struct RetreatMotion {
    start: CoreWorldPosition,
    endpoint: CoreWorldPosition,
    next_segment: u32,
    blocked_by: Option<SolidColliderId>,
}

#[derive(Debug, Clone)]
pub struct CoreMireSimulation {
    definition: CoreEnemyDefinition,
    entity_id: EntityId,
    authored_spawn: CoreWorldPosition,
    position: CoreWorldPosition,
    attacks: CoreNormalAttackSimulation,
    charge: Option<ChargeMotion>,
    completed_charge: Option<(CoreNormalAttackLock, CoreWorldPosition)>,
    retreat: Option<RetreatMotion>,
    approach_x_remainder: i64,
    approach_y_remainder: i64,
    targetless_ticks: u32,
    tick: Tick,
}

impl CoreMireSimulation {
    pub fn new(
        definition: CoreEnemyDefinition,
        entity_id: EntityId,
        authored_spawn: CoreWorldPosition,
    ) -> Result<Self, CoreMireError> {
        if !matches!(
            definition.locomotion(),
            CoreEnemyLocomotionDefinition::RushRetreat {
                approach_speed_milli_tiles_per_second: 3_000,
                trigger_distance_milli_tiles: 2_500,
                charge_distance_milli_tiles: 2_000,
                charge_ticks: 15,
                retreat_speed_milli_tiles_per_second: 3_500,
                retreat_ticks: 45,
            }
        ) || definition.parameters().collision_radius_milli_tiles != 350
            || definition.parameters().hurtbox_radius_milli_tiles != 300
        {
            return Err(CoreMireError::DefinitionDrift);
        }
        Ok(Self {
            attacks: CoreNormalAttackSimulation::new(definition.clone())?,
            definition,
            entity_id,
            authored_spawn,
            position: authored_spawn,
            charge: None,
            completed_charge: None,
            retreat: None,
            approach_x_remainder: 0,
            approach_y_remainder: 0,
            targetless_ticks: 0,
            tick: Tick(0),
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
    pub const fn definition(&self) -> &CoreEnemyDefinition {
        &self.definition
    }

    pub fn advance(
        &mut self,
        arena: &ArenaGeometry,
        candidates: &[CoreTargetCandidate],
        active: bool,
    ) -> Result<CoreMireStep, CoreMireError> {
        let mut staged = self.clone();
        let step = staged.advance_inner(arena, candidates, active)?;
        *self = staged;
        Ok(step)
    }

    pub fn reset(&mut self) -> Result<(), CoreMireError> {
        self.attacks.reset()?;
        self.position = self.authored_spawn;
        self.charge = None;
        self.completed_charge = None;
        self.retreat = None;
        self.approach_x_remainder = 0;
        self.approach_y_remainder = 0;
        self.targetless_ticks = 0;
        Ok(())
    }

    fn advance_inner(
        &mut self,
        arena: &ArenaGeometry,
        candidates: &[CoreTargetCandidate],
        active: bool,
    ) -> Result<CoreMireStep, CoreMireError> {
        if self.attacks.tick() != self.tick {
            return Err(CoreMireError::TickMismatch);
        }
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
                .ok_or(CoreMireError::TickOverflow)?;
        }
        let mut events = Vec::new();
        let mut positioned = false;
        if active
            && self.charge.is_none()
            && self.retreat.is_none()
            && self.attacks.pending_lock().is_none()
        {
            positioned = self.approach(arena, selected_target, &mut events)?;
        }
        let attack = self
            .attacks
            .advance(self.position, candidates, active && positioned)?;
        for event in &attack.attack_events {
            match event {
                CoreNormalAttackEvent::Released {
                    kind: CoreNormalAttackKind::MireCharge,
                    lock,
                    ..
                } => {
                    let endpoint = directed_endpoint(
                        lock.origin(),
                        lock.target().ok_or(CoreMireError::MissingTarget)?.position,
                        2_000,
                    )?;
                    self.charge = Some(ChargeMotion {
                        lock: lock.clone(),
                        endpoint,
                        next_segment: 0,
                        blocked_by: None,
                        contacted: BTreeSet::new(),
                    });
                }
                CoreNormalAttackEvent::MireRetreatStarted {
                    duration_ticks: 45,
                    speed_milli_tiles_per_second: 3_500,
                    ..
                } => {
                    let (lock, realized) = self
                        .completed_charge
                        .take()
                        .ok_or(CoreMireError::MissingCompletedCharge)?;
                    let target = lock.target().ok_or(CoreMireError::MissingTarget)?.position;
                    let endpoint = retreat_endpoint(realized, target, lock.origin())?;
                    self.retreat = Some(RetreatMotion {
                        start: realized,
                        endpoint,
                        next_segment: 0,
                        blocked_by: None,
                    });
                }
                CoreNormalAttackEvent::TelegraphStarted { .. } => {}
                _ => return Err(CoreMireError::DefinitionDrift),
            }
        }
        if self.charge.is_some() {
            self.advance_charge(arena, candidates, &mut events)?;
        }
        if self.retreat.is_some() {
            self.advance_retreat(arena, &mut events)?;
        }
        if self.targetless_ticks >= self.definition.no_target_reset_ticks() {
            self.reset()?;
            events.push(CoreMireEvent::TargetlessReset {
                tick: self.tick,
                restored_position: self.position,
            });
        }
        let step = CoreMireStep {
            tick: self.tick,
            selected_target,
            positioned_for_attack: positioned,
            attack_events: attack.attack_events,
            events,
        };
        self.tick = self
            .tick
            .checked_next()
            .ok_or(CoreMireError::TickOverflow)?;
        Ok(step)
    }

    fn approach(
        &mut self,
        arena: &ArenaGeometry,
        target: Option<CoreSelectedTarget>,
        events: &mut Vec<CoreMireEvent>,
    ) -> Result<bool, CoreMireError> {
        let Some(target) = target else {
            return Ok(false);
        };
        let d = delta(self.position, target.position);
        let distance = length(d)?;
        if distance <= 2_500 {
            return Ok(true);
        }
        let (dx, dy) = normalized_step(
            d,
            3_000,
            distance - 2_500,
            &mut self.approach_x_remainder,
            &mut self.approach_y_remainder,
        )?;
        let from = self.position;
        let desired = add(from, dx, dy)?;
        let (to, blocked) = sweep(arena, from, desired, 350)?;
        self.position = to;
        events.push(CoreMireEvent::Movement {
            tick: self.tick,
            phase: CoreMireMovementPhase::Approach,
            from,
            to,
            blocked_by: blocked,
        });
        Ok(length(delta(to, target.position))? <= 2_500)
    }

    fn advance_charge(
        &mut self,
        arena: &ArenaGeometry,
        candidates: &[CoreTargetCandidate],
        events: &mut Vec<CoreMireEvent>,
    ) -> Result<(), CoreMireError> {
        let mut motion = self.charge.take().ok_or(CoreMireError::MissingCharge)?;
        let from = self.position;
        let desired = interpolate(
            motion.lock.origin(),
            motion.endpoint,
            motion.next_segment + 1,
            15,
        )?;
        let (to, blocked) = if motion.blocked_by.is_some() {
            (from, motion.blocked_by)
        } else {
            sweep(arena, from, desired, 350)?
        };
        motion.blocked_by = motion.blocked_by.or(blocked);
        for target in contacts(arena, from, to, candidates, &mut motion.contacted)? {
            events.push(CoreMireEvent::ChargeContact {
                tick: self.tick,
                cast_id: motion.lock.cast_id(),
                target,
            });
        }
        self.position = to;
        events.push(CoreMireEvent::Movement {
            tick: self.tick,
            phase: CoreMireMovementPhase::Charge,
            from,
            to,
            blocked_by: blocked,
        });
        motion.next_segment += 1;
        if motion.next_segment == 15 {
            self.completed_charge = Some((motion.lock, to));
        } else {
            self.charge = Some(motion);
        }
        Ok(())
    }

    fn advance_retreat(
        &mut self,
        arena: &ArenaGeometry,
        events: &mut Vec<CoreMireEvent>,
    ) -> Result<(), CoreMireError> {
        let mut motion = self.retreat.take().ok_or(CoreMireError::MissingRetreat)?;
        let from = self.position;
        let desired = interpolate(motion.start, motion.endpoint, motion.next_segment + 1, 45)?;
        let (to, blocked) = if motion.blocked_by.is_some() {
            (from, motion.blocked_by)
        } else {
            sweep(arena, from, desired, 350)?
        };
        motion.blocked_by = motion.blocked_by.or(blocked);
        self.position = to;
        events.push(CoreMireEvent::Movement {
            tick: self.tick,
            phase: CoreMireMovementPhase::Retreat,
            from,
            to,
            blocked_by: blocked,
        });
        motion.next_segment += 1;
        if motion.next_segment < 45 {
            self.retreat = Some(motion);
        }
        Ok(())
    }
}

fn directed_endpoint(
    origin: CoreWorldPosition,
    target: CoreWorldPosition,
    distance: i64,
) -> Result<CoreWorldPosition, CoreMireError> {
    let d = delta(origin, target);
    let len = length(d)?;
    if len == 0 {
        return Err(CoreMireError::CoincidentTarget);
    }
    add(
        origin,
        round_div(d.0 * distance, len)?,
        round_div(d.1 * distance, len)?,
    )
}

fn retreat_endpoint(
    realized: CoreWorldPosition,
    target: CoreWorldPosition,
    charge_origin: CoreWorldPosition,
) -> Result<CoreWorldPosition, CoreMireError> {
    let mut d = delta(target, realized);
    if d == (0, 0) {
        let charge = delta(charge_origin, realized);
        d = (-charge.0, -charge.1);
    }
    let len = length(d)?;
    if len == 0 {
        return Err(CoreMireError::CoincidentTarget);
    }
    add(
        realized,
        round_div(d.0 * 5_250, len)?,
        round_div(d.1 * 5_250, len)?,
    )
}

fn contacts(
    arena: &ArenaGeometry,
    from: CoreWorldPosition,
    to: CoreWorldPosition,
    candidates: &[CoreTargetCandidate],
    hit: &mut BTreeSet<EntityId>,
) -> Result<Vec<EntityId>, CoreMireError> {
    let start = vector(from);
    let displacement = vector(to) - start;
    let mut contacts = Vec::new();
    for candidate in candidates {
        if !candidate.living || !candidate.damageable || hit.contains(&candidate.entity_id) {
            continue;
        }
        let world = ProjectileCollisionWorld::new(
            arena,
            vec![EnemyHurtbox::new(
                candidate.entity_id,
                vector(candidate.position),
                PLAYER_RADIUS_TILES,
            )?],
        )?;
        if let Some(contact) = world
            .sweep_circle(start, displacement, 0.35)?
            .filter(|contact| contact.target == CollisionTarget::Enemy(candidate.entity_id))
        {
            contacts.push((contact.fraction, candidate.entity_id));
        }
    }
    contacts.sort_by(|left, right| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)));
    let result = contacts
        .into_iter()
        .map(|(_, entity_id)| {
            hit.insert(entity_id);
            entity_id
        })
        .collect();
    Ok(result)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "authored collision radii are tightly bounded milli-tiles"
)]
fn sweep(
    arena: &ArenaGeometry,
    from: CoreWorldPosition,
    desired: CoreWorldPosition,
    radius: u32,
) -> Result<(CoreWorldPosition, Option<SolidColliderId>), CoreMireError> {
    let start = vector(from);
    let displacement = vector(desired) - start;
    let hit = ProjectileCollisionWorld::new(arena, Vec::new())?.sweep_solids(
        start,
        displacement,
        radius as f32 / 1_000.0,
    )?;
    let fraction = hit.map_or(1.0, |value| value.fraction);
    let d = delta(from, desired);
    let mut dx = scale(d.0, fraction)?;
    let mut dy = scale(d.1, fraction)?;
    if hit.is_some() && (dx != 0 || dy != 0) {
        dx -= d.0.signum();
        dy -= d.1.signum();
    }
    Ok((
        add(from, dx, dy)?,
        hit.and_then(|value| match value.target {
            CollisionTarget::Solid(id) => Some(id),
            CollisionTarget::Enemy(_) => None,
        }),
    ))
}

fn interpolate(
    start: CoreWorldPosition,
    end: CoreWorldPosition,
    completed: u32,
    total: u32,
) -> Result<CoreWorldPosition, CoreMireError> {
    let d = delta(start, end);
    add(
        start,
        round_div(d.0 * i64::from(completed), i64::from(total))?,
        round_div(d.1 * i64::from(completed), i64::from(total))?,
    )
}
fn normalized_step(
    d: (i64, i64),
    speed: u32,
    max: i64,
    rx: &mut i64,
    ry: &mut i64,
) -> Result<(i64, i64), CoreMireError> {
    let len = length(d)?;
    let den = len * 30;
    *rx += d.0 * i64::from(speed);
    *ry += d.1 * i64::from(speed);
    let mut x = *rx / den;
    let mut y = *ry / den;
    *rx %= den;
    *ry %= den;
    let planned = length((x, y))?;
    if planned > max && planned > 0 {
        x = round_div(x * max, planned)?;
        y = round_div(y * max, planned)?;
    }
    Ok((x, y))
}
fn delta(a: CoreWorldPosition, b: CoreWorldPosition) -> (i64, i64) {
    (
        i64::from(b.x_milli_tiles) - i64::from(a.x_milli_tiles),
        i64::from(b.y_milli_tiles) - i64::from(a.y_milli_tiles),
    )
}
fn add(p: CoreWorldPosition, x: i64, y: i64) -> Result<CoreWorldPosition, CoreMireError> {
    Ok(CoreWorldPosition::new(
        i32::try_from(i64::from(p.x_milli_tiles) + x).map_err(|_| CoreMireError::Arithmetic)?,
        i32::try_from(i64::from(p.y_milli_tiles) + y).map_err(|_| CoreMireError::Arithmetic)?,
    ))
}
fn length(d: (i64, i64)) -> Result<i64, CoreMireError> {
    let s =
        d.0.unsigned_abs()
            .checked_mul(d.0.unsigned_abs())
            .and_then(|x| {
                d.1.unsigned_abs()
                    .checked_mul(d.1.unsigned_abs())
                    .and_then(|y| x.checked_add(y))
            })
            .ok_or(CoreMireError::Arithmetic)?;
    i64::try_from(isqrt(s)).map_err(|_| CoreMireError::Arithmetic)
}
fn isqrt(v: u64) -> u64 {
    if v < 2 {
        return v;
    }
    let mut x = v;
    let mut n = x.midpoint(v / x);
    while n < x {
        x = n;
        n = x.midpoint(v / x);
    }
    x
}
fn round_div(n: i64, d: i64) -> Result<i64, CoreMireError> {
    if d <= 0 {
        return Err(CoreMireError::Arithmetic);
    }
    n.checked_add((d / 2) * n.signum())
        .map(|v| v / d)
        .ok_or(CoreMireError::Arithmetic)
}
#[allow(clippy::cast_precision_loss)]
fn vector(p: CoreWorldPosition) -> SimulationVector {
    SimulationVector::new(
        p.x_milli_tiles as f32 / 1_000.0,
        p.y_milli_tiles as f32 / 1_000.0,
    )
}
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn scale(v: i64, f: f32) -> Result<i64, CoreMireError> {
    let s = v as f64 * f64::from(f);
    if !s.is_finite() {
        return Err(CoreMireError::Arithmetic);
    }
    Ok(s.round() as i64)
}

#[derive(Debug, Error)]
pub enum CoreMireError {
    #[error("Mire Leech definition drifted")]
    DefinitionDrift,
    #[error("Mire Leech tick diverged")]
    TickMismatch,
    #[error("Mire Leech requires a target")]
    MissingTarget,
    #[error("Mire Leech target is coincident")]
    CoincidentTarget,
    #[error("Mire Leech charge is missing")]
    MissingCharge,
    #[error("Mire Leech completed charge is missing")]
    MissingCompletedCharge,
    #[error("Mire Leech retreat is missing")]
    MissingRetreat,
    #[error("Mire Leech tick overflowed")]
    TickOverflow,
    #[error("Mire Leech arithmetic overflowed")]
    Arithmetic,
    #[error(transparent)]
    Attack(#[from] CoreNormalAttackError),
    #[error(transparent)]
    Target(#[from] CoreTargetSelectionError),
    #[error(transparent)]
    Collision(#[from] CollisionError),
    #[error(transparent)]
    Hurtbox(#[from] HurtboxError),
}
