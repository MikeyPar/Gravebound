//! Deterministic authority for ordinary fixed-layout dungeon rooms.
//!
//! Content construction owns rosters and anchors. This module owns only `DNG-005` activation,
//! door safety, warning, completion, quiet-period, and reset state.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{Tick, duration_ms_to_ticks_nearest};

pub const FIXED_ROOM_GROUP_WARNING_TICKS: u64 = duration_ms_to_ticks_nearest(900);
pub const FIXED_ROOM_QUIET_TICKS: u64 = duration_ms_to_ticks_nearest(2_000);
pub const FIXED_ROOM_EMPTY_RESET_TICKS: u64 = duration_ms_to_ticks_nearest(3_000);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FixedRoomPhase {
    Dormant,
    AwaitingDoorSafety,
    SpawnWarning,
    Active,
    Quiet,
    Cleared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedRoomInput {
    pub crossed_activation_boundary: bool,
    pub living_inside: u16,
    pub living_party_outside: u16,
    pub doorway_hurtbox_blocked: bool,
    pub required_hostiles_remaining: u16,
    pub required_objectives_remaining: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedRoomEvent {
    ParticipantLocked {
        activation_ordinal: u32,
        participant_count: u16,
    },
    DoorsClosed,
    BeginGroupWarning {
        warning_ticks: u64,
    },
    EncounterActivated,
    CompletionCommitted {
        activation_ordinal: u32,
    },
    ClearHostileOutput,
    BeginQuietPeriod {
        quiet_ticks: u64,
    },
    DiscardUnsecuredDrops,
    RoomReset,
    DoorsOpened,
}

/// Exact capacity-one M03 room lifecycle with atomic step semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixedRoomSimulation {
    required_hostiles: u16,
    required_objectives: u16,
    phase: FixedRoomPhase,
    doors_closed: bool,
    activation_ordinal: u32,
    warning_ends_at: Option<Tick>,
    quiet_ends_at: Option<Tick>,
    empty_since: Option<Tick>,
    previous_hostiles_remaining: u16,
    previous_objectives_remaining: u16,
    last_step_tick: Option<Tick>,
}

impl FixedRoomSimulation {
    pub fn new(required_hostiles: u16, required_objectives: u16) -> Result<Self, FixedRoomError> {
        if required_hostiles == 0 && required_objectives == 0 {
            return Err(FixedRoomError::EmptyEncounter);
        }
        Ok(Self {
            required_hostiles,
            required_objectives,
            phase: FixedRoomPhase::Dormant,
            doors_closed: false,
            activation_ordinal: 0,
            warning_ends_at: None,
            quiet_ends_at: None,
            empty_since: None,
            previous_hostiles_remaining: required_hostiles,
            previous_objectives_remaining: required_objectives,
            last_step_tick: None,
        })
    }

    #[must_use]
    pub const fn phase(&self) -> FixedRoomPhase {
        self.phase
    }

    #[must_use]
    pub const fn doors_closed(&self) -> bool {
        self.doors_closed
    }

    #[must_use]
    pub const fn activation_ordinal(&self) -> u32 {
        self.activation_ordinal
    }

    pub fn step(
        &mut self,
        tick: Tick,
        input: FixedRoomInput,
    ) -> Result<Vec<FixedRoomEvent>, FixedRoomError> {
        let mut staged = self.clone();
        let events = staged.step_inner(tick, input)?;
        *self = staged;
        Ok(events)
    }

    fn step_inner(
        &mut self,
        tick: Tick,
        input: FixedRoomInput,
    ) -> Result<Vec<FixedRoomEvent>, FixedRoomError> {
        self.validate_input(tick, input)?;
        self.last_step_tick = Some(tick);
        let mut events = Vec::with_capacity(5);

        if self.phase == FixedRoomPhase::Dormant && input.crossed_activation_boundary {
            self.activation_ordinal = self
                .activation_ordinal
                .checked_add(1)
                .ok_or(FixedRoomError::ActivationOverflow)?;
            self.phase = FixedRoomPhase::AwaitingDoorSafety;
            self.previous_hostiles_remaining = self.required_hostiles;
            self.previous_objectives_remaining = self.required_objectives;
            events.push(FixedRoomEvent::ParticipantLocked {
                activation_ordinal: self.activation_ordinal,
                participant_count: 1,
            });
        }

        if self.phase == FixedRoomPhase::AwaitingDoorSafety && !input.doorway_hurtbox_blocked {
            self.phase = FixedRoomPhase::SpawnWarning;
            self.doors_closed = true;
            self.warning_ends_at = Some(add_ticks(tick, FIXED_ROOM_GROUP_WARNING_TICKS)?);
            events.push(FixedRoomEvent::DoorsClosed);
            events.push(FixedRoomEvent::BeginGroupWarning {
                warning_ticks: FIXED_ROOM_GROUP_WARNING_TICKS,
            });
        }

        if self.phase == FixedRoomPhase::SpawnWarning
            && self.warning_ends_at.is_some_and(|due| tick >= due)
        {
            self.phase = FixedRoomPhase::Active;
            self.warning_ends_at = None;
            events.push(FixedRoomEvent::EncounterActivated);
        }

        if self.phase == FixedRoomPhase::Active {
            self.validate_progress(input)?;
            self.previous_hostiles_remaining = input.required_hostiles_remaining;
            self.previous_objectives_remaining = input.required_objectives_remaining;
            if input.required_hostiles_remaining == 0 && input.required_objectives_remaining == 0 {
                self.phase = FixedRoomPhase::Quiet;
                self.quiet_ends_at = Some(add_ticks(tick, FIXED_ROOM_QUIET_TICKS)?);
                events.push(FixedRoomEvent::CompletionCommitted {
                    activation_ordinal: self.activation_ordinal,
                });
                events.push(FixedRoomEvent::ClearHostileOutput);
                events.push(FixedRoomEvent::BeginQuietPeriod {
                    quiet_ticks: FIXED_ROOM_QUIET_TICKS,
                });
            } else {
                self.update_empty_reset(tick, input, &mut events)?;
            }
        } else if matches!(
            self.phase,
            FixedRoomPhase::AwaitingDoorSafety | FixedRoomPhase::SpawnWarning
        ) {
            self.update_empty_reset(tick, input, &mut events)?;
        } else {
            self.empty_since = None;
        }

        if self.phase == FixedRoomPhase::Quiet && self.quiet_ends_at.is_some_and(|due| tick >= due)
        {
            self.phase = FixedRoomPhase::Cleared;
            self.quiet_ends_at = None;
            self.doors_closed = false;
            events.push(FixedRoomEvent::DoorsOpened);
        }
        Ok(events)
    }

    fn validate_input(&self, tick: Tick, input: FixedRoomInput) -> Result<(), FixedRoomError> {
        if self.last_step_tick.is_some_and(|last| tick <= last) {
            return Err(FixedRoomError::NonMonotonicTick);
        }
        if input.living_inside > 1 {
            return Err(FixedRoomError::CapacityExceeded);
        }
        if self.phase == FixedRoomPhase::Dormant
            && input.crossed_activation_boundary
            && input.living_inside != 1
        {
            return Err(FixedRoomError::ActivationWithoutParticipant);
        }
        match self.phase {
            FixedRoomPhase::Dormant
                if input.required_hostiles_remaining != self.required_hostiles
                    || input.required_objectives_remaining != self.required_objectives =>
            {
                return Err(FixedRoomError::ProgressOutsideEncounter);
            }
            FixedRoomPhase::Cleared
                if input.required_hostiles_remaining != 0
                    || input.required_objectives_remaining != 0 =>
            {
                return Err(FixedRoomError::ProgressOutsideEncounter);
            }
            _ => {}
        }
        if matches!(
            self.phase,
            FixedRoomPhase::AwaitingDoorSafety | FixedRoomPhase::SpawnWarning
        ) && (input.required_hostiles_remaining != self.required_hostiles
            || input.required_objectives_remaining != self.required_objectives)
            && !matches!(
                self.phase,
                FixedRoomPhase::SpawnWarning
                    if self.warning_ends_at.is_some_and(|due| tick >= due)
            )
        {
            return Err(FixedRoomError::ProgressDuringWarning);
        }
        Ok(())
    }

    fn validate_progress(&self, input: FixedRoomInput) -> Result<(), FixedRoomError> {
        if input.required_hostiles_remaining > self.previous_hostiles_remaining
            || input.required_objectives_remaining > self.previous_objectives_remaining
        {
            return Err(FixedRoomError::ProgressRegressed);
        }
        Ok(())
    }

    fn update_empty_reset(
        &mut self,
        tick: Tick,
        input: FixedRoomInput,
        events: &mut Vec<FixedRoomEvent>,
    ) -> Result<(), FixedRoomError> {
        if input.living_inside == 0 && input.living_party_outside > 0 {
            let empty_since = *self.empty_since.get_or_insert(tick);
            if elapsed_ticks(empty_since, tick)? >= FIXED_ROOM_EMPTY_RESET_TICKS {
                self.phase = FixedRoomPhase::Dormant;
                self.doors_closed = false;
                self.warning_ends_at = None;
                self.quiet_ends_at = None;
                self.empty_since = None;
                self.previous_hostiles_remaining = self.required_hostiles;
                self.previous_objectives_remaining = self.required_objectives;
                events.push(FixedRoomEvent::ClearHostileOutput);
                events.push(FixedRoomEvent::DiscardUnsecuredDrops);
                events.push(FixedRoomEvent::RoomReset);
                events.push(FixedRoomEvent::DoorsOpened);
            }
        } else {
            self.empty_since = None;
        }
        Ok(())
    }
}

fn add_ticks(tick: Tick, amount: u64) -> Result<Tick, FixedRoomError> {
    tick.0
        .checked_add(amount)
        .map(Tick)
        .ok_or(FixedRoomError::TickOverflow)
}

fn elapsed_ticks(start: Tick, end: Tick) -> Result<u64, FixedRoomError> {
    end.0
        .checked_sub(start.0)
        .ok_or(FixedRoomError::NonMonotonicTick)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum FixedRoomError {
    #[error("fixed room requires at least one hostile or objective")]
    EmptyEncounter,
    #[error("fixed-room ticks must increase monotonically")]
    NonMonotonicTick,
    #[error("M03 fixed-room capacity is exactly one")]
    CapacityExceeded,
    #[error("fixed-room activation requires one living participant inside")]
    ActivationWithoutParticipant,
    #[error("fixed-room progress changed outside an active encounter")]
    ProgressOutsideEncounter,
    #[error("fixed-room progress changed during its harmless spawn warning")]
    ProgressDuringWarning,
    #[error("fixed-room required progress increased within one activation")]
    ProgressRegressed,
    #[error("fixed-room activation ordinal overflowed")]
    ActivationOverflow,
    #[error("fixed-room tick arithmetic overflowed")]
    TickOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(hostiles: u16) -> FixedRoomInput {
        FixedRoomInput {
            crossed_activation_boundary: false,
            living_inside: 1,
            living_party_outside: 0,
            doorway_hurtbox_blocked: false,
            required_hostiles_remaining: hostiles,
            required_objectives_remaining: 0,
        }
    }

    #[test]
    fn door_safety_warning_completion_and_quiet_boundaries_are_exact() {
        let mut room = FixedRoomSimulation::new(8, 0).expect("room");
        let mut blocked = input(8);
        blocked.crossed_activation_boundary = true;
        blocked.doorway_hurtbox_blocked = true;
        assert_eq!(
            room.step(Tick(10), blocked).expect("lock"),
            [FixedRoomEvent::ParticipantLocked {
                activation_ordinal: 1,
                participant_count: 1,
            }]
        );
        assert!(!room.doors_closed());
        assert_eq!(
            room.step(Tick(11), input(8)).expect("safe close"),
            [
                FixedRoomEvent::DoorsClosed,
                FixedRoomEvent::BeginGroupWarning { warning_ticks: 27 },
            ]
        );
        assert!(
            room.step(Tick(37), input(8))
                .expect("pre-warning")
                .is_empty()
        );
        assert_eq!(
            room.step(Tick(38), input(8)).expect("activate"),
            [FixedRoomEvent::EncounterActivated]
        );
        assert_eq!(
            room.step(Tick(39), input(0)).expect("complete"),
            [
                FixedRoomEvent::CompletionCommitted {
                    activation_ordinal: 1,
                },
                FixedRoomEvent::ClearHostileOutput,
                FixedRoomEvent::BeginQuietPeriod { quiet_ticks: 60 },
            ]
        );
        assert!(room.step(Tick(98), input(0)).expect("pre-open").is_empty());
        assert_eq!(
            room.step(Tick(99), input(0)).expect("open"),
            [FixedRoomEvent::DoorsOpened]
        );
        assert_eq!(room.phase(), FixedRoomPhase::Cleared);
    }

    #[test]
    fn exact_activation_tick_can_commit_authoritative_completion() {
        let mut room = FixedRoomSimulation::new(1, 0).expect("room");
        let mut activation = input(1);
        activation.crossed_activation_boundary = true;
        room.step(Tick(1), activation).expect("warning start");
        assert_eq!(
            room.step(Tick(28), input(0)).expect("activation clear"),
            [
                FixedRoomEvent::EncounterActivated,
                FixedRoomEvent::CompletionCommitted {
                    activation_ordinal: 1,
                },
                FixedRoomEvent::ClearHostileOutput,
                FixedRoomEvent::BeginQuietPeriod { quiet_ticks: 60 },
            ]
        );
    }

    #[test]
    fn empty_reset_is_exact_boundary_reentry_safe_and_rewardless() {
        let mut room = FixedRoomSimulation::new(1, 0).expect("room");
        let mut crossed = input(1);
        crossed.crossed_activation_boundary = true;
        room.step(Tick(1), crossed).expect("activate warning");
        room.step(Tick(28), input(1)).expect("active");
        let mut empty = input(1);
        empty.living_inside = 0;
        empty.living_party_outside = 1;
        room.step(Tick(29), empty).expect("empty start");
        assert!(room.step(Tick(118), empty).expect("pre-reset").is_empty());
        assert_eq!(
            room.step(Tick(119), empty).expect("reset"),
            [
                FixedRoomEvent::ClearHostileOutput,
                FixedRoomEvent::DiscardUnsecuredDrops,
                FixedRoomEvent::RoomReset,
                FixedRoomEvent::DoorsOpened,
            ]
        );
        assert_eq!(room.phase(), FixedRoomPhase::Dormant);

        let mut second = input(1);
        second.crossed_activation_boundary = true;
        room.step(Tick(120), second).expect("second activation");
        room.step(Tick(147), input(1)).expect("second active");
        room.step(Tick(148), empty).expect("second empty");
        assert!(
            room.step(Tick(237), empty)
                .expect("second pre-boundary")
                .is_empty()
        );
        assert!(
            room.step(Tick(238), input(1))
                .expect("boundary return")
                .is_empty()
        );
        assert_eq!(room.phase(), FixedRoomPhase::Active);
        assert_eq!(room.activation_ordinal(), 2);
    }

    #[test]
    fn invalid_input_rolls_back_every_state_field() {
        let mut room = FixedRoomSimulation::new(2, 0).expect("room");
        let before = room.clone();
        let mut invalid = input(2);
        invalid.crossed_activation_boundary = true;
        invalid.living_inside = 2;
        assert_eq!(
            room.step(Tick(1), invalid),
            Err(FixedRoomError::CapacityExceeded)
        );
        assert_eq!(room, before);
    }
}
