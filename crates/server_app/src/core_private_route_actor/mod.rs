//! Capacity-one authoritative actor graph for the ordinary M03 private-life route.
//!
//! The graph is constrained by all three design authorities:
//! `Gravebound_Production_GDD_v1_Canonical.md` owns the Hall, danger, terminal, and
//! server-authority rules; `Gravebound_Content_Production_Spec_v1.md` owns the exact Core
//! micro-realm and `B0 -> B6` Bell route; and `Gravebound_Development_Roadmap_v1.md` owns the
//! `GB-M03-03` complete-private-life exit gate. Constructing this actor does not advertise the
//! normal route. The dedicated composition root remains responsible for that later gate.

mod directory;
mod state;

#[cfg(test)]
mod tests;

pub use directory::{
    CORE_PRIVATE_ROUTE_ACTOR_MAILBOX_CAPACITY, CorePrivateRouteActorDirectory,
    CorePrivateRouteActorLease, CorePrivateRouteBellPermitLease, CorePrivateRouteExtractionBinding,
    CorePrivateRouteExtractionExitBinding, CorePrivateRouteExtractionPermit,
    CorePrivateRouteRuntimeError, CorePrivateRouteRuntimeReport,
};
pub use state::{
    CorePrivateRouteActor, CorePrivateRouteActorAdvance, CorePrivateRouteActorError,
    CorePrivateRouteActorPosition, CorePrivateRouteActorSeed,
};
