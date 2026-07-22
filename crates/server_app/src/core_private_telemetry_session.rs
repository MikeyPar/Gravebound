//! Optional durable logical-session telemetry for the normal Core private-life server.
//!
//! The canonical GDD (`TECH-123`, `TEL-001`-`005`), Content Production Specification
//! (`CONT-002` and exact Core stable IDs), and Development Roadmap (`ADR-005`, `GB-M03-09`)
//! require committed telemetry facts without making analytics a gameplay writer or availability
//! dependency. This coordinator owns only logical-session observation. It receives an already
//! derived account ID, never an authentication ticket, and contains every persistence failure.

use std::{collections::BTreeMap, fmt, future::Future, pin::Pin, sync::Arc, time::Duration};

use persistence::{
    M03CrashObservationCommandV1, M03SessionObservationCommandV1, M03SessionObservationV1,
    M03TelemetrySessionStartV1, M03TelemetrySourceError, PostgresPersistence, StoredM03CrashKindV1,
    StoredM03CrashReporterV1, StoredM03CrashSourceV1, StoredM03SessionEndReasonV1,
    StoredM03SessionEventV1, StoredM03TelemetryEnvironmentV1, StoredM03TelemetryEventV1,
    StoredM03TelemetryPlatformV1, StoredM03TelemetrySessionV1,
};
use protocol::{NativeCrashKindV1, NativeCrashReportFrameV1, Platform};
use tokio::sync::Mutex;
use tracing::warn;

use crate::core_private_life_foundation::SystemIdentityClock;
use crate::{CORE_IDENTITY_BUILD_ID, CORE_IDENTITY_CONTENT_TARGET, IdentityClock, LOCAL_REGION_ID};

const TELEMETRY_ENVIRONMENT_VARIABLE: &str = "GRAVEBOUND_TELEMETRY_ENVIRONMENT";
const TELEMETRY_REGION_VARIABLE: &str = "GRAVEBOUND_TELEMETRY_REGION_ID";
const TELEMETRY_OPERATION_TIMEOUT: Duration = Duration::from_secs(1);

type RepositoryFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, M03TelemetrySourceError>> + Send + 'a>>;

trait CorePrivateTelemetrySessionRepository: Send + Sync {
    fn load_open(
        &self,
        account_id: [u8; 16],
    ) -> RepositoryFuture<'_, Option<StoredM03TelemetrySessionV1>>;

    fn load_head(
        &self,
        account_id: [u8; 16],
        session_id: [u8; 16],
    ) -> RepositoryFuture<'_, StoredM03SessionEventV1>;

    fn begin<'a>(&'a self, command: &'a M03TelemetrySessionStartV1) -> RepositoryFuture<'a, ()>;

    fn observe<'a>(
        &'a self,
        command: &'a M03SessionObservationCommandV1,
    ) -> RepositoryFuture<'a, ()>;

    fn record_crash<'a>(
        &'a self,
        command: &'a M03CrashObservationCommandV1,
    ) -> RepositoryFuture<'a, ()>;
}

impl CorePrivateTelemetrySessionRepository for PostgresPersistence {
    fn load_open(
        &self,
        account_id: [u8; 16],
    ) -> RepositoryFuture<'_, Option<StoredM03TelemetrySessionV1>> {
        Box::pin(async move { self.load_open_m03_telemetry_session_v1(account_id).await })
    }

    fn load_head(
        &self,
        account_id: [u8; 16],
        session_id: [u8; 16],
    ) -> RepositoryFuture<'_, StoredM03SessionEventV1> {
        Box::pin(async move {
            let source = self
                .load_m03_telemetry_session_head_v1(account_id, session_id)
                .await?;
            let StoredM03TelemetryEventV1::Session(event) = source.event else {
                return Err(M03TelemetrySourceError::CorruptStoredSource);
            };
            Ok(event)
        })
    }

    fn begin<'a>(&'a self, command: &'a M03TelemetrySessionStartV1) -> RepositoryFuture<'a, ()> {
        Box::pin(async move {
            self.begin_m03_telemetry_session_v1(command).await?;
            Ok(())
        })
    }

    fn observe<'a>(
        &'a self,
        command: &'a M03SessionObservationCommandV1,
    ) -> RepositoryFuture<'a, ()> {
        Box::pin(async move {
            self.record_m03_session_observation_v1(command).await?;
            Ok(())
        })
    }

    fn record_crash<'a>(
        &'a self,
        command: &'a M03CrashObservationCommandV1,
    ) -> RepositoryFuture<'a, ()> {
        Box::pin(async move {
            self.record_m03_crash_observation_v1(command).await?;
            Ok(())
        })
    }
}

trait CorePrivateTelemetryIdentitySource: Send + Sync {
    fn next_uuid_v7(&self) -> [u8; 16];
}

#[derive(Debug)]
struct SystemCorePrivateTelemetryIdentitySource;

