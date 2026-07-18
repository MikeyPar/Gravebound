//! Per-account transport ownership for the ordinary Core private-life route.
//!
//! The canonical GDD requires one server-authoritative reliable sequence and generation-safe
//! reconnect behavior (`TECH-015`, `TECH-021`-`023`). The Content Production Specification fixes
//! the closed Hall -> microrealm -> Bell Sepulcher -> Caldus route, and the Development Roadmap
//! requires the M03 loop to survive response loss and reconnect without duplicate authority.
//! A session therefore exists from handshake onward, before a danger actor or Recall channel is
//! available, and later binds those dynamic owners to the same reliable writer.

use std::{collections::BTreeMap, future::Future, path::Path, pin::Pin, sync::Arc};

use persistence::PostgresPersistence;
use protocol::{ActionFrame, ActionKind, CorePrivateRouteContentRevisionV1, InputFrame};
use sim_core::{AimDirection, MovementAction, SimulationVector};
use thiserror::Error;
use tokio::sync::Mutex;

use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CoreB3RewardAuthority,
    CoreB3RewardCompositionError, CoreB3RewardWriterGeneration, CoreBellPortalTransition,
    CoreDurableB3Resolution, CoreDurableBargainRestResolution, CoreExtractionActorDirectory,
    CoreExtractionAuthoritativeTick, CoreExtractionConnectionLease, CoreExtractionHallProjection,
    CoreExtractionRuntimeError, CoreExtractionRuntimeReport, CoreExtractionTransportAttach,
    CoreExtractionTransportDetach, CorePrivateB3RewardRuntime, CorePrivateB3RewardRuntimeError,
    CorePrivateFixedDungeonAdvance, CorePrivateFixedDungeonB3RewardCommit,
    CorePrivateFixedDungeonDriverReady, CorePrivateFixedDungeonRestCommit,
    CorePrivateMicrorealmAbility, CorePrivateMicrorealmAbilityPress, CorePrivateMicrorealmDriver,
    CorePrivateMicrorealmDriverError, CorePrivateMicrorealmDriverHandle,
    CorePrivateMicrorealmDriverObserver, CorePrivateMicrorealmDriverReport,
    CorePrivateMicrorealmIngressError, CorePrivateMicrorealmPreparedHandoff,
    CorePrivateMicrorealmRetainedInput, CorePrivateMicrorealmRuntime, CorePrivateRouteActorLease,
    CoreRecallActorDirectory, CoreRecallActorRetirementReport, CoreRecallAuthoritativeTick,
    CoreRecallConnectionAuthority, CoreRecallConnectionLease, CoreRecallRuntimeError,
    CoreRecallRuntimeReport, CoreRecallTransportAttach, CoreReliableWriter, IdentityClock,
    PostgresCoreB3RewardCoordinator, ProductionExtractionPlanner, ProductionRecallClock,
    ProductionRecallDetachOutcome, SecretRewardEpoch, TRANSPORT_REPLACED_CLOSE_CODE,
};
use crate::{
    core_extraction_runtime::CoreExtractionPreparedWriterHandoff,
    core_recall_runtime::CoreRecallPreparedWriterHandoff,
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

/// Transport-independent authority for retiring exactly one live microrealm allocation. A
/// terminal owner may retain this across `LinkLost`, when no transport lease remains valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateMicrorealmBindingLease {
    account_id: [u8; 16],
    character_id: [u8; 16],
    actor_generation: u64,
    binding_generation: u64,
    route_lease: CorePrivateRouteActorLease,
}

impl CorePrivateMicrorealmBindingLease {
    #[must_use]
    pub const fn account_id(self) -> [u8; 16] {
        self.account_id
    }

    #[must_use]
    pub const fn character_id(self) -> [u8; 16] {
        self.character_id
    }

    #[must_use]
    pub const fn actor_generation(self) -> u64 {
        self.actor_generation
    }

    #[must_use]
    pub const fn binding_generation(self) -> u64 {
        self.binding_generation
    }

    #[must_use]
    pub const fn route_lease(self) -> CorePrivateRouteActorLease {
        self.route_lease
    }
}

/// Reconnect-visible binding exposes immutable retirement identity and read-only committed state.
/// All ingress must pass back through the generation-validating session directory.
#[derive(Debug, Clone)]
pub struct CorePrivateMicrorealmBinding {
    pub lease: CorePrivateMicrorealmBindingLease,
    pub observer: CorePrivateMicrorealmDriverObserver,
}

/// Transport-independent pause token for the one live danger task. The caller resolves the Bell
/// mutation durably while this token freezes the exact frame boundary, then either aborts or
/// installs the committed B0 runtime inside that same task.
#[derive(Debug)]
pub struct CorePrivateLifePreparedBellHandoff {
    pub binding_lease: CorePrivateMicrorealmBindingLease,
    prepared: CorePrivateMicrorealmPreparedHandoff,
}

impl CorePrivateLifePreparedBellHandoff {
    #[must_use]
    pub const fn ready(&self) -> crate::CorePrivateMicrorealmHandoffReady {
        self.prepared.ready()
    }

    pub async fn abort(self) -> Result<(), CorePrivateLifeSessionError> {
        self.prepared.abort().await?;
        Ok(())
    }

