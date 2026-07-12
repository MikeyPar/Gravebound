//! Runnable local QUIC orchestration for the `GB-M02-GATE` playtest build.
//!
//! This module owns transport and scheduling only. Gameplay authority remains in
//! [`InstanceScheduler`], and every gameplay value still comes from validated `fp.1.0.0` data.

use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use protocol::{
    ClientHello, ControlEvent, HandshakeResponse, M02_LOCAL_BUILD_ID, M02_LOCAL_REGION_ID,
    M02_LOCAL_SERVER_NAME, ManifestHash, ProtocolVersion, RELIABLE_FRAME_LIMIT, ReliableEvent,
    ReliableEventFrame, SIMULATION_HZ, SessionControlResultCode, WireMessage, WireText,
    decode_frame, encode_frame,
};
use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use thiserror::Error;
use tokio::{sync::Mutex, task::JoinSet, time::MissedTickBehavior};
use tracing::{debug, info, warn};

use crate::{
    AdmissionState, AuthenticationDecision, HandshakePolicy, InstanceError, InstanceScheduler,
    SERVER_SHUTDOWN_CLOSE_CODE, SessionOwnerId, TransportId, close_transport,
};

pub const LOCAL_BUILD_ID: &str = M02_LOCAL_BUILD_ID;
pub const LOCAL_REGION_ID: &str = M02_LOCAL_REGION_ID;
pub const LOCAL_SERVER_NAME: &str = M02_LOCAL_SERVER_NAME;
const LOCAL_FEATURE_FLAG: &str = "m02-local-runtime";
#[allow(clippy::cast_lossless)] // `From::from` is not const-stable for this conversion.
const TICK_NANOS: u64 = 1_000_000_000 / SIMULATION_HZ as u64;

#[derive(Debug, Clone)]
pub struct LocalServerConfig {
    pub bind_address: SocketAddr,
    pub content_root: PathBuf,
}

