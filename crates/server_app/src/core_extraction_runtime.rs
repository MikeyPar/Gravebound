//! Generation-safe actor, transport, and committed-publication runtime for Core extraction.
//!
//! The canonical GDD requires durable, replayable successful extraction and one reliable sequence
//! (`DTH-011`, `TECH-015`, `TECH-021`-`023`). The Content Production Specification fixes the
//! Caldus exit and Lantern Halls destination, and the Development Roadmap requires response-loss,
//! reconnect, process-restart, and cleanup proof for M03. This runtime owns the in-process portion:
//! one `BossExitReady` actor per account, one current transport binding using the session writer,
//! typed publication from a committed repository transaction, reconnect replay, and bounded
//! retirement. Durable process-restart recovery remains the terminal-first bootstrap owner's job.

use std::{collections::BTreeMap, future::Future, num::NonZeroU64, sync::Arc};

use protocol::{
    ExtractionCommitFrameV1, ExtractionCommitResultV1, ReliableEvent,
    StoredExtractionTerminalResultV1, TERMINAL_INVENTORY_SCHEMA_VERSION,
    TerminalInventoryRejectionCodeV1, TerminalInventoryValidationError,
};
use thiserror::Error;
#[cfg(test)]
use tokio::sync::Notify;
use tokio::{
    sync::{Mutex, oneshot},
    task::JoinHandle,
};

use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CoreExtractionActorHandle,
    CoreExtractionActorInbox, CoreExtractionIntentAuthority, CoreExtractionIntentReply,
    CorePrivateRouteRuntimeError, CoreReliableWriter, CoreTerminalCoordinator, IdentityClock,
    ProductionExtractionIntentActor, ProductionExtractionPlanner,
    ProductionExtractionPreparedIntentV1, ProductionExtractionPublicationProof,
    TRANSPORT_REPLACED_CLOSE_CODE, TerminalKind, committed_extraction_terminal_receipt,
    hall_snapshot_from_stored_extraction, production_extraction_actor_mailbox,
    protocol_extraction_terminal_result,
};

const EXTRACTION_TRANSPORT_DETACHED_CLOSE_CODE: u32 = 0x105;
const EXTRACTION_TRANSPORT_DETACHED_REASON: &[u8] = b"extraction transport detached";

pub trait CoreExtractionAuthoritativeTick: Send + Sync {
    fn current_tick(&self, lease: CoreExtractionActorLease) -> Option<NonZeroU64>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreExtractionActorLease {
    account_id: [u8; 16],
    character_id: [u8; 16],
    route_generation: u64,
}

impl CoreExtractionActorLease {
    #[must_use]
    pub const fn account_id(self) -> [u8; 16] {
        self.account_id
    }

    #[must_use]
    pub const fn character_id(self) -> [u8; 16] {
        self.character_id
    }

