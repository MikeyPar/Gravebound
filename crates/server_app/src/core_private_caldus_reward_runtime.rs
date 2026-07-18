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

use protocol::{CorePrivateRoutePhaseV1, ManifestHash};
use thiserror::Error;
use tokio::{
    sync::{Mutex, watch},
    task::{JoinError, JoinHandle},
};

use crate::{
    AuthenticatedAccount, CoreCaldusRewardAuthority, CoreCaldusRewardAuthorityFailure,
    CoreCaldusRewardAuthorityFailureKind, CoreDurableCaldusResolution,
    CorePrivateCaldusDefeatHandoff, CorePrivateCaldusRewardCommit,
    CorePrivateMicrorealmDriverError, CorePrivateMicrorealmDriverHandle,
    CorePrivateMicrorealmDriverObserver, CorePrivateMicrorealmDriverState,
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
    Acknowledged {
        encounter_id: [u8; 16],
        exit_instance_id: [u8; 16],
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
    pub task_joined: bool,
}

#[derive(Debug, Error)]
pub enum CorePrivateCaldusRewardRuntimeError {
    #[error("Caldus reward runtime task failed")]
    Join(#[source] JoinError),
}

pub struct CorePrivateCaldusRewardRuntime {
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
        authenticated: AuthenticatedAccount,
        progression_content_revision: ManifestHash,
        authority: Arc<dyn CoreCaldusRewardAuthority>,
        driver: CorePrivateMicrorealmDriverHandle,
        observer: CorePrivateMicrorealmDriverObserver,
    ) -> Self {
        Self::spawn_with_sink(
            authenticated,
            progression_content_revision,
            authority,
            Arc::new(driver),
            observer,
        )
    }

    fn spawn_with_sink(
        authenticated: AuthenticatedAccount,
        progression_content_revision: ManifestHash,
        authority: Arc<dyn CoreCaldusRewardAuthority>,
        sink: Arc<dyn CoreCaldusResolutionSink>,
        observer: CorePrivateMicrorealmDriverObserver,
    ) -> Self {
        let (state_tx, state_rx) = watch::channel(CorePrivateCaldusRewardRuntimeState::Watching);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let join = tokio::spawn(run_runtime(
            authenticated,
            progression_content_revision,
            authority,
            sink,
            observer,
            state_tx,
            shutdown_rx,
        ));
        Self {
            state_rx,
            shutdown_tx,
            join: Mutex::new(Some(join)),
        }
    }

    #[must_use]
    pub fn observe(&self) -> watch::Receiver<CorePrivateCaldusRewardRuntimeState> {
        self.state_rx.clone()
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
                task_joined: true,
            }),
        }
    }
}

async fn run_runtime(
    authenticated: AuthenticatedAccount,
    progression_content_revision: ManifestHash,
    authority: Arc<dyn CoreCaldusRewardAuthority>,
    sink: Arc<dyn CoreCaldusResolutionSink>,
    mut observer: CorePrivateMicrorealmDriverObserver,
    state_tx: watch::Sender<CorePrivateCaldusRewardRuntimeState>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> CorePrivateCaldusRewardRuntimeReport {
    let mut report = CorePrivateCaldusRewardRuntimeReport {
        resolution_attempts: 0,
        acknowledgements: 0,
        task_joined: true,
    };
    let mut acknowledged = false;

    loop {
        if *shutdown_rx.borrow() {
            break;
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
            match sink.acknowledge(resolution).await {
                Ok(commit) if commit.route.phase == CorePrivateRoutePhaseV1::BossExitReady => {
                    report.acknowledgements = report.acknowledgements.saturating_add(1);
                    acknowledged = true;
                    state_tx.send_replace(CorePrivateCaldusRewardRuntimeState::Acknowledged {
                        encounter_id,
                        exit_instance_id,
                    });
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
        }
    }
    state_tx.send_replace(CorePrivateCaldusRewardRuntimeState::Shutdown);
    report
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

    use protocol::{
        CorePrivateRouteContentRevisionV1, CorePrivateRoutePhaseV1, CorePrivateRouteRoomV1,
        CorePrivateRouteSceneV1, WorldFlowContentRevisionV1,
    };
    use sim_core::{
        CoreBossParticipant, CoreBossParticipantLock, CoreCaldusAntiCheatState,
        CoreCaldusDefeatPresence, CoreCaldusEligibilityEvidence, CoreCaldusRecallState,
        CoreCaldusSessionState, EntityId, Tick,
    };

    use super::*;
    use crate::{
        AccountId, AuthenticatedNamespace, CorePrivateRouteActorDirectory,
        CorePrivateRouteActorPosition, CorePrivateRouteActorSeed,
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