impl CorePrivateTelemetryIdentitySource for SystemCorePrivateTelemetryIdentitySource {
    fn next_uuid_v7(&self) -> [u8; 16] {
        *uuid::Uuid::now_v7().as_bytes()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CorePrivateTelemetrySessionContext {
    build_id: String,
    content_bundle_version: String,
    region_id: String,
    environment: StoredM03TelemetryEnvironmentV1,
    cohort_tags: Vec<String>,
}

impl CorePrivateTelemetrySessionContext {
    fn configured() -> Option<Self> {
        let environment = match std::env::var(TELEMETRY_ENVIRONMENT_VARIABLE).as_deref() {
            Ok("local") => StoredM03TelemetryEnvironmentV1::Local,
            Ok("test") => StoredM03TelemetryEnvironmentV1::Test,
            Ok("staging") => StoredM03TelemetryEnvironmentV1::Staging,
            Ok("production") => StoredM03TelemetryEnvironmentV1::Production,
            Err(std::env::VarError::NotPresent) => return None,
            Ok(_) | Err(std::env::VarError::NotUnicode(_)) => {
                warn!(
                    variable = TELEMETRY_ENVIRONMENT_VARIABLE,
                    "telemetry environment is invalid; logical-session telemetry is disabled"
                );
                return None;
            }
        };
        let region_id = match std::env::var(TELEMETRY_REGION_VARIABLE) {
            Ok(value) if valid_stable_context_id(&value) => value,
            Err(std::env::VarError::NotPresent)
                if matches!(
                    environment,
                    StoredM03TelemetryEnvironmentV1::Local | StoredM03TelemetryEnvironmentV1::Test
                ) =>
            {
                LOCAL_REGION_ID.to_owned()
            }
            Ok(_) | Err(_) => {
                warn!(
                    variable = TELEMETRY_REGION_VARIABLE,
                    "telemetry region is missing or invalid; logical-session telemetry is disabled"
                );
                return None;
            }
        };
        Some(Self {
            build_id: CORE_IDENTITY_BUILD_ID.to_owned(),
            content_bundle_version: CORE_IDENTITY_CONTENT_TARGET.to_owned(),
            region_id,
            environment,
            cohort_tags: vec!["cohort.private".to_owned()],
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CorePrivateTelemetryTransportLease {
    account_id: [u8; 16],
    generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CorePrivateCrashRecordOutcome {
    Accepted,
    Unavailable,
    IdempotencyConflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrackedLogicalSession {
    session_id: [u8; 16],
    crash_report_session_id: [u8; 16],
    active_generation: u64,
    next_generation: u64,
    connected: bool,
}

#[derive(Debug, Default)]
struct CoordinatorState {
    accepting: bool,
    shutdown_started: bool,
    sessions: BTreeMap<[u8; 16], TrackedLogicalSession>,
}

/// Process-local transport coordinator backed by schema-70 durable logical sessions.
///
/// Public connection handling receives only an optional lease. Failure to allocate or observe a
/// telemetry session is logged without changing authentication, bootstrap, or gameplay control.
pub(crate) struct CorePrivateTelemetrySessionCoordinator {
    repository: Arc<dyn CorePrivateTelemetrySessionRepository>,
    clock: Arc<dyn IdentityClock>,
    identities: Arc<dyn CorePrivateTelemetryIdentitySource>,
    context: CorePrivateTelemetrySessionContext,
    state: Mutex<CoordinatorState>,
}

impl fmt::Debug for CorePrivateTelemetrySessionCoordinator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CorePrivateTelemetrySessionCoordinator")
            .field("context", &self.context)
            .finish_non_exhaustive()
    }
}

impl CorePrivateTelemetrySessionCoordinator {
    pub(crate) fn persistent(persistence: PostgresPersistence) -> Option<Self> {
        let context = CorePrivateTelemetrySessionContext::configured()?;
        Some(Self::new(
            Arc::new(persistence),
            Arc::new(SystemIdentityClock),
            Arc::new(SystemCorePrivateTelemetryIdentitySource),
            context,
        ))
    }

    fn new(
        repository: Arc<dyn CorePrivateTelemetrySessionRepository>,
        clock: Arc<dyn IdentityClock>,
        identities: Arc<dyn CorePrivateTelemetryIdentitySource>,
        context: CorePrivateTelemetrySessionContext,
    ) -> Self {
        Self {
            repository,
            clock,
            identities,
            context,
            state: Mutex::new(CoordinatorState {
                accepting: true,
                ..CoordinatorState::default()
            }),
        }
    }

    /// Begins or recovers the durable logical session before account/bootstrap writes occur.
    /// A currently connected account is a transport handoff, not a disconnect/reconnect pair.
    pub(crate) async fn attach(
        &self,
        account_id: [u8; 16],
        platform: Platform,
    ) -> Option<CorePrivateTelemetryTransportLease> {
        let Ok(lease) = tokio::time::timeout(
            TELEMETRY_OPERATION_TIMEOUT,
            self.attach_inner(account_id, platform),
        )
        .await
        else {
            warn!("telemetry session attach timed out; gameplay continues");
            return None;
        };
        lease
    }

    async fn attach_inner(
        &self,
        account_id: [u8; 16],
        platform: Platform,
    ) -> Option<CorePrivateTelemetryTransportLease> {
        let mut state = self.state.lock().await;
        if !state.accepting {
            return None;
        }
        if let Some(tracked) = state.sessions.get_mut(&account_id) {
            return self.attach_tracked(account_id, tracked).await;
        }
        let (session_id, crash_report_session_id) =
            self.begin_or_recover(account_id, platform).await?;
        state.sessions.insert(
            account_id,
            TrackedLogicalSession {
                session_id,
                crash_report_session_id,
                active_generation: 1,
                next_generation: 2,
                connected: true,
            },
        );
        Some(CorePrivateTelemetryTransportLease {
            account_id,
            generation: 1,
        })
    }

    async fn attach_tracked(
        &self,
        account_id: [u8; 16],
        tracked: &mut TrackedLogicalSession,
    ) -> Option<CorePrivateTelemetryTransportLease> {
        let generation = tracked.next_generation;
        let Some(next_generation) = generation.checked_add(1) else {
            warn!("telemetry transport generation exhausted; gameplay continues");
            return None;
        };
        tracked.next_generation = next_generation;
        if !tracked.connected
            && !self
                .observe(
                    account_id,
                    tracked.session_id,
                    M03SessionObservationV1::Reconnected,
                )
                .await
        {
            return None;
        }
        tracked.active_generation = generation;
        tracked.connected = true;
        Some(CorePrivateTelemetryTransportLease {
            account_id,
            generation,
        })
    }

    async fn begin_or_recover(
        &self,
        account_id: [u8; 16],
        platform: Platform,
    ) -> Option<([u8; 16], [u8; 16])> {
        match self.repository.load_open(account_id).await {
            Ok(Some(stored)) => {
                return self.recover_open(account_id, stored, platform).await;
            }
            Ok(None) => {}
            Err(error) => {
                warn!(%error, "telemetry open-session recovery failed; gameplay continues");
                return None;
            }
        }
        self.start_new(account_id, platform).await
    }

    async fn recover_open(
        &self,
        account_id: [u8; 16],
        stored: StoredM03TelemetrySessionV1,
        platform: Platform,
    ) -> Option<([u8; 16], [u8; 16])> {
        let head = match self
            .repository
            .load_head(account_id, stored.session_id)
            .await
        {
            Ok(head) => head,
            Err(error) => {
                warn!(%error, "telemetry session-head recovery failed; gameplay continues");
                return None;
            }
        };
        match head {
            StoredM03SessionEventV1::Disconnected => self
                .observe(
                    account_id,
                    stored.session_id,
                    M03SessionObservationV1::Reconnected,
                )
                .await
                .then_some((stored.session_id, stored.session_id)),
            StoredM03SessionEventV1::Started | StoredM03SessionEventV1::Reconnected { .. } => {
                let closed = self
                    .observe(
                        account_id,
                        stored.session_id,
                        M03SessionObservationV1::Ended(
                            StoredM03SessionEndReasonV1::TransportClosed,
                        ),
                    )
                    .await;
                if closed {
                    match self.start_new_once(account_id, platform).await {
                        // A panic marker was authored before this next-launch recovery. Bind it
                        // to the just-closed origin session, never to the replacement session.
                        Ok(session_id) => Some((session_id, stored.session_id)),
                        Err(error) => {
                            warn!(%error, "replacement telemetry session start failed; gameplay continues");
                            None
                        }
                    }
                } else {
                    None
                }
            }
            StoredM03SessionEventV1::Ended { .. } => {
                warn!("open telemetry session had a terminal durable head; gameplay continues");
                None
            }
        }
    }

    async fn start_new(
        &self,
        account_id: [u8; 16],
        platform: Platform,
    ) -> Option<([u8; 16], [u8; 16])> {
        match self.start_new_once(account_id, platform).await {
            Ok(session_id) => Some((session_id, session_id)),
            Err(M03TelemetrySourceError::OpenSessionConflict) => {
                if let Ok(Some(stored)) = self.repository.load_open(account_id).await {
                    self.recover_open(account_id, stored, platform).await
                } else {
                    warn!("telemetry session race recovery failed; gameplay continues");
                    None
                }
            }
            Err(error) => {
                warn!(%error, "telemetry session start failed; gameplay continues");
                None
            }
        }
    }

    async fn start_new_once(
        &self,
        account_id: [u8; 16],
        platform: Platform,
    ) -> Result<[u8; 16], M03TelemetrySourceError> {
        let session_id = self.identities.next_uuid_v7();
        let command = M03TelemetrySessionStartV1 {
            session_id,
            account_id,
            build_id: self.context.build_id.clone(),
            content_bundle_version: self.context.content_bundle_version.clone(),
            platform: telemetry_platform(platform),
            region_id: self.context.region_id.clone(),
            environment: self.context.environment,
            cohort_tags: self.context.cohort_tags.clone(),
            started_at_utc_millis: self.clock.unix_millis(),
        };
        self.begin_exact(&command).await?;
        Ok(session_id)
    }

    async fn begin_exact(
        &self,
        command: &M03TelemetrySessionStartV1,
    ) -> Result<(), M03TelemetrySourceError> {
        let first =
            tokio::time::timeout(TELEMETRY_OPERATION_TIMEOUT, self.repository.begin(command))
                .await
                .map_err(|_| M03TelemetrySourceError::Capacity)?;
        match first {
            Ok(()) => Ok(()),
            Err(first) if retryable_observation_error(&first) => {
                tokio::time::timeout(TELEMETRY_OPERATION_TIMEOUT, self.repository.begin(command))
                    .await
                    .map_err(|_| M03TelemetrySourceError::Capacity)?
            }
            Err(error) => Err(error),
        }
    }

    async fn observe(
        &self,
        account_id: [u8; 16],
        session_id: [u8; 16],
        observation: M03SessionObservationV1,
    ) -> bool {
        let command = M03SessionObservationCommandV1 {
            session_id,
            account_id,
            observation_id: self.identities.next_uuid_v7(),
            occurred_at_utc_millis: self.clock.unix_millis(),
            observation,
        };
        let Ok(first) = tokio::time::timeout(
            TELEMETRY_OPERATION_TIMEOUT,
            self.repository.observe(&command),
        )
        .await
        else {
            warn!("telemetry session observation timed out; gameplay continues");
            return false;
        };
        let result = match first {
            Err(ref error) if retryable_observation_error(error) => {
                let Ok(result) = tokio::time::timeout(
                    TELEMETRY_OPERATION_TIMEOUT,
                    self.repository.observe(&command),
                )
                .await
                else {
                    warn!("telemetry session observation retry timed out; gameplay continues");
                    return false;
                };
                result
            }
            result => result,
        };
        match result {
            Ok(()) => true,
            Err(M03TelemetrySourceError::SessionEnded)
                if matches!(observation, M03SessionObservationV1::Ended(_)) =>
            {
                true
            }
            Err(M03TelemetrySourceError::SessionEnded) => {
                warn!(
                    "telemetry session ended during a nonterminal observation; gameplay continues"
                );
                false
            }
            Err(M03TelemetrySourceError::InvalidTransition) => {
                warn!(
                    "telemetry transition contradicted durable session state; gameplay continues"
                );
                false
            }
            Err(error) => {
                warn!(%error, "telemetry session observation failed; gameplay continues");
                false
            }
        }
    }

    /// Durably records one redacted client crash against the currently authenticated logical
    /// session. Client input cannot author account, character, session, source, or reporter.
    pub(crate) async fn record_client_crash(
        &self,
        lease: Option<CorePrivateTelemetryTransportLease>,
        report: &NativeCrashReportFrameV1,
    ) -> CorePrivateCrashRecordOutcome {
        let Some(lease) = lease else {
            return CorePrivateCrashRecordOutcome::Unavailable;
        };
        let crash_report_session_id = {
            let state = self.state.lock().await;
            if state.shutdown_started {
                return CorePrivateCrashRecordOutcome::Unavailable;
            }
            let Some(tracked) = state.sessions.get(&lease.account_id) else {
                return CorePrivateCrashRecordOutcome::Unavailable;
            };
            if !tracked.connected || tracked.active_generation != lease.generation {
                return CorePrivateCrashRecordOutcome::Unavailable;
            }
            tracked.crash_report_session_id
        };
        let command = M03CrashObservationCommandV1 {
            crash_id: report.crash_id,
            account_id: lease.account_id,
            character_id: None,
            session_id: crash_report_session_id,
            source: StoredM03CrashSourceV1::Client,
            kind: crash_kind(report.kind),
            reporter: StoredM03CrashReporterV1::AuthenticatedClient,
            signature: report.signature,
            uptime_millis: report.uptime_millis,
            occurred_at_utc_millis: report.occurred_at_utc_millis,
        };
        let result = tokio::time::timeout(
            TELEMETRY_OPERATION_TIMEOUT,
            self.repository.record_crash(&command),
        )
        .await;
        match result {
            Ok(Ok(())) => CorePrivateCrashRecordOutcome::Accepted,
            Ok(Err(M03TelemetrySourceError::IdempotencyConflict)) => {
                CorePrivateCrashRecordOutcome::IdempotencyConflict
            }
            Ok(Err(error)) => {
                warn!(%error, "client crash observation failed; gameplay continues");
                CorePrivateCrashRecordOutcome::Unavailable
            }
            Err(_) => {
                warn!("client crash observation timed out; gameplay continues");
                CorePrivateCrashRecordOutcome::Unavailable
            }
        }
    }

    /// Records only the current generation's link loss. A replaced transport cannot disconnect
    /// the newer handoff owner when its task exits later.
    pub(crate) async fn detach(&self, lease: Option<CorePrivateTelemetryTransportLease>) {
        let Some(lease) = lease else { return };
        let mut state = self.state.lock().await;
        if state.shutdown_started {
            return;
        }
        let Some(tracked) = state.sessions.get_mut(&lease.account_id) else {
            return;
        };
        if !tracked.connected || tracked.active_generation != lease.generation {
            return;
        }
        tracked.connected = false;
        let _ = self
            .observe(
                lease.account_id,
                tracked.session_id,
                M03SessionObservationV1::Disconnected,
            )
            .await;
    }

    /// Ends only the current transport generation. Native-client clean exit cannot terminate a
    /// newer handoff when its replaced connection task retires later.
    pub(crate) async fn end(
        &self,
        lease: Option<CorePrivateTelemetryTransportLease>,
        reason: StoredM03SessionEndReasonV1,
    ) {
        let Some(lease) = lease else { return };
        let mut state = self.state.lock().await;
        if state.shutdown_started {
            return;
        }
        let Some(tracked) = state.sessions.get(&lease.account_id) else {
            return;
        };
        if !tracked.connected || tracked.active_generation != lease.generation {
            return;
        }
        let session_id = tracked.session_id;
        if self
            .observe(
                lease.account_id,
                session_id,
                M03SessionObservationV1::Ended(reason),
            )
            .await
        {
            state.sessions.remove(&lease.account_id);
        } else if let Some(tracked) = state.sessions.get_mut(&lease.account_id) {
            tracked.connected = false;
        }
    }

    pub(crate) async fn begin_shutdown(&self) {
        let mut state = self.state.lock().await;
        state.accepting = false;
        state.shutdown_started = true;
    }

    /// Ends every logical session owned or recovered by this process. Failures remain observable
    /// in logs but cannot turn telemetry into a server shutdown failure.
    pub(crate) async fn finish_shutdown(&self) {
        let sessions = {
            let mut state = self.state.lock().await;
            std::mem::take(&mut state.sessions)
        };
        for (account_id, tracked) in sessions {
            let _ = self
                .observe(
                    account_id,
                    tracked.session_id,
                    M03SessionObservationV1::Ended(StoredM03SessionEndReasonV1::ServerShutdown),
                )
                .await;
        }
    }
}

const fn telemetry_platform(platform: Platform) -> StoredM03TelemetryPlatformV1 {
    match platform {
        Platform::WindowsNative | Platform::SteamWindows => StoredM03TelemetryPlatformV1::Windows,
    }
}

const fn crash_kind(kind: NativeCrashKindV1) -> StoredM03CrashKindV1 {
    match kind {
        NativeCrashKindV1::Panic => StoredM03CrashKindV1::Panic,
        NativeCrashKindV1::AccessViolation => StoredM03CrashKindV1::AccessViolation,
        NativeCrashKindV1::OutOfMemory => StoredM03CrashKindV1::OutOfMemory,
        NativeCrashKindV1::Watchdog => StoredM03CrashKindV1::Watchdog,
        NativeCrashKindV1::Unknown => StoredM03CrashKindV1::Unknown,
    }
}

fn valid_stable_context_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
        && !["bearer", "token", "secret", "password", "sk_", "eyj"]
            .iter()
            .any(|prefix| value.starts_with(prefix))
        && !value.contains("key=")
}

fn retryable_observation_error(error: &M03TelemetrySourceError) -> bool {
    matches!(
        error,
        M03TelemetrySourceError::Database(_)
            | M03TelemetrySourceError::Persistence(_)
            | M03TelemetrySourceError::PublicationConflict
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex as StdMutex,
        atomic::{AtomicU64, Ordering},
    };

    use persistence::{StoredM03SessionEndReasonV1, StoredM03TelemetrySessionV1};

    use super::*;

    #[derive(Debug, Default)]
    struct FakeRepository {
        state: StdMutex<FakeRepositoryState>,
    }

    #[derive(Debug, Default)]
    struct FakeRepositoryState {
        open: BTreeMap<[u8; 16], StoredM03TelemetrySessionV1>,
        heads: BTreeMap<[u8; 16], StoredM03SessionEventV1>,
        starts: Vec<M03TelemetrySessionStartV1>,
        observations: Vec<M03SessionObservationCommandV1>,
        crashes: Vec<M03CrashObservationCommandV1>,
        fail_load: bool,
    }

    impl CorePrivateTelemetrySessionRepository for FakeRepository {
        fn load_open(
            &self,
            account_id: [u8; 16],
        ) -> RepositoryFuture<'_, Option<StoredM03TelemetrySessionV1>> {
            Box::pin(async move {
                let state = self.state.lock().unwrap();
                if state.fail_load {
                    return Err(M03TelemetrySourceError::SessionNotFound);
                }
                Ok(state.open.get(&account_id).cloned())
            })
        }

        fn load_head(
            &self,
            _account_id: [u8; 16],
            session_id: [u8; 16],
        ) -> RepositoryFuture<'_, StoredM03SessionEventV1> {
            Box::pin(async move {
                self.state
                    .lock()
                    .unwrap()
                    .heads
                    .get(&session_id)
                    .cloned()
                    .ok_or(M03TelemetrySourceError::CorruptStoredSource)
            })
        }

        fn begin<'a>(
            &'a self,
            command: &'a M03TelemetrySessionStartV1,
        ) -> RepositoryFuture<'a, ()> {
            Box::pin(async move {
                let mut state = self.state.lock().unwrap();
                state.starts.push(command.clone());
                state.open.insert(
                    command.account_id,
                    StoredM03TelemetrySessionV1 {
                        session_id: command.session_id,
                        account_id: command.account_id,
                        build_id: command.build_id.clone(),
                        content_bundle_version: command.content_bundle_version.clone(),
                        platform: command.platform,
                        region_id: command.region_id.clone(),
                        environment: command.environment,
                        cohort_tags: command.cohort_tags.clone(),
                        started_at_utc_millis: command.started_at_utc_millis,
                        ended_at_utc_millis: None,
                        end_reason: None,
                    },
                );
                state
                    .heads
                    .insert(command.session_id, StoredM03SessionEventV1::Started);
                Ok(())
            })
        }

        fn observe<'a>(
            &'a self,
            command: &'a M03SessionObservationCommandV1,
        ) -> RepositoryFuture<'a, ()> {
            Box::pin(async move {
                let mut state = self.state.lock().unwrap();
                let head = state
                    .heads
                    .get(&command.session_id)
                    .ok_or(M03TelemetrySourceError::CorruptStoredSource)?;
                let next = match command.observation {
                    M03SessionObservationV1::Disconnected
                        if matches!(
                            head,
                            StoredM03SessionEventV1::Started
                                | StoredM03SessionEventV1::Reconnected { .. }
                        ) =>
                    {
                        StoredM03SessionEventV1::Disconnected
                    }
                    M03SessionObservationV1::Reconnected
                        if matches!(head, StoredM03SessionEventV1::Disconnected) =>
                    {
                        StoredM03SessionEventV1::Reconnected {
                            link_lost_millis: 100,
                        }
                    }
                    M03SessionObservationV1::Ended(reason)
                        if !matches!(head, StoredM03SessionEventV1::Ended { .. }) =>
                    {
                        StoredM03SessionEventV1::Ended {
                            duration_millis: 100,
                            reason,
                        }
                    }
                    _ => return Err(M03TelemetrySourceError::InvalidTransition),
                };
                if matches!(next, StoredM03SessionEventV1::Ended { .. }) {
                    state.open.remove(&command.account_id);
                }
                state.heads.insert(command.session_id, next);
                state.observations.push(command.clone());
                Ok(())
            })
        }

