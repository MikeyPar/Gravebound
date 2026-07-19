//! Transport-independent automatic Caldus durable-resolution owner.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`DNG-006`, `LOOT-002`,
//! `TECH-015`, `TECH-021`-`023`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-BOSS-001`/`002`, `CONT-REWARD-003`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`, `GB-M03-08`).
//!
//! One worker follows the route binding rather than a QUIC transport. It retries one immutable
//! frozen defeat, lets `PostgreSQL` arbitrate against terminal winners, and acknowledges only the
//! opaque matching result through the existing exclusive driver task.

use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use persistence::{
    PersistenceError, PostgresPersistence, ProductionExtractionExpectedVersionsV1,
    StoredActiveDangerAuthorityV1, StoredWorldFlowRevisionV1,
};
use protocol::{
    CorePendingInventoryStateV1, CorePrivateRoutePhaseV1, ManifestHash, ReliableEvent,
    WorldFlowContentRevisionV1,
};
use thiserror::Error;
use tokio::{
    sync::{Mutex, watch},
    task::{JoinError, JoinHandle},
};

use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CoreCaldusRewardAuthority,
    CoreCaldusRewardAuthorityFailure, CoreCaldusRewardAuthorityFailureKind,
    CoreDurableCaldusResolution, CorePrivateCaldusDefeatHandoff, CorePrivateCaldusRewardCommit,
    CorePrivateMicrorealmDriverError, CorePrivateMicrorealmDriverHandle,
    CorePrivateMicrorealmDriverObserver, CorePrivateMicrorealmDriverState,
    CorePrivateRouteActorDirectory, CoreReliableWriter, CoreReliableWriterError,
    ProductionExtractionCaldusReservationV1, project_core_pending_inventory,
};

const INITIAL_RETRY_BACKOFF: Duration = Duration::from_millis(25);
const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(1);

type RuntimeFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

trait CoreCaldusResolutionSink: Send + Sync {
    fn acknowledge(
        &self,
        resolution: CoreDurableCaldusResolution,
    ) -> RuntimeFuture<'_, Result<CorePrivateCaldusRewardCommit, CorePrivateMicrorealmDriverError>>;
}

/// Loads one post-reward, terminal-first custody projection. The client cannot select the danger
/// root, content revision, versions, item destinations, or material balances.
pub trait CoreCaldusPendingInventoryAuthority: Send + Sync {
    fn load(
        &self,
        authenticated: AuthenticatedAccount,
        handoff: CorePrivateCaldusDefeatHandoff,
        world_flow_revision: WorldFlowContentRevisionV1,
    ) -> RuntimeFuture<
        '_,
        Result<CoreCaldusPendingInventorySnapshot, CoreCaldusRewardAuthorityFailure>,
    >;
}

/// Type-erased production seam that constructs, registers, and session-binds one exact actor.
/// The activator consumes the reservation and owns all post-permit cleanup on failure.
pub trait CoreCaldusExtractionActivator: Send + Sync {
    fn activate(
        &self,
        reservation: ProductionExtractionCaldusReservationV1,
        expected_versions: ProductionExtractionExpectedVersionsV1,
    ) -> RuntimeFuture<'_, Result<(), CoreCaldusRewardAuthorityFailure>>;
}

/// Server-only coherent custody material. The wire projection is never promoted back into
/// persistence authority; the original storage versions travel beside it for actor sealing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCaldusPendingInventorySnapshot {
    projection: CorePendingInventoryStateV1,
    expected_versions: ProductionExtractionExpectedVersionsV1,
}

impl CoreCaldusPendingInventorySnapshot {
    pub fn new(
        projection: CorePendingInventoryStateV1,
        expected_versions: ProductionExtractionExpectedVersionsV1,
    ) -> Result<Self, CoreCaldusRewardAuthorityFailure> {
        projection
            .validate()
            .map_err(|error| fatal_failure(error.to_string()))?;
        let wire = projection.expected_extraction_versions;
        if wire.account != expected_versions.account
            || wire.character != expected_versions.character
            || wire.world != expected_versions.world
            || wire.inventory != expected_versions.inventory
            || wire.life_clock != expected_versions.life_metrics
        {
            return Err(fatal_failure(
                "pending inventory projection does not match its storage version authority",
            ));
        }
        Ok(Self {
            projection,
            expected_versions,
        })
    }

    #[must_use]
    pub const fn projection(&self) -> &CorePendingInventoryStateV1 {
        &self.projection
    }

    #[must_use]
    pub const fn expected_versions(&self) -> &ProductionExtractionExpectedVersionsV1 {
        &self.expected_versions
    }

    pub fn into_parts(
        self,
    ) -> (
        CorePendingInventoryStateV1,
        ProductionExtractionExpectedVersionsV1,
    ) {
        (self.projection, self.expected_versions)
    }
}

