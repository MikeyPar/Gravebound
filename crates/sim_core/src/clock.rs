use std::{fmt, time::Duration};

use serde::{Deserialize, Serialize};

/// The exact authoritative simulation frequency.
pub const TICK_RATE_HZ: u32 = 30;
const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// Monotonic authoritative simulation tick.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Tick(pub u64);

impl fmt::Display for Tick {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Tick {
    /// Returns the next tick, failing on the practically unreachable `u64` boundary.
    #[must_use]
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// Converts authored integer milliseconds to simulation ticks using ceiling-to-tick.
#[must_use]
pub const fn duration_ms_to_ticks_ceil(milliseconds: u64) -> u64 {
    let scaled = milliseconds.saturating_mul(TICK_RATE_HZ as u64);
    scaled.div_ceil(1_000)
}

/// Render-loop accumulator that yields an exact average of 30 authoritative steps per second.
///
/// The accumulator stores `elapsed_nanoseconds * 30`, avoiding the drift caused by pretending a
/// 30 Hz step is exactly 33,333,333 nanoseconds. Simulation code still advances one integer tick at
/// a time; elapsed wall time never enters authoritative state.
#[derive(Debug, Default, Clone)]
pub struct FixedStepClock {
    tick: Tick,
    scaled_nanosecond_remainder: u128,
}

impl FixedStepClock {
    /// Creates a clock beginning at tick zero.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            tick: Tick(0),
            scaled_nanosecond_remainder: 0,
        }
    }

    /// Returns the latest completed tick.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// Adds presentation elapsed time and returns the number of fixed simulation steps now due.
    pub fn push_elapsed(&mut self, elapsed: Duration) -> u64 {
        let scaled = elapsed.as_nanos().saturating_mul(u128::from(TICK_RATE_HZ));
        self.scaled_nanosecond_remainder = self.scaled_nanosecond_remainder.saturating_add(scaled);
        let steps = self.scaled_nanosecond_remainder / NANOS_PER_SECOND;
        self.scaled_nanosecond_remainder %= NANOS_PER_SECOND;
        u64::try_from(steps).unwrap_or(u64::MAX)
    }

    /// Marks one fixed step complete.
    pub fn complete_step(&mut self) -> Option<Tick> {
        self.tick = self.tick.checked_next()?;
        Some(self.tick)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_durations_never_compile_shorter() {
        assert_eq!(duration_ms_to_ticks_ceil(0), 0);
        assert_eq!(duration_ms_to_ticks_ceil(1), 1);
        assert_eq!(duration_ms_to_ticks_ceil(33), 1);
        assert_eq!(duration_ms_to_ticks_ceil(34), 2);
        assert_eq!(duration_ms_to_ticks_ceil(1_000), 30);
    }

    #[test]
    fn accumulator_produces_exact_thirty_hertz_average() {
        let mut clock = FixedStepClock::new();
        assert_eq!(clock.push_elapsed(Duration::from_millis(500)), 15);
        for _ in 0..15 {
            clock.complete_step().expect("tick range");
        }
        assert_eq!(clock.push_elapsed(Duration::from_millis(500)), 15);
        for _ in 0..15 {
            clock.complete_step().expect("tick range");
        }
        assert_eq!(clock.tick(), Tick(30));
        assert_eq!(clock.push_elapsed(Duration::from_nanos(33_333_333)), 0);
        assert_eq!(clock.push_elapsed(Duration::from_nanos(1)), 1);
    }
}
