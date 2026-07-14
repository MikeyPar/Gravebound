//! Dedicated stationary Choir Abbot attack authority.
//!
//! `SPEC-CONFLICT-014`, `016`, and `020` fix warning layering, equal-tick priority, rotor phase,
//! and recovery-ring target/gap ownership.

use std::cmp::Reverse;

use thiserror::Error;

use crate::{
    AttackCastId, CoreEnemyDefinition, CoreEnemyKitError, CoreEnemyKitEvent, CoreEnemyKitKind,
    CoreEnemyKitScheduler, CoreEnemyLocomotionDefinition, CoreSelectedTarget, CoreTargetCandidate,
    CoreTargetSelectionError, CoreWorldPosition, Tick, select_core_target,
};

const ROTOR_PATTERN_INDEX: usize = 0;
const RING_PATTERN_INDEX: usize = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreAbbotRotorLock {
    cast_id: AttackCastId,
    pattern_id: String,
    origin: CoreWorldPosition,
    telegraph_started_at: Tick,
    resolves_at: Tick,
}

impl CoreAbbotRotorLock {
    #[must_use]
    pub const fn cast_id(&self) -> AttackCastId {
        self.cast_id
    }
    #[must_use]
    pub fn pattern_id(&self) -> &str {
        &self.pattern_id
    }
    #[must_use]
    pub const fn origin(&self) -> CoreWorldPosition {
        self.origin
    }
    #[must_use]
    pub const fn telegraph_started_at(&self) -> Tick {
        self.telegraph_started_at
    }
    #[must_use]
    pub const fn resolves_at(&self) -> Tick {
        self.resolves_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreAbbotRingLock {
    cast_id: AttackCastId,
    pattern_id: String,
    origin: CoreWorldPosition,
    target: CoreSelectedTarget,
    preview_started_at: Tick,
    resolves_at: Tick,
}

impl CoreAbbotRingLock {
    #[must_use]
    pub const fn cast_id(&self) -> AttackCastId {
        self.cast_id
    }
    #[must_use]
    pub fn pattern_id(&self) -> &str {
        &self.pattern_id
    }
    #[must_use]
    pub const fn origin(&self) -> CoreWorldPosition {
        self.origin
    }
    #[must_use]
    pub const fn target(&self) -> CoreSelectedTarget {
        self.target
    }
    #[must_use]
    pub const fn preview_started_at(&self) -> Tick {
        self.preview_started_at
    }
    #[must_use]
    pub const fn resolves_at(&self) -> Tick {
        self.resolves_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreAbbotEvent {
    RotorTelegraphStarted {
        lock: CoreAbbotRotorLock,
        first_use: bool,
    },
    RotorStarted {
        tick: Tick,
        cycle_index: u32,
        lock: CoreAbbotRotorLock,
    },
    RotorVolleyReleased {
        tick: Tick,
        cycle_index: u32,
        volley_index: u8,
        phase_milli_degrees: i32,
        lock: CoreAbbotRotorLock,
    },
    RecoveryStarted {
        tick: Tick,
        recovery_ticks: u32,
    },
    RecoveryOriginWarningStarted {
        tick: Tick,
        warning_ticks: u32,
        directional_preview_ticks: u32,
    },
    DirectionalGapPreviewStarted {
        lock: CoreAbbotRingLock,
    },
    RecoveryRingReleased {
        tick: Tick,
        lock: CoreAbbotRingLock,
        emitted_indices: [u8; 12],
        omitted_indices: [u8; 4],
    },
    TargetlessReset {
        tick: Tick,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreAbbotStep {
    pub tick: Tick,
    pub selected_target: Option<CoreSelectedTarget>,
    pub kit_events: Vec<CoreEnemyKitEvent>,
    pub events: Vec<CoreAbbotEvent>,
}

#[derive(Debug, Clone)]
pub struct CoreAbbotSimulation {
    definition: CoreEnemyDefinition,
    scheduler: CoreEnemyKitScheduler,
    entity_id: crate::EntityId,
    position: CoreWorldPosition,
    tick: Tick,
    next_cast_ordinal: u64,
    pending_rotor: Option<CoreAbbotRotorLock>,
    active_rotor: Option<(u32, CoreAbbotRotorLock)>,
    pending_ring: Option<CoreAbbotRingLock>,
    targetless_ticks: u32,
}

impl CoreAbbotSimulation {
    pub fn new(
        definition: CoreEnemyDefinition,
        entity_id: crate::EntityId,
        position: CoreWorldPosition,
    ) -> Result<Self, CoreAbbotError> {
        let scheduler = CoreEnemyKitScheduler::new(definition.clone())?;
        if scheduler.kind() != CoreEnemyKitKind::ChoirAbbot
            || definition.locomotion() != &CoreEnemyLocomotionDefinition::Stationary
            || definition.parameters().collision_radius_milli_tiles != 550
            || definition.parameters().hurtbox_radius_milli_tiles != 480
        {
            return Err(CoreAbbotError::DefinitionDrift);
        }
        Ok(Self {
            definition,
            scheduler,
            entity_id,
            position,
            tick: Tick(0),
            next_cast_ordinal: 1,
            pending_rotor: None,
            active_rotor: None,
            pending_ring: None,
            targetless_ticks: 0,
        })
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }
    #[must_use]
    pub const fn entity_id(&self) -> crate::EntityId {
        self.entity_id
    }
    #[must_use]
    pub const fn position(&self) -> CoreWorldPosition {
        self.position
    }
    #[must_use]
    pub const fn definition(&self) -> &CoreEnemyDefinition {
        &self.definition
    }

    pub fn advance(
        &mut self,
        candidates: &[CoreTargetCandidate],
        attacks_enabled: bool,
    ) -> Result<CoreAbbotStep, CoreAbbotError> {
        let mut staged = self.clone();
        let step = staged.advance_inner(candidates, attacks_enabled)?;
        *self = staged;
        Ok(step)
    }

    pub fn reset(&mut self) -> Result<(), CoreAbbotError> {
        self.scheduler.reset()?;
        self.pending_rotor = None;
        self.active_rotor = None;
        self.pending_ring = None;
        self.targetless_ticks = 0;
        Ok(())
    }

    fn advance_inner(
        &mut self,
        candidates: &[CoreTargetCandidate],
        attacks_enabled: bool,
    ) -> Result<CoreAbbotStep, CoreAbbotError> {
        if self.scheduler.tick() != self.tick {
            return Err(CoreAbbotError::SchedulerTickMismatch);
        }
        let selected_target = select_core_target(
            self.position,
            self.definition.parameters().aggro_radius_milli_tiles,
            candidates,
        )?;
        if selected_target.is_some() {
            self.targetless_ticks = 0;
        } else {
            self.targetless_ticks = self
                .targetless_ticks
                .checked_add(1)
                .ok_or(CoreAbbotError::TickOverflow)?;
        }
        let kit_events = self
            .scheduler
            .advance(attacks_enabled && selected_target.is_some())?;
        let mut events = Vec::with_capacity(kit_events.len());
        for event in &kit_events {
            self.consume_event(selected_target, event, &mut events)?;
        }
        if self.targetless_ticks >= self.definition.no_target_reset_ticks() {
            self.reset()?;
            events.push(CoreAbbotEvent::TargetlessReset { tick: self.tick });
        }
        let step = CoreAbbotStep {
            tick: self.tick,
            selected_target,
            kit_events,
            events,
        };
        self.tick = self
            .tick
            .checked_next()
            .ok_or(CoreAbbotError::TickOverflow)?;
        Ok(step)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "the complete Abbot scheduler grammar remains exhaustive"
    )]
    fn consume_event(
        &mut self,
        target: Option<CoreSelectedTarget>,
        event: &CoreEnemyKitEvent,
        output: &mut Vec<CoreAbbotEvent>,
    ) -> Result<(), CoreAbbotError> {
        match event {
            CoreEnemyKitEvent::TelegraphDue {
                tick,
                pattern_index,
                warning_ticks,
                first_use,
            } => {
                self.require_tick(*tick)?;
                if *pattern_index != ROTOR_PATTERN_INDEX || self.pending_rotor.is_some() {
                    return Err(CoreAbbotError::DefinitionDrift);
                }
                let lock = CoreAbbotRotorLock {
                    cast_id: self.next_cast_id()?,
                    pattern_id: self.definition.parameters().patterns[0]
                        .parameters()
                        .id
                        .clone(),
                    origin: self.position,
                    telegraph_started_at: *tick,
                    resolves_at: add_ticks(*tick, *warning_ticks)?,
                };
                self.pending_rotor = Some(lock.clone());
                output.push(CoreAbbotEvent::RotorTelegraphStarted {
                    lock,
                    first_use: *first_use,
                });
            }
            CoreEnemyKitEvent::RotorStarted {
                tick,
                pattern_index,
                cycle_index,
                ..
            } => {
                self.require_tick(*tick)?;
                if *pattern_index != ROTOR_PATTERN_INDEX {
                    return Err(CoreAbbotError::DefinitionDrift);
                }
                let lock = self
                    .pending_rotor
                    .take()
                    .ok_or(CoreAbbotError::ReleaseWithoutTelegraph)?;
                if lock.resolves_at != *tick {
                    return Err(CoreAbbotError::ReleaseBoundaryMismatch);
                }
                self.active_rotor = Some((*cycle_index, lock.clone()));
                output.push(CoreAbbotEvent::RotorStarted {
                    tick: *tick,
                    cycle_index: *cycle_index,
                    lock,
                });
            }
            CoreEnemyKitEvent::RotorVolleyDue {
                tick,
                pattern_index,
                cycle_index,
                volley_index,
                arm_count,
            } => {
                self.require_tick(*tick)?;
                let (active_cycle, lock) = self
                    .active_rotor
                    .as_ref()
                    .ok_or(CoreAbbotError::MissingActiveRotor)?;
                if *pattern_index != ROTOR_PATTERN_INDEX
                    || *arm_count != 2
                    || active_cycle != cycle_index
                    || *volley_index >= 10
                {
                    return Err(CoreAbbotError::DefinitionDrift);
                }
                output.push(CoreAbbotEvent::RotorVolleyReleased {
                    tick: *tick,
                    cycle_index: *cycle_index,
                    volley_index: *volley_index,
                    phase_milli_degrees: i32::from(*volley_index) * 12_250,
                    lock: lock.clone(),
                });
            }
            CoreEnemyKitEvent::RotorRecoveryStarted {
                tick,
                pattern_index,
                recovery_ticks,
            } => {
                self.require_tick(*tick)?;
                if *pattern_index != ROTOR_PATTERN_INDEX || *recovery_ticks != 75 {
                    return Err(CoreAbbotError::DefinitionDrift);
                }
                self.active_rotor
                    .take()
                    .ok_or(CoreAbbotError::MissingActiveRotor)?;
                output.push(CoreAbbotEvent::RecoveryStarted {
                    tick: *tick,
                    recovery_ticks: *recovery_ticks,
                });
            }
            CoreEnemyKitEvent::RecoveryWarningDue {
                tick,
                pattern_index,
                warning_ticks,
                directional_preview_ticks,
            } => {
                self.require_tick(*tick)?;
                if *pattern_index != RING_PATTERN_INDEX
                    || *warning_ticks != 75
                    || *directional_preview_ticks != 20
                {
                    return Err(CoreAbbotError::DefinitionDrift);
                }
                output.push(CoreAbbotEvent::RecoveryOriginWarningStarted {
                    tick: *tick,
                    warning_ticks: *warning_ticks,
                    directional_preview_ticks: *directional_preview_ticks,
                });
            }
            CoreEnemyKitEvent::DirectionalGapPreviewDue {
                tick,
                pattern_index,
                warning_ticks,
            } => {
                self.require_tick(*tick)?;
                if *pattern_index != RING_PATTERN_INDEX
                    || *warning_ticks != 20
                    || self.pending_ring.is_some()
                {
                    return Err(CoreAbbotError::DefinitionDrift);
                }
                let lock = CoreAbbotRingLock {
                    cast_id: self.next_cast_id()?,
                    pattern_id: self.definition.parameters().patterns[1]
                        .parameters()
                        .id
                        .clone(),
                    origin: self.position,
                    target: target.ok_or(CoreAbbotError::MissingTarget)?,
                    preview_started_at: *tick,
                    resolves_at: add_ticks(*tick, *warning_ticks)?,
                };
                self.pending_ring = Some(lock.clone());
                output.push(CoreAbbotEvent::DirectionalGapPreviewStarted { lock });
            }
            CoreEnemyKitEvent::AbbotRecoveryRingDue {
                tick,
                pattern_index,
            } => {
                self.require_tick(*tick)?;
                if *pattern_index != RING_PATTERN_INDEX {
                    return Err(CoreAbbotError::DefinitionDrift);
                }
                let lock = self
                    .pending_ring
                    .take()
                    .ok_or(CoreAbbotError::ReleaseWithoutTelegraph)?;
                if lock.resolves_at != *tick {
                    return Err(CoreAbbotError::ReleaseBoundaryMismatch);
                }
                let (emitted_indices, omitted_indices) = recovery_ring_indices(&lock)?;
                output.push(CoreAbbotEvent::RecoveryRingReleased {
                    tick: *tick,
                    lock,
                    emitted_indices,
                    omitted_indices,
                });
            }
            CoreEnemyKitEvent::MireChargeDue { .. }
            | CoreEnemyKitEvent::MireRetreatDue { .. }
            | CoreEnemyKitEvent::AcolyteFanDue { .. }
            | CoreEnemyKitEvent::KnightChargeDue { .. }
            | CoreEnemyKitEvent::KnightStopRingDue { .. }
            | CoreEnemyKitEvent::KnightShieldFanDue { .. } => {
                return Err(CoreAbbotError::DefinitionDrift);
            }
        }
        Ok(())
    }

    fn next_cast_id(&mut self) -> Result<AttackCastId, CoreAbbotError> {
        let id = AttackCastId::from_ordinal(self.next_cast_ordinal)
            .ok_or(CoreAbbotError::CastIdOverflow)?;
        self.next_cast_ordinal = self
            .next_cast_ordinal
            .checked_add(1)
            .ok_or(CoreAbbotError::CastIdOverflow)?;
        Ok(id)
    }

    fn require_tick(&self, tick: Tick) -> Result<(), CoreAbbotError> {
        if tick == self.tick {
            Ok(())
        } else {
            Err(CoreAbbotError::KitEventTickMismatch)
        }
    }
}

fn recovery_ring_indices(lock: &CoreAbbotRingLock) -> Result<([u8; 12], [u8; 4]), CoreAbbotError> {
    const MIDPOINTS: [(i64, i64); 16] = [
        (831_470, 555_570),
        (555_570, 831_470),
        (195_090, 980_785),
        (-195_090, 980_785),
        (-555_570, 831_470),
        (-831_470, 555_570),
        (-980_785, 195_090),
        (-980_785, -195_090),
        (-831_470, -555_570),
        (-555_570, -831_470),
        (-195_090, -980_785),
        (195_090, -980_785),
        (555_570, -831_470),
        (831_470, -555_570),
        (980_785, -195_090),
        (980_785, 195_090),
    ];
    let direction = (
        i64::from(lock.target.position.x_milli_tiles) - i64::from(lock.origin.x_milli_tiles),
        i64::from(lock.target.position.y_milli_tiles) - i64::from(lock.origin.y_milli_tiles),
    );
    if direction == (0, 0) {
        return Err(CoreAbbotError::CoincidentTarget);
    }
    let start = MIDPOINTS
        .iter()
        .enumerate()
        .max_by_key(|(index, basis)| {
            (
                direction.0 * basis.0 + direction.1 * basis.1,
                Reverse(*index),
            )
        })
        .map(|(index, _)| u8::try_from(index).expect("sixteen groups fit u8"))
        .ok_or(CoreAbbotError::DefinitionDrift)?;
    let omitted = [start, (start + 1) % 16, (start + 2) % 16, (start + 3) % 16];
    let emitted = (0_u8..16)
        .filter(|index| !omitted.contains(index))
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| CoreAbbotError::DefinitionDrift)?;
    Ok((emitted, omitted))
}

fn add_ticks(tick: Tick, amount: u32) -> Result<Tick, CoreAbbotError> {
    tick.0
        .checked_add(u64::from(amount))
        .map(Tick)
        .ok_or(CoreAbbotError::TickOverflow)
}

#[derive(Debug, Error)]
pub enum CoreAbbotError {
    #[error("Choir Abbot definition drifted from its exact authored contract")]
    DefinitionDrift,
    #[error("Abbot scheduler tick diverged from its owner")]
    SchedulerTickMismatch,
    #[error("Abbot scheduler event tick diverged from its owner")]
    KitEventTickMismatch,
    #[error("Abbot directional preview requires a legal target")]
    MissingTarget,
    #[error("Abbot directional target is coincident with its origin")]
    CoincidentTarget,
    #[error("Abbot released without its immutable preview")]
    ReleaseWithoutTelegraph,
    #[error("Abbot release did not match its preview boundary")]
    ReleaseBoundaryMismatch,
    #[error("Abbot rotor event has no active cast")]
    MissingActiveRotor,
    #[error("Abbot cast identity overflowed")]
    CastIdOverflow,
    #[error("Abbot tick arithmetic overflowed")]
    TickOverflow,
    #[error(transparent)]
    Kit(#[from] CoreEnemyKitError),
    #[error(transparent)]
    Target(#[from] CoreTargetSelectionError),
}
