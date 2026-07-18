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
    AuthenticatedAccount, AuthenticatedNamespace, CoreRecallActorHandle, CoreRecallActorInbox,
    CoreRecallCompletionInbox, CoreRecallCompletionOutbox, CoreRecallIntentAuthority,
    CoreRecallIntentReply, CoreRecallReliableWriter, ProductionRecallClock,
    ProductionRecallDetachOutcome, ProductionRecallIntentActor, ProductionRecallPublishedV1,
    ProductionRecallSessionError, ProductionRecallSessionLifecycle,
    ProductionRecallTransportGeneration, TRANSPORT_REPLACED_CLOSE_CODE,
    core_recall_completion_outbox, production_recall_actor_mailbox, send_recall_publication,
};

pub trait CoreRecallAuthoritativeTick: Send + Sync {
    fn current_tick(&self, account_id: [u8; 16], character_id: [u8; 16]) -> NonZeroU64;
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

#[derive(Debug, Clone)]
pub struct CoreRecallActorRegistration {
    pub completion_outbox: CoreRecallCompletionOutbox,
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
}

struct CoreRecallRuntimeState<Clock> {
    accepting: bool,
    shutdown_started: bool,
    actors: BTreeMap<[u8; 16], CoreRecallActorEntry<Clock>>,
    transports: BTreeMap<[u8; 16], ActiveRecallTransport>,
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
            }),
        }
    }

    pub async fn register_actor(
        self: &Arc<Self>,
        authenticated: AuthenticatedAccount,
        actor: Arc<ProductionRecallIntentActor<Clock>>,
    ) -> Result<CoreRecallActorRegistration, CoreRecallRuntimeError> {
        if authenticated.namespace != AuthenticatedNamespace::WipeableTest
            || authenticated.account_id.as_bytes() != actor.account_id()
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
            account_id,
            character_id,
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

    /// Installs the new generation before returning the connection that it superseded. The caller
    /// may therefore close the old transport only after the authoritative handoff has committed.
    pub async fn attach_transport(
        &self,
        authenticated: AuthenticatedAccount,
        connection: quinn::Connection,
    ) -> Result<CoreRecallTransportAttach, CoreRecallRuntimeError> {
        let account_id = authenticated.account_id.as_bytes();
        let mut state = self.state.lock().await;
        if !state.accepting {
            return Err(CoreRecallRuntimeError::Retired);
        }
        let entry = state
            .actors
            .get(&account_id)
            .ok_or(CoreRecallRuntimeError::ActorUnavailable)?;
        if entry.authenticated != authenticated {
            return Err(CoreRecallRuntimeError::InvalidActorBinding);
        }
        let tick = self
            .tick_source
            .current_tick(account_id, entry.character_id)
            .get();
        let transport_lease = entry.lifecycle.attach_transport(tick).await?;
        let lease = CoreRecallConnectionLease {
            account_id,
            character_id: entry.character_id,
            generation: transport_lease.generation(),
        };
        let invalidated_connection = state
            .transports
            .insert(
                account_id,
                ActiveRecallTransport {
                    lease,
                    writer: Arc::new(CoreRecallReliableWriter::new(connection)),
                },
            )
            .map(|active| {
                active.writer.retire(
                    TRANSPORT_REPLACED_CLOSE_CODE,
                    b"authoritative transport handoff",
                );
                active.writer.connection().clone()
            });
        Ok(CoreRecallTransportAttach {
            lease,
            invalidated_connection,
        })
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
            .current_tick(lease.account_id, lease.character_id)
            .get();
        let outcome = entry
            .lifecycle
            .detach_transport(lease.generation, lost_tick, issued_at_unix_ms)
            .await?;
        state.transports.remove(&lease.account_id);
        Ok(outcome)
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
            zero_residue: remaining_actor_tasks == 0
                && remaining_completion_tasks == 0
                && remaining_registered_actors == 0
                && remaining_active_transports == 0,
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
    account_id: [u8; 16],
    character_id: [u8; 16],
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
                tick_source.current_tick(account_id, character_id).get()
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
        fn current_tick(&self, account_id: [u8; 16], character_id: [u8; 16]) -> NonZeroU64 {
            assert_eq!(account_id, ACCOUNT_ID);
            assert_eq!(character_id, CHARACTER_ID);
            NonZeroU64::new(self.0.load(Ordering::SeqCst)).unwrap()
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
            .register_actor(authenticated(), actor())
            .await
            .unwrap();
        let (server_endpoint, client_endpoint, client, server) = live_connection_pair().await;
        let attached = directory
            .attach_transport(authenticated(), server)
            .await
            .unwrap();
        let writer = directory.reliable_writer(attached.lease).await.unwrap();
        let (mut client_send, mut client_receive) = client.open_bi().await.unwrap();
        client_send.write_all(&[1]).await.unwrap();
        client_send.finish().unwrap();
        let (server_send, mut server_receive) = writer.connection().accept_bi().await.unwrap();
        assert_eq!(server_receive.read_to_end(1).await.unwrap(), vec![1]);
        let response = writer
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
        assert_eq!(writer.last_sequence().await, 2);

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
            .register_actor(authenticated(), actor())
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
            .register_actor(authenticated(), actor())
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
    async fn shutdown_closes_actor_authority_and_reports_zero_runtime_residue() {
        let tick_source = Arc::new(TickSource(AtomicU64::new(100)));
        let directory = Arc::new(CoreRecallActorDirectory::new(tick_source));
        directory
            .register_actor(authenticated(), actor())
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
