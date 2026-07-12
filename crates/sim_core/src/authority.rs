//! Transport-independent authoritative combat-test aggregate for `GB-M02-02`.
//!
//! The server supplies definitions compiled from immutable content and simulation-native intent.
//! This module owns the atomic ordering of movement, combat, enemy/hostile resolution, rewards,
//! inventory pickup, eligibility, and live-instance death finality.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::{
    AimDirection, ArenaGeometry, CombatAction, CombatError, CombatStep, ConsumableAction,
    ConsumableError, EnemyLabPlayer, EntityId, EntityIdAllocator, FieldPickup, FieldPickupAccess,
    FieldPickupId, FriendlyProjectile, HostileProjectile, HostileTargetState, InventoryError,
    InventoryStack, MovementAction, MovementError, MovementStep, NormalWaveDefinitions,
    NormalWaveError, NormalWaveInstanceSnapshot, NormalWaveSimulation, NormalWaveSpawn,
    NormalWaveStep, PickupOutcome, PlacementChoice, PlayerCombatState, PlayerMovementState,
    PlayerVitals, ProjectileCollisionWorld, PrototypeInventory, RecallCleanup, RedTonicDefinition,
    RedTonicSimulation, SimulationVector, Tick, TilePoint, TonicBelt, tile_point_to_simulation,
};

