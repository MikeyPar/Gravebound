//! Gravebound authoritative modular-monolith boundary.
//!
//! `server_app` owns sessions, instance orchestration, routing, and authoritative execution of
//! `sim_core`. It must not own rendering, client settings, gameplay rules, or persistence logic.
//! M02 deliberately has no database dependency.

mod ground_expiry;
mod identity;
mod instance;
mod lifecycle;
mod oath_selection;
mod progression_award;
mod progression_query;
mod progression_restore_provider;
mod progression_service;
mod restore_point;
mod reward_rng;
mod reward_service;
mod runtime;
mod session;
mod starter_items;
mod world_flow_coordinator;
mod world_flow_gate;

pub use ground_expiry::{GROUND_EXPIRY_CONTEXT, GroundExpiryError, PostgresGroundExpiryService};
pub use identity::{
    AccountAggregate, AccountId, AccountRepository, AccountRepositoryError, AuthenticatedAccount,
    AuthenticatedNamespace, CharacterIdGenerator, IdentityClock, IdentityEvent, IdentityEventSink,
    IdentityService, InMemoryAccountRepository, MAX_ACCOUNT_MUTATION_RESULTS,
    NoopIdentityEventSink, PostgresAccountRepository,
};
pub use instance::{
    ArenaInstancePhase, HostedInstanceId, InstanceControlResponse, InstanceDiagnostics,
    InstanceError, InstanceKind, InstanceScheduler, M02_ARENA_CAPACITY, M02_SOAK_BOT_COUNT,
    M02_SOAK_DURATION_TICKS, SERVER_TICK_BUDGET_MICROS, SchedulerFrame, SchedulerSnapshotBatch,
    TickTimingReport,
};

pub use lifecycle::{
    DirectoryTickOutput, LINK_LOST_TICKS, LifecycleError, LifecycleResponse, LogicalSessionId,
    ManagedSession, SessionDirectory, SessionOwnerId, SessionPhase, TransportId,
};
pub use oath_selection::{CoreOathSelectionAuthority, PostgresOathSelectionService};
pub use progression_award::{
    CoreProgressionRules, ProgressionAwardCode, ProgressionAwardCommand, ProgressionAwardContext,
    ProgressionAwardError, ProgressionAwardEvidence, ProgressionAwardOutcome,
    ProgressionAwardPayload, ProgressionAwardPlan,
};
pub use progression_query::{
    DisabledProgressionQueryRepository, PostgresProgressionQueryRepository,
    ProgressionQueryRepository, ProgressionQueryRepositoryError, ProgressionQueryService,
    ProgressionQuerySnapshot,
};
pub use progression_restore_provider::PostgresProgressionRestoreProvider;
pub use progression_service::PostgresProgressionAwardService;
pub use restore_point::{
    BeltStackV1, CrashRestoreContext, DangerEntrySnapshotV1, EntryCaptureContext,
    EntryRestoreProvider, InventorySecurityRestoreV1, ItemUid, OathBargainRestoreV1,
    ProgressionRestoreV1, RestorePointError, RestorePointProviders, SafeAggregateVersionsV1,
};
pub use reward_rng::{
    ProductionRewardRng, REWARD_AUDIT_CONTEXT, REWARD_EPOCH_ID_ENV, REWARD_EPOCH_SECRET_ENV,
    REWARD_PLAN_CONTEXT, RewardRngError, RewardSeedMaterial, SecretRewardEpoch,
};
pub use reward_service::{
    PostgresRewardService, RewardGrantContext, RewardGrantError, RewardGrantResult,
    RewardGrantTransaction, RewardGrantedItem, RewardPlacement, replay_result,
};
pub use runtime::{
    BoundCoreIdentityServer, BoundLocalServer, CORE_IDENTITY_BUILD_ID,
    CORE_IDENTITY_CONTENT_TARGET, CoreIdentityServerConfig, CoreIdentityServerReport,
    LOCAL_BUILD_ID, LOCAL_REGION_ID, LOCAL_SERVER_NAME, LocalServerConfig, LocalServerReport,
    LocalServerRuntimeError,
};
pub use session::{
    AuthoritativeSession, IngressAnomaly, IngressAnomalyKind, IngressDiagnostics, InputDisposition,
    InputRejection, MAX_CACHED_MUTATIONS, MAX_NEW_MUTATIONS_PER_TICK, MAX_RECENT_INGRESS_ANOMALIES,
    SessionError,
};
pub use starter_items::{
    CORE_ITEM_CONTENT_REVISION, STARTER_RELIC_ID, STARTER_TONIC_ID, STARTER_WEAPON_ID,
    StarterItemError, StarterItemPlan, initialize_postgres_starter,
};
pub use world_flow_coordinator::{
    DormantWorldFlowPlanner, PostgresDormantWorldFlowCoordinator, WorldFlowIdGenerator,
};
pub use world_flow_gate::{
    PostgresWorldFlowLocationRepository, WorldFlowGateService, WorldFlowLocationRepository,
    WorldFlowRepositoryError,
};

