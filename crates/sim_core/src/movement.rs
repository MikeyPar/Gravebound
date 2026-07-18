use thiserror::Error;

use crate::{
    ArenaGeometry, BodyCollisionWorld, CollisionError, CollisionTarget, EntityId,
    MILLI_TILES_PER_TILE, ProjectileCollisionWorld, SweepHit, TICKS_PER_SECOND, TilePoint,
    TileRectangle,
};

/// Grave Arbalist movement speed from `CLS-020`.
pub const GRAVE_ARBALIST_SPEED_TILES_PER_SECOND: f32 = 5.1;
/// Physical player collision radius from `SIM-005`.
pub const PLAYER_COLLISION_RADIUS_MILLI_TILES: i32 = 300;
pub const PLAYER_COLLISION_RADIUS_TILES: f32 = 0.30;
/// `60 ms` rounded to the nearest 30 Hz duration under `CONT-010`.
pub const MOVEMENT_RESPONSE_TICKS: u32 = 2;
const COLLISION_PASSES: usize = 4;
const CONTACT_EPSILON: f32 = 1.0e-6;
const NETWORK_QUANTIZATION_CONTACT_TOLERANCE_TILES: f32 = 0.001;
const TICKS_PER_SECOND_F32: f32 = 30.0;

/// Renderer-independent vector in northwest-authored simulation coordinates.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct SimulationVector {
    pub x: f32,
    pub y: f32,
}

impl SimulationVector {
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    #[must_use]
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    #[must_use]
    pub const fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    #[must_use]
    pub const fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    pub(crate) fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }
}

impl std::ops::Add for SimulationVector {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl std::ops::Sub for SimulationVector {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl std::ops::Mul<f32> for SimulationVector {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

/// Compact latest-state digital movement action. Opposing bindings cancel on their axis.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MovementAction {
    horizontal_milli: i16,
    vertical_milli: i16,
}

impl MovementAction {
    #[must_use]
    pub fn new(horizontal: i8, vertical: i8) -> Self {
        Self {
            horizontal_milli: i16::from(horizontal.clamp(-1, 1)) * 1_000,
            vertical_milli: i16::from(vertical.clamp(-1, 1)) * 1_000,
        }
    }

    /// Creates a bounded analog action from the shared network fixed-point scale.
    pub fn try_from_milli(horizontal: i16, vertical: i16) -> Result<Self, MovementError> {
        if !(-1_000..=1_000).contains(&horizontal) || !(-1_000..=1_000).contains(&vertical) {
            return Err(MovementError::InputOutOfRange);
        }
        Ok(Self {
            horizontal_milli: horizontal,
            vertical_milli: vertical,
        })
    }

    /// Scales bounded player intent without changing the configured movement speed. This is used
    /// by authoritative state such as Emergency Recall, where the GDD permits a fixed fraction of
    /// ordinary movement while leaving class/content movement facts unchanged.
    pub fn scaled_basis_points(self, basis_points: u16) -> Result<Self, MovementError> {
        if basis_points > 10_000 {
            return Err(MovementError::ScaleOutOfRange);
        }
        let scale = i32::from(basis_points);
        let horizontal = i32::from(self.horizontal_milli) * scale / 10_000;
        let vertical = i32::from(self.vertical_milli) * scale / 10_000;
        Self::try_from_milli(
            i16::try_from(horizontal).map_err(|_| MovementError::InputOutOfRange)?,
            i16::try_from(vertical).map_err(|_| MovementError::InputOutOfRange)?,
        )
    }

    #[must_use]
    pub fn normalized_vector(self) -> SimulationVector {
        let vector = SimulationVector::new(
            f32::from(self.horizontal_milli) / 1_000.0,
            f32::from(self.vertical_milli) / 1_000.0,
        );
        if vector.length_squared() > 1.0 {
            let inverse_length = vector.length().recip();
            vector * inverse_length
        } else {
            vector
        }
    }
}

/// Fixed gameplay values selected by the class and global movement contracts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerMovementConfig {
    pub final_speed_tiles_per_second: f32,
    pub response_ticks: u32,
    pub collision_radius_tiles: f32,
}

impl Default for PlayerMovementConfig {
    fn default() -> Self {
        Self {
            final_speed_tiles_per_second: GRAVE_ARBALIST_SPEED_TILES_PER_SECOND,
            response_ticks: MOVEMENT_RESPONSE_TICKS,
            collision_radius_tiles: PLAYER_COLLISION_RADIUS_TILES,
        }
    }
}

/// Authoritative fixed-tick player movement state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerMovementState {
    position: SimulationVector,
    velocity: SimulationVector,
    config: PlayerMovementConfig,
}