    #[must_use]
    pub const fn route_generation(self) -> u64 {
        self.route_generation
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CoreExtractionTransportGeneration(u64);

impl CoreExtractionTransportGeneration {
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreExtractionConnectionLease {
    actor: CoreExtractionActorLease,
    transport_generation: CoreExtractionTransportGeneration,
}

impl CoreExtractionConnectionLease {
    #[must_use]
    pub const fn actor(self) -> CoreExtractionActorLease {
        self.actor
    }

    #[must_use]
    pub const fn transport_generation(self) -> CoreExtractionTransportGeneration {
        self.transport_generation
    }
}

#[derive(Debug)]
pub struct CoreExtractionTransportAttach {
    pub lease: CoreExtractionConnectionLease,
    pub invalidated_connection: Option<quinn::Connection>,
    pub committed_result_replayed: bool,
}

/// Opaque reservation for extraction's side of a coordinated private-life writer handoff.
/// Preparing reserves the real extraction transport generation without changing active authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CoreExtractionPreparedWriterHandoff {
    lease: CoreExtractionConnectionLease,
}

impl CoreExtractionPreparedWriterHandoff {
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn lease(self) -> CoreExtractionConnectionLease {
        self.lease
    }
}

/// Generation-bound handoff material. The bootstrap/session owner first installs this Hall
/// snapshot, then consumes the token to clear extraction replay state and release only the
/// extraction binding. A stale transport generation cannot acknowledge a newer delivery.
#[derive(Debug)]
pub struct CoreExtractionHallProjection {
    lease: CoreExtractionConnectionLease,
    terminal_id: [u8; 16],
    result_hash: [u8; 32],
    hall: protocol::CharacterLocationSnapshot,
}

impl CoreExtractionHallProjection {
    #[must_use]
    pub(crate) const fn lease(&self) -> CoreExtractionConnectionLease {
        self.lease
    }

    #[must_use]
    pub const fn snapshot(&self) -> &protocol::CharacterLocationSnapshot {
        &self.hall
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreExtractionTransportDetach {
    Detached,
    StaleGenerationIgnored,
    PlannedShutdownIgnored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreExtractionPublicationOutcome {
    FreshDelivered,
    FreshQueued,
    ReplayedDelivered,
    ReplayedQueued,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreExtractionActorRetirementReport {
    pub served_commands: u64,
    pub abandoned_commands: u64,
    pub tick_authority_losses: u64,
    pub committed_result_retained: bool,
    pub zero_actor_residue: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreExtractionRuntimeReport {
    pub served_commands: u64,
    pub abandoned_commands: u64,
    pub tick_authority_losses: u64,
    pub retired_committed_results: usize,
    pub remaining_actor_tasks: usize,
    pub remaining_registered_actors: usize,
    pub remaining_retiring_actors: usize,
    pub route_retirement_failures: u64,
    pub remaining_active_transports: usize,
    pub retired_pending_writer_handoffs: usize,
    pub remaining_committed_results: usize,
    pub zero_residue: bool,
}

#[derive(Debug, Error)]
pub enum CoreExtractionRuntimeError {
    #[error("Core extraction runtime is retired")]
    Retired,
    #[error("Core extraction actor binding is invalid")]
    InvalidActorBinding,
    #[error("Core extraction actor is already registered")]
    ActorAlreadyRegistered,
    #[error("Core extraction actor is unavailable")]
    ActorUnavailable,
    #[error("Core extraction transport is stale or unavailable")]
    TransportUnavailable,
    #[error("Core extraction reliable writer is unavailable")]
    ReliableWriterUnavailable,
    #[error("Core extraction reliable writer is already attached")]
    ReliableWriterAlreadyAttached,
    #[error("Core extraction prepared reliable-writer handoff is stale or invalid")]
    PreparedWriterHandoffMismatch,
    #[error("Core extraction transport generation overflowed")]
    GenerationExhausted,
    #[error("Core extraction prepared intent does not match actor authority")]
    PreparedIntentMismatch,
    #[error("Core extraction transaction is conflicted or corrupt")]
    InvalidCommittedTransaction,
    #[error("Core extraction committed publication conflicts with retained authority")]
    CommittedPublicationConflict,
    #[error("Core extraction committed result has not been delivered")]
    CommittedResultUndelivered,
    #[error("Core extraction losing-terminal retirement proof is invalid")]
    InvalidTerminalWinner,
    #[error("Core extraction runtime shutdown has not started")]
    ShutdownNotStarted,
    #[error("Core extraction actor task failed")]
    ActorTaskFailed(#[source] tokio::task::JoinError),
    #[error("Core extraction route actor retirement failed")]
    RouteRetirement(#[from] CorePrivateRouteRuntimeError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommittedExtractionPublication {
    actor_lease: CoreExtractionActorLease,
    frame: ExtractionCommitFrameV1,
    server_tick: u64,
    stored: StoredExtractionTerminalResultV1,
    hall: protocol::CharacterLocationSnapshot,
}

impl CommittedExtractionPublication {
    fn from_transaction(
        actor_lease: CoreExtractionActorLease,
        intent: &ProductionExtractionPreparedIntentV1,
        proof: &ProductionExtractionPublicationProof,
    ) -> Result<Self, CoreExtractionRuntimeError> {
        let stored = proof
            .transaction()
            .result()
            .ok_or(CoreExtractionRuntimeError::InvalidCommittedTransaction)?;
        let request = intent.input().commit_request();
        if stored.account_id != actor_lease.account_id
            || stored.character_id != actor_lease.character_id
            || stored.account_id != request.account_id
            || stored.character_id != request.character_id
            || stored.mutation_id != request.mutation_id
            || stored.terminal_id != request.terminal_id
            || stored.extraction_request_id != request.extraction_request_id
            || stored.extraction_receipt_id != request.extraction_receipt_id
            || stored.canonical_request_hash
                != intent
                    .prepared()
                    .ok_or(CoreExtractionRuntimeError::PreparedIntentMismatch)?
                    .canonical_request_hash()
            || stored.canonical_plan_hash
                != intent
                    .prepared()
                    .ok_or(CoreExtractionRuntimeError::PreparedIntentMismatch)?
                    .canonical_plan_hash()
            || intent.acceptance().attempt.actor_generation != actor_lease.route_generation
        {
            return Err(CoreExtractionRuntimeError::InvalidCommittedTransaction);
        }
        let expected_receipt = committed_extraction_terminal_receipt(
            intent
                .prepared()
                .ok_or(CoreExtractionRuntimeError::PreparedIntentMismatch)?,
            stored,
        )
        .map_err(|_| CoreExtractionRuntimeError::InvalidCommittedTransaction)?;
        if proof.receipt() != &expected_receipt {
            return Err(CoreExtractionRuntimeError::InvalidCommittedTransaction);
        }
        let projected = protocol_extraction_terminal_result(stored)
            .map_err(|_| CoreExtractionRuntimeError::InvalidCommittedTransaction)?;
        let hall = hall_snapshot_from_stored_extraction(stored)
            .map_err(|_| CoreExtractionRuntimeError::InvalidCommittedTransaction)?;
        Ok(Self {
            actor_lease,
            frame: intent.frame().clone(),
            server_tick: intent.server_tick(),
            stored: projected,
            hall,
        })
    }

    fn exact_payload(&self, frame: &ExtractionCommitFrameV1) -> bool {
        self.frame.schema_version == frame.schema_version
            && self.frame.mutation_id == frame.mutation_id
            && self.frame.character_id == frame.character_id
            && self.frame.issued_at_unix_millis == frame.issued_at_unix_millis
            && self.frame.payload_hash == frame.payload_hash
            && self.frame.payload == frame.payload
    }

    fn result(&self, request_sequence: u32, replayed: bool) -> ExtractionCommitResultV1 {
        ExtractionCommitResultV1::Stored {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            request_sequence,
            replayed,
            result: Box::new(self.stored.clone()),
        }
    }

    fn event(&self, replayed: bool) -> ReliableEvent {
        ReliableEvent::ExtractionCommitResult(Box::new(self.result(self.frame.sequence, replayed)))
    }

    fn same_terminal(&self, other: &Self) -> bool {
        self.actor_lease == other.actor_lease
            && self.frame == other.frame
            && self.server_tick == other.server_tick
            && self.stored == other.stored
            && self.hall == other.hall
    }
}

struct ExtractionActorEntry<Planner, Clock> {
    authenticated: AuthenticatedAccount,
    lease: CoreExtractionActorLease,
    actor: Arc<ProductionExtractionIntentActor<Planner, Clock>>,
    handle: CoreExtractionActorHandle,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<ExtractionActorTaskReport>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExtractionActorTaskReport {
    served: u64,
    abandoned: u64,
    tick_authority_losses: u64,
}

#[derive(Debug)]
struct ActiveExtractionTransport {
    lease: CoreExtractionConnectionLease,
    writer: Arc<CoreReliableWriter>,
}

#[derive(Debug)]
struct PendingExtractionWriterHandoff {
    prepared: CoreExtractionPreparedWriterHandoff,
    authenticated: AuthenticatedAccount,
    writer: Arc<CoreReliableWriter>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtractionDeliveryState {
    InFlight,
    Succeeded,
}

struct ExtractionRuntimeState<Planner, Clock> {
    accepting: bool,
    shutdown_started: bool,
    actors: BTreeMap<[u8; 16], ExtractionActorEntry<Planner, Clock>>,
    retiring_actors: BTreeMap<[u8; 16], CoreExtractionActorLease>,
    transports: BTreeMap<[u8; 16], ActiveExtractionTransport>,
    pending_writer_handoffs: BTreeMap<[u8; 16], PendingExtractionWriterHandoff>,
    retired_pending_writer_handoffs: usize,
    committed: BTreeMap<[u8; 16], CommittedExtractionPublication>,
    delivery: BTreeMap<[u8; 16], (CoreExtractionTransportGeneration, ExtractionDeliveryState)>,
    next_transport_generation: BTreeMap<[u8; 16], u64>,
    route_generation_floors: BTreeMap<([u8; 16], [u8; 16]), u64>,
    retiring_tasks: Vec<JoinHandle<ExtractionActorTaskReport>>,
    served_commands: u64,
    abandoned_commands: u64,
    tick_authority_losses: u64,
    route_retirement_failures: u64,
    #[cfg(test)]
    delivery_test_gate: Option<(Arc<Notify>, Arc<Notify>)>,
}

pub struct CoreExtractionActorDirectory<Planner, Clock, TickSource> {
    tick_source: Arc<TickSource>,
    state: Mutex<ExtractionRuntimeState<Planner, Clock>>,
}

impl<Planner, Clock, TickSource> CoreExtractionActorDirectory<Planner, Clock, TickSource>
where
    Planner: ProductionExtractionPlanner + 'static,
    Clock: IdentityClock + 'static,
    TickSource: CoreExtractionAuthoritativeTick + 'static,
{
    #[must_use]
    pub fn new(tick_source: Arc<TickSource>) -> Self {
        Self {
            tick_source,
            state: Mutex::new(ExtractionRuntimeState {
                accepting: true,
                shutdown_started: false,
                actors: BTreeMap::new(),
                retiring_actors: BTreeMap::new(),
                transports: BTreeMap::new(),
                pending_writer_handoffs: BTreeMap::new(),
                retired_pending_writer_handoffs: 0,
                committed: BTreeMap::new(),
                delivery: BTreeMap::new(),
                next_transport_generation: BTreeMap::new(),
                route_generation_floors: BTreeMap::new(),
                retiring_tasks: Vec::new(),
                served_commands: 0,
                abandoned_commands: 0,
                tick_authority_losses: 0,
                route_retirement_failures: 0,
                #[cfg(test)]
                delivery_test_gate: None,
            }),
        }
    }

    #[cfg(test)]
    pub(crate) async fn install_delivery_test_gate(&self) -> (Arc<Notify>, Arc<Notify>) {
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        self.state.lock().await.delivery_test_gate =
            Some((Arc::clone(&started), Arc::clone(&release)));
        (started, release)
    }

    pub async fn register_actor(
        &self,
        authenticated: AuthenticatedAccount,
        actor: Arc<ProductionExtractionIntentActor<Planner, Clock>>,
    ) -> Result<CoreExtractionActorLease, CoreExtractionRuntimeError> {
        let authority = actor.authority();
        if authenticated.namespace != AuthenticatedNamespace::WipeableTest
            || authenticated.account_id.as_bytes() != authority.account_id()
            || authority.selected_character_id() == [0; 16]
            || authority.actor_generation() == 0
        {
            return Err(CoreExtractionRuntimeError::InvalidActorBinding);
        }
        let account_id = authority.account_id();
        let lease = CoreExtractionActorLease {
            account_id,
            character_id: authority.selected_character_id(),
            route_generation: authority.actor_generation(),
        };
        let mut state = self.state.lock().await;
        if !state.accepting {
            return Err(CoreExtractionRuntimeError::Retired);
        }
        if state.actors.contains_key(&account_id)
            || state.retiring_actors.contains_key(&account_id)
            || state.committed.contains_key(&account_id)
        {
            return Err(CoreExtractionRuntimeError::ActorAlreadyRegistered);
        }
        if state
            .route_generation_floors
            .get(&(lease.account_id, lease.character_id))
            .is_some_and(|floor| lease.route_generation <= *floor)
        {
            return Err(CoreExtractionRuntimeError::InvalidActorBinding);
        }
        let (handle, inbox) = production_extraction_actor_mailbox();
        let (shutdown_send, shutdown_receive) = oneshot::channel();
        let task = tokio::spawn(serve_extraction_actor(
            inbox,
            Arc::clone(&actor),
            Arc::clone(&self.tick_source),
            lease,
            shutdown_receive,
        ));
        state.actors.insert(
            account_id,
            ExtractionActorEntry {
                authenticated,
                lease,
                actor,
                handle,
                shutdown: Some(shutdown_send),
                task: Some(task),
            },
        );
        Ok(lease)
    }

    /// Returns the exact transport-independent actor binding admitted for this account. Session
    /// composition uses it before retaining a `LinkLost` extraction binding, so a foreign
    /// character or stale route generation cannot be attached on the next reconnect.
    pub async fn registered_actor_lease(
        &self,
        authenticated: AuthenticatedAccount,
    ) -> Result<CoreExtractionActorLease, CoreExtractionRuntimeError> {
        if authenticated.namespace != AuthenticatedNamespace::WipeableTest {
            return Err(CoreExtractionRuntimeError::InvalidActorBinding);
        }
        let state = self.state.lock().await;
        if !state.accepting {
            return Err(CoreExtractionRuntimeError::Retired);
        }
        let entry = state
            .actors
            .get(&authenticated.account_id.as_bytes())
            .ok_or(CoreExtractionRuntimeError::ActorUnavailable)?;
        if entry.authenticated != authenticated
            || entry.lease.account_id != authenticated.account_id.as_bytes()
        {
            return Err(CoreExtractionRuntimeError::InvalidActorBinding);
        }
        Ok(entry.lease)
    }

    /// Attaches the reliable writer already owned by the private-life session. A pending durable
    /// result is replayed on the new generation before this method returns.
    #[allow(
        dead_code,
        reason = "the terminal-first bootstrap composition slice will bind this crate-private session-writer seam before normal admission"
    )]
    pub(crate) async fn attach_reliable_writer(
        self: &Arc<Self>,
        authenticated: AuthenticatedAccount,
        writer: Arc<CoreReliableWriter>,
    ) -> Result<CoreExtractionTransportAttach, CoreExtractionRuntimeError> {
        let prepared = self
            .prepare_reliable_writer_handoff(authenticated, writer)
            .await?;
        self.commit_prepared_reliable_writer_handoff_inner(prepared, true)
            .await
    }

    /// Reserves extraction's real transport generation without changing the visible binding.
    /// The exact returned token must be committed or aborted by the private-life session owner.
    pub(crate) async fn prepare_reliable_writer_handoff(
        &self,
        authenticated: AuthenticatedAccount,
        writer: Arc<CoreReliableWriter>,
    ) -> Result<CoreExtractionPreparedWriterHandoff, CoreExtractionRuntimeError> {
        if authenticated.namespace != AuthenticatedNamespace::WipeableTest {
            return Err(CoreExtractionRuntimeError::InvalidActorBinding);
        }
        if !writer.is_available() {
            return Err(CoreExtractionRuntimeError::ReliableWriterUnavailable);
        }
        let account_id = authenticated.account_id.as_bytes();
        let mut state = self.state.lock().await;
        if !state.accepting {
            return Err(CoreExtractionRuntimeError::Retired);
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
            return Err(CoreExtractionRuntimeError::ReliableWriterAlreadyAttached);
        }
        let actor = state
            .actors
            .get(&account_id)
            .map(|entry| (entry.authenticated, entry.lease));
        let committed_lease = state
            .committed
            .get(&account_id)
            .map(|committed| committed.actor_lease);
        let actor_lease = match (actor, committed_lease) {
            (Some((binding, lease)), _) if binding == authenticated => lease,
            (None, Some(lease)) => lease,
            (Some(_), _) => return Err(CoreExtractionRuntimeError::InvalidActorBinding),
            _ => return Err(CoreExtractionRuntimeError::ActorUnavailable),
        };
        let next = *state
            .next_transport_generation
            .entry(account_id)
            .or_insert(1);
        let after = next
            .checked_add(1)
            .ok_or(CoreExtractionRuntimeError::GenerationExhausted)?;
        state.next_transport_generation.insert(account_id, after);
        let prepared = CoreExtractionPreparedWriterHandoff {
            lease: CoreExtractionConnectionLease {
                actor: actor_lease,
                transport_generation: CoreExtractionTransportGeneration(next),
            },
        };
        state.pending_writer_handoffs.insert(
            account_id,
            PendingExtractionWriterHandoff {
                prepared,
                authenticated,
                writer,
            },
        );
        Ok(prepared)
    }

    /// Commits only the exact prepared reservation. Shared writer retirement remains exclusively
    /// owned by the central private-life session.
    #[allow(
        dead_code,
        reason = "consumed by the private-life session composition slice"
    )]
    pub(crate) async fn commit_prepared_reliable_writer_handoff(
        self: &Arc<Self>,
        prepared: CoreExtractionPreparedWriterHandoff,
    ) -> Result<CoreExtractionTransportAttach, CoreExtractionRuntimeError> {
        self.commit_prepared_reliable_writer_handoff_inner(prepared, false)
            .await
    }

    async fn commit_prepared_reliable_writer_handoff_inner(
        self: &Arc<Self>,
        prepared: CoreExtractionPreparedWriterHandoff,
        retire_invalidated_writer: bool,
    ) -> Result<CoreExtractionTransportAttach, CoreExtractionRuntimeError> {
        let account_id = prepared.lease.actor.account_id;
        {
            let state = self.state.lock().await;
            if !state.accepting {
                return Err(CoreExtractionRuntimeError::Retired);
            }
            if state
                .transports
                .get(&account_id)
                .is_some_and(|active| active.lease == prepared.lease)
            {
                let has_committed = state.committed.contains_key(&account_id);
                drop(state);
                let committed_result_replayed = if has_committed {
                    self.deliver_committed(account_id, true).await
                } else {
                    false
                };
                return Ok(CoreExtractionTransportAttach {
                    lease: prepared.lease,
                    invalidated_connection: None,
                    committed_result_replayed,
                });
            }
        }
        let (lease, invalidated_connection, has_committed) = {
            let mut state = self.state.lock().await;
            if !state.accepting {
                return Err(CoreExtractionRuntimeError::Retired);
            }
            let pending = state
                .pending_writer_handoffs
                .get(&account_id)
                .ok_or(CoreExtractionRuntimeError::PreparedWriterHandoffMismatch)?;
            if pending.prepared != prepared {
                return Err(CoreExtractionRuntimeError::PreparedWriterHandoffMismatch);
            }
            if !pending.writer.is_available() {
                return Err(CoreExtractionRuntimeError::ReliableWriterUnavailable);
            }
            let actor = state
                .actors
                .get(&account_id)
                .map(|entry| (entry.authenticated, entry.lease));
            let committed_lease = state
                .committed
                .get(&account_id)
                .map(|committed| committed.actor_lease);
            let authoritative_lease = match (actor, committed_lease) {
                (Some((binding, lease)), _) if binding == pending.authenticated => lease,
                (None, Some(lease)) => lease,
                (Some(_), _) => return Err(CoreExtractionRuntimeError::InvalidActorBinding),
                _ => return Err(CoreExtractionRuntimeError::ActorUnavailable),
            };
            if authoritative_lease != prepared.lease.actor {
                return Err(CoreExtractionRuntimeError::PreparedWriterHandoffMismatch);
            }
            let pending = state
                .pending_writer_handoffs
                .remove(&account_id)
                .ok_or(CoreExtractionRuntimeError::PreparedWriterHandoffMismatch)?;
            let invalidated_connection = state
                .transports
                .insert(
                    account_id,
                    ActiveExtractionTransport {
                        lease: prepared.lease,
                        writer: pending.writer,
                    },
                )
                .map(|active| {
                    if retire_invalidated_writer {
                        active.writer.retire(
                            TRANSPORT_REPLACED_CLOSE_CODE,
                            b"authoritative extraction transport handoff",
                        );
                    }
                    active.writer.connection().clone()
                });
            state.delivery.remove(&account_id);
            (
                prepared.lease,
                invalidated_connection,
                committed_lease.is_some(),
            )
        };
        let committed_result_replayed = if has_committed {
            self.deliver_committed(account_id, true).await
        } else {
            false
        };
        Ok(CoreExtractionTransportAttach {
            lease,
            invalidated_connection,
            committed_result_replayed,
        })
    }

    /// Cancels only the exact prepared reservation and intentionally does not reuse its reserved
    /// extraction transport generation.
    #[allow(
        dead_code,
        reason = "consumed by the private-life session composition slice"
    )]
    pub(crate) async fn abort_prepared_reliable_writer_handoff(
        &self,
        prepared: CoreExtractionPreparedWriterHandoff,
    ) -> Result<(), CoreExtractionRuntimeError> {
        let account_id = prepared.lease.actor.account_id;
        let mut state = self.state.lock().await;
        let pending = state
            .pending_writer_handoffs
            .get(&account_id)
            .ok_or(CoreExtractionRuntimeError::PreparedWriterHandoffMismatch)?;
        if pending.prepared != prepared {
            return Err(CoreExtractionRuntimeError::PreparedWriterHandoffMismatch);
        }
        state.pending_writer_handoffs.remove(&account_id);
        Ok(())
    }

    #[must_use]
    pub fn authority(
        self: &Arc<Self>,
        lease: CoreExtractionConnectionLease,
    ) -> CoreExtractionConnectionAuthority<Planner, Clock, TickSource> {
        CoreExtractionConnectionAuthority {
            directory: Arc::clone(self),
            lease,
        }
    }

    #[cfg(test)]
    pub(crate) async fn reliable_writer(
        &self,
        lease: CoreExtractionConnectionLease,
    ) -> Result<Arc<CoreReliableWriter>, CoreExtractionRuntimeError> {
        let state = self.state.lock().await;
        let active = state
            .transports
            .get(&lease.actor.account_id)
            .ok_or(CoreExtractionRuntimeError::TransportUnavailable)?;
        if active.lease != lease {
            return Err(CoreExtractionRuntimeError::TransportUnavailable);
        }
        Ok(Arc::clone(&active.writer))
    }

    pub async fn publish_coordinated(
        self: &Arc<Self>,
        actor_lease: CoreExtractionActorLease,
        intent: &ProductionExtractionPreparedIntentV1,
        proof: &ProductionExtractionPublicationProof,
    ) -> Result<CoreExtractionPublicationOutcome, CoreExtractionRuntimeError> {
        let actor = {
            let state = self.state.lock().await;
            let entry = state
                .actors
                .get(&actor_lease.account_id)
                .ok_or(CoreExtractionRuntimeError::ActorUnavailable)?;
            if entry.lease != actor_lease {
                return Err(CoreExtractionRuntimeError::InvalidActorBinding);
            }
            Arc::clone(&entry.actor)
        };
        if actor.prepared_intent().await.as_ref() != Some(intent) {
            return Err(CoreExtractionRuntimeError::PreparedIntentMismatch);
        }
        let published =
            CommittedExtractionPublication::from_transaction(actor_lease, intent, proof)?;
        let replayed = proof.is_replay();
        {
            let mut state = self.state.lock().await;
            let entry = state
                .actors
                .get(&actor_lease.account_id)
                .ok_or(CoreExtractionRuntimeError::ActorUnavailable)?;
            if entry.lease != actor_lease
                || published.stored.character_id != actor_lease.character_id
            {
                return Err(CoreExtractionRuntimeError::InvalidActorBinding);
            }
            if let Some(existing) = state.committed.get(&actor_lease.account_id) {
                if !existing.same_terminal(&published) {
                    return Err(CoreExtractionRuntimeError::CommittedPublicationConflict);
                }
            } else {
                state.committed.insert(actor_lease.account_id, published);
            }
        }
        let delivered = self
            .deliver_committed(actor_lease.account_id, replayed)
            .await;
        Ok(match (replayed, delivered) {
            (false, true) => CoreExtractionPublicationOutcome::FreshDelivered,
            (false, false) => CoreExtractionPublicationOutcome::FreshQueued,
            (true, true) => CoreExtractionPublicationOutcome::ReplayedDelivered,
            (true, false) => CoreExtractionPublicationOutcome::ReplayedQueued,
        })
    }

    /// Retires the terminal route actor only after a committed publication is retained. The
    /// publication remains available for response-loss and reconnect replay.
    pub async fn retire_actor_after_commit(
        &self,
        actor_lease: CoreExtractionActorLease,
    ) -> Result<CoreExtractionActorRetirementReport, CoreExtractionRuntimeError> {
        self.retire_actor(actor_lease, true, false).await
    }

    /// Retires extraction authority when the shared terminal coordinator committed a different
    /// producer. The generation floor advances, but the private-life session writer remains alive
    /// for the winning terminal projection and Hall recovery.
    pub async fn retire_actor_after_other_terminal(
        &self,
        actor_lease: CoreExtractionActorLease,
        coordinator: &CoreTerminalCoordinator,
    ) -> Result<CoreExtractionActorRetirementReport, CoreExtractionRuntimeError> {
        let receipt = coordinator
            .committed_receipt()
            .ok_or(CoreExtractionRuntimeError::InvalidTerminalWinner)?;
        let (instance_lineage_id, entry_restore_point_id) = {
            let state = self.state.lock().await;
            let entry = state
                .actors
                .get(&actor_lease.account_id)
                .ok_or(CoreExtractionRuntimeError::ActorUnavailable)?;
            if entry.lease != actor_lease {
                return Err(CoreExtractionRuntimeError::InvalidActorBinding);
            }
            (
                entry.actor.authority().instance_lineage_id(),
                entry.actor.authority().entry_restore_point_id(),
            )
        };
        if coordinator.authenticated_account().namespace != AuthenticatedNamespace::WipeableTest
            || coordinator.authenticated_account().account_id.as_bytes() != actor_lease.account_id
            || receipt.binding().account_id() != &actor_lease.account_id
            || receipt.binding().character_id() != &actor_lease.character_id
            || receipt.binding().lineage_id() != &instance_lineage_id
            || receipt.binding().restore_point_id() != &entry_restore_point_id
            || receipt.kind() == TerminalKind::SuccessfulExtraction
        {
            return Err(CoreExtractionRuntimeError::InvalidTerminalWinner);
        }
        self.retire_actor(actor_lease, false, true).await
    }

    async fn retire_actor(
        &self,
        actor_lease: CoreExtractionActorLease,
        require_extraction_commit: bool,
        release_transport_binding: bool,
    ) -> Result<CoreExtractionActorRetirementReport, CoreExtractionRuntimeError> {
        let mut entry = {
            let mut state = self.state.lock().await;
            if state.committed.contains_key(&actor_lease.account_id) != require_extraction_commit {
                return Err(CoreExtractionRuntimeError::CommittedResultUndelivered);
            }
            let entry = state
                .actors
                .get(&actor_lease.account_id)
                .ok_or(CoreExtractionRuntimeError::ActorUnavailable)?;
            if entry.lease != actor_lease {
                return Err(CoreExtractionRuntimeError::InvalidActorBinding);
            }
            state
                .route_generation_floors
                .entry((actor_lease.account_id, actor_lease.character_id))
                .and_modify(|floor| *floor = (*floor).max(actor_lease.route_generation))
                .or_insert(actor_lease.route_generation);
            let entry = state
                .actors
                .remove(&actor_lease.account_id)
                .ok_or(CoreExtractionRuntimeError::ActorUnavailable)?;
            state
                .retiring_actors
                .insert(actor_lease.account_id, actor_lease);
            if release_transport_binding {
                state.transports.remove(&actor_lease.account_id);
                state.delivery.remove(&actor_lease.account_id);
            }
            state
                .pending_writer_handoffs
                .remove(&actor_lease.account_id);
            entry
        };
        if let Err(error) = entry.actor.retire_route_after_terminal().await {
            if let Some(shutdown) = entry.shutdown.take() {
                let _ = shutdown.send(());
            }
            let mut state = self.state.lock().await;
            if let Some(task) = entry.task.take() {
                state.retiring_tasks.push(task);
            }
            return Err(error.into());
        }
        if let Some(shutdown) = entry.shutdown.take() {
            let _ = shutdown.send(());
        }
        let task = entry
            .task
            .take()
            .ok_or(CoreExtractionRuntimeError::ActorUnavailable)?
            .await
            .map_err(CoreExtractionRuntimeError::ActorTaskFailed)?;
        let mut state = self.state.lock().await;
        state.retiring_actors.remove(&actor_lease.account_id);
        state.served_commands = state.served_commands.saturating_add(task.served);
        state.abandoned_commands = state.abandoned_commands.saturating_add(task.abandoned);
        state.tick_authority_losses = state
            .tick_authority_losses
            .saturating_add(task.tick_authority_losses);
        let zero_actor_residue = !state.actors.contains_key(&actor_lease.account_id)
            && !state.retiring_actors.contains_key(&actor_lease.account_id);
        Ok(CoreExtractionActorRetirementReport {
            served_commands: task.served,
            abandoned_commands: task.abandoned,
            tick_authority_losses: task.tick_authority_losses,
            committed_result_retained: state.committed.contains_key(&actor_lease.account_id),
            zero_actor_residue,
        })
    }

    /// Returns a non-consuming Hall projection only to the exact transport generation that
    /// received the stored terminal result. Failure to install Hall leaves replay authority intact.
    pub async fn prepare_hall_projection(
        &self,
        lease: CoreExtractionConnectionLease,
        terminal_id: [u8; 16],
        result_hash: [u8; 32],
    ) -> Result<CoreExtractionHallProjection, CoreExtractionRuntimeError> {
        let state = self.state.lock().await;
        let account_id = lease.actor.account_id;
        if state.actors.contains_key(&account_id) {
            return Err(CoreExtractionRuntimeError::ActorAlreadyRegistered);
        }
        let committed = state
            .committed
            .get(&account_id)
            .ok_or(CoreExtractionRuntimeError::CommittedResultUndelivered)?;
        let active = state
            .transports
            .get(&account_id)
            .ok_or(CoreExtractionRuntimeError::CommittedResultUndelivered)?;
        if committed.stored.terminal_id != terminal_id
            || committed.stored.result_hash != result_hash
            || active.lease != lease
            || state.delivery.get(&account_id)
                != Some(&(
                    lease.transport_generation,
                    ExtractionDeliveryState::Succeeded,
                ))
        {
            return Err(CoreExtractionRuntimeError::CommittedResultUndelivered);
        }
        Ok(CoreExtractionHallProjection {
            lease,
            terminal_id,
            result_hash,
            hall: committed.hall.clone(),
        })
    }

    /// Consumes the exact projection only after Hall installation succeeds. Replay authority and
    /// the extraction binding are removed atomically; the shared session writer is not retired.
    pub async fn acknowledge_hall_installed(
        &self,
        projection: CoreExtractionHallProjection,
    ) -> Result<(), CoreExtractionRuntimeError> {
        let mut state = self.state.lock().await;
        let lease = projection.lease;
        if state.actors.contains_key(&lease.actor.account_id) {
            return Err(CoreExtractionRuntimeError::CommittedResultUndelivered);
        }
        let active = state
            .transports
            .get(&lease.actor.account_id)
            .ok_or(CoreExtractionRuntimeError::TransportUnavailable)?;
        if active.lease != lease {
            return Err(CoreExtractionRuntimeError::TransportUnavailable);
        }
        let committed = state
            .committed
            .get(&lease.actor.account_id)
            .ok_or(CoreExtractionRuntimeError::CommittedResultUndelivered)?;
        if committed.stored.terminal_id != projection.terminal_id
            || committed.stored.result_hash != projection.result_hash
            || committed.hall != projection.hall
            || state.delivery.get(&lease.actor.account_id)
                != Some(&(
                    lease.transport_generation,
                    ExtractionDeliveryState::Succeeded,
                ))
        {
            return Err(CoreExtractionRuntimeError::CommittedResultUndelivered);
        }
        state.committed.remove(&lease.actor.account_id);
        state.transports.remove(&lease.actor.account_id);
        state.delivery.remove(&lease.actor.account_id);
        Ok(())
    }

    pub async fn detach_transport(
        &self,
        lease: CoreExtractionConnectionLease,
    ) -> CoreExtractionTransportDetach {
        self.detach_transport_inner(lease, true).await
    }

    /// Releases only extraction's exact dynamic binding. The shared reliable writer remains
    /// owned by the central private-life session, which performs the one authoritative retire.
    #[allow(
        dead_code,
        reason = "consumed by the private-life session composition slice"
    )]
    pub(crate) async fn detach_shared_reliable_writer(
        &self,
        lease: CoreExtractionConnectionLease,
    ) -> CoreExtractionTransportDetach {
        self.detach_transport_inner(lease, false).await
    }

    async fn detach_transport_inner(
        &self,
        lease: CoreExtractionConnectionLease,
        retire_writer: bool,
    ) -> CoreExtractionTransportDetach {
        let mut state = self.state.lock().await;
        if state.shutdown_started {
            return CoreExtractionTransportDetach::PlannedShutdownIgnored;
        }
        let Some(active) = state.transports.get(&lease.actor.account_id) else {
            return CoreExtractionTransportDetach::StaleGenerationIgnored;
        };
        if active.lease != lease {
            return CoreExtractionTransportDetach::StaleGenerationIgnored;
        }
        let Some(active) = state.transports.remove(&lease.actor.account_id) else {
            return CoreExtractionTransportDetach::StaleGenerationIgnored;
        };
        state.delivery.remove(&lease.actor.account_id);
        if retire_writer {
            active.writer.retire(
                EXTRACTION_TRANSPORT_DETACHED_CLOSE_CODE,
                EXTRACTION_TRANSPORT_DETACHED_REASON,
            );
        }
        CoreExtractionTransportDetach::Detached
    }

    pub async fn begin_shutdown(&self) -> Vec<quinn::Connection> {
        let (connections, actors) = {
            let mut state = self.state.lock().await;
            state.accepting = false;
            state.shutdown_started = true;
            let connections = std::mem::take(&mut state.transports)
                .into_values()
                .map(|active| {
                    active.writer.retire(
                        crate::SERVER_SHUTDOWN_CLOSE_CODE,
                        b"extraction runtime shutdown",
                    );
                    active.writer.connection().clone()
                })
                .collect();
            state.retired_pending_writer_handoffs = state
                .retired_pending_writer_handoffs
                .saturating_add(state.pending_writer_handoffs.len());
            state.pending_writer_handoffs.clear();
            (connections, std::mem::take(&mut state.actors))
        };
        let mut tasks = Vec::with_capacity(actors.len());
        let mut route_retirement_failures = 0_u64;
        for mut entry in actors.into_values() {
            if entry.actor.retire_route_after_terminal().await.is_err() {
                route_retirement_failures = route_retirement_failures.saturating_add(1);
            }
            if let Some(shutdown) = entry.shutdown.take() {
                let _ = shutdown.send(());
            }
            if let Some(task) = entry.task.take() {
                tasks.push(task);
            }
        }
        let mut state = self.state.lock().await;
        state.retiring_tasks.extend(tasks);
        state.route_retirement_failures = state
            .route_retirement_failures
            .saturating_add(route_retirement_failures);
        connections
    }

    pub async fn finish_shutdown(
        &self,
    ) -> Result<CoreExtractionRuntimeReport, CoreExtractionRuntimeError> {
        let tasks = {
            let mut state = self.state.lock().await;
            if !state.shutdown_started {
                return Err(CoreExtractionRuntimeError::ShutdownNotStarted);
            }
            std::mem::take(&mut state.retiring_tasks)
        };
        let mut served = 0_u64;
        let mut abandoned = 0_u64;
        let mut tick_authority_losses = 0_u64;
        for task in tasks {
            let report = task
                .await
                .map_err(CoreExtractionRuntimeError::ActorTaskFailed)?;
            served = served.saturating_add(report.served);
            abandoned = abandoned.saturating_add(report.abandoned);
            tick_authority_losses =
                tick_authority_losses.saturating_add(report.tick_authority_losses);
        }
        let mut state = self.state.lock().await;
        state.served_commands = state.served_commands.saturating_add(served);
        state.abandoned_commands = state.abandoned_commands.saturating_add(abandoned);
        state.tick_authority_losses = state
            .tick_authority_losses
            .saturating_add(tick_authority_losses);
        let remaining_actor_tasks = state.retiring_tasks.len();
        let remaining_registered_actors = state.actors.len();
        let remaining_retiring_actors = state.retiring_actors.len();
        let remaining_active_transports = state.transports.len();
        let retired_pending_writer_handoffs = state.retired_pending_writer_handoffs;
        state.pending_writer_handoffs.clear();
        let retired_committed_results = state.committed.len();
        state.committed.clear();
        state.delivery.clear();
        state.next_transport_generation.clear();
        state.route_generation_floors.clear();
        let remaining_committed_results = state.committed.len();
        Ok(CoreExtractionRuntimeReport {
            served_commands: state.served_commands,
            abandoned_commands: state.abandoned_commands,
            tick_authority_losses: state.tick_authority_losses,
            retired_committed_results,
            remaining_actor_tasks,
            remaining_registered_actors,
            remaining_retiring_actors,
            route_retirement_failures: state.route_retirement_failures,
            remaining_active_transports,
            retired_pending_writer_handoffs,
            remaining_committed_results,
            zero_residue: remaining_actor_tasks == 0
                && remaining_registered_actors == 0
                && remaining_retiring_actors == 0
                && state.route_retirement_failures == 0
                && remaining_active_transports == 0
                && state.pending_writer_handoffs.is_empty()
                && remaining_committed_results == 0,
        })
    }

    async fn handle_extraction(
        &self,
        lease: CoreExtractionConnectionLease,
        authenticated: AuthenticatedAccount,
        frame: &ExtractionCommitFrameV1,
        fallback_server_tick: u64,
    ) -> CoreExtractionIntentReply {
        let rejection = |code| CoreExtractionIntentReply {
            server_tick: fallback_server_tick,
            result: rejected(frame, code),
        };
        if authenticated.namespace != AuthenticatedNamespace::WipeableTest
            || authenticated.account_id.as_bytes() != lease.actor.account_id
        {
            return rejection(TerminalInventoryRejectionCodeV1::ForeignAuthority);
        }
        let (handle, committed) = {
            let state = self.state.lock().await;
            if !state.accepting {
                return rejection(TerminalInventoryRejectionCodeV1::SourceUnavailable);
            }
            let Some(active) = state.transports.get(&lease.actor.account_id) else {
                return rejection(TerminalInventoryRejectionCodeV1::SourceUnavailable);
            };
            if active.lease != lease {
                return rejection(TerminalInventoryRejectionCodeV1::SourceUnavailable);
            }
            let handle = state.actors.get(&lease.actor.account_id).and_then(|entry| {
                (entry.lease == lease.actor && entry.authenticated == authenticated)
                    .then(|| entry.handle.clone())
            });
            (
                handle,
                state.committed.get(&lease.actor.account_id).cloned(),
            )
        };
        let Some(handle) = handle else {
            return committed.as_ref().map_or_else(
                || rejection(TerminalInventoryRejectionCodeV1::SourceUnavailable),
                |committed| committed_reply(committed, authenticated, frame, fallback_server_tick),
            );
        };
        let reply = handle
            .handle_extraction(authenticated, frame, fallback_server_tick)
            .await;
        let state = self.state.lock().await;
        if state
            .transports
            .get(&lease.actor.account_id)
            .is_none_or(|active| active.lease != lease)
        {
            return rejection(TerminalInventoryRejectionCodeV1::SourceUnavailable);
        }
        if matches!(reply.result, ExtractionCommitResultV1::Pending { .. })
            && let Some(committed) = state.committed.get(&lease.actor.account_id)
        {
            return committed_reply(committed, authenticated, frame, reply.server_tick);
        }
        reply
    }

    async fn deliver_committed(self: &Arc<Self>, account_id: [u8; 16], replayed: bool) -> bool {
        let (lease, writer, published) = {
            let mut state = self.state.lock().await;
            let Some(active) = state.transports.get(&account_id) else {
                return false;
            };
            let Some(published) = state.committed.get(&account_id) else {
                return false;
            };
            if let Some((generation, delivery)) = state.delivery.get(&account_id).copied()
                && generation == active.lease.transport_generation
            {
                return delivery == ExtractionDeliveryState::Succeeded;
            }
            let lease = active.lease;
            let writer = Arc::clone(&active.writer);
            let published = published.clone();
            state.delivery.insert(
                account_id,
                (
                    lease.transport_generation,
                    ExtractionDeliveryState::InFlight,
                ),
            );
            (lease, writer, published)
        };
        let directory = Arc::clone(self);
        let delivery = tokio::spawn(async move {
            directory.wait_for_delivery_test_gate().await;
            let delivered = writer
                .send_event(published.server_tick, published.event(replayed))
                .await
                .is_ok();
            directory
                .complete_delivery(account_id, lease, delivered)
                .await
        });
        match delivery.await {
            Ok(delivered) => delivered,
            Err(_) => self.complete_delivery(account_id, lease, false).await,
        }
    }

    #[cfg_attr(
        not(test),
        allow(
            clippy::unused_async,
            reason = "the production build has no test gate, while the same call site remains awaitable"
        )
    )]
    async fn wait_for_delivery_test_gate(&self) {
        #[cfg(test)]
        {
            let gate = self.state.lock().await.delivery_test_gate.take();
            if let Some((started, release)) = gate {
                started.notify_one();
                release.notified().await;
            }
        }
    }

    async fn complete_delivery(
        &self,
        account_id: [u8; 16],
        lease: CoreExtractionConnectionLease,
        delivered: bool,
    ) -> bool {
        let mut state = self.state.lock().await;
        let Some(active) = state.transports.get(&account_id) else {
            return false;
        };
        if active.lease != lease
            || state.delivery.get(&account_id)
                != Some(&(
                    lease.transport_generation,
                    ExtractionDeliveryState::InFlight,
                ))
        {
            return false;
        }
        if delivered {
            state.delivery.insert(
                account_id,
                (
                    lease.transport_generation,
                    ExtractionDeliveryState::Succeeded,
                ),
            );
        } else {
            state.delivery.remove(&account_id);
        }
        delivered
    }
}

#[derive(Clone)]
pub struct CoreExtractionConnectionAuthority<Planner, Clock, TickSource> {
    directory: Arc<CoreExtractionActorDirectory<Planner, Clock, TickSource>>,
    lease: CoreExtractionConnectionLease,
}

impl<Planner, Clock, TickSource> CoreExtractionIntentAuthority
    for CoreExtractionConnectionAuthority<Planner, Clock, TickSource>
where
    Planner: ProductionExtractionPlanner + 'static,
    Clock: IdentityClock + 'static,
    TickSource: CoreExtractionAuthoritativeTick + 'static,
{
    #[allow(
        clippy::manual_async_fn,
        reason = "the desugared public trait contract guarantees a Send future for QUIC workers"
    )]
    fn handle_extraction<'a>(
        &'a self,
        authenticated: AuthenticatedAccount,
        frame: &'a ExtractionCommitFrameV1,
        server_tick: u64,
    ) -> impl Future<Output = CoreExtractionIntentReply> + Send + 'a {
        async move {
            self.directory
                .handle_extraction(self.lease, authenticated, frame, server_tick)
                .await
        }
    }
}