impl Default for LocalServerConfig {
    fn default() -> Self {
        Self {
            bind_address: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 50_000),
            content_root: PathBuf::from("content"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalServerReport {
    pub accepted_connections: u64,
    pub rejected_connections: u64,
    pub malformed_messages: u64,
    pub dropped_snapshots: u64,
    pub scheduler_frames: u64,
    pub admitted_sessions: u64,
    pub retired_sessions: u64,
    pub zero_residue: bool,
}

#[derive(Debug)]
struct TransportRoute {
    transport: TransportId,
    connection: quinn::Connection,
}

#[derive(Debug, Default)]
struct RuntimeDiagnostics {
    accepted_connections: u64,
    rejected_connections: u64,
    malformed_messages: u64,
    dropped_snapshots: u64,
}

#[derive(Debug)]
struct RuntimeState {
    scheduler: InstanceScheduler,
    content_root: PathBuf,
    started: Instant,
    owners_by_ticket_hash: BTreeMap<[u8; 32], SessionOwnerId>,
    routes_by_owner: BTreeMap<SessionOwnerId, TransportRoute>,
    connections_by_transport: BTreeMap<TransportId, quinn::Connection>,
    next_owner_id: u64,
    next_transport_id: u64,
    diagnostics: RuntimeDiagnostics,
}

impl RuntimeState {
    fn new(content_root: PathBuf) -> Self {
        Self {
            scheduler: InstanceScheduler::default(),
            content_root,
            started: Instant::now(),
            owners_by_ticket_hash: BTreeMap::new(),
            routes_by_owner: BTreeMap::new(),
            connections_by_transport: BTreeMap::new(),
            next_owner_id: 1,
            next_transport_id: 1,
            diagnostics: RuntimeDiagnostics::default(),
        }
    }

    fn reserve_identity(
        &mut self,
        hello: &ClientHello,
    ) -> Result<ConnectionIdentity, LocalServerRuntimeError> {
        let ticket_hash = *blake3::hash(hello.auth_ticket.expose_for_validation()).as_bytes();
        let owner = if let Some(owner) = self.owners_by_ticket_hash.get(&ticket_hash).copied() {
            owner
        } else {
            let owner = SessionOwnerId::new(self.next_owner_id)?;
            self.next_owner_id = self
                .next_owner_id
                .checked_add(1)
                .ok_or(LocalServerRuntimeError::IdentityExhausted)?;
            self.owners_by_ticket_hash.insert(ticket_hash, owner);
            owner
        };
        let transport = TransportId::new(self.next_transport_id)?;
        self.next_transport_id = self
            .next_transport_id
            .checked_add(1)
            .ok_or(LocalServerRuntimeError::IdentityExhausted)?;
        Ok(ConnectionIdentity { owner, transport })
    }

    fn monotonic_micros(&self) -> Result<u64, LocalServerRuntimeError> {
        u64::try_from(self.started.elapsed().as_micros())
            .map_err(|_| LocalServerRuntimeError::ClockOverflow)
    }

    fn handle_reliable(
        &mut self,
        identity: ConnectionIdentity,
        connection: &quinn::Connection,
        message: WireMessage,
    ) -> Result<ReliableDispatch, LocalServerRuntimeError> {
        let (response, invalidated, accepted_control) = match message {
            WireMessage::SessionControlFrame(frame) => {
                let control = self.scheduler.admit_or_route_control(
                    identity.owner,
                    identity.transport,
                    &frame,
                    &self.content_root,
                    self.monotonic_micros()?,
                )?;
                let accepted = control_code(&control.lifecycle.event).is_some_and(|code| {
                    matches!(
                        code,
                        SessionControlResultCode::Joined | SessionControlResultCode::Reattached
                    )
                });
                (
                    WireMessage::ReliableEvent(control.lifecycle.event),
                    control.lifecycle.invalidated_transport,
                    accepted,
                )
            }
            gameplay => (
                self.scheduler.handle_gameplay_reliable(
                    identity.owner,
                    identity.transport,
                    gameplay,
                )?,
                None,
                false,
            ),
        };

        let invalidated_connection = invalidated.and_then(|transport| {
            let old = self.connections_by_transport.remove(&transport);
            if old.is_some()
                && self
                    .routes_by_owner
                    .get(&identity.owner)
                    .is_some_and(|route| route.transport == transport)
            {
                self.routes_by_owner.remove(&identity.owner);
            }
            old
        });
        if accepted_control {
            self.connections_by_transport
                .insert(identity.transport, connection.clone());
            self.routes_by_owner.insert(
                identity.owner,
                TransportRoute {
                    transport: identity.transport,
                    connection: connection.clone(),
                },
            );
        }
        Ok(ReliableDispatch {
            response,
            invalidated_connection,
        })
    }

    fn handle_input(
        &mut self,
        identity: ConnectionIdentity,
        bytes: &[u8],
    ) -> Result<(), LocalServerRuntimeError> {
        let message = match decode_frame(bytes) {
            Ok(message) => message,
            Err(error) => {
                self.diagnostics.malformed_messages =
                    self.diagnostics.malformed_messages.saturating_add(1);
                return Err(error.into());
            }
        };
        let WireMessage::InputFrame(frame) = message else {
            self.diagnostics.malformed_messages =
                self.diagnostics.malformed_messages.saturating_add(1);
            return Err(LocalServerRuntimeError::UnexpectedDatagram);
        };
        self.scheduler
            .submit_input(identity.owner, identity.transport, &frame)?;
        Ok(())
    }

    fn tick_and_dispatch(&mut self) -> Result<(), LocalServerRuntimeError> {
        let frame = self.scheduler.tick()?;
        for batch in frame.snapshot_batches {
            let Some(route) = self.routes_by_owner.get(&batch.owner) else {
                continue;
            };
            for snapshot in batch.snapshots {
                let encoded = encode_frame(&WireMessage::SnapshotChunk(snapshot))?;
                if route.connection.send_datagram(encoded.into()).is_err() {
                    self.diagnostics.dropped_snapshots =
                        self.diagnostics.dropped_snapshots.saturating_add(1);
                }
            }
        }
        let routed_owners = self
            .routes_by_owner
            .keys()
            .copied()
            .collect::<BTreeSet<_>>();
        let retired = self.scheduler.retire_resolved_excluding(&routed_owners)?;
        for owner in retired {
            if let Some(route) = self.routes_by_owner.remove(&owner) {
                self.connections_by_transport.remove(&route.transport);
            }
        }
        Ok(())
    }

    fn detach_transport(&mut self, identity: ConnectionIdentity) {
        self.connections_by_transport.remove(&identity.transport);
        let was_active = self
            .routes_by_owner
            .get(&identity.owner)
            .is_some_and(|route| route.transport == identity.transport);
        if was_active {
            self.routes_by_owner.remove(&identity.owner);
            if let Err(error) = self
                .scheduler
                .transport_lost(identity.owner, identity.transport)
            {
                debug!(%error, "transport was already resolved while disconnecting");
            }
        }
    }

    fn begin_shutdown(
        &mut self,
    ) -> Result<Vec<(quinn::Connection, ReliableEventFrame)>, LocalServerRuntimeError> {
        let events = self.scheduler.begin_shutdown()?;
        Ok(events
            .into_iter()
            .filter_map(|(_, transport, event)| {
                self.connections_by_transport
                    .get(&transport)
                    .cloned()
                    .map(|connection| (connection, event))
            })
            .collect())
    }

    fn finish_shutdown(&mut self) -> Result<LocalServerReport, LocalServerRuntimeError> {
        self.scheduler.finish_shutdown()?;
        self.routes_by_owner.clear();
        self.connections_by_transport.clear();
        Ok(LocalServerReport {
            accepted_connections: self.diagnostics.accepted_connections,
            rejected_connections: self.diagnostics.rejected_connections,
            malformed_messages: self.diagnostics.malformed_messages,
            dropped_snapshots: self.diagnostics.dropped_snapshots,
            scheduler_frames: self.scheduler.diagnostics().scheduler_frames,
            admitted_sessions: self.scheduler.diagnostics().admissions,
            retired_sessions: self.scheduler.diagnostics().retired_sessions,
            zero_residue: self.scheduler.instance_count() == 0
                && self.scheduler.owner_count() == 0
                && self.routes_by_owner.is_empty()
                && self.connections_by_transport.is_empty(),
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct ConnectionIdentity {
    owner: SessionOwnerId,
    transport: TransportId,
}

#[derive(Debug)]
struct ReliableDispatch {
    response: WireMessage,
    invalidated_connection: Option<quinn::Connection>,
}

#[derive(Debug)]
pub struct BoundLocalServer {
    endpoint: quinn::Endpoint,
    certificate: CertificateDer<'static>,
    local_address: SocketAddr,
    policy: HandshakePolicy,
    state: Arc<Mutex<RuntimeState>>,
}

impl BoundLocalServer {
    pub fn bind(config: LocalServerConfig) -> Result<Self, LocalServerRuntimeError> {
        let (_, report) = sim_content::load_and_validate(&config.content_root)
            .map_err(|error| LocalServerRuntimeError::Content(error.to_string()))?;
        if report.content_version != "fp.1.0.0" {
            return Err(LocalServerRuntimeError::ContentVersion(
                report.content_version,
            ));
        }
        let CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(vec![LOCAL_SERVER_NAME.to_owned()])?;
        let certificate = cert.der().clone();
        let private_key = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
        let server_config =
            quinn::ServerConfig::with_single_cert(vec![certificate.clone()], private_key.into())?;
        let endpoint = quinn::Endpoint::server(server_config, config.bind_address)?;
        let local_address = endpoint.local_addr()?;
        let policy = HandshakePolicy {
            required_protocol: ProtocolVersion::current(),
            required_client_build: WireText::new(LOCAL_BUILD_ID)?,
            required_manifest_hash: ManifestHash::new(report.package_hash_blake3)?,
            content_bundle_version: WireText::new(report.content_version)?,
            region_id: WireText::new(LOCAL_REGION_ID)?,
            feature_flags: vec![WireText::new(LOCAL_FEATURE_FLAG)?],
            admission: AdmissionState::Available,
        };
        Ok(Self {
            endpoint,
            certificate,
            local_address,
            policy,
            state: Arc::new(Mutex::new(RuntimeState::new(config.content_root))),
        })
    }

    #[must_use]
    pub const fn local_address(&self) -> SocketAddr {
        self.local_address
    }

    #[must_use]
    pub fn certificate_der(&self) -> &[u8] {
        self.certificate.as_ref()
    }

    pub async fn serve_until<F>(
        self,
        shutdown: F,
    ) -> Result<LocalServerReport, LocalServerRuntimeError>
    where
        F: Future<Output = ()>,
    {
        info!(address = %self.local_address, feature_id = "GB-M02-GATE", "local QUIC server ready");
        let mut interval = tokio::time::interval(Duration::from_nanos(TICK_NANOS));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        interval.tick().await;
        let mut workers = JoinSet::new();
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                biased;
                () = &mut shutdown => break,
                _ = interval.tick() => {
                    self.state.lock().await.tick_and_dispatch()?;
                }
                incoming = self.endpoint.accept() => {
                    let Some(incoming) = incoming else { break };
                    let state = Arc::clone(&self.state);
                    let policy = self.policy.clone();
                    workers.spawn(async move {
                        if let Err(error) = serve_connection(incoming, state, policy).await {
                            warn!(%error, "local playtest connection ended");
                        }
                    });
                }
                completed = workers.join_next(), if !workers.is_empty() => {
                    if let Some(Err(error)) = completed {
                        warn!(%error, "local connection task panicked or was cancelled");
                    }
                }
            }
        }

        let shutdown_events = self.state.lock().await.begin_shutdown()?;
        for (connection, event) in shutdown_events {
            if let Err(error) = send_server_event(&connection, &event).await {
                debug!(%error, "shutdown event could not be delivered");
            }
            close_transport(
                &connection,
                SERVER_SHUTDOWN_CLOSE_CODE,
                b"local server shutdown",
            );
        }
        self.endpoint
            .close(SERVER_SHUTDOWN_CLOSE_CODE.into(), b"local server shutdown");
        while workers.join_next().await.is_some() {}
        self.endpoint.wait_idle().await;
        self.state.lock().await.finish_shutdown()
    }
}

async fn serve_connection(
    incoming: quinn::Incoming,
    state: Arc<Mutex<RuntimeState>>,
    policy: HandshakePolicy,
) -> Result<(), LocalServerRuntimeError> {
    let connection = incoming.await?;
    let Some(identity) = perform_handshake(&connection, &state, &policy).await? else {
        return Ok(());
    };
    let result = run_connection_loop(&connection, &state, identity).await;
    state.lock().await.detach_transport(identity);
    result
}

async fn run_connection_loop(
    connection: &quinn::Connection,
    state: &Arc<Mutex<RuntimeState>>,
    identity: ConnectionIdentity,
) -> Result<(), LocalServerRuntimeError> {
    loop {
        tokio::select! {
            datagram = connection.read_datagram() => {
                match datagram {
                    Ok(bytes) => {
                        if let Err(error) = state.lock().await.handle_input(identity, &bytes) {
                            debug!(%error, owner = identity.owner.get(), "input datagram rejected");
                        }
                    }
                    Err(_) => break,
                }
            }
            stream = connection.accept_bi() => {
                let Ok((mut send, mut receive)) = stream else { break };
                let request = receive.read_to_end(RELIABLE_FRAME_LIMIT).await?;
                let message = match decode_frame(&request) {
                    Ok(message) => message,
                    Err(error) => {
                        let mut state = state.lock().await;
                        state.diagnostics.malformed_messages =
                            state.diagnostics.malformed_messages.saturating_add(1);
                        return Err(error.into());
                    }
                };
                let dispatch = state
                    .lock()
                    .await
                    .handle_reliable(identity, connection, message)?;
                send.write_all(&encode_frame(&dispatch.response)?).await?;
                send.finish()?;
                if let Some(old) = dispatch.invalidated_connection {
                    close_transport(&old, crate::TRANSPORT_REPLACED_CLOSE_CODE, b"new transport accepted");
                }
            }
        }
    }
    Ok(())
}

async fn perform_handshake(
    connection: &quinn::Connection,
    state: &Arc<Mutex<RuntimeState>>,
    policy: &HandshakePolicy,
) -> Result<Option<ConnectionIdentity>, LocalServerRuntimeError> {
    let (mut send, mut receive) = connection.accept_bi().await?;
    let request = receive.read_to_end(RELIABLE_FRAME_LIMIT).await?;
    let WireMessage::ClientHello(hello) = decode_frame(&request)? else {
        return Err(LocalServerRuntimeError::UnexpectedHandshake);
    };
    let response = policy.evaluate(
        &hello,
        AuthenticationDecision::Accepted,
        WireText::new("pending-local-session")?,
    );
    send.write_all(&encode_frame(&WireMessage::HandshakeResponse(
        response.clone(),
    ))?)
    .await?;
    send.finish()?;
    let mut state = state.lock().await;
    match response {
        HandshakeResponse::Accepted(_) => {
            let identity = state.reserve_identity(&hello)?;
            state.diagnostics.accepted_connections =
                state.diagnostics.accepted_connections.saturating_add(1);
            Ok(Some(identity))
        }
        HandshakeResponse::Rejected(_) => {
            state.diagnostics.rejected_connections =
                state.diagnostics.rejected_connections.saturating_add(1);
            Ok(None)
        }
    }
}

async fn send_server_event(
    connection: &quinn::Connection,
    event: &ReliableEventFrame,
) -> Result<(), LocalServerRuntimeError> {
    let mut send = connection.open_uni().await?;
    send.write_all(&encode_frame(&WireMessage::ReliableEvent(event.clone()))?)
        .await?;
    send.finish()?;
    Ok(())
}

fn control_code(event: &ReliableEventFrame) -> Option<SessionControlResultCode> {
    let ReliableEvent::Control(ControlEvent::SessionResult(result)) = &event.event else {
        return None;
    };
    Some(result.code)
}

#[derive(Debug, Error)]
pub enum LocalServerRuntimeError {
    #[error("local server content validation failed: {0}")]
    Content(String),
    #[error("local server requires fp.1.0.0, received {0}")]
    ContentVersion(String),
    #[error("local server identity space exhausted")]
    IdentityExhausted,
    #[error("local server monotonic clock overflowed")]
    ClockOverflow,
    #[error("local server received a non-hello handshake message")]
    UnexpectedHandshake,
    #[error("local server received a non-input datagram")]
    UnexpectedDatagram,
    #[error("local server QUIC transport failed: {0}")]
    Quic(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Rcgen(#[from] rcgen::Error),
    #[error(transparent)]
    Rustls(#[from] rustls::Error),
    #[error(transparent)]
    Bounded(#[from] protocol::BoundedValueError),
    #[error(transparent)]
    Codec(#[from] protocol::WireCodecError),
    #[error(transparent)]
    Instance(#[from] InstanceError),
    #[error(transparent)]
    Lifecycle(#[from] crate::LifecycleError),
    #[error(transparent)]
    Connection(#[from] quinn::ConnectionError),
    #[error(transparent)]
    Read(#[from] quinn::ReadToEndError),
    #[error(transparent)]
    Write(#[from] quinn::WriteError),
    #[error(transparent)]
    ClosedStream(#[from] quinn::ClosedStream),
}

impl From<quinn::ConnectError> for LocalServerRuntimeError {
    fn from(error: quinn::ConnectError) -> Self {
        Self::Quic(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc};

    use protocol::{
        ActionFrame, ActionKind, AuthTicket, Compression, ENTITY_STATE_ALIVE, InputFrame, Platform,
        SessionControlFrame, SessionControlRequest, SessionDestination,
    };
    use tokio::sync::oneshot;

    use super::*;

    fn content_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn hello(content_root: &Path) -> ClientHello {
        hello_for(content_root, b"runtime-integration-player")
    }

    fn hello_for(content_root: &Path, ticket: &[u8]) -> ClientHello {
        let (_, report) = sim_content::load_and_validate(content_root).unwrap();
        ClientHello {
            protocol_major: ProtocolVersion::current().major,
            protocol_minor: ProtocolVersion::current().minor,
            client_build_id: WireText::new(LOCAL_BUILD_ID).unwrap(),
            platform: Platform::WindowsNative,
            supported_compression: vec![Compression::None],
            content_manifest_hash: ManifestHash::new(report.package_hash_blake3).unwrap(),
            auth_ticket: AuthTicket::new(ticket.to_vec()).unwrap(),
            locale: WireText::new("en-US").unwrap(),
        }
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)] // One lifecycle test keeps terminal routing evidence explicit.
    async fn runnable_server_routes_real_quic_and_shuts_down_without_residue() {
        let content_root = content_root();
        let server = BoundLocalServer::bind(LocalServerConfig {
            bind_address: "127.0.0.1:0".parse().unwrap(),
            content_root: content_root.clone(),
        })
        .unwrap();
        let address = server.local_address();
        let certificate = CertificateDer::from(server.certificate_der().to_vec());
        let (shutdown_send, shutdown_receive) = oneshot::channel::<()>();
        let server_task = tokio::spawn(server.serve_until(async {
            let _ = shutdown_receive.await;
        }));

        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate).unwrap();
        let client_config = quinn::ClientConfig::with_root_certificates(Arc::new(roots)).unwrap();
        let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        endpoint.set_default_client_config(client_config);
        let connection = endpoint
            .connect(address, LOCAL_SERVER_NAME)
            .unwrap()
            .await
            .unwrap();
        let handshake = bot_client::perform_handshake(&connection, hello(&content_root))
            .await
            .unwrap();
        assert!(matches!(handshake, HandshakeResponse::Accepted(_)));
        let (_, joined) = bot_client::perform_session_control(
            &connection,
            SessionControlFrame {
                sequence: 1,
                client_tick: 0,
                client_monotonic_micros: 1,
                request: SessionControlRequest::Join,
            },
        )
        .await
        .unwrap();
        assert_eq!(joined.code, SessionControlResultCode::Joined);

        bot_client::send_input_datagram(
            &connection,
            InputFrame {
                sequence: 1,
                client_tick: 1,
                movement_x_milli: 1_000,
                movement_y_milli: 0,
                aim_x_milli: 1_000,
                aim_y_milli: 0,
                held_primary: true,
                primary_sequence: 1,
                ability_1_sequence: 0,
                ability_2_sequence: 0,
            },
        )
        .unwrap();
        let snapshot = tokio::time::timeout(
            Duration::from_secs(2),
            bot_client::receive_snapshot_datagram(&connection),
        )
        .await
        .expect("server emitted a snapshot before timeout")
        .unwrap();
        assert_eq!(snapshot.acknowledged_input_sequence, 1);
        assert!(
            snapshot
                .entities
                .iter()
                .any(|entity| entity.entity_id == 10_000)
        );

        let recall = bot_client::perform_reliable_gameplay(
            &connection,
            WireMessage::ActionFrame(ActionFrame {
                sequence: 1,
                client_tick: snapshot.server_tick,
                action: ActionKind::RecallStart,
            }),
        )
        .await
        .unwrap();
        assert!(matches!(
            recall.event,
            ReliableEvent::ActionResult {
                code: protocol::ActionResultCode::Accepted,
                ..
            }
        ));
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let terminal = bot_client::receive_snapshot_datagram(&connection)
                    .await
                    .unwrap();
                if terminal.entities.iter().any(|entity| {
                    entity.entity_id == protocol::M02_ISOLATED_PLAYER_ENTITY_ID
                        && entity.state_flags & ENTITY_STATE_ALIVE == 0
                }) {
                    break;
                }
            }
        })
        .await
        .expect("terminal Recall snapshot arrived");
        let (_, terminal) = bot_client::perform_session_control(
            &connection,
            SessionControlFrame {
                sequence: 2,
                client_tick: snapshot.server_tick,
                client_monotonic_micros: 2,
                request: SessionControlRequest::Join,
            },
        )
        .await
        .unwrap();
        assert_eq!(terminal.code, SessionControlResultCode::Reattached);
        assert_eq!(terminal.destination, SessionDestination::LanternHalls);

        shutdown_send.send(()).unwrap();
        let report = server_task.await.unwrap().unwrap();
        assert_eq!(report.accepted_connections, 1);
        assert_eq!(report.admitted_sessions, 1);
        assert!(report.scheduler_frames > 0);
        assert_eq!(report.malformed_messages, 0);
        assert!(report.zero_residue);
        endpoint.wait_idle().await;
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)] // Four independent routes stay explicit for leakage review.
    async fn four_concurrent_clients_receive_only_their_isolated_authority_streams() {
        let content_root = content_root();
        let server = BoundLocalServer::bind(LocalServerConfig {
            bind_address: "127.0.0.1:0".parse().unwrap(),
            content_root: content_root.clone(),
        })
        .unwrap();
        let address = server.local_address();
        let certificate = CertificateDer::from(server.certificate_der().to_vec());
        let (shutdown_send, shutdown_receive) = oneshot::channel::<()>();
        let server_task = tokio::spawn(server.serve_until(async {
            let _ = shutdown_receive.await;
        }));

        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate).unwrap();
        let client_config = quinn::ClientConfig::with_root_certificates(Arc::new(roots)).unwrap();
        let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        endpoint.set_default_client_config(client_config);
        let mut clients = Vec::new();
        for ordinal in 1_u8..=4 {
            let connection = endpoint
                .connect(address, LOCAL_SERVER_NAME)
                .unwrap()
                .await
                .unwrap();
            let handshake = bot_client::perform_handshake(
                &connection,
                hello_for(&content_root, &[b'p', ordinal]),
            )
            .await
            .unwrap();
            assert!(matches!(handshake, HandshakeResponse::Accepted(_)));
            let (_, joined) = bot_client::perform_session_control(
                &connection,
                SessionControlFrame {
                    sequence: 1,
                    client_tick: 0,
                    client_monotonic_micros: u64::from(ordinal),
                    request: SessionControlRequest::Join,
                },
            )
            .await
            .unwrap();
            assert_eq!(joined.code, SessionControlResultCode::Joined);
            clients.push(connection);
        }

        let directions = [(1_000, 0), (-1_000, 0), (0, -1_000), (0, 1_000)];
        for (connection, (x, y)) in clients.iter().zip(directions) {
            bot_client::send_input_datagram(
                connection,
                InputFrame {
                    sequence: 1,
                    client_tick: 1,
                    movement_x_milli: x,
                    movement_y_milli: y,
                    aim_x_milli: 1_000,
                    aim_y_milli: 0,
                    held_primary: false,
                    primary_sequence: 0,
                    ability_1_sequence: 0,
                    ability_2_sequence: 0,
                },
            )
            .unwrap();
        }

        let mut positions = Vec::new();
        for connection in &clients {
            let player = tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let snapshot = bot_client::receive_snapshot_datagram(connection)
                        .await
                        .unwrap();
                    if snapshot.acknowledged_input_sequence == 1 {
                        break snapshot
                            .entities
                            .into_iter()
                            .find(|entity| {
                                entity.entity_id == protocol::M02_ISOLATED_PLAYER_ENTITY_ID
                            })
                            .unwrap();
                    }
                }
            })
            .await
            .expect("each client received its routed snapshot");
            positions.push((player.x_milli_tiles, player.y_milli_tiles));
        }
        assert!(positions[0].0 > 4_000);
        assert!(positions[1].0 < 4_000);
        assert!(positions[2].1 < 12_000);
        assert!(positions[3].1 > 12_000);

        shutdown_send.send(()).unwrap();
        let report = server_task.await.unwrap().unwrap();
        assert_eq!(report.accepted_connections, 4);
        assert_eq!(report.admitted_sessions, 4);
        assert_eq!(report.malformed_messages, 0);
        assert!(report.zero_residue);
        endpoint.wait_idle().await;
    }
}