impl PlayerMovementState {
    pub fn at_arena_spawn(arena: &ArenaGeometry) -> Result<Self, MovementError> {
        Self::new(tile_point_to_simulation(arena.player_spawn), arena)
    }

    pub fn new(position: SimulationVector, arena: &ArenaGeometry) -> Result<Self, MovementError> {
        Self::new_with_config(position, PlayerMovementConfig::default(), arena)
    }

    pub fn new_with_config(
        position: SimulationVector,
        config: PlayerMovementConfig,
        arena: &ArenaGeometry,
    ) -> Result<Self, MovementError> {
        let state = Self {
            position,
            velocity: SimulationVector::default(),
            config,
        };
        state.validate(arena)?;
        Ok(state)
    }

    /// Restores a server-authenticated movement state before client-side input replay.
    pub fn from_authoritative_snapshot(
        position: SimulationVector,
        velocity: SimulationVector,
        config: PlayerMovementConfig,
        arena: &ArenaGeometry,
    ) -> Result<Self, MovementError> {
        if !movement_config_is_valid(config) {
            return Err(MovementError::InvalidConfig);
        }
        if velocity.length() > config.final_speed_tiles_per_second + CONTACT_EPSILON {
            return Err(MovementError::VelocityExceedsMaximum);
        }
        let mut state = Self {
            position,
            velocity,
            config,
        };
        if !state.position.is_finite() || !state.velocity.is_finite() {
            return Err(MovementError::NonFiniteState);
        }
        state.resolve_solids(arena);
        if (state.position - position).length() > NETWORK_QUANTIZATION_CONTACT_TOLERANCE_TILES {
            return Err(MovementError::IllegalPosition);
        }
        state.validate(arena)?;
        Ok(state)
    }

    #[must_use]
    pub const fn position(self) -> SimulationVector {
        self.position
    }

    #[must_use]
    pub const fn velocity(self) -> SimulationVector {
        self.velocity
    }

    #[must_use]
    pub const fn config(self) -> PlayerMovementConfig {
        self.config
    }

    /// Advances exactly one authoritative 30 Hz tick.
    pub fn step(
        &mut self,
        action: MovementAction,
        arena: &ArenaGeometry,
    ) -> Result<MovementStep, MovementError> {
        self.validate(arena)?;
        let target_velocity = action.normalized_vector() * self.config.final_speed_tiles_per_second;
        debug_assert_eq!(TICKS_PER_SECOND, 30);
        debug_assert_eq!(self.config.response_ticks, MOVEMENT_RESPONSE_TICKS);
        let maximum_velocity_change = self.config.final_speed_tiles_per_second * 0.5;
        self.velocity = move_towards(self.velocity, target_velocity, maximum_velocity_change);

        let tick_displacement = self.velocity * (1.0 / TICKS_PER_SECOND_F32);
        let maximum_substep = self.config.collision_radius_tiles * 0.5;
        let (substep_count, substep_scale) = if tick_displacement.length() > maximum_substep {
            (2, 0.5)
        } else {
            (1, 1.0)
        };
        let substep = tick_displacement * substep_scale;
        let mut collided = false;

        for _ in 0..substep_count {
            self.position = self.position + substep;
            collided |= self.resolve_solids(arena);
        }

        if !self.position.is_finite() || !self.velocity.is_finite() {
            return Err(MovementError::NonFiniteState);
        }
        Ok(MovementStep {
            position: self.position,
            velocity: self.velocity,
            collided,
        })
    }

    /// Advances walking against immutable arena solids and a distinct dynamic body snapshot.
    pub fn step_with_bodies(
        &mut self,
        action: MovementAction,
        arena: &ArenaGeometry,
        bodies: &BodyCollisionWorld,
    ) -> Result<MovementStep, MovementError> {
        let mut staged = *self;
        let start = staged.position;
        let mut step = staged.step(action, arena)?;
        let displacement = step.position - start;
        if let Some(hit) =
            bodies.sweep_circle(start, displacement, staged.config.collision_radius_tiles)?
        {
            staged.position = start + displacement * hit.fraction;
            staged.velocity = SimulationVector::default();
            staged.validate(arena)?;
            step = MovementStep {
                position: staged.position,
                velocity: staged.velocity,
                collided: true,
            };
        }
        *self = staged;
        Ok(step)
    }

