use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::BellDebtDefinition;
use crate::{
    ArenaGeometry, BASIS_POINTS_PER_ONE, CollisionError, CollisionTarget, EntityId,
    EntityIdAllocator, GraveArbalistOath, GraveMarkDefinition, IntentMathError, MovementAction,
    MovementError, MovementStep, NailTrapEnemy, NailTrapField, NailTrapStep, PlayerMovementState,
    ProjectileCollisionWorld, SimulationVector, SlipstepDefinition, SolidColliderId,
    StillnessDefinition, TICKS_PER_SECOND, Tick, WeaponDefinition,
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
    pub movement: MovementAction,
    pub primary_held: bool,
    pub primary_press_sequence: u32,
    pub ability_1_press_sequence: u32,
    pub ability_2_press_sequence: u32,
}

/// Rule source carried through projectile, collision, intent, and presentation events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FriendlyProjectileSource {
    Primary,
    BellDebtRepeat,
    GraveMark,
}

/// One authoritative friendly projectile.
#[derive(Debug, Clone, PartialEq)]
pub struct FriendlyProjectile {
    id: EntityId,
    source: FriendlyProjectileSource,
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
    empowered_by_slipstep: bool,
    hit_targets: Vec<EntityId>,
    focused_by_stillness: bool,
    release_tick: Tick,
    max_projectiles_per_target: u32,
    damage_multiplier_basis_points: u32,
}

impl FriendlyProjectile {
    #[must_use]
    pub const fn id(&self) -> EntityId {
        self.id
    }

