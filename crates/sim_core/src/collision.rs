use std::{cmp::Ordering, collections::BTreeSet, fmt};

use thiserror::Error;

use crate::{ArenaGeometry, EntityId, MILLI_TILES_PER_TILE, SimulationVector, TileRectangle};

const CONTACT_EPSILON_SQUARED: f32 = 1.0e-12;

/// Semantic side IDs keep authored shell contacts stable across render implementations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShellSide {
    North,
    South,
    West,
    East,
}

impl fmt::Display for ShellSide {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::North => "north",
            Self::South => "south",
            Self::West => "west",
            Self::East => "east",
        })
    }
}

/// Stable identifier for one immutable arena solid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SolidColliderId {
    Shell(ShellSide),
    Pillar(u32),
}

impl fmt::Display for SolidColliderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shell(side) => write!(formatter, "shell.{side}"),
            Self::Pillar(index) => write!(formatter, "pillar.{index}"),
        }
    }
}

/// Stable contact target. Variant order intentionally resolves exact ties solid-first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CollisionTarget {
    Solid(SolidColliderId),
    Enemy(EntityId),
}

impl fmt::Display for CollisionTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Solid(id) => id.fmt(formatter),
            Self::Enemy(id) => write!(formatter, "enemy.{id}"),
        }
    }
}

/// Simulation-owned circular enemy contact geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnemyHurtbox {
    id: EntityId,
    center: SimulationVector,
    radius_tiles: f32,
}

impl EnemyHurtbox {
    pub fn new(
        id: EntityId,
        center: SimulationVector,
        radius_tiles: f32,
    ) -> Result<Self, HurtboxError> {
        if !center.is_finite() {
            return Err(HurtboxError::NonFiniteCenter { id });
        }
        if !radius_tiles.is_finite() || radius_tiles <= 0.0 {
            return Err(HurtboxError::InvalidRadius { id });
        }
        Ok(Self {
            id,
            center,
            radius_tiles,
        })
    }

    #[must_use]
    pub const fn id(self) -> EntityId {
        self.id
    }

    #[must_use]
    pub const fn center(self) -> SimulationVector {
        self.center
    }

    #[must_use]
    pub const fn radius_tiles(self) -> f32 {
        self.radius_tiles
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HurtboxError {
    #[error("enemy {id} hurtbox center must be finite")]
    NonFiniteCenter { id: EntityId },
    #[error("enemy {id} hurtbox radius must be finite and positive")]
    InvalidRadius { id: EntityId },
}

/// Immutable tick collision snapshot. Enemies are canonicalized by stable entity ID.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileCollisionWorld {
    width_tiles: f32,
    height_tiles: f32,
    pillars: Vec<TileRectangle>,
    enemies: Vec<EnemyHurtbox>,
}

impl ProjectileCollisionWorld {
    pub fn new(
        arena: &ArenaGeometry,
        mut enemies: Vec<EnemyHurtbox>,
    ) -> Result<Self, CollisionError> {
        let width_tiles = milli_to_tiles(arena.width_milli_tiles);
        let height_tiles = milli_to_tiles(arena.height_milli_tiles);
        if !width_tiles.is_finite()
            || !height_tiles.is_finite()
            || width_tiles <= 0.0
            || height_tiles <= 0.0
        {
            return Err(CollisionError::InvalidArenaBounds);
        }
        enemies.sort_by_key(|hurtbox| hurtbox.id);
        let mut ids = BTreeSet::new();
        for hurtbox in &enemies {
            if !ids.insert(hurtbox.id) {
                return Err(CollisionError::DuplicateEnemyId(hurtbox.id));
            }
            if circle_overlaps_world_solid(
                hurtbox.center,
                hurtbox.radius_tiles,
                width_tiles,
                height_tiles,
                &arena.pillars,
            ) {
                return Err(CollisionError::EnemyOverlapsSolid(hurtbox.id));
            }
        }
        Ok(Self {
            width_tiles,
            height_tiles,
            pillars: arena.pillars.clone(),
            enemies,
        })
    }

    #[must_use]
    pub fn enemies(&self) -> &[EnemyHurtbox] {
        &self.enemies
    }

    /// Finds one deterministic earliest contact over a closed swept-circle segment.
    pub fn sweep_circle(
        &self,
        start: SimulationVector,
        displacement: SimulationVector,
        radius_tiles: f32,
    ) -> Result<Option<SweepHit>, CollisionError> {
        if !start.is_finite() || !displacement.is_finite() {
            return Err(CollisionError::NonFiniteSweep);
        }
        if !radius_tiles.is_finite() || radius_tiles <= 0.0 {
            return Err(CollisionError::InvalidProjectileRadius);
        }
        self.sweep_circle_ignoring_enemies(start, displacement, radius_tiles, &[])
    }

