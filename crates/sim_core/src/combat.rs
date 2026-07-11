use thiserror::Error;

use crate::{
    CollisionError, CollisionTarget, EntityId, EntityIdAllocator, ProjectileCollisionWorld,
    SimulationVector, TICKS_PER_SECOND, Tick, WeaponDefinition,
};

const AIM_EPSILON_SQUARED: f32 = 1.0e-12;
const RANGE_EPSILON: f32 = 1.0e-6;
const TICKS_PER_SECOND_F32: f32 = 30.0;

/// Finite normalized aim in northwest-authored simulation coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AimDirection(SimulationVector);

impl AimDirection {
    pub fn new(vector: SimulationVector) -> Result<Self, AimDirectionError> {
        if !vector.is_finite() {
            return Err(AimDirectionError::NonFinite);
        }
        let length_squared = vector.length_squared();
        if length_squared <= AIM_EPSILON_SQUARED {
            return Err(AimDirectionError::ZeroLength);
        }
        let inverse_length = length_squared.sqrt().recip();
        Ok(Self(vector * inverse_length))
    }

    #[must_use]
    pub const fn east() -> Self {
        Self(SimulationVector::new(1.0, 0.0))
    }

    #[must_use]
    pub const fn vector(self) -> SimulationVector {
        self.0
    }
}

impl Default for AimDirection {
    fn default() -> Self {
        Self::east()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AimDirectionError {
    #[error("aim direction must be finite")]
    NonFinite,
    #[error("aim direction must have nonzero length")]
    ZeroLength,
}

/// Latest compact combat action sampled by the client.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct CombatAction {
    pub aim: AimDirection,
    pub primary_held: bool,
    pub primary_press_sequence: u32,
}

/// One authoritative friendly projectile. Collision is added by `GB-M01-02B`.
#[derive(Debug, Clone, PartialEq)]
pub struct FriendlyProjectile {
    id: EntityId,
    position: SimulationVector,
    origin: SimulationVector,
    direction: AimDirection,
    distance_travelled_tiles: f32,
    range_tiles: f32,
    speed_tiles_per_second: f32,
    radius_tiles: f32,
    raw_damage: u32,
    pierce_remaining: u32,
    stops_on_first_enemy: bool,
}

impl FriendlyProjectile {
    #[must_use]
    pub const fn id(&self) -> EntityId {
        self.id
    }

    #[must_use]
    pub const fn position(&self) -> SimulationVector {
        self.position
    }

    #[must_use]
    pub const fn origin(&self) -> SimulationVector {
        self.origin
    }

    #[must_use]
    pub const fn direction(&self) -> AimDirection {
        self.direction
    }

    #[must_use]
    pub const fn distance_travelled_tiles(&self) -> f32 {
        self.distance_travelled_tiles
    }

    #[must_use]
    pub const fn range_tiles(&self) -> f32 {
        self.range_tiles
    }

    #[must_use]
    pub const fn speed_tiles_per_second(&self) -> f32 {
        self.speed_tiles_per_second
    }

    #[must_use]
    pub const fn radius_tiles(&self) -> f32 {
        self.radius_tiles
    }

    #[must_use]
    pub const fn raw_damage(&self) -> u32 {
        self.raw_damage
    }

    #[must_use]
    pub const fn pierce_remaining(&self) -> u32 {
        self.pierce_remaining
    }

    #[must_use]
    pub const fn stops_on_first_enemy(&self) -> bool {
        self.stops_on_first_enemy
    }
}

/// Shot event used for prediction/presentation without inspecting Bevy entities.
#[derive(Debug, Clone, PartialEq)]
pub struct ShotEvent {
    pub tick: Tick,
    pub press_sequence: u32,
    pub projectile: FriendlyProjectile,
}

/// Exact terminal range event. The projectile is no longer active after this step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectileExpired {
    pub tick: Tick,
    pub projectile_id: EntityId,
    pub final_position: SimulationVector,
    pub distance_travelled_tiles: f32,
}

/// One authoritative projectile terminal contact. `(tick, projectile_id)` is run-locally unique.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectileCollision {
    pub tick: Tick,
    pub projectile_id: EntityId,
    pub target: CollisionTarget,
    pub final_position: SimulationVector,
    pub distance_travelled_tiles: f32,
}