use protocol::{
    ClientHello, HandshakeRejection, HandshakeResponse, ManifestHash, ProtocolVersion,
    RELIABLE_FRAME_LIMIT, SIMULATION_HZ, SNAPSHOT_HZ, ServerHello, UpdateRates, WireMessage,
    WireText, decode_frame, encode_frame,
};
use thiserror::Error;

pub const TRANSPORT_REPLACED_CLOSE_CODE: u32 = 0x100;
pub const LEAVE_ACCEPTED_CLOSE_CODE: u32 = 0x101;
pub const SERVER_SHUTDOWN_CLOSE_CODE: u32 = 0x102;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerFoundation {
    pub protocol: ProtocolVersion,
    pub rates: UpdateRates,
    pub simulation_ticks_per_second: u32,
}

impl ServerFoundation {
    #[must_use]
    pub const fn m02() -> Self {
        Self {
            protocol: ProtocolVersion::current(),
            rates: UpdateRates::canonical(),
            simulation_ticks_per_second: sim_core::TICKS_PER_SECOND,
        }
    }

    pub fn validate(self) -> Result<(), ServerFoundationError> {
        self.rates
            .validate()
            .map_err(|_| ServerFoundationError::ProtocolRates)?;
        if self.simulation_ticks_per_second != u32::from(SIMULATION_HZ) {
            return Err(ServerFoundationError::SimulationRateMismatch {
                protocol_hz: SIMULATION_HZ,
                simulation_hz: self.simulation_ticks_per_second,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerDoctorReport {
    pub protocol: ProtocolVersion,
    pub simulation_hz: u32,
    pub snapshot_hz: u16,
    pub database_enabled: bool,
    pub transport_enabled: bool,
    pub instance_scheduler_enabled: bool,
}

pub async fn run_doctor() -> Result<ServerDoctorReport, ServerFoundationError> {
    let foundation = ServerFoundation::m02();
    foundation.validate()?;
    tokio::task::yield_now().await;
    Ok(ServerDoctorReport {
        protocol: foundation.protocol,
        simulation_hz: foundation.simulation_ticks_per_second,
        snapshot_hz: SNAPSHOT_HZ,
        database_enabled: false,
        transport_enabled: true,
        instance_scheduler_enabled: true,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ServerFoundationError {
    #[error("protocol update rates failed validation")]
    ProtocolRates,
    #[error(
        "protocol and sim_core tick rates differ: protocol={protocol_hz}, sim_core={simulation_hz}"
    )]
    SimulationRateMismatch {
        protocol_hz: u16,
        simulation_hz: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthenticationDecision {
    Accepted,
    Failed,
    Suspended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionState {
    Available,
    Maintenance,
    RegionFull,
    RateLimited,
    InternalRetryable,
}

/// Immutable admission policy for a single server deployment.
#[derive(Debug, Clone)]
pub struct HandshakePolicy {
    pub required_protocol: ProtocolVersion,
    pub required_client_build: WireText<96>,
    pub required_manifest_hash: ManifestHash,
    pub content_bundle_version: WireText<32>,
    pub region_id: WireText<32>,
    pub feature_flags: Vec<WireText<64>>,
    pub admission: AdmissionState,
}

impl HandshakePolicy {
    /// Evaluates admission in stable precedence order. Authentication is supplied by the auth
    /// boundary so ticket bytes never enter logs or policy diagnostics.
    pub fn evaluate(
        &self,
        client: &ClientHello,
        authentication: AuthenticationDecision,
        session_id: WireText<64>,
    ) -> HandshakeResponse {
        let rejection = if self.admission == AdmissionState::Maintenance {
            Some(HandshakeRejection::Maintenance)
        } else if client.protocol_major != self.required_protocol.major
            || client.protocol_minor != self.required_protocol.minor
        {
            Some(HandshakeRejection::ProtocolUnsupported)
        } else if client.client_build_id != self.required_client_build {
            Some(HandshakeRejection::UpdateRequired)
        } else if client.content_manifest_hash != self.required_manifest_hash {
            Some(HandshakeRejection::ContentMismatch)
        } else if authentication == AuthenticationDecision::Suspended {
            Some(HandshakeRejection::AccountSuspended)
        } else if authentication == AuthenticationDecision::Failed {
            Some(HandshakeRejection::AuthenticationFailed)
        } else if self.admission == AdmissionState::RateLimited {
            Some(HandshakeRejection::RateLimited)
        } else if self.admission == AdmissionState::RegionFull {
            Some(HandshakeRejection::RegionFull)
        } else if self.admission == AdmissionState::InternalRetryable {
            Some(HandshakeRejection::InternalRetryable)
        } else {
            None
        };
        rejection.map_or_else(
            || {
                HandshakeResponse::Accepted(ServerHello {
                    session_id,
                    protocol_major: self.required_protocol.major,
                    protocol_minor: self.required_protocol.minor,
                    required_client_build: self.required_client_build.clone(),
                    content_bundle_version: self.content_bundle_version.clone(),
                    server_tick_rate: SIMULATION_HZ,
                    snapshot_rate: SNAPSHOT_HZ,
                    region_id: self.region_id.clone(),
                    feature_flags: self.feature_flags.clone(),
                })
            },
            HandshakeResponse::Rejected,
        )
    }
}

/// Serves exactly one handshake stream on an established QUIC connection.
pub async fn serve_handshake(
    connection: &quinn::Connection,
    policy: &HandshakePolicy,
    authentication: AuthenticationDecision,
    session_id: WireText<64>,
) -> Result<HandshakeResponse, ServerTransportError> {
    let (mut send, mut receive) = connection
        .accept_bi()
        .await
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    let request = receive
        .read_to_end(RELIABLE_FRAME_LIMIT)
        .await
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    let WireMessage::ClientHello(hello) = decode_frame(&request)? else {
        return Err(ServerTransportError::UnexpectedMessage);
    };
    let response = policy.evaluate(&hello, authentication, session_id);
    let frame = encode_frame(&WireMessage::HandshakeResponse(response.clone()))?;
    send.write_all(&frame)
        .await
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    send.finish()
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    Ok(response)
}

pub async fn receive_gameplay_input(
    connection: &quinn::Connection,
    session: &mut AuthoritativeSession,
) -> Result<InputDisposition, ServerTransportError> {
    let frame = connection
        .read_datagram()
        .await
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    let WireMessage::InputFrame(input) = decode_frame(&frame)? else {
        return Err(ServerTransportError::UnexpectedMessage);
    };
    session.submit_input(&input).map_err(Into::into)
}

/// Receives one canonical Input datagram through the logical-session transport binding.
pub async fn receive_managed_gameplay_input(
    connection: &quinn::Connection,
    session: &mut ManagedSession,
    transport: TransportId,
) -> Result<InputDisposition, ServerTransportError> {
    let frame = connection
        .read_datagram()
        .await
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    let WireMessage::InputFrame(input) = decode_frame(&frame)? else {
        return Err(ServerTransportError::UnexpectedMessage);
    };
    session.submit_input(transport, &input).map_err(Into::into)
}

pub async fn serve_gameplay_reliable(
    connection: &quinn::Connection,
    session: &mut AuthoritativeSession,
) -> Result<WireMessage, ServerTransportError> {
    let (mut send, mut receive) = connection
        .accept_bi()
        .await
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    let request = receive
        .read_to_end(RELIABLE_FRAME_LIMIT)
        .await
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    let response = session.handle_reliable(decode_frame(&request)?)?;
    let frame = encode_frame(&response)?;
    send.write_all(&frame)
        .await
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    send.finish()
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    Ok(response)
}

/// Serves one canonical reliable gameplay request through the active logical transport binding.
pub async fn serve_managed_gameplay_reliable(
    connection: &quinn::Connection,
    session: &mut ManagedSession,
    transport: TransportId,
) -> Result<WireMessage, ServerTransportError> {
    let (mut send, mut receive) = connection
        .accept_bi()
        .await
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    let request = receive
        .read_to_end(RELIABLE_FRAME_LIMIT)
        .await
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    let response = session.handle_gameplay_reliable(transport, decode_frame(&request)?)?;
    let frame = encode_frame(&response)?;
    send.write_all(&frame)
        .await
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    send.finish()
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    Ok(response)
}

/// Serves one bounded Control-channel lifecycle request. Authentication has already resolved the
/// opaque owner ID; ticket bytes never enter the directory. The caller owns the transport table
/// and closes `invalidated_transport` only after this response is committed.
pub async fn serve_session_control(
    connection: &quinn::Connection,
    directory: &mut SessionDirectory,
    owner: SessionOwnerId,
    transport: TransportId,
    content_root: &std::path::Path,
    server_monotonic_micros: u64,
) -> Result<LifecycleResponse, ServerTransportError> {
    let (mut send, mut receive) = connection
        .accept_bi()
        .await
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    let request = receive
        .read_to_end(RELIABLE_FRAME_LIMIT)
        .await
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    let WireMessage::SessionControlFrame(frame) = decode_frame(&request)? else {
        return Err(ServerTransportError::UnexpectedMessage);
    };
    let response = directory.handle_control(
        owner,
        transport,
        &frame,
        content_root,
        server_monotonic_micros,
    )?;
    let wire = WireMessage::ReliableEvent(response.event.clone());
    send.write_all(&encode_frame(&wire)?)
        .await
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    send.finish()
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    Ok(response)
}

/// Serves one Core identity request without admitting the connection to an M02 combat session.
/// The authenticated account is resolved outside this function and cannot be overridden on wire.
pub async fn serve_identity_reliable<R, C, G, E>(
    connection: &quinn::Connection,
    service: &IdentityService<R, C, G, E>,
    authenticated: Option<AuthenticatedAccount>,
    response_sequence: u32,
    server_tick: u64,
) -> Result<protocol::ReliableEventFrame, ServerTransportError>
where
    R: AccountRepository,
    C: IdentityClock,
    G: CharacterIdGenerator,
    E: IdentityEventSink,
{
    if response_sequence == 0 {
        return Err(ServerTransportError::UnexpectedMessage);
    }
    let (mut send, mut receive) = connection
        .accept_bi()
        .await
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    let request = receive
        .read_to_end(RELIABLE_FRAME_LIMIT)
        .await
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    let event = match decode_frame(&request)? {
        WireMessage::AccountBootstrapFrame(frame) => {
            protocol::ReliableEvent::AccountBootstrapResult(
                service.bootstrap(authenticated, &frame).await,
            )
        }
        WireMessage::CharacterMutationFrame(frame) => {
            protocol::ReliableEvent::CharacterMutationResult(
                service.mutate(authenticated, &frame).await,
            )
        }
        _ => return Err(ServerTransportError::UnexpectedMessage),
    };
    let response = protocol::ReliableEventFrame {
        sequence: response_sequence,
        server_tick,
        event,
    };
    send.write_all(&encode_frame(&WireMessage::ReliableEvent(
        response.clone(),
    ))?)
    .await
    .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    send.finish()
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    Ok(response)
}

/// Serves one authenticated Core request and dispatches it to the owning domain authority.
/// World-flow messages share the reliable transport but cannot mutate identity state, and the
/// normal world-flow authority remains fail-closed until its downstream packages are complete.
#[allow(clippy::too_many_arguments)]
pub async fn serve_core_reliable<R, C, G, E, W, WC, P, OC>(
    connection: &quinn::Connection,
    identity: &IdentityService<R, C, G, E>,
    world_flow: &WorldFlowGateService<W, WC>,
    progression: &ProgressionQueryService<P>,
    oath: &CoreOathSelectionAuthority<OC>,
    authenticated: AuthenticatedAccount,
    response_sequence: u32,
    server_tick: u64,
) -> Result<protocol::ReliableEventFrame, ServerTransportError>
where
    R: AccountRepository,
    C: IdentityClock,
    G: CharacterIdGenerator,
    E: IdentityEventSink,
    W: WorldFlowLocationRepository,
    WC: IdentityClock,
    P: ProgressionQueryRepository,
    OC: IdentityClock,
{
    if response_sequence == 0 {
        return Err(ServerTransportError::UnexpectedMessage);
    }
    let (mut send, mut receive) = connection
        .accept_bi()
        .await
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    let request = receive
        .read_to_end(RELIABLE_FRAME_LIMIT)
        .await
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    let event = match decode_frame(&request)? {
        WireMessage::AccountBootstrapFrame(frame) => {
            protocol::ReliableEvent::AccountBootstrapResult(
                identity.bootstrap(Some(authenticated), &frame).await,
            )
        }
        WireMessage::CharacterMutationFrame(frame) => {
            protocol::ReliableEvent::CharacterMutationResult(
                identity.mutate(Some(authenticated), &frame).await,
            )
        }
        WireMessage::WorldFlowFrame(frame) => {
            protocol::ReliableEvent::WorldFlowResult(world_flow.handle(authenticated, &frame).await)
        }
        WireMessage::ProgressionQueryFrame(frame) => protocol::ReliableEvent::ProgressionResult(
            progression.handle(authenticated, &frame).await,
        ),
        WireMessage::OathViewFrame(frame) => {
            protocol::ReliableEvent::OathViewResult(oath.view(authenticated, &frame).await)
        }
        WireMessage::InitialOathSelectionFrame(frame) => {
            protocol::ReliableEvent::InitialOathSelectionResult(
                oath.select(authenticated, &frame).await,
            )
        }
        _ => return Err(ServerTransportError::UnexpectedMessage),
    };
    let response = protocol::ReliableEventFrame {
        sequence: response_sequence,
        server_tick,
        event,
    };
    send.write_all(&encode_frame(&WireMessage::ReliableEvent(
        response.clone(),
    ))?)
    .await
    .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    send.finish()
        .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    Ok(response)
}

pub fn close_transport(connection: &quinn::Connection, close_code: u32, reason: &'static [u8]) {
    connection.close(close_code.into(), reason);
}

pub fn send_gameplay_snapshots(
    connection: &quinn::Connection,
    snapshots: Vec<protocol::SnapshotChunk>,
) -> Result<(), ServerTransportError> {
    for snapshot in snapshots {
        let frame = encode_frame(&WireMessage::SnapshotChunk(snapshot))?;
        connection
            .send_datagram(frame.into())
            .map_err(|error| ServerTransportError::Quic(error.to_string()))?;
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ServerTransportError {
    #[error("QUIC handshake transport failed: {0}")]
    Quic(String),
    #[error("handshake codec failed: {0}")]
    Codec(#[from] protocol::WireCodecError),
    #[error("client sent a non-hello message on the handshake stream")]
    UnexpectedMessage,
    #[error("authoritative session failed: {0}")]
    Session(#[from] SessionError),
    #[error("logical session lifecycle failed: {0}")]
    Lifecycle(#[from] LifecycleError),
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use protocol::{AuthTicket, Compression, Platform};
    use rcgen::{CertifiedKey, generate_simple_self_signed};
    use rustls::pki_types::PrivatePkcs8KeyDer;

    use super::*;

    fn policy() -> HandshakePolicy {
        HandshakePolicy {
            required_protocol: ProtocolVersion::current(),
            required_client_build: WireText::new("dev-1").unwrap(),
            required_manifest_hash: ManifestHash::new("a".repeat(64)).unwrap(),
            content_bundle_version: WireText::new("fp.1.0.0").unwrap(),
            region_id: WireText::new("local").unwrap(),
            feature_flags: vec![WireText::new("m02-handshake").unwrap()],
            admission: AdmissionState::Available,
        }
    }

    fn client_hello() -> ClientHello {
        ClientHello {
            protocol_major: ProtocolVersion::current().major,
            protocol_minor: ProtocolVersion::current().minor,
            client_build_id: WireText::new("dev-1").unwrap(),
            platform: Platform::WindowsNative,
            supported_compression: vec![Compression::None],
            content_manifest_hash: ManifestHash::new("a".repeat(64)).unwrap(),
            auth_ticket: AuthTicket::new(b"integration-ticket".to_vec()).unwrap(),
            locale: WireText::new("en-US").unwrap(),
        }
    }

    #[test]
    fn authoritative_server_uses_the_shared_simulation_rate() {
        assert_eq!(ServerFoundation::m02().validate(), Ok(()));
        assert_eq!(sim_core::TICKS_PER_SECOND, 30);
    }

    #[tokio::test]
    async fn doctor_reports_the_m02_01_transport_boundary() {
        let report = run_doctor().await.expect("M02 foundation doctor");
        assert_eq!(report.protocol, ProtocolVersion::current());
        assert_eq!(report.simulation_hz, 30);
        assert_eq!(report.snapshot_hz, 15);
        assert!(!report.database_enabled);
        assert!(report.transport_enabled);
        assert!(report.instance_scheduler_enabled);
    }

    #[test]
    fn policy_returns_every_required_rejection_and_accepts_valid_clients() {
        let hello = client_hello();
        let session = || WireText::new("session-1").unwrap();
        let accepted = policy().evaluate(&hello, AuthenticationDecision::Accepted, session());
        assert!(matches!(accepted, HandshakeResponse::Accepted(_)));

        let cases = [
            (HandshakeRejection::Maintenance, 0_u8),
            (HandshakeRejection::ProtocolUnsupported, 1),
            (HandshakeRejection::UpdateRequired, 2),
            (HandshakeRejection::ContentMismatch, 3),
            (HandshakeRejection::AccountSuspended, 4),
            (HandshakeRejection::AuthenticationFailed, 5),
            (HandshakeRejection::RateLimited, 6),
            (HandshakeRejection::RegionFull, 7),
            (HandshakeRejection::InternalRetryable, 8),
        ];
        for (expected, case) in cases {
            let mut candidate_policy = policy();
            let mut candidate_hello = hello.clone();
            let mut auth = AuthenticationDecision::Accepted;
            match case {
                0 => candidate_policy.admission = AdmissionState::Maintenance,
                1 => candidate_hello.protocol_major = 2,
                2 => candidate_hello.client_build_id = WireText::new("old").unwrap(),
                3 => {
                    candidate_hello.content_manifest_hash =
                        ManifestHash::new("b".repeat(64)).unwrap();
                }
                4 => auth = AuthenticationDecision::Suspended,
                5 => auth = AuthenticationDecision::Failed,
                6 => candidate_policy.admission = AdmissionState::RateLimited,
                7 => candidate_policy.admission = AdmissionState::RegionFull,
                8 => candidate_policy.admission = AdmissionState::InternalRetryable,
                _ => unreachable!(),
            }
            assert_eq!(
                candidate_policy.evaluate(&candidate_hello, auth, session()),
                HandshakeResponse::Rejected(expected)
            );
        }
    }

    #[tokio::test]
    async fn real_quic_loopback_exchanges_the_versioned_handshake() {
        let CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
        let certificate = cert.der().clone();
        let private_key = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
        let server_config =
            quinn::ServerConfig::with_single_cert(vec![certificate.clone()], private_key.into())
                .unwrap();
        let server_endpoint =
            quinn::Endpoint::server(server_config, "127.0.0.1:0".parse().unwrap()).unwrap();
        let server_address = server_endpoint.local_addr().unwrap();

        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate).unwrap();
        let client_config = quinn::ClientConfig::with_root_certificates(Arc::new(roots)).unwrap();
        let mut client_endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        client_endpoint.set_default_client_config(client_config);

        let server_task = tokio::spawn(async move {
            let connection = server_endpoint.accept().await.unwrap().await.unwrap();
            let response = serve_handshake(
                &connection,
                &policy(),
                AuthenticationDecision::Accepted,
                WireText::new("session-loopback").unwrap(),
            )
            .await
            .unwrap();
            (response, connection)
        });
        let connection = client_endpoint
            .connect(server_address, "localhost")
            .unwrap()
            .await
            .unwrap();
        let client_response = bot_client::perform_handshake(&connection, client_hello())
            .await
            .unwrap();
        let (server_response, _server_connection) = server_task.await.unwrap();
        assert_eq!(client_response, server_response);
        assert!(matches!(client_response, HandshakeResponse::Accepted(_)));
        connection.close(0_u32.into(), b"test complete");
        client_endpoint.wait_idle().await;
    }

    #[tokio::test]
    async fn real_quic_loopback_routes_input_snapshot_and_reliable_action() {
        let CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
        let certificate = cert.der().clone();
        let private_key = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
        let server_config =
            quinn::ServerConfig::with_single_cert(vec![certificate.clone()], private_key.into())
                .unwrap();
        let server_endpoint =
            quinn::Endpoint::server(server_config, "127.0.0.1:0".parse().unwrap()).unwrap();
        let server_address = server_endpoint.local_addr().unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate).unwrap();
        let client_config = quinn::ClientConfig::with_root_certificates(Arc::new(roots)).unwrap();
        let mut client_endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        client_endpoint.set_default_client_config(client_config);

        let server_task = tokio::spawn(async move {
            let connection = server_endpoint.accept().await.unwrap().await.unwrap();
            serve_handshake(
                &connection,
                &policy(),
                AuthenticationDecision::Accepted,
                WireText::new("session-gameplay").unwrap(),
            )
            .await
            .unwrap();
            let content_root =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
            let mut session =
                AuthoritativeSession::from_content_root(&content_root).expect("content session");
            assert_eq!(
                receive_gameplay_input(&connection, &mut session)
                    .await
                    .unwrap(),
                InputDisposition::Accepted
            );
            assert!(session.tick().unwrap().is_empty());
            let snapshots = session.tick().unwrap();
            assert_eq!(snapshots.len(), 1);
            send_gameplay_snapshots(&connection, snapshots).unwrap();
            let reliable = serve_gameplay_reliable(&connection, &mut session)
                .await
                .unwrap();
            (reliable, connection)
        });
        let connection = client_endpoint
            .connect(server_address, "localhost")
            .unwrap()
            .await
            .unwrap();
        bot_client::perform_handshake(&connection, client_hello())
            .await
            .unwrap();
        bot_client::send_input_datagram(
            &connection,
            protocol::InputFrame {
                sequence: 1,
                client_tick: 1,
                movement_x_milli: 0,
                movement_y_milli: 0,
                aim_x_milli: 1_000,
                aim_y_milli: 0,
                held_primary: false,
                primary_sequence: 0,
                ability_1_sequence: 0,
                ability_2_sequence: 0,
            },
        )
        .unwrap();
        let snapshot = bot_client::receive_snapshot_datagram(&connection)
            .await
            .unwrap();
        assert_eq!(snapshot.server_tick, 2);
        assert_eq!(snapshot.acknowledged_input_sequence, 1);
        let event = bot_client::perform_reliable_gameplay(
            &connection,
            WireMessage::ActionFrame(protocol::ActionFrame {
                sequence: 1,
                client_tick: 2,
                action: protocol::ActionKind::Ability1Press,
            }),
        )
        .await
        .unwrap();
        assert!(matches!(
            event.event,
            protocol::ReliableEvent::ActionResult {
                action_sequence: 1,
                code: protocol::ActionResultCode::Accepted
            }
        ));
        let (server_message, _server_connection) = server_task.await.unwrap();
        assert_eq!(server_message, WireMessage::ReliableEvent(event));
        connection.close(0_u32.into(), b"test complete");
        client_endpoint.wait_idle().await;
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)] // Full two-connection QUIC handoff is clearer as one journey.
    async fn real_quic_lifecycle_replaces_old_transport_then_accepts_clean_leave() {
        let CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
        let certificate = cert.der().clone();
        let private_key = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
        let server_config =
            quinn::ServerConfig::with_single_cert(vec![certificate.clone()], private_key.into())
                .unwrap();
        let server_endpoint =
            quinn::Endpoint::server(server_config, "127.0.0.1:0".parse().unwrap()).unwrap();
        let server_address = server_endpoint.local_addr().unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate).unwrap();
        let client_config = quinn::ClientConfig::with_root_certificates(Arc::new(roots)).unwrap();
        let mut client_endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        client_endpoint.set_default_client_config(client_config);

        let (drained_tx, drained_rx) = tokio::sync::oneshot::channel();
        let server_task = tokio::spawn(async move {
            let owner = SessionOwnerId::new(1).unwrap();
            let first_transport = TransportId::new(1).unwrap();
            let second_transport = TransportId::new(2).unwrap();
            let content_root =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
            let mut directory = SessionDirectory::default();

            let first = server_endpoint.accept().await.unwrap().await.unwrap();
            serve_handshake(
                &first,
                &policy(),
                AuthenticationDecision::Accepted,
                WireText::new("session-provisional-1").unwrap(),
            )
            .await
            .unwrap();
            let joined = serve_session_control(
                &first,
                &mut directory,
                owner,
                first_transport,
                &content_root,
                100,
            )
            .await
            .unwrap();
            assert_eq!(joined.invalidated_transport, None);

            let second = server_endpoint.accept().await.unwrap().await.unwrap();
            serve_handshake(
                &second,
                &policy(),
                AuthenticationDecision::Accepted,
                WireText::new("session-provisional-2").unwrap(),
            )
            .await
            .unwrap();
            let replaced = serve_session_control(
                &second,
                &mut directory,
                owner,
                second_transport,
                &content_root,
                200,
            )
            .await
            .unwrap();
            assert_eq!(replaced.invalidated_transport, Some(first_transport));
            close_transport(
                &first,
                TRANSPORT_REPLACED_CLOSE_CODE,
                b"authoritative transport handoff",
            );

            let left = serve_session_control(
                &second,
                &mut directory,
                owner,
                second_transport,
                &content_root,
                300,
            )
            .await
            .unwrap();
            assert_eq!(left.invalidated_transport, Some(second_transport));
            drained_rx.await.unwrap();
            directory
        });

        let first = client_endpoint
            .connect(server_address, "localhost")
            .unwrap()
            .await
            .unwrap();
        bot_client::perform_handshake(&first, client_hello())
            .await
            .unwrap();
        let (_, joined) = bot_client::perform_session_control(
            &first,
            protocol::SessionControlFrame {
                sequence: 1,
                client_tick: 0,
                client_monotonic_micros: 10,
                request: protocol::SessionControlRequest::Join,
            },
        )
        .await
        .unwrap();
        assert_eq!(joined.code, protocol::SessionControlResultCode::Joined);

        let second = client_endpoint
            .connect(server_address, "localhost")
            .unwrap()
            .await
            .unwrap();
        bot_client::perform_handshake(&second, client_hello())
            .await
            .unwrap();
        let (_, reattached) = bot_client::perform_session_control(
            &second,
            protocol::SessionControlFrame {
                sequence: 1,
                client_tick: 0,
                client_monotonic_micros: 20,
                request: protocol::SessionControlRequest::Join,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            reattached.code,
            protocol::SessionControlResultCode::Reattached
        );
        assert!(reattached.replaced_previous_transport);
        assert!(matches!(
            first.closed().await,
            quinn::ConnectionError::ApplicationClosed(_)
        ));

        let (_, leave) = bot_client::perform_session_control(
            &second,
            protocol::SessionControlFrame {
                sequence: 2,
                client_tick: 0,
                client_monotonic_micros: 30,
                request: protocol::SessionControlRequest::Leave,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            leave.code,
            protocol::SessionControlResultCode::LeaveAccepted
        );
        drained_tx.send(()).unwrap();
        close_transport(&second, LEAVE_ACCEPTED_CLOSE_CODE, b"leave accepted");
        assert!(matches!(
            second.closed().await,
            quinn::ConnectionError::LocallyClosed | quinn::ConnectionError::ApplicationClosed(_)
        ));
        let directory = server_task.await.unwrap();
        assert!(matches!(
            directory
                .session(SessionOwnerId::new(1).unwrap())
                .unwrap()
                .phase(),
            SessionPhase::LinkLost {
                lost_tick: 0,
                recall_tick: LINK_LOST_TICKS
            }
        ));
        client_endpoint.wait_idle().await;
    }
}