    pub async fn commit_into_fixed_dungeon(
        self,
        transition: CoreBellPortalTransition,
        expected_content_revision: CorePrivateRouteContentRevisionV1,
        encounters: sim_content::CoreDevelopmentEncounterRooms,
    ) -> Result<CorePrivateFixedDungeonDriverReady, CorePrivateLifeSessionError> {
        self.prepared
            .commit_into_fixed_dungeon(transition, expected_content_revision, encounters)?
            .wait()
            .await
            .map_err(Into::into)
    }
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
    pub extraction_lease: Option<CoreExtractionConnectionLease>,
    pub microrealm: Option<CorePrivateMicrorealmBinding>,
    pub invalidated_connection: Option<quinn::Connection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePrivateLifeTransportDetach {
    Detached {
        recall: Option<ProductionRecallDetachOutcome>,
        extraction: Option<CoreExtractionTransportDetach>,
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
    pub extraction_bound_count: usize,
    pub microrealm_bound_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateLifeSessionReport {
    pub retired_account_count: usize,
    pub remaining_active_transports: usize,
    pub recall: CoreRecallRuntimeReport,
    pub extraction: Option<CoreExtractionRuntimeReport>,
    pub remaining_microrealm_bindings: usize,
    pub microrealm_shutdown_failures: usize,
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
    #[error("Core private-life extraction runtime is unavailable")]
    ExtractionUnavailable,
    #[error("Core private-life extraction authority is already bound")]
    ExtractionAlreadyBound,
    #[error("Core private-life extraction authority is not bound")]
    ExtractionNotBound,
    #[error("Core private-life microrealm authority is already bound")]
    MicrorealmAlreadyBound,
    #[error("Core private-life microrealm authority is not bound")]
    MicrorealmUnavailable,
    #[error("Core private-life microrealm binding generation overflowed")]
    MicrorealmBindingGenerationExhausted,
    #[error("Core private-life microrealm input is invalid")]
    InvalidMicrorealmInput,
    #[error("Core private-life action is not a microrealm ability press")]
    MicrorealmActionUnavailable,
    #[error("Core private-life dynamic writer handoff could not restore one-owner authority")]
    DynamicWriterHandoffInconsistent,
    #[error("Core private-life session shutdown has not started")]
    ShutdownNotStarted,
    #[error("Core private-life Recall runtime failed")]
    Recall(#[from] CoreRecallRuntimeError),
    #[error("Core private-life extraction runtime failed: {0}")]
    Extraction(#[from] CoreExtractionRuntimeError),
    #[error("Core private-life microrealm ingress failed: {0}")]
    MicrorealmIngress(#[from] CorePrivateMicrorealmIngressError),
    #[error("Core private-life microrealm driver failed: {0}")]
    MicrorealmDriver(#[from] CorePrivateMicrorealmDriverError),
    #[error("Core private-life automatic B3 reward runtime failed: {0}")]
    B3RewardRuntime(#[from] CorePrivateB3RewardRuntimeError),
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
    extraction_bound: bool,
    extraction_lease: Option<CoreExtractionConnectionLease>,
    microrealm: Option<BoundMicrorealmDriver>,
    next_microrealm_binding_generation: u64,
}

#[derive(Debug)]
struct BoundMicrorealmDriver {
    lease: CorePrivateMicrorealmBindingLease,
    driver: CorePrivateMicrorealmDriver,
    b3_rewards: Option<CorePrivateB3RewardRuntime>,
}

#[derive(Debug)]
struct SessionState {
    accepting: bool,
    shutdown_started: bool,
    microrealm_shutdown_failures: usize,
    sessions: BTreeMap<[u8; 16], SessionEntry>,
}

#[derive(Debug, Clone, Copy)]
struct PreparedDynamicWriterHandoffs {
    recall: Option<CoreRecallPreparedWriterHandoff>,
    extraction: Option<CoreExtractionPreparedWriterHandoff>,
}

#[derive(Debug)]
struct CommittedDynamicWriterHandoffs {
    recall: Option<CoreRecallTransportAttach>,
    extraction: Option<CoreExtractionTransportAttach>,
}

/// Owns exactly one current transport generation and writer for each authenticated account.
/// Recall may bind after danger entry; later transport handoffs automatically rebind it before
/// the new session generation becomes visible.
pub struct CorePrivateLifeSessionDirectory<Clock, TickSource> {
    recall: Arc<CoreRecallActorDirectory<Clock, TickSource>>,
    extraction: Option<Box<dyn PrivateLifeExtractionRuntime>>,
    b3_rewards: Option<Arc<dyn CoreB3RewardAuthority>>,
    state: Mutex<SessionState>,
}

type RuntimeFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

trait PrivateLifeExtractionRuntime: Send + Sync {
    fn prepare(
        &self,
        authenticated: AuthenticatedAccount,
        writer: Arc<CoreReliableWriter>,
    ) -> RuntimeFuture<'_, Result<CoreExtractionPreparedWriterHandoff, CoreExtractionRuntimeError>>;
    fn commit(
        &self,
        prepared: CoreExtractionPreparedWriterHandoff,
    ) -> RuntimeFuture<'_, Result<CoreExtractionTransportAttach, CoreExtractionRuntimeError>>;
    fn abort(
        &self,
        prepared: CoreExtractionPreparedWriterHandoff,
    ) -> RuntimeFuture<'_, Result<(), CoreExtractionRuntimeError>>;
    fn detach(
        &self,
        lease: CoreExtractionConnectionLease,
    ) -> RuntimeFuture<'_, CoreExtractionTransportDetach>;
    fn acknowledge_hall_installed(
        &self,
        projection: CoreExtractionHallProjection,
    ) -> RuntimeFuture<'_, Result<(), CoreExtractionRuntimeError>>;
    fn begin_shutdown(&self) -> RuntimeFuture<'_, Vec<quinn::Connection>>;
    fn finish_shutdown(
        &self,
    ) -> RuntimeFuture<'_, Result<CoreExtractionRuntimeReport, CoreExtractionRuntimeError>>;
}

impl<Planner, ExtractionClock, ExtractionTicks> PrivateLifeExtractionRuntime
    for Arc<CoreExtractionActorDirectory<Planner, ExtractionClock, ExtractionTicks>>
where
    Planner: ProductionExtractionPlanner + 'static,
    ExtractionClock: IdentityClock + 'static,
    ExtractionTicks: CoreExtractionAuthoritativeTick + 'static,
{
    fn prepare(
        &self,
        authenticated: AuthenticatedAccount,
        writer: Arc<CoreReliableWriter>,
    ) -> RuntimeFuture<'_, Result<CoreExtractionPreparedWriterHandoff, CoreExtractionRuntimeError>>
    {
        Box::pin(async move {
            self.prepare_reliable_writer_handoff(authenticated, writer)
                .await
        })
    }

    fn commit(
        &self,
        prepared: CoreExtractionPreparedWriterHandoff,
    ) -> RuntimeFuture<'_, Result<CoreExtractionTransportAttach, CoreExtractionRuntimeError>> {
        Box::pin(async move { self.commit_prepared_reliable_writer_handoff(prepared).await })
    }

    fn abort(
        &self,
        prepared: CoreExtractionPreparedWriterHandoff,
    ) -> RuntimeFuture<'_, Result<(), CoreExtractionRuntimeError>> {
        Box::pin(async move { self.abort_prepared_reliable_writer_handoff(prepared).await })
    }

    fn detach(
        &self,
        lease: CoreExtractionConnectionLease,
    ) -> RuntimeFuture<'_, CoreExtractionTransportDetach> {
        Box::pin(async move { self.detach_shared_reliable_writer(lease).await })
    }

    fn acknowledge_hall_installed(
        &self,
        projection: CoreExtractionHallProjection,
    ) -> RuntimeFuture<'_, Result<(), CoreExtractionRuntimeError>> {
        Box::pin(async move {
            CoreExtractionActorDirectory::acknowledge_hall_installed(self, projection).await
        })
    }

    fn begin_shutdown(&self) -> RuntimeFuture<'_, Vec<quinn::Connection>> {
        Box::pin(async move { CoreExtractionActorDirectory::begin_shutdown(self).await })
    }

    fn finish_shutdown(
        &self,
    ) -> RuntimeFuture<'_, Result<CoreExtractionRuntimeReport, CoreExtractionRuntimeError>> {
        Box::pin(async move { CoreExtractionActorDirectory::finish_shutdown(self).await })
    }
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
            extraction: None,
            b3_rewards: None,
            state: Mutex::new(SessionState {
                accepting: true,
                shutdown_started: false,
                microrealm_shutdown_failures: 0,
                sessions: BTreeMap::new(),
            }),
        }
    }

    #[must_use]
    pub fn with_extraction_runtime<Planner, ExtractionClock, ExtractionTicks>(
        recall: Arc<CoreRecallActorDirectory<Clock, TickSource>>,
        extraction: Arc<CoreExtractionActorDirectory<Planner, ExtractionClock, ExtractionTicks>>,
    ) -> Self
    where
        Planner: ProductionExtractionPlanner + 'static,
        ExtractionClock: IdentityClock + 'static,
        ExtractionTicks: CoreExtractionAuthoritativeTick + 'static,
    {
        let mut sessions = Self::new(recall);
        sessions.extraction = Some(Box::new(extraction));
        sessions
    }

    #[must_use]
    pub fn with_b3_reward_authority(mut self, authority: Arc<dyn CoreB3RewardAuthority>) -> Self {
        self.b3_rewards = Some(authority);
        self
    }

    /// Installs the one production `PostgreSQL` B3 authority at private-life session construction.
    /// The caller owns reward-epoch loading so one redacted epoch can be shared for the complete
    /// server process rather than re-read or rotated per account, connection, or retry.
    pub fn with_persistent_b3_reward_authority(
        self,
        persistence: PostgresPersistence,
        content_root: &Path,
        epoch: SecretRewardEpoch,
    ) -> Result<Self, CoreB3RewardCompositionError> {
        let authority = Arc::new(PostgresCoreB3RewardCoordinator::load(
            persistence,
            content_root,
            epoch,
        )?);
        Ok(self.with_b3_reward_authority(authority))
    }