async fn serve_extraction_actor<Planner, Clock, TickSource>(
    mut inbox: CoreExtractionActorInbox,
    actor: Arc<ProductionExtractionIntentActor<Planner, Clock>>,
    tick_source: Arc<TickSource>,
    lease: CoreExtractionActorLease,
    mut shutdown: oneshot::Receiver<()>,
) -> ExtractionActorTaskReport
where
    Planner: ProductionExtractionPlanner,
    Clock: IdentityClock,
    TickSource: CoreExtractionAuthoritativeTick,
{
    let mut served = 0_u64;
    loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => {
                inbox.close();
                return ExtractionActorTaskReport {
                    served,
                    abandoned: u64::try_from(inbox.queued_command_count()).unwrap_or(u64::MAX),
                    tick_authority_losses: 0,
                };
            }
            handled = inbox.serve_next_with_tick(actor.as_ref(), || {
                tick_source.current_tick(lease).map(NonZeroU64::get)
            }) => {
                match handled {
                    Ok(true) => served = served.saturating_add(1),
                    Ok(false) => break,
                    Err(()) => {
                        inbox.close();
                        return ExtractionActorTaskReport {
                            served,
                            abandoned: u64::try_from(inbox.queued_command_count()).unwrap_or(u64::MAX),
                            tick_authority_losses: 1,
                        };
                    }
                }
            }
        }
    }
    inbox.close();
    ExtractionActorTaskReport {
        served,
        abandoned: u64::try_from(inbox.queued_command_count()).unwrap_or(u64::MAX),
        tick_authority_losses: 0,
    }
}

