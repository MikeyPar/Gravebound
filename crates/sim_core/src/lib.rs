//! Renderer-independent deterministic simulation primitives.

/// Authoritative simulation frequency required by `TECH-070` and `GB-M00-05`.
pub const TICKS_PER_SECOND: u32 = 30;

/// Returns the crate's immutable diagnostic version.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
