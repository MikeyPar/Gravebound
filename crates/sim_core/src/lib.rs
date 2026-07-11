//! Renderer-independent deterministic simulation primitives.
//!
//! This crate is the sole owner of authoritative time, entity allocation, random streams, and
//! canonical foundation-state hashing. It intentionally has no Bevy or platform dependency.

mod arena;
mod clock;
mod collision;
mod combat;
mod entity;
mod movement;
mod rng;
mod trace;
mod weapon;

pub use arena::{
    ArenaAnchor, ArenaGeometry, ArenaGeometryError, MILLI_TILES_PER_TILE, TilePoint, TileRectangle,
};
pub use clock::{
    FixedStepClock, TICK_RATE_HZ, Tick, duration_ms_to_ticks_ceil, duration_ms_to_ticks_nearest,
};
pub use collision::{
    CollisionError, CollisionTarget, EnemyHurtbox, HurtboxError, ProjectileCollisionWorld,
    ShellSide, SolidColliderId, SweepHit,
};
pub use combat::{
    AimDirection, AimDirectionError, CombatAction, CombatError, CombatStep, FriendlyProjectile,
    PlayerCombatState, ProjectileCollision, ProjectileExpired, ShotEvent,
};
pub use entity::{EntityId, EntityIdAllocator};
pub use movement::{
    GRAVE_ARBALIST_SPEED_TILES_PER_SECOND, MOVEMENT_RESPONSE_TICKS, MovementAction, MovementError,
    MovementStep, PLAYER_COLLISION_RADIUS_TILES, PlayerMovementConfig, PlayerMovementState,
    SimulationVector, tile_point_to_simulation,
};
pub use rng::{DeterministicRng, RngError, derive_stream_seed};
pub use trace::{
    FoundationEntity, FoundationSimulation, InputFrame, TickHash, TraceError, TraceFixture,
    TraceReport, run_trace,
};
pub use weapon::{WeaponDefinition, WeaponDefinitionError, WeaponDefinitionParameters};

/// Authoritative simulation frequency required by `TECH-070` and `GB-M00-05`.
pub const TICKS_PER_SECOND: u32 = TICK_RATE_HZ;

/// Returns the crate's immutable diagnostic version.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
