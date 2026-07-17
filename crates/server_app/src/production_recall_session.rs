//! Generation-safe transport ownership for production Emergency Recall.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` `TECH-015` and
//! `DTH-010`/`011`; `Gravebound_Content_Production_Spec_v1.md` Core danger-route
//! and Lantern Halls contracts; `Gravebound_Development_Roadmap_v1.md`
//! `GB-M03-03`/`08`; and accepted `SPEC-CONFLICT-029`.
//!
//! A QUIC connection is not gameplay authority. This lifecycle grants a monotonically
//! increasing transport generation to the current connection, starts `LinkLost` only when that
//! generation detaches, and ignores the delayed teardown of a transport replaced by an
//! authoritative handoff. Planned server shutdown retires transport ownership without inventing
//! a player disconnect terminal.

use std::sync::Arc;

use thiserror::Error;
use tokio::sync::Mutex;

use crate::{ProductionRecallChannelError, ProductionRecallClock, ProductionRecallIntentActor};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProductionRecallTransportGeneration(u64);

impl ProductionRecallTransportGeneration {
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductionRecallAttachDisposition {
    Fresh,
    Reattached,
    AuthoritativeHandoff,
    TerminalPending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductionRecallTransportLease {
    generation: ProductionRecallTransportGeneration,
    invalidated_generation: Option<ProductionRecallTransportGeneration>,
    disposition: ProductionRecallAttachDisposition,
}

impl ProductionRecallTransportLease {
    #[must_use]
    pub const fn generation(self) -> ProductionRecallTransportGeneration {
        self.generation
    }

    #[must_use]
    pub const fn invalidated_generation(self) -> Option<ProductionRecallTransportGeneration> {
        self.invalidated_generation
    }

    #[must_use]
    pub const fn disposition(self) -> ProductionRecallAttachDisposition {
        self.disposition
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductionRecallDetachOutcome {
    LinkLostStarted { deadline_tick: u64 },
    LinkLostAlreadyPending,
    StaleGenerationIgnored,
    PlannedShutdownIgnored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductionRecallSessionSnapshot {
    pub active_generation: Option<ProductionRecallTransportGeneration>,
    pub link_lost_pending: bool,
    pub retired: bool,
}

impl ProductionRecallSessionSnapshot {
    #[must_use]
    pub const fn has_zero_transport_residue(self) -> bool {
        self.retired && self.active_generation.is_none()
    }
}

#[derive(Debug, Error)]
pub enum ProductionRecallSessionError {
    #[error("production Recall session has retired")]
    Retired,
    #[error("production Recall transport generation overflowed")]
    GenerationExhausted,
    #[error("production Recall transport timing authority is invalid")]
    InvalidTimingAuthority,
    #[error("production Recall actor rejected transport lifecycle authority")]
    Actor(#[source] ProductionRecallChannelError),
}

#[derive(Debug)]
struct ProductionRecallSessionState {
    next_generation: u64,
    active_generation: Option<ProductionRecallTransportGeneration>,
    link_lost_pending: bool,
    retired: bool,
}

/// Shared session boundary owned beside one live character actor.
///
/// The lifecycle mutex remains held while the actor accepts a loss or reconnect transition. This
/// gives transport replacement and delayed connection teardown one linear order without moving
/// terminal authority into the network task.
#[derive(Debug)]
pub struct ProductionRecallSessionLifecycle<Clock> {
    actor: Arc<ProductionRecallIntentActor<Clock>>,
    state: Mutex<ProductionRecallSessionState>,
}

impl<Clock> ProductionRecallSessionLifecycle<Clock>
where
    Clock: ProductionRecallClock,
{
    #[must_use]
    pub fn new(actor: Arc<ProductionRecallIntentActor<Clock>>) -> Self {
        Self {
            actor,
            state: Mutex::new(ProductionRecallSessionState {
                next_generation: 1,
                active_generation: None,
                link_lost_pending: false,
                retired: false,
            }),
        }
    }

    #[must_use]
    pub fn actor(&self) -> &Arc<ProductionRecallIntentActor<Clock>> {
        &self.actor
    }

    /// Attaches a newly accepted transport. An existing generation is invalidated only by the
    /// returned committed lease; its later detach is therefore stale and harmless.
    pub async fn attach_transport(
        &self,
        authoritative_tick: u64,
    ) -> Result<ProductionRecallTransportLease, ProductionRecallSessionError> {
        if authoritative_tick == 0 {
            return Err(ProductionRecallSessionError::InvalidTimingAuthority);
        }
        let mut state = self.state.lock().await;
        if state.retired {
            return Err(ProductionRecallSessionError::Retired);
        }
        let generation = ProductionRecallTransportGeneration(state.next_generation);
        state.next_generation = state
            .next_generation
            .checked_add(1)
            .ok_or(ProductionRecallSessionError::GenerationExhausted)?;

        let invalidated_generation = state.active_generation;
        let disposition = if invalidated_generation.is_some() {
            ProductionRecallAttachDisposition::AuthoritativeHandoff
        } else if state.link_lost_pending {
            match self
                .actor
                .reconnect_before_link_lost_deadline(authoritative_tick)
                .await
            {
                Ok(()) => {
                    state.link_lost_pending = false;
                    ProductionRecallAttachDisposition::Reattached
                }
                Err(
                    ProductionRecallChannelError::LinkLostDeadlineElapsed { .. }
                    | ProductionRecallChannelError::TerminalTickPinned { .. },
                ) => ProductionRecallAttachDisposition::TerminalPending,
                Err(error) => return Err(ProductionRecallSessionError::Actor(error)),
            }
        } else {
            ProductionRecallAttachDisposition::Fresh
        };
        state.active_generation = Some(generation);
        Ok(ProductionRecallTransportLease {
            generation,
            invalidated_generation,
            disposition,
        })
    }

    /// Detaches only the currently authoritative generation. The automatic Recall deadline is
    /// derived from the exact actor tick and cannot be moved by retries or stale transports.
    pub async fn detach_transport(
        &self,
        generation: ProductionRecallTransportGeneration,
        lost_tick: u64,
        issued_at_unix_ms: u64,
    ) -> Result<ProductionRecallDetachOutcome, ProductionRecallSessionError> {
        if lost_tick == 0 || issued_at_unix_ms == 0 {
            return Err(ProductionRecallSessionError::InvalidTimingAuthority);
        }
        let mut state = self.state.lock().await;
        if state.retired {
            return Ok(ProductionRecallDetachOutcome::PlannedShutdownIgnored);
        }
        if state.active_generation != Some(generation) {
            return Ok(ProductionRecallDetachOutcome::StaleGenerationIgnored);
        }
        if state.link_lost_pending {
            state.active_generation = None;
            return Ok(ProductionRecallDetachOutcome::LinkLostAlreadyPending);
        }
        let deadline_tick = lost_tick
            .checked_add(persistence::PRODUCTION_RECALL_LINK_LOST_TICKS)
            .ok_or(ProductionRecallSessionError::InvalidTimingAuthority)?;
        self.actor
            .enter_link_lost(lost_tick, issued_at_unix_ms)
            .await
            .map_err(ProductionRecallSessionError::Actor)?;
        state.active_generation = None;
        state.link_lost_pending = true;
        Ok(ProductionRecallDetachOutcome::LinkLostStarted { deadline_tick })
    }

    /// Retires transport ownership for a planned process shutdown. Shutdown is not a legitimate
    /// death or Recall source, so this deliberately does not enter `LinkLost`.
    pub async fn retire_for_shutdown(&self) -> ProductionRecallSessionSnapshot {
        let mut state = self.state.lock().await;
        state.retired = true;
        state.active_generation = None;
        ProductionRecallSessionSnapshot {
            active_generation: state.active_generation,
            link_lost_pending: state.link_lost_pending,
            retired: state.retired,
        }
    }

    #[must_use]
    pub async fn snapshot(&self) -> ProductionRecallSessionSnapshot {
        let state = self.state.lock().await;
        ProductionRecallSessionSnapshot {
            active_generation: state.active_generation,
            link_lost_pending: state.link_lost_pending,
            retired: state.retired,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProductionRecallPendingAuthorityV1;

    #[derive(Debug, Clone, Copy)]
    struct FixedClock;

    impl ProductionRecallClock for FixedClock {
        fn unix_millis(&self) -> u64 {
            1_000
        }
    }

    fn lifecycle() -> ProductionRecallSessionLifecycle<FixedClock> {
        ProductionRecallSessionLifecycle::new(Arc::new(
            ProductionRecallIntentActor::new(
                FixedClock,
                [1; 16],
                [2; 16],
                ProductionRecallPendingAuthorityV1 {
                    pending_item_count: 0,
                    pending_material_stack_count: 0,
                },
            )
            .unwrap(),
        ))
    }

    #[tokio::test]
    async fn authoritative_handoff_makes_delayed_old_detach_harmless() {
        let lifecycle = lifecycle();
        let first = lifecycle.attach_transport(100).await.unwrap();
        let second = lifecycle.attach_transport(101).await.unwrap();
        assert_eq!(
            second.disposition(),
            ProductionRecallAttachDisposition::AuthoritativeHandoff
        );
        assert_eq!(second.invalidated_generation(), Some(first.generation()));
        assert_eq!(
            lifecycle
                .detach_transport(first.generation(), 102, 2_000)
                .await
                .unwrap(),
            ProductionRecallDetachOutcome::StaleGenerationIgnored
        );
        assert_eq!(
            lifecycle.snapshot().await.active_generation,
            Some(second.generation())
        );
        assert_eq!(lifecycle.actor().pinned_terminal_tick().await, None);
    }

    #[tokio::test]
    async fn current_detach_starts_one_exact_link_lost_window_and_early_reconnect_clears_it() {
        let lifecycle = lifecycle();
        let first = lifecycle.attach_transport(200).await.unwrap();
        assert_eq!(
            lifecycle
                .detach_transport(first.generation(), 200, 3_000)
                .await
                .unwrap(),
            ProductionRecallDetachOutcome::LinkLostStarted { deadline_tick: 290 }
        );
        assert!(lifecycle.snapshot().await.link_lost_pending);

        let reattached = lifecycle.attach_transport(289).await.unwrap();
        assert_eq!(
            reattached.disposition(),
            ProductionRecallAttachDisposition::Reattached
        );
        assert!(!lifecycle.snapshot().await.link_lost_pending);
        assert!(matches!(
            lifecycle
                .actor()
                .reconnect_before_link_lost_deadline(289)
                .await,
            Err(ProductionRecallChannelError::LinkLostNotActive)
        ));
    }

    #[tokio::test]
    async fn deadline_reconnect_preserves_terminal_and_shutdown_leaves_no_transport_residue() {
        let lifecycle = lifecycle();
        let first = lifecycle.attach_transport(200).await.unwrap();
        lifecycle
            .detach_transport(first.generation(), 200, 3_000)
            .await
            .unwrap();

        let late = lifecycle.attach_transport(290).await.unwrap();
        assert_eq!(
            late.disposition(),
            ProductionRecallAttachDisposition::TerminalPending
        );
        assert!(lifecycle.snapshot().await.link_lost_pending);
        let retired = lifecycle.retire_for_shutdown().await;
        assert!(retired.has_zero_transport_residue());
        assert_eq!(
            lifecycle
                .detach_transport(late.generation(), 291, 4_000)
                .await
                .unwrap(),
            ProductionRecallDetachOutcome::PlannedShutdownIgnored
        );
        assert!(matches!(
            lifecycle.attach_transport(291).await,
            Err(ProductionRecallSessionError::Retired)
        ));
    }
}
