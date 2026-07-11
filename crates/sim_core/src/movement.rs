use std::f32::consts::FRAC_1_SQRT_2;

use thiserror::Error;

use crate::{ArenaGeometry, MILLI_TILES_PER_TILE, TICKS_PER_SECOND, TilePoint, TileRectangle};

/// Grave Arbalist movement speed from `CLS-020`.
pub const GRAVE_ARBALIST_SPEED_TILES_PER_SECOND: f32 = 5.1;
/// Physical player collision radius from `SIM-005`.
pub const PLAYER_COLLISION_RADIUS_TILES: f32 = 0.30;
/// `60 ms` rounded to the nearest 30 Hz duration under `CONT-010`.
pub const MOVEMENT_RESPONSE_TICKS: u32 = 2;
const COLLISION_PASSES: usize = 4;
const CONTACT_EPSILON: f32 = 1.0e-6;
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

    fn dot(self, other: Self) -> f32 {
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
    horizontal: i8,
    vertical: i8,
}

impl MovementAction {
    #[must_use]
    pub fn new(horizontal: i8, vertical: i8) -> Self {
        Self {
            horizontal: horizontal.clamp(-1, 1),
            vertical: vertical.clamp(-1, 1),
        }
    }

    #[must_use]
    pub fn normalized_vector(self) -> SimulationVector {
        let x = f32::from(self.horizontal);
        let y = f32::from(self.vertical);
        if self.horizontal != 0 && self.vertical != 0 {
            SimulationVector::new(x * FRAC_1_SQRT_2, y * FRAC_1_SQRT_2)
        } else {
            SimulationVector::new(x, y)
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
        let state = Self {
            position,
            velocity: SimulationVector::default(),
            config: PlayerMovementConfig::default(),
        };
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
            collided |= self.resolve_shell(arena);
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

    fn validate(self, arena: &ArenaGeometry) -> Result<(), MovementError> {
        if !self.position.is_finite() || !self.velocity.is_finite() {
            return Err(MovementError::NonFiniteState);
        }
        if !self.config.final_speed_tiles_per_second.is_finite()
            || self.config.final_speed_tiles_per_second <= 0.0
            || self.config.response_ticks == 0
            || !self.config.collision_radius_tiles.is_finite()
            || self.config.collision_radius_tiles <= 0.0
        {
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
}

/// Observable result from one fixed movement tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovementStep {
    pub position: SimulationVector,
    pub velocity: SimulationVector,
    pub collided: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MovementError {
    #[error("movement state contains a non-finite value")]
    NonFiniteState,
    #[error("movement configuration is invalid")]
    InvalidConfig,
    #[error("player position intersects the arena shell or a solid pillar")]
    IllegalPosition,
}

#[must_use]
pub fn tile_point_to_simulation(point: TilePoint) -> SimulationVector {
    SimulationVector::new(
        milli_to_tiles(point.x_milli_tiles),
        milli_to_tiles(point.y_milli_tiles),
    )
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
        assert_eq!(run(), run());
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
}