    /// Finds the earliest contact while excluding enemies already hit by one piercing projectile.
    pub fn sweep_circle_ignoring_enemies(
        &self,
        start: SimulationVector,
        displacement: SimulationVector,
        radius_tiles: f32,
        ignored_enemies: &[EntityId],
    ) -> Result<Option<SweepHit>, CollisionError> {
        if !start.is_finite() || !displacement.is_finite() {
            return Err(CollisionError::NonFiniteSweep);
        }
        if !radius_tiles.is_finite() || radius_tiles <= 0.0 {
            return Err(CollisionError::InvalidProjectileRadius);
        }
        if !ignored_enemies.windows(2).all(|pair| pair[0] < pair[1]) {
            return Err(CollisionError::IgnoredEnemyIdsNotSortedUnique);
        }
        let mut best = None;
        self.collect_shell_hits(start, displacement, radius_tiles, &mut best)?;
        for (index, pillar) in self.pillars.iter().copied().enumerate() {
            let index = u32::try_from(index).map_err(|_| CollisionError::TooManyPillars)?;
            collect_rounded_rectangle_hits(
                start,
                displacement,
                radius_tiles,
                pillar,
                CollisionTarget::Solid(SolidColliderId::Pillar(index)),
                &mut best,
            )?;
        }
        for enemy in &self.enemies {
            if ignored_enemies.binary_search(&enemy.id).is_ok() {
                continue;
            }
            let combined_radius = radius_tiles + enemy.radius_tiles;
            if !combined_radius.is_finite() {
                return Err(CollisionError::CalculatedNonFiniteContact);
            }
            if let Some(fraction) =
                segment_circle_fraction(start, displacement, enemy.center, combined_radius)?
            {
                consider_hit(
                    &mut best,
                    SweepHit {
                        fraction,
                        target: CollisionTarget::Enemy(enemy.id),
                    },
                );
            }
        }
        Ok(best)
    }

    /// Finds the earliest solid contact, intentionally ignoring nonblocking enemy hurtboxes.
    pub fn sweep_solids(
        &self,
        start: SimulationVector,
        displacement: SimulationVector,
        radius_tiles: f32,
    ) -> Result<Option<SweepHit>, CollisionError> {
        let ignored = self
            .enemies
            .iter()
            .map(|enemy| enemy.id)
            .collect::<Vec<_>>();
        self.sweep_circle_ignoring_enemies(start, displacement, radius_tiles, &ignored)
    }

