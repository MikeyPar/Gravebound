//! Bounded native QUIC client transport for `GB-M02-GATE`.
//!
//! A dedicated Tokio thread owns Quinn. Bevy exchanges latest-state input, a bounded latest
//! snapshot queue, and reliable events without ever giving the transport gameplay authority.

use std::{
    collections::VecDeque,
    net::SocketAddr,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

use protocol::{
    AccountBootstrapFrame, AccountBootstrapRequest, ClientHello, ControlEvent, HandshakeResponse,
    ManifestHash, RELIABLE_FRAME_LIMIT, ReliableEvent, ReliableEventFrame, ReliableEventInbox,
    ReliableEventInboxError, SessionControlFrame, SessionControlRequest, SnapshotChunk,
    WireMessage, WireText, decode_frame, encode_frame,
};
use rustls::pki_types::CertificateDer;
use thiserror::Error;
use tokio::sync::{mpsc as tokio_mpsc, watch};

const RELIABLE_COMMAND_CAPACITY: usize = 64;
const RELIABLE_EVENT_CAPACITY: usize = 64;
const SNAPSHOT_QUEUE_CAPACITY: usize = 16;
const RECONNECT_ATTEMPTS: usize = 10;
const RECONNECT_DELAY: Duration = Duration::from_millis(250);
const RELIABLE_GAP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct NetworkTransportConfig {
    pub server_address: SocketAddr,
    pub server_name: String,
    pub certificate_der: Vec<u8>,
    pub hello: ClientHello,
    pub startup: NetworkStartup,
}

/// Selects the first reliable route after a successful transport handshake.
///
/// Core identity deliberately does not create an M02 combat session. Reconnect refreshes the
/// process-local account projection instead of attempting vulnerable-combat reattachment.
#[derive(Debug, Clone)]
pub enum NetworkStartup {
    CombatSession,
    CoreIdentity { content_manifest_hash: ManifestHash },
}

#[derive(Debug, Clone)]
pub enum TransportEvent {
    Connecting,
    HandshakeAccepted(protocol::ServerHello),
    Reliable(ReliableEventFrame),
    LinkLost,
    Reconnecting { attempt: usize },
    TransportClosed,
    Fatal(String),
}

#[derive(Debug)]
enum ReliableCommand {
    Gameplay(Box<WireMessage>),
    Shutdown,
}

#[derive(Debug)]
enum ConnectedExit {
    Lost,
    Shutdown,
}

#[derive(Debug)]
pub struct NetworkWorkerHandle {
    input: watch::Sender<Option<protocol::InputFrame>>,
    reliable: tokio_mpsc::Sender<ReliableCommand>,
    events: Mutex<mpsc::Receiver<TransportEvent>>,
    snapshots: Arc<Mutex<VecDeque<SnapshotChunk>>>,
}

impl NetworkWorkerHandle {
    pub fn spawn(config: NetworkTransportConfig) -> Result<Self, NetworkTransportError> {
        if config.certificate_der.is_empty() {
            return Err(NetworkTransportError::EmptyCertificate);
        }
        let (input, input_rx) = watch::channel(None);
        let (reliable, reliable_rx) = tokio_mpsc::channel(RELIABLE_COMMAND_CAPACITY);
        let (event_tx, events) = mpsc::sync_channel(RELIABLE_EVENT_CAPACITY);
        let snapshots = Arc::new(Mutex::new(VecDeque::with_capacity(SNAPSHOT_QUEUE_CAPACITY)));
        let worker_snapshots = Arc::clone(&snapshots);
        thread::Builder::new()
            .name("gravebound-network".to_owned())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = event_tx.send(TransportEvent::Fatal(error.to_string()));
                        return;
                    }
                };
                if let Err(error) = runtime.block_on(run_worker(
                    config,
                    input_rx,
                    reliable_rx,
                    event_tx.clone(),
                    worker_snapshots,
                )) {
                    let _ = event_tx.send(TransportEvent::Fatal(error.to_string()));
                }
            })
            .map_err(NetworkTransportError::SpawnThread)?;
        Ok(Self {
            input,
            reliable,
            events: Mutex::new(events),
            snapshots,
        })
    }

    pub fn replace_input(&self, input: protocol::InputFrame) {
        self.input.send_replace(Some(input));
    }

    pub fn queue_reliable(&self, message: WireMessage) -> Result<(), NetworkTransportError> {
        if message.uses_datagram()
            || matches!(
                message,
                WireMessage::ClientHello(_)
                    | WireMessage::HandshakeResponse(_)
                    | WireMessage::SnapshotChunk(_)
                    | WireMessage::ReliableEvent(_)
                    | WireMessage::SessionControlFrame(_)
            )
        {
            return Err(NetworkTransportError::InvalidReliableCommand);
        }
        self.reliable
            .try_send(ReliableCommand::Gameplay(Box::new(message)))
            .map_err(|_| NetworkTransportError::ReliableQueueFull)
    }

    pub fn drain_events(&self) -> Vec<TransportEvent> {
        let receiver = self.events.lock().expect("network event mutex poisoned");
        receiver.try_iter().collect()
    }

    pub fn drain_snapshots(&self) -> Vec<SnapshotChunk> {
        let mut snapshots = self
            .snapshots
            .lock()
            .expect("network snapshot mutex poisoned");
        snapshots.drain(..).collect()
    }

    pub fn shutdown(&self) {
        let _ = self.reliable.try_send(ReliableCommand::Shutdown);
    }
}

