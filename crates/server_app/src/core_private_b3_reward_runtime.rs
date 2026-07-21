//! Transport-independent automatic B3 durable-resolution and publication owner.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`PROG-003`, `SOC-010`,
//! `TECH-015`, `TECH-021`-`023`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-014`, `CONT-REWARD-003`-`004`, `CONT-ROOM-007`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`-`05`).
//!
//! The worker follows the route binding rather than a QUIC transport. It can therefore finish a
//! durable resolution while `LinkLost`, acknowledge only through the existing single-writer driver
//! task, and retain publication for a later transport generation. It never invents a Bargain
//! request, item event, reward destination, or client sequence.

use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use protocol::{ProgressionResult, ReliableEvent};
use thiserror::Error;
use tokio::{
    sync::{Mutex, watch},
    task::{JoinError, JoinHandle},
};

use crate::{
    AuthenticatedAccount, CoreB3RewardCoordinatorError, CoreDurableB3Resolution,
    CorePrivateFixedDungeonB3RewardCommit, CorePrivateMicrorealmDriverError,
    CorePrivateMicrorealmDriverHandle, CorePrivateMicrorealmDriverObserver,
    CorePrivateMicrorealmDriverState, CoreReliableWriter, CoreReliableWriterError,
    PostgresCoreB3RewardCoordinator, ProgressionAwardCode,
};

const INITIAL_RETRY_BACKOFF: Duration = Duration::from_millis(25);
const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(1);

type RuntimeFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreB3RewardAuthorityFailureKind {
    Retryable,
    Fatal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreB3RewardAuthorityFailure {
    pub kind: CoreB3RewardAuthorityFailureKind,
    pub message: Arc<str>,
}

pub trait CoreB3RewardAuthority: Send + Sync {
    fn resolve(
        &self,
        authenticated: AuthenticatedAccount,
        character_id: [u8; 16],
        instance_lineage_id: [u8; 16],
        current_tick: u64,
        handoff: sim_content::CoreB3RewardHandoff,
    ) -> RuntimeFuture<'_, Result<CoreDurableB3Resolution, CoreB3RewardAuthorityFailure>>;
}

impl CoreB3RewardAuthority for PostgresCoreB3RewardCoordinator {
    fn resolve(
        &self,
        authenticated: AuthenticatedAccount,
        character_id: [u8; 16],
        instance_lineage_id: [u8; 16],
        current_tick: u64,
        handoff: sim_content::CoreB3RewardHandoff,
    ) -> RuntimeFuture<'_, Result<CoreDurableB3Resolution, CoreB3RewardAuthorityFailure>> {
        Box::pin(async move {
            self.commit(
                authenticated,
                character_id,
                instance_lineage_id,
                current_tick,
                &handoff,
            )
            .await
            .map_err(|error| classify_authority_failure(&error))
        })
    }
}

trait CoreB3ResolutionSink: Send + Sync {
    fn acknowledge(
        &self,
        resolution: CoreDurableB3Resolution,
    ) -> RuntimeFuture<
        '_,
        Result<CorePrivateFixedDungeonB3RewardCommit, CorePrivateMicrorealmDriverError>,
    >;
}