    /// Applies one authoritative movement-ability segment, stopping exactly at the first solid.
    pub fn apply_forced_displacement(
        &mut self,
        displacement: SimulationVector,
        collision_world: &ProjectileCollisionWorld,
        arena: &ArenaGeometry,
    ) -> Result<ForcedMovementStep, MovementError> {
        self.apply_forced_displacement_inner(displacement, collision_world, None, arena)
    }

    pub fn apply_forced_displacement_with_bodies(
        &mut self,
        displacement: SimulationVector,
        collision_world: &ProjectileCollisionWorld,
        bodies: &BodyCollisionWorld,
        arena: &ArenaGeometry,
    ) -> Result<ForcedMovementStep, MovementError> {
        self.apply_forced_displacement_inner(displacement, collision_world, Some(bodies), arena)
    }

    fn apply_forced_displacement_inner(
        &mut self,
        displacement: SimulationVector,
        collision_world: &ProjectileCollisionWorld,
        bodies: Option<&BodyCollisionWorld>,
        arena: &ArenaGeometry,
    ) -> Result<ForcedMovementStep, MovementError> {
        self.validate(arena)?;
        if !displacement.is_finite() {
            return Err(MovementError::NonFiniteState);
        }
        let start = self.position;
        let solid_hit = collision_world.sweep_solids(
            self.position,
            displacement,
            self.config.collision_radius_tiles,
        )?;
        let body_hit = bodies
            .map(|world| {
                world.sweep_circle(
                    self.position,
                    displacement,
                    self.config.collision_radius_tiles,
                )
            })
            .transpose()?
            .flatten();
        let hit = earliest_hit(solid_hit, body_hit);
        let fraction = hit.map_or(1.0, |hit| hit.fraction);
        let solid = hit.and_then(|hit| match hit.target {
            CollisionTarget::Solid(solid) => Some(solid),
            CollisionTarget::Enemy(_) => None,
        });
        let body = hit.and_then(|hit| match hit.target {
            CollisionTarget::Enemy(body) => Some(body),
            CollisionTarget::Solid(_) => None,
        });
        self.position = self.position + displacement * fraction;
        self.velocity = SimulationVector::default();
        // Closed-form sweep contact can land a few floating-point ULPs inside a rounded solid.
        // Resolve only a reported solid contact back to the legal boundary before validation.
        if solid.is_some() {
            self.resolve_solids(arena);
        }
        self.validate(arena)?;
        Ok(ForcedMovementStep {
            position: self.position,
            travelled_tiles: (self.position - start).length(),
            solid,
            body,
        })
    }

    fn validate(self, arena: &ArenaGeometry) -> Result<(), MovementError> {
        if !self.position.is_finite() || !self.velocity.is_finite() {
            return Err(MovementError::NonFiniteState);
        }
        if !movement_config_is_valid(self.config) {
            return Err(MovementError::InvalidConfig);
        }
        if !position_is_legal(self.position, self.config.collision_radius_tiles, arena) {
            return Err(MovementError::IllegalPosition);
        }
        Ok(())
    }

    fn resolve_shell(&mut self, arena: &ArenaGeometry) -> bool {
        let radius = self.config.collision_radius_tiles;
        let width = milli_to_tiles(arena.width_milli_tiles);
        let height = milli_to_tiles(arena.height_milli_tiles);
        let mut collided = false;
        if self.position.x < radius {
            self.position.x = radius;
            remove_inward_velocity(&mut self.velocity, SimulationVector::new(1.0, 0.0));
            collided = true;
        } else if self.position.x > width - radius {
            self.position.x = width - radius;
            remove_inward_velocity(&mut self.velocity, SimulationVector::new(-1.0, 0.0));
            collided = true;
        }
        if self.position.y < radius {
            self.position.y = radius;
            remove_inward_velocity(&mut self.velocity, SimulationVector::new(0.0, 1.0));
            collided = true;
        } else if self.position.y > height - radius {
            self.position.y = height - radius;
            remove_inward_velocity(&mut self.velocity, SimulationVector::new(0.0, -1.0));
            collided = true;
        }
        collided
    }