    async fn prepare_dynamic_writer_handoffs(
        &self,
        entry: &SessionEntry,
        authenticated: AuthenticatedAccount,
        writer: &Arc<CoreReliableWriter>,
    ) -> Result<PreparedDynamicWriterHandoffs, CorePrivateLifeSessionError> {
        let recall = if entry.recall_bound {
            match self
                .recall
                .prepare_reliable_writer_handoff(authenticated, Arc::clone(writer))
                .await
            {
                Ok(prepared) => Some(prepared),
                Err(error) => {
                    writer.retire(
                        crate::CORE_RELIABLE_WRITE_UNCERTAIN_CLOSE_CODE,
                        b"private-life writer handoff preparation failed",
                    );
                    return Err(error.into());
                }
            }
        } else {
            None
        };
        let extraction = if entry.extraction_bound {
            let Some(extraction) = self.extraction.as_ref() else {
                if let Some(prepared) = recall {
                    let _ = self
                        .recall
                        .abort_prepared_reliable_writer_handoff(prepared)
                        .await;
                }
                writer.retire(
                    crate::CORE_RELIABLE_WRITE_UNCERTAIN_CLOSE_CODE,
                    b"private-life extraction runtime unavailable",
                );
                return Err(CorePrivateLifeSessionError::ExtractionUnavailable);
            };
            match extraction.prepare(authenticated, Arc::clone(writer)).await {
                Ok(prepared) => Some(prepared),
                Err(error) => {
                    if let Some(prepared) = recall {
                        let _ = self
                            .recall
                            .abort_prepared_reliable_writer_handoff(prepared)
                            .await;
                    }
                    writer.retire(
                        crate::CORE_RELIABLE_WRITE_UNCERTAIN_CLOSE_CODE,
                        b"private-life extraction handoff preparation failed",
                    );
                    return Err(error.into());
                }
            }
        } else {
            None
        };
        Ok(PreparedDynamicWriterHandoffs { recall, extraction })
    }

    async fn restore_recall_after_extraction_failure(
        &self,
        entry: &mut SessionEntry,
        authenticated: AuthenticatedAccount,
        writer: &Arc<CoreReliableWriter>,
        issued_at_unix_ms: u64,
        recall_attach: &CoreRecallTransportAttach,
    ) -> Result<(), CorePrivateLifeSessionError> {
        let had_previous_transport = entry.active.is_some();
        let restored = if let Some(previous) = &entry.active {
            match self
                .recall
                .prepare_reliable_writer_handoff(authenticated, Arc::clone(&previous.writer))
                .await
            {
                Ok(prepared) => self
                    .recall
                    .commit_prepared_reliable_writer_handoff(prepared)
                    .await
                    .ok(),
                Err(_) => None,
            }
        } else {
            None
        };
        writer.retire(
            crate::CORE_RELIABLE_WRITE_UNCERTAIN_CLOSE_CODE,
            b"private-life dynamic writer handoff failed",
        );
        if let Some(restored) = restored {
            entry.recall_lease = Some(restored.lease);
            return Ok(());
        }
        let _ = self
            .recall
            .detach_transport(recall_attach.lease, issued_at_unix_ms)
            .await;
        entry.recall_lease = None;
        if let Some(active) = entry.active.take() {
            active.writer.retire(
                crate::CORE_RELIABLE_WRITE_UNCERTAIN_CLOSE_CODE,
                b"private-life writer handoff restore failed",
            );
        }
        if had_previous_transport {
            Err(CorePrivateLifeSessionError::DynamicWriterHandoffInconsistent)
        } else {
            Ok(())
        }
    }

    async fn commit_dynamic_writer_handoffs(
        &self,
        entry: &mut SessionEntry,
        authenticated: AuthenticatedAccount,
        writer: &Arc<CoreReliableWriter>,
        issued_at_unix_ms: u64,
        prepared: PreparedDynamicWriterHandoffs,
    ) -> Result<CommittedDynamicWriterHandoffs, CorePrivateLifeSessionError> {
        let recall = if let Some(prepared_recall) = prepared.recall {
            match self
                .recall
                .commit_prepared_reliable_writer_handoff(prepared_recall)
                .await
            {
                Ok(attached) => Some(attached),
                Err(error) => {
                    let _ = self
                        .recall
                        .abort_prepared_reliable_writer_handoff(prepared_recall)
                        .await;
                    if let (Some(extraction), Some(prepared_extraction)) =
                        (&self.extraction, prepared.extraction)
                    {
                        let _ = extraction.abort(prepared_extraction).await;
                    }
                    writer.retire(
                        crate::CORE_RELIABLE_WRITE_UNCERTAIN_CLOSE_CODE,
                        b"private-life Recall handoff commit failed",
                    );
                    return Err(error.into());
                }
            }
        } else {
            None
        };
        let extraction = if let Some(prepared_extraction) = prepared.extraction {
            let Some(runtime) = self.extraction.as_ref() else {
                if let Some(recall_attach) = &recall {
                    self.restore_recall_after_extraction_failure(
                        entry,
                        authenticated,
                        writer,
                        issued_at_unix_ms,
                        recall_attach,
                    )
                    .await?;
                } else {
                    writer.retire(
                        crate::CORE_RELIABLE_WRITE_UNCERTAIN_CLOSE_CODE,
                        b"private-life extraction runtime unavailable",
                    );
                }
                return Err(CorePrivateLifeSessionError::ExtractionUnavailable);
            };
            match runtime.commit(prepared_extraction).await {
                Ok(attached) => Some(attached),
                Err(error) => {
                    let _ = runtime.abort(prepared_extraction).await;
                    if let Some(recall_attach) = &recall {
                        self.restore_recall_after_extraction_failure(
                            entry,
                            authenticated,
                            writer,
                            issued_at_unix_ms,
                            recall_attach,
                        )
                        .await?;
                    } else {
                        writer.retire(
                            crate::CORE_RELIABLE_WRITE_UNCERTAIN_CLOSE_CODE,
                            b"private-life extraction handoff commit failed",
                        );
                    }
                    return Err(error.into());
                }
            }
        } else {
            None
        };
        Ok(CommittedDynamicWriterHandoffs { recall, extraction })
    }

