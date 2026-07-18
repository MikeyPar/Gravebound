//! Per-account transport ownership for the ordinary Core private-life route.
//!
//! The canonical GDD requires one server-authoritative reliable sequence and generation-safe
//! reconnect behavior (`TECH-015`, `TECH-021`-`023`). The Content Production Specification fixes
//! the closed Hall -> microrealm -> Bell Sepulcher -> Caldus route, and the Development Roadmap
//! requires the M03 loop to survive response loss and reconnect without duplicate authority.
//! A session therefore exists from handshake onward, before a danger actor or Recall channel is
//! available, and later binds those dynamic owners to the same reliable writer.

use std::{collections::BTreeMap, sync::Arc};

use thiserror::Error;
use tokio::sync::Mutex;

use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CoreRecallActorDirectory,
    CoreRecallActorRetirementReport, CoreRecallAuthoritativeTick, CoreRecallConnectionAuthority,
    CoreRecallConnectionLease, CoreRecallRuntimeError, CoreRecallRuntimeReport, CoreReliableWriter,
    ProductionRecallClock, ProductionRecallDetachOutcome, TRANSPORT_REPLACED_CLOSE_CODE,
};

const SESSION_DETACHED_CLOSE_CODE: u32 = 0x104;
const SESSION_DETACHED_REASON: &[u8] = b"private-life transport detached";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CorePrivateLifeTransportGeneration(u64);

impl CorePrivateLifeTransportGeneration {
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateLifeTransportLease {
    account_id: [u8; 16],
    generation: CorePrivateLifeTransportGeneration,
}

impl CorePrivateLifeTransportLease {
    #[must_use]
    pub const fn account_id(self) -> [u8; 16] {
        self.account_id
    }

