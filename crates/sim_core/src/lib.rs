//! Renderer-independent deterministic simulation primitives.
//!
//! This crate is the sole owner of authoritative time, entity allocation, random streams, and
//! canonical foundation-state hashing. It intentionally has no Bevy or platform dependency.

mod clock;
mod entity;
mod rng;
mod trace;

pub use clock::{FixedStepClock, TICK_RATE_HZ, Tick, duration_ms_to_ticks_ceil};
pub use entity::{EntityId, EntityIdAllocator};
pub use rng::{DeterministicRng, RngError, derive_stream_seed};
pub use trace::{
    FoundationEntity, FoundationSimulation, InputFrame, TickHash, TraceError, TraceFixture,
    TraceReport, run_trace,
};

/// Authoritative simulation frequency required by `TECH-070` and `GB-M00-05`.
pub const TICKS_PER_SECOND: u32 = TICK_RATE_HZ;

/// Returns the crate's immutable diagnostic version.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
