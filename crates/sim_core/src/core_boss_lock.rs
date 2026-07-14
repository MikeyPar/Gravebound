//! Deterministic DNG-006 staging, participant lock, introduction, and reset authority.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::{EntityId, Tick};

pub const CORE_BOSS_LOAD_TIMEOUT_TICKS: u32 = 300;
pub const CORE_BOSS_READY_COUNTDOWN_TICKS: u32 = 150;
pub const CORE_CALDUS_INTRODUCTION_TICKS: u32 = 75;
pub const CORE_BOSS_EMPTY_RESET_TICKS: u32 = 150;
pub const CORE_BOSS_MINIMUM_PARTICIPANTS: u8 = 1;
pub const CORE_BOSS_MAXIMUM_PARTICIPANTS: u8 = 8;
pub const CORE_BOSS_RUNTIME_CAPACITY: u8 = 1;
pub const CORE_CALDUS_BASE_HEALTH: u32 = 7_200;
pub const CORE_CALDUS_ADDITIONAL_PARTICIPANT_HEALTH_BASIS_POINTS: u32 = 7_200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CoreBossParticipant {
    pub entity_id: EntityId,
    pub party_slot: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreBossConnectionState {
    Disconnected,
    ConnectedLoading,
    ConnectedLoaded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreBossLifeState {
    Living,
    Dead,
    Recalled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreBossEntrantInput {
    pub participant: CoreBossParticipant,
    pub connection: CoreBossConnectionState,
    pub life: CoreBossLifeState,
    pub inside_boundary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreBossLockInput {
    pub tick: Tick,
    pub entrants: Vec<CoreBossEntrantInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreBossParticipantLock {
    pub attempt_ordinal: u32,
    pub participants: Vec<CoreBossParticipant>,
    pub maximum_health: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreBossLockPhase {
    BossWarning,
    Loading {
        started_at: Tick,
        staged: Vec<CoreBossParticipant>,
    },
    ReadyCountdown {
        started_at: Tick,
        closes_at: Tick,
        staged: Vec<CoreBossParticipant>,
    },
    Introduction {
        started_at: Tick,
        activates_at: Tick,
        lock: CoreBossParticipantLock,
    },
    Combat {
        lock: CoreBossParticipantLock,
    },
    ResetPending {
        started_at: Tick,
        resets_at: Tick,
        lock: CoreBossParticipantLock,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreBossLockEvent {
    StagingStarted {
        tick: Tick,
        participants: Vec<CoreBossParticipant>,
    },
    StagedRosterChanged {
        tick: Tick,
        participants: Vec<CoreBossParticipant>,
    },
    ReadyCountdownStarted {
        tick: Tick,
        closes_at: Tick,
    },
    ReadyCountdownAbandoned {
        tick: Tick,
    },
    LateEntryRejected {
        tick: Tick,
        participant: CoreBossParticipant,
    },
    DoorClosed {
        tick: Tick,
    },
    ParticipantLockCommitted {
        tick: Tick,
        lock: CoreBossParticipantLock,
    },
    EntranceRadiusCleared {
        tick: Tick,
    },
    IntroductionStarted {
        tick: Tick,
        activates_at: Tick,
    },
    CombatStarted {
        tick: Tick,
        lock: CoreBossParticipantLock,
    },
    EmptyResetStarted {
        tick: Tick,
        resets_at: Tick,
    },
    EmptyResetCancelled {
        tick: Tick,
    },
    EmptyResetCompleted {
        tick: Tick,
        cleared_hostiles: bool,
        cleared_unsecured_drops: bool,
        door_reopened: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreBossLockStep {
    pub tick: Tick,
    pub phase: CoreBossLockPhase,
    pub events: Vec<CoreBossLockEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreBossLockSimulation {
    tick: Tick,
    phase: CoreBossLockPhase,
    next_attempt_ordinal: u32,
    rejected_late_entries: BTreeSet<EntityId>,
}

impl Default for CoreBossLockSimulation {
    fn default() -> Self {
        Self {
            tick: Tick(0),
            phase: CoreBossLockPhase::BossWarning,
            next_attempt_ordinal: 1,
            rejected_late_entries: BTreeSet::new(),
        }
    }
}

impl CoreBossLockSimulation {
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    #[must_use]
    pub const fn phase(&self) -> &CoreBossLockPhase {
        &self.phase
    }

    #[must_use]
    pub const fn recall_allowed(&self) -> bool {
        true
    }

    #[must_use]
    pub const fn door_closed(&self) -> bool {
        matches!(
            self.phase,
            CoreBossLockPhase::Introduction { .. }
                | CoreBossLockPhase::Combat { .. }
                | CoreBossLockPhase::ResetPending { .. }
        )
    }

    pub fn step(
        &mut self,
        input: &CoreBossLockInput,
    ) -> Result<CoreBossLockStep, CoreBossLockError> {
        let mut staged = self.clone();
        let step = staged.step_inner(input)?;
        *self = staged;
        Ok(step)
    }

    #[allow(clippy::too_many_lines)] // A linear transaction keeps authoritative transition order reviewable.
    fn step_inner(
        &mut self,
        input: &CoreBossLockInput,
    ) -> Result<CoreBossLockStep, CoreBossLockError> {
        if input.tick != self.tick {
            return Err(CoreBossLockError::TickMismatch {
                expected: self.tick,
                received: input.tick,
            });
        }
        let entrants = validate_entrants(&input.entrants)?;
        let mut events = Vec::new();
        self.reject_late_entries(&entrants, &mut events);
        match self.phase.clone() {
            CoreBossLockPhase::BossWarning => {
                let staged = staged_participants(&entrants);
                if !staged.is_empty() {
                    events.push(CoreBossLockEvent::StagingStarted {
                        tick: self.tick,
                        participants: staged.clone(),
                    });
                    self.phase = CoreBossLockPhase::Loading {
                        started_at: self.tick,
                        staged,
                    };
                    self.maybe_begin_countdown(&entrants, &mut events)?;
                }
            }
            CoreBossLockPhase::Loading { started_at, staged } => {
                let updated = merge_staged(staged, &entrants);
                if let CoreBossLockPhase::Loading {
                    staged: current, ..
                } = &mut self.phase
                    && *current != updated
                {
                    current.clone_from(&updated);
                    events.push(CoreBossLockEvent::StagedRosterChanged {
                        tick: self.tick,
                        participants: updated,
                    });
                }
                let timeout_at = add_ticks(started_at, CORE_BOSS_LOAD_TIMEOUT_TICKS)?;
                if all_staged_loaded(&self.phase, &entrants) || self.tick >= timeout_at {
                    self.begin_countdown(&mut events)?;
                }
            }
            CoreBossLockPhase::ReadyCountdown {
                staged, closes_at, ..
            } => {
                let updated = merge_staged(staged, &entrants);
                if let CoreBossLockPhase::ReadyCountdown {
                    staged: current, ..
                } = &mut self.phase
                    && *current != updated
                {
                    current.clone_from(&updated);
                    events.push(CoreBossLockEvent::StagedRosterChanged {
                        tick: self.tick,
                        participants: updated,
                    });
                }
                if self.tick >= closes_at {
                    self.commit_or_abandon(&entrants, &mut events)?;
                }
            }
            CoreBossLockPhase::Introduction {
                activates_at, lock, ..
            } => {
                if self.tick >= activates_at {
                    events.push(CoreBossLockEvent::CombatStarted {
                        tick: self.tick,
                        lock: lock.clone(),
                    });
                    self.phase = CoreBossLockPhase::Combat { lock };
                } else {
                    self.maybe_begin_empty_reset(&entrants, &mut events)?;
                }
            }
            CoreBossLockPhase::Combat { .. } => {
                self.maybe_begin_empty_reset(&entrants, &mut events)?;
            }
            CoreBossLockPhase::ResetPending {
                resets_at, lock, ..
            } => {
                if any_locked_living_inside(&lock, &entrants) {
                    events.push(CoreBossLockEvent::EmptyResetCancelled { tick: self.tick });
                    self.phase = CoreBossLockPhase::Combat { lock };
                } else if self.tick >= resets_at {
                    events.push(CoreBossLockEvent::EmptyResetCompleted {
                        tick: self.tick,
                        cleared_hostiles: true,
                        cleared_unsecured_drops: true,
                        door_reopened: true,
                    });
                    self.phase = CoreBossLockPhase::BossWarning;
                    self.rejected_late_entries.clear();
                }
            }
        }
        let step = CoreBossLockStep {
            tick: self.tick,
            phase: self.phase.clone(),
            events,
        };
        self.tick = add_ticks(self.tick, 1)?;
        Ok(step)
    }

    fn maybe_begin_countdown(
        &mut self,
        entrants: &BTreeMap<EntityId, CoreBossEntrantInput>,
        events: &mut Vec<CoreBossLockEvent>,
    ) -> Result<(), CoreBossLockError> {
        if all_staged_loaded(&self.phase, entrants) {
            self.begin_countdown(events)?;
        }
        Ok(())
    }

    fn begin_countdown(
        &mut self,
        events: &mut Vec<CoreBossLockEvent>,
    ) -> Result<(), CoreBossLockError> {
        let CoreBossLockPhase::Loading { staged, .. } = self.phase.clone() else {
            return Ok(());
        };
        let closes_at = add_ticks(self.tick, CORE_BOSS_READY_COUNTDOWN_TICKS)?;
        events.push(CoreBossLockEvent::ReadyCountdownStarted {
            tick: self.tick,
            closes_at,
        });
        self.phase = CoreBossLockPhase::ReadyCountdown {
            started_at: self.tick,
            closes_at,
            staged,
        };
        Ok(())
    }

    fn commit_or_abandon(
        &mut self,
        entrants: &BTreeMap<EntityId, CoreBossEntrantInput>,
        events: &mut Vec<CoreBossLockEvent>,
    ) -> Result<(), CoreBossLockError> {
        let CoreBossLockPhase::ReadyCountdown { staged, .. } = &self.phase else {
            return Ok(());
        };
        let mut living = staged
            .iter()
            .filter(|participant| {
                entrants
                    .get(&participant.entity_id)
                    .is_some_and(|entrant| entrant.is_living() && entrant.inside_boundary)
            })
            .copied()
            .collect::<Vec<_>>();
        living.sort_by_key(|participant| (participant.party_slot, participant.entity_id));
        if living.is_empty() {
            events.push(CoreBossLockEvent::ReadyCountdownAbandoned { tick: self.tick });
            self.phase = CoreBossLockPhase::BossWarning;
            return Ok(());
        }
        if living.len() > usize::from(CORE_BOSS_MAXIMUM_PARTICIPANTS) {
            return Err(CoreBossLockError::TooManyLockedParticipants);
        }
        let attempt_ordinal = self.next_attempt_ordinal;
        self.next_attempt_ordinal = self
            .next_attempt_ordinal
            .checked_add(1)
            .ok_or(CoreBossLockError::AttemptOrdinalOverflow)?;
        let lock = CoreBossParticipantLock {
            attempt_ordinal,
            maximum_health: scaled_caldus_health(
                u8::try_from(living.len())
                    .map_err(|_| CoreBossLockError::TooManyLockedParticipants)?,
            )?,
            participants: living,
        };
        let activates_at = add_ticks(self.tick, CORE_CALDUS_INTRODUCTION_TICKS)?;
        events.push(CoreBossLockEvent::DoorClosed { tick: self.tick });
        events.push(CoreBossLockEvent::ParticipantLockCommitted {
            tick: self.tick,
            lock: lock.clone(),
        });
        events.push(CoreBossLockEvent::EntranceRadiusCleared { tick: self.tick });
        events.push(CoreBossLockEvent::IntroductionStarted {
            tick: self.tick,
            activates_at,
        });
        self.phase = CoreBossLockPhase::Introduction {
            started_at: self.tick,
            activates_at,
            lock,
        };
        Ok(())
    }

    fn maybe_begin_empty_reset(
        &mut self,
        entrants: &BTreeMap<EntityId, CoreBossEntrantInput>,
        events: &mut Vec<CoreBossLockEvent>,
    ) -> Result<(), CoreBossLockError> {
        let (CoreBossLockPhase::Introduction { lock, .. } | CoreBossLockPhase::Combat { lock }) =
            &self.phase
        else {
            return Ok(());
        };
        let lock = lock.clone();
        if !any_locked_living(&lock, entrants) && any_living_party_outside(&lock, entrants) {
            let resets_at = add_ticks(self.tick, CORE_BOSS_EMPTY_RESET_TICKS)?;
            events.push(CoreBossLockEvent::EmptyResetStarted {
                tick: self.tick,
                resets_at,
            });
            self.phase = CoreBossLockPhase::ResetPending {
                started_at: self.tick,
                resets_at,
                lock,
            };
        }
        Ok(())
    }

    fn reject_late_entries(
        &mut self,
        entrants: &BTreeMap<EntityId, CoreBossEntrantInput>,
        events: &mut Vec<CoreBossLockEvent>,
    ) {
        let (CoreBossLockPhase::Introduction { lock, .. }
        | CoreBossLockPhase::Combat { lock }
        | CoreBossLockPhase::ResetPending { lock, .. }) = &self.phase
        else {
            return;
        };
        let locked = lock
            .participants
            .iter()
            .map(|participant| participant.entity_id)
            .collect::<BTreeSet<_>>();
        for entrant in entrants.values().filter(|entrant| {
            entrant.is_living()
                && entrant.inside_boundary
                && !locked.contains(&entrant.participant.entity_id)
        }) {
            if self
                .rejected_late_entries
                .insert(entrant.participant.entity_id)
            {
                events.push(CoreBossLockEvent::LateEntryRejected {
                    tick: self.tick,
                    participant: entrant.participant,
                });
            }
        }
    }
}

pub fn scaled_caldus_health(locked_participants: u8) -> Result<u32, CoreBossLockError> {
    if !(CORE_BOSS_MINIMUM_PARTICIPANTS..=CORE_BOSS_MAXIMUM_PARTICIPANTS)
        .contains(&locked_participants)
    {
        return Err(CoreBossLockError::InvalidParticipantCount);
    }
    let factor = 10_000_u64
        + u64::from(CORE_CALDUS_ADDITIONAL_PARTICIPANT_HEALTH_BASIS_POINTS)
            * u64::from(locked_participants - 1);
    let numerator = u64::from(CORE_CALDUS_BASE_HEALTH)
        .checked_mul(factor)
        .ok_or(CoreBossLockError::ArithmeticOverflow)?;
    u32::try_from((numerator + 5_000) / 10_000).map_err(|_| CoreBossLockError::ArithmeticOverflow)
}

fn validate_entrants(
    entrants: &[CoreBossEntrantInput],
) -> Result<BTreeMap<EntityId, CoreBossEntrantInput>, CoreBossLockError> {
    let mut by_entity = BTreeMap::new();
    let mut slots = BTreeSet::new();
    for entrant in entrants {
        if entrant.party_slot() >= CORE_BOSS_MAXIMUM_PARTICIPANTS {
            return Err(CoreBossLockError::InvalidEntrantState);
        }
        if by_entity
            .insert(entrant.participant.entity_id, *entrant)
            .is_some()
        {
            return Err(CoreBossLockError::DuplicateEntity);
        }
        if !slots.insert(entrant.participant.party_slot) {
            return Err(CoreBossLockError::DuplicatePartySlot);
        }
    }
    Ok(by_entity)
}

impl CoreBossEntrantInput {
    const fn party_slot(self) -> u8 {
        self.participant.party_slot
    }

    const fn is_connected(self) -> bool {
        matches!(
            self.connection,
            CoreBossConnectionState::ConnectedLoading | CoreBossConnectionState::ConnectedLoaded
        )
    }

    const fn is_loaded(self) -> bool {
        matches!(self.connection, CoreBossConnectionState::ConnectedLoaded)
    }

    const fn is_living(self) -> bool {
        matches!(self.life, CoreBossLifeState::Living)
    }
}

fn staged_participants(
    entrants: &BTreeMap<EntityId, CoreBossEntrantInput>,
) -> Vec<CoreBossParticipant> {
    let mut staged = entrants
        .values()
        .filter(|entrant| entrant.is_connected() && entrant.is_living() && entrant.inside_boundary)
        .map(|entrant| entrant.participant)
        .collect::<Vec<_>>();
    staged.sort_by_key(|participant| (participant.party_slot, participant.entity_id));
    staged
}

fn merge_staged(
    mut staged: Vec<CoreBossParticipant>,
    entrants: &BTreeMap<EntityId, CoreBossEntrantInput>,
) -> Vec<CoreBossParticipant> {
    let existing = staged
        .iter()
        .map(|participant| participant.entity_id)
        .collect::<BTreeSet<_>>();
    staged.extend(
        staged_participants(entrants)
            .into_iter()
            .filter(|participant| !existing.contains(&participant.entity_id)),
    );
    staged.sort_by_key(|participant| (participant.party_slot, participant.entity_id));
    staged
}

fn all_staged_loaded(
    phase: &CoreBossLockPhase,
    entrants: &BTreeMap<EntityId, CoreBossEntrantInput>,
) -> bool {
    let CoreBossLockPhase::Loading { staged, .. } = phase else {
        return false;
    };
    !staged.is_empty()
        && staged.iter().all(|participant| {
            entrants
                .get(&participant.entity_id)
                .is_some_and(|entrant| entrant.is_connected() && entrant.is_loaded())
        })
}

fn any_locked_living(
    lock: &CoreBossParticipantLock,
    entrants: &BTreeMap<EntityId, CoreBossEntrantInput>,
) -> bool {
    lock.participants.iter().any(|participant| {
        entrants
            .get(&participant.entity_id)
            .is_some_and(|entrant| entrant.is_living())
    })
}

fn any_locked_living_inside(
    lock: &CoreBossParticipantLock,
    entrants: &BTreeMap<EntityId, CoreBossEntrantInput>,
) -> bool {
    lock.participants.iter().any(|participant| {
        entrants
            .get(&participant.entity_id)
            .is_some_and(|entrant| entrant.is_living() && entrant.inside_boundary)
    })
}

fn any_living_party_outside(
    lock: &CoreBossParticipantLock,
    entrants: &BTreeMap<EntityId, CoreBossEntrantInput>,
) -> bool {
    let locked = lock
        .participants
        .iter()
        .map(|participant| participant.entity_id)
        .collect::<BTreeSet<_>>();
    entrants.values().any(|entrant| {
        entrant.is_living()
            && !entrant.inside_boundary
            && !locked.contains(&entrant.participant.entity_id)
    })
}

fn add_ticks(tick: Tick, count: u32) -> Result<Tick, CoreBossLockError> {
    tick.0
        .checked_add(u64::from(count))
        .map(Tick)
        .ok_or(CoreBossLockError::TickOverflow)
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CoreBossLockError {
    #[error("boss lock expected tick {expected:?}, received {received:?}")]
    TickMismatch { expected: Tick, received: Tick },
    #[error("boss entrant state is invalid")]
    InvalidEntrantState,
    #[error("boss entrants contain a duplicate entity")]
    DuplicateEntity,
    #[error("boss entrants contain a duplicate immutable party slot")]
    DuplicatePartySlot,
    #[error("boss participant count must be within 1..=8")]
    InvalidParticipantCount,
    #[error("boss lock contains more than eight participants")]
    TooManyLockedParticipants,
    #[error("boss attempt ordinal overflowed")]
    AttemptOrdinalOverflow,
    #[error("boss lock tick overflowed")]
    TickOverflow,
    #[error("boss lock arithmetic overflowed")]
    ArithmeticOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn participant(id: u64, slot: u8) -> CoreBossParticipant {
        CoreBossParticipant {
            entity_id: EntityId::new(id).expect("entity"),
            party_slot: slot,
        }
    }

    fn entrant(id: u64, slot: u8) -> CoreBossEntrantInput {
        CoreBossEntrantInput {
            participant: participant(id, slot),
            connection: CoreBossConnectionState::ConnectedLoaded,
            life: CoreBossLifeState::Living,
            inside_boundary: true,
        }
    }

    fn step(
        simulation: &mut CoreBossLockSimulation,
        entrants: Vec<CoreBossEntrantInput>,
    ) -> CoreBossLockStep {
        simulation
            .step(&CoreBossLockInput {
                tick: simulation.tick(),
                entrants,
            })
            .expect("boss lock step")
    }

    #[test]
    fn health_scaling_is_exact_for_every_supported_lock_size() {
        assert_eq!(
            (1..=8)
                .map(|count| scaled_caldus_health(count).expect("health"))
                .collect::<Vec<_>>(),
            [
                7_200, 12_384, 17_568, 22_752, 27_936, 33_120, 38_304, 43_488
            ]
        );
        assert_eq!(
            scaled_caldus_health(0),
            Err(CoreBossLockError::InvalidParticipantCount)
        );
        assert_eq!(
            scaled_caldus_health(9),
            Err(CoreBossLockError::InvalidParticipantCount)
        );
    }

    #[test]
    fn loaded_solo_runs_exact_countdown_lock_and_introduction() {
        let mut simulation = CoreBossLockSimulation::default();
        let first = step(&mut simulation, vec![entrant(1, 0)]);
        assert!(matches!(
            first.phase,
            CoreBossLockPhase::ReadyCountdown {
                closes_at: Tick(150),
                ..
            }
        ));
        for _ in 1..150 {
            step(&mut simulation, vec![entrant(1, 0)]);
        }
        let closed = step(&mut simulation, vec![entrant(1, 0)]);
        let CoreBossLockPhase::Introduction {
            activates_at,
            ref lock,
            ..
        } = closed.phase
        else {
            panic!("introduction");
        };
        assert_eq!(activates_at, Tick(225));
        assert_eq!(lock.attempt_ordinal, 1);
        assert_eq!(lock.maximum_health, 7_200);
        assert!(
            closed
                .events
                .iter()
                .any(|event| matches!(event, CoreBossLockEvent::DoorClosed { tick: Tick(150) }))
        );
        for _ in 151..225 {
            step(&mut simulation, vec![entrant(1, 0)]);
        }
        let active = step(&mut simulation, vec![entrant(1, 0)]);
        assert!(matches!(active.phase, CoreBossLockPhase::Combat { .. }));
        assert!(simulation.recall_allowed());
        assert!(simulation.door_closed());
    }

    #[test]
    fn loading_timeout_late_staging_and_party_slot_order_are_deterministic() {
        let mut simulation = CoreBossLockSimulation::default();
        let mut one = entrant(20, 2);
        one.connection = CoreBossConnectionState::ConnectedLoading;
        step(&mut simulation, vec![one]);
        for tick in 1..100 {
            let _ = tick;
            step(&mut simulation, vec![one]);
        }
        let mut two = entrant(10, 0);
        two.connection = CoreBossConnectionState::ConnectedLoading;
        for _ in 100..300 {
            step(&mut simulation, vec![one, two]);
        }
        let timeout = step(&mut simulation, vec![two, one]);
        let CoreBossLockPhase::ReadyCountdown {
            ref staged,
            closes_at,
            ..
        } = timeout.phase
        else {
            panic!("countdown");
        };
        assert_eq!(staged, &[participant(10, 0), participant(20, 2)]);
        assert_eq!(closes_at, Tick(450));
    }

    #[test]
    fn zero_living_at_closure_consumes_no_attempt_and_retry_uses_one() {
        let mut simulation = CoreBossLockSimulation::default();
        step(&mut simulation, vec![entrant(1, 0)]);
        for _ in 1..150 {
            step(&mut simulation, vec![entrant(1, 0)]);
        }
        let mut dead = entrant(1, 0);
        dead.life = CoreBossLifeState::Dead;
        dead.inside_boundary = false;
        let abandoned = step(&mut simulation, vec![dead]);
        assert!(matches!(abandoned.phase, CoreBossLockPhase::BossWarning));
        step(&mut simulation, vec![entrant(1, 0)]);
        for _ in 152..301 {
            step(&mut simulation, vec![entrant(1, 0)]);
        }
        let locked = step(&mut simulation, vec![entrant(1, 0)]);
        let CoreBossLockPhase::Introduction { lock, .. } = locked.phase else {
            panic!("lock");
        };
        assert_eq!(lock.attempt_ordinal, 1);
    }

    #[test]
    fn late_entry_is_rejected_once_and_never_changes_scaling() {
        let mut simulation = CoreBossLockSimulation::default();
        step(&mut simulation, vec![entrant(1, 0)]);
        for _ in 1..=150 {
            step(&mut simulation, vec![entrant(1, 0)]);
        }
        let rejected = step(&mut simulation, vec![entrant(1, 0), entrant(2, 1)]);
        assert_eq!(
            rejected
                .events
                .iter()
                .filter(|event| matches!(event, CoreBossLockEvent::LateEntryRejected { .. }))
                .count(),
            1
        );
        let duplicate = step(&mut simulation, vec![entrant(1, 0), entrant(2, 1)]);
        assert!(
            !duplicate
                .events
                .iter()
                .any(|event| matches!(event, CoreBossLockEvent::LateEntryRejected { .. }))
        );
        let CoreBossLockPhase::Introduction { lock, .. } = simulation.phase() else {
            panic!("introduction");
        };
        assert_eq!(lock.maximum_health, 7_200);
        assert_eq!(lock.participants, [participant(1, 0)]);
    }

    #[test]
    fn zero_living_reset_requires_outside_party_and_honors_cancel_boundary() {
        let mut simulation = CoreBossLockSimulation::default();
        step(&mut simulation, vec![entrant(1, 0)]);
        for _ in 1..=225 {
            step(&mut simulation, vec![entrant(1, 0)]);
        }
        let mut dead = entrant(1, 0);
        dead.life = CoreBossLifeState::Dead;
        dead.inside_boundary = false;
        let mut outside = entrant(2, 1);
        outside.inside_boundary = false;
        let reset = step(&mut simulation, vec![dead, outside]);
        let CoreBossLockPhase::ResetPending { resets_at, .. } = reset.phase else {
            panic!("reset");
        };
        assert_eq!(resets_at, Tick(376));
        for _ in 227..376 {
            step(&mut simulation, vec![dead, outside]);
        }
        let restored = entrant(1, 0);
        let cancelled = step(&mut simulation, vec![restored, outside]);
        assert!(matches!(cancelled.phase, CoreBossLockPhase::Combat { .. }));
        assert!(
            cancelled
                .events
                .contains(&CoreBossLockEvent::EmptyResetCancelled { tick: Tick(376) })
        );

        let started_again = step(&mut simulation, vec![dead, outside]);
        assert!(matches!(
            started_again.phase,
            CoreBossLockPhase::ResetPending { .. }
        ));
        for _ in 378..527 {
            step(&mut simulation, vec![dead, outside]);
        }
        let completed = step(&mut simulation, vec![dead, outside]);
        assert!(matches!(completed.phase, CoreBossLockPhase::BossWarning));
    }

    #[test]
    fn invalid_inputs_roll_back_phase_tick_and_attempt_identity() {
        let mut simulation = CoreBossLockSimulation::default();
        let before = simulation.clone();
        let error = simulation
            .step(&CoreBossLockInput {
                tick: Tick(0),
                entrants: vec![entrant(1, 0), entrant(2, 0)],
            })
            .expect_err("duplicate slot");
        assert_eq!(error, CoreBossLockError::DuplicatePartySlot);
        assert_eq!(simulation, before);
    }
}