impl CoreB3ResolutionSink for CorePrivateMicrorealmDriverHandle {
    fn acknowledge(
        &self,
        resolution: CoreDurableB3Resolution,
    ) -> RuntimeFuture<
        '_,
        Result<CorePrivateFixedDungeonB3RewardCommit, CorePrivateMicrorealmDriverError>,
    > {
        Box::pin(async move { self.commit_fixed_dungeon_b3_reward(resolution).await })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CoreB3RewardWriterGeneration(u64);

impl CoreB3RewardWriterGeneration {
    pub fn new(value: u64) -> Result<Self, CorePrivateB3RewardRuntimeError> {
        if value == 0 {
            Err(CorePrivateB3RewardRuntimeError::InvalidWriterGeneration)
        } else {
            Ok(Self(value))
        }
    }
}

#[derive(Debug, Clone)]
struct WriterBinding {
    generation: CoreB3RewardWriterGeneration,
    writer: Arc<CoreReliableWriter>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorePrivateB3RewardRuntimeState {
    Watching,
    Resolving {
        attempts: u32,
    },
    AwaitingWriter {
        reward_event_id: [u8; 16],
    },
    Published {
        reward_event_id: [u8; 16],
        generation: CoreB3RewardWriterGeneration,
    },
    Faulted {
        message: Arc<str>,
    },
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateB3RewardRuntimeReport {
    pub resolution_attempts: u32,
    pub acknowledgements: u32,
    pub publication_attempts: u32,
    pub publication_generations: u32,
    pub task_joined: bool,
}

#[derive(Debug, Error)]
pub enum CorePrivateB3RewardRuntimeError {
    #[error("B3 reward writer generation must be nonzero")]
    InvalidWriterGeneration,
    #[error("B3 reward runtime task failed")]
    Join(#[source] JoinError),
}

#[derive(Debug, Clone)]
struct RetainedPublication {
    reward_event_id: [u8; 16],
    server_tick: u64,
    progression: Option<ReliableEvent>,
    route: ReliableEvent,
}

pub struct CorePrivateB3RewardRuntime {
    writer_tx: watch::Sender<Option<WriterBinding>>,
    state_rx: watch::Receiver<CorePrivateB3RewardRuntimeState>,
    shutdown_tx: watch::Sender<bool>,
    join: Mutex<Option<JoinHandle<CorePrivateB3RewardRuntimeReport>>>,
}

impl std::fmt::Debug for CorePrivateB3RewardRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CorePrivateB3RewardRuntime")
            .field("state", &*self.state_rx.borrow())
            .finish_non_exhaustive()
    }
}

impl CorePrivateB3RewardRuntime {
    pub(crate) fn spawn(
        authenticated: AuthenticatedAccount,
        character_id: [u8; 16],
        authority: Arc<dyn CoreB3RewardAuthority>,
        driver: CorePrivateMicrorealmDriverHandle,
        observer: CorePrivateMicrorealmDriverObserver,
        initial_writer: Option<(CoreB3RewardWriterGeneration, Arc<CoreReliableWriter>)>,
    ) -> Self {
        let sink: Arc<dyn CoreB3ResolutionSink> = Arc::new(driver);
        Self::spawn_with_sink(
            authenticated,
            character_id,
            authority,
            sink,
            observer,
            initial_writer,
        )
    }

    fn spawn_with_sink(
        authenticated: AuthenticatedAccount,
        character_id: [u8; 16],
        authority: Arc<dyn CoreB3RewardAuthority>,
        sink: Arc<dyn CoreB3ResolutionSink>,
        observer: CorePrivateMicrorealmDriverObserver,
        initial_writer: Option<(CoreB3RewardWriterGeneration, Arc<CoreReliableWriter>)>,
    ) -> Self {
        let initial_writer =
            initial_writer.map(|(generation, writer)| WriterBinding { generation, writer });
        let (writer_tx, writer_rx) = watch::channel(initial_writer);
        let (state_tx, state_rx) = watch::channel(CorePrivateB3RewardRuntimeState::Watching);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let join = tokio::spawn(run_runtime(
            authenticated,
            character_id,
            authority,
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
    pub fn observe(&self) -> watch::Receiver<CorePrivateB3RewardRuntimeState> {
        self.state_rx.clone()
    }

    pub fn attach_writer(
        &self,
        generation: CoreB3RewardWriterGeneration,
        writer: Arc<CoreReliableWriter>,
    ) {
        self.writer_tx
            .send_replace(Some(WriterBinding { generation, writer }));
    }

    pub fn detach_writer(&self, generation: CoreB3RewardWriterGeneration) {
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
    ) -> Result<CorePrivateB3RewardRuntimeReport, CorePrivateB3RewardRuntimeError> {
        self.shutdown_tx.send_replace(true);
        let join = self.join.lock().await.take();
        match join {
            Some(join) => join.await.map_err(CorePrivateB3RewardRuntimeError::Join),
            None => Ok(CorePrivateB3RewardRuntimeReport {
                resolution_attempts: 0,
                acknowledgements: 0,
                publication_attempts: 0,
                publication_generations: 0,
                task_joined: true,
            }),
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the task entry lists every independently owned authority and channel"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one select loop keeps resolution, acknowledgement, and retained publication ordering auditable"
)]
async fn run_runtime(
    authenticated: AuthenticatedAccount,
    character_id: [u8; 16],
    authority: Arc<dyn CoreB3RewardAuthority>,
    sink: Arc<dyn CoreB3ResolutionSink>,
    mut observer: CorePrivateMicrorealmDriverObserver,
    mut writer_rx: watch::Receiver<Option<WriterBinding>>,
    state_tx: watch::Sender<CorePrivateB3RewardRuntimeState>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> CorePrivateB3RewardRuntimeReport {
    let mut report = CorePrivateB3RewardRuntimeReport {
        resolution_attempts: 0,
        acknowledgements: 0,
        publication_attempts: 0,
        publication_generations: 0,
        task_joined: true,
    };
    let mut publication: Option<RetainedPublication> = None;
    let mut attempted_generation: Option<CoreB3RewardWriterGeneration> = None;

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
                state_tx.send_replace(CorePrivateB3RewardRuntimeState::Published {
                    reward_event_id: retained.reward_event_id,
                    generation: binding.generation,
                });
            }
            continue;
        }

        let pending = match observer.latest() {
            CorePrivateMicrorealmDriverState::FixedDungeonRewardPending {
                frame,
                reward_handoff,
                ..
            } if publication.is_none() => Some((frame, reward_handoff)),
            _ => None,
        };
        if let Some((frame, handoff)) = pending {
            let Some(instance_lineage_id) = frame.route.instance_lineage_id else {
                state_tx.send_replace(CorePrivateB3RewardRuntimeState::Faulted {
                    message: Arc::from("B3 pending frame has no instance lineage"),
                });
                break;
            };
            let resolution = match resolve_with_backoff(
                authenticated,
                character_id,
                instance_lineage_id,
                frame.tick.0,
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
                    state_tx.send_replace(CorePrivateB3RewardRuntimeState::Faulted {
                        message: failure.message,
                    });
                    break;
                }
            };
            let committed = match sink.acknowledge(resolution.clone()).await {
                Ok(committed) => committed,
                Err(error) => {
                    state_tx.send_replace(CorePrivateB3RewardRuntimeState::Faulted {
                        message: Arc::from(error.to_string()),
                    });
                    break;
                }
            };
            report.acknowledgements = report.acknowledgements.saturating_add(1);
            match retained_publication(&resolution, &committed, frame.tick.0) {
                Ok(retained) => {
                    state_tx.send_replace(CorePrivateB3RewardRuntimeState::AwaitingWriter {
                        reward_event_id: retained.reward_event_id,
                    });
                    publication = Some(retained);
                    attempted_generation = None;
                }
                Err(message) => {
                    state_tx.send_replace(CorePrivateB3RewardRuntimeState::Faulted { message });
                    break;
                }
            }
            continue;
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
    state_tx.send_replace(CorePrivateB3RewardRuntimeState::Shutdown);
    report
}

#[expect(
    clippy::too_many_arguments,
    reason = "retry preserves the exact immutable B3 authority tuple"
)]
async fn resolve_with_backoff(
    authenticated: AuthenticatedAccount,
    character_id: [u8; 16],
    instance_lineage_id: [u8; 16],
    current_tick: u64,
    handoff: sim_content::CoreB3RewardHandoff,
    authority: &dyn CoreB3RewardAuthority,
    state_tx: &watch::Sender<CorePrivateB3RewardRuntimeState>,
    shutdown_rx: &mut watch::Receiver<bool>,
    report: &mut CorePrivateB3RewardRuntimeReport,
) -> Result<Option<CoreDurableB3Resolution>, CoreB3RewardAuthorityFailure> {
    let mut backoff = INITIAL_RETRY_BACKOFF;
    loop {
        report.resolution_attempts = report.resolution_attempts.saturating_add(1);
        state_tx.send_replace(CorePrivateB3RewardRuntimeState::Resolving {
            attempts: report.resolution_attempts,
        });
        match authority
            .resolve(
                authenticated,
                character_id,
                instance_lineage_id,
                current_tick,
                handoff.clone(),
            )
            .await
        {
            Ok(resolution) => return Ok(Some(resolution)),
            Err(failure) if failure.kind == CoreB3RewardAuthorityFailureKind::Retryable => {
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
    resolution: &CoreDurableB3Resolution,
    committed: &CorePrivateFixedDungeonB3RewardCommit,
    server_tick: u64,
) -> Result<RetainedPublication, Arc<str>> {
    if resolution.reward_event_id() != committed.reward_event_id {
        return Err(Arc::from("B3 acknowledgement changed reward identity"));
    }
    let progression = match resolution {
        CoreDurableB3Resolution::Granted(_) => {
            let outcome = resolution.progression();
            if outcome.code != ProgressionAwardCode::Accepted {
                return Err(Arc::from("granted B3 terminal lacks accepted progression"));
            }
            let projection = outcome
                .projection
                .clone()
                .ok_or_else(|| Arc::from("granted B3 terminal lacks progression projection"))?;
            Some(ReliableEvent::ProgressionResult(
                ProgressionResult::Changed {
                    reward_event_id: outcome.reward_event_id,
                    projection,
                    base_xp: outcome.base_xp,
                    first_clear_bonus_xp: outcome.first_clear_bonus_xp,
                    applied_xp: outcome.applied_xp,
                    discarded_at_core_cap: outcome.discarded_at_core_cap,
                    first_clear_awarded: outcome.first_clear_awarded,
                },
            ))
        }
        CoreDurableB3Resolution::Ineligible(_) => {
            if resolution.progression().code != ProgressionAwardCode::NotEligible {
                return Err(Arc::from(
                    "ineligible B3 terminal lacks NotEligible progression",
                ));
            }
            None
        }
    };
    Ok(RetainedPublication {
        reward_event_id: committed.reward_event_id,
        server_tick,
        progression,
        route: ReliableEvent::CorePrivateRouteState(Box::new(committed.route.clone())),
    })
}

async fn publish(
    writer: &CoreReliableWriter,
    retained: &RetainedPublication,
) -> Result<(), CoreReliableWriterError> {
    if let Some(progression) = &retained.progression {
        writer
            .send_event(retained.server_tick, progression.clone())
            .await?;
    }
    writer
        .send_route_event(retained.server_tick, retained.route.clone())
        .await?;
    Ok(())
}

fn classify_authority_failure(
    error: &CoreB3RewardCoordinatorError,
) -> CoreB3RewardAuthorityFailure {
    let kind = match error {
        CoreB3RewardCoordinatorError::Reward(crate::RewardGrantError::Persistence(
            persistence::PersistenceError::Database(_),
        ))
        | CoreB3RewardCoordinatorError::Persistence(persistence::PersistenceError::Database(_)) => {
            CoreB3RewardAuthorityFailureKind::Retryable
        }
        CoreB3RewardCoordinatorError::InvalidHandoff
        | CoreB3RewardCoordinatorError::ProgressionNotCommitted(_)
        | CoreB3RewardCoordinatorError::MilestoneAuthorityMismatch
        | CoreB3RewardCoordinatorError::IneligibleAuthorityMismatch
        | CoreB3RewardCoordinatorError::Reward(_)
        | CoreB3RewardCoordinatorError::Persistence(_) => CoreB3RewardAuthorityFailureKind::Fatal,
    };
    CoreB3RewardAuthorityFailure {
        kind,
        message: Arc::from(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use protocol::{
        CORE_PRIVATE_ROUTE_SCHEMA_VERSION, CorePrivateRouteContentRevisionV1,
        CorePrivateRoutePhaseV1, CorePrivateRouteReadinessV1, CorePrivateRouteRoomV1,
        CorePrivateRouteSceneV1, CorePrivateRouteStateV1, ManifestHash,
    };
    use rcgen::generate_simple_self_signed;
    use rustls::pki_types::PrivatePkcs8KeyDer;
    use sim_core::{
        CombatStep, EntityId, FixedRoomPhase, MovementStep, RewardLifeState, RewardRecallState,
        RewardTrustState, SimulationVector, SpawnInstanceId, Tick, TilePoint,
    };

    use super::*;
    use crate::{AccountId, AuthenticatedNamespace, CoreDurableB3IneligibleCommit};

    #[derive(Debug)]
    struct ScriptedAuthority {
        attempts: AtomicU32,
        retry_before: u32,
        resolution: CoreDurableB3Resolution,
    }

    impl CoreB3RewardAuthority for ScriptedAuthority {
        fn resolve(
            &self,
            _authenticated: AuthenticatedAccount,
            _character_id: [u8; 16],
            _instance_lineage_id: [u8; 16],
            _current_tick: u64,
            _handoff: sim_content::CoreB3RewardHandoff,
        ) -> RuntimeFuture<'_, Result<CoreDurableB3Resolution, CoreB3RewardAuthorityFailure>>
        {
            let attempt = self.attempts.fetch_add(1, Ordering::AcqRel) + 1;
            let result = if attempt <= self.retry_before {
                Err(CoreB3RewardAuthorityFailure {
                    kind: CoreB3RewardAuthorityFailureKind::Retryable,
                    message: Arc::from("scripted database outage"),
                })
            } else {
                Ok(self.resolution.clone())
            };
            Box::pin(async move { result })
        }
    }

    #[derive(Debug)]
    struct RecordingSink {
        acknowledgements: AtomicU32,
        commit: CorePrivateFixedDungeonB3RewardCommit,
    }

    impl CoreB3ResolutionSink for RecordingSink {
        fn acknowledge(
            &self,
            _resolution: CoreDurableB3Resolution,
        ) -> RuntimeFuture<
            '_,
            Result<CorePrivateFixedDungeonB3RewardCommit, CorePrivateMicrorealmDriverError>,
        > {
            self.acknowledgements.fetch_add(1, Ordering::AcqRel);
            let commit = self.commit.clone();
            Box::pin(async move { Ok(commit) })
        }
    }

    fn hash(byte: char) -> ManifestHash {
        ManifestHash::new(byte.to_string().repeat(64)).expect("valid hash")
    }

    fn authenticated() -> AuthenticatedAccount {
        AuthenticatedAccount {
            account_id: AccountId::new([0x11; 16]).expect("account"),
            namespace: AuthenticatedNamespace::WipeableTest,
        }
    }

    fn handoff() -> sim_content::CoreB3RewardHandoff {
        sim_content::CoreB3RewardHandoff {
            activation_ordinal: 1,
            instance_id: SpawnInstanceId {
                run_ordinal: 1,
                spawn_ordinal: 51,
            },
            actor_id: EntityId::new(100).expect("actor"),
            participant_id: EntityId::new(900).expect("participant"),
            death_tick: Tick(100),
            reward_due_tick: Tick(108),
            reward_profile_id: "reward.miniboss_t1".into(),
            xp_profile_id: "xp.miniboss_t1".into(),
            active_ticks: 100,
            present_ticks: 1,
            direct_damage: 1,
            reference_health: 1_600,
            longest_inactivity_ticks: 99,
            life_state: RewardLifeState::Living,
            recall_state: RewardRecallState::Eligible,
            trust_state: RewardTrustState::InvalidSession,
        }
    }

    fn route() -> CorePrivateRouteStateV1 {
        CorePrivateRouteStateV1 {
            schema_version: CORE_PRIVATE_ROUTE_SCHEMA_VERSION,
            character_id: [0x22; 16],
            character_version: 1,
            content_revision: CorePrivateRouteContentRevisionV1 {
                records_blake3: hash('a'),
                assets_blake3: hash('b'),
                localization_blake3: hash('c'),
            },
            actor_generation: 1,
            state_version: 9,
            instance_lineage_id: Some([0x33; 16]),
            scene: CorePrivateRouteSceneV1::BellSepulcher,
            room: Some(CorePrivateRouteRoomV1::BellKnightB3),
            phase: CorePrivateRoutePhaseV1::RoomQuiet,
            readiness: CorePrivateRouteReadinessV1::canonical(CorePrivateRoutePhaseV1::RoomQuiet),
        }
    }

    fn pending_state(
        reward_handoff: &sim_content::CoreB3RewardHandoff,
    ) -> CorePrivateMicrorealmDriverState {
        let tick = reward_handoff.reward_due_tick;
        CorePrivateMicrorealmDriverState::FixedDungeonRewardPending {
            committed_frames: 108,
            frame: Arc::new(crate::CorePrivateFixedDungeonLiveRoomFrame {
                input_sequence: 7,
                tick,
                player_position: TilePoint::new(8_000, 8_000),
                movement: MovementStep {
                    position: SimulationVector::new(8.0, 8.0),
                    velocity: SimulationVector::new(0.0, 0.0),
                    collided: false,
                },
                combat: CombatStep {
                    tick,
                    ..CombatStep::default()
                },
                observation: crate::core_private_gameplay_observation::core_private_gameplay_observation_test_fixture(
                    tick.0,
                    1,
                    9,
                    7,
                ),
                route: route(),
                step: sim_content::CoreFixedDungeonRoomStep::B3(sim_content::CoreB3FixedRoomStep {
                    tick,
                    phase_after: FixedRoomPhase::Quiet,
                    required_hostiles_remaining: 0,
                    lifecycle_events: Vec::new(),
                    combat: None,
                    reward_handoff: Some(reward_handoff.clone()),
                    reset_cleared_projectiles: Vec::new(),
                }),
                player_damage: Vec::new(),
                player_died: false,
            }),
            reward_handoff: Arc::new(reward_handoff.clone()),
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

    #[tokio::test(start_paused = true)]
    async fn retries_without_transport_then_acknowledges_once_and_retains_publication() {
        let handoff = handoff();
        let resolution =
            CoreDurableB3Resolution::Ineligible(CoreDurableB3IneligibleCommit::test_fixture(
                authenticated(),
                [0x22; 16],
                [0x33; 16],
                handoff.clone(),
            ));
        let reward_event_id = resolution.reward_event_id();
        let authority = Arc::new(ScriptedAuthority {
            attempts: AtomicU32::new(0),
            retry_before: 2,
            resolution,
        });
        let sink = Arc::new(RecordingSink {
            acknowledgements: AtomicU32::new(0),
            commit: CorePrivateFixedDungeonB3RewardCommit {
                route: route(),
                receipt: sim_content::CoreB3RewardReceipt::Committed,
                disposition: sim_content::CoreB3RewardDisposition::IneligibleNoOffer,
                reward_event_id,
                reward_result_hash: None,
                progression_payload_hash: [9; 32],
                bargain_offer_id: None,
                has_no_offer_resolution: true,
            },
        });
        let (observation_tx, observation_rx) =
            watch::channel(CorePrivateMicrorealmDriverState::Starting);
        let runtime = CorePrivateB3RewardRuntime::spawn_with_sink(
            authenticated(),
            [0x22; 16],
            authority.clone(),
            sink.clone(),
            CorePrivateMicrorealmDriverObserver::from_receiver_for_test(observation_rx),
            None,
        );
        let mut state = runtime.observe();
        observation_tx.send_replace(pending_state(&handoff));
        tokio::time::advance(Duration::from_secs(1)).await;
        for _ in 0..16 {
            if matches!(
                &*state.borrow(),
                CorePrivateB3RewardRuntimeState::AwaitingWriter { reward_event_id: id }
                    if *id == reward_event_id
            ) {
                break;
            }
            tokio::task::yield_now().await;
            let _ = state.changed().await;
        }
        assert!(matches!(
            &*state.borrow(),
            CorePrivateB3RewardRuntimeState::AwaitingWriter { reward_event_id: id }
                if *id == reward_event_id
        ));
        assert_eq!(authority.attempts.load(Ordering::Acquire), 3);
        assert_eq!(sink.acknowledgements.load(Ordering::Acquire), 1);

        observation_tx.send_replace(pending_state(&handoff));
        tokio::task::yield_now().await;
        assert_eq!(authority.attempts.load(Ordering::Acquire), 3);
        assert_eq!(sink.acknowledgements.load(Ordering::Acquire), 1);

        let report = runtime.shutdown().await.expect("joined reward runtime");
        assert_eq!(report.resolution_attempts, 3);
        assert_eq!(report.acknowledgements, 1);
        assert_eq!(report.publication_attempts, 0);
        assert_eq!(report.publication_generations, 0);
        assert!(report.task_joined);
    }

    #[tokio::test]
    async fn granted_progression_then_route_replay_contiguously_to_each_new_writer_generation() {
        let handoff = handoff();
        let resolution: CoreDurableB3Resolution = crate::CoreDurableB3RewardCommit::test_fixture(
            authenticated(),
            [0x22; 16],
            [0x33; 16],
            handoff.clone(),
        )
        .into();
        let reward_event_id = resolution.reward_event_id();
        let authority = Arc::new(ScriptedAuthority {
            attempts: AtomicU32::new(0),
            retry_before: 0,
            resolution,
        });
        let sink = Arc::new(RecordingSink {
            acknowledgements: AtomicU32::new(0),
            commit: CorePrivateFixedDungeonB3RewardCommit {
                route: route(),
                receipt: sim_content::CoreB3RewardReceipt::Committed,
                disposition: sim_content::CoreB3RewardDisposition::GrantedOffer,
                reward_event_id,
                reward_result_hash: Some([7; 32]),
                progression_payload_hash: [8; 32],
                bargain_offer_id: Some(reward_event_id),
                has_no_offer_resolution: false,
            },
        });
        let (observation_tx, observation_rx) =
            watch::channel(CorePrivateMicrorealmDriverState::Starting);
        let runtime = CorePrivateB3RewardRuntime::spawn_with_sink(
            authenticated(),
            [0x22; 16],
            authority.clone(),
            sink.clone(),
            CorePrivateMicrorealmDriverObserver::from_receiver_for_test(observation_rx),
            None,
        );
        let mut state = runtime.observe();
        observation_tx.send_replace(pending_state(&handoff));
        tokio::time::timeout(Duration::from_secs(5), async {
            while !matches!(
                &*state.borrow(),
                CorePrivateB3RewardRuntimeState::AwaitingWriter { .. }
            ) {
                state.changed().await.expect("runtime state");
            }
        })
        .await
        .expect("durable acknowledgement");

        let (server_endpoint_1, client_endpoint_1, client_1, server_1) =
            live_connection_pair().await;
        runtime.attach_writer(
            CoreB3RewardWriterGeneration::new(1).unwrap(),
            Arc::new(CoreReliableWriter::new(server_1)),
        );
        let first_progression = receive_reliable_event(&client_1).await;
        let first_route = receive_reliable_event(&client_1).await;
        assert_eq!(first_progression.sequence, 1);
        assert_eq!(first_route.sequence, 2);
        assert_eq!(first_progression.server_tick, handoff.reward_due_tick.0);
        assert_eq!(first_route.server_tick, handoff.reward_due_tick.0);
        assert!(matches!(
            &first_progression.event,
            ReliableEvent::ProgressionResult(ProgressionResult::Changed { .. })
        ));
        assert!(matches!(
            &first_route.event,
            ReliableEvent::CorePrivateRouteState(_)
        ));

        let (server_endpoint_2, client_endpoint_2, client_2, server_2) =
            live_connection_pair().await;
        runtime.attach_writer(
            CoreB3RewardWriterGeneration::new(2).unwrap(),
            Arc::new(CoreReliableWriter::new(server_2)),
        );
        runtime.detach_writer(CoreB3RewardWriterGeneration::new(1).unwrap());
        let second_progression = receive_reliable_event(&client_2).await;
        let second_route = receive_reliable_event(&client_2).await;
        assert_eq!(second_progression.sequence, 1);
        assert_eq!(second_route.sequence, 2);
        assert_eq!(
            second_progression.server_tick,
            first_progression.server_tick
        );
        assert_eq!(second_route.server_tick, first_route.server_tick);
        assert_eq!(second_progression.event, first_progression.event);
        assert_eq!(second_route.event, first_route.event);
        assert_eq!(authority.attempts.load(Ordering::Acquire), 1);
        assert_eq!(sink.acknowledgements.load(Ordering::Acquire), 1);

        let report = runtime.shutdown().await.expect("joined reward runtime");
        assert_eq!(report.publication_attempts, 2);
        assert_eq!(report.publication_generations, 2);
        client_1.close(0_u32.into(), b"test complete");
        client_2.close(0_u32.into(), b"test complete");
        drop(runtime);
        server_endpoint_1.wait_idle().await;
        client_endpoint_1.wait_idle().await;
        server_endpoint_2.wait_idle().await;
        client_endpoint_2.wait_idle().await;
    }
}
