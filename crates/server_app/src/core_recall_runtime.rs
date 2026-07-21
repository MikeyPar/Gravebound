//! Bounded actor and transport runtime for production Emergency Recall.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` `TECH-015`, `TECH-021`-`023`, and
//! `DTH-010`/`011`; `Gravebound_Content_Production_Spec_v1.md` Core danger-route and Lantern
//! Halls contracts; `Gravebound_Development_Roadmap_v1.md` `GB-M03-03`/`08`; and accepted
//! `SPEC-CONFLICT-029`.
//!
//! The normal Core endpoint remains disabled until its parent route gate passes. This runtime is
//! the production-shaped injection seam: one bounded actor mailbox per registered selected
//! character, one authoritative transport generation per account, and explicit shutdown that
//! closes actor inboxes before connection workers can wait forever on a Recall reply.

use std::{collections::BTreeMap, future::Future, num::NonZeroU64, sync::Arc};

use protocol::{
    RecallFrameV1, RecallResultV1, TERMINAL_INVENTORY_SCHEMA_VERSION,
    TerminalInventoryRejectionCodeV1,
};
use thiserror::Error;
use tokio::{
    sync::{Mutex, oneshot},
    task::JoinHandle,
};

use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CorePrivateRouteActorLease,
    CoreRecallActorHandle, CoreRecallActorInbox, CoreRecallCompletionInbox,
    CoreRecallCompletionOutbox, CoreRecallIntentAuthority, CoreRecallIntentReply,
    CoreRecallReliableWriter, ProductionRecallClock, ProductionRecallDetachOutcome,
    ProductionRecallIntentActor, ProductionRecallPublishedV1, ProductionRecallSessionError,
    ProductionRecallSessionLifecycle, ProductionRecallTransportGeneration,
    TRANSPORT_REPLACED_CLOSE_CODE, core_recall_completion_outbox, production_recall_actor_mailbox,
    send_recall_publication,
};

pub trait CoreRecallAuthoritativeTick: Send + Sync {
    fn current_tick(&self, route: CorePrivateRouteActorLease) -> Option<NonZeroU64>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreRecallConnectionLease {
    account_id: [u8; 16],
    character_id: [u8; 16],
    generation: ProductionRecallTransportGeneration,
}

impl CoreRecallConnectionLease {
    #[must_use]
    pub const fn account_id(self) -> [u8; 16] {
        self.account_id
    }

    #[must_use]
    pub const fn character_id(self) -> [u8; 16] {
        self.character_id
    }

    #[must_use]
    pub const fn generation(self) -> ProductionRecallTransportGeneration {
        self.generation
    }
}

#[derive(Debug)]
pub struct CoreRecallTransportAttach {
    pub lease: CoreRecallConnectionLease,
    pub invalidated_connection: Option<quinn::Connection>,
}

/// Opaque reservation for a coordinated private-life writer handoff.
///
/// Preparing reserves a monotonically increasing handoff generation, but deliberately does not
/// change the active Recall transport generation. The private-life session owner may therefore
/// prepare every dynamic runtime against one writer before committing any of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CoreRecallPreparedWriterHandoff {
    account_id: [u8; 16],
    character_id: [u8; 16],
    handoff_generation: u64,
}