    fn resolve_solids(&mut self, arena: &ArenaGeometry) -> bool {
        let mut collided = self.resolve_shell(arena);
        for _ in 0..COLLISION_PASSES {
            let mut pass_collision = false;
            for pillar in &arena.pillars {
                if let Some((normal, depth)) = circle_rectangle_contact(
                    self.position,
                    *pillar,
                    self.config.collision_radius_tiles,
                ) {
                    self.position = self.position + normal * (depth + CONTACT_EPSILON);
                    remove_inward_velocity(&mut self.velocity, normal);
                    pass_collision = true;
                }
            }
            collided |= pass_collision;
            if !pass_collision {
                break;
            }
        }
        collided
    }
}

fn movement_config_is_valid(config: PlayerMovementConfig) -> bool {
    config.final_speed_tiles_per_second.is_finite()
        && config.final_speed_tiles_per_second > 0.0
        && config.response_ticks != 0
        && config.collision_radius_tiles.is_finite()
        && config.collision_radius_tiles > 0.0
}

/// Observable result from one fixed movement tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovementStep {
    pub position: SimulationVector,
    pub velocity: SimulationVector,
    pub collided: bool,
}

/// Result of a nonwalking movement segment such as Slipstep.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ForcedMovementStep {
    pub position: SimulationVector,
    pub travelled_tiles: f32,
    pub solid: Option<crate::SolidColliderId>,
    pub body: Option<EntityId>,
}

