//! Route-generation-bound tick authority for the normal Core private-life graph.
//!
//! The canonical GDD fixes one authoritative 30 Hz simulation domain and lethal-first Recall
//! ordering (`SIM-004`, `DTH-010`, `TECH-012`, `TECH-015`). The Content Production Specification
//! fixes duration conversion and the closed Core danger route (`CONT-010`, `CONT-WORLD-001`), and
//! the Development Roadmap requires production Recall/extraction behavior across reconnect
//! (`GB-M03-03`, `GB-M03-08`). This directory therefore exposes only ticks committed by the one
//! exclusive danger driver and binds every read to the exact private-route actor generation.

use std::{
    collections::BTreeMap,
    num::NonZeroU64,
    sync::{Mutex, MutexGuard},
};

use thiserror::Error;

use crate::{
    CoreExtractionAuthoritativeTick, CorePrivateMicrorealmBindingLease,
    CorePrivateMicrorealmDriverHandle, CorePrivateRouteActorLease, CoreRecallAuthoritativeTick,
};

pub trait CorePrivateLifeAuthoritativeTick: Send + Sync {
    fn current_tick(
        &self,
        route: CorePrivateRouteActorLease,
    ) -> Result<NonZeroU64, CorePrivateLifeTickError>;
}

#[derive(Debug, Clone)]
struct LiveTickBinding {
    binding: CorePrivateMicrorealmBindingLease,
    driver: CorePrivateMicrorealmDriverHandle,
}

#[derive(Debug)]
struct TickDirectoryState {
    accepting: bool,
    shutdown_started: bool,
    bindings: BTreeMap<[u8; 16], LiveTickBinding>,
}

#[derive(Debug)]
pub struct CorePrivateLifeTickDirectory {
    state: Mutex<TickDirectoryState>,
}

impl Default for CorePrivateLifeTickDirectory {
    fn default() -> Self {
        Self::new()
    }
}

impl CorePrivateLifeTickDirectory {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Mutex::new(TickDirectoryState {
                accepting: true,
                shutdown_started: false,
                bindings: BTreeMap::new(),
            }),
        }
    }

    pub(crate) fn bind(
        &self,
        binding: CorePrivateMicrorealmBindingLease,
        driver: CorePrivateMicrorealmDriverHandle,
    ) -> Result<(), CorePrivateLifeTickError> {
        let route = binding.route_lease();
        if binding.account_id() == [0; 16]
            || binding.character_id() == [0; 16]
            || binding.actor_generation() == 0
            || binding.binding_generation() == 0
            || route.account_id() != binding.account_id()
            || route.character_id() != binding.character_id()
            || route.actor_generation() != binding.actor_generation()
        {
            return Err(CorePrivateLifeTickError::InvalidBinding);
        }
        let mut state = lock(&self.state);
        if !state.accepting {
            return Err(CorePrivateLifeTickError::Retired);
        }
        if let Some(existing) = state.bindings.get(&binding.account_id()) {
            if existing.binding == binding && existing.driver.shares_driver_with(&driver) {
                return Ok(());
            }
            return Err(CorePrivateLifeTickError::BindingConflict);
        }
        state
            .bindings
            .insert(binding.account_id(), LiveTickBinding { binding, driver });
        Ok(())
    }

    pub(crate) fn unbind(
        &self,
        binding: CorePrivateMicrorealmBindingLease,
    ) -> Result<(), CorePrivateLifeTickError> {
        let mut state = lock(&self.state);
        let existing = state
            .bindings
            .get(&binding.account_id())
            .ok_or(CorePrivateLifeTickError::Unbound)?;
        if existing.binding != binding {
            return Err(CorePrivateLifeTickError::RouteGenerationMismatch);
        }
        state.bindings.remove(&binding.account_id());
        Ok(())
    }

    pub fn begin_shutdown(&self) {
        let mut state = lock(&self.state);
        state.accepting = false;
        state.shutdown_started = true;
    }

    pub fn finish_shutdown(
        &self,
    ) -> Result<CorePrivateLifeTickDirectoryReport, CorePrivateLifeTickError> {
        let state = lock(&self.state);
        if !state.shutdown_started {
            return Err(CorePrivateLifeTickError::ShutdownNotStarted);
        }
        let remaining_bindings = state.bindings.len();
        Ok(CorePrivateLifeTickDirectoryReport {
            remaining_bindings,
            zero_residue: remaining_bindings == 0,
        })
    }
}

impl CorePrivateLifeAuthoritativeTick for CorePrivateLifeTickDirectory {
    fn current_tick(
        &self,
        route: CorePrivateRouteActorLease,
    ) -> Result<NonZeroU64, CorePrivateLifeTickError> {
        let state = lock(&self.state);
        let binding = state
            .bindings
            .get(&route.account_id())
            .ok_or(CorePrivateLifeTickError::Unbound)?;
        if binding.binding.route_lease() != route {
            return Err(CorePrivateLifeTickError::RouteGenerationMismatch);
        }
        binding
            .driver
            .authoritative_tick()
            .ok_or(CorePrivateLifeTickError::AwaitingFirstCommittedFrame)
    }
}

impl CoreRecallAuthoritativeTick for CorePrivateLifeTickDirectory {
    fn current_tick(&self, route: CorePrivateRouteActorLease) -> Option<NonZeroU64> {
        CorePrivateLifeAuthoritativeTick::current_tick(self, route).ok()
    }
}

impl CoreExtractionAuthoritativeTick for CorePrivateLifeTickDirectory {
    fn current_tick(&self, route: CorePrivateRouteActorLease) -> Option<NonZeroU64> {
        let acknowledged = CorePrivateLifeAuthoritativeTick::current_tick(self, route).ok()?;
        // One frame may already be awaiting terminal-owner acknowledgement. Reserve the following
        // tick so an accepted extraction can never race an absence already in that barrier.
        NonZeroU64::new(acknowledged.get().checked_add(2)?)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateLifeTickDirectoryReport {
    pub remaining_bindings: usize,
    pub zero_residue: bool,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum CorePrivateLifeTickError {
    #[error("private-life tick binding is invalid")]
    InvalidBinding,
    #[error("private-life tick binding conflicts with a live owner")]
    BindingConflict,
    #[error("private-life tick authority is not bound")]
    Unbound,
    #[error("private-life tick route generation is stale or foreign")]
    RouteGenerationMismatch,
    #[error("private-life danger driver has not committed its first frame")]
    AwaitingFirstCommittedFrame,
    #[error("private-life tick directory is retired")]
    Retired,
    #[error("private-life tick-directory shutdown has not started")]
    ShutdownNotStarted,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
