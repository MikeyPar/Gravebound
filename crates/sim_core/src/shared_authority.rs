//! Deterministic multi-player authority aggregate for `GB-M02-09`.
//!
//! One hosted arena advances one enemy world exactly once per tick. Player-owned movement,
//! combat, inventory, pickups, and terminal state remain isolated inside the same transaction.

use std::{collections::BTreeMap, num::NonZeroU64};

use thiserror::Error;

use crate::{
    AuthorityDefinitions, AuthorityEntityKind, AuthorityEntitySnapshot, AuthorityInput,
    AuthorityPhase, AuthorityRecallCommit, CombatAction, CombatStep, ConsumableAction,
    EnemyLabPlayer, EntityId, EntityIdAllocator, FieldPickup, FieldPickupAccess, FieldPickupId,
    InventoryStack, MovementStep, NormalWaveSimulation, NormalWaveSpawn, NormalWaveStep,
    PickupEligibility, PickupOutcome, PlacementChoice, PlayerCombatState, PlayerMovementState,
    PlayerVitals, PrototypeInventory, RedTonicSimulation, SimulationVector, Tick, TonicBelt,
};

pub const SHARED_ARENA_MAX_PLAYERS: usize = 4;
pub const SHARED_FRIENDLY_PROJECTILE_ID_BASE: u64 = 40_000;
pub const SHARED_FRIENDLY_PROJECTILE_ID_STRIDE: u64 = 10_000;
const SHARED_PICKUP_ID_STRIDE: u64 = 1_000_000;

#[derive(Debug, Clone, PartialEq)]
pub struct SharedArenaPlayer {
    movement: PlayerMovementState,
    inventory: PrototypeInventory,
    pickups: Vec<FieldPickup>,
    eligibility: PickupEligibility,
    phase: AuthorityPhase,
    reward_drop_ordinal: u64,
    friendly_projectile_sequences: BTreeMap<EntityId, (u32, u16)>,
}

impl SharedArenaPlayer {
    #[must_use]
    pub const fn movement(&self) -> PlayerMovementState {
        self.movement
    }

    #[must_use]
    pub const fn inventory(&self) -> &PrototypeInventory {
        &self.inventory
    }

    #[must_use]
    pub fn pickups(&self) -> &[FieldPickup] {
        &self.pickups
    }

