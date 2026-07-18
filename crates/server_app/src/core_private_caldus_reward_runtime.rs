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

use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use persistence::{
    PersistenceError, PostgresPersistence, StoredActiveDangerAuthorityV1, StoredWorldFlowRevisionV1,
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
    CorePrivateMicrorealmDriverObserver, CorePrivateMicrorealmDriverState, CoreReliableWriter,
    CoreReliableWriterError, project_core_pending_inventory,
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
    ) -> RuntimeFuture<'_, Result<CorePendingInventoryStateV1, CoreCaldusRewardAuthorityFailure>>;
}

impl CoreCaldusPendingInventoryAuthority for PostgresPersistence {
    fn load(
        &self,
        authenticated: AuthenticatedAccount,
        handoff: CorePrivateCaldusDefeatHandoff,
        world_flow_revision: WorldFlowContentRevisionV1,
    ) -> RuntimeFuture<'_, Result<CorePendingInventoryStateV1, CoreCaldusRewardAuthorityFailure>>
    {
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
            project_core_pending_inventory(&snapshot)
                .map_err(|error| fatal_failure(error.to_string()))
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

pub(crate) struct CorePrivateCaldusRewardRuntimeConfig {
    authenticated: AuthenticatedAccount,
    progression_content_revision: ManifestHash,
    authority: Arc<dyn CoreCaldusRewardAuthority>,
    pending_inventory: Option<Arc<dyn CoreCaldusPendingInventoryAuthority>>,
    world_flow_revision: Option<WorldFlowContentRevisionV1>,
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
    pub publication_attempts: u32,
    pub publication_generations: u32,
    pub task_joined: bool,
}

#[derive(Debug, Error)]
pub enum CorePrivateCaldusRewardRuntimeError {
    #[error("Caldus reward writer generation must be nonzero")]
    InvalidWriterGeneration,
    #[error("Caldus reward runtime task failed")]
    Join(#[source] JoinError),
}

pub struct CorePrivateCaldusRewardRuntime {
    writer_tx: watch::Sender<Option<WriterBinding>>,
    state_rx: watch::Receiver<CorePrivateCaldusRewardRuntimeState>,
    shutdown_tx: watch::Sender<bool>,
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
            join: Mutex::new(Some(join)),
        }
    }

    #[must_use]
    pub fn observe(&self) -> watch::Receiver<CorePrivateCaldusRewardRuntimeState> {
        self.state_rx.clone()
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
            Some(join) => join
                .await
                .map_err(CorePrivateCaldusRewardRuntimeError::Join),
            None => Ok(CorePrivateCaldusRewardRuntimeReport {
                resolution_attempts: 0,
                acknowledgements: 0,
                snapshot_attempts: 0,
                publication_attempts: 0,
                publication_generations: 0,
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
    } = config;
    let mut report = CorePrivateCaldusRewardRuntimeReport {
        resolution_attempts: 0,
        acknowledgements: 0,
        snapshot_attempts: 0,
        publication_attempts: 0,
        publication_generations: 0,
        task_joined: true,
    };
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
                    if let (Some(inventory), Some(world_flow_revision)) =
                        (&pending_inventory, &world_flow_revision)
                    {
                        let projected = match load_snapshot_with_backoff(
                            authenticated,
                            handoff.clone(),
                            world_flow_revision.clone(),
                            inventory.as_ref(),
                            &state_tx,
                            &mut shutdown_rx,
                            &mut report,
                        )
                        .await
                        {
                            Ok(Some(projected)) => projected,
                            Ok(None) => break,
                            Err(failure) => {
                                state_tx.send_replace(
                                    CorePrivateCaldusRewardRuntimeState::Faulted {
                                        message: failure.message,
                                    },
                                );
                                break;
                            }
                        };
                        match retained_publication(encounter_id, &handoff, &commit, projected) {
                            Ok(retained) => {
                                publication = Some(retained);
                                attempted_generation = None;
                                state_tx.send_replace(
                                    CorePrivateCaldusRewardRuntimeState::AwaitingWriter {
                                        encounter_id,
                                    },
                                );
                            }
                            Err(message) => {
                                state_tx.send_replace(
                                    CorePrivateCaldusRewardRuntimeState::Faulted { message },
                                );
                                break;
                            }
                        }
                    } else if pending_inventory.is_none() && world_flow_revision.is_none() {
                        state_tx.send_replace(CorePrivateCaldusRewardRuntimeState::Acknowledged {
                            encounter_id,
                            exit_instance_id,
                        });
                    } else {
                        state_tx.send_replace(CorePrivateCaldusRewardRuntimeState::Faulted {
                            message: Arc::from(
                                "Caldus pending-inventory authority is only partially configured",
                            ),
                        });
                        break;
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
    state_tx.send_replace(CorePrivateCaldusRewardRuntimeState::Shutdown);
    report
}

async fn load_snapshot_with_backoff(
    authenticated: AuthenticatedAccount,
    handoff: CorePrivateCaldusDefeatHandoff,
    world_flow_revision: WorldFlowContentRevisionV1,
    authority: &dyn CoreCaldusPendingInventoryAuthority,
    state_tx: &watch::Sender<CorePrivateCaldusRewardRuntimeState>,
    shutdown_rx: &mut watch::Receiver<bool>,
    report: &mut CorePrivateCaldusRewardRuntimeReport,
) -> Result<Option<CorePendingInventoryStateV1>, CoreCaldusRewardAuthorityFailure> {
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
    commit: &CorePrivateCaldusRewardCommit,
    pending_inventory: CorePendingInventoryStateV1,
) -> Result<RetainedPublication, Arc<str>> {
    pending_inventory
        .validate()
        .map_err(|error| Arc::from(error.to_string()))?;
    commit
        .route
        .validate()
        .map_err(|error| Arc::from(error.to_string()))?;
    if commit.route.phase != CorePrivateRoutePhaseV1::BossExitReady
        || commit.route.character_id != handoff.character_id()
        || commit.route.actor_generation != handoff.route_lease().actor_generation()
        || commit.route.instance_lineage_id != Some(handoff.instance_lineage_id())
        || pending_inventory.character_id != handoff.character_id()
        || pending_inventory.instance_lineage_id != handoff.instance_lineage_id()
        || pending_inventory.entry_restore_point_id != handoff.entry_restore_point_id()
        || pending_inventory.expected_extraction_versions.character
            != commit.route.character_version
    {
        return Err(Arc::from(
            "Caldus pending inventory does not match acknowledged BossExitReady authority",
        ));
    }
    Ok(RetainedPublication {
        encounter_id,
        server_tick: handoff.defeat_tick().0,
        pending_inventory: ReliableEvent::CorePendingInventoryState(Box::new(pending_inventory)),
        route: ReliableEvent::CorePrivateRouteState(Box::new(commit.route.clone())),
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
        let retained =
            retained_publication(encounter_id, &handoff, &commit, pending_inventory(&handoff))
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
            publication_attempts: 0,
            publication_generations: 0,
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
            publication_attempts: 0,
            publication_generations: 0,
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