    #[must_use]
    pub const fn source(&self) -> FriendlyProjectileSource {
        self.source
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

    #[must_use]
    pub const fn empowered_by_slipstep(&self) -> bool {
        self.empowered_by_slipstep
    }

    #[must_use]
    pub const fn focused_by_stillness(&self) -> bool {
        self.focused_by_stillness
    }

    #[must_use]
    pub const fn damage_multiplier_basis_points(&self) -> u32 {
        self.damage_multiplier_basis_points
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
    pub source: FriendlyProjectileSource,
    pub final_position: SimulationVector,
    pub distance_travelled_tiles: f32,
}

/// One authoritative projectile terminal contact. `(tick, projectile_id)` is run-locally unique.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectileCollision {
    pub tick: Tick,
    pub projectile_id: EntityId,
    pub source: FriendlyProjectileSource,
    pub target: CollisionTarget,
    pub final_position: SimulationVector,
    pub distance_travelled_tiles: f32,
    pub contact_ordinal: u32,
    pub empowered_by_slipstep: bool,
    pub focused_by_stillness: bool,
    pub projectile_continues: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawDamageIntentSource {
    Primary,
    BellDebtRepeat,
    GraveMark,
    NailTrap,
}

/// Pre-`COM-002` damage fact. This ticket intentionally performs no health mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawDamageIntent {
    pub tick: Tick,
    pub projectile_id: EntityId,
    pub source: RawDamageIntentSource,
    pub target: EntityId,
    pub base_raw_damage: u32,
    pub multiplier_basis_points: u32,
    pub resolved_raw_damage: u32,
    pub contact_ordinal: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlipstepTransitionKind {
    Began,
    Travelled,
    Collided,
    Completed,
    EmpowermentConsumed,
    EmpowermentExpired,
    ExhaustionExpired,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlipstepTransition {
    pub tick: Tick,
    pub kind: SlipstepTransitionKind,
    pub press_sequence: u32,
    pub position: SimulationVector,
    pub travelled_tiles: f32,
    pub remaining_travel_ticks: u32,
    pub solid: Option<SolidColliderId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlipstepInputResult {
    Began,
    Buffered { readiness_ticks: u32 },
    ConsumedTooEarly { readiness_ticks: u32 },
    BlockedByExhaustion { remaining_ticks: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlipstepInputEvent {
    pub tick: Tick,
    pub press_sequence: u32,
    pub result: SlipstepInputResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedTransitionKind {
    Gained,
    BrokenByMovement,
    BrokenBySlipstep,
    BrokenByDamage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusedTransition {
    pub tick: Tick,
    pub kind: FocusedTransitionKind,
    pub stillness_ticks: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveGraveMark {
    target: EntityId,
    remaining_ticks: u32,
    source_projectile_id: EntityId,
}

impl ActiveGraveMark {
    #[must_use]
    pub const fn target(self) -> EntityId {
        self.target
    }

    #[must_use]
    pub const fn remaining_ticks(self) -> u32 {
        self.remaining_ticks
    }

    #[must_use]
    pub const fn source_projectile_id(self) -> EntityId {
        self.source_projectile_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraveMarkTransitionKind {
    Applied,
    Refreshed,
    Replaced,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraveMarkTransition {
    pub tick: Tick,
    pub kind: GraveMarkTransitionKind,
    pub target: EntityId,
    pub previous_target: Option<EntityId>,
    pub source_projectile_id: EntityId,
    pub remaining_ticks: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraveMarkInputResult {
    Fired { projectile_id: EntityId },
    Buffered { readiness_ticks: u32 },
    ConsumedTooEarly { readiness_ticks: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraveMarkInputEvent {
    pub tick: Tick,
    pub press_sequence: u32,
    pub result: GraveMarkInputResult,
}

/// Events and state summary produced by one fixed combat tick.
#[derive(Debug, Clone, PartialEq)]
pub struct CombatStep {
    pub tick: Tick,
    pub shots: Vec<ShotEvent>,
    pub collisions: Vec<ProjectileCollision>,
    pub expirations: Vec<ProjectileExpired>,
    pub raw_damage_intents: Vec<RawDamageIntent>,
    pub attacker_multiplier_basis_points: u32,
    pub mark_transitions: Vec<GraveMarkTransition>,
    pub grave_mark_inputs: Vec<GraveMarkInputEvent>,
    pub slipstep_inputs: Vec<SlipstepInputEvent>,
    pub slipstep_transitions: Vec<SlipstepTransition>,
    pub direct_damage_reduction_basis_points: u32,
    pub focused_transitions: Vec<FocusedTransition>,
    pub nail_traps: NailTrapStep,
}

impl Default for CombatStep {
    fn default() -> Self {
        Self {
            tick: Tick::default(),
            shots: Vec::new(),
            collisions: Vec::new(),
            expirations: Vec::new(),
            raw_damage_intents: Vec::new(),
            attacker_multiplier_basis_points: BASIS_POINTS_PER_ONE,
            mark_transitions: Vec::new(),
            grave_mark_inputs: Vec::new(),
            slipstep_inputs: Vec::new(),
            slipstep_transitions: Vec::new(),
            direct_damage_reduction_basis_points: 0,
            focused_transitions: Vec::new(),
            nail_traps: NailTrapStep::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PendingGraveMark {
    press_sequence: u32,
    aim: AimDirection,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PendingSlipstep {
    press_sequence: u32,
    direction: AimDirection,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ActiveSlipstep {
    press_sequence: u32,
    direction: AimDirection,
    remaining_ticks: u32,
    target_position: SimulationVector,
}

#[derive(Debug, Clone, PartialEq)]
struct PendingBellRepeat {
    due_tick: Tick,
    press_sequence: u32,
    projectiles: Vec<FriendlyProjectile>,
}

/// Durable Bell Debt checkpoint schema understood by this simulation build.
pub const BELL_DEBT_CHECKPOINT_SCHEMA_VERSION: u16 = 1;
/// Hard allocation and persistence bound for one encoded Bell Debt checkpoint.
pub const MAX_BELL_DEBT_CHECKPOINT_BYTES: usize = 4_096;
const MAX_BELL_CHECKPOINT_PROJECTILES: usize = 32;
const MAX_CHECKPOINT_PROJECTILE_SCALAR: f32 = 1_024.0;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BellDebtCheckpoint {
    schema_version: u16,
    primary_release_count: u8,
    pending_repeat: Option<BellDebtPendingRepeatCheckpoint>,
}

impl BellDebtCheckpoint {
    #[must_use]
    pub const fn primary_release_count(&self) -> u8 {
        self.primary_release_count
    }

    #[must_use]
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    #[must_use]
    pub const fn has_pending_repeat(&self) -> bool {
        self.pending_repeat.is_some()
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CombatError> {
        let bytes = postcard::to_stdvec(self).map_err(|_| CombatError::InvalidBellCheckpoint)?;
        if bytes.is_empty() || bytes.len() > MAX_BELL_DEBT_CHECKPOINT_BYTES {
            return Err(CombatError::InvalidBellCheckpoint);
        }
        Ok(bytes)
    }

    pub fn decode_canonical(bytes: &[u8]) -> Result<Self, CombatError> {
        if bytes.is_empty() || bytes.len() > MAX_BELL_DEBT_CHECKPOINT_BYTES {
            return Err(CombatError::InvalidBellCheckpoint);
        }
        let checkpoint: Self =
            postcard::from_bytes(bytes).map_err(|_| CombatError::InvalidBellCheckpoint)?;
        if checkpoint.canonical_bytes()?.as_slice() != bytes {
            return Err(CombatError::InvalidBellCheckpoint);
        }
        Ok(checkpoint)
    }

    pub fn canonical_digest(&self) -> Result<[u8; 32], CombatError> {
        Ok(*blake3::hash(&self.canonical_bytes()?).as_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BellDebtPendingRepeatCheckpoint {
    ticks_remaining: u32,
    press_sequence: u32,
    projectiles: Vec<BellDebtProjectileCheckpoint>,
}

impl BellDebtPendingRepeatCheckpoint {
    #[must_use]
    pub const fn ticks_remaining(&self) -> u32 {
        self.ticks_remaining
    }

    #[must_use]
    pub const fn press_sequence(&self) -> u32 {
        self.press_sequence
    }

    #[must_use]
    pub fn projectiles(&self) -> &[BellDebtProjectileCheckpoint] {
        &self.projectiles
    }
}

/// Immutable projectile behavior required to reproduce a scheduled Bell repeat.
///
/// Transient identity, location, hit history, distance, and release tick are deliberately
/// excluded because the authoritative simulation regenerates them when the repeat emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BellDebtProjectileCheckpoint {
    direction_x_bits: u32,
    direction_y_bits: u32,
    range_tiles_bits: u32,
    speed_tiles_per_second_bits: u32,
    radius_tiles_bits: u32,
    raw_damage: u32,
    pierce_remaining: u32,
    stops_on_first_enemy: bool,
    empowered_by_slipstep: bool,
    focused_by_stillness: bool,
    max_projectiles_per_target: u32,
}

impl BellDebtProjectileCheckpoint {
    fn from_projectile(projectile: &FriendlyProjectile) -> Self {
        let direction = projectile.direction.vector();
        Self {
            direction_x_bits: direction.x.to_bits(),
            direction_y_bits: direction.y.to_bits(),
            range_tiles_bits: projectile.range_tiles.to_bits(),
            speed_tiles_per_second_bits: projectile.speed_tiles_per_second.to_bits(),
            radius_tiles_bits: projectile.radius_tiles.to_bits(),
            raw_damage: projectile.raw_damage,
            pierce_remaining: projectile.pierce_remaining,
            stops_on_first_enemy: projectile.stops_on_first_enemy,
            empowered_by_slipstep: projectile.empowered_by_slipstep,
            focused_by_stillness: projectile.focused_by_stillness,
            max_projectiles_per_target: projectile.max_projectiles_per_target,
        }
    }

    fn into_projectile(self) -> FriendlyProjectile {
        let origin = SimulationVector::default();
        FriendlyProjectile {
            id: EntityId::new(1).expect("one is a valid placeholder entity ID"),
            source: FriendlyProjectileSource::Primary,
            position: origin,
            origin,
            direction: AimDirection(SimulationVector::new(
                f32::from_bits(self.direction_x_bits),
                f32::from_bits(self.direction_y_bits),
            )),
            distance_travelled_tiles: 0.0,
            range_tiles: f32::from_bits(self.range_tiles_bits),
            speed_tiles_per_second: f32::from_bits(self.speed_tiles_per_second_bits),
            radius_tiles: f32::from_bits(self.radius_tiles_bits),
            raw_damage: self.raw_damage,
            pierce_remaining: self.pierce_remaining,
            stops_on_first_enemy: self.stops_on_first_enemy,
            empowered_by_slipstep: self.empowered_by_slipstep,
            hit_targets: Vec::new(),
            focused_by_stillness: self.focused_by_stillness,
            release_tick: Tick(0),
            max_projectiles_per_target: self.max_projectiles_per_target,
            damage_multiplier_basis_points: BASIS_POINTS_PER_ONE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BellDebtResetReason {
    Acquisition,
    Purge,
    Death,
    Retirement,
    SafeTransfer,
}

/// Simulation-owned primary weapon timer and projectile collection.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerCombatState {
    weapon: WeaponDefinition,
    grave_mark: GraveMarkDefinition,
    slipstep: SlipstepDefinition,
    stillness: StillnessDefinition,
    tick: Tick,
    interval_remaining_ticks: u32,
    last_press_sequence: u32,
    previous_primary_held: bool,
    last_ability_1_press_sequence: u32,
    grave_mark_cooldown_remaining_ticks: u32,
    global_cooldown_remaining_ticks: u32,
    pending_grave_mark: Option<PendingGraveMark>,
    active_grave_mark: Option<ActiveGraveMark>,
    last_ability_2_press_sequence: u32,
    slipstep_cooldown_remaining_ticks: u32,
    exhaustion_remaining_ticks: u32,
    empowered_primary_remaining_ticks: u32,
    pending_slipstep: Option<PendingSlipstep>,
    active_slipstep: Option<ActiveSlipstep>,
    stillness_ticks: u32,
    focused: bool,
    projectile_ids: EntityIdAllocator,
    projectiles: Vec<FriendlyProjectile>,
    outgoing_direct_damage_basis_points: u32,
    bell_debt: Option<BellDebtDefinition>,
    bell_primary_release_count: u8,
    pending_bell_repeat: Option<PendingBellRepeat>,
    oath: Option<GraveArbalistOath>,
    nail_traps: NailTrapField,
}

impl PlayerCombatState {
    pub fn new(
        weapon: WeaponDefinition,
        grave_mark: GraveMarkDefinition,
        slipstep: SlipstepDefinition,
        stillness: StillnessDefinition,
    ) -> Result<Self, CombatError> {
        Self::with_projectile_allocator(
            weapon,
            grave_mark,
            slipstep,
            stillness,
            EntityIdAllocator::default(),
        )
    }

    pub fn with_projectile_allocator(
        weapon: WeaponDefinition,
        grave_mark: GraveMarkDefinition,
        slipstep: SlipstepDefinition,
        stillness: StillnessDefinition,
        projectile_ids: EntityIdAllocator,
    ) -> Result<Self, CombatError> {
        Ok(Self {
            weapon,
            grave_mark,
            slipstep,
            stillness,
            tick: Tick(0),
            interval_remaining_ticks: 0,
            last_press_sequence: 0,
            previous_primary_held: false,
            last_ability_1_press_sequence: 0,
            grave_mark_cooldown_remaining_ticks: 0,
            global_cooldown_remaining_ticks: 0,
            pending_grave_mark: None,
            active_grave_mark: None,
            last_ability_2_press_sequence: 0,
            slipstep_cooldown_remaining_ticks: 0,
            exhaustion_remaining_ticks: 0,
            empowered_primary_remaining_ticks: 0,
            pending_slipstep: None,
            active_slipstep: None,
            stillness_ticks: 0,
            focused: false,
            projectile_ids,
            projectiles: Vec::new(),
            outgoing_direct_damage_basis_points: BASIS_POINTS_PER_ONE,
            bell_debt: None,
            bell_primary_release_count: 0,
            pending_bell_repeat: None,
            oath: None,
            nail_traps: NailTrapField::default(),
        })
    }

    pub fn with_oath(
        weapon: WeaponDefinition,
        grave_mark: GraveMarkDefinition,
        slipstep: SlipstepDefinition,
        stillness: StillnessDefinition,
        oath: GraveArbalistOath,
    ) -> Result<Self, CombatError> {
        let mut state = Self::new(weapon, grave_mark, slipstep, stillness)?;
        state.oath = Some(oath);
        Ok(state)
    }

    pub fn with_core_choices(
        weapon: WeaponDefinition,
        grave_mark: GraveMarkDefinition,
        slipstep: SlipstepDefinition,
        stillness: StillnessDefinition,
        oath: Option<GraveArbalistOath>,
        outgoing_direct_damage_basis_points: u32,
        bell_debt: Option<BellDebtDefinition>,
    ) -> Result<Self, CombatError> {
        if !(1..=crate::MAXIMUM_OUTGOING_DAMAGE_BASIS_POINTS)
            .contains(&outgoing_direct_damage_basis_points)
        {
            return Err(CombatError::InvalidOutgoingDirectDamageMultiplier);
        }
        let mut state = Self::new(weapon, grave_mark, slipstep, stillness)?;
        state.oath = oath;
        state.outgoing_direct_damage_basis_points = outgoing_direct_damage_basis_points;
        state.bell_debt = bell_debt;
        Ok(state)
    }

    /// Starts a new arena with the same validated character configuration and no live combat
    /// state. Oath and Bargain choices are immutable configuration; timers, projectiles, traps,
    /// Bell progress, and buffered inputs belong to the arena being retired.
    pub fn fresh_arena_with_projectile_allocator(
        &self,
        projectile_ids: EntityIdAllocator,
    ) -> Result<Self, CombatError> {
        let mut state = Self::with_projectile_allocator(
            self.weapon.clone(),
            self.grave_mark.clone(),
            self.slipstep.clone(),
            self.stillness.clone(),
            projectile_ids,
        )?;
        state.outgoing_direct_damage_basis_points = self.outgoing_direct_damage_basis_points;
        state.bell_debt = self.bell_debt;
        state.oath = self.oath;
        Ok(state)
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
    pub fn grave_mark_definition(&self) -> &GraveMarkDefinition {
        &self.grave_mark
    }

    #[must_use]
    pub fn slipstep_definition(&self) -> &SlipstepDefinition {
        &self.slipstep
    }

    #[must_use]
    pub fn stillness_definition(&self) -> &StillnessDefinition {
        &self.stillness
    }
    #[must_use]
    pub const fn stillness_ticks(&self) -> u32 {
        self.stillness_ticks
    }
    #[must_use]
    pub const fn focused(&self) -> bool {
        self.focused
    }

    #[must_use]
    pub const fn slipstep_cooldown_remaining_ticks(&self) -> u32 {
        self.slipstep_cooldown_remaining_ticks
    }

    #[must_use]
    pub const fn exhaustion_remaining_ticks(&self) -> u32 {
        self.exhaustion_remaining_ticks
    }

    #[must_use]
    pub const fn empowered_primary_remaining_ticks(&self) -> u32 {
        self.empowered_primary_remaining_ticks
    }

    #[must_use]
    pub const fn slipstep_remaining_travel_ticks(&self) -> u32 {
        match self.active_slipstep {
            Some(active) => active.remaining_ticks,
            None => 0,
        }
    }

    #[must_use]
    pub const fn direct_damage_reduction_basis_points(&self) -> u32 {
        if self.active_slipstep.is_some() {
            self.slipstep.direct_damage_reduction_basis_points()
        } else {
            0
        }
    }

    #[must_use]
    pub const fn outgoing_direct_damage_basis_points(&self) -> u32 {
        self.outgoing_direct_damage_basis_points
    }

    #[must_use]
    pub const fn bell_primary_release_count(&self) -> u8 {
        self.bell_primary_release_count
    }

    #[must_use]
    pub const fn has_pending_bell_repeat(&self) -> bool {
        self.pending_bell_repeat.is_some()
    }

    pub fn export_bell_debt_checkpoint(&self) -> Result<BellDebtCheckpoint, CombatError> {
        let pending_repeat = self
            .pending_bell_repeat
            .as_ref()
            .map(|pending| {
                let remaining = pending
                    .due_tick
                    .0
                    .checked_sub(self.tick.0)
                    .ok_or(CombatError::InvalidBellCheckpoint)?;
                Ok::<BellDebtPendingRepeatCheckpoint, CombatError>(
                    BellDebtPendingRepeatCheckpoint {
                        ticks_remaining: u32::try_from(remaining)
                            .map_err(|_| CombatError::InvalidBellCheckpoint)?,
                        press_sequence: pending.press_sequence,
                        projectiles: pending
                            .projectiles
                            .iter()
                            .map(BellDebtProjectileCheckpoint::from_projectile)
                            .collect(),
                    },
                )
            })
            .transpose()?;
        let checkpoint = BellDebtCheckpoint {
            schema_version: BELL_DEBT_CHECKPOINT_SCHEMA_VERSION,
            primary_release_count: self.bell_primary_release_count,
            pending_repeat,
        };
        self.validate_bell_checkpoint(&checkpoint)?;
        Ok(checkpoint)
    }

    pub fn import_bell_debt_checkpoint(
        &mut self,
        checkpoint: &BellDebtCheckpoint,
    ) -> Result<(), CombatError> {
        self.validate_bell_checkpoint(checkpoint)?;
        self.bell_primary_release_count = checkpoint.primary_release_count;
        self.pending_bell_repeat = checkpoint
            .pending_repeat
            .as_ref()
            .map(|pending| {
                Ok::<PendingBellRepeat, CombatError>(PendingBellRepeat {
                    due_tick: Tick(
                        self.tick
                            .0
                            .checked_add(u64::from(pending.ticks_remaining))
                            .ok_or(CombatError::TickExhausted)?,
                    ),
                    press_sequence: pending.press_sequence,
                    projectiles: pending
                        .projectiles
                        .iter()
                        .copied()
                        .map(BellDebtProjectileCheckpoint::into_projectile)
                        .collect(),
                })
            })
            .transpose()?;
        Ok(())
    }

    pub fn reset_bell_debt(&mut self, _reason: BellDebtResetReason) {
        self.bell_primary_release_count = 0;
        self.pending_bell_repeat = None;
    }

    pub fn cancel_pending_bell_repeat_for_primary_illegal(&mut self) {
        self.pending_bell_repeat = None;
    }

    fn validate_bell_checkpoint(&self, checkpoint: &BellDebtCheckpoint) -> Result<(), CombatError> {
        if checkpoint.schema_version != BELL_DEBT_CHECKPOINT_SCHEMA_VERSION {
            return Err(CombatError::InvalidBellCheckpoint);
        }
        let Some(definition) = self.bell_debt else {
            return if checkpoint.primary_release_count == 0 && checkpoint.pending_repeat.is_none() {
                Ok(())
            } else {
                Err(CombatError::InvalidBellCheckpoint)
            };
        };
        if checkpoint.primary_release_count >= definition.accepted_primary_emissions_per_repeat {
            return Err(CombatError::InvalidBellCheckpoint);
        }
        if let Some(pending) = &checkpoint.pending_repeat
            && (pending.ticks_remaining == 0
                || pending.ticks_remaining > definition.repeat_delay_ticks
                || pending.projectiles.is_empty()
                || pending.projectiles.len() > MAX_BELL_CHECKPOINT_PROJECTILES
                || pending.projectiles.len()
                    != usize::try_from(self.weapon.projectile_count())
                        .map_err(|_| CombatError::InvalidBellCheckpoint)?
                || pending.projectiles.iter().any(|projectile| {
                    let direction = SimulationVector::new(
                        f32::from_bits(projectile.direction_x_bits),
                        f32::from_bits(projectile.direction_y_bits),
                    );
                    let range = f32::from_bits(projectile.range_tiles_bits);
                    let speed = f32::from_bits(projectile.speed_tiles_per_second_bits);
                    let radius = f32::from_bits(projectile.radius_tiles_bits);
                    !direction.is_finite()
                        || (direction.length_squared() - 1.0).abs() > RANGE_EPSILON
                        || !range.is_finite()
                        || !(0.0..=MAX_CHECKPOINT_PROJECTILE_SCALAR).contains(&range)
                        || range <= 0.0
                        || !speed.is_finite()
                        || !(0.0..=MAX_CHECKPOINT_PROJECTILE_SCALAR).contains(&speed)
                        || speed <= 0.0
                        || !radius.is_finite()
                        || !(0.0..=MAX_CHECKPOINT_PROJECTILE_SCALAR).contains(&radius)
                        || radius <= 0.0
                        || projectile.raw_damage == 0
                        || projectile.max_projectiles_per_target == 0
                }))
        {
            return Err(CombatError::InvalidBellCheckpoint);
        }
        Ok(())
    }

    #[must_use]
    pub const fn pending_slipstep_sequence(&self) -> Option<u32> {
        match self.pending_slipstep {
            Some(pending) => Some(pending.press_sequence),
            None => None,
        }
    }

    #[must_use]
    pub const fn grave_mark_cooldown_remaining_ticks(&self) -> u32 {
        self.grave_mark_cooldown_remaining_ticks
    }

    #[must_use]
    pub const fn global_cooldown_remaining_ticks(&self) -> u32 {
        self.global_cooldown_remaining_ticks
    }

    #[must_use]
    pub const fn last_ability_1_press_sequence(&self) -> u32 {
        self.last_ability_1_press_sequence
    }

    #[must_use]
    pub const fn last_ability_2_press_sequence(&self) -> u32 {
        self.last_ability_2_press_sequence
    }

    #[must_use]
    pub const fn pending_grave_mark_sequence(&self) -> Option<u32> {
        match self.pending_grave_mark {
            Some(pending) => Some(pending.press_sequence),
            None => None,
        }
    }

    #[must_use]
    pub const fn active_grave_mark(&self) -> Option<ActiveGraveMark> {
        self.active_grave_mark
    }

    #[must_use]
    pub fn projectiles(&self) -> &[FriendlyProjectile] {
        &self.projectiles
    }

    #[must_use]
    pub const fn oath(&self) -> Option<GraveArbalistOath> {
        self.oath
    }

    #[must_use]
    pub fn nail_traps(&self) -> &NailTrapField {
        &self.nail_traps
    }

    /// Drains all run-owned friendly projectiles in stable identity order after local death.
    pub fn clear_projectiles_for_local_death(&mut self) -> Vec<FriendlyProjectile> {
        self.reset_bell_debt(BellDebtResetReason::Death);
        self.projectiles.sort_by_key(FriendlyProjectile::id);
        std::mem::take(&mut self.projectiles)
    }

    /// Advances one transactionally committed authoritative combat tick.
    pub fn step(
        &mut self,
        action: CombatAction,
        player_position: SimulationVector,
        collision_world: &ProjectileCollisionWorld,
    ) -> Result<CombatStep, CombatError> {
        let mut next = self.clone();
        let (result, _) = next.step_inner(action, player_position, collision_world, None)?;
        *self = next;
        Ok(result)
    }

    /// Advances movement and combat as one transactionally committed avatar tick.
    pub fn step_with_movement(
        &mut self,
        movement: &mut PlayerMovementState,
        action: CombatAction,
        arena: &ArenaGeometry,
        collision_world: &ProjectileCollisionWorld,
    ) -> Result<CombatStep, CombatError> {
        self.step_with_movement_outcome(movement, action, arena, collision_world)
            .map(|(combat, _)| combat)
    }

    /// Advances one avatar tick and exposes the exact single committed movement outcome.
    pub fn step_with_movement_outcome(
        &mut self,
        movement: &mut PlayerMovementState,
        action: CombatAction,
        arena: &ArenaGeometry,
        collision_world: &ProjectileCollisionWorld,
    ) -> Result<(CombatStep, MovementStep), CombatError> {
        let mut next_combat = self.clone();
        let mut next_movement = *movement;
        let (result, movement_step) = next_combat.step_inner(
            action,
            next_movement.position(),
            collision_world,
            Some((&mut next_movement, arena)),
        )?;
        let movement_step = movement_step.ok_or(CombatError::MovementOutcomeMissing)?;
        *self = next_combat;
        *movement = next_movement;
        Ok((result, movement_step))
    }

    fn step_inner(
        &mut self,
        action: CombatAction,
        mut player_position: SimulationVector,
        collision_world: &ProjectileCollisionWorld,
        movement: Option<(&mut PlayerMovementState, &ArenaGeometry)>,
    ) -> Result<(CombatStep, Option<MovementStep>), CombatError> {
        if !player_position.is_finite() {
            return Err(CombatError::NonFinitePlayerPosition);
        }
        self.validate_sequence(action)?;
        self.tick = self.tick.checked_next().ok_or(CombatError::TickExhausted)?;
        let mut step = CombatStep {
            tick: self.tick,
            attacker_multiplier_basis_points: self.outgoing_direct_damage_basis_points,
            ..CombatStep::default()
        };
        self.advance_active_mark(&mut step.mark_transitions);
        self.advance_nail_traps(collision_world, &mut step)?;
        self.advance_projectiles(
            collision_world,
            &mut step.collisions,
            &mut step.expirations,
            &mut step.raw_damage_intents,
            &mut step.mark_transitions,
            &mut step.nail_traps,
        )?;
        self.interval_remaining_ticks = self.interval_remaining_ticks.saturating_sub(1);
        self.grave_mark_cooldown_remaining_ticks =
            self.grave_mark_cooldown_remaining_ticks.saturating_sub(1);
        self.global_cooldown_remaining_ticks =
            self.global_cooldown_remaining_ticks.saturating_sub(1);
        self.slipstep_cooldown_remaining_ticks =
            self.slipstep_cooldown_remaining_ticks.saturating_sub(1);
        self.advance_slipstep_windows(player_position, &mut step.slipstep_transitions);

        self.process_grave_mark_input(action, player_position, &mut step)?;
        self.process_slipstep_input(action, player_position, &mut step)?;

        let mut slipstep_travelled = false;
        let mut movement_step = None;
        if let Some((movement, arena)) = movement {
            if let Some(active) = self.active_slipstep {
                slipstep_travelled = true;
                let displacement = if active.remaining_ticks == 1 {
                    active.target_position - movement.position()
                } else {
                    active.direction.vector() * self.slipstep.travel_per_tick_tiles()
                };
                let moved =
                    movement.apply_forced_displacement(displacement, collision_world, arena)?;
                movement_step = Some(MovementStep {
                    position: moved.position,
                    velocity: movement.velocity(),
                    collided: moved.solid.is_some(),
                });
                player_position = moved.position;
                step.direct_damage_reduction_basis_points =
                    self.slipstep.direct_damage_reduction_basis_points();
                let remaining = active.remaining_ticks.saturating_sub(1);
                let terminal = moved.solid.is_some() || remaining == 0;
                step.slipstep_transitions.push(SlipstepTransition {
                    tick: self.tick,
                    kind: if moved.solid.is_some() {
                        SlipstepTransitionKind::Collided
                    } else if terminal {
                        SlipstepTransitionKind::Completed
                    } else {
                        SlipstepTransitionKind::Travelled
                    },
                    press_sequence: active.press_sequence,
                    position: moved.position,
                    travelled_tiles: moved.travelled_tiles,
                    remaining_travel_ticks: if terminal { 0 } else { remaining },
                    solid: moved.solid,
                });
                self.active_slipstep = (!terminal).then_some(ActiveSlipstep {
                    remaining_ticks: remaining,
                    ..active
                });
            } else {
                movement_step = Some(movement.step(action.movement, arena)?);
                player_position = movement.position();
            }
            self.update_stillness(
                movement.velocity(),
                movement.config().final_speed_tiles_per_second,
                slipstep_travelled,
                &mut step.focused_transitions,
            );
        } else if self.active_slipstep.is_some() {
            return Err(CombatError::MovementStateRequired);
        }

        self.emit_due_bell_repeat(player_position, &mut step.shots)?;
        self.resolve_primary(action, player_position, &mut step)?;
        Ok((step, movement_step))
    }

    fn advance_nail_traps(
        &mut self,
        collision_world: &ProjectileCollisionWorld,
        step: &mut CombatStep,
    ) -> Result<(), CombatError> {
        let enemies = collision_world
            .enemies()
            .iter()
            .map(|enemy| NailTrapEnemy {
                entity_id: enemy.id(),
                position: enemy.center(),
                radius_tiles: enemy.radius_tiles(),
            })
            .collect::<Vec<_>>();
        step.nail_traps = self.nail_traps.step(self.tick, &enemies)?;
        step.raw_damage_intents
            .extend(
                step.nail_traps
                    .triggers
                    .iter()
                    .map(|trigger| RawDamageIntent {
                        tick: trigger.tick,
                        projectile_id: trigger.trap_id,
                        source: RawDamageIntentSource::NailTrap,
                        target: trigger.target_id,
                        base_raw_damage: trigger.snapshot_weapon_raw_damage,
                        multiplier_basis_points: crate::NAILKEEPER_DAMAGE_BASIS_POINTS,
                        resolved_raw_damage: trigger.raw_damage,
                        contact_ordinal: 0,
                    }),
            );
        Ok(())
    }

    fn resolve_primary(
        &mut self,
        action: CombatAction,
        player_position: SimulationVector,
        step: &mut CombatStep,
    ) -> Result<(), CombatError> {
        let new_press = action.primary_press_sequence > self.last_press_sequence;
        if new_press {
            self.last_press_sequence = action.primary_press_sequence;
        }
        self.previous_primary_held = action.primary_held;
        if (action.primary_held || new_press) && self.interval_remaining_ticks == 0 {
            let empowered = self.empowered_primary_remaining_ticks > 0;
            let focused = self.focused;
            let speed_multiplier = if empowered {
                BASIS_POINTS_PER_ONE
                    .checked_add(self.slipstep.projectile_speed_bonus_basis_points())
                    .ok_or(CombatError::ProjectileModifierOverflow)?
            } else if focused {
                BASIS_POINTS_PER_ONE
                    .checked_add(self.stillness.projectile_speed_bonus_basis_points())
                    .ok_or(CombatError::ProjectileModifierOverflow)?
            } else {
                BASIS_POINTS_PER_ONE
            };
            let speed = scale_f32_basis_points(
                self.weapon.projectile_speed_tiles_per_second(),
                speed_multiplier,
            );
            let pierce = if empowered {
                self.weapon
                    .pierce()
                    .checked_add(self.slipstep.pierce_bonus())
                    .ok_or(CombatError::ProjectileModifierOverflow)?
            } else {
                self.weapon.pierce()
            };
            let directions = self.weapon.projectile_directions_millionths().to_vec();
            let mut released_projectiles = Vec::with_capacity(directions.len());
            for local_direction in directions {
                let projectile_id = self
                    .projectile_ids
                    .allocate()
                    .ok_or(CombatError::ProjectileIdExhausted)?;
                let projectile = FriendlyProjectile {
                    id: projectile_id,
                    source: FriendlyProjectileSource::Primary,
                    position: player_position,
                    origin: player_position,
                    direction: compose_local_direction(action.aim, local_direction)?,
                    distance_travelled_tiles: 0.0,
                    range_tiles: self.weapon.range_tiles(),
                    speed_tiles_per_second: speed,
                    radius_tiles: self.weapon.projectile_radius_tiles(),
                    raw_damage: if focused {
                        self.stillness
                            .focused_primary_raw_damage(self.weapon.raw_damage())?
                    } else {
                        self.weapon.raw_damage()
                    },
                    pierce_remaining: pierce,
                    stops_on_first_enemy: self.weapon.stops_on_first_enemy(),
                    empowered_by_slipstep: empowered,
                    hit_targets: Vec::new(),
                    focused_by_stillness: focused,
                    release_tick: self.tick,
                    max_projectiles_per_target: self.weapon.max_projectiles_per_target(),
                    damage_multiplier_basis_points: BASIS_POINTS_PER_ONE,
                };
                self.projectiles.push(projectile.clone());
                released_projectiles.push(projectile.clone());
                step.shots.push(ShotEvent {
                    tick: self.tick,
                    press_sequence: action.primary_press_sequence,
                    projectile,
                });
            }
            self.record_bell_primary_release(action.primary_press_sequence, released_projectiles)?;
            self.interval_remaining_ticks = self.weapon.attack_interval_ticks();
            if empowered {
                self.empowered_primary_remaining_ticks = 0;
                step.slipstep_transitions.push(SlipstepTransition {
                    tick: self.tick,
                    kind: SlipstepTransitionKind::EmpowermentConsumed,
                    press_sequence: action.primary_press_sequence,
                    position: player_position,
                    travelled_tiles: 0.0,
                    remaining_travel_ticks: self.slipstep_remaining_travel_ticks(),
                    solid: None,
                });
            }
        }
        Ok(())
    }

    fn record_bell_primary_release(
        &mut self,
        press_sequence: u32,
        projectiles: Vec<FriendlyProjectile>,
    ) -> Result<(), CombatError> {
        let Some(definition) = self.bell_debt else {
            return Ok(());
        };
        self.bell_primary_release_count = self
            .bell_primary_release_count
            .checked_add(1)
            .ok_or(CombatError::BellCounterOverflow)?;
        if self.bell_primary_release_count < definition.accepted_primary_emissions_per_repeat {
            return Ok(());
        }
        if self.bell_primary_release_count != definition.accepted_primary_emissions_per_repeat
            || self.pending_bell_repeat.is_some()
        {
            return Err(CombatError::BellPendingRepeatConflict);
        }
        self.bell_primary_release_count = 0;
        let due_tick = Tick(
            self.tick
                .0
                .checked_add(u64::from(definition.repeat_delay_ticks))
                .ok_or(CombatError::TickExhausted)?,
        );
        self.pending_bell_repeat = Some(PendingBellRepeat {
            due_tick,
            press_sequence,
            projectiles,
        });
        Ok(())
    }

    fn emit_due_bell_repeat(
        &mut self,
        player_position: SimulationVector,
        shots: &mut Vec<ShotEvent>,
    ) -> Result<(), CombatError> {
        let Some(pending) = self.pending_bell_repeat.as_ref() else {
            return Ok(());
        };
        if pending.due_tick != self.tick {
            return Ok(());
        }
        let definition = self
            .bell_debt
            .ok_or(CombatError::BellPendingRepeatConflict)?;
        let pending = self
            .pending_bell_repeat
            .take()
            .ok_or(CombatError::BellPendingRepeatConflict)?;
        for mut projectile in pending.projectiles {
            projectile.id = self
                .projectile_ids
                .allocate()
                .ok_or(CombatError::ProjectileIdExhausted)?;
            projectile.source = FriendlyProjectileSource::BellDebtRepeat;
            projectile.position = player_position;
            projectile.origin = player_position;
            projectile.distance_travelled_tiles = 0.0;
            projectile.hit_targets.clear();
            projectile.release_tick = self.tick;
            projectile.damage_multiplier_basis_points =
                definition.repeat_damage_multiplier_basis_points;
            self.projectiles.push(projectile.clone());
            shots.push(ShotEvent {
                tick: self.tick,
                press_sequence: pending.press_sequence,
                projectile,
            });
        }
        Ok(())
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
        if action.ability_1_press_sequence < self.last_ability_1_press_sequence {
            return Err(CombatError::StaleAbilityOnePressSequence {
                received: action.ability_1_press_sequence,
                last: self.last_ability_1_press_sequence,
            });
        }
        if action.ability_2_press_sequence < self.last_ability_2_press_sequence {
            return Err(CombatError::StaleAbilityTwoPressSequence {
                received: action.ability_2_press_sequence,
                last: self.last_ability_2_press_sequence,
            });
        }
        Ok(())
    }

    fn advance_slipstep_windows(
        &mut self,
        position: SimulationVector,
        transitions: &mut Vec<SlipstepTransition>,
    ) {
        if self.exhaustion_remaining_ticks > 0 {
            self.exhaustion_remaining_ticks -= 1;
            if self.exhaustion_remaining_ticks == 0 {
                transitions.push(SlipstepTransition {
                    tick: self.tick,
                    kind: SlipstepTransitionKind::ExhaustionExpired,
                    press_sequence: self.last_ability_2_press_sequence,
                    position,
                    travelled_tiles: 0.0,
                    remaining_travel_ticks: self.slipstep_remaining_travel_ticks(),
                    solid: None,
                });
            }
        }
        if self.empowered_primary_remaining_ticks > 0 {
            self.empowered_primary_remaining_ticks -= 1;
            if self.empowered_primary_remaining_ticks == 0 {
                transitions.push(SlipstepTransition {
                    tick: self.tick,
                    kind: SlipstepTransitionKind::EmpowermentExpired,
                    press_sequence: self.last_ability_2_press_sequence,
                    position,
                    travelled_tiles: 0.0,
                    remaining_travel_ticks: self.slipstep_remaining_travel_ticks(),
                    solid: None,
                });
            }
        }
    }

    fn process_slipstep_input(
        &mut self,
        action: CombatAction,
        position: SimulationVector,
        step: &mut CombatStep,
    ) -> Result<(), CombatError> {
        let new_press = action.ability_2_press_sequence > self.last_ability_2_press_sequence;
        if new_press {
            self.last_ability_2_press_sequence = action.ability_2_press_sequence;
            let movement = action.movement.normalized_vector();
            let direction = if movement.length_squared() > AIM_EPSILON_SQUARED {
                AimDirection::new(movement)?
            } else {
                AimDirection::new(action.aim.vector() * -1.0)?
            };
            if self.exhaustion_remaining_ticks > 0 {
                step.slipstep_inputs.push(SlipstepInputEvent {
                    tick: self.tick,
                    press_sequence: action.ability_2_press_sequence,
                    result: SlipstepInputResult::BlockedByExhaustion {
                        remaining_ticks: self.exhaustion_remaining_ticks,
                    },
                });
                return Ok(());
            }
            let readiness = self
                .slipstep_cooldown_remaining_ticks
                .max(self.global_cooldown_remaining_ticks);
            if readiness == 0 {
                self.begin_slipstep(action.ability_2_press_sequence, direction, position, step);
            } else if readiness <= self.slipstep.input_buffer_ticks() {
                self.pending_slipstep = Some(PendingSlipstep {
                    press_sequence: action.ability_2_press_sequence,
                    direction,
                });
                step.slipstep_inputs.push(SlipstepInputEvent {
                    tick: self.tick,
                    press_sequence: action.ability_2_press_sequence,
                    result: SlipstepInputResult::Buffered {
                        readiness_ticks: readiness,
                    },
                });
            } else {
                step.slipstep_inputs.push(SlipstepInputEvent {
                    tick: self.tick,
                    press_sequence: action.ability_2_press_sequence,
                    result: SlipstepInputResult::ConsumedTooEarly {
                        readiness_ticks: readiness,
                    },
                });
            }
        }
        if self.exhaustion_remaining_ticks == 0
            && self.slipstep_cooldown_remaining_ticks == 0
            && self.global_cooldown_remaining_ticks == 0
            && let Some(pending) = self.pending_slipstep.take()
        {
            self.begin_slipstep(pending.press_sequence, pending.direction, position, step);
        }
        Ok(())
    }

    fn begin_slipstep(
        &mut self,
        press_sequence: u32,
        direction: AimDirection,
        position: SimulationVector,
        step: &mut CombatStep,
    ) {
        if self.focused {
            self.focused = false;
            self.stillness_ticks = 0;
            step.focused_transitions.push(FocusedTransition {
                tick: self.tick,
                kind: FocusedTransitionKind::BrokenBySlipstep,
                stillness_ticks: 0,
            });
        }
        self.pending_slipstep = None;
        self.slipstep_cooldown_remaining_ticks = self.slipstep.cooldown_ticks();
        self.global_cooldown_remaining_ticks = self.slipstep.global_cooldown_ticks();
        self.exhaustion_remaining_ticks = self.slipstep.exhaustion_ticks();
        self.empowered_primary_remaining_ticks = self.slipstep.empowered_window_ticks();
        self.active_slipstep = Some(ActiveSlipstep {
            press_sequence,
            direction,
            remaining_ticks: self.slipstep.travel_ticks(),
            target_position: position + direction.vector() * self.slipstep.travel_tiles(),
        });
        step.slipstep_inputs.push(SlipstepInputEvent {
            tick: self.tick,
            press_sequence,
            result: SlipstepInputResult::Began,
        });
        step.slipstep_transitions.push(SlipstepTransition {
            tick: self.tick,
            kind: SlipstepTransitionKind::Began,
            press_sequence,
            position,
            travelled_tiles: 0.0,
            remaining_travel_ticks: self.slipstep.travel_ticks(),
            solid: None,
        });
    }

    fn update_stillness(
        &mut self,
        velocity: SimulationVector,
        final_speed: f32,
        slipstep_travelled: bool,
        transitions: &mut Vec<FocusedTransition>,
    ) {
        if slipstep_travelled {
            self.stillness_ticks = 0;
            return;
        }
        if movement_is_still(
            velocity,
            final_speed,
            self.stillness.movement_threshold_basis_points(),
        ) {
            if !self.focused {
                self.stillness_ticks = self.stillness_ticks.saturating_add(1);
                if self.stillness_ticks >= self.stillness.activation_ticks() {
                    self.focused = true;
                    transitions.push(FocusedTransition {
                        tick: self.tick,
                        kind: FocusedTransitionKind::Gained,
                        stillness_ticks: self.stillness_ticks,
                    });
                }
            }
        } else {
            let was_focused = self.focused;
            self.focused = false;
            self.stillness_ticks = 0;
            if was_focused {
                transitions.push(FocusedTransition {
                    tick: self.tick,
                    kind: FocusedTransitionKind::BrokenByMovement,
                    stillness_ticks: 0,
                });
            }
        }
    }

    pub fn break_focused_from_damage(&mut self) -> Option<FocusedTransition> {
        if !self.focused {
            return None;
        }
        self.focused = false;
        self.stillness_ticks = 0;
        Some(FocusedTransition {
            tick: self.tick,
            kind: FocusedTransitionKind::BrokenByDamage,
            stillness_ticks: 0,
        })
    }

    fn advance_active_mark(&mut self, transitions: &mut Vec<GraveMarkTransition>) {
        let Some(mut mark) = self.active_grave_mark else {
            return;
        };
        if mark.remaining_ticks == 1 {
            transitions.push(GraveMarkTransition {
                tick: self.tick,
                kind: GraveMarkTransitionKind::Expired,
                target: mark.target,
                previous_target: None,
                source_projectile_id: mark.source_projectile_id,
                remaining_ticks: 0,
            });
            self.active_grave_mark = None;
        } else {
            mark.remaining_ticks -= 1;
            self.active_grave_mark = Some(mark);
        }
    }

    fn process_grave_mark_input(
        &mut self,
        action: CombatAction,
        player_position: SimulationVector,
        step: &mut CombatStep,
    ) -> Result<(), CombatError> {
        let new_press = action.ability_1_press_sequence > self.last_ability_1_press_sequence;
        let readiness = self
            .grave_mark_cooldown_remaining_ticks
            .max(self.global_cooldown_remaining_ticks);
        if new_press {
            self.last_ability_1_press_sequence = action.ability_1_press_sequence;
            if readiness <= self.grave_mark.input_buffer_ticks() {
                self.pending_grave_mark = Some(PendingGraveMark {
                    press_sequence: action.ability_1_press_sequence,
                    aim: action.aim,
                });
                if readiness > 0 {
                    step.grave_mark_inputs.push(GraveMarkInputEvent {
                        tick: self.tick,
                        press_sequence: action.ability_1_press_sequence,
                        result: GraveMarkInputResult::Buffered {
                            readiness_ticks: readiness,
                        },
                    });
                }
            } else {
                step.grave_mark_inputs.push(GraveMarkInputEvent {
                    tick: self.tick,
                    press_sequence: action.ability_1_press_sequence,
                    result: GraveMarkInputResult::ConsumedTooEarly {
                        readiness_ticks: readiness,
                    },
                });
            }
        }

        if readiness == 0
            && let Some(pending) = self.pending_grave_mark.take()
        {
            let projectile_id = self.fire_grave_mark(pending, player_position, &mut step.shots)?;
            step.grave_mark_inputs.push(GraveMarkInputEvent {
                tick: self.tick,
                press_sequence: pending.press_sequence,
                result: GraveMarkInputResult::Fired { projectile_id },
            });
        }
        Ok(())
    }

    fn fire_grave_mark(
        &mut self,
        pending: PendingGraveMark,
        player_position: SimulationVector,
        shots: &mut Vec<ShotEvent>,
    ) -> Result<EntityId, CombatError> {
        let projectile_id = self
            .projectile_ids
            .allocate()
            .ok_or(CombatError::ProjectileIdExhausted)?;
        let projectile = FriendlyProjectile {
            id: projectile_id,
            source: FriendlyProjectileSource::GraveMark,
            position: player_position,
            origin: player_position,
            direction: pending.aim,
            distance_travelled_tiles: 0.0,
            range_tiles: self.grave_mark.range_tiles(),
            speed_tiles_per_second: self.grave_mark.projectile_speed_tiles_per_second(),
            radius_tiles: self.grave_mark.projectile_radius_tiles(),
            raw_damage: self
                .grave_mark
                .grave_mark_raw_intent(self.weapon.raw_damage())?,
            pierce_remaining: 0,
            stops_on_first_enemy: true,
            empowered_by_slipstep: false,
            hit_targets: Vec::new(),
            focused_by_stillness: false,
            release_tick: self.tick,
            max_projectiles_per_target: 1,
            damage_multiplier_basis_points: BASIS_POINTS_PER_ONE,
        };
        self.projectiles.push(projectile.clone());
        shots.push(ShotEvent {
            tick: self.tick,
            press_sequence: pending.press_sequence,
            projectile,
        });
        self.grave_mark_cooldown_remaining_ticks = self.grave_mark.cooldown_ticks();
        self.global_cooldown_remaining_ticks = self.grave_mark.global_cooldown_ticks();
        Ok(projectile_id)
    }

    #[allow(clippy::too_many_lines)] // One loop preserves stable sweep/contact/cap ordering.
    fn advance_projectiles(
        &mut self,
        collision_world: &ProjectileCollisionWorld,
        collisions: &mut Vec<ProjectileCollision>,
        expirations: &mut Vec<ProjectileExpired>,
        raw_damage_intents: &mut Vec<RawDamageIntent>,
        mark_transitions: &mut Vec<GraveMarkTransition>,
        nail_trap_step: &mut NailTrapStep,
    ) -> Result<(), CombatError> {
        let mut terminal_projectiles = Vec::new();
        let mut release_target_hits = initial_release_target_hits(&self.projectiles);
        for projectile in &mut self.projectiles {
            let remaining = projectile.range_tiles - projectile.distance_travelled_tiles;
            debug_assert_eq!(TICKS_PER_SECOND, 30);
            let full_step = projectile.speed_tiles_per_second / TICKS_PER_SECOND_F32;
            let travel = full_step.min(remaining.max(0.0));
            let mut unspent = travel;
            let mut terminal = false;
            while unspent > RANGE_EPSILON && !terminal {
                let displacement = projectile.direction.vector() * unspent;
                let ignored_targets = capped_release_targets(projectile, &release_target_hits);
                let hit = collision_world.sweep_circle_ignoring_enemies(
                    projectile.position,
                    displacement,
                    projectile.radius_tiles,
                    &ignored_targets,
                )?;
                let Some(hit) = hit else {
                    projectile.position = projectile.position + displacement;
                    projectile.distance_travelled_tiles += unspent;
                    break;
                };
                let realized_travel = unspent * hit.fraction;
                projectile.position = projectile.position + displacement * hit.fraction;
                projectile.distance_travelled_tiles += realized_travel;
                unspent -= realized_travel;
                if !projectile.position.is_finite()
                    || !projectile.distance_travelled_tiles.is_finite()
                {
                    return Err(CombatError::NonFiniteCollisionResult);
                }
                let contact_ordinal = u32::try_from(projectile.hit_targets.len())
                    .map_err(|_| CombatError::ContactOrdinalExhausted)?;
                collisions.push(ProjectileCollision {
                    tick: self.tick,
                    projectile_id: projectile.id,
                    source: projectile.source,
                    target: hit.target,
                    final_position: projectile.position,
                    distance_travelled_tiles: projectile.distance_travelled_tiles,
                    contact_ordinal,
                    empowered_by_slipstep: projectile.empowered_by_slipstep,
                    focused_by_stillness: projectile.focused_by_stillness,
                    projectile_continues: false,
                });
                match hit.target {
                    CollisionTarget::Enemy(target) => {
                        record_enemy_contact(
                            &self.grave_mark,
                            self.weapon.raw_damage(),
                            &mut self.active_grave_mark,
                            EnemyContactFact {
                                tick: self.tick,
                                projectile_id: projectile.id,
                                source: projectile.source,
                                target,
                                raw_damage: projectile.raw_damage,
                                contact_ordinal,
                                damage_multiplier_basis_points: projectile
                                    .damage_multiplier_basis_points,
                            },
                            raw_damage_intents,
                            mark_transitions,
                        )?;
                        if projectile.source == FriendlyProjectileSource::GraveMark
                            && self.oath == Some(GraveArbalistOath::Nailkeeper)
                        {
                            spawn_nail_trap(
                                &mut self.projectile_ids,
                                &mut self.nail_traps,
                                projectile.position,
                                self.tick,
                                self.weapon.raw_damage(),
                                nail_trap_step,
                            )?;
                        }
                        match projectile.hit_targets.binary_search(&target) {
                            Ok(_) => return Err(CombatError::DuplicateProjectileTarget(target)),
                            Err(index) => projectile.hit_targets.insert(index, target),
                        }
                        record_release_target_hit(&mut release_target_hits, projectile, target);
                        if projectile.pierce_remaining > 0 {
                            projectile.pierce_remaining -= 1;
                            collisions
                                .last_mut()
                                .expect("collision just pushed")
                                .projectile_continues = true;
                        } else {
                            terminal = true;
                        }
                    }
                    CollisionTarget::Solid(_) => {
                        if projectile.source == FriendlyProjectileSource::GraveMark
                            && self.oath == Some(GraveArbalistOath::Nailkeeper)
                        {
                            spawn_nail_trap(
                                &mut self.projectile_ids,
                                &mut self.nail_traps,
                                projectile.position,
                                self.tick,
                                self.weapon.raw_damage(),
                                nail_trap_step,
                            )?;
                        }
                        terminal = true;
                    }
                }
            }
            if terminal {
                terminal_projectiles.push(projectile.id);
            } else if remaining <= full_step + RANGE_EPSILON {
                projectile.distance_travelled_tiles = projectile.range_tiles;
                expirations.push(ProjectileExpired {
                    tick: self.tick,
                    projectile_id: projectile.id,
                    source: projectile.source,
                    final_position: projectile.position,
                    distance_travelled_tiles: projectile.range_tiles,
                });
            }
        }
        if !terminal_projectiles.is_empty() || !expirations.is_empty() {
            self.projectiles.retain(|projectile| {
                !terminal_projectiles.contains(&projectile.id)
                    && expirations
                        .iter()
                        .all(|expiration| expiration.projectile_id != projectile.id)
            });
        }
        Ok(())
    }
}

fn spawn_nail_trap(
    ids: &mut EntityIdAllocator,
    field: &mut NailTrapField,
    position: SimulationVector,
    tick: Tick,
    weapon_raw_damage: u32,
    output: &mut NailTrapStep,
) -> Result<(), CombatError> {
    let trap_id = ids.allocate().ok_or(CombatError::ProjectileIdExhausted)?;
    let removed = field.spawn(trap_id, position, tick, weapon_raw_damage)?;
    output.spawned.push(trap_id);
    output.removals.extend(removed);
    Ok(())
}

fn capped_release_targets(
    projectile: &FriendlyProjectile,
    release_target_hits: &BTreeMap<(FriendlyProjectileSource, Tick, EntityId), u32>,
) -> Vec<EntityId> {
    let mut ignored = projectile.hit_targets.clone();
    ignored.extend(release_target_hits.iter().filter_map(
        |((source, release_tick, target), hits)| {
            (*source == projectile.source
                && *release_tick == projectile.release_tick
                && *hits >= projectile.max_projectiles_per_target)
                .then_some(*target)
        },
    ));
    ignored.sort_unstable();
    ignored.dedup();
    ignored
}

fn initial_release_target_hits(
    projectiles: &[FriendlyProjectile],
) -> BTreeMap<(FriendlyProjectileSource, Tick, EntityId), u32> {
    let mut hits = BTreeMap::new();
    for projectile in projectiles {
        for target in &projectile.hit_targets {
            *hits
                .entry((projectile.source, projectile.release_tick, *target))
                .or_default() += 1;
        }
    }
    hits
}

fn record_release_target_hit(
    hits: &mut BTreeMap<(FriendlyProjectileSource, Tick, EntityId), u32>,
    projectile: &FriendlyProjectile,
    target: EntityId,
) {
    *hits
        .entry((projectile.source, projectile.release_tick, target))
        .or_default() += 1;
}

#[derive(Debug, Clone, Copy)]
struct EnemyContactFact {
    tick: Tick,
    projectile_id: EntityId,
    source: FriendlyProjectileSource,
    target: EntityId,
    raw_damage: u32,
    contact_ordinal: u32,
    damage_multiplier_basis_points: u32,
}

#[allow(clippy::cast_precision_loss)] // Basis-point values are small validated gameplay inputs.
fn scale_f32_basis_points(value: f32, basis_points: u32) -> f32 {
    value * basis_points as f32 / BASIS_POINTS_PER_ONE as f32
}

fn movement_is_still(
    velocity: SimulationVector,
    final_speed: f32,
    threshold_basis_points: u32,
) -> bool {
    velocity.length() < scale_f32_basis_points(final_speed, threshold_basis_points)
}

fn record_enemy_contact(
    grave_mark: &GraveMarkDefinition,
    weapon_raw_damage: u32,
    active_mark: &mut Option<ActiveGraveMark>,
    contact: EnemyContactFact,
    raw_damage_intents: &mut Vec<RawDamageIntent>,
    mark_transitions: &mut Vec<GraveMarkTransition>,
) -> Result<(), CombatError> {
    match contact.source {
        FriendlyProjectileSource::Primary | FriendlyProjectileSource::BellDebtRepeat => {
            let marked = active_mark.is_some_and(|mark| mark.target == contact.target);
            let multiplier_basis_points = if marked {
                BASIS_POINTS_PER_ONE
                    .checked_add(grave_mark.marked_primary_bonus_basis_points())
                    .ok_or(CombatError::IntentMath(IntentMathError::Overflow))?
            } else {
                BASIS_POINTS_PER_ONE
            };
            let ordinary_raw_damage = if marked {
                grave_mark.marked_primary_raw_intent(contact.raw_damage)?
            } else {
                contact.raw_damage
            };
            let resolved_raw_damage = scale_u32_basis_points_half_up(
                ordinary_raw_damage,
                contact.damage_multiplier_basis_points,
            )?;
            let multiplier_basis_points = scale_u32_basis_points_half_up(
                multiplier_basis_points,
                contact.damage_multiplier_basis_points,
            )?;
            raw_damage_intents.push(RawDamageIntent {
                tick: contact.tick,
                projectile_id: contact.projectile_id,
                source: if contact.source == FriendlyProjectileSource::BellDebtRepeat {
                    RawDamageIntentSource::BellDebtRepeat
                } else {
                    RawDamageIntentSource::Primary
                },
                target: contact.target,
                base_raw_damage: contact.raw_damage,
                multiplier_basis_points,
                resolved_raw_damage,
                contact_ordinal: contact.contact_ordinal,
            });
        }
        FriendlyProjectileSource::GraveMark => {
            raw_damage_intents.push(RawDamageIntent {
                tick: contact.tick,
                projectile_id: contact.projectile_id,
                source: RawDamageIntentSource::GraveMark,
                target: contact.target,
                base_raw_damage: weapon_raw_damage,
                multiplier_basis_points: grave_mark.weapon_damage_multiplier_basis_points(),
                resolved_raw_damage: contact.raw_damage,
                contact_ordinal: contact.contact_ordinal,
            });
            let previous_target = active_mark.map(|mark| mark.target);
            let kind = match previous_target {
                None => GraveMarkTransitionKind::Applied,
                Some(previous) if previous == contact.target => GraveMarkTransitionKind::Refreshed,
                Some(_) => GraveMarkTransitionKind::Replaced,
            };
            *active_mark = Some(ActiveGraveMark {
                target: contact.target,
                remaining_ticks: grave_mark.duration_ticks(),
                source_projectile_id: contact.projectile_id,
            });
            mark_transitions.push(GraveMarkTransition {
                tick: contact.tick,
                kind,
                target: contact.target,
                previous_target,
                source_projectile_id: contact.projectile_id,
                remaining_ticks: grave_mark.duration_ticks(),
            });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CombatError {
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
    #[error("stale ability-1 press sequence {received}; last accepted is {last}")]
    StaleAbilityOnePressSequence { received: u32, last: u32 },
    #[error("stale ability-2 press sequence {received}; last accepted is {last}")]
    StaleAbilityTwoPressSequence { received: u32, last: u32 },
    #[error("Slipstep requires the authoritative movement state")]
    MovementStateRequired,
    #[error(transparent)]
    Oath(#[from] crate::OathMechanicError),
    #[error("movement-aware combat tick did not produce a movement outcome")]
    MovementOutcomeMissing,
    #[error("projectile modifier arithmetic overflowed")]
    ProjectileModifierOverflow,
    #[error("projectile contact ordinal exhausted u32")]
    ContactOrdinalExhausted,
    #[error("projectile attempted to hit enemy {0} more than once")]
    DuplicateProjectileTarget(EntityId),
    #[error("Slipstep direction failed validation: {0}")]
    SlipstepDirection(#[from] AimDirectionError),
    #[error("player movement failed: {0}")]
    Movement(#[from] MovementError),
    #[error("projectile collision failed: {0}")]
    Collision(#[from] CollisionError),
    #[error("projectile collision produced non-finite terminal state")]
    NonFiniteCollisionResult,
    #[error("raw-damage intent failed: {0}")]
    IntentMath(#[from] IntentMathError),
    #[error("outgoing direct-damage multiplier is outside the global cap")]
    InvalidOutgoingDirectDamageMultiplier,
    #[error("Bell Debt accepted-release counter overflowed")]
    BellCounterOverflow,
    #[error("Bell Debt pending-repeat state conflicts with its deterministic cadence")]
    BellPendingRepeatConflict,
    #[error("Bell Debt checkpoint state is invalid for the immutable loadout")]
    InvalidBellCheckpoint,
}

fn scale_u32_basis_points_half_up(value: u32, basis_points: u32) -> Result<u32, CombatError> {
    let scaled = u64::from(value)
        .checked_mul(u64::from(basis_points))
        .and_then(|product| product.checked_add(u64::from(BASIS_POINTS_PER_ONE / 2)))
        .ok_or(CombatError::ProjectileModifierOverflow)?
        / u64::from(BASIS_POINTS_PER_ONE);
    u32::try_from(scaled).map_err(|_| CombatError::ProjectileModifierOverflow)
}

#[allow(clippy::cast_precision_loss)]
fn compose_local_direction(
    aim: AimDirection,
    local_millionths: (i32, i32),
) -> Result<AimDirection, CombatError> {
    let local_x = local_millionths.0 as f32 / 1_000_000.0;
    let local_y = local_millionths.1 as f32 / 1_000_000.0;
    let base = aim.vector();
    Ok(AimDirection::new(SimulationVector::new(
        base.x * local_x - base.y * local_y,
        base.x * local_y + base.y * local_x,
    ))?)
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
            projectile_directions_millionths: vec![(1_000_000, 0)],
            max_projectiles_per_target: 1,
            pierce: 0,
            stops_on_first_enemy: true,
        })
        .expect("Pine Crossbow")
    }

    fn scatterbow() -> WeaponDefinition {
        WeaponDefinition::new(WeaponDefinitionParameters {
            content_id: "item.prototype.weapon.scatterbow".to_owned(),
            raw_damage: 12,
            attack_interval_ticks: 16,
            range_milli_tiles: 8_000,
            projectile_speed_milli_tiles_per_second: 10_500,
            projectile_radius_milli_tiles: 100,
            projectile_count: 3,
            projectile_directions_millionths: vec![
                (990_268, -139_173),
                (1_000_000, 0),
                (990_268, 139_173),
            ],
            max_projectiles_per_target: 2,
            pierce: 0,
            stops_on_first_enemy: true,
        })
        .expect("Scatterbow")
    }

    fn bell_debt() -> BellDebtDefinition {
        BellDebtDefinition {
            accepted_primary_emissions_per_repeat: 5,
            repeat_delay_ticks: 9,
            repeat_damage_multiplier_basis_points: 5_000,
            primary_attack_rate_multiplier_basis_points: 8_500,
            counts_legal_misses: true,
            generated_repeats_advance_counter: false,
            snapshots_aim_and_resolved_behavior: true,
            uses_live_origin_at_repeat: true,
            repeat_is_recursive: false,
            repeat_spends_cooldown_or_resource: false,
            counter_persists_reconnect_and_room_change: true,
            counter_resets_on_acquisition_purge_death_retirement_or_safe_transfer: true,
            cancel_pending_repeat_when_dead_transferred_or_primary_illegal: true,
        }
    }

    fn bell_combat() -> PlayerCombatState {
        PlayerCombatState::with_core_choices(
            scatterbow(),
            grave_mark(),
            slipstep(),
            stillness(),
            None,
            10_000,
            Some(bell_debt()),
        )
        .unwrap()
    }

    #[test]
    fn bell_counts_multibolt_misses_once_and_repeats_from_live_origin_at_nine_ticks() {
        let mut combat = bell_combat();
        let world = empty_world();
        let mut repeat = None;
        for tick in 1..=74 {
            let position = if tick == 74 {
                SimulationVector::new(9.0, 7.0)
            } else {
                SimulationVector::new(4.0, 7.0)
            };
            let step = combat
                .step(
                    CombatAction {
                        primary_held: true,
                        primary_press_sequence: 1,
                        ..CombatAction::default()
                    },
                    position,
                    &world,
                )
                .unwrap();
            if tick == 1 {
                assert_eq!(step.shots.len(), 3);
                assert_eq!(combat.bell_primary_release_count(), 1);
            }
            if !step.shots.is_empty()
                && step.shots[0].projectile.source() == FriendlyProjectileSource::BellDebtRepeat
            {
                repeat = Some(step);
            }
        }
        let repeat = repeat.expect("fifth accepted release must repeat");
        assert_eq!(repeat.tick, Tick(74));
        assert_eq!(repeat.shots.len(), 3);
        assert!(repeat.shots.iter().all(|shot| {
            shot.projectile.source() == FriendlyProjectileSource::BellDebtRepeat
                && shot.projectile.origin() == SimulationVector::new(9.0, 7.0)
                && shot.projectile.damage_multiplier_basis_points() == 5_000
        }));
        assert_eq!(combat.bell_primary_release_count(), 0);
        assert!(!combat.has_pending_bell_repeat());
    }

    #[test]
    fn bell_checkpoint_and_reset_seams_preserve_or_cancel_exact_state() {
        let world = empty_world();
        let mut original = bell_combat();
        for _ in 1..=65 {
            original
                .step(
                    CombatAction {
                        primary_held: true,
                        primary_press_sequence: 1,
                        ..CombatAction::default()
                    },
                    SimulationVector::new(4.0, 7.0),
                    &world,
                )
                .unwrap();
        }
        let checkpoint = original.export_bell_debt_checkpoint().unwrap();
        assert_eq!(checkpoint.primary_release_count(), 0);
        assert_eq!(
            checkpoint.schema_version(),
            BELL_DEBT_CHECKPOINT_SCHEMA_VERSION
        );
        assert!(checkpoint.has_pending_repeat());
        let encoded = checkpoint.canonical_bytes().unwrap();
        assert_eq!(
            BellDebtCheckpoint::decode_canonical(&encoded).unwrap(),
            checkpoint
        );
        assert_eq!(
            checkpoint.canonical_digest().unwrap(),
            *blake3::hash(&encoded).as_bytes()
        );

        let mut restored = bell_combat();
        restored.import_bell_debt_checkpoint(&checkpoint).unwrap();
        let mut repeated = None;
        for _ in 1..=9 {
            let step = restored
                .step(
                    CombatAction::default(),
                    SimulationVector::new(8.0, 6.0),
                    &world,
                )
                .unwrap();
            if !step.shots.is_empty() {
                repeated = Some(step);
            }
        }
        assert!(repeated.unwrap().shots.iter().all(|shot| {
            shot.projectile.source() == FriendlyProjectileSource::BellDebtRepeat
                && shot.projectile.origin() == SimulationVector::new(8.0, 6.0)
        }));

        original.cancel_pending_bell_repeat_for_primary_illegal();
        assert!(!original.has_pending_bell_repeat());
        original
            .step(
                CombatAction {
                    primary_held: true,
                    primary_press_sequence: 1,
                    ..CombatAction::default()
                },
                SimulationVector::new(4.0, 7.0),
                &world,
            )
            .unwrap();
        original.reset_bell_debt(BellDebtResetReason::SafeTransfer);
        assert_eq!(original.bell_primary_release_count(), 0);
        assert!(!original.has_pending_bell_repeat());

        let mut invalid = checkpoint;
        invalid.primary_release_count = 5;
        assert_eq!(
            bell_combat()
                .import_bell_debt_checkpoint(&invalid)
                .unwrap_err(),
            CombatError::InvalidBellCheckpoint
        );

        invalid.primary_release_count = 0;
        invalid.schema_version = BELL_DEBT_CHECKPOINT_SCHEMA_VERSION + 1;
        assert_eq!(
            bell_combat()
                .import_bell_debt_checkpoint(&invalid)
                .unwrap_err(),
            CombatError::InvalidBellCheckpoint
        );

        invalid.schema_version = BELL_DEBT_CHECKPOINT_SCHEMA_VERSION;
        invalid.pending_repeat.as_mut().unwrap().projectiles[0].direction_x_bits =
            f32::NAN.to_bits();
        assert_eq!(
            bell_combat()
                .import_bell_debt_checkpoint(&invalid)
                .unwrap_err(),
            CombatError::InvalidBellCheckpoint
        );
        assert_eq!(
            BellDebtCheckpoint::decode_canonical(&vec![0; MAX_BELL_DEBT_CHECKPOINT_BYTES + 1])
                .unwrap_err(),
            CombatError::InvalidBellCheckpoint
        );
    }

    #[test]
    fn fresh_arena_preserves_core_configuration_and_resets_transient_state() {
        let mut original = PlayerCombatState::with_core_choices(
            scatterbow(),
            grave_mark(),
            slipstep(),
            stillness(),
            Some(GraveArbalistOath::Nailkeeper),
            11_800,
            Some(bell_debt()),
        )
        .unwrap();
        original
            .step(
                CombatAction {
                    primary_held: true,
                    primary_press_sequence: 7,
                    ..CombatAction::default()
                },
                SimulationVector::new(4.0, 7.0),
                &empty_world(),
            )
            .unwrap();
        assert_eq!(original.bell_primary_release_count(), 1);
        assert!(!original.projectiles().is_empty());

        let fresh = original
            .fresh_arena_with_projectile_allocator(EntityIdAllocator::starting_at(
                NonZeroU64::new(90_000).unwrap(),
            ))
            .unwrap();

        assert_eq!(fresh.oath(), Some(GraveArbalistOath::Nailkeeper));
        assert_eq!(fresh.outgoing_direct_damage_basis_points(), 11_800);
        assert_eq!(fresh.tick(), Tick(0));
        assert_eq!(fresh.interval_remaining_ticks(), 0);
        assert_eq!(fresh.bell_primary_release_count(), 0);
        assert!(!fresh.has_pending_bell_repeat());
        assert!(fresh.projectiles().is_empty());
        assert!(fresh.nail_traps().traps().is_empty());
    }

    #[test]
    fn bell_repeat_multiplier_applies_after_ordinary_mark_resolution() {
        let target = EntityId::new(50).unwrap();
        let mut active_mark = Some(ActiveGraveMark {
            target,
            remaining_ticks: 30,
            source_projectile_id: EntityId::new(49).unwrap(),
        });
        let mut intents = Vec::new();
        record_enemy_contact(
            &grave_mark(),
            12,
            &mut active_mark,
            EnemyContactFact {
                tick: Tick(1),
                projectile_id: EntityId::new(51).unwrap(),
                source: FriendlyProjectileSource::BellDebtRepeat,
                target,
                raw_damage: 12,
                contact_ordinal: 0,
                damage_multiplier_basis_points: 5_000,
            },
            &mut intents,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].source, RawDamageIntentSource::BellDebtRepeat);
        assert_eq!(intents[0].resolved_raw_damage, 7);
        assert_eq!(intents[0].multiplier_basis_points, 5_750);
    }

    fn grave_mark() -> GraveMarkDefinition {
        GraveMarkDefinition::new(crate::GraveMarkDefinitionParameters {
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
        .expect("Grave Mark")
    }

    fn slipstep() -> SlipstepDefinition {
        SlipstepDefinition::new(crate::SlipstepDefinitionParameters {
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
        .expect("Slipstep")
    }

    fn stillness() -> StillnessDefinition {
        StillnessDefinition::new(crate::StillnessDefinitionParameters {
            content_id: "ability.arbalist.stillness".to_owned(),
            activation_ticks: 18,
            movement_threshold_basis_points: 2_000,
            projectile_speed_bonus_basis_points: 1_000,
            primary_damage_bonus_basis_points: 800,
            break_on_damage: true,
            break_on_slipstep: true,
        })
        .expect("Stillness")
    }

    fn combat_state() -> PlayerCombatState {
        PlayerCombatState::new(pine_crossbow(), grave_mark(), slipstep(), stillness())
            .expect("combat")
    }

    #[test]
    fn scatterbow_emits_three_distinct_bolts_and_caps_one_target_at_two() {
        let mut combat =
            PlayerCombatState::new(scatterbow(), grave_mark(), slipstep(), stillness())
                .expect("combat");
        let player = SimulationVector::new(4.0, 12.0);
        let target = EntityId::new(70).expect("target");
        let world = world_with(
            vec![],
            vec![
                EnemyHurtbox::new(target, SimulationVector::new(4.45, 12.0), 0.30)
                    .expect("hurtbox"),
            ],
        );
        let fired = combat
            .step(held(1, AimDirection::east()), player, &world)
            .expect("fire");
        assert_eq!(fired.shots.len(), 3);
        assert!(fired.shots[0].projectile.direction().vector().y < 0.0);
        assert_eq!(fired.shots[1].projectile.direction(), AimDirection::east());
        assert!(fired.shots[2].projectile.direction().vector().y > 0.0);
        let contact = combat
            .step(
                CombatAction {
                    primary_press_sequence: 1,
                    ..CombatAction::default()
                },
                player,
                &world,
            )
            .expect("advance");
        assert_eq!(contact.raw_damage_intents.len(), 2);
        assert!(
            contact
                .raw_damage_intents
                .iter()
                .all(|intent| intent.target == target && intent.resolved_raw_damage == 12)
        );
        assert_eq!(combat.projectiles().len(), 1);
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
            movement: MovementAction::default(),
            primary_held: true,
            primary_press_sequence: sequence,
            ability_1_press_sequence: 0,
            ability_2_press_sequence: 0,
        }
    }

    fn released(sequence: u32, aim: AimDirection) -> CombatAction {
        CombatAction {
            aim,
            movement: MovementAction::default(),
            primary_held: false,
            primary_press_sequence: sequence,
            ability_1_press_sequence: 0,
            ability_2_press_sequence: 0,
        }
    }

    fn ability_press(sequence: u32, aim: AimDirection) -> CombatAction {
        CombatAction {
            aim,
            movement: MovementAction::default(),
            primary_held: false,
            primary_press_sequence: 0,
            ability_1_press_sequence: sequence,
            ability_2_press_sequence: 0,
        }
    }

    fn released_with_ability(
        primary_sequence: u32,
        ability_sequence: u32,
        aim: AimDirection,
    ) -> CombatAction {
        CombatAction {
            aim,
            movement: MovementAction::default(),
            primary_held: false,
            primary_press_sequence: primary_sequence,
            ability_1_press_sequence: ability_sequence,
            ability_2_press_sequence: 0,
        }
    }

    fn ability_two_action(
        sequence: u32,
        movement: MovementAction,
        primary_held: bool,
        primary_sequence: u32,
    ) -> CombatAction {
        CombatAction {
            aim: AimDirection::east(),
            movement,
            primary_held,
            primary_press_sequence: primary_sequence,
            ability_1_press_sequence: 0,
            ability_2_press_sequence: sequence,
        }
    }

    fn movement_arena(pillars: Vec<TileRectangle>) -> ArenaGeometry {
        ArenaGeometry {
            id: "arena.slipstep_test".to_owned(),
            width_milli_tiles: 32_000,
            height_milli_tiles: 24_000,
            shell_thickness_milli_tiles: 1_000,
            player_spawn: TilePoint::new(4_000, 12_000),
            boss_spawn: TilePoint::new(24_000, 12_000),
            pillars,
            anchors: vec![],
        }
        .validated()
        .expect("Slipstep arena")
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
    fn grave_mark_fires_immediately_with_exact_definition_and_timers() {
        let mut combat = combat_state();
        let world = empty_world();
        let step = combat
            .step(
                ability_press(1, AimDirection::east()),
                SimulationVector::new(4.0, 12.0),
                &world,
            )
            .expect("Grave Mark fire");
        assert_eq!(step.shots.len(), 1);
        let projectile = &step.shots[0].projectile;
        assert_eq!(projectile.source(), FriendlyProjectileSource::GraveMark);
        assert_eq!(projectile.id().get(), 1);
        assert_eq!(projectile.raw_damage(), 36);
        assert!((projectile.range_tiles() - 11.0).abs() < f32::EPSILON);
        assert!((projectile.radius_tiles() - 0.12).abs() < f32::EPSILON);
        assert_eq!(combat.grave_mark_cooldown_remaining_ticks(), 150);
        assert_eq!(combat.global_cooldown_remaining_ticks(), 5);
        assert_eq!(
            step.grave_mark_inputs,
            [GraveMarkInputEvent {
                tick: Tick(1),
                press_sequence: 1,
                result: GraveMarkInputResult::Fired {
                    projectile_id: EntityId::new(1).expect("ID")
                }
            }]
        );
    }

    #[test]
    fn grave_mark_buffers_only_inside_exact_three_tick_window() {
        let mut combat = combat_state();
        let world = empty_world();
        let position = SimulationVector::new(4.0, 12.0);
        combat
            .step(ability_press(1, AimDirection::east()), position, &world)
            .expect("first fire");
        while combat.grave_mark_cooldown_remaining_ticks() > 5 {
            combat
                .step(
                    released_with_ability(0, 1, AimDirection::east()),
                    position,
                    &world,
                )
                .expect("cooldown");
        }
        let too_early = combat
            .step(ability_press(2, AimDirection::east()), position, &world)
            .expect("too early");
        assert_eq!(combat.grave_mark_cooldown_remaining_ticks(), 4);
        assert_eq!(combat.pending_grave_mark_sequence(), None);
        assert_eq!(
            too_early.grave_mark_inputs[0].result,
            GraveMarkInputResult::ConsumedTooEarly { readiness_ticks: 4 }
        );

        let buffered = combat
            .step(ability_press(3, AimDirection::east()), position, &world)
            .expect("buffer");
        assert_eq!(combat.grave_mark_cooldown_remaining_ticks(), 3);
        assert_eq!(combat.pending_grave_mark_sequence(), Some(3));
        assert_eq!(
            buffered.grave_mark_inputs[0].result,
            GraveMarkInputResult::Buffered { readiness_ticks: 3 }
        );
        let replaced_at_two = combat
            .step(ability_press(4, AimDirection::east()), position, &world)
            .expect("replace at two");
        assert_eq!(combat.pending_grave_mark_sequence(), Some(4));
        assert_eq!(
            replaced_at_two.grave_mark_inputs[0].result,
            GraveMarkInputResult::Buffered { readiness_ticks: 2 }
        );
        let replaced_at_one = combat
            .step(ability_press(5, AimDirection::east()), position, &world)
            .expect("replace at one");
        assert_eq!(combat.pending_grave_mark_sequence(), Some(5));
        assert_eq!(
            replaced_at_one.grave_mark_inputs[0].result,
            GraveMarkInputResult::Buffered { readiness_ticks: 1 }
        );
        let fired = combat
            .step(
                released_with_ability(0, 5, AimDirection::east()),
                position,
                &world,
            )
            .expect("buffered fire");
        assert_eq!(fired.shots.len(), 1);
        assert_eq!(fired.shots[0].press_sequence, 5);
        assert_eq!(combat.pending_grave_mark_sequence(), None);
        assert_eq!(combat.grave_mark_cooldown_remaining_ticks(), 150);
    }

    #[test]
    fn grave_mark_moves_point_four_then_clamps_exact_twenty_eighth_step() {
        let mut combat = combat_state();
        let world = empty_world();
        let origin = SimulationVector::new(4.0, 12.0);
        combat
            .step(ability_press(1, AimDirection::east()), origin, &world)
            .expect("fire");
        for _ in 0..27 {
            let step = combat
                .step(
                    released_with_ability(0, 1, AimDirection::east()),
                    origin,
                    &world,
                )
                .expect("full travel");
            assert!(step.expirations.is_empty());
        }
        let projectile = &combat.projectiles()[0];
        assert_eq!(projectile.source(), FriendlyProjectileSource::GraveMark);
        assert!((projectile.distance_travelled_tiles() - 10.8).abs() < 1.0e-5);
        let terminal = combat
            .step(
                released_with_ability(0, 1, AimDirection::east()),
                origin,
                &world,
            )
            .expect("clamped terminal");
        assert!(combat.projectiles().is_empty());
        assert_eq!(terminal.expirations.len(), 1);
        assert_eq!(
            terminal.expirations[0].source,
            FriendlyProjectileSource::GraveMark
        );
        assert!((terminal.expirations[0].final_position.x - 15.0).abs() < 1.0e-5);
        assert!((terminal.expirations[0].distance_travelled_tiles - 11.0).abs() < f32::EPSILON);
    }

    #[test]
    fn grave_mark_and_later_id_primary_emit_exact_same_tick_intents() {
        let target_id = EntityId::new(100).expect("target ID");
        let target =
            EnemyHurtbox::new(target_id, SimulationVector::new(4.6, 12.0), 0.15).expect("target");
        let world = world_with(vec![], vec![target]);
        let mut combat = combat_state();
        let origin = SimulationVector::new(4.0, 12.0);
        combat
            .step(
                CombatAction {
                    aim: AimDirection::east(),
                    movement: MovementAction::default(),
                    primary_held: true,
                    primary_press_sequence: 1,
                    ability_1_press_sequence: 1,
                    ability_2_press_sequence: 0,
                },
                origin,
                &world,
            )
            .expect("dual fire");
        assert_eq!(
            combat.projectiles()[0].source(),
            FriendlyProjectileSource::GraveMark
        );
        assert_eq!(
            combat.projectiles()[1].source(),
            FriendlyProjectileSource::Primary
        );
        let contact = combat
            .step(
                released_with_ability(1, 1, AimDirection::east()),
                origin,
                &world,
            )
            .expect("contacts");
        assert_eq!(contact.collisions.len(), 2);
        assert_eq!(contact.raw_damage_intents.len(), 2);
        assert_eq!(
            contact.raw_damage_intents[0],
            RawDamageIntent {
                tick: Tick(2),
                projectile_id: EntityId::new(1).expect("ID"),
                source: RawDamageIntentSource::GraveMark,
                target: target_id,
                base_raw_damage: 20,
                multiplier_basis_points: 18_000,
                resolved_raw_damage: 36,
                contact_ordinal: 0,
            }
        );
        assert_eq!(contact.raw_damage_intents[1].resolved_raw_damage, 23);
        assert_eq!(
            contact.raw_damage_intents[1].multiplier_basis_points,
            11_500
        );
        assert_eq!(
            contact.mark_transitions[0].kind,
            GraveMarkTransitionKind::Applied
        );
        let mark = combat.active_grave_mark().expect("active mark");
        assert_eq!(mark.target(), target_id);
        assert_eq!(mark.remaining_ticks(), 120);

        for _ in 0..119 {
            combat
                .step(
                    released_with_ability(1, 1, AimDirection::east()),
                    origin,
                    &world,
                )
                .expect("mark duration");
        }
        assert_eq!(
            combat
                .active_grave_mark()
                .expect("last active tick")
                .remaining_ticks(),
            1
        );
        let expired = combat
            .step(
                released_with_ability(1, 1, AimDirection::east()),
                origin,
                &world,
            )
            .expect("mark expiry");
        assert_eq!(
            expired.mark_transitions[0].kind,
            GraveMarkTransitionKind::Expired
        );
        assert_eq!(combat.active_grave_mark(), None);
    }

    #[test]
    fn fixed_grave_mark_trace_is_bit_identical_and_pins_lifetime() {
        let run = || {
            let target_id = EntityId::new(100).expect("target ID");
            let target = EnemyHurtbox::new(target_id, SimulationVector::new(4.6, 12.0), 0.15)
                .expect("target");
            let world = world_with(vec![], vec![target]);
            let mut combat = combat_state();
            let mut events = Vec::new();
            let mut states = Vec::new();
            for index in 0..122 {
                let action = if index == 0 {
                    CombatAction {
                        aim: AimDirection::east(),
                        movement: MovementAction::default(),
                        primary_held: true,
                        primary_press_sequence: 1,
                        ability_1_press_sequence: 1,
                        ability_2_press_sequence: 0,
                    }
                } else {
                    released_with_ability(1, 1, AimDirection::east())
                };
                let step = combat
                    .step(action, SimulationVector::new(4.0, 12.0), &world)
                    .expect("trace step");
                events.extend(step.collisions.iter().map(|collision| {
                    let intent = step
                        .raw_damage_intents
                        .iter()
                        .find(|intent| intent.projectile_id == collision.projectile_id)
                        .expect("enemy collision intent");
                    (
                        collision.tick.0,
                        collision.projectile_id.get(),
                        collision.source,
                        collision.final_position.x.to_bits(),
                        collision.distance_travelled_tiles.to_bits(),
                        intent.multiplier_basis_points,
                        intent.resolved_raw_damage,
                    )
                }));
                if matches!(step.tick.0, 1 | 2 | 121 | 122) {
                    states.push((
                        step.tick.0,
                        combat.grave_mark_cooldown_remaining_ticks(),
                        combat.active_grave_mark().map(ActiveGraveMark::target),
                        combat
                            .active_grave_mark()
                            .map_or(0, ActiveGraveMark::remaining_ticks),
                    ));
                }
            }
            (events, states)
        };
        let first = run();
        assert_eq!(first, run());
        assert_eq!(
            first,
            (
                vec![
                    (
                        2,
                        1,
                        FriendlyProjectileSource::GraveMark,
                        1_082_822_492,
                        1_051_260_354,
                        18_000,
                        36,
                    ),
                    (
                        2,
                        2,
                        FriendlyProjectileSource::Primary,
                        1_082_864_435,
                        1_051_931_441,
                        11_500,
                        23,
                    ),
                ],
                vec![
                    (1, 150, None, 0),
                    (2, 149, EntityId::new(100), 120),
                    (121, 30, EntityId::new(100), 1),
                    (122, 29, None, 0),
                ],
            )
        );
    }

    #[test]
    fn grave_mark_refreshes_replaces_and_ignores_solid_contacts() {
        let first_id = EntityId::new(100).expect("first ID");
        let second_id = EntityId::new(101).expect("second ID");
        let world = world_with(
            vec![TileRectangle::new(10_000, 5_000, 2_000, 3_000)],
            vec![
                EnemyHurtbox::new(first_id, SimulationVector::new(4.6, 12.0), 0.15).expect("first"),
                EnemyHurtbox::new(second_id, SimulationVector::new(8.6, 12.0), 0.15)
                    .expect("second"),
            ],
        );
        let mut combat = combat_state();
        let fire_and_hit = |combat: &mut PlayerCombatState,
                            sequence: u32,
                            origin: SimulationVector|
         -> CombatStep {
            combat.grave_mark_cooldown_remaining_ticks = 0;
            combat.global_cooldown_remaining_ticks = 0;
            combat
                .step(
                    ability_press(sequence, AimDirection::east()),
                    origin,
                    &world,
                )
                .expect("fire");
            combat
                .step(
                    released_with_ability(0, sequence, AimDirection::east()),
                    origin,
                    &world,
                )
                .expect("hit")
        };
        let applied = fire_and_hit(&mut combat, 1, SimulationVector::new(4.0, 12.0));
        assert_eq!(
            applied.mark_transitions[0].kind,
            GraveMarkTransitionKind::Applied
        );
        let refreshed = fire_and_hit(&mut combat, 2, SimulationVector::new(4.0, 12.0));
        assert_eq!(
            refreshed.mark_transitions[0].kind,
            GraveMarkTransitionKind::Refreshed
        );
        assert_eq!(
            refreshed.mark_transitions[0].previous_target,
            Some(first_id)
        );
        let replaced = fire_and_hit(&mut combat, 3, SimulationVector::new(8.0, 12.0));
        assert_eq!(
            replaced.mark_transitions[0].kind,
            GraveMarkTransitionKind::Replaced
        );
        assert_eq!(replaced.mark_transitions[0].previous_target, Some(first_id));
        assert_eq!(
            combat.active_grave_mark().expect("mark").target(),
            second_id
        );

        combat.grave_mark_cooldown_remaining_ticks = 0;
        combat.global_cooldown_remaining_ticks = 0;
        let before = combat.active_grave_mark();
        combat
            .step(
                ability_press(4, AimDirection::east()),
                SimulationVector::new(9.5, 6.5),
                &world,
            )
            .expect("wall fire");
        let blocked = combat
            .step(
                released_with_ability(0, 4, AimDirection::east()),
                SimulationVector::new(9.5, 6.5),
                &world,
            )
            .expect("wall hit");
        assert_eq!(
            blocked.collisions[0].target,
            CollisionTarget::Solid(crate::SolidColliderId::Pillar(0))
        );
        assert!(blocked.raw_damage_intents.is_empty());
        assert!(blocked.mark_transitions.is_empty());
        assert_eq!(
            combat.active_grave_mark().map(ActiveGraveMark::target),
            before.map(ActiveGraveMark::target)
        );
    }

    #[test]
    fn stale_ability_sequence_is_transactional() {
        let mut combat = combat_state();
        let world = empty_world();
        let position = SimulationVector::new(4.0, 12.0);
        combat
            .step(ability_press(1, AimDirection::east()), position, &world)
            .expect("first press");
        let before = combat.clone();
        assert_eq!(
            combat.step(ability_press(0, AimDirection::east()), position, &world),
            Err(CombatError::StaleAbilityOnePressSequence {
                received: 0,
                last: 1
            })
        );
        assert_eq!(combat, before);
    }

    #[test]
    fn held_fire_is_immediate_then_exactly_fourteen_ticks_apart() {
        let mut combat = combat_state();
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
        let mut combat = combat_state();
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
        let mut combat = combat_state();
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
        let mut combat = combat_state();
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
        let mut combat = combat_state();
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
        let mut combat = combat_state();
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
        let mut combat = combat_state();
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
    fn fixed_collision_trace_is_bit_identical_and_pins_terminal_snapshot() {
        let run = || {
            let target_id = EntityId::new(100).expect("target ID");
            let target = EnemyHurtbox::new(target_id, SimulationVector::new(4.6, 12.0), 0.15)
                .expect("target");
            let world = world_with(
                vec![TileRectangle::new(10_000, 5_000, 2_000, 3_000)],
                vec![target],
            );
            let mut combat = combat_state();
            let mut snapshot = Vec::new();
            for index in 0..16 {
                let (action, position) = match index {
                    0 => (
                        held(1, AimDirection::east()),
                        SimulationVector::new(4.0, 12.0),
                    ),
                    14 => (
                        held(2, AimDirection::east()),
                        SimulationVector::new(9.5, 6.5),
                    ),
                    _ if index < 14 => (
                        released(1, AimDirection::east()),
                        SimulationVector::new(4.0, 12.0),
                    ),
                    _ => (
                        released(2, AimDirection::east()),
                        SimulationVector::new(9.5, 6.5),
                    ),
                };
                let step = combat.step(action, position, &world).expect("trace step");
                snapshot.extend(step.collisions.into_iter().map(|collision| {
                    (
                        collision.tick.0,
                        collision.projectile_id.get(),
                        collision.target,
                        collision.final_position.x.to_bits(),
                        collision.final_position.y.to_bits(),
                        collision.distance_travelled_tiles.to_bits(),
                    )
                }));
            }
            snapshot
        };
        let first = run();
        assert_eq!(first, run());
        assert_eq!(
            first,
            vec![
                (
                    2,
                    1,
                    CollisionTarget::Enemy(EntityId::new(100).expect("target ID")),
                    1_082_864_435,
                    1_094_713_344,
                    1_051_931_441,
                ),
                (
                    16,
                    2,
                    CollisionTarget::Solid(crate::SolidColliderId::Pillar(0)),
                    1_092_511_334,
                    1_087_373_312,
                    1_053_609_152,
                ),
            ]
        );
    }

    #[test]
    fn same_tick_collision_events_are_emitted_in_projectile_id_order() {
        let first_target = EnemyHurtbox::new(
            EntityId::new(100).expect("first target ID"),
            SimulationVector::new(10.24, 12.0),
            0.15,
        )
        .expect("first target");
        let second_target = EnemyHurtbox::new(
            EntityId::new(101).expect("second target ID"),
            SimulationVector::new(20.6, 12.0),
            0.15,
        )
        .expect("second target");
        let world = world_with(vec![], vec![second_target, first_target]);
        let mut combat = combat_state();
        for index in 0..15 {
            let action = if index == 0 {
                held(1, AimDirection::east())
            } else if index == 14 {
                held(2, AimDirection::east())
            } else {
                released(1, AimDirection::east())
            };
            let position = if index == 14 {
                SimulationVector::new(20.0, 12.0)
            } else {
                SimulationVector::new(4.0, 12.0)
            };
            combat.step(action, position, &world).expect("setup step");
        }
        let terminal = combat
            .step(
                released(2, AimDirection::east()),
                SimulationVector::new(20.0, 12.0),
                &world,
            )
            .expect("shared terminal tick");
        assert_eq!(terminal.tick, Tick(16));
        assert_eq!(terminal.collisions.len(), 2);
        assert_eq!(terminal.collisions[0].projectile_id.get(), 1);
        assert_eq!(terminal.collisions[1].projectile_id.get(), 2);
        assert_eq!(
            terminal.collisions[0].target,
            CollisionTarget::Enemy(first_target.id())
        );
        assert_eq!(
            terminal.collisions[1].target,
            CollisionTarget::Enemy(second_target.id())
        );
    }

    #[test]
    fn projectile_locks_release_aim_and_origin_while_player_moves() {
        let mut combat = combat_state();
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
        let mut combat = PlayerCombatState::with_projectile_allocator(
            pine_crossbow(),
            grave_mark(),
            slipstep(),
            stillness(),
            allocator,
        )
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
    fn slipstep_travels_exactly_two_tiles_over_five_ticks_and_exposes_reduction() {
        let arena = movement_arena(vec![]);
        let world = ProjectileCollisionWorld::new(&arena, vec![]).expect("world");
        let mut movement = PlayerMovementState::at_arena_spawn(&arena).expect("movement");
        let mut combat = combat_state();
        let first = combat
            .step_with_movement(
                &mut movement,
                ability_two_action(1, MovementAction::new(1, 0), false, 0),
                &arena,
                &world,
            )
            .expect("begin");
        assert!((movement.position().x - 4.4).abs() < 1.0e-6);
        assert_eq!(first.direct_damage_reduction_basis_points, 2_500);
        assert_eq!(combat.slipstep_cooldown_remaining_ticks(), 240);
        assert_eq!(combat.exhaustion_remaining_ticks(), 45);
        assert_eq!(combat.empowered_primary_remaining_ticks(), 45);
        assert_eq!(combat.slipstep_remaining_travel_ticks(), 4);

        for expected_x in [4.8, 5.2, 5.6, 6.0] {
            combat
                .step_with_movement(
                    &mut movement,
                    ability_two_action(1, MovementAction::default(), false, 0),
                    &arena,
                    &world,
                )
                .expect("travel");
            assert!((movement.position().x - expected_x).abs() < 2.0e-6);
        }
        assert_eq!(combat.slipstep_remaining_travel_ticks(), 0);
        assert_eq!(movement.velocity(), SimulationVector::default());
        assert_eq!(movement.position().x.to_bits(), 6.0_f32.to_bits());
    }

    #[test]
    fn neutral_slipstep_moves_backward_from_aim_and_solid_contact_shortens_cast() {
        let pillar = TileRectangle::new(10_000, 5_000, 2_000, 3_000);
        let arena = movement_arena(vec![pillar]);
        let world = ProjectileCollisionWorld::new(&arena, vec![]).expect("world");
        let mut movement =
            PlayerMovementState::new(SimulationVector::new(9.5, 6.5), &arena).expect("movement");
        let mut combat = combat_state();
        let collision = combat
            .step_with_movement(
                &mut movement,
                CombatAction {
                    aim: AimDirection::new(SimulationVector::new(-1.0, 0.0)).expect("west"),
                    movement: MovementAction::default(),
                    primary_held: false,
                    primary_press_sequence: 0,
                    ability_1_press_sequence: 0,
                    ability_2_press_sequence: 1,
                },
                &arena,
                &world,
            )
            .expect("solid stop");
        assert!((movement.position().x - 9.7).abs() < 1.0e-6);
        assert_eq!(combat.slipstep_remaining_travel_ticks(), 0);
        assert!(collision.slipstep_transitions.iter().any(|transition| {
            transition.kind == SlipstepTransitionKind::Collided
                && transition.solid == Some(SolidColliderId::Pillar(0))
        }));

        let mut movement = PlayerMovementState::at_arena_spawn(&arena).expect("movement");
        let mut combat = combat_state();
        combat
            .step_with_movement(
                &mut movement,
                ability_two_action(1, MovementAction::default(), false, 0),
                &arena,
                &world,
            )
            .expect("neutral backward");
        assert!((movement.position().x - 3.6).abs() < 1.0e-6);
    }

    #[test]
    fn same_tick_primary_consumes_empowerment_and_pierces_two_targets_once_each() {
        let arena = movement_arena(vec![]);
        let targets = [
            EnemyHurtbox::new(
                EntityId::new(100).expect("ID"),
                SimulationVector::new(5.2, 12.0),
                0.15,
            )
            .expect("target"),
            EnemyHurtbox::new(
                EntityId::new(101).expect("ID"),
                SimulationVector::new(5.8, 12.0),
                0.15,
            )
            .expect("target"),
        ];
        let world = ProjectileCollisionWorld::new(&arena, targets.to_vec()).expect("world");
        let mut movement = PlayerMovementState::at_arena_spawn(&arena).expect("movement");
        let mut combat = combat_state();
        let cast = combat
            .step_with_movement(
                &mut movement,
                ability_two_action(1, MovementAction::new(1, 0), true, 1),
                &arena,
                &world,
            )
            .expect("cast and fire");
        let shot = &cast.shots[0].projectile;
        assert!(shot.empowered_by_slipstep());
        assert!((shot.origin().x - 4.4).abs() < 1.0e-6);
        assert!((shot.speed_tiles_per_second() - 15.6).abs() < 1.0e-5);
        assert_eq!(shot.pierce_remaining(), 1);
        assert_eq!(combat.empowered_primary_remaining_ticks(), 0);

        let mut contacts = Vec::new();
        for _ in 0..4 {
            let step = combat
                .step_with_movement(
                    &mut movement,
                    ability_two_action(1, MovementAction::default(), false, 1),
                    &arena,
                    &world,
                )
                .expect("advance");
            contacts.extend(step.collisions);
        }
        assert_eq!(contacts.len(), 2);
        assert_eq!(
            contacts[0].target,
            CollisionTarget::Enemy(EntityId::new(100).expect("ID"))
        );
        assert!(contacts[0].projectile_continues);
        assert_eq!(contacts[0].contact_ordinal, 0);
        assert_eq!(
            contacts[1].target,
            CollisionTarget::Enemy(EntityId::new(101).expect("ID"))
        );
        assert!(!contacts[1].projectile_continues);
        assert_eq!(contacts[1].contact_ordinal, 1);
        assert!(combat.projectiles().is_empty());
    }

    #[test]
    fn empowered_projectile_stops_on_solid_after_first_enemy() {
        let arena = movement_arena(vec![TileRectangle::new(6_000, 11_000, 1_000, 2_000)]);
        let target = EnemyHurtbox::new(
            EntityId::new(100).expect("ID"),
            SimulationVector::new(5.2, 12.0),
            0.15,
        )
        .expect("target");
        let world = ProjectileCollisionWorld::new(&arena, vec![target]).expect("world");
        let mut movement = PlayerMovementState::at_arena_spawn(&arena).expect("movement");
        let mut combat = combat_state();
        combat
            .step_with_movement(
                &mut movement,
                ability_two_action(1, MovementAction::new(1, 0), true, 1),
                &arena,
                &world,
            )
            .expect("cast");
        let mut contacts = Vec::new();
        for _ in 0..4 {
            let step = combat
                .step_with_movement(
                    &mut movement,
                    ability_two_action(1, MovementAction::default(), false, 1),
                    &arena,
                    &world,
                )
                .expect("advance");
            contacts.extend(step.collisions);
        }
        assert_eq!(contacts.len(), 2);
        assert_eq!(
            contacts[0].target,
            CollisionTarget::Enemy(EntityId::new(100).expect("ID"))
        );
        assert!(contacts[0].projectile_continues);
        assert_eq!(
            contacts[1].target,
            CollisionTarget::Solid(SolidColliderId::Pillar(0))
        );
        assert_eq!(contacts[1].contact_ordinal, 1);
        assert!(!contacts[1].projectile_continues);
    }

    #[test]
    fn empowerment_expires_before_same_tick_fire_and_exhaustion_rejects_recast() {
        let arena = movement_arena(vec![]);
        let world = ProjectileCollisionWorld::new(&arena, vec![]).expect("world");
        let mut movement = PlayerMovementState::at_arena_spawn(&arena).expect("movement");
        let mut combat = combat_state();
        combat
            .step_with_movement(
                &mut movement,
                ability_two_action(1, MovementAction::new(1, 0), false, 0),
                &arena,
                &world,
            )
            .expect("cast");
        let blocked = combat
            .step_with_movement(
                &mut movement,
                ability_two_action(2, MovementAction::new(1, 0), false, 0),
                &arena,
                &world,
            )
            .expect("blocked recast");
        assert!(matches!(
            blocked.slipstep_inputs[0].result,
            SlipstepInputResult::BlockedByExhaustion {
                remaining_ticks: 44
            }
        ));
        for _ in 0..43 {
            combat
                .step_with_movement(
                    &mut movement,
                    ability_two_action(2, MovementAction::default(), false, 0),
                    &arena,
                    &world,
                )
                .expect("window");
        }
        assert_eq!(combat.empowered_primary_remaining_ticks(), 1);
        let expiry_fire = combat
            .step_with_movement(
                &mut movement,
                ability_two_action(2, MovementAction::default(), true, 1),
                &arena,
                &world,
            )
            .expect("expiry fire");
        assert_eq!(combat.empowered_primary_remaining_ticks(), 0);
        assert!(!expiry_fire.shots[0].projectile.empowered_by_slipstep());
        assert!(
            expiry_fire
                .slipstep_transitions
                .iter()
                .any(|transition| transition.kind == SlipstepTransitionKind::EmpowermentExpired)
        );
    }

    #[test]
    fn slipstep_buffers_only_at_three_ticks_and_sequence_failure_rolls_back_avatar() {
        let arena = movement_arena(vec![]);
        let world = ProjectileCollisionWorld::new(&arena, vec![]).expect("world");
        let mut movement = PlayerMovementState::at_arena_spawn(&arena).expect("movement");
        let mut combat = combat_state();
        combat
            .step_with_movement(
                &mut movement,
                ability_two_action(1, MovementAction::new(1, 0), false, 0),
                &arena,
                &world,
            )
            .expect("first cast");
        while combat.slipstep_cooldown_remaining_ticks() > 5 {
            combat
                .step_with_movement(
                    &mut movement,
                    ability_two_action(1, MovementAction::default(), false, 0),
                    &arena,
                    &world,
                )
                .expect("cooldown");
        }
        let too_early = combat
            .step_with_movement(
                &mut movement,
                ability_two_action(2, MovementAction::new(1, 0), false, 0),
                &arena,
                &world,
            )
            .expect("too early");
        assert_eq!(combat.slipstep_cooldown_remaining_ticks(), 4);
        assert_eq!(
            too_early.slipstep_inputs[0].result,
            SlipstepInputResult::ConsumedTooEarly { readiness_ticks: 4 }
        );
        let buffered = combat
            .step_with_movement(
                &mut movement,
                ability_two_action(3, MovementAction::new(1, 0), false, 0),
                &arena,
                &world,
            )
            .expect("buffer");
        assert_eq!(combat.pending_slipstep_sequence(), Some(3));
        assert_eq!(
            buffered.slipstep_inputs[0].result,
            SlipstepInputResult::Buffered { readiness_ticks: 3 }
        );

        let before_combat = combat.clone();
        let before_movement = movement;
        let stale = ability_two_action(2, MovementAction::new(-1, 0), false, 0);
        assert_eq!(
            combat.step_with_movement(&mut movement, stale, &arena, &world),
            Err(CombatError::StaleAbilityTwoPressSequence {
                received: 2,
                last: 3,
            })
        );
        assert_eq!(combat, before_combat);
        assert_eq!(movement, before_movement);
    }

    #[test]
    fn stillness_gains_on_tick_eighteen_and_modifies_same_tick_primary() {
        let arena = movement_arena(vec![]);
        let world = ProjectileCollisionWorld::new(&arena, vec![]).expect("world");
        let mut movement = PlayerMovementState::at_arena_spawn(&arena).expect("movement");
        let mut combat = combat_state();
        for _ in 0..17 {
            combat
                .step_with_movement(
                    &mut movement,
                    released(0, AimDirection::east()),
                    &arena,
                    &world,
                )
                .expect("focus buildup");
        }
        assert!(!combat.focused());
        assert_eq!(combat.stillness_ticks(), 17);
        let gained = combat
            .step_with_movement(&mut movement, held(1, AimDirection::east()), &arena, &world)
            .expect("gain and fire");
        assert!(combat.focused());
        assert_eq!(
            gained.focused_transitions[0].kind,
            FocusedTransitionKind::Gained
        );
        let projectile = &gained.shots[0].projectile;
        assert!(projectile.focused_by_stillness());
        assert_eq!(projectile.raw_damage(), 22);
        assert!((projectile.speed_tiles_per_second() - 13.2).abs() < 1.0e-5);
    }

    #[test]
    fn stillness_threshold_is_strict_below_at_exact_twenty_percent() {
        let threshold = scale_f32_basis_points(5.1, 2_000);
        assert!(movement_is_still(
            SimulationVector::new(threshold - 0.001, 0.0),
            5.1,
            2_000
        ));
        assert!(!movement_is_still(
            SimulationVector::new(threshold, 0.0),
            5.1,
            2_000
        ));
        assert!(!movement_is_still(
            SimulationVector::new(threshold + 0.001, 0.0),
            5.1,
            2_000
        ));
    }

    #[test]
    fn movement_slipstep_and_damage_break_focused_before_later_shots() {
        let arena = movement_arena(vec![]);
        let world = ProjectileCollisionWorld::new(&arena, vec![]).expect("world");
        let focus = |combat: &mut PlayerCombatState, movement: &mut PlayerMovementState| {
            for _ in 0..18 {
                combat
                    .step_with_movement(movement, released(0, AimDirection::east()), &arena, &world)
                    .expect("focus");
            }
            assert!(combat.focused());
        };

        let mut movement = PlayerMovementState::at_arena_spawn(&arena).expect("movement");
        let mut combat = combat_state();
        focus(&mut combat, &mut movement);
        let movement_break = combat
            .step_with_movement(
                &mut movement,
                CombatAction {
                    movement: MovementAction::new(1, 0),
                    ..held(1, AimDirection::east())
                },
                &arena,
                &world,
            )
            .expect("movement break");
        assert!(!movement_break.shots[0].projectile.focused_by_stillness());
        assert_eq!(
            movement_break.focused_transitions[0].kind,
            FocusedTransitionKind::BrokenByMovement
        );

        let mut movement = PlayerMovementState::at_arena_spawn(&arena).expect("movement");
        let mut combat = combat_state();
        focus(&mut combat, &mut movement);
        let slip_break = combat
            .step_with_movement(
                &mut movement,
                ability_two_action(1, MovementAction::new(1, 0), true, 1),
                &arena,
                &world,
            )
            .expect("Slipstep break");
        assert!(slip_break.shots[0].projectile.empowered_by_slipstep());
        assert!(!slip_break.shots[0].projectile.focused_by_stillness());
        assert!(
            slip_break
                .focused_transitions
                .iter()
                .any(|transition| { transition.kind == FocusedTransitionKind::BrokenBySlipstep })
        );

        let mut movement = PlayerMovementState::at_arena_spawn(&arena).expect("movement");
        let mut combat = combat_state();
        focus(&mut combat, &mut movement);
        let damage_break = combat.break_focused_from_damage().expect("damage break");
        assert_eq!(damage_break.kind, FocusedTransitionKind::BrokenByDamage);
        assert!(!combat.focused());
    }

    #[test]
    fn nailkeeper_grave_mark_contact_spawns_arms_and_triggers_exact_trap() {
        let mut combat = PlayerCombatState::with_oath(
            pine_crossbow(),
            grave_mark(),
            slipstep(),
            stillness(),
            GraveArbalistOath::Nailkeeper,
        )
        .unwrap();
        let inside = world_with(
            vec![],
            vec![
                EnemyHurtbox::new(
                    EntityId::new(50).unwrap(),
                    SimulationVector::new(10.5, 10.0),
                    0.25,
                )
                .unwrap(),
            ],
        );
        let away = world_with(
            vec![],
            vec![
                EnemyHurtbox::new(
                    EntityId::new(50).unwrap(),
                    SimulationVector::new(20.0, 20.0),
                    0.25,
                )
                .unwrap(),
            ],
        );
        let origin = SimulationVector::new(10.0, 10.0);
        let idle = released_with_ability(0, 1, AimDirection::east());
        combat
            .step(ability_press(1, AimDirection::east()), origin, &inside)
            .unwrap();
        let contact = combat.step(idle, origin, &inside).unwrap();
        assert_eq!(contact.mark_transitions.len(), 1);
        assert_eq!(contact.nail_traps.spawned.len(), 1);
        let trap_id = contact.nail_traps.spawned[0];
        assert_eq!(combat.nail_traps().traps()[0].id(), trap_id);

        let mut armed = None;
        while combat.tick().0 < 14 {
            let step = combat.step(idle, origin, &inside).unwrap();
            if !step.nail_traps.armed.is_empty() {
                armed = Some(step);
            }
        }
        assert_eq!(armed.unwrap().nail_traps.armed, vec![trap_id]);
        assert!(
            combat
                .step(idle, origin, &away)
                .unwrap()
                .nail_traps
                .triggers
                .is_empty()
        );
        let triggered = combat.step(idle, origin, &inside).unwrap();
        assert_eq!(triggered.nail_traps.triggers.len(), 1);
        assert_eq!(
            triggered.nail_traps.triggers[0].target_id,
            EntityId::new(50).unwrap()
        );
        assert_eq!(triggered.nail_traps.triggers[0].raw_damage, 18);
        assert_eq!(triggered.nail_traps.triggers[0].frostbind_ticks, 45);
        assert_eq!(triggered.raw_damage_intents.len(), 1);
        assert_eq!(
            triggered.raw_damage_intents[0].source,
            RawDamageIntentSource::NailTrap
        );
        assert_eq!(triggered.raw_damage_intents[0].resolved_raw_damage, 18);
        assert!(combat.nail_traps().traps().is_empty());
    }

    #[test]
    fn focused_primary_composes_with_same_tick_earlier_grave_mark() {
        let arena = movement_arena(vec![]);
        let target_id = EntityId::new(100).expect("ID");
        let target =
            EnemyHurtbox::new(target_id, SimulationVector::new(4.6, 12.0), 0.15).expect("target");
        let world = ProjectileCollisionWorld::new(&arena, vec![target]).expect("world");
        let mut movement = PlayerMovementState::at_arena_spawn(&arena).expect("movement");
        let mut combat = combat_state();
        for _ in 0..18 {
            combat
                .step_with_movement(
                    &mut movement,
                    released(0, AimDirection::east()),
                    &arena,
                    &world,
                )
                .expect("focus");
        }
        combat
            .step_with_movement(
                &mut movement,
                CombatAction {
                    aim: AimDirection::east(),
                    movement: MovementAction::default(),
                    primary_held: true,
                    primary_press_sequence: 1,
                    ability_1_press_sequence: 1,
                    ability_2_press_sequence: 0,
                },
                &arena,
                &world,
            )
            .expect("dual fire");
        let contact = combat
            .step_with_movement(
                &mut movement,
                released_with_ability(1, 1, AimDirection::east()),
                &arena,
                &world,
            )
            .expect("contacts");
        assert_eq!(contact.raw_damage_intents.len(), 2);
        assert_eq!(contact.raw_damage_intents[1].base_raw_damage, 22);
        assert_eq!(contact.raw_damage_intents[1].resolved_raw_damage, 25);
    }

    #[test]
    fn fixed_fire_trace_has_exact_stable_snapshot() {
        let mut combat = combat_state();
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