    #[must_use]
    pub const fn generation(self) -> CorePrivateLifeTransportGeneration {
        self.generation
    }
}

#[derive(Debug)]
pub struct CorePrivateLifeTransportAttach {
    pub lease: CorePrivateLifeTransportLease,
    pub writer: Arc<CoreReliableWriter>,
    pub recall_lease: Option<CoreRecallConnectionLease>,
    pub invalidated_connection: Option<quinn::Connection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePrivateLifeTransportDetach {
    Detached {
        recall: Option<ProductionRecallDetachOutcome>,
    },
    StaleGenerationIgnored,
    PlannedShutdownIgnored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateLifeSessionSnapshot {
    pub accepting: bool,
    pub shutdown_started: bool,
    pub retained_account_count: usize,
    pub active_transport_count: usize,
    pub recall_bound_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateLifeSessionReport {
    pub retired_account_count: usize,
    pub remaining_active_transports: usize,
    pub recall: CoreRecallRuntimeReport,
    pub zero_residue: bool,
}

#[derive(Debug, Error)]
pub enum CorePrivateLifeSessionError {
    #[error("Core private-life sessions are retired")]
    Retired,
    #[error("Core private-life session binding is invalid")]
    InvalidAccountBinding,
    #[error("Core private-life transport generation overflowed")]
    GenerationExhausted,
    #[error("Core private-life session is unavailable")]
    SessionUnavailable,
    #[error("Core private-life transport generation is stale")]
    StaleTransport,
    #[error("Core private-life Recall authority is already bound")]
    RecallAlreadyBound,
    #[error("Core private-life Recall authority is not bound")]
    RecallUnavailable,
    #[error("Core private-life session shutdown has not started")]
    ShutdownNotStarted,
    #[error("Core private-life Recall runtime failed")]
    Recall(#[from] CoreRecallRuntimeError),
}

#[derive(Debug)]
struct ActiveTransport {
    lease: CorePrivateLifeTransportLease,
    writer: Arc<CoreReliableWriter>,
}

#[derive(Debug)]
struct SessionEntry {
    authenticated: AuthenticatedAccount,
    next_generation: u64,
    active: Option<ActiveTransport>,
    recall_bound: bool,
    recall_lease: Option<CoreRecallConnectionLease>,
}

#[derive(Debug)]
struct SessionState {
    accepting: bool,
    shutdown_started: bool,
    sessions: BTreeMap<[u8; 16], SessionEntry>,
}

/// Owns exactly one current transport generation and writer for each authenticated account.
/// Recall may bind after danger entry; later transport handoffs automatically rebind it before
/// the new session generation becomes visible.
pub struct CorePrivateLifeSessionDirectory<Clock, TickSource> {
    recall: Arc<CoreRecallActorDirectory<Clock, TickSource>>,
    state: Mutex<SessionState>,
}

impl<Clock, TickSource> CorePrivateLifeSessionDirectory<Clock, TickSource>
where
    Clock: ProductionRecallClock + 'static,
    TickSource: CoreRecallAuthoritativeTick + 'static,
{
    #[must_use]
    pub fn new(recall: Arc<CoreRecallActorDirectory<Clock, TickSource>>) -> Self {
        Self {
            recall,
            state: Mutex::new(SessionState {
                accepting: true,
                shutdown_started: false,
                sessions: BTreeMap::new(),
            }),
        }
    }

    /// Accepts a transport after authentication. No route or danger owner is required yet.
    /// When Recall is already bound, its writer handoff commits first so no new session can be
    /// advertised with a split reliable sequence.
    pub async fn attach_transport(
        &self,
        authenticated: AuthenticatedAccount,
        connection: quinn::Connection,
    ) -> Result<CorePrivateLifeTransportAttach, CorePrivateLifeSessionError> {
        if authenticated.namespace != AuthenticatedNamespace::WipeableTest {
            return Err(CorePrivateLifeSessionError::InvalidAccountBinding);
        }
        let account_id = authenticated.account_id.as_bytes();
        let writer = Arc::new(CoreReliableWriter::new(connection));
        let mut state = self.state.lock().await;
        if !state.accepting {
            return Err(CorePrivateLifeSessionError::Retired);
        }
        let entry = state.sessions.entry(account_id).or_insert(SessionEntry {
            authenticated,
            next_generation: 1,
            active: None,
            recall_bound: false,
            recall_lease: None,
        });
        if entry.authenticated != authenticated {
            return Err(CorePrivateLifeSessionError::InvalidAccountBinding);
        }
        let generation = CorePrivateLifeTransportGeneration(entry.next_generation);
        let next_generation = entry
            .next_generation
            .checked_add(1)
            .ok_or(CorePrivateLifeSessionError::GenerationExhausted)?;

        let recall_attach = if entry.recall_bound {
            Some(
                self.recall
                    .attach_reliable_writer(authenticated, Arc::clone(&writer))
                    .await?,
            )
        } else {
            None
        };
        let lease = CorePrivateLifeTransportLease {
            account_id,
            generation,
        };
        let previous = entry.active.replace(ActiveTransport {
            lease,
            writer: Arc::clone(&writer),
        });
        entry.next_generation = next_generation;
        entry.recall_lease = recall_attach.as_ref().map(|attached| attached.lease);

        let invalidated_connection = if let Some(attached) = recall_attach {
            attached.invalidated_connection
        } else {
            previous.map(|active| {
                active.writer.retire(
                    TRANSPORT_REPLACED_CLOSE_CODE,
                    b"authoritative private-life transport handoff",
                );
                active.writer.connection().clone()
            })
        };
        Ok(CorePrivateLifeTransportAttach {
            lease,
            writer,
            recall_lease: entry.recall_lease,
            invalidated_connection,
        })
    }

    /// Binds a newly registered danger actor to the current session writer. Hall and Character
    /// Select sessions remain legal without this binding; callers invoke it only after the live
    /// danger generation exists.
    pub async fn bind_recall(
        &self,
        lease: CorePrivateLifeTransportLease,
    ) -> Result<CoreRecallConnectionLease, CorePrivateLifeSessionError> {
        let mut state = self.state.lock().await;
        if !state.accepting {
            return Err(CorePrivateLifeSessionError::Retired);
        }
        let entry = state
            .sessions
            .get_mut(&lease.account_id)
            .ok_or(CorePrivateLifeSessionError::SessionUnavailable)?;
        let active = entry
            .active
            .as_ref()
            .ok_or(CorePrivateLifeSessionError::SessionUnavailable)?;
        if active.lease != lease {
            return Err(CorePrivateLifeSessionError::StaleTransport);
        }
        if entry.recall_bound {
            return Err(CorePrivateLifeSessionError::RecallAlreadyBound);
        }
        let attached = self
            .recall
            .attach_reliable_writer(entry.authenticated, Arc::clone(&active.writer))
            .await?;
        entry.recall_bound = true;
        entry.recall_lease = Some(attached.lease);
        Ok(attached.lease)
    }

    pub async fn writer(
        &self,
        lease: CorePrivateLifeTransportLease,
    ) -> Result<Arc<CoreReliableWriter>, CorePrivateLifeSessionError> {
        let state = self.state.lock().await;
        let active = state
            .sessions
            .get(&lease.account_id)
            .and_then(|entry| entry.active.as_ref())
            .ok_or(CorePrivateLifeSessionError::SessionUnavailable)?;
        if active.lease != lease {
            return Err(CorePrivateLifeSessionError::StaleTransport);
        }
        Ok(Arc::clone(&active.writer))
    }

    pub async fn recall_authority(
        self: &Arc<Self>,
        lease: CorePrivateLifeTransportLease,
    ) -> Result<CoreRecallConnectionAuthority<Clock, TickSource>, CorePrivateLifeSessionError> {
        let state = self.state.lock().await;
        let entry = state
            .sessions
            .get(&lease.account_id)
            .ok_or(CorePrivateLifeSessionError::SessionUnavailable)?;
        if entry.active.as_ref().map(|active| active.lease) != Some(lease) {
            return Err(CorePrivateLifeSessionError::StaleTransport);
        }
        let recall_lease = entry
            .recall_lease
            .ok_or(CorePrivateLifeSessionError::RecallUnavailable)?;
        Ok(self.recall.authority(recall_lease))
    }

    /// Removes the danger-only Recall actor without retiring the session writer. The terminal
    /// coordinator calls this only after its stored result and destination projection have been
    /// published; a later danger generation must register and bind a fresh actor explicitly.
    pub async fn unbind_recall(
        &self,
        lease: CorePrivateLifeTransportLease,
    ) -> Result<CoreRecallActorRetirementReport, CorePrivateLifeSessionError> {
        let mut state = self.state.lock().await;
        if !state.accepting {
            return Err(CorePrivateLifeSessionError::Retired);
        }
        let entry = state
            .sessions
            .get_mut(&lease.account_id)
            .ok_or(CorePrivateLifeSessionError::SessionUnavailable)?;
        if entry.active.as_ref().map(|active| active.lease) != Some(lease) {
            return Err(CorePrivateLifeSessionError::StaleTransport);
        }
        if !entry.recall_bound {
            return Err(CorePrivateLifeSessionError::RecallUnavailable);
        }
        let report = self.recall.retire_actor(entry.authenticated).await?;
        entry.recall_bound = false;
        entry.recall_lease = None;
        Ok(report)
    }

    pub async fn detach_transport(
        &self,
        lease: CorePrivateLifeTransportLease,
        issued_at_unix_ms: u64,
    ) -> Result<CorePrivateLifeTransportDetach, CorePrivateLifeSessionError> {
        let mut state = self.state.lock().await;
        if state.shutdown_started {
            return Ok(CorePrivateLifeTransportDetach::PlannedShutdownIgnored);
        }
        let Some(entry) = state.sessions.get_mut(&lease.account_id) else {
            return Ok(CorePrivateLifeTransportDetach::StaleGenerationIgnored);
        };
        if entry.active.as_ref().map(|active| active.lease) != Some(lease) {
            return Ok(CorePrivateLifeTransportDetach::StaleGenerationIgnored);
        }
        let recall = if let Some(recall_lease) = entry.recall_lease.take() {
            Some(
                self.recall
                    .detach_transport(recall_lease, issued_at_unix_ms)
                    .await?,
            )
        } else {
            None
        };
        if let Some(active) = entry.active.take() {
            active
                .writer
                .retire(SESSION_DETACHED_CLOSE_CODE, SESSION_DETACHED_REASON);
        }
        Ok(CorePrivateLifeTransportDetach::Detached { recall })
    }

    /// Stops admission and retires every writer before Recall inbox shutdown. Duplicate
    /// connection handles are harmless and let the caller close all aggregate owners uniformly.
    pub async fn begin_shutdown(&self) -> Vec<quinn::Connection> {
        let mut state = self.state.lock().await;
        state.accepting = false;
        state.shutdown_started = true;
        let mut connections = Vec::new();
        for entry in state.sessions.values_mut() {
            if let Some(active) = entry.active.take() {
                active.writer.retire(
                    crate::SERVER_SHUTDOWN_CLOSE_CODE,
                    b"private-life server shutdown",
                );
                connections.push(active.writer.connection().clone());
            }
            entry.recall_lease = None;
        }
        drop(state);
        connections.extend(self.recall.begin_shutdown().await);
        connections
    }

    pub async fn finish_shutdown(
        &self,
    ) -> Result<CorePrivateLifeSessionReport, CorePrivateLifeSessionError> {
        {
            let state = self.state.lock().await;
            if !state.shutdown_started {
                return Err(CorePrivateLifeSessionError::ShutdownNotStarted);
            }
        }
        let recall = self.recall.finish_shutdown().await?;
        let mut state = self.state.lock().await;
        let retired_account_count = state.sessions.len();
        let remaining_active_transports = state
            .sessions
            .values()
            .filter(|entry| entry.active.is_some())
            .count();
        state.sessions.clear();
        Ok(CorePrivateLifeSessionReport {
            retired_account_count,
            remaining_active_transports,
            recall,
            zero_residue: remaining_active_transports == 0 && recall.zero_residue,
        })
    }

    #[must_use]
    pub async fn snapshot(&self) -> CorePrivateLifeSessionSnapshot {
        let state = self.state.lock().await;
        CorePrivateLifeSessionSnapshot {
            accepting: state.accepting,
            shutdown_started: state.shutdown_started,
            retained_account_count: state.sessions.len(),
            active_transport_count: state
                .sessions
                .values()
                .filter(|entry| entry.active.is_some())
                .count(),
            recall_bound_count: state
                .sessions
                .values()
                .filter(|entry| entry.recall_bound)
                .count(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        num::NonZeroU64,
        sync::atomic::{AtomicU64, Ordering},
    };

    use protocol::{
        ActionResultCode, RecallFrameV1, RecallIntentV1, RecallResultV1, ReliableEvent,
        TERMINAL_INVENTORY_SCHEMA_VERSION,
    };
    use rcgen::generate_simple_self_signed;
    use rustls::pki_types::PrivatePkcs8KeyDer;

    use super::*;
    use crate::{
        AccountId, CoreRecallIntentAuthority, ProductionRecallIntentActor,
        ProductionRecallPendingAuthorityV1,
    };

    const ACCOUNT_ID: [u8; 16] = [71; 16];
    const CHARACTER_ID: [u8; 16] = [72; 16];

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

    fn recall_frame(sequence: u32) -> RecallFrameV1 {
        RecallFrameV1 {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            sequence,
            character_id: CHARACTER_ID,
            client_tick: 10,
            intent: RecallIntentV1::Start,
        }
    }

    async fn live_connection_pair() -> (
        quinn::Endpoint,
        quinn::Endpoint,
        quinn::Connection,
        quinn::Connection,
    ) {
        let rcgen::CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
        let certificate = cert.der().clone();
        let private_key = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
        let server_config =
            quinn::ServerConfig::with_single_cert(vec![certificate.clone()], private_key.into())
                .unwrap();
        let server_endpoint =
            quinn::Endpoint::server(server_config, "127.0.0.1:0".parse().unwrap()).unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate).unwrap();
        let client_config = quinn::ClientConfig::with_root_certificates(Arc::new(roots)).unwrap();
        let mut client_endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        client_endpoint.set_default_client_config(client_config);
        let connecting = client_endpoint
            .connect(server_endpoint.local_addr().unwrap(), "localhost")
            .unwrap();
        let incoming = server_endpoint.accept().await.unwrap();
        let (client, server) = tokio::join!(connecting, incoming);
        (
            server_endpoint,
            client_endpoint,
            client.unwrap(),
            server.unwrap(),
        )
    }

    async fn write_response(
        client: &quinn::Connection,
        writer: &CoreReliableWriter,
    ) -> protocol::ReliableEventFrame {
        let (mut client_send, mut client_receive) = client.open_bi().await.unwrap();
        client_send.write_all(&[1]).await.unwrap();
        client_send.finish().unwrap();
        let (server_send, mut server_receive) = writer.connection().accept_bi().await.unwrap();
        assert_eq!(server_receive.read_to_end(1).await.unwrap(), vec![1]);
        let frame = writer
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
        let bytes = client_receive
            .read_to_end(protocol::RELIABLE_FRAME_LIMIT)
            .await
            .unwrap();
        let protocol::WireMessage::ReliableEvent(received) =
            protocol::decode_frame(&bytes).unwrap()
        else {
            panic!("expected reliable response");
        };
        assert_eq!(received, frame);
        frame
    }

    #[tokio::test]
    async fn handshake_session_binds_recall_to_the_existing_writer_and_sequence() {
        let ticks = Arc::new(TickSource(AtomicU64::new(100)));
        let recall = Arc::new(CoreRecallActorDirectory::new(ticks));
        let sessions = Arc::new(CorePrivateLifeSessionDirectory::new(Arc::clone(&recall)));
        let (server_endpoint, client_endpoint, client, server) = live_connection_pair().await;

        let attached = sessions
            .attach_transport(authenticated(), server)
            .await
            .unwrap();
        assert_eq!(
            write_response(&client, attached.writer.as_ref())
                .await
                .sequence,
            1
        );
        recall
            .register_actor(authenticated(), actor())
            .await
            .unwrap();
        let recall_lease = sessions.bind_recall(attached.lease).await.unwrap();
        let recall_writer = recall.reliable_writer(recall_lease).await.unwrap();
        assert!(Arc::ptr_eq(&attached.writer, &recall_writer));

        let authority = Arc::clone(&sessions)
            .recall_authority(attached.lease)
            .await
            .unwrap();
        assert!(matches!(
            authority
                .handle_recall(authenticated(), &recall_frame(1), 0)
                .await
                .result,
            RecallResultV1::Pending {
                started_tick: 100,
                ..
            }
        ));
        let pushed = attached
            .writer
            .send_event(
                112,
                ReliableEvent::ActionResult {
                    action_sequence: 10,
                    code: ActionResultCode::Accepted,
                },
            )
            .await
            .unwrap();
        assert_eq!(pushed.sequence, 2);
        assert_eq!(
            bot_client::receive_server_reliable(&client).await.unwrap(),
            pushed
        );

        for connection in sessions.begin_shutdown().await {
            connection.close(0_u32.into(), b"test shutdown");
        }
        let report = sessions.finish_shutdown().await.unwrap();
        assert_eq!(report.retired_account_count, 1);
        assert!(report.zero_residue);
        client.close(0_u32.into(), b"test complete");
        server_endpoint.wait_idle().await;
        client_endpoint.wait_idle().await;
    }

    #[tokio::test]
    async fn handoff_rebinds_recall_before_visibility_and_stale_detach_is_harmless() {
        let ticks = Arc::new(TickSource(AtomicU64::new(100)));
        let recall = Arc::new(CoreRecallActorDirectory::new(Arc::clone(&ticks)));
        let sessions = Arc::new(CorePrivateLifeSessionDirectory::new(Arc::clone(&recall)));
        recall
            .register_actor(authenticated(), actor())
            .await
            .unwrap();
        let (first_server_endpoint, first_client_endpoint, first_client, first_server) =
            live_connection_pair().await;
        let first = sessions
            .attach_transport(authenticated(), first_server)
            .await
            .unwrap();
        sessions.bind_recall(first.lease).await.unwrap();
        let old_authority = Arc::clone(&sessions)
            .recall_authority(first.lease)
            .await
            .unwrap();

        ticks.0.store(101, Ordering::SeqCst);
        let (second_server_endpoint, second_client_endpoint, second_client, second_server) =
            live_connection_pair().await;
        let second = sessions
            .attach_transport(authenticated(), second_server)
            .await
            .unwrap();
        assert!(second.recall_lease.is_some());
        assert!(second.invalidated_connection.is_some());
        assert!(!first.writer.is_available());
        tokio::time::timeout(std::time::Duration::from_secs(5), first_client.closed())
            .await
            .unwrap();
        assert!(matches!(
            sessions.detach_transport(first.lease, 5_500).await.unwrap(),
            CorePrivateLifeTransportDetach::StaleGenerationIgnored
        ));
        assert!(matches!(
            old_authority
                .handle_recall(authenticated(), &recall_frame(1), 0)
                .await
                .result,
            RecallResultV1::Rejected { .. }
        ));
        let new_authority = Arc::clone(&sessions)
            .recall_authority(second.lease)
            .await
            .unwrap();
        assert!(matches!(
            new_authority
                .handle_recall(authenticated(), &recall_frame(1), 0)
                .await
                .result,
            RecallResultV1::Pending {
                started_tick: 101,
                ..
            }
        ));

        ticks.0.store(102, Ordering::SeqCst);
        assert!(matches!(
            sessions
                .detach_transport(second.lease, 6_000)
                .await
                .unwrap(),
            CorePrivateLifeTransportDetach::Detached {
                recall: Some(ProductionRecallDetachOutcome::LinkLostStarted { deadline_tick: 192 })
            }
        ));
        assert_eq!(sessions.snapshot().await.active_transport_count, 0);

        ticks.0.store(191, Ordering::SeqCst);
        let (third_server_endpoint, third_client_endpoint, third_client, third_server) =
            live_connection_pair().await;
        let third = sessions
            .attach_transport(authenticated(), third_server)
            .await
            .unwrap();
        assert!(third.recall_lease.is_some());
        assert_eq!(third.lease.generation().get(), 3);
        assert_eq!(sessions.snapshot().await.active_transport_count, 1);

        let retired = sessions.unbind_recall(third.lease).await.unwrap();
        assert!(retired.detached_transport_binding);
        assert!(retired.zero_residue);
        assert!(third.writer.is_available());
        assert_eq!(sessions.snapshot().await.recall_bound_count, 0);
        recall
            .register_actor(authenticated(), actor())
            .await
            .unwrap();
        sessions.bind_recall(third.lease).await.unwrap();
        assert_eq!(sessions.snapshot().await.recall_bound_count, 1);

        for connection in sessions.begin_shutdown().await {
            connection.close(0_u32.into(), b"test shutdown");
        }
        assert!(sessions.finish_shutdown().await.unwrap().zero_residue);
        second_client.close(0_u32.into(), b"test complete");
        third_client.close(0_u32.into(), b"test complete");
        first_server_endpoint.wait_idle().await;
        first_client_endpoint.wait_idle().await;
        second_server_endpoint.wait_idle().await;
        second_client_endpoint.wait_idle().await;
        third_server_endpoint.wait_idle().await;
        third_client_endpoint.wait_idle().await;
    }
}
