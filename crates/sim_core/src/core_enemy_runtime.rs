//! Renderer-independent target acquisition and telegraph-lock primitives for Core enemies.
//!
//! These types implement the authority-neutral part of `CONT-ENEMY-001`. Kit scheduling, leash
//! reference semantics, and introduction timing remain separate so unresolved design choices cannot
//! leak into otherwise stable selection and snapshot behavior.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::{AimVector, AttackCastId, EntityId, Tick};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CoreWorldPosition {
    pub x_milli_tiles: i32,
    pub y_milli_tiles: i32,
}

impl CoreWorldPosition {
    #[must_use]
    pub const fn new(x_milli_tiles: i32, y_milli_tiles: i32) -> Self {
        Self {
            x_milli_tiles,
            y_milli_tiles,
        }
    }

    #[must_use]
    pub fn squared_distance_to(self, other: Self) -> u128 {
        let x = i128::from(other.x_milli_tiles) - i128::from(self.x_milli_tiles);
        let y = i128::from(other.y_milli_tiles) - i128::from(self.y_milli_tiles);
        x.unsigned_abs()
            .saturating_mul(x.unsigned_abs())
            .saturating_add(y.unsigned_abs().saturating_mul(y.unsigned_abs()))
    }

    fn aim_to(self, other: Self) -> Result<AimVector, CoreAttackLockError> {
        let x = i64::from(other.x_milli_tiles) - i64::from(self.x_milli_tiles);
        let y = i64::from(other.y_milli_tiles) - i64::from(self.y_milli_tiles);
        let aim = AimVector {
            x: i32::try_from(x).map_err(|_| CoreAttackLockError::AimDeltaOverflow)?,
            y: i32::try_from(y).map_err(|_| CoreAttackLockError::AimDeltaOverflow)?,
        };
        if aim.is_valid() {
            Ok(aim)
        } else {
            Err(CoreAttackLockError::CoincidentTarget)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreTargetCandidate {
    pub entity_id: EntityId,
    pub position: CoreWorldPosition,
    pub living: bool,
    pub damageable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreSelectedTarget {
    pub entity_id: EntityId,
    pub position: CoreWorldPosition,
    pub squared_distance_milli_tiles: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CoreTargetSelectionError {
    #[error("Core target acquisition requires a nonzero aggro radius")]
    ZeroAggroRadius,
    #[error("Core target candidate {entity_id} appeared more than once")]
    DuplicateCandidate { entity_id: EntityId },
}

/// Selects the nearest living, damageable target inside the inclusive aggro boundary.
///
/// Ordering is `(squared distance, entity ID)`, so input iteration order and floating-point
/// equality can never influence authority.
pub fn select_core_target(
    origin: CoreWorldPosition,
    aggro_radius_milli_tiles: u32,
    candidates: &[CoreTargetCandidate],
) -> Result<Option<CoreSelectedTarget>, CoreTargetSelectionError> {
    if aggro_radius_milli_tiles == 0 {
        return Err(CoreTargetSelectionError::ZeroAggroRadius);
    }
    let mut seen = BTreeSet::new();
    for candidate in candidates {
        if !seen.insert(candidate.entity_id) {
            return Err(CoreTargetSelectionError::DuplicateCandidate {
                entity_id: candidate.entity_id,
            });
        }
    }

    let aggro_squared =
        u128::from(aggro_radius_milli_tiles).saturating_mul(u128::from(aggro_radius_milli_tiles));
    Ok(candidates
        .iter()
        .filter(|candidate| candidate.living && candidate.damageable)
        .filter_map(|candidate| {
            let squared_distance = origin.squared_distance_to(candidate.position);
            (squared_distance <= aggro_squared).then_some(CoreSelectedTarget {
                entity_id: candidate.entity_id,
                position: candidate.position,
                squared_distance_milli_tiles: squared_distance,
            })
        })
        .min_by_key(|target| (target.squared_distance_milli_tiles, target.entity_id)))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreAttackLock {
    cast_id: AttackCastId,
    pattern_id: String,
    pattern_index: usize,
    target_id: EntityId,
    origin_position: CoreWorldPosition,
    target_position: CoreWorldPosition,
    aim_delta: AimVector,
    telegraph_started_at: Tick,
    resolves_at: Tick,
}

impl CoreAttackLock {
    pub fn new(
        cast_id: AttackCastId,
        pattern_id: String,
        pattern_index: usize,
        origin_position: CoreWorldPosition,
        target: CoreSelectedTarget,
        telegraph_started_at: Tick,
        telegraph_ticks: u32,
    ) -> Result<Self, CoreAttackLockError> {
        if !valid_content_id(&pattern_id) {
            return Err(CoreAttackLockError::InvalidPatternId);
        }
        if telegraph_ticks == 0 {
            return Err(CoreAttackLockError::ZeroTelegraph);
        }
        let aim_delta = origin_position.aim_to(target.position)?;
        let resolves_at = Tick(
            telegraph_started_at
                .0
                .checked_add(u64::from(telegraph_ticks))
                .ok_or(CoreAttackLockError::TickOverflow)?,
        );
        Ok(Self {
            cast_id,
            pattern_id,
            pattern_index,
            target_id: target.entity_id,
            origin_position,
            target_position: target.position,
            aim_delta,
            telegraph_started_at,
            resolves_at,
        })
    }

    #[must_use]
    pub const fn cast_id(&self) -> AttackCastId {
        self.cast_id
    }

    #[must_use]
    pub fn pattern_id(&self) -> &str {
        &self.pattern_id
    }

    #[must_use]
    pub const fn pattern_index(&self) -> usize {
        self.pattern_index
    }

    #[must_use]
    pub const fn target_id(&self) -> EntityId {
        self.target_id
    }

    #[must_use]
    pub const fn origin_position(&self) -> CoreWorldPosition {
        self.origin_position
    }

    #[must_use]
    pub const fn target_position(&self) -> CoreWorldPosition {
        self.target_position
    }

    #[must_use]
    pub const fn aim_delta(&self) -> AimVector {
        self.aim_delta
    }

    #[must_use]
    pub const fn telegraph_started_at(&self) -> Tick {
        self.telegraph_started_at
    }

    #[must_use]
    pub const fn resolves_at(&self) -> Tick {
        self.resolves_at
    }

    #[must_use]
    pub const fn is_ready_at(&self, tick: Tick) -> bool {
        tick.0 >= self.resolves_at.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CoreAttackLockError {
    #[error("Core attack lock pattern ID is invalid")]
    InvalidPatternId,
    #[error("Core attack lock telegraph must last at least one tick")]
    ZeroTelegraph,
    #[error("Core attack lock target is coincident with its origin")]
    CoincidentTarget,
    #[error("Core attack lock aim delta exceeds fixed-point range")]
    AimDeltaOverflow,
    #[error("Core attack lock resolve tick overflowed")]
    TickOverflow,
}

fn valid_content_id(id: &str) -> bool {
    !id.is_empty()
        && id.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: u64) -> EntityId {
        EntityId::new(value).expect("nonzero entity ID")
    }

    fn candidate(entity_id: u64, x_milli_tiles: i32, y_milli_tiles: i32) -> CoreTargetCandidate {
        CoreTargetCandidate {
            entity_id: id(entity_id),
            position: CoreWorldPosition::new(x_milli_tiles, y_milli_tiles),
            living: true,
            damageable: true,
        }
    }

    #[test]
    fn target_selection_is_permutation_invariant_and_ties_lowest_entity_id() {
        let origin = CoreWorldPosition::new(1_000, 1_000);
        let forward = [
            candidate(9, 4_000, 5_000),
            candidate(3, -2_000, 5_000),
            candidate(7, 1_000, 8_000),
        ];
        let reverse = [forward[2], forward[1], forward[0]];

        let first = select_core_target(origin, 12_000, &forward)
            .expect("valid candidates")
            .expect("target");
        let second = select_core_target(origin, 12_000, &reverse)
            .expect("valid candidates")
            .expect("target");

        assert_eq!(first, second);
        assert_eq!(first.entity_id, id(3));
        assert_eq!(first.squared_distance_milli_tiles, 25_000_000);
    }

    #[test]
    fn eligibility_and_inclusive_aggro_boundary_are_exact() {
        let origin = CoreWorldPosition::new(0, 0);
        let mut dead = candidate(1, 1, 0);
        dead.living = false;
        let mut immune = candidate(2, 2, 0);
        immune.damageable = false;
        let boundary = candidate(3, 12_000, 0);
        let outside = candidate(4, 12_001, 0);

        assert_eq!(
            select_core_target(origin, 12_000, &[dead, immune, outside, boundary])
                .expect("valid candidates")
                .expect("boundary target")
                .entity_id,
            id(3)
        );
        assert_eq!(
            select_core_target(origin, 12_000, &[dead, immune, outside]).expect("valid candidates"),
            None
        );
    }

    #[test]
    fn duplicate_candidates_and_zero_radius_fail_before_selection() {
        let origin = CoreWorldPosition::new(0, 0);
        let duplicate = [candidate(5, 100, 0), candidate(5, 200, 0)];
        assert_eq!(
            select_core_target(origin, 12_000, &duplicate),
            Err(CoreTargetSelectionError::DuplicateCandidate { entity_id: id(5) })
        );
        assert_eq!(
            select_core_target(origin, 0, &[]),
            Err(CoreTargetSelectionError::ZeroAggroRadius)
        );
    }

    #[test]
    fn telegraph_lock_freezes_origin_target_aim_and_boundary_tick() {
        let origin = CoreWorldPosition::new(2_000, 3_000);
        let selected = CoreSelectedTarget {
            entity_id: id(11),
            position: CoreWorldPosition::new(6_000, 9_000),
            squared_distance_milli_tiles: 52_000_000,
        };
        let lock = CoreAttackLock::new(
            AttackCastId::from_ordinal(4).expect("cast ID"),
            "pattern.enemy.bell_acolyte.alternating_fan".to_owned(),
            0,
            origin,
            selected,
            Tick(100),
            12,
        )
        .expect("attack lock");

        let moved_target = CoreWorldPosition::new(-20_000, -30_000);
        assert_ne!(moved_target, lock.target_position());
        assert_eq!(lock.origin_position(), origin);
        assert_eq!(lock.target_position(), selected.position);
        assert_eq!(lock.aim_delta(), AimVector { x: 4_000, y: 6_000 });
        assert_eq!(lock.telegraph_started_at(), Tick(100));
        assert_eq!(lock.resolves_at(), Tick(112));
        assert!(!lock.is_ready_at(Tick(111)));
        assert!(lock.is_ready_at(Tick(112)));
    }

    #[test]
    fn lock_creation_rejects_invalid_input_without_partial_result() {
        let selected = CoreSelectedTarget {
            entity_id: id(1),
            position: CoreWorldPosition::new(0, 0),
            squared_distance_milli_tiles: 0,
        };
        let create = |pattern: &str, origin, ticks, start| {
            CoreAttackLock::new(
                AttackCastId::FIRST,
                pattern.to_owned(),
                0,
                origin,
                selected,
                start,
                ticks,
            )
        };
        assert_eq!(
            create("Bad Pattern", CoreWorldPosition::new(1, 0), 1, Tick(0)),
            Err(CoreAttackLockError::InvalidPatternId)
        );
        assert_eq!(
            create("pattern.valid", CoreWorldPosition::new(1, 0), 0, Tick(0)),
            Err(CoreAttackLockError::ZeroTelegraph)
        );
        assert_eq!(
            create("pattern.valid", CoreWorldPosition::new(0, 0), 1, Tick(0)),
            Err(CoreAttackLockError::CoincidentTarget)
        );
        assert_eq!(
            create(
                "pattern.valid",
                CoreWorldPosition::new(i32::MIN, 0),
                1,
                Tick(0)
            ),
            Err(CoreAttackLockError::AimDeltaOverflow)
        );
        assert_eq!(
            create(
                "pattern.valid",
                CoreWorldPosition::new(1, 0),
                1,
                Tick(u64::MAX)
            ),
            Err(CoreAttackLockError::TickOverflow)
        );
    }
}