impl CoreCaldusPendingInventoryAuthority for PostgresPersistence {
    fn load(
        &self,
        authenticated: AuthenticatedAccount,
        handoff: CorePrivateCaldusDefeatHandoff,
        world_flow_revision: WorldFlowContentRevisionV1,
    ) -> RuntimeFuture<
        '_,
        Result<CoreCaldusPendingInventorySnapshot, CoreCaldusRewardAuthorityFailure>,
    > {
        Box::pin(async move {
            if authenticated.namespace != AuthenticatedNamespace::WipeableTest
                || authenticated.account_id.as_bytes() != handoff.route_lease().account_id()
                || handoff.character_id() != handoff.route_lease().character_id()
            {
                return Err(fatal_failure(
                    "pending inventory authority does not match frozen Caldus defeat",
                ));
            }
            let snapshot = self
                .load_current_danger_extraction_snapshot_v1(
                    StoredActiveDangerAuthorityV1 {
                        account_id: authenticated.account_id.as_bytes(),
                        character_id: handoff.character_id(),
                        instance_lineage_id: handoff.instance_lineage_id(),
                        entry_restore_point_id: handoff.entry_restore_point_id(),
                    },
                    &StoredWorldFlowRevisionV1 {
                        records_blake3: world_flow_revision.records_blake3.as_str().to_owned(),
                        assets_blake3: world_flow_revision.assets_blake3.as_str().to_owned(),
                        localization_blake3: world_flow_revision
                            .localization_blake3
                            .as_str()
                            .to_owned(),
                    },
                )
                .await
                .map_err(|error| snapshot_failure(&error))?;
            let expected_versions = snapshot.expected_versions;
            let projection = project_core_pending_inventory(&snapshot)
                .map_err(|error| fatal_failure(error.to_string()))?;
            CoreCaldusPendingInventorySnapshot::new(projection, expected_versions)
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CoreCaldusRewardWriterGeneration(u64);

impl CoreCaldusRewardWriterGeneration {
    pub fn new(value: u64) -> Result<Self, CorePrivateCaldusRewardRuntimeError> {
        if value == 0 {
            return Err(CorePrivateCaldusRewardRuntimeError::InvalidWriterGeneration);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone)]
struct WriterBinding {
    generation: CoreCaldusRewardWriterGeneration,
    writer: Arc<CoreReliableWriter>,
}

#[derive(Debug, Clone)]
struct RetainedPublication {
    encounter_id: [u8; 16],
    server_tick: u64,
    pending_inventory: ReliableEvent,
    route: ReliableEvent,
}

enum PostAcknowledgementOutcome {
    Shutdown,
    AcknowledgedOnly,
    Publication(Box<RetainedPublication>),
}

pub(crate) struct CorePrivateCaldusRewardRuntimeConfig {
    authenticated: AuthenticatedAccount,
    progression_content_revision: ManifestHash,
    authority: Arc<dyn CoreCaldusRewardAuthority>,
    pending_inventory: Option<Arc<dyn CoreCaldusPendingInventoryAuthority>>,
    world_flow_revision: Option<WorldFlowContentRevisionV1>,
    extraction_route: Option<CorePrivateRouteActorDirectory>,
    extraction_activator: Option<Arc<dyn CoreCaldusExtractionActivator>>,
}

impl CorePrivateCaldusRewardRuntimeConfig {
    pub(crate) fn new(
        authenticated: AuthenticatedAccount,
        progression_content_revision: ManifestHash,
        authority: Arc<dyn CoreCaldusRewardAuthority>,
    ) -> Self {
        Self {
            authenticated,
            progression_content_revision,
            authority,
            pending_inventory: None,
            world_flow_revision: None,
            extraction_route: None,
            extraction_activator: None,
        }
    }

    #[must_use]
    pub(crate) fn with_pending_inventory(
        mut self,
        authority: Arc<dyn CoreCaldusPendingInventoryAuthority>,
        world_flow_revision: WorldFlowContentRevisionV1,
    ) -> Self {
        self.pending_inventory = Some(authority);
        self.world_flow_revision = Some(world_flow_revision);
        self
    }

    #[must_use]
    pub(crate) fn with_extraction_activation(
        mut self,
        route_directory: CorePrivateRouteActorDirectory,
        activator: Arc<dyn CoreCaldusExtractionActivator>,
    ) -> Self {
        self.extraction_route = Some(route_directory);
        self.extraction_activator = Some(activator);
        self
    }
}

impl CoreCaldusResolutionSink for CorePrivateMicrorealmDriverHandle {
    fn acknowledge(
        &self,
        resolution: CoreDurableCaldusResolution,
    ) -> RuntimeFuture<'_, Result<CorePrivateCaldusRewardCommit, CorePrivateMicrorealmDriverError>>
    {
        Box::pin(async move { self.commit_caldus_reward(resolution).await })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorePrivateCaldusRewardRuntimeState {
    Watching,
    Resolving {
        attempts: u32,
    },
    LoadingInventory {
        attempts: u32,
    },
    ReservingExtraction,
    ActivatingExtraction,
    Acknowledged {
        encounter_id: [u8; 16],
        exit_instance_id: [u8; 16],
    },
    AwaitingWriter {
        encounter_id: [u8; 16],
    },
    Published {
        encounter_id: [u8; 16],
        generation: CoreCaldusRewardWriterGeneration,
    },
    Faulted {
        message: Arc<str>,
    },
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateCaldusRewardRuntimeReport {
    pub resolution_attempts: u32,
    pub acknowledgements: u32,
    pub snapshot_attempts: u32,
    pub extraction_reservations: u32,
    pub extraction_activations: u32,
    pub extraction_abort_failures: u32,
    pub publication_attempts: u32,
    pub publication_generations: u32,
    pub faulted: bool,
    pub task_joined: bool,
}

#[derive(Debug, Error)]
pub enum CorePrivateCaldusRewardRuntimeError {
    #[error("Caldus reward writer generation must be nonzero")]
    InvalidWriterGeneration,
    #[error("Caldus reward runtime task failed")]
    Join(#[source] JoinError),
    #[error(
        "Caldus reward runtime faulted before shutdown ({extraction_abort_failures} reservation abort failures)"
    )]
    Faulted { extraction_abort_failures: u32 },
}

pub struct CorePrivateCaldusRewardRuntime {
    writer_tx: watch::Sender<Option<WriterBinding>>,
    state_rx: watch::Receiver<CorePrivateCaldusRewardRuntimeState>,
    shutdown_tx: watch::Sender<bool>,
    terminal_completed: AtomicBool,
    join: Mutex<Option<JoinHandle<CorePrivateCaldusRewardRuntimeReport>>>,
}

impl std::fmt::Debug for CorePrivateCaldusRewardRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CorePrivateCaldusRewardRuntime")
            .field("state", &*self.state_rx.borrow())
            .finish_non_exhaustive()
    }
}

impl CorePrivateCaldusRewardRuntime {
    pub(crate) fn spawn(
        config: CorePrivateCaldusRewardRuntimeConfig,
        driver: CorePrivateMicrorealmDriverHandle,
        observer: CorePrivateMicrorealmDriverObserver,
        initial_writer: Option<(CoreCaldusRewardWriterGeneration, Arc<CoreReliableWriter>)>,
    ) -> Self {
        Self::spawn_with_sink(config, Arc::new(driver), observer, initial_writer)
    }

    fn spawn_with_sink(
        config: CorePrivateCaldusRewardRuntimeConfig,
        sink: Arc<dyn CoreCaldusResolutionSink>,
        observer: CorePrivateMicrorealmDriverObserver,
        initial_writer: Option<(CoreCaldusRewardWriterGeneration, Arc<CoreReliableWriter>)>,
    ) -> Self {
        let initial_writer =
            initial_writer.map(|(generation, writer)| WriterBinding { generation, writer });
        let (writer_tx, writer_rx) = watch::channel(initial_writer);
        let (state_tx, state_rx) = watch::channel(CorePrivateCaldusRewardRuntimeState::Watching);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let join = tokio::spawn(run_runtime(
            config,
            sink,
            observer,
            writer_rx,
            state_tx,
            shutdown_rx,
        ));
        Self {
            writer_tx,
            state_rx,
            shutdown_tx,
            terminal_completed: AtomicBool::new(false),
            join: Mutex::new(Some(join)),
        }
    }

    #[must_use]
    pub fn observe(&self) -> watch::Receiver<CorePrivateCaldusRewardRuntimeState> {
        self.state_rx.clone()
    }

    /// Prevents the danger owner from being discarded while a durable boss result, reservation,
    /// extraction actor, retained publication, or fail-closed reconciliation still owns it.
    #[must_use]
    pub fn blocks_danger_unbind(&self) -> bool {
        !self.terminal_completed.load(Ordering::Acquire)
            && matches!(
                &*self.state_rx.borrow(),
                CorePrivateCaldusRewardRuntimeState::Resolving { .. }
                    | CorePrivateCaldusRewardRuntimeState::LoadingInventory { .. }
                    | CorePrivateCaldusRewardRuntimeState::ReservingExtraction
                    | CorePrivateCaldusRewardRuntimeState::ActivatingExtraction
                    | CorePrivateCaldusRewardRuntimeState::Acknowledged { .. }
                    | CorePrivateCaldusRewardRuntimeState::AwaitingWriter { .. }
                    | CorePrivateCaldusRewardRuntimeState::Published { .. }
                    | CorePrivateCaldusRewardRuntimeState::Faulted { .. }
            )
    }

    /// Releases retained boss-exit publication only after the owning terminal path has committed
    /// its destination or an exact competing terminal has retired extraction authority.
    pub fn acknowledge_terminal_complete(&self) {
        self.terminal_completed.store(true, Ordering::Release);
        self.shutdown_tx.send_replace(true);
    }

    pub fn attach_writer(
        &self,
        generation: CoreCaldusRewardWriterGeneration,
        writer: Arc<CoreReliableWriter>,
    ) {
        self.writer_tx
            .send_replace(Some(WriterBinding { generation, writer }));
    }

    pub fn detach_writer(&self, generation: CoreCaldusRewardWriterGeneration) {
        self.writer_tx.send_if_modified(|current| {
            if current
                .as_ref()
                .is_some_and(|binding| binding.generation == generation)
            {
                *current = None;
                true
            } else {
                false
            }
        });
    }

    pub async fn shutdown(
        &self,
    ) -> Result<CorePrivateCaldusRewardRuntimeReport, CorePrivateCaldusRewardRuntimeError> {
        self.shutdown_tx.send_replace(true);
        match self.join.lock().await.take() {
            Some(join) => {
                let report = join
                    .await
                    .map_err(CorePrivateCaldusRewardRuntimeError::Join)?;
                if report.faulted {
                    Err(CorePrivateCaldusRewardRuntimeError::Faulted {
                        extraction_abort_failures: report.extraction_abort_failures,
                    })
                } else {
                    Ok(report)
                }
            }
            None => Ok(CorePrivateCaldusRewardRuntimeReport {
                resolution_attempts: 0,
                acknowledgements: 0,
                snapshot_attempts: 0,
                extraction_reservations: 0,
                extraction_activations: 0,
                extraction_abort_failures: 0,
                publication_attempts: 0,
                publication_generations: 0,
                faulted: false,
                task_joined: true,
            }),
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one select loop keeps durable resolution, snapshot, and generation-safe publication ordering auditable"
)]
async fn run_runtime(
    config: CorePrivateCaldusRewardRuntimeConfig,
    sink: Arc<dyn CoreCaldusResolutionSink>,
    mut observer: CorePrivateMicrorealmDriverObserver,
    mut writer_rx: watch::Receiver<Option<WriterBinding>>,
    state_tx: watch::Sender<CorePrivateCaldusRewardRuntimeState>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> CorePrivateCaldusRewardRuntimeReport {
    let CorePrivateCaldusRewardRuntimeConfig {
        authenticated,
        progression_content_revision,
        authority,
        pending_inventory,
        world_flow_revision,
        extraction_route,
        extraction_activator,
    } = config;
    let mut report = CorePrivateCaldusRewardRuntimeReport {
        resolution_attempts: 0,
        acknowledgements: 0,
        snapshot_attempts: 0,
        extraction_reservations: 0,
        extraction_activations: 0,
        extraction_abort_failures: 0,
        publication_attempts: 0,
        publication_generations: 0,
        faulted: false,
        task_joined: true,
    };
    let inventory_configured = pending_inventory.is_some() && world_flow_revision.is_some();
    let extraction_configured = extraction_route.is_some() && extraction_activator.is_some();
    let configuration_is_complete = pending_inventory.is_some() == world_flow_revision.is_some()
        && extraction_route.is_some() == extraction_activator.is_some()
        && (!extraction_configured || inventory_configured);
    if !configuration_is_complete {
        report.faulted = true;
        state_tx.send_replace(CorePrivateCaldusRewardRuntimeState::Faulted {
            message: Arc::from("Caldus custody/extraction pipeline is only partially configured"),
        });
        return report;
    }
    let mut acknowledged = false;
    let mut publication: Option<RetainedPublication> = None;
    let mut attempted_generation: Option<CoreCaldusRewardWriterGeneration> = None;

    loop {
        if *shutdown_rx.borrow() {
            break;
        }
        let current_writer = writer_rx.borrow().clone();
        if let Some(retained) = &publication
            && let Some(binding) = current_writer
            && attempted_generation.is_none_or(|prior| binding.generation > prior)
        {
            attempted_generation = Some(binding.generation);
            report.publication_attempts = report.publication_attempts.saturating_add(1);
            if publish(&binding.writer, retained).await.is_ok() {
                report.publication_generations = report.publication_generations.saturating_add(1);
                state_tx.send_replace(CorePrivateCaldusRewardRuntimeState::Published {
                    encounter_id: retained.encounter_id,
                    generation: binding.generation,
                });
            }
            continue;
        }
        let pending =
            match observer.latest() {
                CorePrivateMicrorealmDriverState::CaldusRewardPending {
                    reward_handoff, ..
                } if !acknowledged => Some(reward_handoff),
                _ => None,
            };
        if let Some(handoff) = pending {
            let resolution = match resolve_with_backoff(
                authenticated,
                progression_content_revision.clone(),
                handoff.as_ref().clone(),
                authority.as_ref(),
                &state_tx,
                &mut shutdown_rx,
                &mut report,
            )
            .await
            {
                Ok(Some(resolution)) => resolution,
                Ok(None) => break,
                Err(failure) => {
                    state_tx.send_replace(CorePrivateCaldusRewardRuntimeState::Faulted {
                        message: failure.message,
                    });
                    break;
                }
            };
            let encounter_id = resolution.exit().encounter_id;
            let exit_instance_id = resolution.exit().exit_instance_id;
            let handoff = resolution.handoff().clone();
            match sink.acknowledge(resolution).await {
                Ok(commit) if commit.route.phase == CorePrivateRoutePhaseV1::BossExitReady => {
                    report.acknowledgements = report.acknowledgements.saturating_add(1);
                    acknowledged = true;
                    match prepare_post_acknowledgement(
                        authenticated,
                        encounter_id,
                        &handoff,
                        &commit,
                        pending_inventory.as_ref(),
                        world_flow_revision.as_ref(),
                        extraction_route.as_ref(),
                        extraction_activator.as_ref(),
                        &state_tx,
                        &mut shutdown_rx,
                        &mut report,
                    )
                    .await
                    {
                        Ok(PostAcknowledgementOutcome::Publication(retained)) => {
                            publication = Some(*retained);
                            attempted_generation = None;
                            state_tx.send_replace(
                                CorePrivateCaldusRewardRuntimeState::AwaitingWriter {
                                    encounter_id,
                                },
                            );
                        }
                        Ok(PostAcknowledgementOutcome::AcknowledgedOnly) => {
                            state_tx.send_replace(
                                CorePrivateCaldusRewardRuntimeState::Acknowledged {
                                    encounter_id,
                                    exit_instance_id,
                                },
                            );
                        }
                        Ok(PostAcknowledgementOutcome::Shutdown) => break,
                        Err(message) => {
                            state_tx.send_replace(CorePrivateCaldusRewardRuntimeState::Faulted {
                                message,
                            });
                            break;
                        }
                    }
                    continue;
                }
                Ok(_) => {
                    state_tx.send_replace(CorePrivateCaldusRewardRuntimeState::Faulted {
                        message: Arc::from("Caldus acknowledgement did not publish BossExitReady"),
                    });
                    break;
                }
                Err(error) => {
                    state_tx.send_replace(CorePrivateCaldusRewardRuntimeState::Faulted {
                        message: Arc::from(error.to_string()),
                    });
                    break;
                }
            }
        }

        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            changed = observer.changed() => {
                if changed.is_err() {
                    break;
                }
            }
            changed = writer_rx.changed() => {
                if changed.is_err() {
                    break;
                }
            }
        }
    }
    report.faulted = matches!(
        &*state_tx.borrow(),
        CorePrivateCaldusRewardRuntimeState::Faulted { .. }
    );
    if !report.faulted {
        state_tx.send_replace(CorePrivateCaldusRewardRuntimeState::Shutdown);
    }
    report
}

#[expect(
    clippy::too_many_arguments,
    reason = "the post-acknowledgement boundary keeps every server-owned authority explicit"
)]
async fn prepare_post_acknowledgement(
    authenticated: AuthenticatedAccount,
    encounter_id: [u8; 16],
    handoff: &CorePrivateCaldusDefeatHandoff,
    commit: &CorePrivateCaldusRewardCommit,
    pending_inventory: Option<&Arc<dyn CoreCaldusPendingInventoryAuthority>>,
    world_flow_revision: Option<&WorldFlowContentRevisionV1>,
    extraction_route: Option<&CorePrivateRouteActorDirectory>,
    extraction_activator: Option<&Arc<dyn CoreCaldusExtractionActivator>>,
    state_tx: &watch::Sender<CorePrivateCaldusRewardRuntimeState>,
    shutdown_rx: &mut watch::Receiver<bool>,
    report: &mut CorePrivateCaldusRewardRuntimeReport,
) -> Result<PostAcknowledgementOutcome, Arc<str>> {
    let (Some(inventory), Some(world_flow_revision)) = (pending_inventory, world_flow_revision)
    else {
        return if pending_inventory.is_none() && world_flow_revision.is_none() {
            Ok(PostAcknowledgementOutcome::AcknowledgedOnly)
        } else {
            Err(Arc::from(
                "Caldus pending-inventory authority is only partially configured",
            ))
        };
    };
    let mut reservation = match (extraction_route, extraction_activator) {
        (Some(route_directory), Some(_)) => {
            state_tx.send_replace(CorePrivateCaldusRewardRuntimeState::ReservingExtraction);
            let reservation = ProductionExtractionCaldusReservationV1::reserve(
                authenticated,
                route_directory.clone(),
                handoff,
                commit,
                world_flow_revision.clone(),
            )
            .await
            .map_err(|error| Arc::from(error.to_string()))?;
            report.extraction_reservations = report.extraction_reservations.saturating_add(1);
            Some(reservation)
        }
        (None, None) => None,
        _ => {
            return Err(Arc::from(
                "Caldus extraction activation is only partially configured",
            ));
        }
    };
    let snapshot = match load_snapshot_with_backoff(
        authenticated,
        handoff.clone(),
        world_flow_revision.clone(),
        inventory.as_ref(),
        state_tx,
        shutdown_rx,
        report,
    )
    .await
    {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => {
            abort_reservation(reservation.as_ref(), report, shutdown_rx).await?;
            return Ok(PostAcknowledgementOutcome::Shutdown);
        }
        Err(failure) => {
            return Err(abort_reservation(reservation.as_ref(), report, shutdown_rx)
                .await
                .err()
                .unwrap_or(failure.message));
        }
    };
    let route = reservation.as_ref().map_or_else(
        || commit.route.clone(),
        |reservation| reservation.accepted_route().clone(),
    );
    let (projected, expected_versions) = snapshot.into_parts();
    let retained = match retained_publication(encounter_id, handoff, &route, projected) {
        Ok(retained) => retained,
        Err(message) => {
            return Err(abort_reservation(reservation.as_ref(), report, shutdown_rx)
                .await
                .err()
                .unwrap_or(message));
        }
    };
    if let Some(reservation) = reservation.take() {
        state_tx.send_replace(CorePrivateCaldusRewardRuntimeState::ActivatingExtraction);
        report.extraction_activations = report.extraction_activations.saturating_add(1);
        extraction_activator
            .expect("reservation requires configured activator")
            .activate(reservation, expected_versions)
            .await
            .map_err(|failure| failure.message)?;
    }
    Ok(PostAcknowledgementOutcome::Publication(Box::new(retained)))
}

async fn abort_reservation(
    reservation: Option<&ProductionExtractionCaldusReservationV1>,
    report: &mut CorePrivateCaldusRewardRuntimeReport,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> Result<(), Arc<str>> {
    let Some(reservation) = reservation else {
        return Ok(());
    };
    let mut backoff = INITIAL_RETRY_BACKOFF;
    loop {
        match reservation.abort().await {
            Ok(()) => return Ok(()),
            Err(error) => {
                report.extraction_abort_failures =
                    report.extraction_abort_failures.saturating_add(1);
                let message = Arc::from(format!(
                    "Caldus extraction reservation abort failed: {error}"
                ));
                if *shutdown_rx.borrow() {
                    return Err(message);
                }
                tokio::select! {
                    () = tokio::time::sleep(backoff) => {}
                    changed = shutdown_rx.changed() => {
                        if changed.is_err() || *shutdown_rx.borrow() {
                            return Err(message);
                        }
                    }
                }
                backoff = backoff.saturating_mul(2).min(MAX_RETRY_BACKOFF);
            }
        }
    }
}

async fn load_snapshot_with_backoff(
    authenticated: AuthenticatedAccount,
    handoff: CorePrivateCaldusDefeatHandoff,
    world_flow_revision: WorldFlowContentRevisionV1,
    authority: &dyn CoreCaldusPendingInventoryAuthority,
    state_tx: &watch::Sender<CorePrivateCaldusRewardRuntimeState>,
    shutdown_rx: &mut watch::Receiver<bool>,
    report: &mut CorePrivateCaldusRewardRuntimeReport,
) -> Result<Option<CoreCaldusPendingInventorySnapshot>, CoreCaldusRewardAuthorityFailure> {
    let mut backoff = INITIAL_RETRY_BACKOFF;
    loop {
        report.snapshot_attempts = report.snapshot_attempts.saturating_add(1);
        match authority
            .load(authenticated, handoff.clone(), world_flow_revision.clone())
            .await
        {
            Ok(snapshot) => return Ok(Some(snapshot)),
            Err(failure) if failure.kind == CoreCaldusRewardAuthorityFailureKind::Retryable => {
                state_tx.send_replace(CorePrivateCaldusRewardRuntimeState::LoadingInventory {
                    attempts: report.snapshot_attempts,
                });
                tokio::select! {
                    () = tokio::time::sleep(backoff) => {}
                    changed = shutdown_rx.changed() => {
                        if changed.is_err() || *shutdown_rx.borrow() {
                            return Ok(None);
                        }
                    }
                }
                backoff = backoff.saturating_mul(2).min(MAX_RETRY_BACKOFF);
            }
            Err(failure) => return Err(failure),
        }
    }
}

fn retained_publication(
    encounter_id: [u8; 16],
    handoff: &CorePrivateCaldusDefeatHandoff,
    route: &protocol::CorePrivateRouteStateV1,
    pending_inventory: CorePendingInventoryStateV1,
) -> Result<RetainedPublication, Arc<str>> {
    pending_inventory
        .validate()
        .map_err(|error| Arc::from(error.to_string()))?;
    route
        .validate()
        .map_err(|error| Arc::from(error.to_string()))?;
    if route.phase != CorePrivateRoutePhaseV1::BossExitReady
        || route.character_id != handoff.character_id()
        || route.actor_generation != handoff.route_lease().actor_generation()
        || route.instance_lineage_id != Some(handoff.instance_lineage_id())
        || pending_inventory.character_id != handoff.character_id()
        || pending_inventory.instance_lineage_id != handoff.instance_lineage_id()
        || pending_inventory.entry_restore_point_id != handoff.entry_restore_point_id()
        || pending_inventory.expected_extraction_versions.character != route.character_version
    {
        return Err(Arc::from(
            "Caldus pending inventory does not match acknowledged BossExitReady authority",
        ));
    }
    Ok(RetainedPublication {
        encounter_id,
        server_tick: handoff.defeat_tick().0,
        pending_inventory: ReliableEvent::CorePendingInventoryState(Box::new(pending_inventory)),
        route: ReliableEvent::CorePrivateRouteState(Box::new(route.clone())),
    })
}

async fn publish(
    writer: &CoreReliableWriter,
    retained: &RetainedPublication,
) -> Result<(), CoreReliableWriterError> {
    writer
        .send_event(retained.server_tick, retained.pending_inventory.clone())
        .await?;
    writer
        .send_event(retained.server_tick, retained.route.clone())
        .await?;
    Ok(())
}

fn snapshot_failure(error: &PersistenceError) -> CoreCaldusRewardAuthorityFailure {
    let kind = if matches!(error, PersistenceError::Database(_)) {
        CoreCaldusRewardAuthorityFailureKind::Retryable
    } else {
        CoreCaldusRewardAuthorityFailureKind::Fatal
    };
    CoreCaldusRewardAuthorityFailure {
        kind,
        message: Arc::from(error.to_string()),
    }
}

fn fatal_failure(message: impl Into<Arc<str>>) -> CoreCaldusRewardAuthorityFailure {
    CoreCaldusRewardAuthorityFailure {
        kind: CoreCaldusRewardAuthorityFailureKind::Fatal,
        message: message.into(),
    }
}

async fn resolve_with_backoff(
    authenticated: AuthenticatedAccount,
    progression_content_revision: ManifestHash,
    handoff: CorePrivateCaldusDefeatHandoff,
    authority: &dyn CoreCaldusRewardAuthority,
    state_tx: &watch::Sender<CorePrivateCaldusRewardRuntimeState>,
    shutdown_rx: &mut watch::Receiver<bool>,
    report: &mut CorePrivateCaldusRewardRuntimeReport,
) -> Result<Option<CoreDurableCaldusResolution>, CoreCaldusRewardAuthorityFailure> {
    let mut backoff = INITIAL_RETRY_BACKOFF;
    loop {
        report.resolution_attempts = report.resolution_attempts.saturating_add(1);
        state_tx.send_replace(CorePrivateCaldusRewardRuntimeState::Resolving {
            attempts: report.resolution_attempts,
        });
        match authority
            .resolve(
                authenticated,
                progression_content_revision.clone(),
                handoff.clone(),
            )
            .await
        {
            Ok(resolution) => return Ok(Some(resolution)),
            Err(failure) if failure.kind == CoreCaldusRewardAuthorityFailureKind::Retryable => {
                tokio::select! {
                    () = tokio::time::sleep(backoff) => {}
                    changed = shutdown_rx.changed() => {
                        if changed.is_err() || *shutdown_rx.borrow() {
                            return Ok(None);
                        }
                    }
                }
                backoff = backoff.saturating_mul(2).min(MAX_RETRY_BACKOFF);
            }
            Err(failure) => return Err(failure),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex as StdMutex,
        atomic::{AtomicU32, Ordering},
    };

    use content_schema::{CoreCaldusSafeArrival, MilliTilePoint};
    use protocol::{
        CORE_PENDING_INVENTORY_SCHEMA_VERSION, CorePendingInventoryStateV1,
        CorePrivateRouteContentRevisionV1, CorePrivateRoutePhaseV1, CorePrivateRouteRoomV1,
        CorePrivateRouteSceneV1, ReliableEvent, TerminalExpectedVersionsV1, WireText,
        WorldFlowContentRevisionV1,
    };
    use rcgen::generate_simple_self_signed;
    use rustls::pki_types::PrivatePkcs8KeyDer;
    use sim_core::{
        CoreBossParticipant, CoreBossParticipantLock, CoreCaldusAntiCheatState,
        CoreCaldusDefeatPresence, CoreCaldusEligibilityEvidence, CoreCaldusRecallState,
        CoreCaldusSessionState, EntityId, Tick,
    };

    use super::*;
    use crate::{
        AccountId, AuthenticatedNamespace, CaldusExitPresentation,
        CorePrivateCaldusRewardCommitDisposition, CorePrivateRouteActorAdvance,
        CorePrivateRouteActorDirectory, CorePrivateRouteActorPosition, CorePrivateRouteActorSeed,
    };

    #[derive(Debug)]
    struct RejectingAuthority {
        kind: CoreCaldusRewardAuthorityFailureKind,
        calls: AtomicU32,
        seen: StdMutex<Vec<CorePrivateCaldusDefeatHandoff>>,
    }

    impl CoreCaldusRewardAuthority for RejectingAuthority {
        fn resolve(
            &self,
            _authenticated: AuthenticatedAccount,
            _progression_content_revision: ManifestHash,
            handoff: CorePrivateCaldusDefeatHandoff,
        ) -> RuntimeFuture<'_, Result<CoreDurableCaldusResolution, CoreCaldusRewardAuthorityFailure>>
        {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.seen.lock().expect("seen handoffs").push(handoff);
            Box::pin(async move {
                Err(CoreCaldusRewardAuthorityFailure {
                    kind: self.kind,
                    message: Arc::from("injected authority failure"),
                })
            })
        }
    }

    fn hash(byte: char) -> ManifestHash {
        ManifestHash::new(byte.to_string().repeat(64)).expect("hash")
    }

    fn fixture() -> (
        AuthenticatedAccount,
        CorePrivateCaldusDefeatHandoff,
        CorePrivateRouteActorDirectory,
    ) {
        let authenticated = AuthenticatedAccount {
            account_id: AccountId::new([0x81; 16]).expect("account"),
            namespace: AuthenticatedNamespace::WipeableTest,
        };
        let directory = CorePrivateRouteActorDirectory::new();
        let lease = directory
            .register_actor(
                authenticated,
                CorePrivateRouteActorSeed {
                    character_id: [0x82; 16],
                    character_version: 1,
                    content_revision: CorePrivateRouteContentRevisionV1 {
                        records_blake3: hash('1'),
                        assets_blake3: hash('2'),
                        localization_blake3: hash('3'),
                    },
                    world_flow_revision: WorldFlowContentRevisionV1 {
                        records_blake3: hash('4'),
                        assets_blake3: hash('5'),
                        localization_blake3: hash('6'),
                    },
                    position: CorePrivateRouteActorPosition {
                        instance_lineage_id: Some([0x83; 16]),
                        scene: CorePrivateRouteSceneV1::BellSepulcher,
                        room: Some(CorePrivateRouteRoomV1::CaldusArenaB6),
                        phase: CorePrivateRoutePhaseV1::BossDefeated,
                    },
                },
                1,
            )
            .expect("route actor");
        let route = directory.snapshot(lease).expect("route snapshot");
        let participant = CoreBossParticipant {
            entity_id: EntityId::new(81_000).expect("participant"),
            party_slot: 0,
        };
        let handoff = CorePrivateCaldusDefeatHandoff {
            route_lease: lease,
            route_state_version: route.state_version,
            instance_lineage_id: [0x83; 16],
            entry_restore_point_id: [0x84; 16],
            lock: CoreBossParticipantLock {
                attempt_ordinal: 1,
                participants: vec![participant],
                maximum_health: 7_200,
            },
            active_duration_ticks: 900,
            defeat_tick: Tick(900),
            character_id: [0x82; 16],
            expected_progression_version: 1,
            eligibility: vec![CoreCaldusEligibilityEvidence {
                participant,
                presence_ticks: 900,
                direct_damage: 7_200,
                effective_healing_to_others: 0,
                damage_prevented_on_others: 0,
                objective_credits: 0,
                longest_inactivity_ticks: 0,
                defeat_presence: CoreCaldusDefeatPresence::AliveAndPresent,
                recall_state: CoreCaldusRecallState::Stayed,
                session_state: CoreCaldusSessionState::Valid,
                anti_cheat_state: CoreCaldusAntiCheatState::Valid,
            }],
        };
        (authenticated, handoff, directory)
    }

    fn pending_inventory(handoff: &CorePrivateCaldusDefeatHandoff) -> CorePendingInventoryStateV1 {
        CorePendingInventoryStateV1 {
            schema_version: CORE_PENDING_INVENTORY_SCHEMA_VERSION,
            character_id: handoff.character_id(),
            instance_lineage_id: handoff.instance_lineage_id(),
            entry_restore_point_id: handoff.entry_restore_point_id(),
            location_content_id: WireText::new("dungeon.bell_sepulcher").expect("location"),
            content_revision: WorldFlowContentRevisionV1 {
                records_blake3: hash('4'),
                assets_blake3: hash('5'),
                localization_blake3: hash('6'),
            },
            expected_extraction_versions: TerminalExpectedVersionsV1 {
                account: 1,
                character: 1,
                world: 1,
                inventory: 2,
                life_clock: 1,
            },
            items: Vec::new(),
            materials: Vec::new(),
        }
    }

    async fn boss_exit_commit(
        directory: &CorePrivateRouteActorDirectory,
        handoff: &CorePrivateCaldusDefeatHandoff,
    ) -> CorePrivateCaldusRewardCommit {
        let route = directory
            .advance(
                handoff.route_lease(),
                CorePrivateRouteActorAdvance::BossExitReady,
            )
            .await
            .expect("BossExitReady");
        let identities = sim_core::CoreCaldusVictoryIdentities::derive(
            handoff.instance_lineage_id(),
            handoff.lock(),
        )
        .expect("Caldus identities");
        CorePrivateCaldusRewardCommit {
            route,
            exit: CaldusExitPresentation {
                exit_instance_id: identities.exit_instance_id.bytes(),
                content_id: "portal.exit.dungeon.bell_sepulcher".to_owned(),
                asset_id: "asset.portal.exit.bell".to_owned(),
                display_name: "Return to Lantern Halls".to_owned(),
                description: "Secure pending custody before Hall return.".to_owned(),
                tags: vec!["portal".to_owned()],
                point: MilliTilePoint { x: 2_500, y: 9_000 },
                destination_content_id: "hub.lantern_halls_01".to_owned(),
                arrival: CoreCaldusSafeArrival::HallDefault,
                requires_committed_extraction_receipt: true,
            },
            disposition: CorePrivateCaldusRewardCommitDisposition::Committed,
        }
    }

    fn empty_report() -> CorePrivateCaldusRewardRuntimeReport {
        CorePrivateCaldusRewardRuntimeReport {
            resolution_attempts: 0,
            acknowledgements: 0,
            snapshot_attempts: 0,
            extraction_reservations: 0,
            extraction_activations: 0,
            extraction_abort_failures: 0,
            publication_attempts: 0,
            publication_generations: 0,
            faulted: false,
            task_joined: true,
        }
    }

    struct OrderedInventoryAuthority {
        directory: CorePrivateRouteActorDirectory,
        handoff: CorePrivateCaldusDefeatHandoff,
        order: Arc<StdMutex<Vec<&'static str>>>,
    }

    impl CoreCaldusPendingInventoryAuthority for OrderedInventoryAuthority {
        fn load(
            &self,
            _authenticated: AuthenticatedAccount,
            _handoff: CorePrivateCaldusDefeatHandoff,
            _world_flow_revision: WorldFlowContentRevisionV1,
        ) -> RuntimeFuture<
            '_,
            Result<CoreCaldusPendingInventorySnapshot, CoreCaldusRewardAuthorityFailure>,
        > {
            Box::pin(async move {
                assert_eq!(
                    self.directory
                        .snapshot(self.handoff.route_lease())
                        .expect("reserved route")
                        .phase,
                    CorePrivateRoutePhaseV1::TerminalPending
                );
                self.order.lock().unwrap().push("snapshot");
                CoreCaldusPendingInventorySnapshot::new(
                    pending_inventory(&self.handoff),
                    ProductionExtractionExpectedVersionsV1 {
                        account: 1,
                        character: 1,
                        world: 1,
                        inventory: 2,
                        life_metrics: 1,
                    },
                )
            })
        }
    }

    struct RetainingActivator {
        order: Arc<StdMutex<Vec<&'static str>>>,
        reservation: StdMutex<Option<ProductionExtractionCaldusReservationV1>>,
    }

    impl CoreCaldusExtractionActivator for RetainingActivator {
        fn activate(
            &self,
            reservation: ProductionExtractionCaldusReservationV1,
            expected_versions: ProductionExtractionExpectedVersionsV1,
        ) -> RuntimeFuture<'_, Result<(), CoreCaldusRewardAuthorityFailure>> {
            Box::pin(async move {
                assert_eq!(
                    expected_versions,
                    ProductionExtractionExpectedVersionsV1 {
                        account: 1,
                        character: 1,
                        world: 1,
                        inventory: 2,
                        life_metrics: 1,
                    }
                );
                self.order.lock().unwrap().push("activate");
                *self.reservation.lock().unwrap() = Some(reservation);
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn pending_snapshot_keeps_storage_versions_separate_and_exact() {
        let (_authenticated, handoff, directory) = fixture();
        let expected = ProductionExtractionExpectedVersionsV1 {
            account: 1,
            character: 1,
            world: 1,
            inventory: 2,
            life_metrics: 1,
        };
        let snapshot =
            CoreCaldusPendingInventorySnapshot::new(pending_inventory(&handoff), expected)
                .expect("coherent storage and wire authority");
        assert_eq!(snapshot.expected_versions(), &expected);
        assert_eq!(snapshot.projection().character_id, handoff.character_id());

        let mut changed = expected;
        changed.inventory = 3;
        assert!(
            CoreCaldusPendingInventorySnapshot::new(pending_inventory(&handoff), changed).is_err()
        );
        directory.begin_shutdown();
        assert!(directory.finish_shutdown().await.unwrap().zero_residue);
    }

    #[tokio::test]
    async fn automatic_post_acknowledgement_reserves_before_snapshot_then_activates() {
        let (authenticated, handoff, directory) = fixture();
        let commit = boss_exit_commit(&directory, &handoff).await;
        let order = Arc::new(StdMutex::new(Vec::new()));
        let inventory: Arc<dyn CoreCaldusPendingInventoryAuthority> =
            Arc::new(OrderedInventoryAuthority {
                directory: directory.clone(),
                handoff: handoff.clone(),
                order: Arc::clone(&order),
            });
        let retaining = Arc::new(RetainingActivator {
            order: Arc::clone(&order),
            reservation: StdMutex::new(None),
        });
        let activator: Arc<dyn CoreCaldusExtractionActivator> = retaining.clone();
        let (state_tx, _state_rx) = watch::channel(CorePrivateCaldusRewardRuntimeState::Watching);
        let (_shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let mut report = empty_report();
        let revision = pending_inventory(&handoff).content_revision;

        let outcome = prepare_post_acknowledgement(
            authenticated,
            sim_core::CoreCaldusVictoryIdentities::derive(
                handoff.instance_lineage_id(),
                handoff.lock(),
            )
            .unwrap()
            .encounter_id
            .bytes(),
            &handoff,
            &commit,
            Some(&inventory),
            Some(&revision),
            Some(&directory),
            Some(&activator),
            &state_tx,
            &mut shutdown_rx,
            &mut report,
        )
        .await
        .expect("automatic post-acknowledgement path");

        assert!(matches!(
            outcome,
            PostAcknowledgementOutcome::Publication(_)
        ));
        assert_eq!(*order.lock().unwrap(), vec!["snapshot", "activate"]);
        assert_eq!(report.snapshot_attempts, 1);
        assert_eq!(report.extraction_reservations, 1);
        assert_eq!(report.extraction_activations, 1);
        assert_eq!(
            directory.snapshot(handoff.route_lease()).unwrap().phase,
            CorePrivateRoutePhaseV1::TerminalPending
        );

        let reservation = retaining
            .reservation
            .lock()
            .unwrap()
            .take()
            .expect("activator retained exact reservation");
        reservation.abort().await.expect("test cleanup");
        directory.begin_shutdown();
        assert!(directory.finish_shutdown().await.unwrap().zero_residue);
    }

    #[tokio::test]
    async fn shutdown_during_failed_reservation_abort_is_bounded_and_faulted() {
        let (authenticated, handoff, directory) = fixture();
        let commit = boss_exit_commit(&directory, &handoff).await;
        let reservation = ProductionExtractionCaldusReservationV1::reserve(
            authenticated,
            directory.clone(),
            &handoff,
            &commit,
            pending_inventory(&handoff).content_revision,
        )
        .await
        .expect("fresh reservation");
        directory.begin_shutdown();
        let (_shutdown_tx, mut shutdown_rx) = watch::channel(true);
        let mut report = empty_report();
        report.extraction_reservations = 1;

        let failure = tokio::time::timeout(
            Duration::from_millis(100),
            abort_reservation(Some(&reservation), &mut report, &mut shutdown_rx),
        )
        .await
        .expect("shutdown cleanup must not hang")
        .expect_err("retired route cannot accept exact abort");
        assert!(failure.contains("reservation abort failed"));
        assert_eq!(report.extraction_abort_failures, 1);
        assert!(directory.finish_shutdown().await.unwrap().zero_residue);
    }

    #[test]
    fn exact_terminal_completion_releases_retained_publication_unbind_gate() {
        let (writer_tx, _writer_rx) = watch::channel(None::<WriterBinding>);
        let (_state_tx, state_rx) =
            watch::channel(CorePrivateCaldusRewardRuntimeState::Published {
                encounter_id: [0xA1; 16],
                generation: CoreCaldusRewardWriterGeneration::new(1).unwrap(),
            });
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let runtime = CorePrivateCaldusRewardRuntime {
            writer_tx,
            state_rx,
            shutdown_tx,
            terminal_completed: AtomicBool::new(false),
            join: Mutex::new(None),
        };

        assert!(runtime.blocks_danger_unbind());
        runtime.acknowledge_terminal_complete();
        assert!(!runtime.blocks_danger_unbind());
        assert!(*shutdown_rx.borrow());
    }

    async fn live_connection_pair() -> (
        quinn::Endpoint,
        quinn::Endpoint,
        quinn::Connection,
        quinn::Connection,
    ) {
        let rcgen::CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(vec!["localhost".to_owned()]).expect("certificate");
        let certificate = cert.der().clone();
        let private_key = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
        let server_config =
            quinn::ServerConfig::with_single_cert(vec![certificate.clone()], private_key.into())
                .expect("server TLS");
        let server_endpoint =
            quinn::Endpoint::server(server_config, "127.0.0.1:0".parse().unwrap())
                .expect("server endpoint");
        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate).expect("root certificate");
        let client_config =
            quinn::ClientConfig::with_root_certificates(Arc::new(roots)).expect("client TLS");
        let mut client_endpoint =
            quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).expect("client endpoint");
        client_endpoint.set_default_client_config(client_config);
        let connecting = client_endpoint
            .connect(server_endpoint.local_addr().unwrap(), "localhost")
            .expect("connect");
        let incoming = server_endpoint.accept().await.expect("incoming");
        let (client, server) = tokio::join!(connecting, incoming);
        (
            server_endpoint,
            client_endpoint,
            client.expect("client connection"),
            server.expect("server connection"),
        )
    }

    async fn receive_reliable_event(client: &quinn::Connection) -> protocol::ReliableEventFrame {
        let mut receive = tokio::time::timeout(Duration::from_secs(5), client.accept_uni())
            .await
            .expect("reliable push timeout")
            .expect("reliable push stream");
        let bytes = receive.read_to_end(1_048_576).await.expect("push bytes");
        match crate::decode_frame(&bytes).expect("reliable frame") {
            protocol::WireMessage::ReliableEvent(frame) => frame,
            message => panic!("unexpected server push: {message:?}"),
        }
    }

    #[tokio::test]
    async fn pending_inventory_precedes_and_replays_with_boss_exit_ready() {
        let (_authenticated, handoff, directory) = fixture();
        let route = directory
            .advance(
                handoff.route_lease(),
                CorePrivateRouteActorAdvance::BossExitReady,
            )
            .await
            .expect("BossExitReady");
        let commit = CorePrivateCaldusRewardCommit {
            route,
            exit: CaldusExitPresentation {
                exit_instance_id: [0x91; 16],
                content_id: "portal.exit.dungeon.bell_sepulcher".to_owned(),
                asset_id: "asset.portal.exit.bell".to_owned(),
                display_name: "Return to Lantern Halls".to_owned(),
                description: "Secure pending custody before Hall return.".to_owned(),
                tags: vec!["portal".to_owned()],
                point: MilliTilePoint { x: 2_500, y: 9_000 },
                destination_content_id: "hub.lantern_halls_01".to_owned(),
                arrival: CoreCaldusSafeArrival::HallDefault,
                requires_committed_extraction_receipt: true,
            },
            disposition: CorePrivateCaldusRewardCommitDisposition::Committed,
        };
        let encounter_id = [0x92; 16];
        let retained = retained_publication(
            encounter_id,
            &handoff,
            &commit.route,
            pending_inventory(&handoff),
        )
        .expect("retained publication");

        let (server_endpoint_1, client_endpoint_1, client_1, server_1) =
            live_connection_pair().await;
        let writer_1 = CoreReliableWriter::new(server_1);
        publish(&writer_1, &retained)
            .await
            .expect("first publication");
        let first_pending = receive_reliable_event(&client_1).await;
        let first_route = receive_reliable_event(&client_1).await;
        assert_eq!(first_pending.sequence, 1);
        assert_eq!(first_route.sequence, 2);
        assert!(matches!(
            &first_pending.event,
            ReliableEvent::CorePendingInventoryState(_)
        ));
        assert!(matches!(
            &first_route.event,
            ReliableEvent::CorePrivateRouteState(_)
        ));

        let (server_endpoint_2, client_endpoint_2, client_2, server_2) =
            live_connection_pair().await;
        let writer_2 = CoreReliableWriter::new(server_2);
        publish(&writer_2, &retained)
            .await
            .expect("reconnect publication");
        let second_pending = receive_reliable_event(&client_2).await;
        let second_route = receive_reliable_event(&client_2).await;
        assert_eq!(second_pending.event, first_pending.event);
        assert_eq!(second_route.event, first_route.event);
        assert_eq!(second_pending.server_tick, handoff.defeat_tick().0);
        assert_eq!(second_route.server_tick, handoff.defeat_tick().0);

        client_1.close(0_u32.into(), b"test complete");
        client_2.close(0_u32.into(), b"test complete");
        drop(writer_1);
        drop(writer_2);
        server_endpoint_1.wait_idle().await;
        client_endpoint_1.wait_idle().await;
        server_endpoint_2.wait_idle().await;
        client_endpoint_2.wait_idle().await;
        directory.begin_shutdown();
        assert!(
            directory
                .finish_shutdown()
                .await
                .expect("shutdown")
                .zero_residue
        );
    }

    #[tokio::test]
    async fn fatal_failure_attempts_the_exact_frozen_handoff_once() {
        let (authenticated, handoff, _directory) = fixture();
        let authority = RejectingAuthority {
            kind: CoreCaldusRewardAuthorityFailureKind::Fatal,
            calls: AtomicU32::new(0),
            seen: StdMutex::new(Vec::new()),
        };
        let (state_tx, _state_rx) = watch::channel(CorePrivateCaldusRewardRuntimeState::Watching);
        let (_shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let mut report = CorePrivateCaldusRewardRuntimeReport {
            resolution_attempts: 0,
            acknowledgements: 0,
            snapshot_attempts: 0,
            extraction_reservations: 0,
            extraction_activations: 0,
            extraction_abort_failures: 0,
            publication_attempts: 0,
            publication_generations: 0,
            faulted: false,
            task_joined: true,
        };

        let failure = resolve_with_backoff(
            authenticated,
            hash('7'),
            handoff.clone(),
            &authority,
            &state_tx,
            &mut shutdown_rx,
            &mut report,
        )
        .await
        .expect_err("fatal failure");

        assert_eq!(failure.kind, CoreCaldusRewardAuthorityFailureKind::Fatal);
        assert_eq!(authority.calls.load(Ordering::SeqCst), 1);
        assert_eq!(report.resolution_attempts, 1);
        assert_eq!(
            *authority.seen.lock().expect("seen handoffs"),
            vec![handoff]
        );
    }

    #[tokio::test]
    async fn retryable_failure_preserves_the_attempt_until_shutdown() {
        let (authenticated, handoff, _directory) = fixture();
        let authority = RejectingAuthority {
            kind: CoreCaldusRewardAuthorityFailureKind::Retryable,
            calls: AtomicU32::new(0),
            seen: StdMutex::new(Vec::new()),
        };
        let (state_tx, _state_rx) = watch::channel(CorePrivateCaldusRewardRuntimeState::Watching);
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let mut report = CorePrivateCaldusRewardRuntimeReport {
            resolution_attempts: 0,
            acknowledgements: 0,
            snapshot_attempts: 0,
            extraction_reservations: 0,
            extraction_activations: 0,
            extraction_abort_failures: 0,
            publication_attempts: 0,
            publication_generations: 0,
            faulted: false,
            task_joined: true,
        };
        let stop = tokio::spawn(async move {
            tokio::task::yield_now().await;
            shutdown_tx.send_replace(true);
        });

        let result = resolve_with_backoff(
            authenticated,
            hash('7'),
            handoff.clone(),
            &authority,
            &state_tx,
            &mut shutdown_rx,
            &mut report,
        )
        .await
        .expect("retry loop shutdown");
        stop.await.expect("shutdown trigger");

        assert!(result.is_none());
        assert_eq!(authority.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            *authority.seen.lock().expect("seen handoffs"),
            vec![handoff]
        );
    }
}