#[derive(Debug, Clone)]
pub struct CoreRecallActorRegistration {
    pub completion_outbox: CoreRecallCompletionOutbox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreRecallActorRetirementReport {
    pub served_actor_commands: u64,
    pub abandoned_actor_commands: u64,
    pub delivered_completion_publications: u64,
    pub undelivered_completion_publications: u64,
    pub abandoned_completion_publications: u64,
    pub detached_transport_binding: bool,
    pub zero_residue: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreRecallRuntimeReport {
    pub served_actor_commands: u64,
    pub abandoned_actor_commands: u64,
    pub delivered_completion_publications: u64,
    pub undelivered_completion_publications: u64,
    pub abandoned_completion_publications: u64,
    pub remaining_actor_tasks: usize,
    pub remaining_completion_tasks: usize,
    pub remaining_registered_actors: usize,
    pub remaining_active_transports: usize,
    pub retired_pending_writer_handoffs: usize,
    pub zero_residue: bool,
}

#[derive(Debug, Error)]
pub enum CoreRecallRuntimeError {
    #[error("Core Recall runtime is not accepting actors or transports")]
    Retired,
    #[error("Core Recall actor binding is invalid")]
    InvalidActorBinding,
    #[error("Core Recall actor is already registered for this account")]
    ActorAlreadyRegistered,
    #[error("Core Recall actor is not registered for this account")]
    ActorUnavailable,
    #[error("Core Recall reliable writer is unavailable")]
    ReliableWriterUnavailable,
    #[error("Core Recall reliable writer is already attached")]
    ReliableWriterAlreadyAttached,
    #[error("Core Recall prepared reliable-writer handoff is stale or invalid")]
    PreparedWriterHandoffMismatch,
    #[error("Core Recall authoritative simulation tick is unavailable")]
    AuthoritativeTickUnavailable,
    #[error("Core Recall reliable-writer handoff generation overflowed")]
    WriterHandoffGenerationExhausted,
    #[error("Core Recall runtime shutdown has not started")]
    ShutdownNotStarted,
    #[error("Core Recall actor task failed")]
    ActorTaskFailed(#[source] tokio::task::JoinError),
    #[error("Core Recall session lifecycle rejected transport authority")]
    Session(#[from] ProductionRecallSessionError),
}

struct CoreRecallActorEntry<Clock> {
    authenticated: AuthenticatedAccount,
    character_id: [u8; 16],
    route_lease: CorePrivateRouteActorLease,
    lifecycle: Arc<ProductionRecallSessionLifecycle<Clock>>,
    handle: CoreRecallActorHandle,
    shutdown: Option<oneshot::Sender<()>>,
    actor_task: Option<JoinHandle<CoreRecallActorTaskReport>>,
    completion_shutdown: Option<oneshot::Sender<()>>,
    completion_task: Option<JoinHandle<CoreRecallCompletionTaskReport>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CoreRecallActorTaskReport {
    served: u64,
    abandoned: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CoreRecallCompletionTaskReport {
    delivered: u64,
    undelivered: u64,
    abandoned: u64,
}

#[derive(Debug)]
struct ActiveRecallTransport {
    lease: CoreRecallConnectionLease,
    writer: Arc<CoreRecallReliableWriter>,
    handoff_generation: u64,
}

#[derive(Debug)]
struct PendingRecallWriterHandoff {
    prepared: CoreRecallPreparedWriterHandoff,
    authenticated: AuthenticatedAccount,
    writer: Arc<CoreRecallReliableWriter>,
}

struct CoreRecallRuntimeState<Clock> {
    accepting: bool,
    shutdown_started: bool,
    actors: BTreeMap<[u8; 16], CoreRecallActorEntry<Clock>>,
    transports: BTreeMap<[u8; 16], ActiveRecallTransport>,
    pending_writer_handoffs: BTreeMap<[u8; 16], PendingRecallWriterHandoff>,
    next_writer_handoff_generation: BTreeMap<[u8; 16], u64>,
    retired_pending_writer_handoffs: usize,
}

pub struct CoreRecallActorDirectory<Clock, TickSource> {
    tick_source: Arc<TickSource>,
    state: Mutex<CoreRecallRuntimeState<Clock>>,
}

impl<Clock, TickSource> CoreRecallActorDirectory<Clock, TickSource>
where
    Clock: ProductionRecallClock + 'static,
    TickSource: CoreRecallAuthoritativeTick + 'static,
{
    #[must_use]
    pub fn new(tick_source: Arc<TickSource>) -> Self {
        Self {
            tick_source,
            state: Mutex::new(CoreRecallRuntimeState {
                accepting: true,
                shutdown_started: false,
                actors: BTreeMap::new(),
                transports: BTreeMap::new(),
                pending_writer_handoffs: BTreeMap::new(),
                next_writer_handoff_generation: BTreeMap::new(),
                retired_pending_writer_handoffs: 0,
            }),
        }
    }

    pub async fn register_actor(
        self: &Arc<Self>,
        authenticated: AuthenticatedAccount,
        route_lease: CorePrivateRouteActorLease,
        actor: Arc<ProductionRecallIntentActor<Clock>>,
    ) -> Result<CoreRecallActorRegistration, CoreRecallRuntimeError> {
        if authenticated.namespace != AuthenticatedNamespace::WipeableTest
            || authenticated.account_id.as_bytes() != actor.account_id()
            || route_lease.account_id() != actor.account_id()
            || route_lease.character_id() != actor.character_id()
            || route_lease.actor_generation() == 0
        {
            return Err(CoreRecallRuntimeError::InvalidActorBinding);
        }
        let account_id = actor.account_id();
        let character_id = actor.character_id();
        let mut state = self.state.lock().await;
        if !state.accepting {
            return Err(CoreRecallRuntimeError::Retired);
        }
        if state.actors.contains_key(&account_id) {
            return Err(CoreRecallRuntimeError::ActorAlreadyRegistered);
        }
        let (handle, inbox) = production_recall_actor_mailbox();
        let (completion_outbox, completion_inbox) = core_recall_completion_outbox();
        let (shutdown_send, shutdown_receive) = oneshot::channel();
        let (completion_shutdown_send, completion_shutdown_receive) = oneshot::channel();
        let tick_source = Arc::clone(&self.tick_source);
        let task_actor = Arc::clone(&actor);
        let actor_task = tokio::spawn(serve_actor_mailbox(
            inbox,
            task_actor,
            tick_source,
            route_lease,
            shutdown_receive,
        ));
        let completion_task = tokio::spawn(serve_completion_outbox(
            completion_inbox,
            Arc::downgrade(self),
            account_id,
            completion_shutdown_receive,
        ));
        state.actors.insert(
            account_id,
            CoreRecallActorEntry {
                authenticated,
                character_id,
                route_lease,
                lifecycle: Arc::new(ProductionRecallSessionLifecycle::new(actor)),
                handle,
                shutdown: Some(shutdown_send),
                actor_task: Some(actor_task),
                completion_shutdown: Some(completion_shutdown_send),
                completion_task: Some(completion_task),
            },
        );
        Ok(CoreRecallActorRegistration { completion_outbox })
    }

    /// Returns the exact route-bound Recall projection consumed by the simulation driver. This
    /// is read-only actor authority: transport code cannot publish channel state or substitute a
    /// different character generation.
    pub(crate) async fn terminal_authorities(
        &self,
        authenticated: AuthenticatedAccount,
        route_lease: CorePrivateRouteActorLease,
    ) -> Result<
        (
            crate::CorePrivateRecallTerminalHandle,
            tokio::sync::watch::Receiver<crate::ProductionRecallLiveProjectionV1>,
        ),
        CoreRecallRuntimeError,
    > {
        let state = self.state.lock().await;
        if !state.accepting {
            return Err(CoreRecallRuntimeError::Retired);
        }
        let entry = state
            .actors
            .get(&authenticated.account_id.as_bytes())
            .ok_or(CoreRecallRuntimeError::ActorUnavailable)?;
        if entry.authenticated != authenticated
            || entry.character_id != route_lease.character_id()
            || entry.route_lease != route_lease
        {
            return Err(CoreRecallRuntimeError::InvalidActorBinding);
        }
        Ok((
            crate::CorePrivateRecallTerminalHandle::new(Arc::clone(entry.lifecycle.actor())),
            entry.lifecycle.actor().subscribe_live_projection(),
        ))
    }

    /// Installs the new generation before returning the connection that it superseded. The caller
    /// may therefore close the old transport only after the authoritative handoff has committed.
    pub async fn attach_transport(
        &self,
        authenticated: AuthenticatedAccount,
        connection: quinn::Connection,
    ) -> Result<CoreRecallTransportAttach, CoreRecallRuntimeError> {
        self.attach_reliable_writer(
            authenticated,
            Arc::new(CoreRecallReliableWriter::new(connection)),
        )
        .await
    }

    /// Binds Recall authority to the reliable writer already owned by the account session.
    /// Identity responses, route projections, extraction results, and Recall publications must
    /// use this same sequence space. Passing an already attached or retired writer fails before
    /// allocating a new Recall transport generation.
    pub async fn attach_reliable_writer(
        &self,
        authenticated: AuthenticatedAccount,
        writer: Arc<CoreRecallReliableWriter>,
    ) -> Result<CoreRecallTransportAttach, CoreRecallRuntimeError> {
        let prepared = self
            .prepare_reliable_writer_handoff(authenticated, writer)
            .await?;
        self.commit_prepared_reliable_writer_handoff_inner(prepared, true)
            .await
    }

    /// Reserves Recall's side of a coordinated session-writer handoff without changing visible
    /// transport authority. The exact returned token must be committed or aborted.
    pub(crate) async fn prepare_reliable_writer_handoff(
        &self,
        authenticated: AuthenticatedAccount,
        writer: Arc<CoreRecallReliableWriter>,
    ) -> Result<CoreRecallPreparedWriterHandoff, CoreRecallRuntimeError> {
        if !writer.is_available() {
            return Err(CoreRecallRuntimeError::ReliableWriterUnavailable);
        }
        let account_id = authenticated.account_id.as_bytes();
        let mut state = self.state.lock().await;
        if !state.accepting {
            return Err(CoreRecallRuntimeError::Retired);
        }
        if let Some(pending) = state.pending_writer_handoffs.get(&account_id)
            && pending.authenticated == authenticated
            && Arc::ptr_eq(&pending.writer, &writer)
        {
            return Ok(pending.prepared);
        }
        if state
            .transports
            .values()
            .any(|active| Arc::ptr_eq(&active.writer, &writer))
            || state
                .pending_writer_handoffs
                .iter()
                .any(|(pending_account, pending)| {
                    *pending_account != account_id && Arc::ptr_eq(&pending.writer, &writer)
                })
        {
            return Err(CoreRecallRuntimeError::ReliableWriterAlreadyAttached);
        }
        let entry = state
            .actors
            .get(&account_id)
            .ok_or(CoreRecallRuntimeError::ActorUnavailable)?;
        if entry.authenticated != authenticated {
            return Err(CoreRecallRuntimeError::InvalidActorBinding);
        }
        let character_id = entry.character_id;
        let generation = *state
            .next_writer_handoff_generation
            .entry(account_id)
            .or_insert(1);
        let after = generation
            .checked_add(1)
            .ok_or(CoreRecallRuntimeError::WriterHandoffGenerationExhausted)?;
        state
            .next_writer_handoff_generation
            .insert(account_id, after);
        let prepared = CoreRecallPreparedWriterHandoff {
            account_id,
            character_id,
            handoff_generation: generation,
        };
        state.pending_writer_handoffs.insert(
            account_id,
            PendingRecallWriterHandoff {
                prepared,
                authenticated,
                writer,
            },
        );
        Ok(prepared)
    }

    /// Commits only the exact prepared reservation. Neither the superseded writer nor the newly
    /// shared writer is retired; the central private-life session owns their lifecycle.
    #[allow(
        dead_code,
        reason = "consumed by the private-life session composition slice"
    )]
    pub(crate) async fn commit_prepared_reliable_writer_handoff(
        &self,
        prepared: CoreRecallPreparedWriterHandoff,
    ) -> Result<CoreRecallTransportAttach, CoreRecallRuntimeError> {
        self.commit_prepared_reliable_writer_handoff_inner(prepared, false)
            .await
    }

    async fn commit_prepared_reliable_writer_handoff_inner(
        &self,
        prepared: CoreRecallPreparedWriterHandoff,
        retire_invalidated_writer: bool,
    ) -> Result<CoreRecallTransportAttach, CoreRecallRuntimeError> {
        let account_id = prepared.account_id;
        let mut state = self.state.lock().await;
        if !state.accepting {
            return Err(CoreRecallRuntimeError::Retired);
        }
        if let Some(active) = state.transports.get(&account_id)
            && active.handoff_generation == prepared.handoff_generation
            && active.lease.character_id == prepared.character_id
        {
            return Ok(CoreRecallTransportAttach {
                lease: active.lease,
                invalidated_connection: None,
            });
        }
        let pending = state
            .pending_writer_handoffs
            .get(&account_id)
            .ok_or(CoreRecallRuntimeError::PreparedWriterHandoffMismatch)?;
        if pending.prepared != prepared {
            return Err(CoreRecallRuntimeError::PreparedWriterHandoffMismatch);
        }
        if !pending.writer.is_available() {
            return Err(CoreRecallRuntimeError::ReliableWriterUnavailable);
        }
        let entry = state
            .actors
            .get(&account_id)
            .ok_or(CoreRecallRuntimeError::ActorUnavailable)?;
        if entry.authenticated != pending.authenticated
            || entry.character_id != prepared.character_id
        {
            return Err(CoreRecallRuntimeError::InvalidActorBinding);
        }
        let authenticated = pending.authenticated;
        let writer = Arc::clone(&pending.writer);
        let tick = self
            .tick_source
            .current_tick(entry.route_lease)
            .ok_or(CoreRecallRuntimeError::AuthoritativeTickUnavailable)?
            .get();
        let transport_lease = entry.lifecycle.attach_transport(tick).await?;
        let lease = CoreRecallConnectionLease {
            account_id,
            character_id: entry.character_id,
            generation: transport_lease.generation(),
        };
        let removed = state
            .pending_writer_handoffs
            .remove(&account_id)
            .ok_or(CoreRecallRuntimeError::PreparedWriterHandoffMismatch)?;
        debug_assert_eq!(removed.authenticated, authenticated);
        let invalidated_connection = state
            .transports
            .insert(
                account_id,
                ActiveRecallTransport {
                    lease,
                    writer,
                    handoff_generation: prepared.handoff_generation,
                },
            )
            .map(|active| {
                if retire_invalidated_writer {
                    active.writer.retire(
                        TRANSPORT_REPLACED_CLOSE_CODE,
                        b"authoritative transport handoff",
                    );
                }
                active.writer.connection().clone()
            });
        Ok(CoreRecallTransportAttach {
            lease,
            invalidated_connection,
        })
    }

    /// Cancels only the exact pending reservation. The currently active binding remains intact;
    /// its reserved generation is intentionally never reused.
    #[allow(
        dead_code,
        reason = "consumed by the private-life session composition slice"
    )]
    pub(crate) async fn abort_prepared_reliable_writer_handoff(
        &self,
        prepared: CoreRecallPreparedWriterHandoff,
    ) -> Result<(), CoreRecallRuntimeError> {
        let mut state = self.state.lock().await;
        let pending = state
            .pending_writer_handoffs
            .get(&prepared.account_id)
            .ok_or(CoreRecallRuntimeError::PreparedWriterHandoffMismatch)?;
        if pending.prepared != prepared {
            return Err(CoreRecallRuntimeError::PreparedWriterHandoffMismatch);
        }
        state.pending_writer_handoffs.remove(&prepared.account_id);
        Ok(())
    }

    #[must_use]
    pub fn authority(
        self: &Arc<Self>,
        lease: CoreRecallConnectionLease,
    ) -> CoreRecallConnectionAuthority<Clock, TickSource> {
        CoreRecallConnectionAuthority {
            directory: Arc::clone(self),
            lease,
        }
    }

    pub async fn reliable_writer(
        &self,
        lease: CoreRecallConnectionLease,
    ) -> Result<Arc<CoreRecallReliableWriter>, CoreRecallRuntimeError> {
        let state = self.state.lock().await;
        let active = state
            .transports
            .get(&lease.account_id)
            .ok_or(CoreRecallRuntimeError::ActorUnavailable)?;
        if active.lease != lease {
            return Err(CoreRecallRuntimeError::ActorUnavailable);
        }
        Ok(Arc::clone(&active.writer))
    }

    pub async fn detach_transport(
        &self,
        lease: CoreRecallConnectionLease,
        issued_at_unix_ms: u64,
    ) -> Result<ProductionRecallDetachOutcome, CoreRecallRuntimeError> {
        let mut state = self.state.lock().await;
        if state.shutdown_started {
            return Ok(ProductionRecallDetachOutcome::PlannedShutdownIgnored);
        }
        let Some(active) = state.transports.get(&lease.account_id) else {
            return Ok(ProductionRecallDetachOutcome::StaleGenerationIgnored);
        };
        if active.lease != lease {
            return Ok(ProductionRecallDetachOutcome::StaleGenerationIgnored);
        }
        let entry = state
            .actors
            .get(&lease.account_id)
            .ok_or(CoreRecallRuntimeError::ActorUnavailable)?;
        let lost_tick = self
            .tick_source
            .current_tick(entry.route_lease)
            .ok_or(CoreRecallRuntimeError::AuthoritativeTickUnavailable)?
            .get();
        let outcome = entry
            .lifecycle
            .detach_transport(lease.generation, lost_tick, issued_at_unix_ms)
            .await?;
        state.transports.remove(&lease.account_id);
        Ok(outcome)
    }

    /// Retires one danger actor after its terminal transition has completed. The shared writer is
    /// only detached from Recall; it remains owned by the private-life session for the resulting
    /// Hall or Character Select projection.
    pub async fn retire_actor(
        &self,
        authenticated: AuthenticatedAccount,
    ) -> Result<CoreRecallActorRetirementReport, CoreRecallRuntimeError> {
        let account_id = authenticated.account_id.as_bytes();
        let (mut entry, detached_transport_binding) = {
            let mut state = self.state.lock().await;
            if !state.accepting {
                return Err(CoreRecallRuntimeError::Retired);
            }
            let entry = state
                .actors
                .remove(&account_id)
                .ok_or(CoreRecallRuntimeError::ActorUnavailable)?;
            if entry.authenticated != authenticated {
                state.actors.insert(account_id, entry);
                return Err(CoreRecallRuntimeError::InvalidActorBinding);
            }
            let detached_transport_binding = state.transports.remove(&account_id).is_some();
            state.pending_writer_handoffs.remove(&account_id);
            (entry, detached_transport_binding)
        };

        entry.lifecycle.retire_for_shutdown().await;
        if let Some(shutdown) = entry.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(shutdown) = entry.completion_shutdown.take() {
            let _ = shutdown.send(());
        }
        let actor = entry
            .actor_task
            .take()
            .ok_or(CoreRecallRuntimeError::ActorUnavailable)?
            .await
            .map_err(CoreRecallRuntimeError::ActorTaskFailed)?;
        let completion = entry
            .completion_task
            .take()
            .ok_or(CoreRecallRuntimeError::ActorUnavailable)?
            .await
            .map_err(CoreRecallRuntimeError::ActorTaskFailed)?;
        let directory_has_zero_residue = {
            let state = self.state.lock().await;
            !state.actors.contains_key(&account_id) && !state.transports.contains_key(&account_id)
        };
        Ok(CoreRecallActorRetirementReport {
            served_actor_commands: actor.served,
            abandoned_actor_commands: actor.abandoned,
            delivered_completion_publications: completion.delivered,
            undelivered_completion_publications: completion.undelivered,
            abandoned_completion_publications: completion.abandoned,
            detached_transport_binding,
            zero_residue: entry.actor_task.is_none()
                && entry.completion_task.is_none()
                && directory_has_zero_residue,
        })
    }

    /// Stops accepting work and closes actor inboxes before network workers are joined. Returned
    /// connections should be closed by the caller with the server-shutdown reason.
    pub async fn begin_shutdown(&self) -> Vec<quinn::Connection> {
        let mut state = self.state.lock().await;
        state.accepting = false;
        state.shutdown_started = true;
        let connections = std::mem::take(&mut state.transports)
            .into_values()
            .map(|active| active.writer.connection().clone())
            .collect();
        state.retired_pending_writer_handoffs = state
            .retired_pending_writer_handoffs
            .saturating_add(state.pending_writer_handoffs.len());
        state.pending_writer_handoffs.clear();
        for entry in state.actors.values() {
            entry.lifecycle.retire_for_shutdown().await;
        }
        for entry in state.actors.values_mut() {
            if let Some(shutdown) = entry.shutdown.take() {
                let _ = shutdown.send(());
            }
            if let Some(shutdown) = entry.completion_shutdown.take() {
                let _ = shutdown.send(());
            }
        }
        connections
    }

    pub async fn finish_shutdown(&self) -> Result<CoreRecallRuntimeReport, CoreRecallRuntimeError> {
        let (actor_tasks, completion_tasks) = {
            let mut state = self.state.lock().await;
            if !state.shutdown_started {
                return Err(CoreRecallRuntimeError::ShutdownNotStarted);
            }
            let actor_tasks = state
                .actors
                .values_mut()
                .filter_map(|entry| entry.actor_task.take())
                .collect::<Vec<_>>();
            let completion_tasks = state
                .actors
                .values_mut()
                .filter_map(|entry| entry.completion_task.take())
                .collect::<Vec<_>>();
            (actor_tasks, completion_tasks)
        };
        let mut served_actor_commands = 0_u64;
        let mut abandoned_actor_commands = 0_u64;
        for task in actor_tasks {
            let report = task
                .await
                .map_err(CoreRecallRuntimeError::ActorTaskFailed)?;
            served_actor_commands = served_actor_commands.saturating_add(report.served);
            abandoned_actor_commands = abandoned_actor_commands.saturating_add(report.abandoned);
        }
        let mut delivered_completion_publications = 0_u64;
        let mut undelivered_completion_publications = 0_u64;
        let mut abandoned_completion_publications = 0_u64;
        for task in completion_tasks {
            let report = task
                .await
                .map_err(CoreRecallRuntimeError::ActorTaskFailed)?;
            delivered_completion_publications =
                delivered_completion_publications.saturating_add(report.delivered);
            undelivered_completion_publications =
                undelivered_completion_publications.saturating_add(report.undelivered);
            abandoned_completion_publications =
                abandoned_completion_publications.saturating_add(report.abandoned);
        }
        let mut state = self.state.lock().await;
        let remaining_actor_tasks = state
            .actors
            .values()
            .filter(|entry| entry.actor_task.is_some())
            .count();
        let remaining_completion_tasks = state
            .actors
            .values()
            .filter(|entry| entry.completion_task.is_some())
            .count();
        state.actors.clear();
        let remaining_registered_actors = state.actors.len();
        let remaining_active_transports = state.transports.len();
        let retired_pending_writer_handoffs = state.retired_pending_writer_handoffs;
        state.pending_writer_handoffs.clear();
        state.next_writer_handoff_generation.clear();
        Ok(CoreRecallRuntimeReport {
            served_actor_commands,
            abandoned_actor_commands,
            delivered_completion_publications,
            undelivered_completion_publications,
            abandoned_completion_publications,
            remaining_actor_tasks,
            remaining_completion_tasks,
            remaining_registered_actors,
            remaining_active_transports,
            retired_pending_writer_handoffs,
            zero_residue: remaining_actor_tasks == 0
                && remaining_completion_tasks == 0
                && remaining_registered_actors == 0
                && remaining_active_transports == 0
                && state.pending_writer_handoffs.is_empty(),
        })
    }

    async fn handle_recall(
        &self,
        lease: CoreRecallConnectionLease,
        authenticated: AuthenticatedAccount,
        frame: &RecallFrameV1,
        fallback_server_tick: u64,
    ) -> CoreRecallIntentReply {
        let rejection = |code| CoreRecallIntentReply {
            server_tick: fallback_server_tick,
            result: RecallResultV1::Rejected {
                schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
                request_sequence: frame.sequence,
                character_id: frame.character_id,
                code,
            },
        };
        let handle = {
            let state = self.state.lock().await;
            if !state.accepting {
                return rejection(TerminalInventoryRejectionCodeV1::SourceUnavailable);
            }
            let Some(active) = state.transports.get(&lease.account_id) else {
                return rejection(TerminalInventoryRejectionCodeV1::SourceUnavailable);
            };
            let Some(entry) = state.actors.get(&lease.account_id) else {
                return rejection(TerminalInventoryRejectionCodeV1::SourceUnavailable);
            };
            if active.lease != lease {
                return rejection(TerminalInventoryRejectionCodeV1::SourceUnavailable);
            }
            if authenticated != entry.authenticated || frame.character_id != entry.character_id {
                return rejection(TerminalInventoryRejectionCodeV1::ForeignAuthority);
            }
            entry.handle.clone()
        };
        handle
            .handle_recall(authenticated, frame, fallback_server_tick)
            .await
    }

    async fn deliver_publication(
        &self,
        account_id: [u8; 16],
        published: ProductionRecallPublishedV1,
    ) -> bool {
        let result_character_id = match &published.result {
            RecallResultV1::Stored { result, .. } => result.character_id,
            RecallResultV1::Pending { character_id, .. }
            | RecallResultV1::Cancelled { character_id, .. }
            | RecallResultV1::Rejected { character_id, .. } => *character_id,
        };
        loop {
            let (lease, writer) = {
                let state = self.state.lock().await;
                let Some(entry) = state.actors.get(&account_id) else {
                    return false;
                };
                if result_character_id != entry.character_id {
                    return false;
                }
                let Some(active) = state.transports.get(&account_id) else {
                    return false;
                };
                (active.lease, Arc::clone(&active.writer))
            };
            let delivered = send_recall_publication(writer.as_ref(), &published)
                .await
                .is_ok();
            let state = self.state.lock().await;
            let Some(active) = state.transports.get(&account_id) else {
                return false;
            };
            if active.lease == lease {
                return delivered;
            }
            // A committed publication may have raced the authoritative handoff. Retry the same
            // durable result in the winning generation's independent sequence space.
        }
    }
}

#[derive(Clone)]
pub struct CoreRecallConnectionAuthority<Clock, TickSource> {
    directory: Arc<CoreRecallActorDirectory<Clock, TickSource>>,
    lease: CoreRecallConnectionLease,
}

impl<Clock, TickSource> CoreRecallIntentAuthority
    for CoreRecallConnectionAuthority<Clock, TickSource>
where
    Clock: ProductionRecallClock + 'static,
    TickSource: CoreRecallAuthoritativeTick + 'static,
{
    #[allow(
        clippy::manual_async_fn,
        reason = "the desugared public trait contract guarantees a Send future for spawned QUIC workers"
    )]
    fn handle_recall<'a>(
        &'a self,
        authenticated: AuthenticatedAccount,
        frame: &'a RecallFrameV1,
        server_tick: u64,
    ) -> impl Future<Output = CoreRecallIntentReply> + Send + 'a {
        async move {
            self.directory
                .handle_recall(self.lease, authenticated, frame, server_tick)
                .await
        }
    }
}

