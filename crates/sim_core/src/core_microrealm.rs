use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{Tick, TilePoint, duration_ms_to_ticks_nearest};

pub const CORE_MICROREALM_TRIGGER_DELAY_TICKS: u64 = duration_ms_to_ticks_nearest(1_000);
pub const CORE_MICROREALM_PACK_WARNING_TICKS: u64 = duration_ms_to_ticks_nearest(900);
pub const CORE_MICROREALM_EMPTY_RESET_TICKS: u64 = duration_ms_to_ticks_nearest(5_000);
const MOVEMENT_TRIGGER_DISTANCE_MILLI_TILES: i64 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoreMicrorealmPhase {
    Dormant,
    Waiting,
    Active,
    Cleared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreMicrorealmInput {
    pub entrant_position: TilePoint,
    pub primary_released: bool,
    pub living_participants: u16,
    pub pack_cleared: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreMicrorealmEvent {
    BeginPackWarning { warning_ticks: u64 },
    ResetPack,
    Cleared,
}

/// Exact capacity-one Core microrealm lifecycle. Enemy construction remains owned by `03D`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreMicrorealmSimulation {
    entry_spawn: TilePoint,
    phase: CoreMicrorealmPhase,
    next_transition_tick: Option<Tick>,
    empty_since_tick: Option<Tick>,
    last_step_tick: Option<Tick>,
}

impl CoreMicrorealmSimulation {
    #[must_use]
    pub const fn new(entry_spawn: TilePoint) -> Self {
        Self {
            entry_spawn,
            phase: CoreMicrorealmPhase::Dormant,
            next_transition_tick: None,
            empty_since_tick: None,
            last_step_tick: None,
        }
    }

    #[must_use]
    pub const fn phase(&self) -> CoreMicrorealmPhase {
        self.phase
    }

    #[must_use]
    pub const fn bell_portal_available(&self) -> bool {
        matches!(self.phase, CoreMicrorealmPhase::Cleared)
    }

    pub fn step(
        &mut self,
        tick: Tick,
        input: CoreMicrorealmInput,
    ) -> Result<Vec<CoreMicrorealmEvent>, CoreMicrorealmError> {
        if self.last_step_tick.is_some_and(|last| tick <= last) {
            return Err(CoreMicrorealmError::NonMonotonicTick);
        }
        if input.living_participants > 1 {
            return Err(CoreMicrorealmError::CapacityExceeded);
        }
        if input.pack_cleared && self.phase != CoreMicrorealmPhase::Active {
            return Err(CoreMicrorealmError::ClearOutsideActivePhase);
        }
        self.last_step_tick = Some(tick);
        let mut events = Vec::with_capacity(1);

        if self.phase == CoreMicrorealmPhase::Dormant
            && input.living_participants == 1
            && (input.primary_released || self.moved_beyond_trigger(input.entrant_position))
        {
            self.phase = CoreMicrorealmPhase::Waiting;
            self.next_transition_tick = Some(add_ticks(tick, CORE_MICROREALM_TRIGGER_DELAY_TICKS)?);
        }

        if self.phase == CoreMicrorealmPhase::Waiting
            && self.next_transition_tick.is_some_and(|due| tick >= due)
        {
            self.phase = CoreMicrorealmPhase::Active;
            self.next_transition_tick = None;
            events.push(CoreMicrorealmEvent::BeginPackWarning {
                warning_ticks: CORE_MICROREALM_PACK_WARNING_TICKS,
            });
        }

        if self.phase == CoreMicrorealmPhase::Active {
            if input.pack_cleared {
                self.phase = CoreMicrorealmPhase::Cleared;
                self.empty_since_tick = None;
                events.push(CoreMicrorealmEvent::Cleared);
            } else if input.living_participants == 0 {
                let empty_since = *self.empty_since_tick.get_or_insert(tick);
                if elapsed_ticks(empty_since, tick)? >= CORE_MICROREALM_EMPTY_RESET_TICKS {
                    self.phase = CoreMicrorealmPhase::Dormant;
                    self.empty_since_tick = None;
                    events.push(CoreMicrorealmEvent::ResetPack);
                }
            } else {
                self.empty_since_tick = None;
            }
        }
        Ok(events)
    }

    fn moved_beyond_trigger(&self, position: TilePoint) -> bool {
        let dx = i64::from(position.x_milli_tiles) - i64::from(self.entry_spawn.x_milli_tiles);
        let dy = i64::from(position.y_milli_tiles) - i64::from(self.entry_spawn.y_milli_tiles);
        let threshold = MOVEMENT_TRIGGER_DISTANCE_MILLI_TILES;
        dx * dx + dy * dy > threshold * threshold
    }
}

fn add_ticks(tick: Tick, ticks: u64) -> Result<Tick, CoreMicrorealmError> {
    tick.0
        .checked_add(ticks)
        .map(Tick)
        .ok_or(CoreMicrorealmError::TickOverflow)
}

fn elapsed_ticks(start: Tick, end: Tick) -> Result<u64, CoreMicrorealmError> {
    end.0
        .checked_sub(start.0)
        .ok_or(CoreMicrorealmError::NonMonotonicTick)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CoreMicrorealmError {
    #[error("Core microrealm ticks must increase monotonically")]
    NonMonotonicTick,
    #[error("Core microrealm capacity is exactly one")]
    CapacityExceeded,
    #[error("Core microrealm pack cannot clear outside Active")]
    ClearOutsideActivePhase,
    #[error("Core microrealm tick arithmetic overflowed")]
    TickOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPAWN: TilePoint = TilePoint::new(8_500, 40_500);

    fn input(position: TilePoint) -> CoreMicrorealmInput {
        CoreMicrorealmInput {
            entrant_position: position,
            primary_released: false,
            living_participants: 1,
            pack_cleared: false,
        }
    }

    #[test]
    fn movement_trigger_waits_one_second_then_requests_exact_warning() {
        let mut simulation = CoreMicrorealmSimulation::new(SPAWN);
        assert!(
            simulation
                .step(Tick(1), input(TilePoint::new(9_500, 40_500)))
                .expect("exactly one tile")
                .is_empty()
        );
        assert_eq!(simulation.phase(), CoreMicrorealmPhase::Dormant);
        assert!(
            simulation
                .step(Tick(2), input(TilePoint::new(9_501, 40_500)))
                .expect("beyond one tile")
                .is_empty()
        );
        assert_eq!(simulation.phase(), CoreMicrorealmPhase::Waiting);
        assert!(
            simulation
                .step(Tick(31), input(TilePoint::new(9_501, 40_500)))
                .expect("before due")
                .is_empty()
        );
        assert_eq!(
            simulation
                .step(Tick(32), input(TilePoint::new(9_501, 40_500)))
                .expect("due"),
            vec![CoreMicrorealmEvent::BeginPackWarning { warning_ticks: 27 }]
        );
        assert_eq!(simulation.phase(), CoreMicrorealmPhase::Active);
    }

    #[test]
    fn primary_release_triggers_and_active_empty_state_resets_after_five_seconds() {
        let mut simulation = CoreMicrorealmSimulation::new(SPAWN);
        let mut released = input(SPAWN);
        released.primary_released = true;
        simulation.step(Tick(10), released).expect("release");
        simulation.step(Tick(40), input(SPAWN)).expect("active");

        let mut empty = input(SPAWN);
        empty.living_participants = 0;
        assert!(simulation.step(Tick(41), empty).expect("empty").is_empty());
        assert!(
            simulation
                .step(Tick(190), empty)
                .expect("before reset")
                .is_empty()
        );
        assert_eq!(
            simulation.step(Tick(191), empty).expect("reset"),
            vec![CoreMicrorealmEvent::ResetPack]
        );
        assert_eq!(simulation.phase(), CoreMicrorealmPhase::Dormant);
    }

    #[test]
    fn authoritative_clear_is_terminal_and_opens_bell_portal() {
        let mut simulation = CoreMicrorealmSimulation::new(SPAWN);
        let mut released = input(SPAWN);
        released.primary_released = true;
        simulation.step(Tick(1), released).expect("release");
        simulation.step(Tick(31), input(SPAWN)).expect("active");
        let mut cleared = input(SPAWN);
        cleared.pack_cleared = true;
        assert_eq!(
            simulation.step(Tick(32), cleared).expect("clear"),
            vec![CoreMicrorealmEvent::Cleared]
        );
        assert!(simulation.bell_portal_available());
        assert_eq!(simulation.phase(), CoreMicrorealmPhase::Cleared);
        assert!(
            simulation
                .step(Tick(33), input(SPAWN))
                .expect("idle")
                .is_empty()
        );
        assert_eq!(simulation.phase(), CoreMicrorealmPhase::Cleared);
    }

    #[test]
    fn capacity_clear_order_and_tick_regressions_fail_closed() {
        let mut simulation = CoreMicrorealmSimulation::new(SPAWN);
        let mut over_capacity = input(SPAWN);
        over_capacity.living_participants = 2;
        assert_eq!(
            simulation.step(Tick(1), over_capacity),
            Err(CoreMicrorealmError::CapacityExceeded)
        );
        let mut premature_clear = input(SPAWN);
        premature_clear.pack_cleared = true;
        assert_eq!(
            simulation.step(Tick(2), premature_clear),
            Err(CoreMicrorealmError::ClearOutsideActivePhase)
        );
        simulation.step(Tick(3), input(SPAWN)).expect("first valid");
        assert_eq!(
            simulation.step(Tick(3), input(SPAWN)),
            Err(CoreMicrorealmError::NonMonotonicTick)
        );
    }
}