impl Drop for NetworkWorkerHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

async fn run_worker(
    config: NetworkTransportConfig,
    mut input: watch::Receiver<Option<protocol::InputFrame>>,
    mut reliable: tokio_mpsc::Receiver<ReliableCommand>,
    events: mpsc::SyncSender<TransportEvent>,
    snapshots: Arc<Mutex<VecDeque<SnapshotChunk>>>,
) -> Result<(), NetworkTransportError> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(CertificateDer::from(config.certificate_der.clone()))?;
    let client_config = quinn::ClientConfig::with_root_certificates(Arc::new(roots))
        .map_err(|error| NetworkTransportError::TlsConfiguration(error.to_string()))?;
    let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().expect("valid bind address"))?;
    endpoint.set_default_client_config(client_config);

    send_event(&events, TransportEvent::Connecting)?;
    let mut prior_session: Option<WireText<64>> = None;
    let mut core_bootstrapped = false;
    let mut combat_reliable_sequence = 0_u32;
    let mut control_sequence = 1_u32;
    let mut reconnect_attempt = 0_usize;
    loop {
        let (connection, server_hello) = match connect_and_handshake(&endpoint, &config).await {
            Ok(accepted) => accepted,
            Err(error)
                if (prior_session.is_some() || core_bootstrapped)
                    && reconnect_attempt < RECONNECT_ATTEMPTS =>
            {
                reconnect_attempt += 1;
                send_event(
                    &events,
                    TransportEvent::Reconnecting {
                        attempt: reconnect_attempt,
                    },
                )?;
                tokio::time::sleep(RECONNECT_DELAY).await;
                let _ = error;
                continue;
            }
            Err(error) => return Err(error),
        };
        send_event(&events, TransportEvent::HandshakeAccepted(server_hello))?;
        let lifecycle_event = perform_startup(
            &connection,
            &config.startup,
            prior_session.as_ref(),
            core_bootstrapped,
            control_sequence,
        )
        .await?;
        control_sequence = control_sequence
            .checked_add(1)
            .ok_or(NetworkTransportError::SequenceExhausted)?;
        match &config.startup {
            NetworkStartup::CombatSession => {
                let session_id = session_id_from_event(&lifecycle_event)?;
                prior_session = Some(session_id);
            }
            NetworkStartup::CoreIdentity { .. } => core_bootstrapped = true,
        }
        let previous_reliable_sequence = if matches!(&config.startup, NetworkStartup::CombatSession)
        {
            combat_reliable_sequence
        } else {
            0
        };
        let (connected, delivered_sequence) = run_connected_transport(
            &connection,
            &mut input,
            &mut reliable,
            &events,
            &snapshots,
            lifecycle_event,
            previous_reliable_sequence,
        )
        .await?;
        if matches!(&config.startup, NetworkStartup::CombatSession) {
            combat_reliable_sequence = delivered_sequence;
        }
        match connected {
            ConnectedExit::Shutdown => {
                connection.close(0_u32.into(), b"native client shutdown");
                endpoint.wait_idle().await;
                send_event(&events, TransportEvent::TransportClosed)?;
                return Ok(());
            }
            ConnectedExit::Lost => {
                send_event(&events, TransportEvent::LinkLost)?;
                reconnect_attempt = 1;
                send_event(
                    &events,
                    TransportEvent::Reconnecting {
                        attempt: reconnect_attempt,
                    },
                )?;
                tokio::time::sleep(RECONNECT_DELAY).await;
            }
        }
    }
}