fn earliest_hit(first: Option<SweepHit>, second: Option<SweepHit>) -> Option<SweepHit> {
    match (first, second) {
        (Some(first), Some(second)) => Some(
            if first
                .fraction
                .total_cmp(&second.fraction)
                .then_with(|| first.target.cmp(&second.target))
                .is_le()
            {
                first
            } else {
                second
            },
        ),
        (Some(hit), None) | (None, Some(hit)) => Some(hit),
        (None, None) => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MovementError {
    #[error("movement input components must remain within -1000..=1000")]
    InputOutOfRange,
    #[error("movement intent scale must remain within 0..=10000 basis points")]
    ScaleOutOfRange,
    #[error("movement state contains a non-finite value")]
    NonFiniteState,
    #[error("movement configuration is invalid")]
    InvalidConfig,
    #[error("authoritative movement velocity exceeds configured final speed")]
    VelocityExceedsMaximum,
    #[error("simulation position cannot be represented in fixed-point world coordinates")]
    PositionOutOfRange,
    #[error("player position intersects the arena shell or a solid pillar")]
    IllegalPosition,
    #[error(transparent)]
    Collision(#[from] CollisionError),
}

#[must_use]
pub fn tile_point_to_simulation(point: TilePoint) -> SimulationVector {
    SimulationVector::new(
        milli_to_tiles(point.x_milli_tiles),
        milli_to_tiles(point.y_milli_tiles),
    )
}

/// Quantizes a finite simulation position into the shared millitile world projection. Gameplay
/// authority remains in simulation space; this conversion is the one canonical route/wire view.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub fn simulation_to_tile_point(position: SimulationVector) -> Result<TilePoint, MovementError> {
    if !position.is_finite() {
        return Err(MovementError::NonFiniteState);
    }
    let x = (position.x * MILLI_TILES_PER_TILE as f32).round();
    let y = (position.y * MILLI_TILES_PER_TILE as f32).round();
    if x < i32::MIN as f32 || x > i32::MAX as f32 || y < i32::MIN as f32 || y > i32::MAX as f32 {
        return Err(MovementError::PositionOutOfRange);
    }
    Ok(TilePoint::new(x as i32, y as i32))
}

fn move_towards(
    current: SimulationVector,
    target: SimulationVector,
    maximum_delta: f32,
) -> SimulationVector {
    let delta = target - current;
    let distance = delta.length();
    if distance <= maximum_delta || distance <= CONTACT_EPSILON {
        target
    } else {
        current + delta * (maximum_delta / distance)
    }
}

fn remove_inward_velocity(velocity: &mut SimulationVector, outward_normal: SimulationVector) {
    let inward_speed = velocity.dot(outward_normal);
    if inward_speed < 0.0 {
        *velocity = *velocity - outward_normal * inward_speed;
        if velocity.x.abs() < CONTACT_EPSILON {
            velocity.x = 0.0;
        }
        if velocity.y.abs() < CONTACT_EPSILON {
            velocity.y = 0.0;
        }
    }
}

fn circle_rectangle_contact(
    center: SimulationVector,
    rectangle: TileRectangle,
    radius: f32,
) -> Option<(SimulationVector, f32)> {
    let left = milli_to_tiles(rectangle.x_milli_tiles);
    let top = milli_to_tiles(rectangle.y_milli_tiles);
    let right = left + milli_to_tiles(rectangle.width_milli_tiles);
    let bottom = top + milli_to_tiles(rectangle.height_milli_tiles);
    let nearest = SimulationVector::new(center.x.clamp(left, right), center.y.clamp(top, bottom));
    let delta = center - nearest;
    let distance_squared = delta.length_squared();
    if distance_squared >= radius * radius {
        return None;
    }
    if distance_squared > CONTACT_EPSILON * CONTACT_EPSILON {
        let distance = distance_squared.sqrt();
        return Some((delta * (1.0 / distance), radius - distance));
    }

    let candidates = [
        (center.x - left, SimulationVector::new(-1.0, 0.0)),
        (right - center.x, SimulationVector::new(1.0, 0.0)),
        (center.y - top, SimulationVector::new(0.0, -1.0)),
        (bottom - center.y, SimulationVector::new(0.0, 1.0)),
    ];
    let (face_distance, normal) = candidates
        .into_iter()
        .min_by(|first, second| first.0.total_cmp(&second.0))
        .expect("four rectangle faces");
    Some((normal, radius + face_distance + CONTACT_EPSILON))
}

fn position_is_legal(position: SimulationVector, radius: f32, arena: &ArenaGeometry) -> bool {
    if position.x < radius
        || position.y < radius
        || position.x > milli_to_tiles(arena.width_milli_tiles) - radius
        || position.y > milli_to_tiles(arena.height_milli_tiles) - radius
    {
        return false;
    }
    arena
        .pillars
        .iter()
        .all(|pillar| circle_rectangle_contact(position, *pillar, radius).is_none())
}

#[allow(clippy::cast_precision_loss)]
fn milli_to_tiles(value: i32) -> f32 {
    value as f32 / MILLI_TILES_PER_TILE as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ArenaGeometry;

    fn arena(
        width: i32,
        height: i32,
        spawn: TilePoint,
        pillars: Vec<TileRectangle>,
    ) -> ArenaGeometry {
        ArenaGeometry {
            id: "arena.movement_test".to_owned(),
            width_milli_tiles: width,
            height_milli_tiles: height,
            shell_thickness_milli_tiles: 1_000,
            player_spawn: spawn,
            boss_spawn: TilePoint::new(width - 1_000, height - 1_000),
            pillars,
            anchors: vec![],
        }
        .validated()
        .expect("movement test arena")
    }

    #[test]
    fn digital_action_clamps_and_normalizes_without_diagonal_gain() {
        assert_eq!(MovementAction::new(3, -7), MovementAction::new(1, -1));
        let cardinal = MovementAction::new(1, 0).normalized_vector();
        let diagonal = MovementAction::new(1, 1).normalized_vector();
        assert!((cardinal.length() - 1.0).abs() < f32::EPSILON);
        assert!((diagonal.length() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn network_fixed_point_preserves_analog_magnitude_and_bounds() {
        let half = MovementAction::try_from_milli(300, -400).expect("bounded analog input");
        assert!((half.normalized_vector().length() - 0.5).abs() < f32::EPSILON);
        let clamped_diagonal =
            MovementAction::try_from_milli(1_000, 1_000).expect("bounded diagonal");
        assert!((clamped_diagonal.normalized_vector().length() - 1.0).abs() < f32::EPSILON);
        assert_eq!(
            MovementAction::try_from_milli(1_001, 0),
            Err(MovementError::InputOutOfRange)
        );
    }

    #[test]
    fn simulation_position_quantizes_once_to_the_shared_millitile_projection() {
        let point = TilePoint::new(8_501, 40_499);
        assert_eq!(
            simulation_to_tile_point(tile_point_to_simulation(point)),
            Ok(point)
        );
        assert_eq!(
            simulation_to_tile_point(SimulationVector::new(f32::NAN, 1.0)),
            Err(MovementError::NonFiniteState)
        );
        assert_eq!(
            simulation_to_tile_point(SimulationVector::new(f32::MAX, 1.0)),
            Err(MovementError::PositionOutOfRange)
        );
    }

    #[test]
    fn response_reaches_exact_class_speed_in_two_ticks() {
        let arena = arena(200_000, 200_000, TilePoint::new(100_000, 100_000), vec![]);
        let mut player = PlayerMovementState::at_arena_spawn(&arena).expect("player");
        player
            .step(MovementAction::new(1, 0), &arena)
            .expect("tick 1");
        assert!((player.velocity().length() - 2.55).abs() < 1.0e-6);
        player
            .step(MovementAction::new(1, 0), &arena)
            .expect("tick 2");
        assert!(
            (player.velocity().length() - GRAVE_ARBALIST_SPEED_TILES_PER_SECOND).abs() < 1.0e-6
        );
        player
            .step(MovementAction::default(), &arena)
            .expect("stop 1");
        player
            .step(MovementAction::default(), &arena)
            .expect("stop 2");
        assert_eq!(player.velocity(), SimulationVector::default());
    }

    #[test]
    fn cardinal_and_diagonal_paths_match_over_equal_tick_counts() {
        let arena = arena(200_000, 200_000, TilePoint::new(100_000, 100_000), vec![]);
        let start = tile_point_to_simulation(arena.player_spawn);
        let mut cardinal = PlayerMovementState::at_arena_spawn(&arena).expect("cardinal");
        let mut diagonal = PlayerMovementState::at_arena_spawn(&arena).expect("diagonal");
        for _ in 0..100 {
            cardinal
                .step(MovementAction::new(1, 0), &arena)
                .expect("cardinal tick");
            diagonal
                .step(MovementAction::new(1, 1), &arena)
                .expect("diagonal tick");
        }
        let cardinal_distance = (cardinal.position() - start).length();
        let diagonal_distance = (diagonal.position() - start).length();
        assert!(
            (cardinal_distance - diagonal_distance).abs() < 5.0e-4,
            "cardinal={cardinal_distance}, diagonal={diagonal_distance}"
        );
    }

    #[test]
    fn shell_and_pillar_reject_sustained_pressure() {
        let arena = arena(
            32_000,
            24_000,
            TilePoint::new(4_000, 12_000),
            vec![TileRectangle::new(10_000, 5_000, 2_000, 3_000)],
        );
        let mut shell = PlayerMovementState::at_arena_spawn(&arena).expect("shell player");
        for _ in 0..200 {
            shell
                .step(MovementAction::new(-1, 0), &arena)
                .expect("shell tick");
        }
        assert!((shell.position().x - PLAYER_COLLISION_RADIUS_TILES).abs() < 1.0e-6);

        let mut pillar = PlayerMovementState::new(SimulationVector::new(8.0, 6.5), &arena)
            .expect("pillar player");
        for _ in 0..100 {
            pillar
                .step(MovementAction::new(1, 0), &arena)
                .expect("pillar tick");
        }
        assert!((pillar.position().x - 9.7).abs() < 1.0e-5);
        assert!(pillar.velocity().x.abs() < 1.0e-6);
    }

    #[test]
    fn walking_uses_physical_body_radius_and_allows_tangent_departure() {
        let arena = arena(20_000, 20_000, TilePoint::new(8_000, 10_000), vec![]);
        let boss = EntityId::new(40_002).expect("boss");
        let bodies = BodyCollisionWorld::new(
            &arena,
            vec![
                crate::EnemyBodyCollider::new(boss, SimulationVector::new(10.0, 10.0), 0.70)
                    .expect("body"),
            ],
        )
        .expect("body world");
        let mut player = PlayerMovementState::at_arena_spawn(&arena).expect("player");
        for _ in 0..20 {
            player
                .step_with_bodies(MovementAction::new(1, 0), &arena, &bodies)
                .expect("body tick");
        }
        assert!((player.position().x - 9.0).abs() < 1.0e-5);
        assert_eq!(player.velocity(), SimulationVector::default());

        player
            .step_with_bodies(MovementAction::new(-1, 0), &arena, &bodies)
            .expect("depart tangent");
        assert!(player.position().x < 9.0);
    }

    #[test]
    fn forced_displacement_stops_at_combined_player_and_body_radius() {
        let arena = arena(20_000, 20_000, TilePoint::new(8_000, 10_000), vec![]);
        let boss = EntityId::new(40_002).expect("boss");
        let bodies = BodyCollisionWorld::new(
            &arena,
            vec![
                crate::EnemyBodyCollider::new(boss, SimulationVector::new(10.0, 10.0), 0.70)
                    .expect("body"),
            ],
        )
        .expect("body world");
        let projectiles = ProjectileCollisionWorld::new(&arena, Vec::new()).expect("world");
        let mut player = PlayerMovementState::at_arena_spawn(&arena).expect("player");
        let moved = player
            .apply_forced_displacement_with_bodies(
                SimulationVector::new(5.0, 0.0),
                &projectiles,
                &bodies,
                &arena,
            )
            .expect("forced movement");
        assert!((moved.position.x - 9.0).abs() < 1.0e-5);
        assert_eq!(moved.body, Some(boss));
        assert_eq!(moved.solid, None);
    }

    #[test]
    fn diagonal_corner_pressure_stays_outside_circle_contact() {
        let pillar = TileRectangle::new(10_000, 5_000, 2_000, 3_000);
        let arena = arena(32_000, 24_000, TilePoint::new(9_000, 4_000), vec![pillar]);
        let mut player = PlayerMovementState::at_arena_spawn(&arena).expect("player");
        for _ in 0..180 {
            player
                .step(MovementAction::new(1, 1), &arena)
                .expect("corner tick");
        }
        assert!(player.position().is_finite());
        assert!(
            circle_rectangle_contact(player.position(), pillar, PLAYER_COLLISION_RADIUS_TILES)
                .is_none()
        );
    }

    #[test]
    fn replayed_input_sequence_is_bit_identical() {
        let arena = arena(80_000, 80_000, TilePoint::new(20_000, 20_000), vec![]);
        let sequence = [
            MovementAction::new(1, 0),
            MovementAction::new(1, 1),
            MovementAction::new(0, 1),
            MovementAction::new(-1, 1),
            MovementAction::default(),
        ];
        let run = || {
            let mut state = PlayerMovementState::at_arena_spawn(&arena).expect("player");
            for action in sequence.into_iter().cycle().take(90) {
                state.step(action, &arena).expect("step");
            }
            [
                state.position().x.to_bits(),
                state.position().y.to_bits(),
                state.velocity().x.to_bits(),
                state.velocity().y.to_bits(),
            ]
        };
        let first = run();
        assert_eq!(first, run());
        assert_eq!(
            first,
            [1_102_371_517, 1_105_337_698, 3_205_476_002, 1_070_197_224]
        );
    }

    #[test]
    fn illegal_or_non_finite_spawn_fails_closed() {
        let arena = arena(
            32_000,
            24_000,
            TilePoint::new(4_000, 12_000),
            vec![TileRectangle::new(10_000, 5_000, 2_000, 3_000)],
        );
        assert_eq!(
            PlayerMovementState::new(SimulationVector::new(10.5, 6.0), &arena),
            Err(MovementError::IllegalPosition)
        );
        assert_eq!(
            PlayerMovementState::new(SimulationVector::new(f32::NAN, 2.0), &arena),
            Err(MovementError::NonFiniteState)
        );
    }

    #[test]
    fn authoritative_millitile_contact_repairs_only_quantization_depth() {
        let arena = arena(32_000, 24_000, TilePoint::new(4_000, 12_000), vec![]);
        let repaired = PlayerMovementState::from_authoritative_snapshot(
            SimulationVector::new(0.2995, 12.0),
            SimulationVector::new(-1.0, 0.0),
            PlayerMovementConfig::default(),
            &arena,
        )
        .expect("sub-millitile shell contact");
        assert!(repaired.position().x >= PLAYER_COLLISION_RADIUS_TILES);
        assert!(repaired.velocity().x.abs() < CONTACT_EPSILON);
        assert_eq!(
            PlayerMovementState::from_authoritative_snapshot(
                SimulationVector::new(0.29, 12.0),
                SimulationVector::default(),
                PlayerMovementConfig::default(),
                &arena,
            ),
            Err(MovementError::IllegalPosition)
        );
    }
}