#[derive(Debug, Clone)]
pub struct AuthorityDefinitions {
    pub arena: ArenaGeometry,
    pub wave: NormalWaveDefinitions,
    pub combat: PlayerCombatState,
    pub red_tonic: RedTonicDefinition,
    pub maximum_health: u32,
    pub starting_armor: u32,
    pub resistance_basis_points: i32,
    pub reward_stacks: Vec<InventoryStack>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickupEligibility {
    pub valid_session: bool,
    pub reward_eligible: bool,
}

impl PickupEligibility {
    #[must_use]
    pub const fn eligible(self) -> bool {
        self.valid_session && self.reward_eligible
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AuthorityInput {
    pub movement: MovementAction,
    pub aim: AimDirection,
    pub primary_held: bool,
    pub primary_sequence: u32,
    pub ability_1_sequence: u32,
    pub ability_2_sequence: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityPhase {
    Alive,
    Recalled { committed_at: Tick },
    Dead { committed_at: Tick },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityRecallCommit {
    pub committed_at: Tick,
    pub inventory: RecallCleanup,
    pub cleared_ground_pickups: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuthorityStep {
    pub tick: Tick,
    pub state_version: u64,
    pub movement: MovementStep,
    pub combat: CombatStep,
    pub wave: NormalWaveStep,
    pub spawned_pickups: Vec<FieldPickupId>,
    pub death_committed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityEntityKind {
    Player,
    Enemy,
    FriendlyProjectile,
    HostileProjectile,
    PersonalPickup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorityEntitySnapshot {
    pub entity_id: u64,
    pub kind: AuthorityEntityKind,
    pub x_milli_tiles: i32,
    pub y_milli_tiles: i32,
    pub velocity_x_milli_tiles_per_second: i32,
    pub velocity_y_milli_tiles_per_second: i32,
    pub source_input_sequence: u32,
    pub source_projectile_ordinal: u16,
    pub current_health: u32,
    pub maximum_health: u32,
    pub alive: bool,
    pub eligible: bool,
    pub collected: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuthoritativeArena {
    arena: ArenaGeometry,
    movement: PlayerMovementState,
    wave: NormalWaveSimulation,
    inventory: PrototypeInventory,
    reward_stacks: Vec<InventoryStack>,
    pickups: Vec<FieldPickup>,
    eligibility: PickupEligibility,
    phase: AuthorityPhase,
    state_version: u64,
    reward_drop_ordinal: u64,
    friendly_projectile_sequences: BTreeMap<EntityId, (u32, u16)>,
}

impl AuthoritativeArena {
    pub fn new(
        definitions: AuthorityDefinitions,
        player_entity_id: EntityId,
        spawns: Vec<NormalWaveSpawn>,
        eligibility: PickupEligibility,
        hostile_projectile_ids: EntityIdAllocator,
    ) -> Result<Self, AuthorityError> {
        let movement = PlayerMovementState::at_arena_spawn(&definitions.arena)?;
        let vitals = PlayerVitals::new(definitions.maximum_health, definitions.maximum_health)?;
        let consumables =
            RedTonicSimulation::new(definitions.red_tonic, vitals, TonicBelt::first_playable())?;
        let player = EnemyLabPlayer {
            target: HostileTargetState {
                entity_id: player_entity_id,
                position: movement.position(),
                target_is_immune: false,
                resistance_basis_points: definitions.resistance_basis_points,
                additional_direct_damage_reductions_basis_points: Vec::new(),
                armor: definitions.starting_armor,
                current_barrier: 0,
                health_damage_cap_basis_points: None,
            },
            consumables,
            combat: definitions.combat,
        };
        let wave = NormalWaveSimulation::new(
            definitions.wave,
            definitions.arena.clone(),
            spawns,
            player,
            hostile_projectile_ids,
            Tick(1),
        )?;
        Ok(Self {
            arena: definitions.arena,
            movement,
            wave,
            inventory: PrototypeInventory::first_playable_loadout(1)?,
            reward_stacks: definitions.reward_stacks,
            pickups: Vec::new(),
            eligibility,
            phase: AuthorityPhase::Alive,
            state_version: 1,
            reward_drop_ordinal: 1,
            friendly_projectile_sequences: BTreeMap::new(),
        })
    }

    #[must_use]
    pub const fn phase(&self) -> AuthorityPhase {
        self.phase
    }

    #[must_use]
    pub const fn state_version(&self) -> u64 {
        self.state_version
    }

    #[must_use]
    pub const fn movement(&self) -> PlayerMovementState {
        self.movement
    }

    #[must_use]
    pub const fn player(&self) -> &EnemyLabPlayer {
        self.wave.player()
    }

    #[must_use]
    pub const fn inventory(&self) -> &PrototypeInventory {
        &self.inventory
    }

    #[must_use]
    pub fn pickups(&self) -> &[FieldPickup] {
        &self.pickups
    }

    pub fn step(&mut self, input: AuthorityInput) -> Result<AuthorityStep, AuthorityError> {
        let mut next = self.clone();
        let result = next.step_inner(input)?;
        *self = next;
        Ok(result)
    }

    fn step_inner(&mut self, input: AuthorityInput) -> Result<AuthorityStep, AuthorityError> {
        if !matches!(self.phase, AuthorityPhase::Alive) {
            return Err(AuthorityError::Dead);
        }
        let movement = self.movement.step(input.movement, &self.arena)?;
        let collision_world =
            ProjectileCollisionWorld::new(&self.arena, self.wave.alive_hurtboxes()?)?;
        let combat = self.wave.player_mut().combat.step_with_movement(
            &mut self.movement,
            CombatAction {
                aim: input.aim,
                movement: input.movement,
                primary_held: input.primary_held,
                primary_press_sequence: input.primary_sequence,
                ability_1_press_sequence: input.ability_1_sequence,
                ability_2_press_sequence: input.ability_2_sequence,
            },
            &self.arena,
            &collision_world,
        )?;
        let mut shot_ordinals = BTreeMap::<u32, u16>::new();
        for shot in &combat.shots {
            let ordinal = shot_ordinals.entry(shot.press_sequence).or_default();
            self.friendly_projectile_sequences
                .insert(shot.projectile.id(), (shot.press_sequence, *ordinal));
            *ordinal = ordinal
                .checked_add(1)
                .ok_or(AuthorityError::ProjectileOrdinalExhausted)?;
        }
        self.wave
            .player_mut()
            .consumables
            .step(ConsumableAction::default())?;
        self.wave.player_mut().target.position = self.movement.position();
        let wave = self.wave.step(&combat)?;
        self.friendly_projectile_sequences
            .retain(|projectile_id, _| {
                self.wave
                    .player()
                    .combat
                    .projectiles()
                    .iter()
                    .any(|projectile| projectile.id() == *projectile_id)
            });
        let spawned_pickups = self.materialize_due_rewards(&wave)?;
        let tick = wave.tick;
        let death_committed = self.wave.player().consumables.vitals().current_health() == 0;
        if death_committed {
            self.wave
                .player_mut()
                .combat
                .clear_projectiles_for_local_death();
            self.wave.clear_hostiles_for_player_death();
            self.friendly_projectile_sequences.clear();
            self.inventory.clear_for_restart();
            self.pickups.clear();
            self.eligibility.reward_eligible = false;
            self.phase = AuthorityPhase::Dead { committed_at: tick };
        }
        self.state_version = self
            .state_version
            .checked_add(1)
            .ok_or(AuthorityError::StateVersionExhausted)?;
        Ok(AuthorityStep {
            tick,
            state_version: self.state_version,
            movement,
            combat,
            wave,
            spawned_pickups,
            death_committed,
        })
    }

    fn materialize_due_rewards(
        &mut self,
        step: &NormalWaveStep,
    ) -> Result<Vec<FieldPickupId>, AuthorityError> {
        let mut spawned = Vec::new();
        if !self.eligibility.eligible() {
            return Ok(spawned);
        }
        for drop in &step.drops {
            for stack in &self.reward_stacks {
                let pickup_id = FieldPickupId::new(self.reward_drop_ordinal)?;
                self.reward_drop_ordinal = self
                    .reward_drop_ordinal
                    .checked_add(1)
                    .ok_or(AuthorityError::PickupIdExhausted)?;
                self.pickups.push(FieldPickup::new(
                    pickup_id,
                    stack.clone(),
                    drop.event.position,
                    step.tick,
                )?);
                spawned.push(pickup_id);
            }
        }
        Ok(spawned)
    }

    pub fn apply_pickup(
        &mut self,
        pickup_id: FieldPickupId,
        placement: PlacementChoice,
    ) -> Result<PickupOutcome, AuthorityError> {
        if !matches!(self.phase, AuthorityPhase::Alive) {
            return Err(AuthorityError::Dead);
        }
        if !self.eligibility.eligible() {
            return Err(AuthorityError::Ineligible);
        }
        let pickup = self
            .pickups
            .iter_mut()
            .find(|pickup| pickup.pickup_id() == pickup_id)
            .ok_or(AuthorityError::PickupNotFound(pickup_id))?;
        if pickup.is_collected() {
            return Err(AuthorityError::PickupAlreadyResolved(pickup_id));
        }
        let result = self.inventory.apply_field_pickup(
            pickup,
            placement,
            self.movement.position(),
            FieldPickupAccess::Interact,
            self.wave.tick(),
        )?;
        self.state_version = self
            .state_version
            .checked_add(1)
            .ok_or(AuthorityError::StateVersionExhausted)?;
        Ok(result)
    }

    /// Commits Emergency Recall atomically. Durable transfer to Lantern Halls is a persistence
    /// adapter concern, but live authority freezes combat and destroys unsecured backpack/ground
    /// state while preserving equipped gear and belt consumables.
    pub fn commit_emergency_recall(&mut self) -> Result<AuthorityRecallCommit, AuthorityError> {
        if !matches!(self.phase, AuthorityPhase::Alive) {
            return Err(AuthorityError::Dead);
        }
        let mut next = self.clone();
        let committed_at = next.wave.player().combat.tick();
        next.wave
            .player_mut()
            .combat
            .clear_projectiles_for_local_death();
        next.wave.clear_hostiles_for_player_death();
        next.friendly_projectile_sequences.clear();
        let inventory = next.inventory.clear_pending_for_recall();
        let cleared_ground_pickups = next.pickups.len();
        next.pickups.clear();
        next.eligibility.reward_eligible = false;
        next.phase = AuthorityPhase::Recalled { committed_at };
        next.state_version = next
            .state_version
            .checked_add(1)
            .ok_or(AuthorityError::StateVersionExhausted)?;
        *self = next;
        Ok(AuthorityRecallCommit {
            committed_at,
            inventory,
            cleared_ground_pickups,
        })
    }

    pub fn snapshots(&self) -> Result<Vec<AuthorityEntitySnapshot>, AuthorityError> {
        let mut snapshots = Vec::new();
        let player_vitals = self.wave.player().consumables.vitals();
        snapshots.push(snapshot(
            self.wave.player().target.entity_id.get(),
            AuthorityEntityKind::Player,
            self.movement.position(),
            self.movement.velocity(),
            0,
            0,
            SnapshotState {
                current_health: player_vitals.current_health(),
                maximum_health: player_vitals.maximum_health(),
                alive: matches!(self.phase, AuthorityPhase::Alive),
                eligible: self.eligibility.eligible(),
                collected: false,
            },
        )?);
        for enemy in self.wave.snapshots() {
            snapshots.push(enemy_snapshot(&enemy)?);
        }
        for projectile in self.wave.player().combat.projectiles() {
            snapshots.push(projectile_snapshot(
                projectile,
                AuthorityEntityKind::FriendlyProjectile,
                self.friendly_projectile_sequences
                    .get(&projectile.id())
                    .copied()
                    .ok_or(AuthorityError::MissingProjectileSourceSequence)?,
            )?);
        }
        for projectile in self.wave.hostile_projectiles() {
            snapshots.push(hostile_projectile_snapshot(projectile)?);
        }
        for pickup in &self.pickups {
            snapshots.push(snapshot(
                pickup.pickup_id().get(),
                AuthorityEntityKind::PersonalPickup,
                pickup.position(),
                SimulationVector::default(),
                0,
                0,
                SnapshotState {
                    current_health: 0,
                    maximum_health: 0,
                    alive: !pickup.is_collected() && !pickup.is_expired_at(self.wave.tick()),
                    eligible: self.eligibility.eligible(),
                    collected: pickup.is_collected(),
                },
            )?);
        }
        snapshots.sort_by_key(|entity| (entity.entity_id, entity.kind as u8));
        Ok(snapshots)
    }
}

fn enemy_snapshot(
    enemy: &NormalWaveInstanceSnapshot,
) -> Result<AuthorityEntitySnapshot, AuthorityError> {
    snapshot(
        enemy.entity_id.get(),
        AuthorityEntityKind::Enemy,
        milli_position(enemy.position_milli_tiles),
        SimulationVector::default(),
        0,
        0,
        SnapshotState {
            current_health: enemy.health.current_health,
            maximum_health: enemy.health.max_health,
            alive: enemy.health.alive,
            eligible: false,
            collected: false,
        },
    )
}

fn projectile_snapshot(
    projectile: &FriendlyProjectile,
    kind: AuthorityEntityKind,
    source: (u32, u16),
) -> Result<AuthorityEntitySnapshot, AuthorityError> {
    snapshot(
        projectile.id().get(),
        kind,
        projectile.position(),
        projectile.direction().vector() * projectile.speed_tiles_per_second(),
        source.0,
        source.1,
        SnapshotState::active_non_health(),
    )
}

fn hostile_projectile_snapshot(
    projectile: &HostileProjectile,
) -> Result<AuthorityEntitySnapshot, AuthorityError> {
    snapshot(
        projectile.id().get(),
        AuthorityEntityKind::HostileProjectile,
        projectile.position(),
        projectile.direction().vector() * projectile.speed_tiles_per_second(),
        0,
        0,
        SnapshotState::active_non_health(),
    )
}

#[derive(Debug, Clone, Copy)]
struct SnapshotState {
    current_health: u32,
    maximum_health: u32,
    alive: bool,
    eligible: bool,
    collected: bool,
}

impl SnapshotState {
    const fn active_non_health() -> Self {
        Self {
            current_health: 0,
            maximum_health: 0,
            alive: true,
            eligible: false,
            collected: false,
        }
    }
}

fn snapshot(
    entity_id: u64,
    kind: AuthorityEntityKind,
    position: SimulationVector,
    velocity: SimulationVector,
    source_input_sequence: u32,
    source_projectile_ordinal: u16,
    state: SnapshotState,
) -> Result<AuthorityEntitySnapshot, AuthorityError> {
    Ok(AuthorityEntitySnapshot {
        entity_id,
        kind,
        x_milli_tiles: tiles_to_milli(position.x)?,
        y_milli_tiles: tiles_to_milli(position.y)?,
        velocity_x_milli_tiles_per_second: tiles_to_milli(velocity.x)?,
        velocity_y_milli_tiles_per_second: tiles_to_milli(velocity.y)?,
        source_input_sequence,
        source_projectile_ordinal,
        current_health: state.current_health,
        maximum_health: state.maximum_health,
        alive: state.alive,
        eligible: state.eligible,
        collected: state.collected,
    })
}

#[allow(clippy::cast_possible_truncation)] // Rounded value is range-checked in f64 first.
fn tiles_to_milli(value: f32) -> Result<i32, AuthorityError> {
    if !value.is_finite() {
        return Err(AuthorityError::NonFiniteSnapshot);
    }
    let scaled = (f64::from(value) * 1_000.0).round();
    if scaled < f64::from(i32::MIN) || scaled > f64::from(i32::MAX) {
        return Err(AuthorityError::SnapshotOutOfRange);
    }
    Ok(scaled as i32)
}

fn milli_position(position: (i32, i32)) -> SimulationVector {
    tile_point_to_simulation(TilePoint::new(position.0, position.1))
}

#[derive(Debug, Error)]
pub enum AuthorityError {
    #[error("authoritative character is dead")]
    Dead,
    #[error("session is not eligible for its personal pickup")]
    Ineligible,
    #[error("personal pickup {0:?} was not found")]
    PickupNotFound(FieldPickupId),
    #[error("personal pickup {0:?} was already resolved")]
    PickupAlreadyResolved(FieldPickupId),
    #[error("authoritative state version exhausted")]
    StateVersionExhausted,
    #[error("authoritative pickup identity exhausted")]
    PickupIdExhausted,
    #[error("snapshot position is non-finite")]
    NonFiniteSnapshot,
    #[error("snapshot position exceeds fixed-point range")]
    SnapshotOutOfRange,
    #[error("friendly projectile is missing its source input sequence")]
    MissingProjectileSourceSequence,
    #[error("friendly projectile ordinal exhausted")]
    ProjectileOrdinalExhausted,
    #[error(transparent)]
    Movement(#[from] MovementError),
    #[error(transparent)]
    Combat(#[from] CombatError),
    #[error(transparent)]
    Consumable(#[from] ConsumableError),
    #[error(transparent)]
    NormalWave(#[from] NormalWaveError),
    #[error(transparent)]
    Collision(#[from] crate::CollisionError),
    #[error(transparent)]
    Inventory(#[from] InventoryError),
    #[error(transparent)]
    Vitals(#[from] crate::VitalsError),
}