async fn perform_startup(
    connection: &quinn::Connection,
    startup: &NetworkStartup,
    prior_session: Option<&WireText<64>>,
    core_bootstrapped: bool,
    control_sequence: u32,
) -> Result<ReliableEventFrame, NetworkTransportError> {
    match startup {
        NetworkStartup::CombatSession => {
            let request = prior_session.map_or(SessionControlRequest::Join, |prior_session_id| {
                SessionControlRequest::Reconnect {
                    prior_session_id: prior_session_id.clone(),
                }
            });
            perform_control(
                connection,
                SessionControlFrame {
                    sequence: control_sequence,
                    client_tick: 0,
                    client_monotonic_micros: 0,
                    request,
                },
            )
            .await
        }
        NetworkStartup::CoreIdentity {
            content_manifest_hash,
        } => {
            perform_core_bootstrap(
                connection,
                AccountBootstrapFrame {
                    sequence: control_sequence,
                    request: if core_bootstrapped {
                        AccountBootstrapRequest::Refresh
                    } else {
                        AccountBootstrapRequest::Bootstrap
                    },
                    content_manifest_hash: content_manifest_hash.clone(),
                },
            )
            .await
        }
    }
}

async fn run_connected_transport(
    connection: &quinn::Connection,
    input: &mut watch::Receiver<Option<protocol::InputFrame>>,
    reliable: &mut tokio_mpsc::Receiver<ReliableCommand>,
    events: &mpsc::SyncSender<TransportEvent>,
    snapshots: &Arc<Mutex<VecDeque<SnapshotChunk>>>,
    lifecycle_event: ReliableEventFrame,
    previous_reliable_sequence: u32,
) -> Result<(ConnectedExit, u32), NetworkTransportError> {
    let mut reliable_inbox = ReliableEventInbox::resume_after(previous_reliable_sequence);
    deliver_reliable(events, &mut reliable_inbox, lifecycle_event)?;
    let connected = run_connected(
        connection,
        input,
        reliable,
        events,
        snapshots,
        &mut reliable_inbox,
    )
    .await?;
    Ok((connected, reliable_inbox.last_delivered_sequence()))
}

async fn connect_and_handshake(
    endpoint: &quinn::Endpoint,
    config: &NetworkTransportConfig,
) -> Result<(quinn::Connection, protocol::ServerHello), NetworkTransportError> {
    let connection = endpoint
        .connect(config.server_address, &config.server_name)?
        .await?;
    let request = encode_frame(&WireMessage::ClientHello(config.hello.clone()))?;
    let response = exchange_reliable_bytes(&connection, &request).await?;
    let WireMessage::HandshakeResponse(response) = decode_frame(&response)? else {
        return Err(NetworkTransportError::UnexpectedMessage);
    };
    match response {
        HandshakeResponse::Accepted(server_hello) => Ok((connection, server_hello)),
        HandshakeResponse::Rejected(rejection) => {
            Err(NetworkTransportError::HandshakeRejected(rejection))
        }
    }
}

