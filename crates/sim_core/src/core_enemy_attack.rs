//! Immutable target/cast locks for Core-authored normal-enemy kit schedules.
//!
//! Locomotion decides only whether a new discrete attack may begin. Once a telegraph exists, this
//! authority pins its origin, target (when geometry is target-relative), cast identity, and exact
//! release tick. Renderer state and later target movement cannot alter a release.

use thiserror::Error;

use crate::{
    AttackCastId, CoreEnemyDefinition, CoreEnemyKitError, CoreEnemyKitEvent, CoreEnemyKitKind,
    CoreEnemyKitScheduler, CorePatternGeometryDefinition, CoreSelectedTarget, CoreTargetCandidate,
    CoreTargetSelectionError, CoreWorldPosition, Tick, select_core_target,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreNormalAttackKind {
    MireCharge,
    AcolyteFan,
    SkullRotorStart,
    SkullRotorVolley { cycle_index: u32, volley_index: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreNormalAttackLock {
    cast_id: AttackCastId,
    pattern_index: usize,
    pattern_id: String,
    origin: CoreWorldPosition,
    target: Option<CoreSelectedTarget>,
    telegraph_started_at: Tick,
    resolves_at: Tick,
}

impl CoreNormalAttackLock {
    #[must_use]
    pub const fn cast_id(&self) -> AttackCastId {
        self.cast_id
    }

    #[must_use]
    pub const fn pattern_index(&self) -> usize {
        self.pattern_index
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
    pub const fn target(&self) -> Option<CoreSelectedTarget> {
        self.target
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
pub enum CoreNormalAttackEvent {
    TelegraphStarted {
        lock: CoreNormalAttackLock,
        first_use: bool,
    },
    Released {
        tick: Tick,
        kind: CoreNormalAttackKind,
        lock: CoreNormalAttackLock,
    },
    RotorRecoveryStarted {
        tick: Tick,
        recovery_ticks: u32,
    },
    MireRetreatStarted {
        tick: Tick,
        speed_milli_tiles_per_second: u32,
        duration_ticks: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreNormalAttackStep {
    pub tick: Tick,
    pub selected_target: Option<CoreSelectedTarget>,
    pub kit_events: Vec<CoreEnemyKitEvent>,
    pub attack_events: Vec<CoreNormalAttackEvent>,
}

/// Attack overlay for Mire Leech, Bell Acolyte, and Choir Skull.
#[derive(Debug, Clone)]
pub struct CoreNormalAttackSimulation {
    definition: CoreEnemyDefinition,
    scheduler: CoreEnemyKitScheduler,
    tick: Tick,
    next_cast_ordinal: u64,
    pending_lock: Option<CoreNormalAttackLock>,
    active_rotor: Option<(u32, CoreNormalAttackLock)>,
}

impl CoreNormalAttackSimulation {
    pub fn new(definition: CoreEnemyDefinition) -> Result<Self, CoreNormalAttackError> {
        let scheduler = CoreEnemyKitScheduler::new(definition.clone())?;
        if !matches!(
            scheduler.kind(),
            CoreEnemyKitKind::MireLeech
                | CoreEnemyKitKind::BellAcolyte
                | CoreEnemyKitKind::ChoirSkull
        ) {
            return Err(CoreNormalAttackError::UnsupportedActor {
                content_id: definition.parameters().content_id.clone(),
            });
        }
        Ok(Self {
            definition,
            scheduler,
            tick: Tick(0),
            next_cast_ordinal: 1,
            pending_lock: None,
            active_rotor: None,
        })
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    #[must_use]
    pub const fn kind(&self) -> CoreEnemyKitKind {
        self.scheduler.kind()
    }

    #[must_use]
    pub const fn pending_lock(&self) -> Option<&CoreNormalAttackLock> {
        self.pending_lock.as_ref()
    }

    pub fn advance(
        &mut self,
        origin: CoreWorldPosition,
        candidates: &[CoreTargetCandidate],
        positioned_for_attack: bool,
    ) -> Result<CoreNormalAttackStep, CoreNormalAttackError> {
        let mut staged = self.clone();
        let step = staged.advance_inner(origin, candidates, positioned_for_attack)?;
        *self = staged;
        Ok(step)
    }

    /// Clears every pending/active cast and restores first-use kit state at the current tick.
    pub fn reset(&mut self) -> Result<(), CoreNormalAttackError> {
        let mut staged = self.clone();
        staged.scheduler.reset()?;
        staged.next_cast_ordinal = 1;
        staged.pending_lock = None;
        staged.active_rotor = None;
        *self = staged;
        Ok(())
    }

    fn advance_inner(
        &mut self,
        origin: CoreWorldPosition,
        candidates: &[CoreTargetCandidate],
        positioned_for_attack: bool,
    ) -> Result<CoreNormalAttackStep, CoreNormalAttackError> {
        if self.scheduler.tick() != self.tick {
            return Err(CoreNormalAttackError::SchedulerTickMismatch);
        }
        let selected_target = select_core_target(
            origin,
            self.definition.parameters().aggro_radius_milli_tiles,
            candidates,
        )?;
        let kit_events = self
            .scheduler
            .advance(positioned_for_attack && selected_target.is_some())?;
        let mut attack_events = Vec::with_capacity(kit_events.len());
        for event in &kit_events {
            self.consume_kit_event(origin, selected_target, event, &mut attack_events)?;
        }
        let step = CoreNormalAttackStep {
            tick: self.tick,
            selected_target,
            kit_events,
            attack_events,
        };
        self.tick = self
            .tick
            .checked_next()
            .ok_or(CoreNormalAttackError::TickOverflow)?;
        Ok(step)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "the complete normal-kit event grammar remains auditable in one exhaustive match"
    )]
    fn consume_kit_event(
        &mut self,
        origin: CoreWorldPosition,
        target: Option<CoreSelectedTarget>,
        event: &CoreEnemyKitEvent,
        output: &mut Vec<CoreNormalAttackEvent>,
    ) -> Result<(), CoreNormalAttackError> {
        match event {
            CoreEnemyKitEvent::TelegraphDue {
                tick,
                pattern_index,
                warning_ticks,
                first_use,
            } => {
                self.require_event_tick(*tick)?;
                if self.pending_lock.is_some() {
                    return Err(CoreNormalAttackError::OverlappingTelegraph);
                }
                let target = match self.scheduler.kind() {
                    CoreEnemyKitKind::ChoirSkull => target,
                    CoreEnemyKitKind::MireLeech | CoreEnemyKitKind::BellAcolyte => {
                        Some(target.ok_or(CoreNormalAttackError::MissingTarget)?)
                    }
                    CoreEnemyKitKind::SepulcherKnight | CoreEnemyKitKind::ChoirAbbot => {
                        return Err(CoreNormalAttackError::UnsupportedActor {
                            content_id: self.definition.parameters().content_id.clone(),
                        });
                    }
                };
                let lock = self.new_lock(*pattern_index, origin, target, *tick, *warning_ticks)?;
                self.pending_lock = Some(lock.clone());
                output.push(CoreNormalAttackEvent::TelegraphStarted {
                    lock,
                    first_use: *first_use,
                });
            }
            CoreEnemyKitEvent::MireChargeDue {
                tick,
                pattern_index,
                ..
            } => {
                let lock = self.take_resolved_lock(*tick, *pattern_index)?;
                output.push(CoreNormalAttackEvent::Released {
                    tick: *tick,
                    kind: CoreNormalAttackKind::MireCharge,
                    lock,
                });
            }
            CoreEnemyKitEvent::AcolyteFanDue {
                tick,
                pattern_index,
                ..
            } => {
                let lock = self.take_resolved_lock(*tick, *pattern_index)?;
                output.push(CoreNormalAttackEvent::Released {
                    tick: *tick,
                    kind: CoreNormalAttackKind::AcolyteFan,
                    lock,
                });
            }
            CoreEnemyKitEvent::RotorStarted {
                tick,
                pattern_index,
                cycle_index,
                ..
            } => {
                let lock = self.take_resolved_lock(*tick, *pattern_index)?;
                self.active_rotor = Some((*cycle_index, lock.clone()));
                output.push(CoreNormalAttackEvent::Released {
                    tick: *tick,
                    kind: CoreNormalAttackKind::SkullRotorStart,
                    lock,
                });
            }
            CoreEnemyKitEvent::RotorVolleyDue {
                tick,
                pattern_index,
                cycle_index,
                volley_index,
                ..
            } => {
                self.require_event_tick(*tick)?;
                let (active_cycle, lock) = self
                    .active_rotor
                    .as_ref()
                    .ok_or(CoreNormalAttackError::MissingActiveRotor)?;
                if active_cycle != cycle_index || lock.pattern_index != *pattern_index {
                    return Err(CoreNormalAttackError::RotorCycleMismatch);
                }
                output.push(CoreNormalAttackEvent::Released {
                    tick: *tick,
                    kind: CoreNormalAttackKind::SkullRotorVolley {
                        cycle_index: *cycle_index,
                        volley_index: *volley_index,
                    },
                    lock: lock.clone(),
                });
            }
            CoreEnemyKitEvent::RotorRecoveryStarted {
                tick,
                recovery_ticks,
                ..
            } => {
                self.require_event_tick(*tick)?;
                self.active_rotor
                    .take()
                    .ok_or(CoreNormalAttackError::MissingActiveRotor)?;
                output.push(CoreNormalAttackEvent::RotorRecoveryStarted {
                    tick: *tick,
                    recovery_ticks: *recovery_ticks,
                });
            }
            CoreEnemyKitEvent::MireRetreatDue {
                tick,
                speed_milli_tiles_per_second,
                duration_ticks,
            } => {
                self.require_event_tick(*tick)?;
                output.push(CoreNormalAttackEvent::MireRetreatStarted {
                    tick: *tick,
                    speed_milli_tiles_per_second: *speed_milli_tiles_per_second,
                    duration_ticks: *duration_ticks,
                });
            }
            CoreEnemyKitEvent::KnightChargeDue { .. }
            | CoreEnemyKitEvent::KnightStopRingDue { .. }
            | CoreEnemyKitEvent::KnightShieldFanDue { .. }
            | CoreEnemyKitEvent::RecoveryWarningDue { .. }
            | CoreEnemyKitEvent::DirectionalGapPreviewDue { .. }
            | CoreEnemyKitEvent::AbbotRecoveryRingDue { .. } => {
                return Err(CoreNormalAttackError::UnsupportedActor {
                    content_id: self.definition.parameters().content_id.clone(),
                });
            }
        }
        Ok(())
    }

    fn new_lock(
        &mut self,
        pattern_index: usize,
        origin: CoreWorldPosition,
        target: Option<CoreSelectedTarget>,
        telegraph_started_at: Tick,
        warning_ticks: u32,
    ) -> Result<CoreNormalAttackLock, CoreNormalAttackError> {
        let pattern = self
            .definition
            .parameters()
            .patterns
            .get(pattern_index)
            .ok_or(CoreNormalAttackError::InvalidPatternIndex)?;
        if matches!(
            pattern.geometry(),
            CorePatternGeometryDefinition::RotatingArms { .. }
        ) != (self.scheduler.kind() == CoreEnemyKitKind::ChoirSkull)
        {
            return Err(CoreNormalAttackError::DefinitionDrift);
        }
        let cast_id = AttackCastId::from_ordinal(self.next_cast_ordinal)
            .ok_or(CoreNormalAttackError::CastIdOverflow)?;
        self.next_cast_ordinal = self
            .next_cast_ordinal
            .checked_add(1)
            .ok_or(CoreNormalAttackError::CastIdOverflow)?;
        let resolves_at = telegraph_started_at
            .0
            .checked_add(u64::from(warning_ticks))
            .map(Tick)
            .ok_or(CoreNormalAttackError::TickOverflow)?;
        Ok(CoreNormalAttackLock {
            cast_id,
            pattern_index,
            pattern_id: pattern.parameters().id.clone(),
            origin,
            target,
            telegraph_started_at,
            resolves_at,
        })
    }

    fn take_resolved_lock(
        &mut self,
        tick: Tick,
        pattern_index: usize,
    ) -> Result<CoreNormalAttackLock, CoreNormalAttackError> {
        self.require_event_tick(tick)?;
        let lock = self
            .pending_lock
            .take()
            .ok_or(CoreNormalAttackError::ReleaseWithoutTelegraph)?;
        if lock.pattern_index != pattern_index || lock.resolves_at != tick {
            return Err(CoreNormalAttackError::ReleaseBoundaryMismatch);
        }
        Ok(lock)
    }

    fn require_event_tick(&self, tick: Tick) -> Result<(), CoreNormalAttackError> {
        if tick == self.tick {
            Ok(())
        } else {
            Err(CoreNormalAttackError::KitEventTickMismatch)
        }
    }
}

#[derive(Debug, Error)]
pub enum CoreNormalAttackError {
    #[error("Core normal attack overlay does not support {content_id}")]
    UnsupportedActor { content_id: String },
    #[error("Core normal attack scheduler tick diverged from its owner")]
    SchedulerTickMismatch,
    #[error("Core normal attack scheduler emitted an event at the wrong tick")]
    KitEventTickMismatch,
    #[error("Core normal attack requires a legal target for target-relative geometry")]
    MissingTarget,
    #[error("Core normal attack started a second telegraph before resolving the first")]
    OverlappingTelegraph,
    #[error("Core normal attack released without a pending telegraph")]
    ReleaseWithoutTelegraph,
    #[error("Core normal attack release did not match its exact pattern or warning boundary")]
    ReleaseBoundaryMismatch,
    #[error("Core normal rotor volley has no active rotor cast")]
    MissingActiveRotor,
    #[error("Core normal rotor cycle or pattern diverged from its active cast")]
    RotorCycleMismatch,
    #[error("Core normal attack pattern index is invalid")]
    InvalidPatternIndex,
    #[error("Core normal attack definition drifted from its scheduler grammar")]
    DefinitionDrift,
    #[error("Core normal attack cast identity overflowed")]
    CastIdOverflow,
    #[error("Core normal attack tick arithmetic overflowed")]
    TickOverflow,
    #[error(transparent)]
    Kit(#[from] CoreEnemyKitError),
    #[error(transparent)]
    Target(#[from] CoreTargetSelectionError),
}
