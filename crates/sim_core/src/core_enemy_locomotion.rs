//! Fixed-point locomotion for Core-authored normal enemies.
//!
//! `SPEC-CONFLICT-017` fixes the otherwise unspecified Acolyte distance correction and Choir
//! Skull orbit phase. This module owns those decisions independently from attack scheduling.

use thiserror::Error;

use crate::{
    ArenaGeometry, CollisionError, CollisionTarget, CoreEnemyDefinition,
    CoreEnemyLocomotionDefinition, CoreSelectedTarget, CoreWorldPosition, EnemyHurtbox, EntityId,
    HostileError, HurtboxError, ProjectileCollisionWorld, SimulationVector, SolidColliderId,
};

const TICKS_PER_SECOND: i64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreNormalLocomotionKind {
    BellAcolyte,
    ChoirSkull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreNormalLocomotionStep {
    pub kind: CoreNormalLocomotionKind,
    pub from: CoreWorldPosition,
    pub to: CoreWorldPosition,
    pub blocked_by: Option<SolidColliderId>,
    pub positioned_for_attack: bool,
}

#[derive(Debug, Clone)]
enum CoreNormalLocomotionState {
    MaintainDistance {
        speed_milli_tiles_per_second: u32,
        preferred_distance_milli_tiles: u32,
    },
    OrbitAnchor {
        speed_milli_tiles_per_second: u32,
        radius_milli_tiles: u32,
        reached_orbit: bool,
        phase_x_milli_tiles: i64,
        phase_y_milli_tiles: i64,
        phase_x_remainder: i64,
        phase_y_remainder: i64,
    },
}

/// Collision-aware fixed-point owner for the two moving authored actors in B2.
#[derive(Debug, Clone)]
pub struct CoreNormalLocomotionSimulation {
    entity_id: EntityId,
    home: CoreWorldPosition,
    position: CoreWorldPosition,
    hurtbox_radius_milli_tiles: u32,
    x_remainder: i64,
    y_remainder: i64,
    state: CoreNormalLocomotionState,
}

impl CoreNormalLocomotionSimulation {
    pub fn new(
        definition: &CoreEnemyDefinition,
        entity_id: EntityId,
        home: CoreWorldPosition,
    ) -> Result<Self, CoreNormalLocomotionError> {
        let state = match (
            definition.parameters().content_id.as_str(),
            definition.locomotion(),
        ) {
            (
                "enemy.bell_acolyte",
                CoreEnemyLocomotionDefinition::MaintainDistance {
                    movement_speed_milli_tiles_per_second: 3_000,
                    preferred_distance_milli_tiles: 6_000,
                },
            ) => CoreNormalLocomotionState::MaintainDistance {
                speed_milli_tiles_per_second: 3_000,
                preferred_distance_milli_tiles: 6_000,
            },
            (
                "enemy.choir_skull",
                CoreEnemyLocomotionDefinition::OrbitAnchor {
                    movement_speed_milli_tiles_per_second: 2_800,
                    orbit_radius_milli_tiles: 3_000,
                },
            ) => CoreNormalLocomotionState::OrbitAnchor {
                speed_milli_tiles_per_second: 2_800,
                radius_milli_tiles: 3_000,
                reached_orbit: false,
                phase_x_milli_tiles: 3_000,
                phase_y_milli_tiles: 0,
                phase_x_remainder: 0,
                phase_y_remainder: 0,
            },
            (content_id, _) => {
                return Err(CoreNormalLocomotionError::UnsupportedActor {
                    content_id: content_id.to_owned(),
                });
            }
        };
        Ok(Self {
            entity_id,
            home,
            position: home,
            hurtbox_radius_milli_tiles: definition.parameters().hurtbox_radius_milli_tiles,
            x_remainder: 0,
            y_remainder: 0,
            state,
        })
    }

    #[must_use]
    pub const fn position(&self) -> CoreWorldPosition {
        self.position
    }

    pub fn advance(
        &mut self,
        arena: &ArenaGeometry,
        target: Option<CoreSelectedTarget>,
    ) -> Result<CoreNormalLocomotionStep, CoreNormalLocomotionError> {
        let mut staged = self.clone();
        let step = staged.advance_inner(arena, target)?;
        *self = staged;
        Ok(step)
    }

    pub fn reset(&mut self) {
        self.position = self.home;
        self.x_remainder = 0;
        self.y_remainder = 0;
        if let CoreNormalLocomotionState::OrbitAnchor {
            reached_orbit,
            phase_x_milli_tiles,
            phase_y_milli_tiles,
            phase_x_remainder,
            phase_y_remainder,
            ..
        } = &mut self.state
        {
            *reached_orbit = false;
            *phase_x_milli_tiles = 3_000;
            *phase_y_milli_tiles = 0;
            *phase_x_remainder = 0;
            *phase_y_remainder = 0;
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "the two approved locomotion contracts share one atomic state transition"
    )]
    fn advance_inner(
        &mut self,
        arena: &ArenaGeometry,
        target: Option<CoreSelectedTarget>,
    ) -> Result<CoreNormalLocomotionStep, CoreNormalLocomotionError> {
        let from = self.position;
        let (kind, desired, positioned_for_attack) = match &mut self.state {
            CoreNormalLocomotionState::MaintainDistance {
                speed_milli_tiles_per_second,
                preferred_distance_milli_tiles,
            } => {
                let Some(target) = target else {
                    return Ok(CoreNormalLocomotionStep {
                        kind: CoreNormalLocomotionKind::BellAcolyte,
                        from,
                        to: from,
                        blocked_by: None,
                        positioned_for_attack: false,
                    });
                };
                let target_delta = delta(from, target.position);
                let distance = vector_length(target_delta)?;
                if distance == u64::from(*preferred_distance_milli_tiles) {
                    (CoreNormalLocomotionKind::BellAcolyte, from, true)
                } else {
                    let toward_target = distance > u64::from(*preferred_distance_milli_tiles);
                    let maximum_step =
                        distance.abs_diff(u64::from(*preferred_distance_milli_tiles));
                    let (move_x, move_y) = planned_normalized_step(
                        target_delta,
                        *speed_milli_tiles_per_second,
                        maximum_step,
                        toward_target,
                        &mut self.x_remainder,
                        &mut self.y_remainder,
                    )?;
                    (
                        CoreNormalLocomotionKind::BellAcolyte,
                        add_position(from, move_x, move_y)?,
                        false,
                    )
                }
            }
            CoreNormalLocomotionState::OrbitAnchor {
                speed_milli_tiles_per_second,
                radius_milli_tiles,
                reached_orbit,
                phase_x_milli_tiles,
                phase_y_milli_tiles,
                phase_x_remainder,
                phase_y_remainder,
            } => {
                let orbit_target = if *reached_orbit {
                    advance_clockwise_phase(
                        *speed_milli_tiles_per_second,
                        *radius_milli_tiles,
                        phase_x_milli_tiles,
                        phase_y_milli_tiles,
                        phase_x_remainder,
                        phase_y_remainder,
                    )?;
                    add_position(self.home, *phase_x_milli_tiles, *phase_y_milli_tiles)?
                } else {
                    add_position(self.home, i64::from(*radius_milli_tiles), 0)?
                };
                let target_delta = delta(from, orbit_target);
                let distance = vector_length(target_delta)?;
                let desired = if distance == 0 {
                    *reached_orbit = true;
                    from
                } else if distance <= u64::from((*speed_milli_tiles_per_second).div_ceil(30)) {
                    self.x_remainder = 0;
                    self.y_remainder = 0;
                    *reached_orbit = true;
                    orbit_target
                } else {
                    let (move_x, move_y) = planned_normalized_step(
                        target_delta,
                        *speed_milli_tiles_per_second,
                        distance,
                        true,
                        &mut self.x_remainder,
                        &mut self.y_remainder,
                    )?;
                    add_position(from, move_x, move_y)?
                };
                (
                    CoreNormalLocomotionKind::ChoirSkull,
                    desired,
                    *reached_orbit,
                )
            }
        };
        let (to, blocked_by) = self.sweep_to(arena, desired)?;
        self.position = to;
        let positioned_for_attack = match self.state {
            CoreNormalLocomotionState::MaintainDistance {
                preferred_distance_milli_tiles,
                ..
            } => target.is_some_and(|target| {
                vector_length(delta(to, target.position))
                    .is_ok_and(|distance| distance == u64::from(preferred_distance_milli_tiles))
            }),
            CoreNormalLocomotionState::OrbitAnchor { reached_orbit, .. } => {
                reached_orbit && positioned_for_attack
            }
        };
        Ok(CoreNormalLocomotionStep {
            kind,
            from,
            to,
            blocked_by,
            positioned_for_attack,
        })
    }

    #[allow(clippy::cast_precision_loss)] // Authored radii are tightly bounded milli-tiles.
    fn sweep_to(
        &self,
        arena: &ArenaGeometry,
        desired: CoreWorldPosition,
    ) -> Result<(CoreWorldPosition, Option<SolidColliderId>), CoreNormalLocomotionError> {
        let from = world_to_vector(self.position);
        let desired_vector = world_to_vector(desired);
        let displacement = desired_vector - from;
        let world = ProjectileCollisionWorld::new(arena, Vec::new())?;
        let hit = world.sweep_solids(
            from,
            displacement,
            self.hurtbox_radius_milli_tiles as f32 / 1_000.0,
        )?;
        let fraction = hit.map_or(1.0, |contact| contact.fraction);
        let dx = i64::from(desired.x_milli_tiles) - i64::from(self.position.x_milli_tiles);
        let dy = i64::from(desired.y_milli_tiles) - i64::from(self.position.y_milli_tiles);
        let mut applied_x = scale_by_fraction(dx, fraction)?;
        let mut applied_y = scale_by_fraction(dy, fraction)?;
        if hit.is_some() && (applied_x != 0 || applied_y != 0) {
            applied_x = applied_x.saturating_sub(dx.signum());
            applied_y = applied_y.saturating_sub(dy.signum());
        }
        let to = add_position(self.position, applied_x, applied_y)?;
        ProjectileCollisionWorld::new(
            arena,
            vec![EnemyHurtbox::new(
                self.entity_id,
                world_to_vector(to),
                self.hurtbox_radius_milli_tiles as f32 / 1_000.0,
            )?],
        )?;
        Ok((
            to,
            hit.and_then(|contact| match contact.target {
                CollisionTarget::Solid(id) => Some(id),
                CollisionTarget::Enemy(_) => None,
            }),
        ))
    }
}

fn advance_clockwise_phase(
    speed: u32,
    radius: u32,
    phase_x: &mut i64,
    phase_y: &mut i64,
    remainder_x: &mut i64,
    remainder_y: &mut i64,
) -> Result<(), CoreNormalLocomotionError> {
    let tangent = (-*phase_y, *phase_x);
    let (dx, dy) =
        planned_normalized_step(tangent, speed, u64::MAX, true, remainder_x, remainder_y)?;
    let candidate = (phase_x.saturating_add(dx), phase_y.saturating_add(dy));
    let candidate_length = vector_length(candidate)?;
    *phase_x = divide_round_nearest(
        candidate.0.saturating_mul(i64::from(radius)),
        candidate_length,
    )?;
    *phase_y = divide_round_nearest(
        candidate.1.saturating_mul(i64::from(radius)),
        candidate_length,
    )?;
    Ok(())
}

fn planned_normalized_step(
    direction: (i64, i64),
    speed: u32,
    maximum_step: u64,
    forward: bool,
    remainder_x: &mut i64,
    remainder_y: &mut i64,
) -> Result<(i64, i64), CoreNormalLocomotionError> {
    let length = vector_length(direction)?;
    if length == 0 {
        return Ok((0, 0));
    }
    let sign = if forward { 1_i64 } else { -1_i64 };
    let denominator = i64::try_from(length)
        .ok()
        .and_then(|length| length.checked_mul(TICKS_PER_SECOND))
        .ok_or(CoreNormalLocomotionError::ArithmeticOverflow)?;
    *remainder_x = remainder_x
        .checked_add(
            direction
                .0
                .checked_mul(i64::from(speed))
                .and_then(|value| value.checked_mul(sign))
                .ok_or(CoreNormalLocomotionError::ArithmeticOverflow)?,
        )
        .ok_or(CoreNormalLocomotionError::ArithmeticOverflow)?;
    *remainder_y = remainder_y
        .checked_add(
            direction
                .1
                .checked_mul(i64::from(speed))
                .and_then(|value| value.checked_mul(sign))
                .ok_or(CoreNormalLocomotionError::ArithmeticOverflow)?,
        )
        .ok_or(CoreNormalLocomotionError::ArithmeticOverflow)?;
    let mut dx = *remainder_x / denominator;
    let mut dy = *remainder_y / denominator;
    *remainder_x %= denominator;
    *remainder_y %= denominator;
    let planned_length = vector_length((dx, dy))?;
    if planned_length > maximum_step && planned_length > 0 {
        dx = divide_round_nearest(
            dx.checked_mul(
                i64::try_from(maximum_step)
                    .map_err(|_| CoreNormalLocomotionError::ArithmeticOverflow)?,
            )
            .ok_or(CoreNormalLocomotionError::ArithmeticOverflow)?,
            planned_length,
        )?;
        dy = divide_round_nearest(
            dy.checked_mul(
                i64::try_from(maximum_step)
                    .map_err(|_| CoreNormalLocomotionError::ArithmeticOverflow)?,
            )
            .ok_or(CoreNormalLocomotionError::ArithmeticOverflow)?,
            planned_length,
        )?;
    }
    Ok((dx, dy))
}

fn delta(from: CoreWorldPosition, to: CoreWorldPosition) -> (i64, i64) {
    (
        i64::from(to.x_milli_tiles) - i64::from(from.x_milli_tiles),
        i64::from(to.y_milli_tiles) - i64::from(from.y_milli_tiles),
    )
}

fn add_position(
    position: CoreWorldPosition,
    dx: i64,
    dy: i64,
) -> Result<CoreWorldPosition, CoreNormalLocomotionError> {
    Ok(CoreWorldPosition::new(
        i32::try_from(
            i64::from(position.x_milli_tiles)
                .checked_add(dx)
                .ok_or(CoreNormalLocomotionError::ArithmeticOverflow)?,
        )
        .map_err(|_| CoreNormalLocomotionError::ArithmeticOverflow)?,
        i32::try_from(
            i64::from(position.y_milli_tiles)
                .checked_add(dy)
                .ok_or(CoreNormalLocomotionError::ArithmeticOverflow)?,
        )
        .map_err(|_| CoreNormalLocomotionError::ArithmeticOverflow)?,
    ))
}

fn vector_length(vector: (i64, i64)) -> Result<u64, CoreNormalLocomotionError> {
    let squared = vector
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
        .ok_or(CoreNormalLocomotionError::ArithmeticOverflow)?;
    Ok(integer_sqrt(squared))
}

fn integer_sqrt(value: u64) -> u64 {
    if value < 2 {
        return value;
    }
    let mut low = 1_u64;
    let mut high = value / 2 + 1;
    while low <= high {
        let mid = low + (high - low) / 2;
        if mid <= value / mid {
            low = mid + 1;
        } else {
            high = mid - 1;
        }
    }
    high
}

fn divide_round_nearest(value: i64, divisor: u64) -> Result<i64, CoreNormalLocomotionError> {
    let divisor =
        i64::try_from(divisor).map_err(|_| CoreNormalLocomotionError::ArithmeticOverflow)?;
    let adjustment = divisor / 2 * value.signum();
    value
        .checked_add(adjustment)
        .map(|value| value / divisor)
        .ok_or(CoreNormalLocomotionError::ArithmeticOverflow)
}

#[allow(clippy::cast_precision_loss)]
fn world_to_vector(position: CoreWorldPosition) -> SimulationVector {
    SimulationVector::new(
        position.x_milli_tiles as f32 / 1_000.0,
        position.y_milli_tiles as f32 / 1_000.0,
    )
}

#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)] // Collision fractions are finite [0,1] values applied to tightly bounded room deltas.
fn scale_by_fraction(value: i64, fraction: f32) -> Result<i64, CoreNormalLocomotionError> {
    let scaled = value as f64 * f64::from(fraction);
    if !scaled.is_finite() || scaled < i64::MIN as f64 || scaled > i64::MAX as f64 {
        return Err(CoreNormalLocomotionError::ArithmeticOverflow);
    }
    Ok(scaled.trunc() as i64)
}

#[derive(Debug, Error)]
pub enum CoreNormalLocomotionError {
    #[error("Core normal locomotion does not support {content_id}")]
    UnsupportedActor { content_id: String },
    #[error("Core normal locomotion fixed-point arithmetic overflowed")]
    ArithmeticOverflow,
    #[error(transparent)]
    Collision(#[from] CollisionError),
    #[error(transparent)]
    Hostile(#[from] HostileError),
    #[error(transparent)]
    Hurtbox(#[from] HurtboxError),
}