/// Events and state summary produced by one fixed combat tick.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct CombatStep {
    pub tick: Tick,
    pub shots: Vec<ShotEvent>,
    pub collisions: Vec<ProjectileCollision>,
    pub expirations: Vec<ProjectileExpired>,
}

/// Simulation-owned primary weapon timer and projectile collection.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerCombatState {
    weapon: WeaponDefinition,
    tick: Tick,
    interval_remaining_ticks: u32,
    last_press_sequence: u32,
    previous_primary_held: bool,
    projectile_ids: EntityIdAllocator,
    projectiles: Vec<FriendlyProjectile>,
}

impl PlayerCombatState {
    pub fn new(weapon: WeaponDefinition) -> Result<Self, CombatError> {
        Self::with_projectile_allocator(weapon, EntityIdAllocator::default())
    }

    pub fn with_projectile_allocator(
        weapon: WeaponDefinition,
        projectile_ids: EntityIdAllocator,
    ) -> Result<Self, CombatError> {
        if weapon.projectile_count() != 1 {
            return Err(CombatError::UnsupportedProjectileCount(
                weapon.projectile_count(),
            ));
        }
        Ok(Self {
            weapon,
            tick: Tick(0),
            interval_remaining_ticks: 0,
            last_press_sequence: 0,
            previous_primary_held: false,
            projectile_ids,
            projectiles: Vec::new(),
        })
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    #[must_use]
    pub const fn interval_remaining_ticks(&self) -> u32 {
        self.interval_remaining_ticks
    }

    #[must_use]
    pub const fn last_press_sequence(&self) -> u32 {
        self.last_press_sequence
    }

    #[must_use]
    pub const fn previous_primary_held(&self) -> bool {
        self.previous_primary_held
    }

    #[must_use]
    pub fn weapon(&self) -> &WeaponDefinition {
        &self.weapon
    }

    #[must_use]
    pub fn projectiles(&self) -> &[FriendlyProjectile] {
        &self.projectiles
    }

    /// Advances one transactionally committed authoritative combat tick.
    pub fn step(
        &mut self,
        action: CombatAction,
        player_position: SimulationVector,
        collision_world: &ProjectileCollisionWorld,
    ) -> Result<CombatStep, CombatError> {
        let mut next = self.clone();
        let result = next.step_inner(action, player_position, collision_world)?;
        *self = next;
        Ok(result)
    }

    fn step_inner(
        &mut self,
        action: CombatAction,
        player_position: SimulationVector,
        collision_world: &ProjectileCollisionWorld,
    ) -> Result<CombatStep, CombatError> {
        if !player_position.is_finite() {
            return Err(CombatError::NonFinitePlayerPosition);
        }
        self.validate_sequence(action)?;
        self.tick = self.tick.checked_next().ok_or(CombatError::TickExhausted)?;
        let mut step = CombatStep {
            tick: self.tick,
            ..CombatStep::default()
        };
        self.advance_projectiles(collision_world, &mut step.collisions, &mut step.expirations)?;
        self.interval_remaining_ticks = self.interval_remaining_ticks.saturating_sub(1);

        let new_press = action.primary_press_sequence > self.last_press_sequence;
        if new_press {
            self.last_press_sequence = action.primary_press_sequence;
        }
        self.previous_primary_held = action.primary_held;
        if (action.primary_held || new_press) && self.interval_remaining_ticks == 0 {
            let projectile_id = self
                .projectile_ids
                .allocate()
                .ok_or(CombatError::ProjectileIdExhausted)?;
            let projectile = FriendlyProjectile {
                id: projectile_id,
                position: player_position,
                origin: player_position,
                direction: action.aim,
                distance_travelled_tiles: 0.0,
                range_tiles: self.weapon.range_tiles(),
                speed_tiles_per_second: self.weapon.projectile_speed_tiles_per_second(),
                radius_tiles: self.weapon.projectile_radius_tiles(),
                raw_damage: self.weapon.raw_damage(),
                pierce_remaining: self.weapon.pierce(),
                stops_on_first_enemy: self.weapon.stops_on_first_enemy(),
            };
            self.projectiles.push(projectile.clone());
            step.shots.push(ShotEvent {
                tick: self.tick,
                press_sequence: action.primary_press_sequence,
                projectile,
            });
            self.interval_remaining_ticks = self.weapon.attack_interval_ticks();
        }
        Ok(step)
    }

    fn validate_sequence(&self, action: CombatAction) -> Result<(), CombatError> {
        if action.primary_press_sequence < self.last_press_sequence {
            return Err(CombatError::StalePressSequence {
                received: action.primary_press_sequence,
                last: self.last_press_sequence,
            });
        }
        let rising_edge = action.primary_held && !self.previous_primary_held;
        if rising_edge && action.primary_press_sequence == self.last_press_sequence {
            return Err(CombatError::MissingPressSequence {
                sequence: action.primary_press_sequence,
            });
        }
        Ok(())
    }

    fn advance_projectiles(
        &mut self,
        collision_world: &ProjectileCollisionWorld,
        collisions: &mut Vec<ProjectileCollision>,
        expirations: &mut Vec<ProjectileExpired>,
    ) -> Result<(), CombatError> {
        for projectile in &mut self.projectiles {
            let remaining = projectile.range_tiles - projectile.distance_travelled_tiles;
            debug_assert_eq!(TICKS_PER_SECOND, 30);
            let full_step = projectile.speed_tiles_per_second / TICKS_PER_SECOND_F32;
            let travel = full_step.min(remaining.max(0.0));
            let displacement = projectile.direction.vector() * travel;
            if let Some(hit) = collision_world.sweep_circle(
                projectile.position,
                displacement,
                projectile.radius_tiles,
            )? {
                let realized_travel = travel * hit.fraction;
                projectile.position = projectile.position + displacement * hit.fraction;
                projectile.distance_travelled_tiles += realized_travel;
                if !projectile.position.is_finite()
                    || !projectile.distance_travelled_tiles.is_finite()
                {
                    return Err(CombatError::NonFiniteCollisionResult);
                }
                collisions.push(ProjectileCollision {
                    tick: self.tick,
                    projectile_id: projectile.id,
                    target: hit.target,
                    final_position: projectile.position,
                    distance_travelled_tiles: projectile.distance_travelled_tiles,
                });
                continue;
            }
            projectile.position = projectile.position + displacement;
            projectile.distance_travelled_tiles += travel;
            if remaining <= full_step + RANGE_EPSILON {
                projectile.distance_travelled_tiles = projectile.range_tiles;
                expirations.push(ProjectileExpired {
                    tick: self.tick,
                    projectile_id: projectile.id,
                    final_position: projectile.position,
                    distance_travelled_tiles: projectile.range_tiles,
                });
            }
        }
        if !collisions.is_empty() || !expirations.is_empty() {
            self.projectiles.retain(|projectile| {
                collisions
                    .iter()
                    .all(|collision| collision.projectile_id != projectile.id)
                    && expirations
                        .iter()
                        .all(|expiration| expiration.projectile_id != projectile.id)
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CombatError {
    #[error("only single-projectile primary patterns are supported, received {0}")]
    UnsupportedProjectileCount(u32),
    #[error("player position must be finite")]
    NonFinitePlayerPosition,
    #[error("combat simulation tick exhausted u64")]
    TickExhausted,
    #[error("projectile entity ID space is exhausted")]
    ProjectileIdExhausted,
    #[error("stale primary press sequence {received}; last accepted is {last}")]
    StalePressSequence { received: u32, last: u32 },
    #[error("rising primary edge reused press sequence {sequence}")]
    MissingPressSequence { sequence: u32 },
    #[error("projectile collision failed: {0}")]
    Collision(#[from] CollisionError),
    #[error("projectile collision produced non-finite terminal state")]
    NonFiniteCollisionResult,
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::*;
    use crate::{
        ArenaGeometry, EnemyHurtbox, TilePoint, TileRectangle, WeaponDefinition,
        WeaponDefinitionParameters,
    };

    fn pine_crossbow() -> WeaponDefinition {
        WeaponDefinition::new(WeaponDefinitionParameters {
            content_id: "item.prototype.weapon.pine_crossbow".to_owned(),
            raw_damage: 20,
            attack_interval_ticks: 14,
            range_milli_tiles: 9_500,
            projectile_speed_milli_tiles_per_second: 12_000,
            projectile_radius_milli_tiles: 100,
            projectile_count: 1,
            pierce: 0,
            stops_on_first_enemy: true,
        })
        .expect("Pine Crossbow")
    }

    fn empty_world() -> ProjectileCollisionWorld {
        let arena = ArenaGeometry {
            id: "arena.combat_test".to_owned(),
            width_milli_tiles: 100_000,
            height_milli_tiles: 100_000,
            shell_thickness_milli_tiles: 1_000,
            player_spawn: TilePoint::new(4_000, 12_000),
            boss_spawn: TilePoint::new(80_000, 80_000),
            pillars: vec![],
            anchors: vec![],
        }
        .validated()
        .expect("arena");
        ProjectileCollisionWorld::new(&arena, vec![]).expect("collision world")
    }

    fn world_with(
        pillars: Vec<TileRectangle>,
        enemies: Vec<EnemyHurtbox>,
    ) -> ProjectileCollisionWorld {
        let arena = ArenaGeometry {
            id: "arena.combat_collision_test".to_owned(),
            width_milli_tiles: 100_000,
            height_milli_tiles: 100_000,
            shell_thickness_milli_tiles: 1_000,
            player_spawn: TilePoint::new(4_000, 12_000),
            boss_spawn: TilePoint::new(80_000, 80_000),
            pillars,
            anchors: vec![],
        }
        .validated()
        .expect("arena");
        ProjectileCollisionWorld::new(&arena, enemies).expect("collision world")
    }

    fn held(sequence: u32, aim: AimDirection) -> CombatAction {
        CombatAction {
            aim,
            primary_held: true,
            primary_press_sequence: sequence,
        }
    }

    fn released(sequence: u32, aim: AimDirection) -> CombatAction {
        CombatAction {
            aim,
            primary_held: false,
            primary_press_sequence: sequence,
        }
    }

    #[test]
    fn aim_normalizes_and_rejects_ambiguous_values() {
        let diagonal = AimDirection::new(SimulationVector::new(3.0, 4.0)).expect("aim");
        assert!((diagonal.vector().length() - 1.0).abs() < f32::EPSILON);
        assert_eq!(
            AimDirection::new(SimulationVector::default()),
            Err(AimDirectionError::ZeroLength)
        );
        assert_eq!(
            AimDirection::new(SimulationVector::new(f32::NAN, 0.0)),
            Err(AimDirectionError::NonFinite)
        );
    }

    #[test]
    fn held_fire_is_immediate_then_exactly_fourteen_ticks_apart() {
        let mut combat = PlayerCombatState::new(pine_crossbow()).expect("combat");
        let world = empty_world();
        let mut shot_ticks = Vec::new();
        for _ in 0..30 {
            let step = combat
                .step(
                    held(1, AimDirection::east()),
                    SimulationVector::new(4.0, 12.0),
                    &world,
                )
                .expect("step");
            shot_ticks.extend(step.shots.into_iter().map(|shot| shot.tick.0));
        }
        assert_eq!(shot_ticks, [1, 15, 29]);
        assert_eq!(combat.projectiles().len(), 2);
    }

    #[test]
    fn release_does_not_reset_interval_and_new_ready_press_fires() {
        let mut combat = PlayerCombatState::new(pine_crossbow()).expect("combat");
        let world = empty_world();
        let position = SimulationVector::new(4.0, 12.0);
        assert_eq!(
            combat
                .step(held(1, AimDirection::east()), position, &world)
                .expect("fire")
                .shots
                .len(),
            1
        );
        combat
            .step(released(1, AimDirection::east()), position, &world)
            .expect("release");
        assert!(
            combat
                .step(held(2, AimDirection::east()), position, &world)
                .expect("early press")
                .shots
                .is_empty()
        );
        combat
            .step(released(2, AimDirection::east()), position, &world)
            .expect("release");
        while combat.interval_remaining_ticks() > 0 {
            combat
                .step(released(2, AimDirection::east()), position, &world)
                .expect("cooldown");
        }
        assert_eq!(
            combat
                .step(held(3, AimDirection::east()), position, &world)
                .expect("ready press")
                .shots
                .len(),
            1
        );
    }

    #[test]
    fn short_press_sequence_fires_even_when_latest_state_is_released() {
        let mut combat = PlayerCombatState::new(pine_crossbow()).expect("combat");
        let world = empty_world();
        let step = combat
            .step(
                released(1, AimDirection::east()),
                SimulationVector::new(4.0, 12.0),
                &world,
            )
            .expect("tap");
        assert_eq!(step.shots.len(), 1);
        assert_eq!(step.shots[0].press_sequence, 1);
    }

    #[test]
    fn sequence_failures_are_transactional() {
        let mut combat = PlayerCombatState::new(pine_crossbow()).expect("combat");
        let world = empty_world();
        let position = SimulationVector::new(4.0, 12.0);
        combat
            .step(held(1, AimDirection::east()), position, &world)
            .expect("press");
        combat
            .step(released(1, AimDirection::east()), position, &world)
            .expect("release");
        let before = combat.clone();
        assert_eq!(
            combat.step(held(1, AimDirection::east()), position, &world),
            Err(CombatError::MissingPressSequence { sequence: 1 })
        );
        assert_eq!(combat, before);
        assert_eq!(
            combat.step(released(0, AimDirection::east()), position, &world),
            Err(CombatError::StalePressSequence {
                received: 0,
                last: 1
            })
        );
        assert_eq!(combat, before);
    }

    #[test]
    fn projectile_moves_point_four_and_expires_at_exact_range() {
        let mut combat = PlayerCombatState::new(pine_crossbow()).expect("combat");
        let world = empty_world();
        let origin = SimulationVector::new(4.0, 12.0);
        combat
            .step(held(1, AimDirection::east()), origin, &world)
            .expect("fire");
        let step = combat
            .step(released(1, AimDirection::east()), origin, &world)
            .expect("travel 1");
        assert!(step.expirations.is_empty());
        assert!((combat.projectiles()[0].position().x - 4.4).abs() < 1.0e-6);
        assert!((combat.projectiles()[0].distance_travelled_tiles() - 0.4).abs() < 1.0e-6);

        for _ in 0..22 {
            combat
                .step(released(1, AimDirection::east()), origin, &world)
                .expect("full travel");
        }
        assert!((combat.projectiles()[0].distance_travelled_tiles() - 9.2).abs() < 1.0e-5);
        let terminal = combat
            .step(released(1, AimDirection::east()), origin, &world)
            .expect("terminal travel");
        assert!(combat.projectiles().is_empty());
        assert_eq!(terminal.expirations.len(), 1);
        assert!((terminal.expirations[0].final_position.x - 13.5).abs() < 1.0e-5);
        assert!((terminal.expirations[0].distance_travelled_tiles - 9.5).abs() < f32::EPSILON);
    }

    #[test]
    fn zero_pierce_enemy_contact_emits_one_terminal_event_without_damage_mutation() {
        let target_id = EntityId::new(100).expect("target ID");
        let target =
            EnemyHurtbox::new(target_id, SimulationVector::new(4.6, 12.0), 0.15).expect("target");
        let world = world_with(vec![], vec![target]);
        let mut combat = PlayerCombatState::new(pine_crossbow()).expect("combat");
        let origin = SimulationVector::new(4.0, 12.0);
        combat
            .step(held(1, AimDirection::east()), origin, &world)
            .expect("fire");
        let terminal = combat
            .step(released(1, AimDirection::east()), origin, &world)
            .expect("contact");
        assert!(combat.projectiles().is_empty());
        assert!(terminal.expirations.is_empty());
        assert_eq!(terminal.collisions.len(), 1);
        let collision = terminal.collisions[0];
        assert_eq!(collision.tick, Tick(2));
        assert_eq!(collision.projectile_id.get(), 1);
        assert_eq!(collision.target, CollisionTarget::Enemy(target_id));
        assert!((collision.final_position.x - 4.35).abs() < 1.0e-5);
        assert!((collision.final_position.y - 12.0).abs() < f32::EPSILON);
        assert!((collision.distance_travelled_tiles - 0.35).abs() < 1.0e-5);
    }

    #[test]
    fn solid_contact_wins_over_range_expiry_and_preserves_projectile_order() {
        let world = world_with(
            vec![TileRectangle::new(10_000, 5_000, 2_000, 3_000)],
            vec![],
        );
        let mut combat = PlayerCombatState::new(pine_crossbow()).expect("combat");
        let origin = SimulationVector::new(9.5, 6.5);
        combat
            .step(held(1, AimDirection::east()), origin, &world)
            .expect("fire first");
        for _ in 0..13 {
            combat
                .step(released(1, AimDirection::east()), origin, &world)
                .expect("cooldown");
        }
        combat
            .step(held(2, AimDirection::east()), origin, &world)
            .expect("fire second");
        let terminal = combat
            .step(released(2, AimDirection::east()), origin, &world)
            .expect("contacts");
        let ids: Vec<_> = terminal
            .collisions
            .iter()
            .map(|collision| collision.projectile_id.get())
            .collect();
        assert_eq!(ids, [2]);
        assert_eq!(
            terminal.collisions[0].target,
            CollisionTarget::Solid(crate::SolidColliderId::Pillar(0))
        );
        assert!((terminal.collisions[0].final_position.x - 9.9).abs() < 1.0e-5);
        assert!(terminal.expirations.is_empty());
    }

    #[test]
    fn projectile_locks_release_aim_and_origin_while_player_moves() {
        let mut combat = PlayerCombatState::new(pine_crossbow()).expect("combat");
        let world = empty_world();
        let south = AimDirection::new(SimulationVector::new(0.0, 1.0)).expect("south");
        combat
            .step(held(1, south), SimulationVector::new(4.0, 12.0), &world)
            .expect("fire");
        combat
            .step(
                released(1, AimDirection::east()),
                SimulationVector::new(8.0, 8.0),
                &world,
            )
            .expect("move");
        let projectile = &combat.projectiles()[0];
        assert_eq!(projectile.origin(), SimulationVector::new(4.0, 12.0));
        assert_eq!(projectile.direction(), south);
        assert_eq!(projectile.position(), SimulationVector::new(4.0, 12.4));
    }

    #[test]
    fn projectile_id_exhaustion_fails_without_partial_tick() {
        let allocator = EntityIdAllocator::starting_at(NonZeroU64::new(u64::MAX).expect("nonzero"));
        let mut combat = PlayerCombatState::with_projectile_allocator(pine_crossbow(), allocator)
            .expect("combat");
        let before = combat.clone();
        let world = empty_world();
        assert_eq!(
            combat.step(
                held(1, AimDirection::east()),
                SimulationVector::new(4.0, 12.0),
                &world,
            ),
            Err(CombatError::ProjectileIdExhausted)
        );
        assert_eq!(combat, before);
    }

    #[test]
    fn fixed_fire_trace_has_exact_stable_snapshot() {
        let mut combat = PlayerCombatState::new(pine_crossbow()).expect("combat");
        let world = empty_world();
        let northeast = AimDirection::new(SimulationVector::new(1.0, -1.0)).expect("aim");
        let mut shot_ticks = Vec::new();
        for _ in 0..35 {
            let step = combat
                .step(
                    held(1, northeast),
                    SimulationVector::new(20.0, 20.0),
                    &world,
                )
                .expect("trace step");
            shot_ticks.extend(step.shots.into_iter().map(|shot| shot.tick.0));
        }
        assert_eq!(shot_ticks, [1, 15, 29]);
        let snapshot: Vec<_> = combat
            .projectiles()
            .iter()
            .map(|projectile| {
                (
                    projectile.id().get(),
                    projectile.position().x.to_bits(),
                    projectile.position().y.to_bits(),
                    projectile.distance_travelled_tiles().to_bits(),
                )
            })
            .collect();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(
            snapshot,
            vec![
                (2, 1_103_970_620, 1_097_170_312, 1_090_519_041),
                (3, 1_101_894_546, 1_100_115_054, 1_075_419_546),
            ]
        );
    }
}