    #[must_use]
    pub const fn phase(&self) -> AuthorityPhase {
        self.phase
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SharedAuthorityStep {
    pub tick: Tick,
    pub state_version: u64,
    pub movement: BTreeMap<EntityId, MovementStep>,
    pub combat: BTreeMap<EntityId, CombatStep>,
    pub wave: NormalWaveStep,
    pub spawned_pickups: BTreeMap<EntityId, Vec<FieldPickupId>>,
    pub deaths_committed: Vec<EntityId>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SharedAuthoritativeArena {
    arena: crate::ArenaGeometry,
    wave: NormalWaveSimulation,
    players: BTreeMap<EntityId, SharedArenaPlayer>,
    reward_stacks: Vec<InventoryStack>,
    state_version: u64,
}

impl SharedAuthoritativeArena {
    pub fn new(
        definitions: AuthorityDefinitions,
        mut player_ids: Vec<EntityId>,
        spawns: Vec<NormalWaveSpawn>,
        eligibility: PickupEligibility,
        hostile_projectile_ids: EntityIdAllocator,
    ) -> Result<Self, SharedAuthorityError> {
        player_ids.sort_unstable();
        if player_ids.is_empty() || player_ids.len() > SHARED_ARENA_MAX_PLAYERS {
            return Err(SharedAuthorityError::InvalidPlayerCount(player_ids.len()));
        }
        if player_ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(SharedAuthorityError::DuplicatePlayerIdentity);
        }

        let mut players = BTreeMap::new();
        let mut wave_players = Vec::with_capacity(player_ids.len());
        for (index, player_id) in player_ids.into_iter().enumerate() {
            let player_ordinal =
                u64::try_from(index + 1).map_err(|_| SharedAuthorityError::IdentityOverflow)?;
            let movement = PlayerMovementState::at_arena_spawn(&definitions.arena)?;
            let vitals = PlayerVitals::new(definitions.maximum_health, definitions.maximum_health)?;
            let consumables = RedTonicSimulation::new(
                definitions.red_tonic.clone(),
                vitals,
                TonicBelt::first_playable(),
            )?;
            let projectile_start = SHARED_FRIENDLY_PROJECTILE_ID_BASE
                .checked_add(
                    player_ordinal
                        .checked_sub(1)
                        .and_then(|ordinal| {
                            ordinal.checked_mul(SHARED_FRIENDLY_PROJECTILE_ID_STRIDE)
                        })
                        .ok_or(SharedAuthorityError::IdentityOverflow)?,
                )
                .and_then(NonZeroU64::new)
                .ok_or(SharedAuthorityError::IdentityOverflow)?;
            let combat = PlayerCombatState::with_projectile_allocator(
                definitions.combat.weapon().clone(),
                definitions.combat.grave_mark_definition().clone(),
                definitions.combat.slipstep_definition().clone(),
                definitions.combat.stillness_definition().clone(),
                EntityIdAllocator::starting_at(projectile_start),
            )?;
            wave_players.push(EnemyLabPlayer {
                target: crate::HostileTargetState {
                    entity_id: player_id,
                    position: movement.position(),
                    target_is_immune: false,
                    resistance_basis_points: definitions.resistance_basis_points,
                    additional_direct_damage_reductions_basis_points: Vec::new(),
                    armor: definitions.starting_armor,
                    current_barrier: 0,
                    health_damage_cap_basis_points: None,
                },
                consumables,
                combat,
            });
            let run_ordinal = u32::try_from(player_ordinal)
                .map_err(|_| SharedAuthorityError::IdentityOverflow)?;
            players.insert(
                player_id,
                SharedArenaPlayer {
                    movement,
                    inventory: PrototypeInventory::first_playable_loadout(run_ordinal)?,
                    pickups: Vec::new(),
                    eligibility,
                    phase: AuthorityPhase::Alive,
                    reward_drop_ordinal: 1,
                    friendly_projectile_sequences: BTreeMap::new(),
                },
            );
        }
        let first = wave_players.remove(0);
        let mut wave = NormalWaveSimulation::new(
            definitions.wave,
            definitions.arena.clone(),
            spawns,
            first,
            hostile_projectile_ids,
            Tick(1),
        )?;
        for player in wave_players {
            wave.add_player(player)?;
        }
        Ok(Self {
            arena: definitions.arena,
            wave,
            players,
            reward_stacks: definitions.reward_stacks,
            state_version: 1,
        })
    }

    #[must_use]
    pub const fn state_version(&self) -> u64 {
        self.state_version
    }

    #[must_use]
    pub const fn wave(&self) -> &NormalWaveSimulation {
        &self.wave
    }

    #[must_use]
    pub const fn players(&self) -> &BTreeMap<EntityId, SharedArenaPlayer> {
        &self.players
    }

    pub fn apply_pickup(
        &mut self,
        player_id: EntityId,
        pickup_id: FieldPickupId,
        placement: PlacementChoice,
    ) -> Result<PickupOutcome, SharedAuthorityError> {
        let player = self
            .players
            .get_mut(&player_id)
            .ok_or(SharedAuthorityError::UnknownPlayer(player_id))?;
        if !matches!(player.phase, AuthorityPhase::Alive) || !player.eligibility.eligible() {
            return Err(SharedAuthorityError::PlayerUnavailable(player_id));
        }
        let pickup = player
            .pickups
            .iter_mut()
            .find(|pickup| pickup.pickup_id() == pickup_id)
            .ok_or(SharedAuthorityError::PickupNotFound(pickup_id))?;
        let outcome = player.inventory.apply_field_pickup(
            pickup,
            placement,
            player.movement.position(),
            FieldPickupAccess::Interact,
            self.wave.tick(),
        )?;
        self.state_version = self
            .state_version
            .checked_add(1)
            .ok_or(SharedAuthorityError::StateVersionExhausted)?;
        Ok(outcome)
    }

    /// Commits only the selected player's automatic `LinkLost` Recall. Shared enemies, hostile
    /// projectiles, lanes, rewards, and surviving players remain untouched.
    pub fn commit_automatic_recall(
        &mut self,
        player_id: EntityId,
    ) -> Result<AuthorityRecallCommit, SharedAuthorityError> {
        self.commit_automatic_recall_at(player_id, self.wave.tick())
    }

    pub fn commit_automatic_recall_at(
        &mut self,
        player_id: EntityId,
        committed_at: Tick,
    ) -> Result<AuthorityRecallCommit, SharedAuthorityError> {
        let mut next = self.clone();
        let player = next
            .players
            .get_mut(&player_id)
            .ok_or(SharedAuthorityError::UnknownPlayer(player_id))?;
        if !matches!(player.phase, AuthorityPhase::Alive) {
            return Err(SharedAuthorityError::PlayerUnavailable(player_id));
        }
        let wave_player = next
            .wave
            .players_mut()
            .get_mut(&player_id)
            .ok_or(SharedAuthorityError::PlayerInvariant)?;
        wave_player.combat.clear_projectiles_for_local_death();
        wave_player.target.target_is_immune = true;
        player.friendly_projectile_sequences.clear();
        let inventory = player.inventory.clear_pending_for_recall();
        let cleared_ground_pickups = player.pickups.len();
        player.pickups.clear();
        player.eligibility.reward_eligible = false;
        player.phase = AuthorityPhase::Recalled { committed_at };
        next.state_version = next
            .state_version
            .checked_add(1)
            .ok_or(SharedAuthorityError::StateVersionExhausted)?;
        *self = next;
        Ok(AuthorityRecallCommit {
            committed_at,
            inventory,
            cleared_ground_pickups,
        })
    }

    /// Produces one stable shared-world snapshot while exposing only the recipient's personal
    /// pickups. Every friendly projectile carries its owning player entity.
    #[allow(clippy::cast_precision_loss, clippy::too_many_lines)] // Millitile source values are bounded; one ordered encoder is audit-friendly.
    pub fn snapshots_for(
        &self,
        recipient: EntityId,
    ) -> Result<Vec<AuthorityEntitySnapshot>, SharedAuthorityError> {
        if !self.players.contains_key(&recipient) {
            return Err(SharedAuthorityError::UnknownPlayer(recipient));
        }
        let mut snapshots = Vec::new();
        for (player_id, player) in &self.players {
            let wave_player = self
                .wave
                .players()
                .get(player_id)
                .ok_or(SharedAuthorityError::PlayerInvariant)?;
            let vitals = wave_player.consumables.vitals();
            snapshots.push(shared_snapshot(
                player_id.get(),
                AuthorityEntityKind::Player,
                player.movement.position(),
                player.movement.velocity(),
                0,
                0,
                0,
                vitals.current_health(),
                vitals.maximum_health(),
                matches!(player.phase, AuthorityPhase::Alive),
                player.eligibility.eligible(),
                false,
            )?);
        }
        for enemy in self.wave.snapshots() {
            snapshots.push(shared_snapshot(
                enemy.entity_id.get(),
                AuthorityEntityKind::Enemy,
                SimulationVector::new(
                    enemy.position_milli_tiles.0 as f32 / 1_000.0,
                    enemy.position_milli_tiles.1 as f32 / 1_000.0,
                ),
                SimulationVector::default(),
                0,
                0,
                0,
                enemy.health.current_health,
                enemy.health.max_health,
                enemy.health.alive,
                false,
                false,
            )?);
        }
        for (player_id, player) in &self.players {
            let wave_player = self
                .wave
                .players()
                .get(player_id)
                .ok_or(SharedAuthorityError::PlayerInvariant)?;
            for projectile in wave_player.combat.projectiles() {
                let source = player
                    .friendly_projectile_sequences
                    .get(&projectile.id())
                    .copied()
                    .ok_or(SharedAuthorityError::MissingProjectileProvenance)?;
                snapshots.push(shared_snapshot(
                    projectile.id().get(),
                    AuthorityEntityKind::FriendlyProjectile,
                    projectile.position(),
                    projectile.direction().vector() * projectile.speed_tiles_per_second(),
                    player_id.get(),
                    source.0,
                    source.1,
                    0,
                    0,
                    true,
                    false,
                    false,
                )?);
            }
        }
        for projectile in self.wave.hostile_projectiles() {
            snapshots.push(shared_snapshot(
                projectile.id().get(),
                AuthorityEntityKind::HostileProjectile,
                projectile.position(),
                projectile.direction().vector() * projectile.speed_tiles_per_second(),
                0,
                0,
                0,
                0,
                0,
                true,
                false,
                false,
            )?);
        }
        let recipient_player = &self.players[&recipient];
        for pickup in &recipient_player.pickups {
            snapshots.push(shared_snapshot(
                pickup.pickup_id().get(),
                AuthorityEntityKind::PersonalPickup,
                pickup.position(),
                SimulationVector::default(),
                0,
                0,
                0,
                0,
                0,
                !pickup.is_collected(),
                recipient_player.eligibility.eligible(),
                pickup.is_collected(),
            )?);
        }
        snapshots.sort_by_key(|snapshot| snapshot.entity_id);
        Ok(snapshots)
    }

    pub fn step(
        &mut self,
        inputs: &BTreeMap<EntityId, AuthorityInput>,
    ) -> Result<SharedAuthorityStep, SharedAuthorityError> {
        let mut next = self.clone();
        let step = next.step_inner(inputs)?;
        *self = next;
        Ok(step)
    }

    #[allow(clippy::too_many_lines)] // The clone-then-commit order remains linear and auditable.
    fn step_inner(
        &mut self,
        inputs: &BTreeMap<EntityId, AuthorityInput>,
    ) -> Result<SharedAuthorityStep, SharedAuthorityError> {
        let active_ids = self
            .players
            .iter()
            .filter_map(|(id, player)| matches!(player.phase, AuthorityPhase::Alive).then_some(*id))
            .collect::<Vec<_>>();
        if inputs.len() != active_ids.len() || active_ids.iter().any(|id| !inputs.contains_key(id))
        {
            return Err(SharedAuthorityError::IncompleteInputSet);
        }
        let collision_world =
            crate::ProjectileCollisionWorld::new(&self.arena, self.wave.alive_hurtboxes()?)?;
        let mut movement_steps = BTreeMap::new();
        let mut combat_steps = BTreeMap::new();
        for player_id in active_ids {
            let input = inputs[&player_id];
            let player = self
                .players
                .get_mut(&player_id)
                .ok_or(SharedAuthorityError::PlayerInvariant)?;
            let wave_player = self
                .wave
                .players_mut()
                .get_mut(&player_id)
                .ok_or(SharedAuthorityError::PlayerInvariant)?;
            let (combat, movement) = wave_player.combat.step_with_movement_outcome(
                &mut player.movement,
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
            let mut ordinals = BTreeMap::<u32, u16>::new();
            for shot in &combat.shots {
                let ordinal = ordinals.entry(shot.press_sequence).or_default();
                player
                    .friendly_projectile_sequences
                    .insert(shot.projectile.id(), (shot.press_sequence, *ordinal));
                *ordinal = ordinal
                    .checked_add(1)
                    .ok_or(SharedAuthorityError::ProjectileOrdinalExhausted)?;
            }
            wave_player.consumables.step(ConsumableAction::default())?;
            wave_player.target.position = player.movement.position();
            movement_steps.insert(player_id, movement);
            combat_steps.insert(player_id, combat);
        }
        let wave = self.wave.step_players(&combat_steps)?;
        let mut spawned_pickups = BTreeMap::new();
        for (player_id, player) in &mut self.players {
            let Some(wave_player) = self.wave.players_mut().get_mut(player_id) else {
                return Err(SharedAuthorityError::PlayerInvariant);
            };
            player
                .friendly_projectile_sequences
                .retain(|projectile_id, _| {
                    wave_player
                        .combat
                        .projectiles()
                        .iter()
                        .any(|projectile| projectile.id() == *projectile_id)
                });
            let mut player_spawned = Vec::new();
            if player.eligibility.eligible() && matches!(player.phase, AuthorityPhase::Alive) {
                for drop in &wave.drops {
                    for stack in &self.reward_stacks {
                        let pickup_value = player_id
                            .get()
                            .checked_mul(SHARED_PICKUP_ID_STRIDE)
                            .and_then(|base| base.checked_add(player.reward_drop_ordinal))
                            .ok_or(SharedAuthorityError::IdentityOverflow)?;
                        let pickup_id = FieldPickupId::new(pickup_value)?;
                        player.reward_drop_ordinal = player
                            .reward_drop_ordinal
                            .checked_add(1)
                            .ok_or(SharedAuthorityError::IdentityOverflow)?;
                        player.pickups.push(FieldPickup::new(
                            pickup_id,
                            stack.clone(),
                            drop.event.position,
                            wave.tick,
                        )?);
                        player_spawned.push(pickup_id);
                    }
                }
            }
            spawned_pickups.insert(*player_id, player_spawned);
        }
        let mut deaths_committed = Vec::new();
        for (player_id, player) in &mut self.players {
            let wave_player = self
                .wave
                .players_mut()
                .get_mut(player_id)
                .ok_or(SharedAuthorityError::PlayerInvariant)?;
            if matches!(player.phase, AuthorityPhase::Alive)
                && wave_player.consumables.vitals().current_health() == 0
            {
                wave_player.combat.clear_projectiles_for_local_death();
                player.friendly_projectile_sequences.clear();
                player.inventory.clear_for_restart();
                player.pickups.clear();
                player.eligibility.reward_eligible = false;
                player.phase = AuthorityPhase::Dead {
                    committed_at: wave.tick,
                };
                deaths_committed.push(*player_id);
            }
        }
        self.state_version = self
            .state_version
            .checked_add(1)
            .ok_or(SharedAuthorityError::StateVersionExhausted)?;
        Ok(SharedAuthorityStep {
            tick: wave.tick,
            state_version: self.state_version,
            movement: movement_steps,
            combat: combat_steps,
            wave,
            spawned_pickups,
            deaths_committed,
        })
    }
}

#[derive(Debug, Error)]
pub enum SharedAuthorityError {
    #[error("shared arena player count {0} is outside 1..=4")]
    InvalidPlayerCount(usize),
    #[error("shared arena player identities must be unique")]
    DuplicatePlayerIdentity,
    #[error("shared arena identity space exhausted")]
    IdentityOverflow,
    #[error("shared arena requires one input for every living player and no unknown inputs")]
    IncompleteInputSet,
    #[error("shared arena player stores diverged")]
    PlayerInvariant,
    #[error("shared arena does not contain player {0}")]
    UnknownPlayer(EntityId),
    #[error("shared arena player {0} is terminal or ineligible")]
    PlayerUnavailable(EntityId),
    #[error("personal pickup {0:?} does not belong to this player")]
    PickupNotFound(FieldPickupId),
    #[error("friendly projectile is missing owner-qualified input provenance")]
    MissingProjectileProvenance,
    #[error("shared snapshot value is non-finite or outside millitile range")]
    SnapshotOutOfRange,
    #[error("friendly projectile ordinal exhausted")]
    ProjectileOrdinalExhausted,
    #[error("shared state version exhausted")]
    StateVersionExhausted,
    #[error(transparent)]
    Movement(#[from] crate::MovementError),
    #[error(transparent)]
    Vitals(#[from] crate::VitalsError),
    #[error(transparent)]
    Consumable(#[from] crate::ConsumableError),
    #[error(transparent)]
    Combat(#[from] crate::CombatError),
    #[error(transparent)]
    Inventory(#[from] crate::InventoryError),
    #[error(transparent)]
    NormalWave(#[from] crate::NormalWaveError),
    #[error(transparent)]
    Collision(#[from] crate::CollisionError),
}

#[allow(clippy::too_many_arguments)]
fn shared_snapshot(
    entity_id: u64,
    kind: AuthorityEntityKind,
    position: SimulationVector,
    velocity: SimulationVector,
    source_entity_id: u64,
    source_input_sequence: u32,
    source_projectile_ordinal: u16,
    current_health: u32,
    maximum_health: u32,
    alive: bool,
    eligible: bool,
    collected: bool,
) -> Result<AuthorityEntitySnapshot, SharedAuthorityError> {
    Ok(AuthorityEntitySnapshot {
        entity_id,
        kind,
        x_milli_tiles: shared_tiles_to_milli(position.x)?,
        y_milli_tiles: shared_tiles_to_milli(position.y)?,
        velocity_x_milli_tiles_per_second: shared_tiles_to_milli(velocity.x)?,
        velocity_y_milli_tiles_per_second: shared_tiles_to_milli(velocity.y)?,
        source_entity_id,
        source_input_sequence,
        source_projectile_ordinal,
        current_health,
        maximum_health,
        alive,
        eligible,
        collected,
    })
}

#[allow(clippy::cast_possible_truncation)]
fn shared_tiles_to_milli(value: f32) -> Result<i32, SharedAuthorityError> {
    if !value.is_finite() {
        return Err(SharedAuthorityError::SnapshotOutOfRange);
    }
    let scaled = (f64::from(value) * 1_000.0).round();
    if scaled < f64::from(i32::MIN) || scaled > f64::from(i32::MAX) {
        return Err(SharedAuthorityError::SnapshotOutOfRange);
    }
    Ok(scaled as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AimDirection, ArenaGeometry, GraveMarkDefinition, GraveMarkDefinitionParameters,
        MovementAction, NormalWaveDefinitions, NormalWaveEnemyKind, RedTonicDefinition,
        SimulationVector, SlipstepDefinition, SlipstepDefinitionParameters, SpawnInstanceId,
        StillnessDefinition, StillnessDefinitionParameters, TilePoint, WeaponDefinition,
        WeaponDefinitionParameters, normal_wave_projectile_allocator,
    };

    fn id(value: u64) -> EntityId {
        EntityId::new(value).unwrap()
    }

    fn definitions() -> AuthorityDefinitions {
        let arena = ArenaGeometry {
            id: "arena.test.shared_authority".to_owned(),
            width_milli_tiles: 32_000,
            height_milli_tiles: 24_000,
            shell_thickness_milli_tiles: 1_000,
            player_spawn: TilePoint::new(16_000, 12_000),
            boss_spawn: TilePoint::new(24_000, 12_000),
            pillars: Vec::new(),
            anchors: Vec::new(),
        }
        .validated()
        .unwrap();
        let weapon = WeaponDefinition::new(WeaponDefinitionParameters {
            content_id: "item.prototype.weapon.pine_crossbow".to_owned(),
            raw_damage: 20,
            attack_interval_ticks: 14,
            range_milli_tiles: 9_500,
            projectile_speed_milli_tiles_per_second: 12_000,
            projectile_radius_milli_tiles: 100,
            projectile_count: 1,
            projectile_directions_millionths: vec![(1_000_000, 0)],
            max_projectiles_per_target: 1,
            pierce: 0,
            stops_on_first_enemy: true,
        })
        .unwrap();
        let grave_mark = GraveMarkDefinition::new(GraveMarkDefinitionParameters {
            content_id: "ability.arbalist.grave_mark".to_owned(),
            cooldown_ticks: 150,
            global_cooldown_ticks: 5,
            input_buffer_ticks: 3,
            projectile_speed_milli_tiles_per_second: 12_000,
            range_milli_tiles: 11_000,
            projectile_radius_milli_tiles: 120,
            weapon_damage_multiplier_basis_points: 18_000,
            duration_ticks: 120,
            marked_primary_bonus_basis_points: 1_500,
            maximum_marked_targets: 1,
            consumes_on_solid: true,
        })
        .unwrap();
        let slipstep = SlipstepDefinition::new(SlipstepDefinitionParameters {
            content_id: "ability.arbalist.slipstep".to_owned(),
            cooldown_ticks: 240,
            global_cooldown_ticks: 5,
            input_buffer_ticks: 3,
            travel_milli_tiles: 2_000,
            travel_ticks: 5,
            direct_damage_reduction_basis_points: 2_500,
            empowered_window_ticks: 45,
            projectile_speed_bonus_basis_points: 3_000,
            pierce_bonus: 1,
            exhaustion_ticks: 45,
        })
        .unwrap();
        let stillness = StillnessDefinition::new(StillnessDefinitionParameters {
            content_id: "ability.arbalist.stillness".to_owned(),
            activation_ticks: 18,
            movement_threshold_basis_points: 2_000,
            projectile_speed_bonus_basis_points: 1_000,
            primary_damage_bonus_basis_points: 800,
            break_on_damage: true,
            break_on_slipstep: true,
        })
        .unwrap();
        AuthorityDefinitions {
            arena,
            wave: NormalWaveDefinitions::first_playable(),
            combat: PlayerCombatState::new(weapon, grave_mark, slipstep, stillness).unwrap(),
            red_tonic: RedTonicDefinition::first_playable(),
            maximum_health: 128,
            starting_armor: 0,
            resistance_basis_points: 0,
            reward_stacks: Vec::new(),
        }
    }

    fn arena() -> SharedAuthoritativeArena {
        SharedAuthoritativeArena::new(
            definitions(),
            vec![id(10_001), id(10_000)],
            vec![NormalWaveSpawn {
                instance_id: SpawnInstanceId {
                    run_ordinal: 1,
                    spawn_ordinal: 1,
                },
                kind: NormalWaveEnemyKind::DrownedPilgrim,
                position_milli_tiles: (8_000, 3_000),
            }],
            PickupEligibility {
                valid_session: true,
                reward_eligible: true,
            },
            normal_wave_projectile_allocator(1).unwrap(),
        )
        .unwrap()
    }

    fn input(horizontal: i8, primary_sequence: u32) -> AuthorityInput {
        AuthorityInput {
            movement: MovementAction::new(horizontal, 0),
            aim: AimDirection::east(),
            primary_held: primary_sequence != 0,
            primary_sequence,
            ability_1_sequence: 0,
            ability_2_sequence: 0,
        }
    }

    #[test]
    fn shared_step_is_atomic_and_uses_disjoint_player_projectile_namespaces() {
        let mut arena = arena();
        let step = arena
            .step(&BTreeMap::from([
                (id(10_000), input(-1, 1)),
                (id(10_001), input(1, 1)),
            ]))
            .unwrap();
        assert_eq!(step.tick, Tick(1));
        assert_eq!(step.movement.len(), 2);
        assert_eq!(
            step.combat[&id(10_000)].shots[0].projectile.id().get(),
            40_000
        );
        assert_eq!(
            step.combat[&id(10_001)].shots[0].projectile.id().get(),
            50_000
        );
        assert!(
            arena.players()[&id(10_000)].movement().position().x
                < SimulationVector::new(16.0, 12.0).x
        );
        assert!(
            arena.players()[&id(10_001)].movement().position().x
                > SimulationVector::new(16.0, 12.0).x
        );
    }

    #[test]
    fn incomplete_or_unknown_input_set_rolls_back_every_store() {
        let mut arena = arena();
        let before = arena.clone();
        assert!(matches!(
            arena.step(&BTreeMap::from([(id(10_000), input(0, 0))])),
            Err(SharedAuthorityError::IncompleteInputSet)
        ));
        assert_eq!(arena, before);
    }

    #[test]
    fn snapshots_are_owner_qualified_and_recall_is_strictly_player_local() {
        let mut arena = arena();
        arena
            .step(&BTreeMap::from([
                (id(10_000), input(0, 1)),
                (id(10_001), input(0, 1)),
            ]))
            .unwrap();
        let before = arena.snapshots_for(id(10_001)).unwrap();
        let projectile_owners = before
            .iter()
            .filter(|snapshot| snapshot.kind == AuthorityEntityKind::FriendlyProjectile)
            .map(|snapshot| snapshot.source_entity_id)
            .collect::<Vec<_>>();
        assert_eq!(projectile_owners, vec![10_000, 10_001]);

        let recall = arena.commit_automatic_recall(id(10_000)).unwrap();
        assert_eq!(recall.committed_at, Tick(2));
        assert!(matches!(
            arena.players()[&id(10_000)].phase(),
            AuthorityPhase::Recalled { .. }
        ));
        assert!(matches!(
            arena.players()[&id(10_001)].phase(),
            AuthorityPhase::Alive
        ));
        let after = arena.snapshots_for(id(10_001)).unwrap();
        let recalled = after
            .iter()
            .find(|snapshot| snapshot.entity_id == 10_000)
            .unwrap();
        assert!(!recalled.alive);
        assert!(after.iter().any(|snapshot| {
            snapshot.kind == AuthorityEntityKind::FriendlyProjectile
                && snapshot.source_entity_id == 10_001
        }));
        assert!(!after.iter().any(|snapshot| {
            snapshot.kind == AuthorityEntityKind::FriendlyProjectile
                && snapshot.source_entity_id == 10_000
        }));

        let mut released = input(0, 1);
        released.primary_held = false;
        arena
            .step(&BTreeMap::from([(id(10_001), released)]))
            .unwrap();
    }

    #[test]
    fn construction_rejects_duplicate_empty_and_over_capacity_rosters() {
        let make = |ids| {
            SharedAuthoritativeArena::new(
                definitions(),
                ids,
                vec![NormalWaveSpawn {
                    instance_id: SpawnInstanceId {
                        run_ordinal: 1,
                        spawn_ordinal: 1,
                    },
                    kind: NormalWaveEnemyKind::DrownedPilgrim,
                    position_milli_tiles: (8_000, 3_000),
                }],
                PickupEligibility {
                    valid_session: true,
                    reward_eligible: true,
                },
                normal_wave_projectile_allocator(1).unwrap(),
            )
        };
        assert!(matches!(
            make(Vec::new()),
            Err(SharedAuthorityError::InvalidPlayerCount(0))
        ));
        assert!(matches!(
            make(vec![id(1), id(1)]),
            Err(SharedAuthorityError::DuplicatePlayerIdentity)
        ));
        assert!(matches!(
            make(vec![id(1), id(2), id(3), id(4), id(5)]),
            Err(SharedAuthorityError::InvalidPlayerCount(5))
        ));
    }
}