        fn record_crash<'a>(
            &'a self,
            command: &'a M03CrashObservationCommandV1,
        ) -> RepositoryFuture<'a, ()> {
            Box::pin(async move {
                let mut state = self.state.lock().unwrap();
                if let Some(stored) = state
                    .crashes
                    .iter()
                    .find(|stored| stored.crash_id == command.crash_id)
                {
                    return if stored == command {
                        Ok(())
                    } else {
                        Err(M03TelemetrySourceError::IdempotencyConflict)
                    };
                }
                state.crashes.push(command.clone());
                Ok(())
            })
        }
    }

    #[derive(Debug)]
    struct TestClock(AtomicU64);

    impl IdentityClock for TestClock {
        fn unix_millis(&self) -> u64 {
            self.0.fetch_add(100, Ordering::Relaxed)
        }
    }

    #[derive(Debug)]
    struct TestIdentities(AtomicU64);

    impl CorePrivateTelemetryIdentitySource for TestIdentities {
        fn next_uuid_v7(&self) -> [u8; 16] {
            let sequence = self.0.fetch_add(1, Ordering::Relaxed);
            let mut value = [0_u8; 16];
            value[..8].copy_from_slice(&sequence.to_be_bytes());
            value[6] = (value[6] & 0x0f) | 0x70;
            value[8] = (value[8] & 0x3f) | 0x80;
            value
        }
    }

    fn coordinator(repository: Arc<FakeRepository>) -> CorePrivateTelemetrySessionCoordinator {
        CorePrivateTelemetrySessionCoordinator::new(
            repository,
            Arc::new(TestClock(AtomicU64::new(1_000))),
            Arc::new(TestIdentities(AtomicU64::new(1))),
            CorePrivateTelemetrySessionContext {
                build_id: CORE_IDENTITY_BUILD_ID.to_owned(),
                content_bundle_version: CORE_IDENTITY_CONTENT_TARGET.to_owned(),
                region_id: LOCAL_REGION_ID.to_owned(),
                environment: StoredM03TelemetryEnvironmentV1::Test,
                cohort_tags: vec!["cohort.private".to_owned()],
            },
        )
    }

    #[tokio::test]
    async fn handoff_is_one_session_and_stale_detach_cannot_report_link_loss() {
        let repository = Arc::new(FakeRepository::default());
        let coordinator = coordinator(Arc::clone(&repository));
        let first = coordinator
            .attach([1; 16], Platform::WindowsNative)
            .await
            .unwrap();
        let handoff = coordinator
            .attach([1; 16], Platform::WindowsNative)
            .await
            .unwrap();
        coordinator.detach(Some(first)).await;
        assert!(repository.state.lock().unwrap().observations.is_empty());

        coordinator.detach(Some(handoff)).await;
        let reconnect = coordinator
            .attach([1; 16], Platform::WindowsNative)
            .await
            .unwrap();
        coordinator.begin_shutdown().await;
        coordinator.detach(Some(reconnect)).await;
        coordinator.finish_shutdown().await;

        let state = repository.state.lock().unwrap();
        assert_eq!(state.starts.len(), 1);
        assert_eq!(state.observations.len(), 3);
        assert!(matches!(
            state.observations[0].observation,
            M03SessionObservationV1::Disconnected
        ));
        assert!(matches!(
            state.observations[1].observation,
            M03SessionObservationV1::Reconnected
        ));
        assert!(matches!(
            state.observations[2].observation,
            M03SessionObservationV1::Ended(StoredM03SessionEndReasonV1::ServerShutdown)
        ));
        let session_id = state.starts[0].session_id;
        assert!(
            state
                .observations
                .iter()
                .all(|observation| observation.session_id == session_id)
        );
        assert!(
            std::iter::once(session_id)
                .chain(state.observations.iter().map(|event| event.observation_id))
                .all(is_uuid_v7)
        );
    }

    #[tokio::test]
    async fn client_crash_uses_server_bound_authority_and_exact_replay() {
        let repository = Arc::new(FakeRepository::default());
        let coordinator = coordinator(Arc::clone(&repository));
        let lease = coordinator
            .attach([7; 16], Platform::WindowsNative)
            .await
            .unwrap();
        let report = NativeCrashReportFrameV1 {
            schema_version: protocol::NATIVE_CRASH_SCHEMA_VERSION,
            crash_id: test_uuid_v7(77),
            kind: NativeCrashKindV1::Panic,
            signature: [9; 32],
            uptime_millis: 1_200,
            occurred_at_utc_millis: 1_100,
        };

        assert_eq!(
            coordinator.record_client_crash(Some(lease), &report).await,
            CorePrivateCrashRecordOutcome::Accepted
        );
        assert_eq!(
            coordinator.record_client_crash(Some(lease), &report).await,
            CorePrivateCrashRecordOutcome::Accepted
        );
        {
            let state = repository.state.lock().unwrap();
            assert_eq!(state.crashes.len(), 1);
            let stored = &state.crashes[0];
            assert_eq!(stored.account_id, [7; 16]);
            assert_eq!(stored.session_id, state.starts[0].session_id);
            assert_eq!(stored.character_id, None);
            assert_eq!(stored.source, StoredM03CrashSourceV1::Client);
            assert_eq!(
                stored.reporter,
                StoredM03CrashReporterV1::AuthenticatedClient
            );
            assert_eq!(stored.signature, [9; 32]);
        }

        let mut changed = report.clone();
        changed.signature = [8; 32];
        assert_eq!(
            coordinator.record_client_crash(Some(lease), &changed).await,
            CorePrivateCrashRecordOutcome::IdempotencyConflict
        );
        let replacement = coordinator
            .attach([7; 16], Platform::WindowsNative)
            .await
            .unwrap();
        assert_eq!(
            coordinator.record_client_crash(Some(lease), &report).await,
            CorePrivateCrashRecordOutcome::Unavailable
        );
        assert_eq!(
            coordinator
                .record_client_crash(Some(replacement), &report)
                .await,
            CorePrivateCrashRecordOutcome::Accepted
        );
    }

    #[tokio::test]
    async fn restart_recovers_exact_open_session_context_without_starting_another() {
        let repository = Arc::new(FakeRepository::default());
        let recovered_session = StoredM03TelemetrySessionV1 {
            session_id: test_uuid_v7(90),
            account_id: [2; 16],
            build_id: "older-build".into(),
            content_bundle_version: "older-content".into(),
            platform: StoredM03TelemetryPlatformV1::Linux,
            region_id: "older-region".into(),
            environment: StoredM03TelemetryEnvironmentV1::Staging,
            cohort_tags: vec!["cohort.private".into()],
            started_at_utc_millis: 500,
            ended_at_utc_millis: None,
            end_reason: None,
        };
        repository
            .state
            .lock()
            .unwrap()
            .open
            .insert([2; 16], recovered_session.clone());
        repository.state.lock().unwrap().heads.insert(
            recovered_session.session_id,
            StoredM03SessionEventV1::Disconnected,
        );
        let coordinator = coordinator(Arc::clone(&repository));

        coordinator
            .attach([2; 16], Platform::SteamWindows)
            .await
            .unwrap();

        let state = repository.state.lock().unwrap();
        assert!(state.starts.is_empty());
        assert_eq!(state.open.get(&[2; 16]), Some(&recovered_session));
        assert_eq!(state.observations.len(), 1);
        assert_eq!(
            state.observations[0].session_id,
            recovered_session.session_id
        );
        assert!(matches!(
            state.observations[0].observation,
            M03SessionObservationV1::Reconnected
        ));
    }

    #[tokio::test]
    async fn restart_closes_and_replaces_an_orphaned_connected_session() {
        let repository = Arc::new(FakeRepository::default());
        let old_session_id = test_uuid_v7(91);
        repository.state.lock().unwrap().open.insert(
            [4; 16],
            StoredM03TelemetrySessionV1 {
                session_id: old_session_id,
                account_id: [4; 16],
                build_id: "older-build".into(),
                content_bundle_version: "older-content".into(),
                platform: StoredM03TelemetryPlatformV1::Linux,
                region_id: "older-region".into(),
                environment: StoredM03TelemetryEnvironmentV1::Staging,
                cohort_tags: vec!["cohort.private".into()],
                started_at_utc_millis: 500,
                ended_at_utc_millis: None,
                end_reason: None,
            },
        );
        repository
            .state
            .lock()
            .unwrap()
            .heads
            .insert(old_session_id, StoredM03SessionEventV1::Started);
        let coordinator = coordinator(Arc::clone(&repository));

        let lease = coordinator
            .attach([4; 16], Platform::SteamWindows)
            .await
            .unwrap();
        let report = NativeCrashReportFrameV1 {
            schema_version: protocol::NATIVE_CRASH_SCHEMA_VERSION,
            crash_id: test_uuid_v7(92),
            kind: NativeCrashKindV1::Panic,
            signature: [6; 32],
            uptime_millis: 500,
            occurred_at_utc_millis: 900,
        };
        assert_eq!(
            coordinator.record_client_crash(Some(lease), &report).await,
            CorePrivateCrashRecordOutcome::Accepted
        );

        let state = repository.state.lock().unwrap();
        assert_eq!(state.observations.len(), 1);
        assert!(matches!(
            state.observations[0].observation,
            M03SessionObservationV1::Ended(StoredM03SessionEndReasonV1::TransportClosed)
        ));
        assert_eq!(state.starts.len(), 1);
        assert_ne!(state.starts[0].session_id, old_session_id);
        assert_eq!(
            state.starts[0].platform,
            StoredM03TelemetryPlatformV1::Windows
        );
        assert_eq!(state.starts[0].cohort_tags, ["cohort.private"]);
        assert_eq!(
            state.starts[0].environment,
            StoredM03TelemetryEnvironmentV1::Test
        );
        assert_eq!(state.crashes.len(), 1);
        assert_eq!(state.crashes[0].session_id, old_session_id);
        assert_ne!(state.crashes[0].session_id, state.starts[0].session_id);
    }

    #[tokio::test]
    async fn crash_without_an_authenticated_unambiguous_lease_is_unavailable() {
        let repository = Arc::new(FakeRepository::default());
        let coordinator = coordinator(Arc::clone(&repository));
        let report = NativeCrashReportFrameV1 {
            schema_version: protocol::NATIVE_CRASH_SCHEMA_VERSION,
            crash_id: test_uuid_v7(93),
            kind: NativeCrashKindV1::Unknown,
            signature: [7; 32],
            uptime_millis: 1,
            occurred_at_utc_millis: 2,
        };
        assert_eq!(
            coordinator.record_client_crash(None, &report).await,
            CorePrivateCrashRecordOutcome::Unavailable
        );
        assert!(repository.state.lock().unwrap().crashes.is_empty());
    }

    #[tokio::test]
    async fn clean_exit_ends_current_generation_and_next_attach_starts_fresh() {
        let repository = Arc::new(FakeRepository::default());
        let coordinator = coordinator(Arc::clone(&repository));
        let first = coordinator
            .attach([5; 16], Platform::WindowsNative)
            .await
            .unwrap();
        coordinator
            .end(Some(first), StoredM03SessionEndReasonV1::CleanExit)
            .await;
        coordinator
            .attach([5; 16], Platform::WindowsNative)
            .await
            .unwrap();

        let state = repository.state.lock().unwrap();
        assert_eq!(state.starts.len(), 2);
        assert_ne!(state.starts[0].session_id, state.starts[1].session_id);
        assert_eq!(state.observations.len(), 1);
        assert!(matches!(
            state.observations[0].observation,
            M03SessionObservationV1::Ended(StoredM03SessionEndReasonV1::CleanExit)
        ));
    }

    #[tokio::test]
    async fn failed_reconnect_observation_never_claims_a_connected_telemetry_lease() {
        let repository = Arc::new(FakeRepository::default());
        let coordinator = coordinator(Arc::clone(&repository));
        let first = coordinator
            .attach([6; 16], Platform::WindowsNative)
            .await
            .unwrap();
        coordinator.detach(Some(first)).await;
        let session_id = repository.state.lock().unwrap().starts[0].session_id;
        repository.state.lock().unwrap().heads.insert(
            session_id,
            StoredM03SessionEventV1::Ended {
                duration_millis: 100,
                reason: StoredM03SessionEndReasonV1::TransportClosed,
            },
        );

        assert!(
            coordinator
                .attach([6; 16], Platform::WindowsNative)
                .await
                .is_none()
        );
        assert!(!coordinator.state.lock().await.sessions[&[6; 16]].connected);
    }

    #[tokio::test]
    async fn repository_failure_returns_no_lease_and_never_panics_or_retries_gameplay() {
        let repository = Arc::new(FakeRepository::default());
        repository.state.lock().unwrap().fail_load = true;
        let coordinator = coordinator(Arc::clone(&repository));

        assert!(
            coordinator
                .attach([3; 16], Platform::WindowsNative)
                .await
                .is_none()
        );
        coordinator.detach(None).await;
        coordinator.begin_shutdown().await;
        coordinator.finish_shutdown().await;

        let state = repository.state.lock().unwrap();
        assert!(state.starts.is_empty());
        assert!(state.observations.is_empty());
    }

    #[test]
    fn region_context_accepts_canonical_ids_and_rejects_secrets_or_unstable_text() {
        assert!(valid_stable_context_id("us-west-2.private"));
        assert!(valid_stable_context_id(LOCAL_REGION_ID));
        assert!(!valid_stable_context_id(""));
        assert!(!valid_stable_context_id("US-West-2"));
        assert!(!valid_stable_context_id("secret-region"));
        assert!(!valid_stable_context_id("west/key=value"));
    }

    fn test_uuid_v7(seed: u8) -> [u8; 16] {
        let mut value = [seed; 16];
        value[6] = (value[6] & 0x0f) | 0x70;
        value[8] = (value[8] & 0x3f) | 0x80;
        value
    }

    fn is_uuid_v7(value: [u8; 16]) -> bool {
        value[6] >> 4 == 7 && value[8] >> 6 == 2
    }
}