    /// Accepts a transport after authentication. No route or danger owner is required yet.
    /// When Recall is already bound, its writer handoff commits first so no new session can be
    /// advertised with a split reliable sequence.
    pub async fn attach_transport(
        &self,
        authenticated: AuthenticatedAccount,
        connection: quinn::Connection,
        issued_at_unix_ms: u64,
    ) -> Result<CorePrivateLifeTransportAttach, CorePrivateLifeSessionError> {
        if authenticated.namespace != AuthenticatedNamespace::WipeableTest {
            return Err(CorePrivateLifeSessionError::InvalidAccountBinding);
        }
        let account_id = authenticated.account_id.as_bytes();
        let writer = Arc::new(CoreReliableWriter::new(connection));
        let mut state = self.state.lock().await;
        if !state.accepting {
            writer.retire(
                crate::SERVER_SHUTDOWN_CLOSE_CODE,
                b"private-life session admission retired",
            );
            return Err(CorePrivateLifeSessionError::Retired);
        }
        let entry = state.sessions.entry(account_id).or_insert(SessionEntry {
            authenticated,
            next_generation: 1,
            active: None,
            recall_bound: false,
            recall_lease: None,
            extraction_bound: false,
            extraction_lease: None,
            microrealm: None,
            next_microrealm_binding_generation: 1,
        });
        if entry.authenticated != authenticated {
            writer.retire(
                crate::CORE_RELIABLE_WRITE_UNCERTAIN_CLOSE_CODE,
                b"private-life account binding mismatch",
            );
            return Err(CorePrivateLifeSessionError::InvalidAccountBinding);
        }
        let generation = CorePrivateLifeTransportGeneration(entry.next_generation);
        let Some(next_generation) = entry.next_generation.checked_add(1) else {
            writer.retire(
                crate::CORE_RELIABLE_WRITE_UNCERTAIN_CLOSE_CODE,
                b"private-life session generation exhausted",
            );
            return Err(CorePrivateLifeSessionError::GenerationExhausted);
        };

        let prepared = self
            .prepare_dynamic_writer_handoffs(entry, authenticated, &writer)
            .await?;
        let committed = self
            .commit_dynamic_writer_handoffs(
                entry,
                authenticated,
                &writer,
                issued_at_unix_ms,
                prepared,
            )
            .await?;
        let lease = CorePrivateLifeTransportLease {
            account_id,
            generation,
        };
        let previous = entry.active.replace(ActiveTransport {
            lease,
            writer: Arc::clone(&writer),
        });
        entry.next_generation = next_generation;
        entry.recall_lease = committed.recall.as_ref().map(|attached| attached.lease);
        entry.extraction_lease = committed.extraction.as_ref().map(|attached| attached.lease);
        if let Some(bound) = &entry.microrealm {
            bound.driver.handle().mark_reward_session_active();
            if let Some(b3_rewards) = &bound.b3_rewards {
                b3_rewards.attach_writer(
                    CoreB3RewardWriterGeneration::new(generation.get())?,
                    Arc::clone(&writer),
                );
            }
        }

        let invalidated_connection = previous.map(|active| {
            active.writer.retire(
                TRANSPORT_REPLACED_CLOSE_CODE,
                b"authoritative private-life transport handoff",
            );
            active.writer.connection().clone()
        });
        Ok(CorePrivateLifeTransportAttach {
            lease,
            writer,
            recall_lease: entry.recall_lease,
            extraction_lease: entry.extraction_lease,
            microrealm: entry
                .microrealm
                .as_ref()
                .map(|bound| CorePrivateMicrorealmBinding {
                    lease: bound.lease,
                    observer: bound.driver.handle().observe(),
                }),
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
        let prepared = self
            .recall
            .prepare_reliable_writer_handoff(entry.authenticated, Arc::clone(&active.writer))
            .await?;
        let attached = match self
            .recall
            .commit_prepared_reliable_writer_handoff(prepared)
            .await
        {
            Ok(attached) => attached,
            Err(error) => {
                let _ = self
                    .recall
                    .abort_prepared_reliable_writer_handoff(prepared)
                    .await;
                return Err(error.into());
            }
        };
        entry.recall_bound = true;
        entry.recall_lease = Some(attached.lease);
        Ok(attached.lease)
    }

    /// Binds one generation-pinned live microrealm owner to the retained account session. The
    /// owner is transport-independent: disconnect keeps it alive for `LinkLost` vulnerability, and
    /// reconnect returns the same allocation to the winning transport generation.
    pub async fn bind_microrealm(
        &self,
        lease: CorePrivateLifeTransportLease,
        runtime: CorePrivateMicrorealmRuntime,
    ) -> Result<CorePrivateMicrorealmBinding, CorePrivateLifeSessionError> {
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
        if runtime.account_id() != lease.account_id
            || entry.authenticated.account_id.as_bytes() != runtime.account_id()
        {
            return Err(CorePrivateLifeSessionError::InvalidAccountBinding);
        }
        if entry.microrealm.is_some() {
            return Err(CorePrivateLifeSessionError::MicrorealmAlreadyBound);
        }
        let binding_generation = entry.next_microrealm_binding_generation;
        let Some(next_binding_generation) = binding_generation.checked_add(1) else {
            return Err(CorePrivateLifeSessionError::MicrorealmBindingGenerationExhausted);
        };
        let route_lease = runtime.route_lease();
        let binding_lease = CorePrivateMicrorealmBindingLease {
            account_id: lease.account_id,
            character_id: route_lease.character_id(),
            actor_generation: route_lease.actor_generation(),
            binding_generation,
            route_lease,
        };
        let driver = CorePrivateMicrorealmDriver::spawn(runtime);
        let handle = driver.handle();
        let active = entry
            .active
            .as_ref()
            .ok_or(CorePrivateLifeSessionError::SessionUnavailable)?;
        let b3_rewards = self.b3_rewards.as_ref().map(|authority| {
            CorePrivateB3RewardRuntime::spawn(
                entry.authenticated,
                binding_lease.character_id(),
                Arc::clone(authority),
                handle.clone(),
                handle.observe(),
                Some((
                    CoreB3RewardWriterGeneration::new(active.lease.generation.get())
                        .expect("session transport generations are nonzero"),
                    Arc::clone(&active.writer),
                )),
            )
        });
        let binding = CorePrivateMicrorealmBinding {
            lease: binding_lease,
            observer: handle.observe(),
        };
        entry.next_microrealm_binding_generation = next_binding_generation;
        entry.microrealm = Some(BoundMicrorealmDriver {
            lease: binding_lease,
            driver,
            b3_rewards,
        });
        Ok(binding)
    }

    /// Returns live danger authority only to the current transport generation. The returned owner
    /// remains valid across a later detach so the server tick loop can keep the character live.
    pub async fn microrealm_authority(
        &self,
        lease: CorePrivateLifeTransportLease,
    ) -> Result<CorePrivateMicrorealmBinding, CorePrivateLifeSessionError> {
        let state = self.state.lock().await;
        let entry = state
            .sessions
            .get(&lease.account_id)
            .ok_or(CorePrivateLifeSessionError::SessionUnavailable)?;
        if entry.active.as_ref().map(|active| active.lease) != Some(lease) {
            return Err(CorePrivateLifeSessionError::StaleTransport);
        }
        entry
            .microrealm
            .as_ref()
            .map(|bound| CorePrivateMicrorealmBinding {
                lease: bound.lease,
                observer: bound.driver.handle().observe(),
            })
            .ok_or(CorePrivateLifeSessionError::MicrorealmUnavailable)
    }

    /// Freezes the exact session-owned danger task between frames before any Bell reservation or
    /// durable mutation begins. The pause is transport-independent: reconnect observes the same
    /// binding and task. Known rejection must explicitly abort; dropping the acknowledged token
    /// is an unknown durable outcome and remains frozen for restart/receipt reconciliation.
    pub async fn prepare_bell_handoff(
        &self,
        lease: CorePrivateLifeTransportLease,
    ) -> Result<CorePrivateLifePreparedBellHandoff, CorePrivateLifeSessionError> {
        let (binding_lease, handle) = {
            let state = self.state.lock().await;
            let entry = state
                .sessions
                .get(&lease.account_id)
                .ok_or(CorePrivateLifeSessionError::SessionUnavailable)?;
            if entry.active.as_ref().map(|active| active.lease) != Some(lease) {
                return Err(CorePrivateLifeSessionError::StaleTransport);
            }
            let bound = entry
                .microrealm
                .as_ref()
                .ok_or(CorePrivateLifeSessionError::MicrorealmUnavailable)?;
            (bound.lease, bound.driver.handle())
        };
        let prepared = handle.prepare_handoff().await?;
        Ok(CorePrivateLifePreparedBellHandoff {
            binding_lease,
            prepared,
        })
    }

    /// Requests the next server-selected fixed-dungeon transition through the current transport
    /// generation. No destination or phase crosses this boundary: the session validates ownership,
    /// and the existing danger task decides whether its canonical node is ready to advance.
    pub async fn advance_fixed_dungeon(
        &self,
        lease: CorePrivateLifeTransportLease,
    ) -> Result<CorePrivateFixedDungeonAdvance, CorePrivateLifeSessionError> {
        let handle = {
            let state = self.state.lock().await;
            let entry = state
                .sessions
                .get(&lease.account_id)
                .ok_or(CorePrivateLifeSessionError::SessionUnavailable)?;
            if entry.active.as_ref().map(|active| active.lease) != Some(lease) {
                return Err(CorePrivateLifeSessionError::StaleTransport);
            }
            entry
                .microrealm
                .as_ref()
                .map(|bound| bound.driver.handle())
                .ok_or(CorePrivateLifeSessionError::MicrorealmUnavailable)?
        };
        handle.advance_fixed_dungeon().await.map_err(Into::into)
    }

    /// Applies an opaque persistence-produced B4 outcome through the current transport
    /// generation. Account binding is checked before the value reaches the sole runtime owner;
    /// character and dangerous-instance lineage are checked again inside that owner.
    pub async fn resolve_fixed_dungeon_rest(
        &self,
        lease: CorePrivateLifeTransportLease,
        durable: CoreDurableBargainRestResolution,
    ) -> Result<CorePrivateFixedDungeonRestCommit, CorePrivateLifeSessionError> {
        if durable.account_id() != lease.account_id {
            return Err(CorePrivateLifeSessionError::InvalidAccountBinding);
        }
        let handle = {
            let state = self.state.lock().await;
            let entry = state
                .sessions
                .get(&lease.account_id)
                .ok_or(CorePrivateLifeSessionError::SessionUnavailable)?;
            if entry.active.as_ref().map(|active| active.lease) != Some(lease) {
                return Err(CorePrivateLifeSessionError::StaleTransport);
            }
            entry
                .microrealm
                .as_ref()
                .map(|bound| bound.driver.handle())
                .ok_or(CorePrivateLifeSessionError::MicrorealmUnavailable)?
        };
        handle
            .resolve_fixed_dungeon_rest(durable)
            .await
            .map_err(Into::into)
    }

    /// Applies a server-coordinator-produced B3 reward proof through the current transport
    /// generation. Account binding is checked before the proof reaches the sole task; character,
    /// lineage, tick, and exact handoff are checked again inside the runtime owner.
    pub async fn commit_fixed_dungeon_b3_reward(
        &self,
        lease: CorePrivateLifeTransportLease,
        durable: CoreDurableB3Resolution,
    ) -> Result<CorePrivateFixedDungeonB3RewardCommit, CorePrivateLifeSessionError> {
        if durable.account_id() != lease.account_id {
            return Err(CorePrivateLifeSessionError::InvalidAccountBinding);
        }
        let handle = {
            let state = self.state.lock().await;
            let entry = state
                .sessions
                .get(&lease.account_id)
                .ok_or(CorePrivateLifeSessionError::SessionUnavailable)?;
            if entry.active.as_ref().map(|active| active.lease) != Some(lease) {
                return Err(CorePrivateLifeSessionError::StaleTransport);
            }
            entry
                .microrealm
                .as_ref()
                .map(|bound| bound.driver.handle())
                .ok_or(CorePrivateLifeSessionError::MicrorealmUnavailable)?
        };
        handle
            .commit_fixed_dungeon_b3_reward(durable)
            .await
            .map_err(Into::into)
    }

    /// Validates compact input against the negotiated protocol, checks the current transport
    /// generation under the session lock, then submits without retaining that lock.
    pub async fn submit_microrealm_input(
        &self,
        lease: CorePrivateLifeTransportLease,
        frame: &InputFrame,
    ) -> Result<(), CorePrivateLifeSessionError> {
        frame
            .validate()
            .map_err(|_| CorePrivateLifeSessionError::InvalidMicrorealmInput)?;
        let movement =
            MovementAction::try_from_milli(frame.movement_x_milli, frame.movement_y_milli)
                .map_err(|_| CorePrivateLifeSessionError::InvalidMicrorealmInput)?;
        let aim = AimDirection::new(SimulationVector::new(
            f32::from(frame.aim_x_milli),
            f32::from(frame.aim_y_milli),
        ))
        .map_err(|_| CorePrivateLifeSessionError::InvalidMicrorealmInput)?;
        self.submit_microrealm_ingress(lease, |handle| {
            handle.submit_latest_input(CorePrivateMicrorealmRetainedInput {
                input_sequence: u64::from(frame.sequence),
                movement,
                aim,
                primary_held: frame.held_primary,
                primary_sequence: frame.primary_sequence,
            })
        })
        .await
    }

    /// Submits only reliable ability presses. Recall and interactions retain their dedicated
    /// owners and cannot be smuggled into the combat driver.
    pub async fn submit_microrealm_action(
        &self,
        lease: CorePrivateLifeTransportLease,
        frame: &ActionFrame,
    ) -> Result<(), CorePrivateLifeSessionError> {
        frame
            .validate()
            .map_err(|_| CorePrivateLifeSessionError::InvalidMicrorealmInput)?;
        let ability = match frame.action {
            ActionKind::Ability1Press => CorePrivateMicrorealmAbility::Ability1,
            ActionKind::Ability2Press => CorePrivateMicrorealmAbility::Ability2,
            ActionKind::RecallStart | ActionKind::RecallCancel | ActionKind::Interact => {
                return Err(CorePrivateLifeSessionError::MicrorealmActionUnavailable);
            }
        };
        self.submit_microrealm_ingress(lease, |handle| {
            handle.submit_ability_press(CorePrivateMicrorealmAbilityPress {
                action_sequence: frame.sequence,
                ability,
            })
        })
        .await
    }

    async fn submit_microrealm_ingress(
        &self,
        lease: CorePrivateLifeTransportLease,
        submit: impl FnOnce(
            &CorePrivateMicrorealmDriverHandle,
        ) -> Result<(), CorePrivateMicrorealmIngressError>,
    ) -> Result<(), CorePrivateLifeSessionError> {
        let state = self.state.lock().await;
        let entry = state
            .sessions
            .get(&lease.account_id)
            .ok_or(CorePrivateLifeSessionError::SessionUnavailable)?;
        if entry.active.as_ref().map(|active| active.lease) != Some(lease) {
            return Err(CorePrivateLifeSessionError::StaleTransport);
        }
        let handle = entry
            .microrealm
            .as_ref()
            .map(|bound| bound.driver.handle())
            .ok_or(CorePrivateLifeSessionError::MicrorealmUnavailable)?;
        // This non-awaiting reducer enqueue is linearized with transport replacement/detach. The
        // one-way lock order is session -> ingress; the driver never calls back into the session.
        submit(&handle)?;
        Ok(())
    }

    /// Removes exactly one terminal or transfer-retired live owner without touching the shared
    /// writer. A later danger generation must bind a fresh runtime.
    pub async fn unbind_microrealm(
        &self,
        lease: CorePrivateMicrorealmBindingLease,
    ) -> Result<CorePrivateMicrorealmDriverReport, CorePrivateLifeSessionError> {
        let mut state = self.state.lock().await;
        if !state.accepting {
            return Err(CorePrivateLifeSessionError::Retired);
        }
        let entry = state
            .sessions
            .get_mut(&lease.account_id)
            .ok_or(CorePrivateLifeSessionError::SessionUnavailable)?;
        if entry.authenticated.account_id.as_bytes() != lease.account_id
            || entry
                .microrealm
                .as_ref()
                .is_none_or(|bound| bound.lease != lease)
        {
            return Err(CorePrivateLifeSessionError::MicrorealmUnavailable);
        }
        let bound = entry
            .microrealm
            .take()
            .ok_or(CorePrivateLifeSessionError::MicrorealmUnavailable)?;
        drop(state);
        let b3_result = if let Some(b3_rewards) = &bound.b3_rewards {
            b3_rewards.shutdown().await.map(|_| ())
        } else {
            Ok(())
        };
        let driver_result = bound.driver.shutdown().await;
        b3_result?;
        driver_result.map_err(Into::into)
    }

    /// Binds the current Boss-exit extraction actor to the exact private-life writer. This is
    /// independent from Recall because terminal authority may exist before or after Recall is
    /// armed, but both bindings always share the session's one reliable sequence.
    pub async fn bind_extraction(
        &self,
        lease: CorePrivateLifeTransportLease,
    ) -> Result<CoreExtractionConnectionLease, CorePrivateLifeSessionError> {
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
        if entry.extraction_bound {
            return Err(CorePrivateLifeSessionError::ExtractionAlreadyBound);
        }
        let extraction = self
            .extraction
            .as_ref()
            .ok_or(CorePrivateLifeSessionError::ExtractionUnavailable)?;
        let prepared = extraction
            .prepare(entry.authenticated, Arc::clone(&active.writer))
            .await?;
        let attached = match extraction.commit(prepared).await {
            Ok(attached) => attached,
            Err(error) => {
                let _ = extraction.abort(prepared).await;
                return Err(error.into());
            }
        };
        entry.extraction_bound = true;
        entry.extraction_lease = Some(attached.lease);
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

    pub async fn extraction_lease(
        &self,
        lease: CorePrivateLifeTransportLease,
    ) -> Result<CoreExtractionConnectionLease, CorePrivateLifeSessionError> {
        let state = self.state.lock().await;
        let entry = state
            .sessions
            .get(&lease.account_id)
            .ok_or(CorePrivateLifeSessionError::SessionUnavailable)?;
        if entry.active.as_ref().map(|active| active.lease) != Some(lease) {
            return Err(CorePrivateLifeSessionError::StaleTransport);
        }
        entry
            .extraction_lease
            .ok_or(CorePrivateLifeSessionError::ExtractionNotBound)
    }

    /// Consumes extraction replay authority only after the exact committed Hall projection has
    /// been installed. The session writer remains live for Hall control.
    pub async fn acknowledge_extraction_hall_installed(
        &self,
        lease: CorePrivateLifeTransportLease,
        projection: CoreExtractionHallProjection,
    ) -> Result<(), CorePrivateLifeSessionError> {
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
        let extraction_lease = entry
            .extraction_lease
            .ok_or(CorePrivateLifeSessionError::ExtractionNotBound)?;
        if projection.lease() != extraction_lease {
            return Err(CorePrivateLifeSessionError::StaleTransport);
        }
        self.extraction
            .as_ref()
            .ok_or(CorePrivateLifeSessionError::ExtractionUnavailable)?
            .acknowledge_hall_installed(projection)
            .await?;
        entry.extraction_bound = false;
        entry.extraction_lease = None;
        Ok(())
    }

    /// Clears extraction's dynamic transport binding after another terminal producer wins. The
    /// terminal coordinator retires the actor first; an already-cleared runtime lease is therefore
    /// an expected exact outcome and never retires the shared session writer.
    pub async fn unbind_extraction(
        &self,
        lease: CorePrivateLifeTransportLease,
    ) -> Result<CoreExtractionTransportDetach, CorePrivateLifeSessionError> {
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
        let extraction_lease = entry
            .extraction_lease
            .ok_or(CorePrivateLifeSessionError::ExtractionNotBound)?;
        let outcome = self
            .extraction
            .as_ref()
            .ok_or(CorePrivateLifeSessionError::ExtractionUnavailable)?
            .detach(extraction_lease)
            .await;
        entry.extraction_bound = false;
        entry.extraction_lease = None;
        Ok(outcome)
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
        let microrealm = entry.microrealm.as_ref().map(|bound| bound.driver.handle());
        let extraction = if let Some(extraction_lease) = entry.extraction_lease.take() {
            Some(
                self.extraction
                    .as_ref()
                    .ok_or(CorePrivateLifeSessionError::ExtractionUnavailable)?
                    .detach(extraction_lease)
                    .await,
            )
        } else {
            None
        };
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
            if let Some(b3_rewards) = entry
                .microrealm
                .as_ref()
                .and_then(|bound| bound.b3_rewards.as_ref())
            {
                b3_rewards.detach_writer(
                    CoreB3RewardWriterGeneration::new(lease.generation.get())
                        .expect("session transport generations are nonzero"),
                );
            }
            active
                .writer
                .retire(SESSION_DETACHED_CLOSE_CODE, SESSION_DETACHED_REASON);
        }
        if let Some(handle) = microrealm {
            match handle.neutralize_for_link_lost() {
                Ok(()) | Err(CorePrivateMicrorealmIngressError::DriverFrozen) => {}
                Err(error) => return Err(error.into()),
            }
        }
        drop(state);
        Ok(CorePrivateLifeTransportDetach::Detached { recall, extraction })
    }

    /// Stops admission and retires every writer before Recall inbox shutdown. Duplicate
    /// connection handles are harmless and let the caller close all aggregate owners uniformly.
    pub async fn begin_shutdown(&self) -> Vec<quinn::Connection> {
        let mut state = self.state.lock().await;
        state.accepting = false;
        state.shutdown_started = true;
        let mut connections = Vec::new();
        let mut microrealm_drivers = Vec::new();
        for entry in state.sessions.values_mut() {
            if let Some(active) = entry.active.take() {
                active.writer.retire(
                    crate::SERVER_SHUTDOWN_CLOSE_CODE,
                    b"private-life server shutdown",
                );
                connections.push(active.writer.connection().clone());
            }
            entry.recall_lease = None;
            entry.extraction_lease = None;
            if let Some(bound) = entry.microrealm.take() {
                microrealm_drivers.push(bound);
            }
        }
        drop(state);
        let mut microrealm_shutdown_failures = 0_usize;
        for bound in microrealm_drivers {
            let b3_failed = if let Some(b3_rewards) = &bound.b3_rewards {
                b3_rewards.shutdown().await.is_err()
            } else {
                false
            };
            let driver_failed = bound.driver.shutdown().await.is_err();
            if b3_failed || driver_failed {
                microrealm_shutdown_failures = microrealm_shutdown_failures.saturating_add(1);
            }
        }
        if microrealm_shutdown_failures > 0 {
            self.state.lock().await.microrealm_shutdown_failures = microrealm_shutdown_failures;
        }
        connections.extend(self.recall.begin_shutdown().await);
        if let Some(extraction) = &self.extraction {
            connections.extend(extraction.begin_shutdown().await);
        }
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
        let extraction = if let Some(extraction) = &self.extraction {
            Some(extraction.finish_shutdown().await?)
        } else {
            None
        };
        let mut state = self.state.lock().await;
        let retired_account_count = state.sessions.len();
        let remaining_active_transports = state
            .sessions
            .values()
            .filter(|entry| entry.active.is_some())
            .count();
        let extraction_zero_residue = extraction.as_ref().is_none_or(|report| report.zero_residue);
        let remaining_microrealm_bindings = state
            .sessions
            .values()
            .filter(|entry| entry.microrealm.is_some())
            .count();
        let microrealm_shutdown_failures = state.microrealm_shutdown_failures;
        state.sessions.clear();
        Ok(CorePrivateLifeSessionReport {
            retired_account_count,
            remaining_active_transports,
            recall,
            extraction,
            remaining_microrealm_bindings,
            microrealm_shutdown_failures,
            zero_residue: remaining_active_transports == 0
                && recall.zero_residue
                && extraction_zero_residue
                && remaining_microrealm_bindings == 0
                && microrealm_shutdown_failures == 0,
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
            extraction_bound_count: state
                .sessions
                .values()
                .filter(|entry| entry.extraction_bound)
                .count(),
            microrealm_bound_count: state
                .sessions
                .values()
                .filter(|entry| entry.microrealm.is_some())
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
        ActionResultCode, CorePrivateRouteContentRevisionV1, CorePrivateRoutePhaseV1,
        CorePrivateRouteRoomV1, CorePrivateRouteSceneV1, ManifestHash, RecallFrameV1,
        RecallIntentV1, RecallResultV1, ReliableEvent, TERMINAL_INVENTORY_SCHEMA_VERSION,
        WorldFlowContentRevisionV1,
    };
    use rcgen::generate_simple_self_signed;
    use rustls::pki_types::PrivatePkcs8KeyDer;
    use sim_core::Tick;

    use super::*;
    use crate::{
        AccountId, CoreBellPortalAuthority, CoreBellPortalBinding,
        CorePrivateMicrorealmDriverState, CorePrivateRouteActorAdvance,
        CorePrivateRouteActorDirectory, CorePrivateRouteActorPosition, CorePrivateRouteActorSeed,
        CoreRecallIntentAuthority, ProductionRecallIntentActor, ProductionRecallPendingAuthorityV1,
    };

    const ACCOUNT_ID: [u8; 16] = [71; 16];
    const CHARACTER_ID: [u8; 16] = [72; 16];
    const LINEAGE_ID: [u8; 16] = [73; 16];

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

    fn hash(byte: char) -> ManifestHash {
        ManifestHash::new(byte.to_string().repeat(64)).unwrap()
    }

    fn route_revision() -> CorePrivateRouteContentRevisionV1 {
        CorePrivateRouteContentRevisionV1 {
            records_blake3: hash('a'),
            assets_blake3: hash('b'),
            localization_blake3: hash('c'),
        }
    }

    fn world_revision() -> WorldFlowContentRevisionV1 {
        WorldFlowContentRevisionV1 {
            records_blake3: hash('d'),
            assets_blake3: hash('e'),
            localization_blake3: hash('f'),
        }
    }

    fn live_microrealm() -> (
        CorePrivateRouteActorDirectory,
        CorePrivateRouteActorLease,
        CorePrivateMicrorealmRuntime,
    ) {
        let routes = CorePrivateRouteActorDirectory::new();
        let lease = routes
            .register_actor(
                authenticated(),
                CorePrivateRouteActorSeed {
                    character_id: CHARACTER_ID,
                    character_version: 2,
                    content_revision: route_revision(),
                    world_flow_revision: world_revision(),
                    position: CorePrivateRouteActorPosition {
                        instance_lineage_id: Some(LINEAGE_ID),
                        scene: CorePrivateRouteSceneV1::CoreMicrorealm,
                        room: None,
                        phase: CorePrivateRoutePhaseV1::MicrorealmDormant,
                    },
                },
                7,
            )
            .unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let world = sim_content::load_core_development_world_flow(&root).unwrap();
        let scene = world.compile_microrealm_scene().unwrap();
        let encounters = sim_content::load_core_development_encounter_rooms(&root).unwrap();
        let runtime = CorePrivateMicrorealmRuntime::new(
            routes.clone(),
            lease,
            &route_revision(),
            &scene,
            encounters,
            world,
            crate::combat_factory::core_character_combat_test_fixture(CHARACTER_ID),
        )
        .unwrap();
        (routes, lease, runtime)
    }

    async fn commit_bell_route(
        routes: &CorePrivateRouteActorDirectory,
        route_lease: CorePrivateRouteActorLease,
    ) -> CoreBellPortalTransition {
        for advance in [
            CorePrivateRouteActorAdvance::MicrorealmWaiting,
            CorePrivateRouteActorAdvance::MicrorealmActive,
            CorePrivateRouteActorAdvance::MicrorealmCleared,
        ] {
            routes.advance(route_lease, advance).await.unwrap();
        }
        routes
            .set_bell_portal_in_range(route_lease, true)
            .await
            .unwrap();
        let binding = CoreBellPortalBinding {
            account_id: ACCOUNT_ID,
            character_id: CHARACTER_ID,
            mutation_id: [74; 16],
            instance_lineage_id: LINEAGE_ID,
            entry_restore_point_id: [75; 16],
            character_version: 2,
            content_revision: world_revision(),
        };
        let permit = routes.prepare_bell_portal(binding.clone()).await.unwrap();
        let transition = CoreBellPortalTransition {
            binding,
            transfer_id: [76; 16],
            destination_character_version: 3,
        };
        routes
            .commit_bell_portal(permit, transition.clone())
            .await
            .unwrap();
        transition
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
        let recall = Arc::new(CoreRecallActorDirectory::<FixedClock, _>::new(ticks));
        let sessions = Arc::new(CorePrivateLifeSessionDirectory::new(Arc::clone(&recall)));
        let (server_endpoint, client_endpoint, client, server) = live_connection_pair().await;

        let attached = sessions
            .attach_transport(authenticated(), server, 5_000)
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
    #[allow(
        clippy::too_many_lines,
        reason = "one real-QUIC lifecycle proves generation-safe ingress, handoff, LinkLost, reconnect, transport-independent retirement, and residue"
    )]
    async fn live_microrealm_survives_handoff_and_link_lost_until_exact_unbind() {
        let ticks = Arc::new(TickSource(AtomicU64::new(100)));
        let recall = Arc::new(CoreRecallActorDirectory::<FixedClock, _>::new(ticks));
        let sessions = Arc::new(CorePrivateLifeSessionDirectory::new(recall));
        let (routes, route_lease, runtime) = live_microrealm();

        let (first_server_endpoint, first_client_endpoint, first_client, first_server) =
            live_connection_pair().await;
        let first = sessions
            .attach_transport(authenticated(), first_server, 5_000)
            .await
            .unwrap();
        let bound = sessions
            .bind_microrealm(first.lease, runtime)
            .await
            .unwrap();
        assert_eq!(bound.lease.route_lease(), route_lease);
        assert_eq!(sessions.snapshot().await.microrealm_bound_count, 1);
        sessions
            .submit_microrealm_input(
                first.lease,
                &InputFrame {
                    sequence: 1,
                    client_tick: 1,
                    movement_x_milli: 1_000,
                    movement_y_milli: 0,
                    aim_x_milli: 0,
                    aim_y_milli: 1_000,
                    held_primary: true,
                    primary_sequence: 1,
                    ability_1_sequence: 0,
                    ability_2_sequence: 0,
                },
            )
            .await
            .unwrap();
        sessions
            .submit_microrealm_input(
                first.lease,
                &InputFrame {
                    sequence: 2,
                    client_tick: 2,
                    movement_x_milli: 1_000,
                    movement_y_milli: 0,
                    aim_x_milli: 0,
                    aim_y_milli: 1_000,
                    held_primary: false,
                    primary_sequence: 0,
                    ability_1_sequence: 0,
                    ability_2_sequence: 0,
                },
            )
            .await
            .unwrap();
        sessions
            .submit_microrealm_action(
                first.lease,
                &ActionFrame {
                    sequence: 1,
                    client_tick: 2,
                    action: ActionKind::Ability2Press,
                },
            )
            .await
            .unwrap();

        let (second_server_endpoint, second_client_endpoint, second_client, second_server) =
            live_connection_pair().await;
        let second = sessions
            .attach_transport(authenticated(), second_server, 5_100)
            .await
            .unwrap();
        assert_eq!(second.microrealm.as_ref().unwrap().lease, bound.lease);
        assert!(matches!(
            sessions
                .submit_microrealm_input(
                    first.lease,
                    &InputFrame {
                        sequence: 3,
                        client_tick: 3,
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
                .await,
            Err(CorePrivateLifeSessionError::StaleTransport)
        ));
        sessions
            .submit_microrealm_action(
                second.lease,
                &ActionFrame {
                    sequence: 2,
                    client_tick: 3,
                    action: ActionKind::Ability1Press,
                },
            )
            .await
            .unwrap();
        assert!(matches!(
            sessions.microrealm_authority(first.lease).await,
            Err(CorePrivateLifeSessionError::StaleTransport)
        ));
        assert!(matches!(
            sessions
                .detach_transport(second.lease, 5_200)
                .await
                .unwrap(),
            CorePrivateLifeTransportDetach::Detached { .. }
        ));
        let detached = sessions.snapshot().await;
        assert_eq!(detached.active_transport_count, 0);
        assert_eq!(detached.microrealm_bound_count, 1);

        let (third_server_endpoint, third_client_endpoint, third_client, third_server) =
            live_connection_pair().await;
        let third = sessions
            .attach_transport(authenticated(), third_server, 5_300)
            .await
            .unwrap();
        let rebound = sessions.microrealm_authority(third.lease).await.unwrap();
        assert_eq!(rebound.lease, bound.lease);
        assert_eq!(third.microrealm.as_ref().unwrap().lease, bound.lease);
        assert!(matches!(
            sessions.detach_transport(third.lease, 5_400).await.unwrap(),
            CorePrivateLifeTransportDetach::Detached { .. }
        ));
        let driver_report = sessions.unbind_microrealm(bound.lease).await.unwrap();
        assert!(driver_report.task_joined);
        assert!(!driver_report.driver_task_live_after_join);
        assert_eq!(driver_report.link_lost_neutralizations, 2);
        assert_eq!(sessions.snapshot().await.microrealm_bound_count, 0);

        for connection in sessions.begin_shutdown().await {
            connection.close(0_u32.into(), b"test shutdown");
        }
        let report = sessions.finish_shutdown().await.unwrap();
        assert_eq!(report.remaining_microrealm_bindings, 0);
        assert!(report.zero_residue);
        routes.begin_shutdown();
        assert!(routes.finish_shutdown().await.unwrap().zero_residue);

        for client in [&first_client, &second_client, &third_client] {
            client.close(0_u32.into(), b"test complete");
        }
        drop(first);
        drop(second);
        drop(third);
        drop(bound);
        drop(rebound);
        first_server_endpoint.wait_idle().await;
        first_client_endpoint.wait_idle().await;
        second_server_endpoint.wait_idle().await;
        second_client_endpoint.wait_idle().await;
        third_server_endpoint.wait_idle().await;
        third_client_endpoint.wait_idle().await;
    }

    #[tokio::test]
    async fn bell_conversion_keeps_one_session_task_and_observer_across_reconnect() {
        let ticks = Arc::new(TickSource(AtomicU64::new(100)));
        let recall = Arc::new(CoreRecallActorDirectory::<FixedClock, _>::new(ticks));
        let sessions = Arc::new(CorePrivateLifeSessionDirectory::new(recall));
        let (routes, route_lease, runtime) = live_microrealm();
        let runtime =
            crate::core_private_microrealm_runtime::core_bell_ready_runtime_test_fixture(runtime);
        let (first_server_endpoint, first_client_endpoint, first_client, first_server) =
            live_connection_pair().await;
        let first = sessions
            .attach_transport(authenticated(), first_server, 5_000)
            .await
            .unwrap();
        let mut first_binding = sessions
            .bind_microrealm(first.lease, runtime)
            .await
            .unwrap();
        let prepared = sessions.prepare_bell_handoff(first.lease).await.unwrap();

        let (second_server_endpoint, second_client_endpoint, second_client, second_server) =
            live_connection_pair().await;
        let second = sessions
            .attach_transport(authenticated(), second_server, 5_100)
            .await
            .unwrap();
        let mut second_binding = second.microrealm.expect("reconnected danger binding");
        assert_eq!(first_binding.lease, second_binding.lease);
        assert_eq!(prepared.binding_lease, second_binding.lease);

        let transition = commit_bell_route(&routes, route_lease).await;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let encounters = sim_content::load_core_development_encounter_rooms(&root).unwrap();
        let ready = prepared
            .commit_into_fixed_dungeon(transition, route_revision(), encounters)
            .await
            .unwrap();
        assert_eq!(ready.route_lease, route_lease);
        assert!(matches!(
            first_binding.observer.changed().await.unwrap(),
            CorePrivateMicrorealmDriverState::FixedDungeonReady { ready: published }
                if published == ready
        ));
        assert!(matches!(
            second_binding.observer.changed().await.unwrap(),
            CorePrivateMicrorealmDriverState::FixedDungeonReady { ready: published }
                if published == ready
        ));
        assert!(matches!(
            sessions.advance_fixed_dungeon(first.lease).await,
            Err(CorePrivateLifeSessionError::StaleTransport)
        ));
        let entered = sessions
            .advance_fixed_dungeon(second.lease)
            .await
            .expect("current transport advances to server-selected B1");
        assert_eq!(
            entered.transition.to,
            sim_content::CoreFixedDungeonNode::BellCrossB1
        );
        assert!(matches!(
            second_binding.observer.changed().await.unwrap(),
            CorePrivateMicrorealmDriverState::FixedDungeonReady { ready: published }
                if published.node == sim_content::CoreFixedDungeonNode::BellCrossB1
        ));
        let running = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            second_binding.observer.changed(),
        )
        .await
        .expect("fixed-room driver deadline")
        .expect("fixed-room observation");
        assert!(matches!(
            running,
            CorePrivateMicrorealmDriverState::FixedDungeonRunning { ref frame, .. }
                if frame.tick == Tick(33)
                    && frame.route.room == Some(CorePrivateRouteRoomV1::BellCrossB1)
        ));
        assert!(matches!(
            first_binding.observer.changed().await.unwrap(),
            CorePrivateMicrorealmDriverState::FixedDungeonRunning { ref frame, .. }
                if frame.tick == Tick(33)
        ));
        assert_eq!(sessions.snapshot().await.microrealm_bound_count, 1);

        for connection in sessions.begin_shutdown().await {
            connection.close(0_u32.into(), b"test shutdown");
        }
        assert!(sessions.finish_shutdown().await.unwrap().zero_residue);
        routes.begin_shutdown();
        assert!(routes.finish_shutdown().await.unwrap().zero_residue);
        first_client.close(0_u32.into(), b"test complete");
        second_client.close(0_u32.into(), b"test complete");
        first_server_endpoint.wait_idle().await;
        first_client_endpoint.wait_idle().await;
        second_server_endpoint.wait_idle().await;
        second_client_endpoint.wait_idle().await;
    }

    #[tokio::test]
    #[allow(
        clippy::too_many_lines,
        reason = "one contiguous real-QUIC lifecycle proves handoff, stale detach, LinkLost, reconnect, actor replacement, and zero-residue shutdown"
    )]
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
            .attach_transport(authenticated(), first_server, 5_000)
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
            .attach_transport(authenticated(), second_server, 5_500)
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
                recall: Some(ProductionRecallDetachOutcome::LinkLostStarted { deadline_tick: 192 }),
                ..
            }
        ));
        assert_eq!(sessions.snapshot().await.active_transport_count, 0);

        ticks.0.store(191, Ordering::SeqCst);
        let (third_server_endpoint, third_client_endpoint, third_client, third_server) =
            live_connection_pair().await;
        let third = sessions
            .attach_transport(authenticated(), third_server, 5_900)
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
