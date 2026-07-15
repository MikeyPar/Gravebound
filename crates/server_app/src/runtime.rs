//! Runnable local QUIC orchestration for the `GB-M02-GATE` playtest build.
//!
//! This module owns transport and scheduling only. Gameplay authority remains in
//! [`InstanceScheduler`], and every gameplay value still comes from validated `fp.1.0.0` data.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    future::Future,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use protocol::{
    CORE_TEST_IDENTITY_FEATURE_FLAG, ClientHello, ControlEvent, HandshakeResponse,
    M02_LOCAL_BUILD_ID, M02_LOCAL_REGION_ID, M02_LOCAL_SERVER_NAME, M03_CORE_DEV_BUILD_ID,
    M03_CORE_DEV_CONTENT_TARGET, ManifestHash, ProtocolVersion, RELIABLE_FRAME_LIMIT,
    ReliableEvent, ReliableEventFrame, SIMULATION_HZ, SessionControlResultCode, WireMessage,
    WireText, decode_frame, encode_frame,
};
use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use thiserror::Error;
use tokio::{sync::Mutex, task::JoinSet, time::MissedTickBehavior};
use tracing::{debug, info, warn};

use crate::{
    AccountId, AccountRepository, AdmissionState, AuthenticatedAccount, AuthenticatedNamespace,
    AuthenticationDecision, CharacterIdGenerator, CoreBargainAuthority, CoreOathSelectionAuthority,
    CoreSafeInventoryAuthority, DeathViewRepository, DeathViewService, DisabledDeathViewRepository,
    DisabledProgressionQueryRepository, HandshakePolicy, IdentityClock, IdentityService,
    InMemoryAccountRepository, InstanceError, InstanceScheduler, NoopIdentityEventSink,
    PostgresAccountRepository, PostgresBargainService, PostgresDeathViewRepository,
    PostgresOathSelectionService, PostgresProgressionQueryRepository,
    PostgresWorldFlowLocationRepository, ProgressionQueryRepository, ProgressionQueryService,
    SERVER_SHUTDOWN_CLOSE_CODE, SessionOwnerId, TransportId, WorldFlowGateService,
    WorldFlowLocationRepository, close_transport, serve_core_reliable,
};

pub const LOCAL_BUILD_ID: &str = M02_LOCAL_BUILD_ID;
pub const LOCAL_REGION_ID: &str = M02_LOCAL_REGION_ID;
pub const LOCAL_SERVER_NAME: &str = M02_LOCAL_SERVER_NAME;
pub const CORE_IDENTITY_BUILD_ID: &str = M03_CORE_DEV_BUILD_ID;
pub const CORE_IDENTITY_CONTENT_TARGET: &str = M03_CORE_DEV_CONTENT_TARGET;
const LOCAL_FEATURE_FLAG: &str = "m02-local-runtime";
#[allow(clippy::cast_lossless)] // `From::from` is not const-stable for this conversion.
const TICK_NANOS: u64 = 1_000_000_000 / SIMULATION_HZ as u64;

#[derive(Debug, Clone, Copy)]
struct SystemIdentityClock;

impl IdentityClock for SystemIdentityClock {
    fn unix_millis(&self) -> u64 {
        u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        )
        .unwrap_or(u64::MAX)
    }
}

#[derive(Debug, Default)]
struct ProcessCharacterIds(AtomicU64);

impl CharacterIdGenerator for ProcessCharacterIds {
    fn next_id(&self) -> [u8; 16] {
        let ordinal = self.0.fetch_add(1, Ordering::Relaxed).saturating_add(1);
        let hash = blake3::hash(&ordinal.to_le_bytes());
        let mut id = [0; 16];
        id.copy_from_slice(&hash.as_bytes()[..16]);
        id
    }
}

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

#[derive(Debug, Clone)]
pub struct CoreIdentityServerConfig {
    pub bind_address: SocketAddr,
    pub content_root: PathBuf,
}