fn committed_reply(
    committed: &CommittedExtractionPublication,
    authenticated: AuthenticatedAccount,
    frame: &ExtractionCommitFrameV1,
    fallback_server_tick: u64,
) -> CoreExtractionIntentReply {
    let code = match frame.validate() {
        Err(TerminalInventoryValidationError::PayloadHashMismatch) => {
            Some(TerminalInventoryRejectionCodeV1::PayloadHashMismatch)
        }
        Err(_) => Some(TerminalInventoryRejectionCodeV1::InvalidRequest),
        Ok(())
            if authenticated.namespace != AuthenticatedNamespace::WipeableTest
                || authenticated.account_id.as_bytes() != committed.actor_lease.account_id =>
        {
            Some(TerminalInventoryRejectionCodeV1::ForeignAuthority)
        }
        Ok(()) if committed.exact_payload(frame) => None,
        Ok(()) if frame.payload.extraction_request_id != committed.stored.extraction_request_id => {
            Some(TerminalInventoryRejectionCodeV1::ForeignAuthority)
        }
        Ok(()) => Some(TerminalInventoryRejectionCodeV1::IdempotencyConflict),
    };
    CoreExtractionIntentReply {
        server_tick: if code.is_none() {
            committed.server_tick
        } else {
            fallback_server_tick
        },
        result: code.map_or_else(
            || committed.result(frame.sequence, true),
            |code| rejected(frame, code),
        ),
    }
}

fn rejected(
    frame: &ExtractionCommitFrameV1,
    code: TerminalInventoryRejectionCodeV1,
) -> ExtractionCommitResultV1 {
    ExtractionCommitResultV1::Rejected {
        schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
        request_sequence: frame.sequence,
        mutation_id: frame.mutation_id,
        character_id: frame.character_id,
        extraction_request_id: frame.payload.extraction_request_id,
        code,
    }
}