async fn run_connected(
    connection: &quinn::Connection,
    input: &mut watch::Receiver<Option<protocol::InputFrame>>,
    reliable: &mut tokio_mpsc::Receiver<ReliableCommand>,
    events: &mpsc::SyncSender<TransportEvent>,
    snapshots: &Arc<Mutex<VecDeque<SnapshotChunk>>>,
    reliable_inbox: &mut ReliableEventInbox,
) -> Result<ConnectedExit, NetworkTransportError> {
    let mut gap_deadline = None;
    loop {
        tokio::select! {
            () = tokio::time::sleep_until(
                gap_deadline.unwrap_or_else(tokio::time::Instant::now)
            ), if gap_deadline.is_some() => {
                connection.close(0_u32.into(), b"reliable sequence gap");
                return Ok(ConnectedExit::Lost);
            }
            result = input.changed() => {
                if result.is_err() {
                    return Ok(ConnectedExit::Shutdown);
                }
                let latest = input.borrow_and_update().clone();
                if let Some(frame) = latest {
                    let bytes = encode_frame(&WireMessage::InputFrame(frame))?;
                    connection.send_datagram(bytes.into())?;
                }
            }
            command = reliable.recv() => {
                match command {
                    Some(ReliableCommand::Gameplay(message)) => {
                        let request = encode_frame(message.as_ref())?;
                        let response = exchange_reliable_bytes(connection, &request).await?;
                        let WireMessage::ReliableEvent(event) = decode_frame(&response)? else {
                            return Err(NetworkTransportError::UnexpectedMessage);
                        };
                        if deliver_reliable(events, reliable_inbox, event).is_err() {
                            connection.close(0_u32.into(), b"invalid reliable sequence");
                            return Ok(ConnectedExit::Lost);
                        }
                    }
                    Some(ReliableCommand::Shutdown) | None => {
                        return Ok(ConnectedExit::Shutdown);
                    }
                }
            }
            datagram = connection.read_datagram() => {
                let Ok(bytes) = datagram else {
                    return Ok(ConnectedExit::Lost);
                };
                let WireMessage::SnapshotChunk(snapshot) = decode_frame(&bytes)? else {
                    return Err(NetworkTransportError::UnexpectedMessage);
                };
                push_latest_snapshot(snapshots, snapshot);
            }
            stream = connection.accept_uni() => {
                let Ok(mut receive) = stream else {
                    return Ok(ConnectedExit::Lost);
                };
                let bytes = receive.read_to_end(RELIABLE_FRAME_LIMIT).await?;
                let WireMessage::ReliableEvent(event) = decode_frame(&bytes)? else {
                    return Err(NetworkTransportError::UnexpectedMessage);
                };
                if deliver_reliable(events, reliable_inbox, event).is_err() {
                    connection.close(0_u32.into(), b"invalid reliable sequence");
                    return Ok(ConnectedExit::Lost);
                }
            }
        }
        if reliable_inbox.has_gap() {
            gap_deadline.get_or_insert_with(|| tokio::time::Instant::now() + RELIABLE_GAP_TIMEOUT);
        } else {
            gap_deadline = None;
        }
    }
}

async fn perform_control(
    connection: &quinn::Connection,
    frame: SessionControlFrame,
) -> Result<ReliableEventFrame, NetworkTransportError> {
    let request = encode_frame(&WireMessage::SessionControlFrame(frame))?;
    let response = exchange_reliable_bytes(connection, &request).await?;
    let WireMessage::ReliableEvent(event) = decode_frame(&response)? else {
        return Err(NetworkTransportError::UnexpectedMessage);
    };
    let ReliableEvent::Control(ControlEvent::SessionResult(result)) = &event.event else {
        return Err(NetworkTransportError::UnexpectedMessage);
    };
    if !result.accepted {
        return Err(NetworkTransportError::SessionRejected(result.code));
    }
    Ok(event)
}

async fn perform_core_bootstrap(
    connection: &quinn::Connection,
    frame: AccountBootstrapFrame,
) -> Result<ReliableEventFrame, NetworkTransportError> {
    let request = encode_frame(&WireMessage::AccountBootstrapFrame(frame))?;
    let response = exchange_reliable_bytes(connection, &request).await?;
    let WireMessage::ReliableEvent(event) = decode_frame(&response)? else {
        return Err(NetworkTransportError::UnexpectedMessage);
    };
    if !matches!(event.event, ReliableEvent::AccountBootstrapResult(_)) {
        return Err(NetworkTransportError::UnexpectedMessage);
    }
    Ok(event)
}

async fn exchange_reliable_bytes(
    connection: &quinn::Connection,
    request: &[u8],
) -> Result<Vec<u8>, NetworkTransportError> {
    let (mut send, mut receive) = connection.open_bi().await?;
    send.write_all(request).await?;
    send.finish()?;
    Ok(receive.read_to_end(RELIABLE_FRAME_LIMIT).await?)
}

fn session_id_from_event(
    event: &ReliableEventFrame,
) -> Result<WireText<64>, NetworkTransportError> {
    let ReliableEvent::Control(ControlEvent::SessionResult(result)) = &event.event else {
        return Err(NetworkTransportError::UnexpectedMessage);
    };
    Ok(result.session_id.clone())
}