impl Default for CoreIdentityServerConfig {
    fn default() -> Self {
        Self {
            bind_address: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 50_001),
            content_root: PathBuf::from("content"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreIdentityServerReport {
    pub accepted_connections: u64,
    pub rejected_connections: u64,
    pub combat_sessions_admitted: u64,
    pub persistence_enabled: bool,
}

type CoreIdentityAuthority<R> =
    IdentityService<R, SystemIdentityClock, ProcessCharacterIds, NoopIdentityEventSink>;

struct CoreShrineAuthorities {
    oath: CoreOathSelectionAuthority<SystemIdentityClock>,
    bargain: CoreBargainAuthority<SystemIdentityClock>,
}

struct CoreReadRepositories<Progression, DeathViews> {
    progression: Progression,
    death_views: DeathViews,
}

/// Explicit Core-development endpoint. It never creates an [`InstanceScheduler`] and therefore
/// cannot silently route identity clients into the M02 combat laboratory.
pub struct BoundCoreIdentityServer<
    R = InMemoryAccountRepository,
    W = InMemoryAccountRepository,
    P = DisabledProgressionQueryRepository,
    D = DisabledDeathViewRepository,
> {
    endpoint: quinn::Endpoint,
    certificate: CertificateDer<'static>,
    local_address: SocketAddr,
    policy: HandshakePolicy,
    authority: Arc<CoreIdentityAuthority<R>>,
    world_flow: Arc<WorldFlowGateService<W, SystemIdentityClock>>,
    progression: Arc<ProgressionQueryService<P>>,
    death_views: Arc<DeathViewService<D>>,
    oath: Arc<CoreOathSelectionAuthority<SystemIdentityClock>>,
    bargain: Arc<CoreBargainAuthority<SystemIdentityClock>>,
    safe_inventory: Arc<CoreSafeInventoryAuthority>,
    persistence_enabled: bool,
}

impl<R, W, P, D> fmt::Debug for BoundCoreIdentityServer<R, W, P, D> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BoundCoreIdentityServer")
            .field("local_address", &self.local_address)
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl BoundCoreIdentityServer {
    pub fn bind(config: &CoreIdentityServerConfig) -> Result<Self, LocalServerRuntimeError> {
        let repository = InMemoryAccountRepository::default();
        let progression_content =
            sim_content::load_core_development_progression(&config.content_root)
                .map_err(|error| LocalServerRuntimeError::Content(error.to_string()))?;
        Self::bind_with_repositories(
            config,
            repository.clone(),
            repository,
            CoreReadRepositories {
                progression: DisabledProgressionQueryRepository,
                death_views: DisabledDeathViewRepository,
            },
            &progression_content,
            CoreShrineAuthorities {
                oath: CoreOathSelectionAuthority::disabled(),
                bargain: CoreBargainAuthority::disabled(),
            },
            false,
        )
    }
}

impl
    BoundCoreIdentityServer<
        PostgresAccountRepository,
        PostgresWorldFlowLocationRepository,
        PostgresProgressionQueryRepository,
        PostgresDeathViewRepository,
    >
{
    pub fn bind_persistent(
        config: &CoreIdentityServerConfig,
        repository: PostgresAccountRepository,
    ) -> Result<Self, LocalServerRuntimeError> {
        let persistence = repository.persistence();
        let progression_content =
            sim_content::load_core_development_progression(&config.content_root)
                .map_err(|error| LocalServerRuntimeError::Content(error.to_string()))?;
        let world_flow = PostgresWorldFlowLocationRepository::new(persistence.clone());
        let progression =
            PostgresProgressionQueryRepository::new(persistence.clone(), &progression_content)
                .map_err(|error| LocalServerRuntimeError::Content(error.to_string()))?;
        let death_content = sim_content::load_core_development_world_flow(&config.content_root)
            .map_err(|error| LocalServerRuntimeError::Content(error.to_string()))?;
        let death_revision = protocol::DeathViewContentRevisionV1 {
            records_blake3: ManifestHash::new(death_content.hashes().records_blake3.clone())?,
            assets_blake3: ManifestHash::new(death_content.hashes().assets_blake3.clone())?,
            localization_blake3: ManifestHash::new(
                death_content.hashes().localization_blake3.clone(),
            )?,
        };
        let death_views = PostgresDeathViewRepository::new(persistence.clone(), death_revision);
        let oath_content = sim_content::load_core_development_oaths_bargains(&config.content_root)
            .map_err(|error| LocalServerRuntimeError::Content(error.to_string()))?;
        let oath = PostgresOathSelectionService::new(
            persistence.clone(),
            SystemIdentityClock,
            &oath_content,
        )
        .map(CoreOathSelectionAuthority::persistent)
        .map_err(|error| LocalServerRuntimeError::Content(error.to_string()))?;
        let bargain = PostgresBargainService::new(persistence, SystemIdentityClock, &oath_content)
            .map(CoreBargainAuthority::persistent)
            .map_err(|error| LocalServerRuntimeError::Content(error.to_string()))?;
        Self::bind_with_repositories(
            config,
            repository,
            world_flow,
            CoreReadRepositories {
                progression,
                death_views,
            },
            &progression_content,
            CoreShrineAuthorities { oath, bargain },
            true,
        )
    }
}

impl<R, W, P, D> BoundCoreIdentityServer<R, W, P, D>
where
    R: AccountRepository + 'static,
    W: WorldFlowLocationRepository + 'static,
    P: ProgressionQueryRepository + 'static,
    D: DeathViewRepository + 'static,
{
    fn bind_with_repositories(
        config: &CoreIdentityServerConfig,
        repository: R,
        world_flow_repository: W,
        reads: CoreReadRepositories<P, D>,
        progression_content: &sim_content::CoreDevelopmentProgression,
        shrines: CoreShrineAuthorities,
        persistence_enabled: bool,
    ) -> Result<Self, LocalServerRuntimeError> {
        sim_content::load_core_development_identity(&config.content_root)
            .map_err(|error| LocalServerRuntimeError::Content(error.to_string()))?;
        let (_, source_report) = sim_content::load_and_validate(&config.content_root)
            .map_err(|error| LocalServerRuntimeError::Content(error.to_string()))?;
        let required_manifest_hash = ManifestHash::new(source_report.package_hash_blake3)?;
        let world_flow_content =
            sim_content::load_core_development_world_flow(&config.content_root)
                .map_err(|error| LocalServerRuntimeError::Content(error.to_string()))?;
        let world_flow_revision = protocol::WorldFlowContentRevisionV1 {
            records_blake3: ManifestHash::new(world_flow_content.hashes().records_blake3.clone())?,
            assets_blake3: ManifestHash::new(world_flow_content.hashes().assets_blake3.clone())?,
            localization_blake3: ManifestHash::new(
                world_flow_content.hashes().localization_blake3.clone(),
            )?,
        };
        let death_view_revision = protocol::DeathViewContentRevisionV1 {
            records_blake3: world_flow_revision.records_blake3.clone(),
            assets_blake3: world_flow_revision.assets_blake3.clone(),
            localization_blake3: world_flow_revision.localization_blake3.clone(),
        };
        let CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(vec![LOCAL_SERVER_NAME.to_owned()])?;
        let certificate = cert.der().clone();
        let private_key = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
        let server_config =
            quinn::ServerConfig::with_single_cert(vec![certificate.clone()], private_key.into())?;
        let endpoint = quinn::Endpoint::server(server_config, config.bind_address)?;
        let local_address = endpoint.local_addr()?;
        let mut feature_flags = vec![WireText::new(CORE_TEST_IDENTITY_FEATURE_FLAG)?];
        if persistence_enabled {
            feature_flags.push(WireText::new(protocol::CORE_DEATH_VIEW_FEATURE_FLAG)?);
        }
        let policy = HandshakePolicy {
            required_protocol: ProtocolVersion::current(),
            required_client_build: WireText::new(CORE_IDENTITY_BUILD_ID)?,
            required_manifest_hash: required_manifest_hash.clone(),
            content_bundle_version: WireText::new(CORE_IDENTITY_CONTENT_TARGET)?,
            region_id: WireText::new(LOCAL_REGION_ID)?,
            feature_flags,
            admission: AdmissionState::Available,
        };
        let authority = Arc::new(IdentityService::new(
            repository,
            SystemIdentityClock,
            ProcessCharacterIds::default(),
            NoopIdentityEventSink,
            required_manifest_hash.clone(),
        ));
        let world_flow = Arc::new(WorldFlowGateService::new(
            world_flow_repository,
            SystemIdentityClock,
            world_flow_revision,
        ));
        let progression = Arc::new(
            ProgressionQueryService::new(reads.progression, progression_content)
                .map_err(|error| LocalServerRuntimeError::Content(error.to_string()))?,
        );
        let death_views = Arc::new(DeathViewService::new(
            reads.death_views,
            death_view_revision,
        ));
        let oath = Arc::new(shrines.oath);
        let bargain = Arc::new(shrines.bargain);
        let safe_inventory = Arc::new(CoreSafeInventoryAuthority::disabled());
        Ok(Self {
            endpoint,
            certificate,
            local_address,
            policy,
            authority,
            world_flow,
            progression,
            death_views,
            oath,
            bargain,
            safe_inventory,
            persistence_enabled,
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
    ) -> Result<CoreIdentityServerReport, LocalServerRuntimeError>
    where
        F: Future<Output = ()>,
    {
        info!(
            address = %self.local_address,
            feature_id = if self.persistence_enabled { "GB-M03-02B" } else { "GB-M03-01B" },
            persistence_enabled = self.persistence_enabled,
            "Core identity server ready"
        );
        let accepted = Arc::new(AtomicU64::new(0));
        let rejected = Arc::new(AtomicU64::new(0));
        let mut workers = JoinSet::new();
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                biased;
                () = &mut shutdown => break,
                incoming = self.endpoint.accept() => {
                    let Some(incoming) = incoming else { break };
                    let policy = self.policy.clone();
                    let authority = Arc::clone(&self.authority);
                    let world_flow = Arc::clone(&self.world_flow);
                    let progression = Arc::clone(&self.progression);
                    let death_views = Arc::clone(&self.death_views);
                    let oath = Arc::clone(&self.oath);
                    let bargain = Arc::clone(&self.bargain);
                    let safe_inventory = Arc::clone(&self.safe_inventory);
                    let accepted = Arc::clone(&accepted);
                    let rejected = Arc::clone(&rejected);
                    workers.spawn(async move {
                        match serve_core_identity_connection(incoming, policy, authority, world_flow, progression, death_views, oath, bargain, safe_inventory).await {
                            Ok(true) => { accepted.fetch_add(1, Ordering::Relaxed); }
                            Ok(false) => { rejected.fetch_add(1, Ordering::Relaxed); }
                            Err(error) => warn!(%error, "Core identity connection ended"),
                        }
                    });
                }
                completed = workers.join_next(), if !workers.is_empty() => {
                    if let Some(Err(error)) = completed {
                        warn!(%error, "Core identity task panicked or was cancelled");
                    }
                }
            }
        }
        self.endpoint.close(
            SERVER_SHUTDOWN_CLOSE_CODE.into(),
            b"Core identity server shutdown",
        );
        while workers.join_next().await.is_some() {}
        self.endpoint.wait_idle().await;
        Ok(CoreIdentityServerReport {
            accepted_connections: accepted.load(Ordering::Relaxed),
            rejected_connections: rejected.load(Ordering::Relaxed),
            combat_sessions_admitted: 0,
            persistence_enabled: self.persistence_enabled,
        })
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "each injected authority retains an independently auditable fail-closed boundary"
)]
async fn serve_core_identity_connection<R, W, P, D>(
    incoming: quinn::Incoming,
    policy: HandshakePolicy,
    authority: Arc<CoreIdentityAuthority<R>>,
    world_flow: Arc<WorldFlowGateService<W, SystemIdentityClock>>,
    progression: Arc<ProgressionQueryService<P>>,
    death_views: Arc<DeathViewService<D>>,
    oath: Arc<CoreOathSelectionAuthority<SystemIdentityClock>>,
    bargain: Arc<CoreBargainAuthority<SystemIdentityClock>>,
    safe_inventory: Arc<CoreSafeInventoryAuthority>,
) -> Result<bool, LocalServerRuntimeError>
where
    R: AccountRepository,
    W: WorldFlowLocationRepository,
    P: ProgressionQueryRepository,
    D: DeathViewRepository,
{
    let connection = incoming.await?;
    let (mut send, mut receive) = connection.accept_bi().await?;
    let request = receive.read_to_end(RELIABLE_FRAME_LIMIT).await?;
    let WireMessage::ClientHello(hello) = decode_frame(&request)? else {
        return Err(LocalServerRuntimeError::UnexpectedHandshake);
    };
    let response = policy.evaluate(
        &hello,
        AuthenticationDecision::Accepted,
        WireText::new("core-identity-session")?,
    );
    send.write_all(&encode_frame(&WireMessage::HandshakeResponse(
        response.clone(),
    ))?)
    .await?;
    send.finish()?;
    if !matches!(response, HandshakeResponse::Accepted(_)) {
        return Ok(false);
    }
    let hash = blake3::hash(hello.auth_ticket.expose_for_validation());
    let mut account_bytes = [0; 16];
    account_bytes.copy_from_slice(&hash.as_bytes()[..16]);
    let account_id =
        AccountId::new(account_bytes).ok_or(LocalServerRuntimeError::IdentityExhausted)?;
    let authenticated = AuthenticatedAccount {
        account_id,
        namespace: AuthenticatedNamespace::WipeableTest,
    };
    let mut response_sequence = 0_u32;
    loop {
        response_sequence = response_sequence
            .checked_add(1)
            .ok_or(LocalServerRuntimeError::IdentityExhausted)?;
        if serve_core_reliable(
            &connection,
            authority.as_ref(),
            world_flow.as_ref(),
            progression.as_ref(),
            death_views.as_ref(),
            oath.as_ref(),
            bargain.as_ref(),
            safe_inventory.as_ref(),
            authenticated,
            response_sequence,
            0,
        )
        .await
        .is_err()
        {
            break;
        }
    }
    Ok(true)
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
        AccountBootstrapFrame, AccountBootstrapRequest, AccountBootstrapResult, ActionFrame,
        ActionKind, AuthTicket, CharacterMutationFrame, CharacterMutationPayload, Compression,
        InputFrame, Platform, SessionControlFrame, SessionControlRequest, WorldFlowFrame,
        WorldFlowRequest, WorldFlowResult, WorldTransferCommand, WorldTransferMutation,
        WorldTransferPayload, WorldTransferResultCode,
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

    fn core_hello(content_root: &Path, ticket: &[u8]) -> ClientHello {
        let (_, report) = sim_content::load_and_validate(content_root).unwrap();
        ClientHello {
            protocol_major: ProtocolVersion::current().major,
            protocol_minor: ProtocolVersion::current().minor,
            client_build_id: WireText::new(M03_CORE_DEV_BUILD_ID).unwrap(),
            platform: Platform::WindowsNative,
            supported_compression: vec![Compression::None],
            content_manifest_hash: ManifestHash::new(report.package_hash_blake3).unwrap(),
            auth_ticket: AuthTicket::new(ticket.to_vec()).unwrap(),
            locale: WireText::new("en-US").unwrap(),
        }
    }

    fn bootstrap(content_root: &Path, sequence: u32) -> AccountBootstrapFrame {
        let (_, report) = sim_content::load_and_validate(content_root).unwrap();
        AccountBootstrapFrame {
            sequence,
            request: AccountBootstrapRequest::Bootstrap,
            content_manifest_hash: ManifestHash::new(report.package_hash_blake3).unwrap(),
        }
    }

    fn world_flow_revision(content_root: &Path) -> protocol::WorldFlowContentRevisionV1 {
        let compiled = sim_content::load_core_development_world_flow(content_root).unwrap();
        protocol::WorldFlowContentRevisionV1 {
            records_blake3: ManifestHash::new(compiled.hashes().records_blake3.clone()).unwrap(),
            assets_blake3: ManifestHash::new(compiled.hashes().assets_blake3.clone()).unwrap(),
            localization_blake3: ManifestHash::new(compiled.hashes().localization_blake3.clone())
                .unwrap(),
        }
    }

    fn current_unix_millis() -> u64 {
        u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap()
    }

    async fn assert_core_world_flow_is_fail_closed(
        connection: &quinn::Connection,
        content_root: &Path,
        created: &protocol::CharacterMutationResult,
    ) {
        let created_snapshot = created.snapshot.as_ref().unwrap();
        let character_id = created_snapshot.characters[0].character_id;
        let select_payload = CharacterMutationPayload::Select { character_id };
        let (_, selected) = bot_client::perform_character_mutation(
            connection,
            CharacterMutationFrame {
                mutation_id: [2; 16],
                expected_account_version: created_snapshot.account_version,
                payload_hash: select_payload.canonical_hash(),
                issued_at_unix_millis: current_unix_millis(),
                payload: select_payload,
            },
        )
        .await
        .unwrap();
        assert!(selected.accepted);

        let transfer_payload = WorldTransferPayload {
            content_revision: world_flow_revision(content_root),
            command: WorldTransferCommand::EnterHallFromCharacterSelect,
        };
        let (_, result) = bot_client::perform_world_flow(
            connection,
            WorldFlowFrame {
                sequence: 3,
                request: WorldFlowRequest::Transfer(WorldTransferMutation {
                    mutation_id: [3; 16],
                    character_id,
                    expected_character_version: 1,
                    issued_at_unix_millis: current_unix_millis(),
                    payload_hash: transfer_payload.canonical_hash(),
                    payload: transfer_payload,
                }),
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            result,
            WorldFlowResult::Transfer {
                accepted: false,
                code: WorldTransferResultCode::StageDisabled,
                transfer_id: None,
                snapshot: Some(_),
                ..
            }
        ));
    }

    #[tokio::test]
    async fn core_identity_real_quic_reconnects_and_server_restart_wipes() {
        let content_root = content_root();
        let server = BoundCoreIdentityServer::bind(&CoreIdentityServerConfig {
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
        let ticket = b"core-runtime-test-account";
        let connection = endpoint
            .connect(address, LOCAL_SERVER_NAME)
            .unwrap()
            .await
            .unwrap();
        let handshake =
            bot_client::perform_handshake(&connection, core_hello(&content_root, ticket))
                .await
                .unwrap();
        let HandshakeResponse::Accepted(server_hello) = handshake else {
            panic!("Core handshake must be accepted")
        };
        assert!(
            server_hello
                .feature_flags
                .iter()
                .all(|flag| flag.as_str() != protocol::CORE_WORLD_FLOW_FEATURE_FLAG)
        );
        assert!(
            server_hello
                .feature_flags
                .iter()
                .all(|flag| flag.as_str() != protocol::CORE_SAFE_INVENTORY_FEATURE_FLAG)
        );
        assert!(
            server_hello
                .feature_flags
                .iter()
                .all(|flag| flag.as_str() != protocol::CORE_DEATH_VIEW_FEATURE_FLAG)
        );
        let (_, initial) =
            bot_client::perform_account_bootstrap(&connection, bootstrap(&content_root, 1))
                .await
                .unwrap();
        let AccountBootstrapResult::Snapshot(initial) = initial else {
            panic!("initial Core account snapshot")
        };
        assert!(initial.characters.is_empty());
        let payload = CharacterMutationPayload::Create {
            class_id: WireText::new(protocol::GRAVE_ARBALIST_CLASS_ID).unwrap(),
        };
        let (_, created) = bot_client::perform_character_mutation(
            &connection,
            CharacterMutationFrame {
                mutation_id: [1; 16],
                expected_account_version: 1,
                payload_hash: payload.canonical_hash(),
                issued_at_unix_millis: current_unix_millis(),
                payload,
            },
        )
        .await
        .unwrap();
        assert!(created.accepted);
        assert_core_world_flow_is_fail_closed(&connection, &content_root, &created).await;
        connection.close(0_u32.into(), b"reconnect");

        let connection = endpoint
            .connect(address, LOCAL_SERVER_NAME)
            .unwrap()
            .await
            .unwrap();
        bot_client::perform_handshake(&connection, core_hello(&content_root, ticket))
            .await
            .unwrap();
        let (_, reconnected) =
            bot_client::perform_account_bootstrap(&connection, bootstrap(&content_root, 2))
                .await
                .unwrap();
        let AccountBootstrapResult::Snapshot(reconnected) = reconnected else {
            panic!("reconnected Core account snapshot")
        };
        assert_eq!(reconnected.characters.len(), 1);
        connection.close(0_u32.into(), b"restart test");
        shutdown_send.send(()).unwrap();
        let report = server_task.await.unwrap().unwrap();
        assert_eq!(report.combat_sessions_admitted, 0);
        assert!(!report.persistence_enabled);
        endpoint.wait_idle().await;

        assert_core_restart_wipes(&content_root, ticket).await;
    }

    async fn assert_core_restart_wipes(content_root: &Path, ticket: &[u8]) {
        let restarted = BoundCoreIdentityServer::bind(&CoreIdentityServerConfig {
            bind_address: "127.0.0.1:0".parse().unwrap(),
            content_root: content_root.to_path_buf(),
        })
        .unwrap();
        let address = restarted.local_address();
        let certificate = CertificateDer::from(restarted.certificate_der().to_vec());
        let (shutdown_send, shutdown_receive) = oneshot::channel::<()>();
        let restarted_task = tokio::spawn(restarted.serve_until(async {
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
        bot_client::perform_handshake(&connection, core_hello(content_root, ticket))
            .await
            .unwrap();
        let (_, wiped) =
            bot_client::perform_account_bootstrap(&connection, bootstrap(content_root, 1))
                .await
                .unwrap();
        let AccountBootstrapResult::Snapshot(wiped) = wiped else {
            panic!("wiped Core account snapshot")
        };
        assert!(wiped.characters.is_empty());
        connection.close(0_u32.into(), b"complete");
        shutdown_send.send(()).unwrap();
        restarted_task.await.unwrap().unwrap();
        endpoint.wait_idle().await;
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
            Duration::from_secs(10),
            bot_client::receive_snapshot_datagram(&connection),
        )
        .await
        .expect("server emitted a snapshot after the participant-lock window")
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
                code: protocol::ActionResultCode::RecallUnavailableCombatLaboratory,
                ..
            }
        ));
        assert_eq!(recall.server_tick, snapshot.server_tick);

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
    #[allow(clippy::too_many_lines)] // Four recipient routes stay explicit for shared-state review.
    async fn four_concurrent_clients_share_one_authoritative_world() {
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
            clients.push((
                connection,
                joined
                    .controlled_entity_id
                    .expect("controlled player binding"),
            ));
        }

        let directions = [(1_000, 0), (-1_000, 0), (0, -1_000), (0, 1_000)];
        for ((connection, _), (x, y)) in clients.iter().zip(directions) {
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
        let mut shared_enemy_facts = Vec::new();
        for (connection, controlled_entity_id) in &clients {
            let (player, player_count, enemy_fact) =
                tokio::time::timeout(Duration::from_secs(2), async {
                    loop {
                        let snapshot = bot_client::receive_snapshot_datagram(connection)
                            .await
                            .unwrap();
                        if snapshot.acknowledged_input_sequence == 1 {
                            let player_count = snapshot
                                .entities
                                .iter()
                                .filter(|entity| entity.kind == protocol::EntityKind::Player)
                                .count();
                            let enemy_fact = snapshot
                                .entities
                                .iter()
                                .find(|entity| entity.kind == protocol::EntityKind::Enemy)
                                .map(|entity| {
                                    (
                                        entity.entity_id,
                                        entity.current_health,
                                        entity.maximum_health,
                                    )
                                })
                                .expect("shared enemy snapshot");
                            let player = snapshot
                                .entities
                                .into_iter()
                                .find(|entity| entity.entity_id == *controlled_entity_id)
                                .unwrap();
                            break (player, player_count, enemy_fact);
                        }
                    }
                })
                .await
                .expect("each client received its routed snapshot");
            positions.push((player.x_milli_tiles, player.y_milli_tiles));
            assert_eq!(player_count, 4);
            shared_enemy_facts.push(enemy_fact);
        }
        assert!(positions[0].0 > 4_000);
        assert!(positions[1].0 < 4_000);
        assert!(positions[2].1 < 12_000);
        assert!(positions[3].1 > 12_000);
        assert!(shared_enemy_facts.windows(2).all(|pair| pair[0] == pair[1]));

        shutdown_send.send(()).unwrap();
        let report = server_task.await.unwrap().unwrap();
        assert_eq!(report.accepted_connections, 4);
        assert_eq!(report.admitted_sessions, 4);
        assert_eq!(report.malformed_messages, 0);
        assert!(report.zero_residue);
        endpoint.wait_idle().await;
    }
}
