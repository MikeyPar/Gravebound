//! Renderer-independent target acquisition and telegraph-lock primitives for Core enemies.
//!
//! These types implement the authority-neutral part of `CONT-ENEMY-001`. Kit scheduling, leash
//! reference semantics, and introduction timing remain separate so unresolved design choices cannot
//! leak into otherwise stable selection and snapshot behavior.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::{
    AimVector, AttackCastId, CoreEnemyDefinition, CoreEnemyStateStage,
    CorePatternWarningDefinition, EntityId, Tick,
};

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

#[derive(Debug, Clone, PartialEq, Eq)]
enum CoreEnemyRuntimeState {
    SpawnTelegraph { ready_at: Tick },
    Acquire,
    MoveOrPosition,
    Telegraph { attack_lock: CoreAttackLock },
    Attack { attack_lock: CoreAttackLock },
    Recover { ends_at: Tick },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreEnemyRuntimeEvent {
    SpawnTelegraphStarted {
        ends_at: Tick,
    },
    IntroductionStarted {
        ends_at: Tick,
    },
    StateChanged {
        state: CoreEnemyStateStage,
    },
    TargetChanged {
        previous: Option<EntityId>,
        current: Option<EntityId>,
    },
    TelegraphStarted {
        attack_lock: CoreAttackLock,
    },
    AttackReady {
        attack_lock: CoreAttackLock,
    },
    RecoverStarted {
        ends_at: Tick,
    },
    ResetToSpawn {
        position: CoreWorldPosition,
        restored_health: u32,
        cleared_hostile_output: bool,
        reward_granted: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CoreEnemyRuntimeError {
    #[error(transparent)]
    TargetSelection(#[from] CoreTargetSelectionError),
    #[error(transparent)]
    AttackLock(#[from] CoreAttackLockError),
    #[error("Core enemy runtime tick arithmetic overflowed")]
    TickOverflow,
    #[error("Core enemy runtime pattern index is invalid")]
    InvalidPatternIndex,
    #[error("Core enemy runtime transition is invalid for the current state")]
    InvalidStateTransition,
    #[error("Core enemy runtime has no legal target to lock")]
    MissingTarget,
    #[error("Core enemy runtime does not support a standalone lock for this warning")]
    ParentWarningRequiresKitScheduler,
    #[error("Core enemy runtime health mutation is invalid")]
    InvalidHealthMutation,
    #[error("Core enemy runtime cast identity overflowed")]
    CastIdOverflow,
}

/// Shared deterministic lifecycle for one Core-authored enemy.
///
/// Kit schedulers own movement and attack choice. This layer owns the common target, state,
/// telegraph snapshot, and reset invariants required by `CONT-ENEMY-001`.
#[derive(Debug, Clone)]
pub struct CoreEnemySimulation {
    definition: CoreEnemyDefinition,
    tick: Tick,
    state: CoreEnemyRuntimeState,
    home_position: CoreWorldPosition,
    position: CoreWorldPosition,
    current_health: u32,
    target: Option<CoreSelectedTarget>,
    next_target_scan_at: Tick,
    no_target_since: Option<Tick>,
    next_cast_ordinal: u64,
    completed_pattern_ids: BTreeSet<String>,
    emitted_initial_events: bool,
}

impl CoreEnemySimulation {
    #[must_use]
    pub fn new(definition: CoreEnemyDefinition, home_position: CoreWorldPosition) -> Self {
        let ready_at = Tick(u64::from(
            definition
                .spawn_warning_ticks()
                .max(definition.introduction_ticks()),
        ));
        let maximum_health = definition.parameters().maximum_health;
        Self {
            definition,
            tick: Tick(0),
            state: CoreEnemyRuntimeState::SpawnTelegraph { ready_at },
            home_position,
            position: home_position,
            current_health: maximum_health,
            target: None,
            next_target_scan_at: ready_at,
            no_target_since: None,
            next_cast_ordinal: 1,
            completed_pattern_ids: BTreeSet::new(),
            emitted_initial_events: false,
        }
    }

    #[must_use]
    pub const fn definition(&self) -> &CoreEnemyDefinition {
        &self.definition
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    #[must_use]
    pub const fn stage(&self) -> CoreEnemyStateStage {
        match self.state {
            CoreEnemyRuntimeState::SpawnTelegraph { .. } => CoreEnemyStateStage::SpawnTelegraph,
            CoreEnemyRuntimeState::Acquire => CoreEnemyStateStage::Acquire,
            CoreEnemyRuntimeState::MoveOrPosition => CoreEnemyStateStage::MoveOrPosition,
            CoreEnemyRuntimeState::Telegraph { .. } => CoreEnemyStateStage::Telegraph,
            CoreEnemyRuntimeState::Attack { .. } => CoreEnemyStateStage::Attack,
            CoreEnemyRuntimeState::Recover { .. } => CoreEnemyStateStage::Recover,
        }
    }

    #[must_use]
    pub const fn home_position(&self) -> CoreWorldPosition {
        self.home_position
    }

    #[must_use]
    pub const fn position(&self) -> CoreWorldPosition {
        self.position
    }

    pub const fn set_position(&mut self, position: CoreWorldPosition) {
        self.position = position;
    }

    #[must_use]
    pub const fn current_health(&self) -> u32 {
        self.current_health
    }

    #[must_use]
    pub fn current_target(&self) -> Option<CoreSelectedTarget> {
        self.target
    }

    #[must_use]
    pub fn active_attack_lock(&self) -> Option<&CoreAttackLock> {
        match &self.state {
            CoreEnemyRuntimeState::Telegraph { attack_lock }
            | CoreEnemyRuntimeState::Attack { attack_lock } => Some(attack_lock),
            CoreEnemyRuntimeState::SpawnTelegraph { .. }
            | CoreEnemyRuntimeState::Acquire
            | CoreEnemyRuntimeState::MoveOrPosition
            | CoreEnemyRuntimeState::Recover { .. } => None,
        }
    }

    pub fn apply_damage(&mut self, damage: u32) -> Result<(), CoreEnemyRuntimeError> {
        if damage == 0 || damage >= self.current_health {
            return Err(CoreEnemyRuntimeError::InvalidHealthMutation);
        }
        self.current_health -= damage;
        Ok(())
    }

    /// Advances exactly one authoritative tick using clone-stage-commit semantics.
    pub fn advance(
        &mut self,
        candidates: &[CoreTargetCandidate],
    ) -> Result<Vec<CoreEnemyRuntimeEvent>, CoreEnemyRuntimeError> {
        let mut staged = self.clone();
        let events = staged.advance_inner(candidates)?;
        *self = staged;
        Ok(events)
    }

    pub fn begin_telegraph(
        &mut self,
        pattern_index: usize,
    ) -> Result<CoreEnemyRuntimeEvent, CoreEnemyRuntimeError> {
        let mut staged = self.clone();
        let event = staged.begin_telegraph_inner(pattern_index)?;
        *self = staged;
        Ok(event)
    }

    pub fn finish_attack(
        &mut self,
        recover_ticks: u32,
    ) -> Result<CoreEnemyRuntimeEvent, CoreEnemyRuntimeError> {
        let mut staged = self.clone();
        let attack_lock = match &staged.state {
            CoreEnemyRuntimeState::Attack { attack_lock } => attack_lock.clone(),
            _ => return Err(CoreEnemyRuntimeError::InvalidStateTransition),
        };
        staged
            .completed_pattern_ids
            .insert(attack_lock.pattern_id().to_owned());
        let ends_at = add_ticks(staged.tick, recover_ticks)?;
        staged.state = CoreEnemyRuntimeState::Recover { ends_at };
        *self = staged;
        Ok(CoreEnemyRuntimeEvent::RecoverStarted { ends_at })
    }

    fn advance_inner(
        &mut self,
        candidates: &[CoreTargetCandidate],
    ) -> Result<Vec<CoreEnemyRuntimeEvent>, CoreEnemyRuntimeError> {
        let now = self.tick;
        let mut events = Vec::new();
        if !self.emitted_initial_events {
            let CoreEnemyRuntimeState::SpawnTelegraph { ready_at } = self.state else {
                return Err(CoreEnemyRuntimeError::InvalidStateTransition);
            };
            events.push(CoreEnemyRuntimeEvent::SpawnTelegraphStarted {
                ends_at: add_ticks(Tick(0), self.definition.spawn_warning_ticks())?,
            });
            if self.definition.introduction_ticks() > 0 {
                events.push(CoreEnemyRuntimeEvent::IntroductionStarted {
                    ends_at: add_ticks(Tick(0), self.definition.introduction_ticks())?,
                });
            }
            debug_assert_eq!(
                ready_at.0,
                u64::from(
                    self.definition
                        .spawn_warning_ticks()
                        .max(self.definition.introduction_ticks())
                )
            );
            self.emitted_initial_events = true;
        }

        if let CoreEnemyRuntimeState::SpawnTelegraph { ready_at } = self.state {
            if now >= ready_at {
                self.state = CoreEnemyRuntimeState::Acquire;
                events.push(CoreEnemyRuntimeEvent::StateChanged {
                    state: CoreEnemyStateStage::Acquire,
                });
            } else {
                self.tick = next_tick(now)?;
                return Ok(events);
            }
        }

        let previous_target = self.target.map(|target| target.entity_id);
        self.refresh_target(candidates, now)?;
        let current_target = self.target.map(|target| target.entity_id);
        if previous_target != current_target {
            events.push(CoreEnemyRuntimeEvent::TargetChanged {
                previous: previous_target,
                current: current_target,
            });
        }

        if self.target.is_some() {
            self.no_target_since = None;
        } else {
            let since = *self.no_target_since.get_or_insert(now);
            if now.0.saturating_sub(since.0) >= u64::from(self.definition.no_target_reset_ticks()) {
                self.reset_to_spawn(now, &mut events)?;
                self.tick = next_tick(now)?;
                return Ok(events);
            }
        }

        match &self.state {
            CoreEnemyRuntimeState::Acquire if self.target.is_some() => {
                self.state = CoreEnemyRuntimeState::MoveOrPosition;
                events.push(CoreEnemyRuntimeEvent::StateChanged {
                    state: CoreEnemyStateStage::MoveOrPosition,
                });
            }
            CoreEnemyRuntimeState::MoveOrPosition if self.target.is_none() => {
                self.state = CoreEnemyRuntimeState::Acquire;
                events.push(CoreEnemyRuntimeEvent::StateChanged {
                    state: CoreEnemyStateStage::Acquire,
                });
            }
            CoreEnemyRuntimeState::Telegraph { attack_lock }
                if now >= attack_lock.resolves_at() =>
            {
                let attack_lock = attack_lock.clone();
                self.state = CoreEnemyRuntimeState::Attack {
                    attack_lock: attack_lock.clone(),
                };
                events.push(CoreEnemyRuntimeEvent::StateChanged {
                    state: CoreEnemyStateStage::Attack,
                });
                events.push(CoreEnemyRuntimeEvent::AttackReady { attack_lock });
            }
            CoreEnemyRuntimeState::Recover { ends_at } if now >= *ends_at => {
                self.state = CoreEnemyRuntimeState::Acquire;
                events.push(CoreEnemyRuntimeEvent::StateChanged {
                    state: CoreEnemyStateStage::Acquire,
                });
                if self.target.is_some() {
                    self.state = CoreEnemyRuntimeState::MoveOrPosition;
                    events.push(CoreEnemyRuntimeEvent::StateChanged {
                        state: CoreEnemyStateStage::MoveOrPosition,
                    });
                }
            }
            CoreEnemyRuntimeState::SpawnTelegraph { .. }
            | CoreEnemyRuntimeState::Acquire
            | CoreEnemyRuntimeState::MoveOrPosition
            | CoreEnemyRuntimeState::Telegraph { .. }
            | CoreEnemyRuntimeState::Attack { .. }
            | CoreEnemyRuntimeState::Recover { .. } => {}
        }
        self.tick = next_tick(now)?;
        Ok(events)
    }

    fn refresh_target(
        &mut self,
        candidates: &[CoreTargetCandidate],
        now: Tick,
    ) -> Result<(), CoreEnemyRuntimeError> {
        let nearest = select_core_target(
            self.position,
            self.definition.parameters().aggro_radius_milli_tiles,
            candidates,
        )?;
        let retained = self.target.and_then(|target| {
            candidates
                .iter()
                .find(|candidate| candidate.entity_id == target.entity_id)
                .filter(|candidate| {
                    candidate.living
                        && candidate.damageable
                        && self.position.squared_distance_to(candidate.position)
                            <= squared_radius(self.definition.parameters().leash_radius_milli_tiles)
                })
                .map(|candidate| CoreSelectedTarget {
                    entity_id: candidate.entity_id,
                    position: candidate.position,
                    squared_distance_milli_tiles: self
                        .position
                        .squared_distance_to(candidate.position),
                })
        });
        let scan_due = now >= self.next_target_scan_at;
        self.target = match (retained, scan_due) {
            (Some(current), false) => Some(current),
            (Some(current), true) => nearest.or(Some(current)),
            (None, _) => nearest,
        };
        if scan_due {
            self.next_target_scan_at = add_ticks(now, self.definition.target_reacquire_ticks())?;
        }
        Ok(())
    }

    fn begin_telegraph_inner(
        &mut self,
        pattern_index: usize,
    ) -> Result<CoreEnemyRuntimeEvent, CoreEnemyRuntimeError> {
        if !matches!(self.state, CoreEnemyRuntimeState::MoveOrPosition) {
            return Err(CoreEnemyRuntimeError::InvalidStateTransition);
        }
        let target = self.target.ok_or(CoreEnemyRuntimeError::MissingTarget)?;
        let pattern = self
            .definition
            .parameters()
            .patterns
            .get(pattern_index)
            .ok_or(CoreEnemyRuntimeError::InvalidPatternIndex)?;
        let telegraph_ticks = match pattern.warning() {
            CorePatternWarningDefinition::Standalone {
                first_ticks,
                repeated_ticks,
            } => {
                if self
                    .completed_pattern_ids
                    .contains(&pattern.parameters().id)
                {
                    *repeated_ticks
                } else {
                    *first_ticks
                }
            }
            CorePatternWarningDefinition::RecoveryPreview {
                directional_gap_preview_ticks,
                ..
            } => *directional_gap_preview_ticks,
            CorePatternWarningDefinition::ParentOnly => {
                return Err(CoreEnemyRuntimeError::ParentWarningRequiresKitScheduler);
            }
        };
        let cast_id = AttackCastId::from_ordinal(self.next_cast_ordinal)
            .ok_or(CoreEnemyRuntimeError::CastIdOverflow)?;
        self.next_cast_ordinal = self
            .next_cast_ordinal
            .checked_add(1)
            .ok_or(CoreEnemyRuntimeError::CastIdOverflow)?;
        let attack_lock = CoreAttackLock::new(
            cast_id,
            pattern.parameters().id.clone(),
            pattern_index,
            self.position,
            target,
            self.tick,
            telegraph_ticks,
        )?;
        self.state = CoreEnemyRuntimeState::Telegraph {
            attack_lock: attack_lock.clone(),
        };
        Ok(CoreEnemyRuntimeEvent::TelegraphStarted { attack_lock })
    }

    fn reset_to_spawn(
        &mut self,
        now: Tick,
        events: &mut Vec<CoreEnemyRuntimeEvent>,
    ) -> Result<(), CoreEnemyRuntimeError> {
        self.position = self.home_position;
        self.current_health = self.definition.parameters().maximum_health;
        self.target = None;
        self.no_target_since = None;
        self.next_cast_ordinal = 1;
        self.completed_pattern_ids.clear();
        let gate_ticks = self
            .definition
            .spawn_warning_ticks()
            .max(self.definition.introduction_ticks());
        let ready_at = add_ticks(now, gate_ticks)?;
        self.next_target_scan_at = ready_at;
        self.state = CoreEnemyRuntimeState::SpawnTelegraph { ready_at };
        events.push(CoreEnemyRuntimeEvent::ResetToSpawn {
            position: self.home_position,
            restored_health: self.current_health,
            cleared_hostile_output: true,
            reward_granted: false,
        });
        events.push(CoreEnemyRuntimeEvent::StateChanged {
            state: CoreEnemyStateStage::SpawnTelegraph,
        });
        events.push(CoreEnemyRuntimeEvent::SpawnTelegraphStarted {
            ends_at: add_ticks(now, self.definition.spawn_warning_ticks())?,
        });
        if self.definition.introduction_ticks() > 0 {
            events.push(CoreEnemyRuntimeEvent::IntroductionStarted {
                ends_at: add_ticks(now, self.definition.introduction_ticks())?,
            });
        }
        Ok(())
    }
}

const fn squared_radius(radius: u32) -> u128 {
    (radius as u128).saturating_mul(radius as u128)
}

fn add_ticks(tick: Tick, amount: u32) -> Result<Tick, CoreEnemyRuntimeError> {
    tick.0
        .checked_add(u64::from(amount))
        .map(Tick)
        .ok_or(CoreEnemyRuntimeError::TickOverflow)
}

fn next_tick(tick: Tick) -> Result<Tick, CoreEnemyRuntimeError> {
    tick.checked_next()
        .ok_or(CoreEnemyRuntimeError::TickOverflow)
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
    use crate::{
        CORE_ENEMY_STATE_SEQUENCE, CoreAttackGroupRule, CoreEnemyDefinitionParameters,
        CoreEnemyLocomotionParameters, CoreEnemyRole, CorePatternDefinition,
        CorePatternDefinitionParameters, CorePatternGeometryParameters,
        CorePatternWarningParameters, CoreTargetSelection, CoreTelegraphLock, Counterplay,
        DamageBand, DamageType, EchoMemoryFamily, HostileDisposition,
    };

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

    fn mire_definition(introduction_ticks: u32) -> CoreEnemyDefinition {
        let introduction_milliseconds = introduction_ticks.saturating_mul(100) / 3;
        let pattern = CorePatternDefinition::new(CorePatternDefinitionParameters {
            id: "pattern.enemy.mire_leech.charge".to_owned(),
            owner_id: "enemy.mire_leech".to_owned(),
            telegraph_id: "pattern.enemy.mire_leech.charge.telegraph".to_owned(),
            audio_cue_id: "pattern.enemy.mire_leech.charge.warning".to_owned(),
            major_audio_cue_id: None,
            damage_type: DamageType::Physical,
            damage_band: DamageBand::Pressure,
            raw_damage: 12,
            threat_cost: 2,
            warning: CorePatternWarningParameters::Standalone {
                first_milliseconds: 400,
                repeated_milliseconds: 300,
            },
            cycle_milliseconds: 2_500,
            quiet_milliseconds: 1_500,
            geometry: CorePatternGeometryParameters::Charge {
                distance_milli_tiles: 2_000,
                duration_milliseconds: 500,
            },
            counterplay: Counterplay::LeaveTelegraph,
            memory_family: EchoMemoryFamily::ChargeOrContact,
            disposition: HostileDisposition::OneContactHitPerCast,
            attack_group_rule: CoreAttackGroupRule::OneContactHitPerCast,
            acceleration_milli_tiles_per_second_squared: 0,
            pierces_players: false,
            status_count: 0,
            cancel_on_phase_change: true,
            persisted_maximum_active_instances: 1,
        })
        .expect("Mire pattern");
        CoreEnemyDefinition::new(CoreEnemyDefinitionParameters {
            content_id: "enemy.mire_leech".to_owned(),
            role: CoreEnemyRole::Fodder,
            state_sequence: CORE_ENEMY_STATE_SEQUENCE,
            target_selection: CoreTargetSelection::NearestLivingDamageableInAggroTieLowestEntityId,
            telegraph_lock: CoreTelegraphLock::AimAndPositionAtTelegraphStart,
            maximum_health: 70,
            armor: 0,
            collision_radius_milli_tiles: 350,
            hurtbox_radius_milli_tiles: 300,
            aggro_radius_milli_tiles: 12_000,
            leash_radius_milli_tiles: 16_000,
            target_reacquire_milliseconds: 250,
            no_target_reset_milliseconds: 5_000,
            spawn_warning_milliseconds: 900,
            spawn_invulnerability_milliseconds: 1_000,
            introduction_milliseconds,
            contact_damage: 0,
            drop_reward_on_reset: false,
            locomotion: CoreEnemyLocomotionParameters::RushRetreat {
                approach_speed_milli_tiles_per_second: 3_000,
                trigger_distance_milli_tiles: 2_500,
                charge_distance_milli_tiles: 2_000,
                charge_duration_milliseconds: 500,
                retreat_speed_milli_tiles_per_second: 3_500,
                retreat_duration_milliseconds: 1_500,
            },
            patterns: vec![pattern],
            reward_profile_id: "reward.normal_outer".to_owned(),
            xp_profile_id: "xp.normal_t1".to_owned(),
        })
        .expect("Mire definition")
    }

    fn advance_to_acquire(
        simulation: &mut CoreEnemySimulation,
        candidates: &[CoreTargetCandidate],
    ) {
        while simulation.tick() < Tick(27) {
            simulation.advance(candidates).expect("spawn advance");
        }
        simulation.advance(candidates).expect("acquire advance");
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

    #[test]
    fn spawn_warning_and_miniboss_introduction_overlap_at_the_longer_gate() {
        let mut simulation =
            CoreEnemySimulation::new(mire_definition(90), CoreWorldPosition::new(4_000, 5_000));
        let initial = simulation.advance(&[]).expect("initial advance");
        assert_eq!(
            initial,
            vec![
                CoreEnemyRuntimeEvent::SpawnTelegraphStarted { ends_at: Tick(27) },
                CoreEnemyRuntimeEvent::IntroductionStarted { ends_at: Tick(90) },
            ]
        );
        while simulation.tick() < Tick(90) {
            simulation.advance(&[]).expect("introduction advance");
            assert_eq!(simulation.stage(), CoreEnemyStateStage::SpawnTelegraph);
        }
        let boundary = simulation.advance(&[]).expect("introduction boundary");
        assert_eq!(simulation.stage(), CoreEnemyStateStage::Acquire);
        assert!(boundary.contains(&CoreEnemyRuntimeEvent::StateChanged {
            state: CoreEnemyStateStage::Acquire,
        }));
    }

    #[test]
    fn target_rescan_and_actor_to_target_leash_boundaries_are_exact() {
        let mut simulation =
            CoreEnemySimulation::new(mire_definition(0), CoreWorldPosition::new(0, 0));
        let first = candidate(8, 10_000, 0);
        advance_to_acquire(&mut simulation, &[first]);
        assert_eq!(
            simulation.current_target().expect("target").entity_id,
            id(8)
        );
        assert_eq!(simulation.stage(), CoreEnemyStateStage::MoveOrPosition);

        let nearer = candidate(2, 1_000, 0);
        while simulation.tick() < Tick(35) {
            simulation
                .advance(&[first, nearer])
                .expect("pre-rescan advance");
            assert_eq!(
                simulation.current_target().expect("target").entity_id,
                id(8)
            );
        }
        simulation
            .advance(&[first, nearer])
            .expect("rescan boundary");
        assert_eq!(
            simulation.current_target().expect("target").entity_id,
            id(2)
        );

        let leash_edge = candidate(2, 16_000, 0);
        simulation.advance(&[leash_edge]).expect("leash edge");
        assert_eq!(
            simulation.current_target().expect("target").entity_id,
            id(2)
        );
        let beyond = candidate(2, 16_001, 0);
        simulation.advance(&[beyond]).expect("beyond leash");
        assert_eq!(simulation.current_target(), None);
        assert_eq!(simulation.stage(), CoreEnemyStateStage::Acquire);
    }

    #[test]
    fn telegraph_release_uses_frozen_snapshot_and_first_then_repeat_warning() {
        let mut simulation =
            CoreEnemySimulation::new(mire_definition(0), CoreWorldPosition::new(0, 0));
        let initial_target = candidate(4, 2_000, 0);
        advance_to_acquire(&mut simulation, &[initial_target]);
        let first = simulation.begin_telegraph(0).expect("first telegraph");
        let CoreEnemyRuntimeEvent::TelegraphStarted {
            attack_lock: first_lock,
        } = first
        else {
            panic!("telegraph event");
        };
        assert_eq!(first_lock.resolves_at(), Tick(40));

        let moved_target = candidate(4, 0, 2_000);
        while simulation.tick() <= first_lock.resolves_at() {
            simulation
                .advance(&[moved_target])
                .expect("warning advance");
        }
        let active = simulation.active_attack_lock().expect("active attack");
        assert_eq!(simulation.stage(), CoreEnemyStateStage::Attack);
        assert_eq!(active.target_position(), initial_target.position);
        assert_eq!(active.aim_delta(), AimVector { x: 2_000, y: 0 });

        simulation.finish_attack(0).expect("finish attack");
        simulation
            .advance(&[moved_target])
            .expect("recover boundary");
        let repeated = simulation.begin_telegraph(0).expect("repeat telegraph");
        let CoreEnemyRuntimeEvent::TelegraphStarted {
            attack_lock: repeated_lock,
        } = repeated
        else {
            panic!("repeat event");
        };
        assert_eq!(
            repeated_lock.resolves_at().0 - repeated_lock.telegraph_started_at().0,
            9
        );
    }

    #[test]
    fn no_target_reset_is_atomic_at_150_ticks_and_boundary_return_wins() {
        let home = CoreWorldPosition::new(3_000, 4_000);
        let mut reset = CoreEnemySimulation::new(mire_definition(0), home);
        advance_to_acquire(&mut reset, &[]);
        reset.apply_damage(20).expect("nonlethal damage");
        reset.set_position(CoreWorldPosition::new(9_000, 9_000));
        while reset.tick() < Tick(177) {
            let events = reset.advance(&[]).expect("no-target advance");
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, CoreEnemyRuntimeEvent::ResetToSpawn { .. }))
            );
        }
        let boundary = reset.advance(&[]).expect("reset boundary");
        assert_eq!(reset.stage(), CoreEnemyStateStage::SpawnTelegraph);
        assert_eq!(reset.position(), home);
        assert_eq!(reset.current_health(), 70);
        assert!(boundary.contains(&CoreEnemyRuntimeEvent::ResetToSpawn {
            position: home,
            restored_health: 70,
            cleared_hostile_output: true,
            reward_granted: false,
        }));

        let mut rescued = CoreEnemySimulation::new(mire_definition(0), home);
        advance_to_acquire(&mut rescued, &[]);
        while rescued.tick() < Tick(177) {
            rescued.advance(&[]).expect("no-target advance");
        }
        let return_target = candidate(6, 4_000, 4_000);
        let returned = rescued.advance(&[return_target]).expect("boundary return");
        assert!(
            !returned
                .iter()
                .any(|event| matches!(event, CoreEnemyRuntimeEvent::ResetToSpawn { .. }))
        );
        assert_eq!(rescued.stage(), CoreEnemyStateStage::MoveOrPosition);
        assert_eq!(
            rescued.current_target().expect("returned target").entity_id,
            id(6)
        );
    }

    #[test]
    fn malformed_candidate_input_is_transactional() {
        let mut simulation =
            CoreEnemySimulation::new(mire_definition(0), CoreWorldPosition::new(0, 0));
        advance_to_acquire(&mut simulation, &[candidate(1, 1_000, 0)]);
        let before = (
            simulation.tick(),
            simulation.stage(),
            simulation.current_target(),
            simulation.current_health(),
        );
        let duplicate = [candidate(2, 1_000, 0), candidate(2, 2_000, 0)];
        assert!(matches!(
            simulation.advance(&duplicate),
            Err(CoreEnemyRuntimeError::TargetSelection(
                CoreTargetSelectionError::DuplicateCandidate { .. }
            ))
        ));
        assert_eq!(
            before,
            (
                simulation.tick(),
                simulation.stage(),
                simulation.current_target(),
                simulation.current_health(),
            )
        );
    }
}