    fn collect_shell_hits(
        &self,
        start: SimulationVector,
        displacement: SimulationVector,
        radius: f32,
        best: &mut Option<SweepHit>,
    ) -> Result<(), CollisionError> {
        let min_x = radius;
        let min_y = radius;
        let max_x = self.width_tiles - radius;
        let max_y = self.height_tiles - radius;
        if max_x < min_x || max_y < min_y {
            return Err(CollisionError::ProjectileTooLargeForArena);
        }
        for (is_contact, target) in [
            (start.y <= min_y, ShellSide::North),
            (start.y >= max_y, ShellSide::South),
            (start.x <= min_x, ShellSide::West),
            (start.x >= max_x, ShellSide::East),
        ] {
            if is_contact {
                consider_hit(
                    best,
                    SweepHit {
                        fraction: 0.0,
                        target: CollisionTarget::Solid(SolidColliderId::Shell(target)),
                    },
                );
            }
        }
        for (fraction, target) in [
            axis_contact_fraction(start.y, displacement.y, min_y, false)
                .map(|value| (value, ShellSide::North)),
            axis_contact_fraction(start.y, displacement.y, max_y, true)
                .map(|value| (value, ShellSide::South)),
            axis_contact_fraction(start.x, displacement.x, min_x, false)
                .map(|value| (value, ShellSide::West)),
            axis_contact_fraction(start.x, displacement.x, max_x, true)
                .map(|value| (value, ShellSide::East)),
        ]
        .into_iter()
        .flatten()
        {
            validate_fraction(fraction)?;
            consider_hit(
                best,
                SweepHit {
                    fraction,
                    target: CollisionTarget::Solid(SolidColliderId::Shell(target)),
                },
            );
        }
        Ok(())
    }
}

/// Earliest closed-segment contact expressed as a fraction of candidate displacement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SweepHit {
    pub fraction: f32,
    pub target: CollisionTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CollisionError {
    #[error("arena bounds must be finite and positive")]
    InvalidArenaBounds,
    #[error("projectile radius must be finite and positive")]
    InvalidProjectileRadius,
    #[error("projectile circle is too large for the arena")]
    ProjectileTooLargeForArena,
    #[error("sweep start and displacement must be finite")]
    NonFiniteSweep,
    #[error("duplicate enemy hurtbox ID {0}")]
    DuplicateEnemyId(EntityId),
    #[error("enemy hurtbox {0} overlaps an arena solid")]
    EnemyOverlapsSolid(EntityId),
    #[error("pillar count exceeds stable u32 collider IDs")]
    TooManyPillars,
    #[error("collision calculation produced a non-finite contact")]
    CalculatedNonFiniteContact,
    #[error("ignored enemy IDs must be sorted and unique")]
    IgnoredEnemyIdsNotSortedUnique,
}

fn axis_contact_fraction(
    start: f32,
    displacement: f32,
    boundary: f32,
    positive_direction: bool,
) -> Option<f32> {
    if (positive_direction && displacement > 0.0) || (!positive_direction && displacement < 0.0) {
        let fraction = (boundary - start) / displacement;
        (0.0..=1.0).contains(&fraction).then_some(fraction)
    } else {
        None
    }
}

fn collect_rounded_rectangle_hits(
    start: SimulationVector,
    displacement: SimulationVector,
    radius: f32,
    rectangle: TileRectangle,
    target: CollisionTarget,
    best: &mut Option<SweepHit>,
) -> Result<(), CollisionError> {
    let left = milli_to_tiles(rectangle.x_milli_tiles);
    let top = milli_to_tiles(rectangle.y_milli_tiles);
    let right = left + milli_to_tiles(rectangle.width_milli_tiles);
    let bottom = top + milli_to_tiles(rectangle.height_milli_tiles);
    if circle_rectangle_distance_squared(start, left, top, right, bottom) <= radius * radius {
        consider_hit(
            best,
            SweepHit {
                fraction: 0.0,
                target,
            },
        );
        return Ok(());
    }

    let face_candidates = [
        axis_contact_fraction(start.x, displacement.x, left - radius, true)
            .filter(|fraction| in_closed_range(start.y + displacement.y * *fraction, top, bottom)),
        axis_contact_fraction(start.x, displacement.x, right + radius, false)
            .filter(|fraction| in_closed_range(start.y + displacement.y * *fraction, top, bottom)),
        axis_contact_fraction(start.y, displacement.y, top - radius, true)
            .filter(|fraction| in_closed_range(start.x + displacement.x * *fraction, left, right)),
        axis_contact_fraction(start.y, displacement.y, bottom + radius, false)
            .filter(|fraction| in_closed_range(start.x + displacement.x * *fraction, left, right)),
    ];
    for fraction in face_candidates.into_iter().flatten() {
        validate_fraction(fraction)?;
        consider_hit(best, SweepHit { fraction, target });
    }

    for (corner, x_side, y_side) in [
        (
            SimulationVector::new(left, top),
            Ordering::Less,
            Ordering::Less,
        ),
        (
            SimulationVector::new(right, top),
            Ordering::Greater,
            Ordering::Less,
        ),
        (
            SimulationVector::new(left, bottom),
            Ordering::Less,
            Ordering::Greater,
        ),
        (
            SimulationVector::new(right, bottom),
            Ordering::Greater,
            Ordering::Greater,
        ),
    ] {
        let Some(fraction) = segment_circle_fraction(start, displacement, corner, radius)? else {
            continue;
        };
        let impact = start + displacement * fraction;
        let x_order = impact.x.total_cmp(&corner.x);
        let y_order = impact.y.total_cmp(&corner.y);
        if (x_order == x_side || x_order == Ordering::Equal)
            && (y_order == y_side || y_order == Ordering::Equal)
        {
            consider_hit(best, SweepHit { fraction, target });
        }
    }
    Ok(())
}

fn segment_circle_fraction(
    start: SimulationVector,
    displacement: SimulationVector,
    center: SimulationVector,
    radius: f32,
) -> Result<Option<f32>, CollisionError> {
    let relative = start - center;
    let radius_squared = radius * radius;
    if relative.length_squared() <= radius_squared {
        return Ok(Some(0.0));
    }
    let a = displacement.length_squared();
    if a <= CONTACT_EPSILON_SQUARED {
        return Ok(None);
    }
    let b = 2.0 * relative.dot(displacement);
    let c = relative.length_squared() - radius_squared;
    let discriminant = b.mul_add(b, -4.0 * a * c);
    if !discriminant.is_finite() {
        return Err(CollisionError::CalculatedNonFiniteContact);
    }
    if discriminant < 0.0 {
        return Ok(None);
    }
    let fraction = (-b - discriminant.sqrt()) / (2.0 * a);
    validate_fraction_or_none(fraction)
}

fn validate_fraction_or_none(fraction: f32) -> Result<Option<f32>, CollisionError> {
    if !fraction.is_finite() {
        return Err(CollisionError::CalculatedNonFiniteContact);
    }
    Ok((0.0..=1.0).contains(&fraction).then_some(fraction))
}

fn validate_fraction(fraction: f32) -> Result<(), CollisionError> {
    if fraction.is_finite() && (0.0..=1.0).contains(&fraction) {
        Ok(())
    } else {
        Err(CollisionError::CalculatedNonFiniteContact)
    }
}

fn consider_hit(best: &mut Option<SweepHit>, candidate: SweepHit) {
    let replace = best.is_none_or(|current| {
        candidate
            .fraction
            .total_cmp(&current.fraction)
            .then_with(|| candidate.target.cmp(&current.target))
            == Ordering::Less
    });
    if replace {
        *best = Some(candidate);
    }
}

fn circle_overlaps_world_solid(
    center: SimulationVector,
    radius: f32,
    width: f32,
    height: f32,
    pillars: &[TileRectangle],
) -> bool {
    if center.x - radius < 0.0
        || center.y - radius < 0.0
        || center.x + radius > width
        || center.y + radius > height
    {
        return true;
    }
    pillars.iter().copied().any(|rectangle| {
        let left = milli_to_tiles(rectangle.x_milli_tiles);
        let top = milli_to_tiles(rectangle.y_milli_tiles);
        let right = left + milli_to_tiles(rectangle.width_milli_tiles);
        let bottom = top + milli_to_tiles(rectangle.height_milli_tiles);
        circle_rectangle_distance_squared(center, left, top, right, bottom) < radius * radius
    })
}

fn circle_rectangle_distance_squared(
    center: SimulationVector,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
) -> f32 {
    let nearest = SimulationVector::new(center.x.clamp(left, right), center.y.clamp(top, bottom));
    (center - nearest).length_squared()
}

fn in_closed_range(value: f32, minimum: f32, maximum: f32) -> bool {
    value >= minimum && value <= maximum
}

#[allow(clippy::cast_precision_loss)]
fn milli_to_tiles(value: i32) -> f32 {
    value as f32 / MILLI_TILES_PER_TILE as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArenaGeometry, TilePoint};