async fn serve_actor_mailbox<Clock, TickSource>(
    mut inbox: CoreRecallActorInbox,
    actor: Arc<ProductionRecallIntentActor<Clock>>,
    tick_source: Arc<TickSource>,
    route_lease: CorePrivateRouteActorLease,
    mut shutdown: oneshot::Receiver<()>,
) -> CoreRecallActorTaskReport
where
    Clock: ProductionRecallClock,
    TickSource: CoreRecallAuthoritativeTick,
{
    let mut served = 0_u64;
    loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => {
                inbox.close();
                return CoreRecallActorTaskReport {
                    served,
                    abandoned: u64::try_from(inbox.queued_command_count()).unwrap_or(u64::MAX),
                };
            }
            handled = inbox.serve_next_with_tick(actor.as_ref(), || {
                tick_source
                    .current_tick(route_lease)
                    .map(NonZeroU64::get)
            }) => {
                if !handled {
                    break;
                }
                served = served.saturating_add(1);
            }
        }
    }
    inbox.close();
    CoreRecallActorTaskReport {
        served,
        abandoned: u64::try_from(inbox.queued_command_count()).unwrap_or(u64::MAX),
    }
}

async fn serve_completion_outbox<Clock, TickSource>(
    mut inbox: CoreRecallCompletionInbox,
    directory: std::sync::Weak<CoreRecallActorDirectory<Clock, TickSource>>,
    account_id: [u8; 16],
    mut shutdown: oneshot::Receiver<()>,
) -> CoreRecallCompletionTaskReport
where
    Clock: ProductionRecallClock + 'static,
    TickSource: CoreRecallAuthoritativeTick + 'static,
{
    let mut delivered = 0_u64;
    let mut undelivered = 0_u64;
    loop {
        let next = tokio::select! {
            biased;
            _ = &mut shutdown => None,
            published = inbox.receive_next() => published,
        };
        let Some(published) = next else {
            let abandoned = u64::try_from(inbox.retire_undelivered()).unwrap_or(u64::MAX);
            return CoreRecallCompletionTaskReport {
                delivered,
                undelivered,
                abandoned,
            };
        };
        let sent = if let Some(directory) = directory.upgrade() {
            directory.deliver_publication(account_id, published).await
        } else {
            false
        };
        if sent {
            delivered = delivered.saturating_add(1);
        } else {
            undelivered = undelivered.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::{AccountId, ProductionRecallPendingAuthorityV1};
    use protocol::{
        ActionResultCode, CharacterLocation, CharacterLocationSnapshot, RecallIntentV1,
        RecallTerminalTriggerV1, ReliableEvent, SafeArrival, StoredRecallTerminalResultV1,
        TERMINAL_HALL_CONTENT_ID, TerminalInventoryRejectionCodeV1, TerminalVersionAdvanceV1,
        TerminalVersionVectorV1, WireMessage, WireText, decode_frame,
    };

    const ACCOUNT_ID: [u8; 16] = [41; 16];
    const CHARACTER_ID: [u8; 16] = [42; 16];

    #[derive(Debug, Clone, Copy)]
    struct FixedClock;

    impl ProductionRecallClock for FixedClock {
        fn unix_millis(&self) -> u64 {
            5_000
        }
    }

    #[derive(Debug)]
    struct TickSource(AtomicU64);

    impl CoreRecallAuthoritativeTick for TickSource {
        fn current_tick(&self, route: CorePrivateRouteActorLease) -> Option<NonZeroU64> {
            assert_eq!(route.account_id(), ACCOUNT_ID);
            assert_eq!(route.character_id(), CHARACTER_ID);
            NonZeroU64::new(self.0.load(Ordering::SeqCst))
        }
    }

    fn authenticated() -> AuthenticatedAccount {
        AuthenticatedAccount {
            account_id: AccountId::new(ACCOUNT_ID).unwrap(),
            namespace: AuthenticatedNamespace::WipeableTest,
        }
    }

    fn actor() -> Arc<ProductionRecallIntentActor<FixedClock>> {
        Arc::new(
            ProductionRecallIntentActor::new(
                FixedClock,
                ACCOUNT_ID,
                CHARACTER_ID,
                ProductionRecallPendingAuthorityV1 {
                    pending_item_count: 0,
                    pending_material_stack_count: 0,
                },
            )
            .unwrap(),
        )
    }

    fn route_lease() -> CorePrivateRouteActorLease {
        CorePrivateRouteActorLease::for_test(ACCOUNT_ID, CHARACTER_ID, 1)
    }

    fn frame() -> RecallFrameV1 {
        RecallFrameV1 {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            sequence: 1,
            character_id: CHARACTER_ID,
            client_tick: 10,
            intent: RecallIntentV1::Start,
        }
    }

    const fn version(before: u64, after: u64) -> TerminalVersionAdvanceV1 {
        TerminalVersionAdvanceV1 { before, after }
    }

    fn publication() -> ProductionRecallPublishedV1 {
        ProductionRecallPublishedV1 {
            result: RecallResultV1::Stored {
                schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
                request_sequence: Some(1),
                replayed: false,
                result: Box::new(StoredRecallTerminalResultV1 {
                    character_id: CHARACTER_ID,
                    terminal_id: [51; 16],
                    result_hash: [52; 32],
                    trigger: RecallTerminalTriggerV1::Explicit,
                    committed_at_unix_millis: 5_100,
                    completion_tick: 112,
                    destination_content_id: WireText::new(TERMINAL_HALL_CONTENT_ID).unwrap(),
                    versions: TerminalVersionVectorV1 {
                        account: version(5, 5),
                        character: version(6, 7),
                        world: version(6, 7),
                        inventory: version(7, 8),
                        life_clock: version(8, 9),
                    },
                    stabilized_item_count: 0,
                    stabilized_items_digest: [53; 32],
                    destroyed_item_count: 0,
                    destroyed_items_digest: [54; 32],
                    destroyed_material_stack_count: 0,
                    destroyed_materials_digest: [55; 32],
                }),
            },
            hall: CharacterLocationSnapshot {
                character_id: CHARACTER_ID,
                character_version: 7,
                location: CharacterLocation::Safe {
                    location_id: WireText::new(TERMINAL_HALL_CONTENT_ID).unwrap(),
                    arrival: SafeArrival::HallDefault,
                },
            },
            explicit_client_tick: Some(10),
        }
    }

    async fn connection_pair() -> (quinn::Endpoint, quinn::Endpoint, quinn::Connection) {
        let (server_endpoint, client_endpoint, client, server) = live_connection_pair().await;
        drop(client);
        (server_endpoint, client_endpoint, server)
    }

    async fn live_connection_pair() -> (
        quinn::Endpoint,
        quinn::Endpoint,
        quinn::Connection,
        quinn::Connection,
    ) {
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
        let certificate = cert.der().clone();
        let private_key = rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der());
        let server_config =
            quinn::ServerConfig::with_single_cert(vec![certificate.clone()], private_key.into())
                .unwrap();
        let server_endpoint =
            quinn::Endpoint::server(server_config, "127.0.0.1:0".parse().unwrap()).unwrap();
        let address = server_endpoint.local_addr().unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate).unwrap();
        let client_config = quinn::ClientConfig::with_root_certificates(Arc::new(roots)).unwrap();
        let mut client_endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        client_endpoint.set_default_client_config(client_config);
        let connecting = client_endpoint.connect(address, "localhost").unwrap();
        let incoming = server_endpoint.accept().await.unwrap();
        let (client, server) = tokio::join!(connecting, incoming);
        (
            server_endpoint,
            client_endpoint,
            client.unwrap(),
            server.unwrap(),
        )
    }

    #[tokio::test]
    async fn completion_push_shares_response_sequence_and_shutdown_drains_delivery_task() {
        let tick_source = Arc::new(TickSource(AtomicU64::new(100)));
        let directory = Arc::new(CoreRecallActorDirectory::new(tick_source));
        let registration = directory
            .register_actor(authenticated(), route_lease(), actor())
            .await
            .unwrap();
        let (server_endpoint, client_endpoint, client, server) = live_connection_pair().await;
        let session_writer = Arc::new(CoreRecallReliableWriter::new(server));
        let (mut client_send, mut client_receive) = client.open_bi().await.unwrap();
        client_send.write_all(&[1]).await.unwrap();
        client_send.finish().unwrap();
        let (server_send, mut server_receive) =
            session_writer.connection().accept_bi().await.unwrap();
        assert_eq!(server_receive.read_to_end(1).await.unwrap(), vec![1]);
        let response = session_writer
            .send_response(
                server_send,
                111,
                ReliableEvent::ActionResult {
                    action_sequence: 9,
                    code: ActionResultCode::Accepted,
                },
            )
            .await
            .unwrap();
        assert_eq!(response.sequence, 1);
        assert_eq!(response.server_tick, 111);
        let response_bytes = client_receive
            .read_to_end(protocol::RELIABLE_FRAME_LIMIT)
            .await
            .unwrap();
        let WireMessage::ReliableEvent(received_response) = decode_frame(&response_bytes).unwrap()
        else {
            panic!("expected reliable response frame");
        };
        assert_eq!(received_response, response);

        let attached = directory
            .attach_reliable_writer(authenticated(), Arc::clone(&session_writer))
            .await
            .unwrap();
        let recall_writer = directory.reliable_writer(attached.lease).await.unwrap();
        assert!(Arc::ptr_eq(&session_writer, &recall_writer));

        registration
            .completion_outbox
            .try_publish(publication())
            .unwrap();
        let pushed = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            bot_client::receive_server_reliable(&client),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(pushed.sequence, 2);
        assert_eq!(pushed.server_tick, 112);
        assert!(matches!(
            pushed.event,
            ReliableEvent::RecallResult(result)
                if matches!(*result, RecallResultV1::Stored { replayed: false, .. })
        ));
        assert_eq!(session_writer.last_sequence().await, 2);

        for connection in directory.begin_shutdown().await {
            connection.close(0_u32.into(), b"test shutdown");
        }
        let report = directory.finish_shutdown().await.unwrap();
        assert_eq!(report.delivered_completion_publications, 1);
        assert_eq!(report.undelivered_completion_publications, 0);
        assert_eq!(report.abandoned_completion_publications, 0);
        assert_eq!(report.remaining_completion_tasks, 0);
        assert!(report.zero_residue);
        client.close(0_u32.into(), b"test complete");
        server_endpoint.wait_idle().await;
        client_endpoint.wait_idle().await;
    }

    #[tokio::test]
    async fn completion_after_handoff_uses_only_the_winning_transport_generation() {
        let tick_source = Arc::new(TickSource(AtomicU64::new(100)));
        let directory = Arc::new(CoreRecallActorDirectory::new(tick_source));
        directory
            .register_actor(authenticated(), route_lease(), actor())
            .await
            .unwrap();
        let (first_server_endpoint, first_client_endpoint, first_client, first_server) =
            live_connection_pair().await;
        let first = directory
            .attach_transport(authenticated(), first_server)
            .await
            .unwrap();
        let first_writer = directory.reliable_writer(first.lease).await.unwrap();
        let blocked_sequence = first_writer.hold_sequence_for_test().await;
        let delivery_directory = Arc::clone(&directory);
        let delivery_task = tokio::spawn(async move {
            delivery_directory
                .deliver_publication(ACCOUNT_ID, publication())
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while Arc::strong_count(&first_writer) < 3 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        let (second_server_endpoint, second_client_endpoint, second_client, second_server) =
            live_connection_pair().await;
        let second = directory
            .attach_transport(authenticated(), second_server)
            .await
            .unwrap();
        assert!(second.invalidated_connection.is_some());
        assert!(!first_writer.is_available());
        tokio::time::timeout(std::time::Duration::from_secs(5), first_client.closed())
            .await
            .unwrap();

        drop(blocked_sequence);
        assert!(delivery_task.await.unwrap());
        let pushed = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            bot_client::receive_server_reliable(&second_client),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(pushed.sequence, 1);
        assert_eq!(pushed.server_tick, 112);
        assert!(matches!(pushed.event, ReliableEvent::RecallResult(_)));

        for connection in directory.begin_shutdown().await {
            connection.close(0_u32.into(), b"test shutdown");
        }
        assert!(directory.finish_shutdown().await.unwrap().zero_residue);
        second_client.close(0_u32.into(), b"test complete");
        first_server_endpoint.wait_idle().await;
        first_client_endpoint.wait_idle().await;
        second_server_endpoint.wait_idle().await;
        second_client_endpoint.wait_idle().await;
    }

    #[tokio::test]
    async fn authoritative_handoff_rejects_old_authority_before_stale_detach() {
        let tick_source = Arc::new(TickSource(AtomicU64::new(100)));
        let directory = Arc::new(CoreRecallActorDirectory::new(Arc::clone(&tick_source)));
        directory
            .register_actor(authenticated(), route_lease(), actor())
            .await
            .unwrap();
        let (first_server_endpoint, first_client_endpoint, first_connection) =
            connection_pair().await;
        let first = directory
            .attach_transport(authenticated(), first_connection)
            .await
            .unwrap();
        let first_authority = directory.authority(first.lease);

        tick_source.0.store(101, Ordering::SeqCst);
        let (second_server_endpoint, second_client_endpoint, second_connection) =
            connection_pair().await;
        let second = directory
            .attach_transport(authenticated(), second_connection)
            .await
            .unwrap();
        let invalidated = second
            .invalidated_connection
            .expect("authoritative handoff returns the old connection only after generation swap");
        invalidated.close(0_u32.into(), b"authoritative handoff");

        assert!(matches!(
            first_authority
                .handle_recall(authenticated(), &frame(), 0)
                .await
                .result,
            RecallResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::SourceUnavailable,
                ..
            }
        ));
        assert_eq!(
            directory
                .detach_transport(first.lease, 6_000)
                .await
                .unwrap(),
            ProductionRecallDetachOutcome::StaleGenerationIgnored
        );
        let second_authority = directory.authority(second.lease);
        let second_result = second_authority
            .handle_recall(authenticated(), &frame(), 0)
            .await
            .result;
        assert!(
            matches!(
                second_result,
                RecallResultV1::Pending {
                    started_tick: 101,
                    ..
                }
            ),
            "new authoritative transport must reach the actor: {second_result:?}"
        );

        for connection in directory.begin_shutdown().await {
            connection.close(0_u32.into(), b"test shutdown");
        }
        assert!(directory.finish_shutdown().await.unwrap().zero_residue);
        first_server_endpoint.wait_idle().await;
        first_client_endpoint.wait_idle().await;
        second_server_endpoint.wait_idle().await;
        second_client_endpoint.wait_idle().await;
    }

    #[tokio::test]
    async fn missing_authoritative_tick_rejects_without_mutating_recall_state() {
        let tick_source = Arc::new(TickSource(AtomicU64::new(100)));
        let directory = Arc::new(CoreRecallActorDirectory::new(Arc::clone(&tick_source)));
        directory
            .register_actor(authenticated(), route_lease(), actor())
            .await
            .unwrap();
        let (server_endpoint, client_endpoint, connection) = connection_pair().await;
        let attached = directory
            .attach_transport(authenticated(), connection)
            .await
            .unwrap();
        let authority = directory.authority(attached.lease);

        tick_source.0.store(0, Ordering::SeqCst);
        let unavailable = authority
            .handle_recall(authenticated(), &frame(), 999)
            .await;
        assert_eq!(unavailable.server_tick, 999);
        assert!(matches!(
            unavailable.result,
            RecallResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::SourceUnavailable,
                ..
            }
        ));

        tick_source.0.store(101, Ordering::SeqCst);
        let accepted = authority
            .handle_recall(authenticated(), &frame(), 999)
            .await;
        assert_eq!(accepted.server_tick, 101);
        assert!(matches!(
            accepted.result,
            RecallResultV1::Pending {
                started_tick: 101,
                ..
            }
        ));

        for connection in directory.begin_shutdown().await {
            connection.close(0_u32.into(), b"test shutdown");
        }
        assert!(directory.finish_shutdown().await.unwrap().zero_residue);
        server_endpoint.wait_idle().await;
        client_endpoint.wait_idle().await;
    }

    #[tokio::test]
    #[allow(
        clippy::too_many_lines,
        reason = "one real-QUIC test keeps the complete prepare, recover, supersede, abort, commit, retry, and shutdown contract contiguous"
    )]
    async fn coordinated_writer_handoff_is_recoverable_exact_and_non_retiring() {
        let tick_source = Arc::new(TickSource(AtomicU64::new(100)));
        let directory = Arc::new(CoreRecallActorDirectory::new(Arc::clone(&tick_source)));
        directory
            .register_actor(authenticated(), route_lease(), actor())
            .await
            .unwrap();
        let (first_server_endpoint, first_client_endpoint, first_client, first_connection) =
            live_connection_pair().await;
        let first_writer = Arc::new(CoreRecallReliableWriter::new(first_connection));
        let first = directory
            .attach_reliable_writer(authenticated(), Arc::clone(&first_writer))
            .await
            .unwrap();
        let old_authority = directory.authority(first.lease);

        let (second_server_endpoint, second_client_endpoint, second_client, second_connection) =
            live_connection_pair().await;
        let second_writer = Arc::new(CoreRecallReliableWriter::new(second_connection));
        let abandoned = directory
            .prepare_reliable_writer_handoff(authenticated(), Arc::clone(&second_writer))
            .await
            .unwrap();
        assert_eq!(
            directory
                .prepare_reliable_writer_handoff(authenticated(), Arc::clone(&second_writer))
                .await
                .unwrap(),
            abandoned,
            "a cancelled prepare call must recover its exact reservation"
        );

        let (third_server_endpoint, third_client_endpoint, third_client, third_connection) =
            live_connection_pair().await;
        let third_writer = Arc::new(CoreRecallReliableWriter::new(third_connection));
        let superseding = directory
            .prepare_reliable_writer_handoff(authenticated(), Arc::clone(&third_writer))
            .await
            .unwrap();
        assert!(superseding.handoff_generation > abandoned.handoff_generation);
        assert!(matches!(
            directory
                .commit_prepared_reliable_writer_handoff(abandoned)
                .await,
            Err(CoreRecallRuntimeError::PreparedWriterHandoffMismatch)
        ));
        assert!(matches!(
            old_authority
                .handle_recall(authenticated(), &frame(), 0)
                .await
                .result,
            RecallResultV1::Pending { .. }
        ));

        directory
            .abort_prepared_reliable_writer_handoff(superseding)
            .await
            .unwrap();
        assert!(Arc::ptr_eq(
            &directory.reliable_writer(first.lease).await.unwrap(),
            &first_writer
        ));
        let exact = directory
            .prepare_reliable_writer_handoff(authenticated(), Arc::clone(&third_writer))
            .await
            .unwrap();
        assert!(exact.handoff_generation > superseding.handoff_generation);
        let committed = directory
            .commit_prepared_reliable_writer_handoff(exact)
            .await
            .unwrap();
        assert!(committed.invalidated_connection.is_some());
        assert!(first_writer.is_available());
        assert!(third_writer.is_available());
        let replayed_commit = directory
            .commit_prepared_reliable_writer_handoff(exact)
            .await
            .unwrap();
        assert_eq!(replayed_commit.lease, committed.lease);
        assert!(replayed_commit.invalidated_connection.is_none());
        let changed = CoreRecallPreparedWriterHandoff {
            handoff_generation: exact.handoff_generation + 1,
            ..exact
        };
        assert!(matches!(
            directory
                .commit_prepared_reliable_writer_handoff(changed)
                .await,
            Err(CoreRecallRuntimeError::PreparedWriterHandoffMismatch)
        ));
        directory
            .prepare_reliable_writer_handoff(authenticated(), Arc::clone(&second_writer))
            .await
            .unwrap();

        for connection in directory.begin_shutdown().await {
            connection.close(0_u32.into(), b"test shutdown");
        }
        let report = directory.finish_shutdown().await.unwrap();
        assert_eq!(report.retired_pending_writer_handoffs, 1);
        assert!(report.zero_residue);
        assert!(first_writer.is_available());
        assert!(second_writer.is_available());
        first_writer.retire(0_u32, b"central session cleanup");
        second_writer.retire(0_u32, b"central session cleanup");
        third_writer.retire(0_u32, b"central session cleanup");
        first_client.close(0_u32.into(), b"test complete");
        second_client.close(0_u32.into(), b"test complete");
        third_client.close(0_u32.into(), b"test complete");
        first_server_endpoint.wait_idle().await;
        first_client_endpoint.wait_idle().await;
        second_server_endpoint.wait_idle().await;
        second_client_endpoint.wait_idle().await;
        third_server_endpoint.wait_idle().await;
        third_client_endpoint.wait_idle().await;
    }

    #[tokio::test]
    async fn shutdown_closes_actor_authority_and_reports_zero_runtime_residue() {
        let tick_source = Arc::new(TickSource(AtomicU64::new(100)));
        let directory = Arc::new(CoreRecallActorDirectory::new(tick_source));
        directory
            .register_actor(authenticated(), route_lease(), actor())
            .await
            .unwrap();
        let (server_endpoint, client_endpoint, connection) = connection_pair().await;
        let attached = directory
            .attach_transport(authenticated(), connection)
            .await
            .unwrap();
        let authority = directory.authority(attached.lease);
        assert!(matches!(
            authority
                .handle_recall(authenticated(), &frame(), 0)
                .await
                .result,
            RecallResultV1::Pending {
                started_tick: 100,
                ..
            }
        ));

        for connection in directory.begin_shutdown().await {
            connection.close(0_u32.into(), b"test shutdown");
        }
        let rejected = authority.handle_recall(authenticated(), &frame(), 0).await;
        assert!(matches!(
            rejected.result,
            RecallResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::SourceUnavailable,
                ..
            }
        ));
        let report = directory.finish_shutdown().await.unwrap();
        assert_eq!(report.served_actor_commands, 1);
        assert_eq!(report.remaining_actor_tasks, 0);
        assert_eq!(report.remaining_registered_actors, 0);
        assert_eq!(report.remaining_active_transports, 0);
        assert!(report.zero_residue);
        server_endpoint.wait_idle().await;
        client_endpoint.wait_idle().await;
    }
}