fn push_latest_snapshot(snapshots: &Arc<Mutex<VecDeque<SnapshotChunk>>>, snapshot: SnapshotChunk) {
    let mut snapshots = snapshots.lock().expect("network snapshot mutex poisoned");
    if snapshots.len() == SNAPSHOT_QUEUE_CAPACITY {
        snapshots.pop_front();
    }
    snapshots.push_back(snapshot);
}

fn send_event(
    events: &mpsc::SyncSender<TransportEvent>,
    event: TransportEvent,
) -> Result<(), NetworkTransportError> {
    events
        .send(event)
        .map_err(|_| NetworkTransportError::EventReceiverClosed)
}

fn deliver_reliable(
    events: &mpsc::SyncSender<TransportEvent>,
    inbox: &mut ReliableEventInbox,
    event: ReliableEventFrame,
) -> Result<(), NetworkTransportError> {
    for ready in inbox.push(event)? {
        send_event(events, TransportEvent::Reliable(ready))?;
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum NetworkTransportError {
    #[error("local server certificate is empty")]
    EmptyCertificate,
    #[error("reliable network command is not a legal gameplay request")]
    InvalidReliableCommand,
    #[error("reliable command queue is full or closed")]
    ReliableQueueFull,
    #[error("network worker event receiver closed")]
    EventReceiverClosed,
    #[error("network worker received an unexpected protocol message")]
    UnexpectedMessage,
    #[error(transparent)]
    ReliableSequence(#[from] ReliableEventInboxError),
    #[error("network TLS configuration failed: {0}")]
    TlsConfiguration(String),
    #[error("network control sequence exhausted")]
    SequenceExhausted,
    #[error("server rejected handshake: {0:?}")]
    HandshakeRejected(protocol::HandshakeRejection),
    #[error("server rejected session control: {0:?}")]
    SessionRejected(protocol::SessionControlResultCode),
    #[error("failed to spawn native network worker: {0}")]
    SpawnThread(std::io::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Rustls(#[from] rustls::Error),
    #[error(transparent)]
    Connect(#[from] quinn::ConnectError),
    #[error(transparent)]
    Connection(#[from] quinn::ConnectionError),
    #[error(transparent)]
    SendDatagram(#[from] quinn::SendDatagramError),
    #[error(transparent)]
    Read(#[from] quinn::ReadToEndError),
    #[error(transparent)]
    Write(#[from] quinn::WriteError),
    #[error(transparent)]
    ClosedStream(#[from] quinn::ClosedStream),
    #[error(transparent)]
    Codec(#[from] protocol::WireCodecError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reliable(sequence: u32) -> ReliableEventFrame {
        ReliableEventFrame {
            sequence,
            server_tick: u64::from(sequence),
            event: ReliableEvent::ActionResult {
                action_sequence: sequence,
                code: protocol::ActionResultCode::Accepted,
            },
        }
    }

    #[test]
    fn snapshot_queue_is_strictly_bounded_and_retains_latest_state() {
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        for sequence in 1..=32 {
            push_latest_snapshot(
                &queue,
                SnapshotChunk {
                    sequence,
                    server_tick: u64::from(sequence),
                    state_version: u64::from(sequence),
                    acknowledged_input_sequence: sequence,
                    chunk_index: 0,
                    chunk_count: 1,
                    entities: Vec::new(),
                },
            );
        }
        let queue = queue.lock().unwrap();
        assert_eq!(queue.len(), SNAPSHOT_QUEUE_CAPACITY);
        assert_eq!(queue.front().unwrap().sequence, 17);
        assert_eq!(queue.back().unwrap().sequence, 32);
    }

    #[test]
    fn reliable_transport_publishes_cross_stream_events_contiguously() {
        let (events, receive) = mpsc::sync_channel(4);
        let mut inbox = ReliableEventInbox::new();
        deliver_reliable(&events, &mut inbox, reliable(2)).unwrap();
        assert!(receive.try_recv().is_err());

        deliver_reliable(&events, &mut inbox, reliable(1)).unwrap();
        let first = receive.try_recv().unwrap();
        let second = receive.try_recv().unwrap();
        assert!(matches!(first, TransportEvent::Reliable(frame) if frame.sequence == 1));
        assert!(matches!(second, TransportEvent::Reliable(frame) if frame.sequence == 2));
        assert!(receive.try_recv().is_err());
    }
}