    fn arena() -> ArenaGeometry {
        ArenaGeometry {
            id: "arena.collision_test".to_owned(),
            width_milli_tiles: 32_000,
            height_milli_tiles: 24_000,
            shell_thickness_milli_tiles: 1_000,
            player_spawn: TilePoint::new(4_000, 12_000),
            boss_spawn: TilePoint::new(24_000, 12_000),
            pillars: vec![TileRectangle::new(10_000, 5_000, 2_000, 3_000)],
            anchors: vec![],
        }
        .validated()
        .expect("arena")
    }

    fn enemy(id: u64, center: SimulationVector, radius: f32) -> EnemyHurtbox {
        EnemyHurtbox::new(EntityId::new(id).expect("ID"), center, radius).expect("hurtbox")
    }

    #[test]
    fn shell_and_pillar_faces_stop_at_projectile_center_contact() {
        let world = ProjectileCollisionWorld::new(&arena(), vec![]).expect("world");
        let shell = world
            .sweep_circle(
                SimulationVector::new(0.5, 12.0),
                SimulationVector::new(-0.4, 0.0),
                0.1,
            )
            .expect("sweep")
            .expect("hit");
        assert_eq!(
            shell.target,
            CollisionTarget::Solid(SolidColliderId::Shell(ShellSide::West))
        );
        assert!((shell.fraction - 1.0).abs() < f32::EPSILON);

        let pillar = world
            .sweep_circle(
                SimulationVector::new(9.7, 6.5),
                SimulationVector::new(0.4, 0.0),
                0.1,
            )
            .expect("sweep")
            .expect("hit");
        assert_eq!(
            pillar.target,
            CollisionTarget::Solid(SolidColliderId::Pillar(0))
        );
        assert!((pillar.fraction - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn rounded_pillar_corner_distinguishes_near_miss_hit_and_tangent() {
        let world = ProjectileCollisionWorld::new(&arena(), vec![]).expect("world");
        assert!(
            world
                .sweep_circle(
                    SimulationVector::new(9.8, 4.8),
                    SimulationVector::new(0.1, 0.0),
                    0.1,
                )
                .expect("near miss")
                .is_none()
        );
        let tangent = world
            .sweep_circle(
                SimulationVector::new(9.5, 4.9),
                SimulationVector::new(0.6, 0.0),
                0.1,
            )
            .expect("tangent")
            .expect("hit");
        assert_eq!(
            tangent.target,
            CollisionTarget::Solid(SolidColliderId::Pillar(0))
        );
        assert!((tangent.fraction - (0.5 / 0.6)).abs() < 1.0e-3);
    }

    #[test]
    fn enemy_sweep_catches_tunneling_tangent_and_initial_overlap() {
        let target = enemy(9, SimulationVector::new(5.0, 12.0), 0.15);
        let world = ProjectileCollisionWorld::new(&arena(), vec![target]).expect("world");
        let hit = world
            .sweep_circle(
                SimulationVector::new(4.0, 12.0),
                SimulationVector::new(2.0, 0.0),
                0.1,
            )
            .expect("sweep")
            .expect("hit");
        assert_eq!(hit.target, CollisionTarget::Enemy(target.id()));
        assert!((hit.fraction - 0.375).abs() < 1.0e-6);

        let tangent = world
            .sweep_circle(
                SimulationVector::new(4.0, 11.75),
                SimulationVector::new(2.0, 0.0),
                0.1,
            )
            .expect("tangent")
            .expect("hit");
        assert_eq!(tangent.target, CollisionTarget::Enemy(target.id()));

        let overlap = world
            .sweep_circle(target.center(), SimulationVector::new(0.4, 0.0), 0.1)
            .expect("overlap")
            .expect("hit");
        assert!(overlap.fraction.abs() < f32::EPSILON);
    }

    #[test]
    fn exact_ties_are_solid_first_then_stable_id() {
        let touching_pillar = enemy(2, SimulationVector::new(10.34, 6.5), 0.34);
        let same_enemy = enemy(1, touching_pillar.center(), touching_pillar.radius_tiles());
        let world = ProjectileCollisionWorld::new(&arena(), vec![touching_pillar, same_enemy])
            .expect_err("enemy overlap with pillar is invalid");
        assert_eq!(world, CollisionError::EnemyOverlapsSolid(same_enemy.id()));

        let low = enemy(1, SimulationVector::new(8.0, 12.0), 0.34);
        let high = enemy(2, low.center(), low.radius_tiles());
        let world = ProjectileCollisionWorld::new(&arena(), vec![high, low]).expect("world");
        let hit = world
            .sweep_circle(
                SimulationVector::new(7.0, 12.0),
                SimulationVector::new(2.0, 0.0),
                0.1,
            )
            .expect("sweep")
            .expect("hit");
        assert_eq!(hit.target, CollisionTarget::Enemy(low.id()));

        let mut best = Some(SweepHit {
            fraction: 0.5,
            target: CollisionTarget::Enemy(low.id()),
        });
        consider_hit(
            &mut best,
            SweepHit {
                fraction: 0.5,
                target: CollisionTarget::Solid(SolidColliderId::Pillar(3)),
            },
        );
        assert_eq!(
            best.expect("tie winner").target,
            CollisionTarget::Solid(SolidColliderId::Pillar(3))
        );
    }

    #[test]
    fn world_rejects_duplicate_invalid_and_solid_overlapping_hurtboxes() {
        let duplicate = enemy(1, SimulationVector::new(8.0, 12.0), 0.34);
        assert_eq!(
            ProjectileCollisionWorld::new(&arena(), vec![duplicate, duplicate]),
            Err(CollisionError::DuplicateEnemyId(duplicate.id()))
        );
        let overlapping = enemy(3, SimulationVector::new(10.1, 6.0), 0.34);
        assert_eq!(
            ProjectileCollisionWorld::new(&arena(), vec![overlapping]),
            Err(CollisionError::EnemyOverlapsSolid(overlapping.id()))
        );
        assert!(
            EnemyHurtbox::new(
                EntityId::new(4).expect("ID"),
                SimulationVector::new(f32::NAN, 0.0),
                0.2
            )
            .is_err()
        );
    }
}
