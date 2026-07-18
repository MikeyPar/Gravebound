//! Independent fixed-rate owner for one live Core private-microrealm runtime.
//!
//! The three authorities are `Gravebound_Production_GDD_v1_Canonical.md` (`SIM-004`, `TECH-012`,
//! `TECH-015`), `Gravebound_Content_Production_Spec_v1.md` (`CONT-WORLD-001`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`).
//!
//! This driver deliberately has no transport writer. Authenticated ingress coalesces compact
//! continuous input into one retained state while reliable ability presses advance separately.
//! The task is the runtime's only mutable owner and publishes committed frames for a higher-level
//! session owner to project later. Creating a driver does not enable ordinary route admission.

use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use sim_core::{AimDirection, MovementAction, Tick};
use thiserror::Error;
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::{JoinError, JoinHandle},
    time::{Instant, MissedTickBehavior},
};

use crate::{
    CoreBellPortalTransition, CorePrivateFixedDungeonAdvance, CorePrivateFixedDungeonLiveRoomFrame,
    CorePrivateFixedDungeonRuntime, CorePrivateFixedDungeonRuntimeError,
    CorePrivateMicrorealmInput, CorePrivateMicrorealmRuntime, CorePrivateMicrorealmRuntimeError,
    CorePrivateMicrorealmStep,
};

const NANOS_PER_SECOND: u64 = 1_000_000_000;
const DRIVER_TICK_NANOS: u64 = NANOS_PER_SECOND / 30;
const DRIVER_TICK_DURATION: Duration = Duration::from_nanos(DRIVER_TICK_NANOS);
const _: () = assert!(sim_core::TICKS_PER_SECOND == 30);

static ACTIVE_CORE_MICROREALM_DRIVER_TASKS: AtomicUsize = AtomicUsize::new(0);

/// Compact continuous intent retained between independent server frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CorePrivateMicrorealmRetainedInput {
    pub input_sequence: u64,
    pub movement: MovementAction,
    pub aim: AimDirection,
    pub primary_held: bool,
    pub primary_sequence: u32,
}

impl Default for CorePrivateMicrorealmRetainedInput {
    fn default() -> Self {
        Self {
            input_sequence: 0,
            movement: MovementAction::default(),
            aim: AimDirection::east(),
            primary_held: false,
            primary_sequence: 0,
        }
    }
}

/// Reliable action kind accepted independently from the compact latest-state channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePrivateMicrorealmAbility {
    Ability1,
    Ability2,
}

/// One already-authenticated reliable ability press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateMicrorealmAbilityPress {
    pub action_sequence: u32,
    pub ability: CorePrivateMicrorealmAbility,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct RetainedFrameInput {
    continuous: CorePrivateMicrorealmRetainedInput,
    ability_1_sequence: u32,
    ability_2_sequence: u32,
}

impl RetainedFrameInput {
    fn runtime_input(self) -> CorePrivateMicrorealmInput {
        CorePrivateMicrorealmInput {
            input_sequence: self.continuous.input_sequence,
            movement: self.continuous.movement,
            aim: self.continuous.aim,
            primary_held: self.continuous.primary_held,
            primary_sequence: self.continuous.primary_sequence,
            ability_1_sequence: self.ability_1_sequence,
            ability_2_sequence: self.ability_2_sequence,
        }
    }
}

#[derive(Debug)]
struct IngressReducer {
    retained: RetainedFrameInput,
    last_action_sequence: u32,
    accepting: bool,
}

impl Default for IngressReducer {
    fn default() -> Self {
        Self {
            retained: RetainedFrameInput::default(),
            last_action_sequence: 0,
            accepting: true,
        }
    }
}

#[derive(Debug, Default)]
struct SharedMetrics {
    accepted_input_updates: AtomicU64,
    accepted_ability_presses: AtomicU64,
    link_lost_neutralizations: AtomicU64,
}

#[derive(Debug)]
struct SharedIngress {
    reducer: Mutex<IngressReducer>,
    retained_tx: watch::Sender<RetainedFrameInput>,
    metrics: SharedMetrics,
    task_live: AtomicBool,
}

impl SharedIngress {
    fn stop_accepting(&self) {
        if let Ok(mut reducer) = self.reducer.lock() {
            reducer.accepting = false;
        }
    }

    fn resume_accepting(&self) {
        if let Ok(mut reducer) = self.reducer.lock() {
            reducer.accepting = true;
        }
    }

    /// Clears scene-local held intent at an authoritative room relocation while retaining aim and
    /// every accepted sequence watermark. A Bell-held move or primary cannot bleed into B1, and
    /// reconnect cannot replay an older action after the transition.
    fn neutralize_for_scene_transition(&self) {
        if let Ok(mut reducer) = self.reducer.lock() {
            reducer.retained.continuous.movement = MovementAction::default();
            reducer.retained.continuous.primary_held = false;
            self.publish_locked(&reducer);
        }
    }

    fn publish_locked(&self, reducer: &IngressReducer) {
        self.retained_tx.send_replace(reducer.retained);
    }
}

/// Cloneable, non-writing ingress and observation handle for the exclusive driver task.
#[derive(Debug, Clone)]
pub struct CorePrivateMicrorealmDriverHandle {
    ingress: Arc<SharedIngress>,
    observation_rx: watch::Receiver<CorePrivateMicrorealmDriverState>,
    handoff_tx: mpsc::Sender<CorePrivateMicrorealmHandoffRequest>,
    fixed_advance_tx: mpsc::Sender<CorePrivateFixedDungeonAdvanceRequest>,
}

impl CorePrivateMicrorealmDriverHandle {
    /// Pauses this task between frames without transferring ownership to the caller. The returned
    /// decision token is independent of any transport generation, so reconnect cannot create a
    /// second simulation owner while durable Bell resolution is in flight.
    pub async fn prepare_handoff(
        &self,
    ) -> Result<CorePrivateMicrorealmPreparedHandoff, CorePrivateMicrorealmDriverError> {
        let (ready_tx, ready_rx) = oneshot::channel();
        let (decision_tx, decision_rx) = oneshot::channel();
        self.handoff_tx
            .send(CorePrivateMicrorealmHandoffRequest {
                ready_tx,
                decision_rx,
            })
            .await
            .map_err(|_| CorePrivateMicrorealmDriverError::HandoffControlClosed)?;
        let ready = ready_rx
            .await
            .map_err(|_| CorePrivateMicrorealmDriverError::HandoffControlClosed)?
            .map_err(|()| CorePrivateMicrorealmDriverError::HandoffNotReady)?;
        Ok(CorePrivateMicrorealmPreparedHandoff {
            ready,
            decision_tx: Some(decision_tx),
        })
    }

    /// Requests the next canonical fixed-route transition without accepting any client-authored
    /// destination. The existing task resolves B0/B1/... readiness and performs the route CAS.
    pub async fn advance_fixed_dungeon(
        &self,
    ) -> Result<CorePrivateFixedDungeonAdvance, CorePrivateMicrorealmDriverError> {
        let (result_tx, result_rx) = oneshot::channel();
        self.fixed_advance_tx
            .send(CorePrivateFixedDungeonAdvanceRequest { result_tx })
            .await
            .map_err(|_| CorePrivateMicrorealmDriverError::FixedDungeonControlClosed)?;
        result_rx
            .await
            .map_err(|_| CorePrivateMicrorealmDriverError::FixedDungeonControlClosed)?
            .map_err(CorePrivateMicrorealmDriverError::FixedDungeonAdvance)
    }

    /// Replaces the single retained compact input when its client sequence is newer.
    pub fn submit_latest_input(
        &self,
        mut input: CorePrivateMicrorealmRetainedInput,
    ) -> Result<(), CorePrivateMicrorealmIngressError> {
        let mut reducer = self
            .ingress
            .reducer
            .lock()
            .map_err(|_| CorePrivateMicrorealmIngressError::Unavailable)?;
        ensure_accepting(&reducer)?;
        if input.input_sequence == 0 {
            return Err(CorePrivateMicrorealmIngressError::ZeroInputSequence);
        }
        if input.input_sequence <= reducer.retained.continuous.input_sequence {
            return Err(CorePrivateMicrorealmIngressError::StaleInputSequence {
                last: reducer.retained.continuous.input_sequence,
                received: input.input_sequence,
            });
        }
        let maximum_primary_sequence = reducer.retained.continuous.primary_sequence;
        if input.primary_held && input.primary_sequence < maximum_primary_sequence {
            return Err(
                CorePrivateMicrorealmIngressError::PrimarySequenceRegressed {
                    last: maximum_primary_sequence,
                    received: input.primary_sequence,
                },
            );
        }
        // Release frames in the established session wire contract may carry zero. Preserve the
        // server's maximum accepted sequence so a release cannot re-arm an already consumed shot.
        input.primary_sequence = input.primary_sequence.max(maximum_primary_sequence);
        reducer.retained.continuous = input;
        self.ingress.publish_locked(&reducer);
        self.ingress
            .metrics
            .accepted_input_updates
            .fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Accepts a reliable action sequence and advances exactly one server-owned ability sequence.
    pub fn submit_ability_press(
        &self,
        press: CorePrivateMicrorealmAbilityPress,
    ) -> Result<(), CorePrivateMicrorealmIngressError> {
        let mut reducer = self
            .ingress
            .reducer
            .lock()
            .map_err(|_| CorePrivateMicrorealmIngressError::Unavailable)?;
        ensure_accepting(&reducer)?;
        if press.action_sequence == 0 {
            return Err(CorePrivateMicrorealmIngressError::ZeroActionSequence);
        }
        if press.action_sequence <= reducer.last_action_sequence {
            return Err(CorePrivateMicrorealmIngressError::StaleActionSequence {
                last: reducer.last_action_sequence,
                received: press.action_sequence,
            });
        }
        let sequence = match press.ability {
            CorePrivateMicrorealmAbility::Ability1 => &mut reducer.retained.ability_1_sequence,
            CorePrivateMicrorealmAbility::Ability2 => &mut reducer.retained.ability_2_sequence,
        };
        *sequence = sequence
            .checked_add(1)
            .ok_or(CorePrivateMicrorealmIngressError::AbilitySequenceExhausted)?;
        reducer.last_action_sequence = press.action_sequence;
        self.ingress.publish_locked(&reducer);
        self.ingress
            .metrics
            .accepted_ability_presses
            .fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Clears transport-carried continuous danger input while retaining aim and accepted presses.
    pub fn neutralize_for_link_lost(&self) -> Result<(), CorePrivateMicrorealmIngressError> {
        let mut reducer = self
            .ingress
            .reducer
            .lock()
            .map_err(|_| CorePrivateMicrorealmIngressError::Unavailable)?;
        ensure_accepting(&reducer)?;
        reducer.retained.continuous.movement = MovementAction::default();
        reducer.retained.continuous.primary_held = false;
        self.ingress.publish_locked(&reducer);
        self.ingress
            .metrics
            .link_lost_neutralizations
            .fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    #[must_use]
    pub fn latest_retained_input(&self) -> CorePrivateMicrorealmRetainedInput {
        self.ingress.reducer.lock().map_or_else(
            |poisoned| poisoned.into_inner().retained.continuous,
            |state| state.retained.continuous,
        )
    }

    #[must_use]
    pub fn observe(&self) -> CorePrivateMicrorealmDriverObserver {
        CorePrivateMicrorealmDriverObserver {
            receiver: self.observation_rx.clone(),
        }
    }
}

fn ensure_accepting(reducer: &IngressReducer) -> Result<(), CorePrivateMicrorealmIngressError> {
    if reducer.accepting {
        Ok(())
    } else {
        Err(CorePrivateMicrorealmIngressError::DriverFrozen)
    }
}

#[derive(Debug, Clone)]
pub struct CorePrivateMicrorealmDriverObserver {
    receiver: watch::Receiver<CorePrivateMicrorealmDriverState>,
}

impl CorePrivateMicrorealmDriverObserver {
    #[must_use]
    pub fn latest(&self) -> CorePrivateMicrorealmDriverState {
        self.receiver.borrow().clone()
    }

    pub async fn changed(
        &mut self,
    ) -> Result<CorePrivateMicrorealmDriverState, CorePrivateMicrorealmObservationError> {
        self.receiver
            .changed()
            .await
            .map_err(|_| CorePrivateMicrorealmObservationError::Closed)?;
        Ok(self.latest())
    }
}

/// Fail-closed reason retained when the authoritative frame owner stops advancing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePrivateMicrorealmFaultKind {
    RouteAuthority,
    TickExhausted,
    Simulation,
}

/// Stable fault projection; the underlying non-cloneable runtime error never crosses owners.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePrivateMicrorealmDriverFault {
    pub kind: CorePrivateMicrorealmFaultKind,
    pub message: Arc<str>,
    pub last_committed_tick: Tick,
}

/// Latest bounded committed state. Terminal and fault variants are frozen until shutdown.
#[derive(Debug, Clone)]
pub enum CorePrivateMicrorealmDriverState {
    Starting,
    Running {
        committed_frames: u64,
        step: Arc<CorePrivateMicrorealmStep>,
    },
    TerminalPending {
        committed_frames: u64,
        lethal_step: Arc<CorePrivateMicrorealmStep>,
    },
    BellResolutionPending {
        committed_frames: u64,
        final_tick: Tick,
    },
    FixedDungeonReady {
        ready: CorePrivateFixedDungeonDriverReady,
    },
    FixedDungeonRunning {
        committed_frames: u64,
        frame: Arc<CorePrivateFixedDungeonLiveRoomFrame>,
    },
    FixedDungeonTerminalPending {
        committed_frames: u64,
        lethal_frame: Arc<CorePrivateFixedDungeonLiveRoomFrame>,
    },
    Faulted {
        committed_frames: u64,
        fault: CorePrivateMicrorealmDriverFault,
    },
}

impl CorePrivateMicrorealmDriverState {
    #[must_use]
    pub fn latest_step(&self) -> Option<&CorePrivateMicrorealmStep> {
        match self {
            Self::Running { step, .. } => Some(step),
            Self::TerminalPending { lethal_step, .. } => Some(lethal_step),
            Self::Starting
            | Self::BellResolutionPending { .. }
            | Self::FixedDungeonReady { .. }
            | Self::FixedDungeonRunning { .. }
            | Self::FixedDungeonTerminalPending { .. }
            | Self::Faulted { .. } => None,
        }
    }
}

/// Joined task report. This is scheduler evidence, not durable terminal evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePrivateMicrorealmDriverOutcome {
    Shutdown,
    BellResolutionPending,
    FixedDungeonReady,
    TerminalPending,
    FixedDungeonTerminalPending,
    Faulted,
}

/// Frame-boundary authority captured when the live Bell portal is still valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateMicrorealmHandoffReady {
    pub committed_frames: u64,
    pub final_tick: Tick,
}

/// Stable non-frame observation emitted after the same exclusive task has consumed its microrealm
/// and owns the route-bound B0-B6 runtime. It identifies B0, B4, B6, and the atomic boundary before
/// the first frame of a newly entered combat room. Normal admission remains independently gated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateFixedDungeonDriverReady {
    pub committed_microrealm_frames: u64,
    pub final_microrealm_tick: Tick,
    pub transfer_id: [u8; 16],
    pub route_lease: crate::CorePrivateRouteActorLease,
    pub node: sim_content::CoreFixedDungeonNode,
}

/// Awaitable acknowledgement for an irreversible in-task conversion. Dropping this value only
/// drops the caller's acknowledgement; it cannot cancel the decision already owned by the task.
#[derive(Debug)]
pub struct CorePrivateFixedDungeonConversion {
    result_rx: oneshot::Receiver<Result<CorePrivateFixedDungeonDriverReady, String>>,
}

impl CorePrivateFixedDungeonConversion {
    pub async fn wait(
        self,
    ) -> Result<CorePrivateFixedDungeonDriverReady, CorePrivateMicrorealmDriverError> {
        self.result_rx
            .await
            .map_err(|_| CorePrivateMicrorealmDriverError::HandoffDecisionLost)?
            .map_err(CorePrivateMicrorealmDriverError::FixedDungeonConversion)
    }
}

#[derive(Debug)]
enum CorePrivateMicrorealmHandoffDecision {
    Abort(oneshot::Sender<()>),
    Convert(Box<CorePrivateFixedDungeonConversionRequest>),
}

#[derive(Debug)]
struct CorePrivateFixedDungeonConversionRequest {
    transition: CoreBellPortalTransition,
    expected_content_revision: protocol::CorePrivateRouteContentRevisionV1,
    encounters: sim_content::CoreDevelopmentEncounterRooms,
    result_tx: oneshot::Sender<Result<CorePrivateFixedDungeonDriverReady, String>>,
}

#[derive(Debug)]
struct CorePrivateFixedDungeonAdvanceRequest {
    result_tx: oneshot::Sender<Result<CorePrivateFixedDungeonAdvance, String>>,
}

#[derive(Debug)]
struct CorePrivateMicrorealmHandoffRequest {
    ready_tx: oneshot::Sender<Result<CorePrivateMicrorealmHandoffReady, ()>>,
    decision_rx: oneshot::Receiver<CorePrivateMicrorealmHandoffDecision>,
}

/// Non-cloneable two-phase pause. Explicit abort resumes the exact runtime. Dropping after the
/// pause acknowledgement is treated as an unknown durable outcome and stays frozen until restart
/// reconciliation; conversion consumes the microrealm only inside its existing exclusive task.
#[derive(Debug)]
pub struct CorePrivateMicrorealmPreparedHandoff {
    ready: CorePrivateMicrorealmHandoffReady,
    decision_tx: Option<oneshot::Sender<CorePrivateMicrorealmHandoffDecision>>,
}

impl CorePrivateMicrorealmPreparedHandoff {
    #[must_use]
    pub const fn ready(&self) -> CorePrivateMicrorealmHandoffReady {
        self.ready
    }

    pub async fn abort(
        mut self,
    ) -> Result<CorePrivateMicrorealmHandoffReady, CorePrivateMicrorealmDriverError> {
        let decision = self
            .decision_tx
            .take()
            .ok_or(CorePrivateMicrorealmDriverError::HandoffDecisionLost)?;
        let (resumed_tx, resumed_rx) = oneshot::channel();
        decision
            .send(CorePrivateMicrorealmHandoffDecision::Abort(resumed_tx))
            .map_err(|_| CorePrivateMicrorealmDriverError::HandoffDecisionLost)?;
        resumed_rx
            .await
            .map_err(|_| CorePrivateMicrorealmDriverError::HandoffDecisionLost)?;
        Ok(self.ready)
    }

    /// Installs the already-durable Bell result inside the existing task. Once this returns, loss
    /// of the acknowledgement cannot undo the conversion or lose either runtime: the
    /// authoritative result remains visible through the original observer.
    pub fn commit_into_fixed_dungeon(
        mut self,
        transition: CoreBellPortalTransition,
        expected_content_revision: protocol::CorePrivateRouteContentRevisionV1,
        encounters: sim_content::CoreDevelopmentEncounterRooms,
    ) -> Result<CorePrivateFixedDungeonConversion, CorePrivateMicrorealmDriverError> {
        let (result_tx, result_rx) = oneshot::channel();
        self.decision_tx
            .take()
            .ok_or(CorePrivateMicrorealmDriverError::HandoffDecisionLost)?
            .send(CorePrivateMicrorealmHandoffDecision::Convert(Box::new(
                CorePrivateFixedDungeonConversionRequest {
                    transition,
                    expected_content_revision,
                    encounters,
                    result_tx,
                },
            )))
            .map_err(|_| CorePrivateMicrorealmDriverError::HandoffDecisionLost)?;
        Ok(CorePrivateFixedDungeonConversion { result_rx })
    }
}

/// Joined task report. This is scheduler evidence, not durable terminal evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateMicrorealmDriverReport {
    pub committed_frames: u64,
    pub final_tick: Tick,
    pub skipped_deadlines: u64,
    pub accepted_input_updates: u64,
    pub accepted_ability_presses: u64,
    pub link_lost_neutralizations: u64,
    pub outcome: CorePrivateMicrorealmDriverOutcome,
    pub task_joined: bool,
    pub driver_task_live_after_join: bool,
    pub active_driver_tasks_after_join: usize,
}

/// Exclusive task owner. Dropping it requests graceful frame-complete shutdown; `shutdown` joins.
#[derive(Debug)]
pub struct CorePrivateMicrorealmDriver {
    handle: CorePrivateMicrorealmDriverHandle,
    shutdown_tx: watch::Sender<bool>,
    join: Option<JoinHandle<CorePrivateMicrorealmDriverTaskExit>>,
}

impl CorePrivateMicrorealmDriver {
    #[must_use]
    pub fn spawn(runtime: CorePrivateMicrorealmRuntime) -> Self {
        spawn_driver(runtime)
    }

    #[must_use]
    pub fn handle(&self) -> CorePrivateMicrorealmDriverHandle {
        self.handle.clone()
    }

    /// Pauses the exclusive owner between frames only when its owned simulation still proves the
    /// cleared Bell interaction. Cancellation before acknowledgement resumes automatically.
    pub async fn prepare_handoff(
        &self,
    ) -> Result<CorePrivateMicrorealmPreparedHandoff, CorePrivateMicrorealmDriverError> {
        self.handle.prepare_handoff().await
    }

    pub async fn shutdown(
        mut self,
    ) -> Result<CorePrivateMicrorealmDriverReport, CorePrivateMicrorealmDriverError> {
        self.handle.ingress.stop_accepting();
        self.shutdown_tx.send_replace(true);
        let join = self
            .join
            .take()
            .ok_or(CorePrivateMicrorealmDriverError::AlreadyJoined)?;
        let mut exit = join.await.map_err(CorePrivateMicrorealmDriverError::Task)?;
        let report = &mut exit.report;
        report.task_joined = true;
        report.driver_task_live_after_join = self.handle.ingress.task_live.load(Ordering::Acquire);
        report.active_driver_tasks_after_join = active_core_microrealm_driver_tasks();
        Ok(*report)
    }
}

impl Drop for CorePrivateMicrorealmDriver {
    fn drop(&mut self) {
        self.handle.ingress.stop_accepting();
        self.shutdown_tx.send_replace(true);
    }
}

#[must_use]
pub fn active_core_microrealm_driver_tasks() -> usize {
    ACTIVE_CORE_MICROREALM_DRIVER_TASKS.load(Ordering::Acquire)
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CorePrivateMicrorealmIngressError {
    #[error("Core private-microrealm ingress is unavailable")]
    Unavailable,
    #[error("Core private-microrealm driver is terminal, faulted, or shutting down")]
    DriverFrozen,
    #[error("latest-state input sequence must be nonzero")]
    ZeroInputSequence,
    #[error("latest-state input sequence {received} is not newer than {last}")]
    StaleInputSequence { last: u64, received: u64 },
    #[error("primary sequence {received} regressed below {last}")]
    PrimarySequenceRegressed { last: u32, received: u32 },
    #[error("reliable action sequence must be nonzero")]
    ZeroActionSequence,
    #[error("reliable action sequence {received} is not newer than {last}")]
    StaleActionSequence { last: u32, received: u32 },
    #[error("server-owned ability press sequence exhausted")]
    AbilitySequenceExhausted,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CorePrivateMicrorealmObservationError {
    #[error("Core private-microrealm observation channel closed")]
    Closed,
}

#[derive(Debug, Error)]
pub enum CorePrivateMicrorealmDriverError {
    #[error("Core private-microrealm driver was already joined")]
    AlreadyJoined,
    #[error("Core private-microrealm driver task failed: {0}")]
    Task(#[source] JoinError),
    #[error("Core private-microrealm handoff control is closed")]
    HandoffControlClosed,
    #[error("Core private-microrealm handoff requires a live cleared Bell interaction")]
    HandoffNotReady,
    #[error("Core private-microrealm handoff decision was lost")]
    HandoffDecisionLost,
    #[error("Core fixed-dungeon conversion failed: {0}")]
    FixedDungeonConversion(String),
    #[error("Core fixed-dungeon control is closed")]
    FixedDungeonControlClosed,
    #[error("Core fixed-dungeon advance failed: {0}")]
    FixedDungeonAdvance(String),
}

trait MicrorealmFrameRuntime: Send + 'static {
    fn step_frame(
        &mut self,
        input: CorePrivateMicrorealmInput,
    ) -> impl Future<Output = Result<CorePrivateMicrorealmStep, CorePrivateMicrorealmRuntimeError>> + Send;

    fn handoff_ready(&self) -> bool {
        false
    }

    fn into_live_runtime(self) -> Option<CorePrivateMicrorealmRuntime>
    where
        Self: Sized,
    {
        None
    }
}

impl MicrorealmFrameRuntime for CorePrivateMicrorealmRuntime {
    fn step_frame(
        &mut self,
        input: CorePrivateMicrorealmInput,
    ) -> impl Future<Output = Result<CorePrivateMicrorealmStep, CorePrivateMicrorealmRuntimeError>> + Send
    {
        self.step(input)
    }

    fn handoff_ready(&self) -> bool {
        self.bell_transfer_ready()
    }

    fn into_live_runtime(self) -> Option<CorePrivateMicrorealmRuntime> {
        Some(self)
    }
}

#[derive(Debug)]
struct CorePrivateMicrorealmDriverTaskExit {
    report: CorePrivateMicrorealmDriverReport,
}

fn spawn_driver<R>(runtime: R) -> CorePrivateMicrorealmDriver
where
    R: MicrorealmFrameRuntime,
{
    let (retained_tx, retained_rx) = watch::channel(RetainedFrameInput::default());
    let ingress = Arc::new(SharedIngress {
        reducer: Mutex::new(IngressReducer::default()),
        retained_tx,
        metrics: SharedMetrics::default(),
        task_live: AtomicBool::new(true),
    });
    let (observation_tx, observation_rx) =
        watch::channel(CorePrivateMicrorealmDriverState::Starting);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let (handoff_tx, handoff_rx) = mpsc::channel(1);
    let (fixed_advance_tx, fixed_advance_rx) = mpsc::channel(1);
    ACTIVE_CORE_MICROREALM_DRIVER_TASKS.fetch_add(1, Ordering::AcqRel);
    let task_ingress = Arc::clone(&ingress);
    let join = tokio::spawn(async move {
        let _task_guard = ActiveDriverTaskGuard {
            ingress: Arc::clone(&task_ingress),
        };
        run_driver(
            runtime,
            task_ingress,
            retained_rx,
            observation_tx,
            shutdown_rx,
            handoff_rx,
            fixed_advance_rx,
        )
        .await
    });
    CorePrivateMicrorealmDriver {
        handle: CorePrivateMicrorealmDriverHandle {
            ingress,
            observation_rx,
            handoff_tx: handoff_tx.clone(),
            fixed_advance_tx,
        },
        shutdown_tx,
        join: Some(join),
    }
}

struct ActiveDriverTaskGuard {
    ingress: Arc<SharedIngress>,
}

impl Drop for ActiveDriverTaskGuard {
    fn drop(&mut self) {
        self.ingress.task_live.store(false, Ordering::Release);
        ACTIVE_CORE_MICROREALM_DRIVER_TASKS.fetch_sub(1, Ordering::AcqRel);
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one select loop owns the full frame-boundary handoff and fail-closed shutdown order"
)]
async fn run_driver<R>(
    mut runtime: R,
    ingress: Arc<SharedIngress>,
    mut retained_rx: watch::Receiver<RetainedFrameInput>,
    observation_tx: watch::Sender<CorePrivateMicrorealmDriverState>,
    mut shutdown_rx: watch::Receiver<bool>,
    mut handoff_rx: mpsc::Receiver<CorePrivateMicrorealmHandoffRequest>,
    mut fixed_advance_rx: mpsc::Receiver<CorePrivateFixedDungeonAdvanceRequest>,
) -> CorePrivateMicrorealmDriverTaskExit
where
    R: MicrorealmFrameRuntime,
{
    let mut interval = fixed_driver_interval().await;
    let mut committed_frames = 0_u64;
    let mut final_tick = Tick(0);
    let mut skipped_deadlines = 0_u64;
    let mut outcome = CorePrivateMicrorealmDriverOutcome::Shutdown;

    loop {
        let deadline = tokio::select! {
            biased;
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
                continue;
            }
            request = handoff_rx.recv() => {
                let Some(request) = request else {
                    break;
                };
                match handle_handoff_request(
                    runtime.handoff_ready(),
                    &ingress,
                    &observation_tx,
                    request,
                    committed_frames,
                    final_tick,
                    &mut shutdown_rx,
                    &mut fixed_advance_rx,
                ).await {
                    HandoffControlOutcome::Continue => continue,
                    HandoffControlOutcome::Convert(request) => {
                        return convert_runtime_and_wait(
                            runtime,
                            request,
                            &ingress,
                            &observation_tx,
                            committed_frames,
                            final_tick,
                            skipped_deadlines,
                            &mut shutdown_rx,
                            &mut handoff_rx,
                            &mut fixed_advance_rx,
                        ).await;
                    }
                    HandoffControlOutcome::Indeterminate => {
                        outcome = CorePrivateMicrorealmDriverOutcome::BellResolutionPending;
                        wait_for_shutdown(
                            &mut shutdown_rx,
                            &mut handoff_rx,
                            &mut fixed_advance_rx,
                            "fixed dungeon is unavailable while Bell resolution is indeterminate",
                        ).await;
                        break;
                    }
                    HandoffControlOutcome::Shutdown => break,
                }
            }
            request = fixed_advance_rx.recv() => {
                let Some(request) = request else {
                    break;
                };
                reject_fixed_advance(request, "fixed dungeon is not installed");
                continue;
            }
            deadline = interval.tick() => deadline,
        };
        let lateness = Instant::now().saturating_duration_since(deadline);
        let missed = lateness.as_nanos() / u128::from(DRIVER_TICK_NANOS);
        skipped_deadlines =
            skipped_deadlines.saturating_add(u64::try_from(missed).unwrap_or(u64::MAX));

        let retained = *retained_rx.borrow_and_update();
        match runtime.step_frame(retained.runtime_input()).await {
            Ok(step) => {
                committed_frames = committed_frames.saturating_add(1);
                final_tick = step.tick;
                let step = Arc::new(step);
                if step.player_died {
                    outcome = CorePrivateMicrorealmDriverOutcome::TerminalPending;
                    ingress.stop_accepting();
                    observation_tx.send_replace(
                        CorePrivateMicrorealmDriverState::TerminalPending {
                            committed_frames,
                            lethal_step: step,
                        },
                    );
                    wait_for_shutdown(
                        &mut shutdown_rx,
                        &mut handoff_rx,
                        &mut fixed_advance_rx,
                        "fixed dungeon cannot advance after a terminal frame",
                    )
                    .await;
                    break;
                }
                observation_tx.send_replace(CorePrivateMicrorealmDriverState::Running {
                    committed_frames,
                    step,
                });
            }
            Err(error) => {
                outcome = CorePrivateMicrorealmDriverOutcome::Faulted;
                ingress.stop_accepting();
                observation_tx.send_replace(CorePrivateMicrorealmDriverState::Faulted {
                    committed_frames,
                    fault: runtime_fault(&error, final_tick),
                });
                wait_for_shutdown(
                    &mut shutdown_rx,
                    &mut handoff_rx,
                    &mut fixed_advance_rx,
                    "fixed dungeon cannot advance after a driver fault",
                )
                .await;
                break;
            }
        }
    }
    ingress.stop_accepting();
    driver_task_exit(
        &ingress,
        committed_frames,
        final_tick,
        skipped_deadlines,
        outcome,
    )
}

async fn fixed_driver_interval() -> tokio::time::Interval {
    let mut interval = tokio::time::interval(DRIVER_TICK_DURATION);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    interval.tick().await;
    interval
}

#[allow(clippy::too_many_arguments)]
async fn convert_runtime_and_wait<R>(
    runtime: R,
    request: Box<CorePrivateFixedDungeonConversionRequest>,
    ingress: &Arc<SharedIngress>,
    observation_tx: &watch::Sender<CorePrivateMicrorealmDriverState>,
    committed_frames: u64,
    final_tick: Tick,
    skipped_deadlines: u64,
    shutdown_rx: &mut watch::Receiver<bool>,
    handoff_rx: &mut mpsc::Receiver<CorePrivateMicrorealmHandoffRequest>,
    fixed_advance_rx: &mut mpsc::Receiver<CorePrivateFixedDungeonAdvanceRequest>,
) -> CorePrivateMicrorealmDriverTaskExit
where
    R: MicrorealmFrameRuntime,
{
    ingress.stop_accepting();
    let CorePrivateFixedDungeonConversionRequest {
        transition,
        expected_content_revision,
        encounters,
        result_tx,
    } = *request;
    let transfer_id = transition.transfer_id;
    let converted = runtime
        .into_live_runtime()
        .ok_or_else(|| "test-only runtime cannot become a fixed dungeon".to_owned())
        .and_then(|microrealm| {
            CorePrivateFixedDungeonRuntime::from_committed_bell(
                microrealm,
                &transition,
                &expected_content_revision,
                encounters,
            )
            .map_err(|error| error.to_string())
        });
    match converted {
        Ok(fixed_dungeon) => {
            let final_tick = fixed_dungeon.tick();
            let ready = CorePrivateFixedDungeonDriverReady {
                committed_microrealm_frames: committed_frames,
                final_microrealm_tick: final_tick,
                transfer_id,
                route_lease: fixed_dungeon.route_lease(),
                node: fixed_dungeon.node(),
            };
            observation_tx
                .send_replace(CorePrivateMicrorealmDriverState::FixedDungeonReady { ready });
            let _ = result_tx.send(Ok(ready));
            run_fixed_dungeon(
                fixed_dungeon,
                ready,
                ingress,
                observation_tx,
                committed_frames,
                final_tick,
                skipped_deadlines,
                shutdown_rx,
                handoff_rx,
                fixed_advance_rx,
            )
            .await
        }
        Err(message) => {
            observation_tx.send_replace(CorePrivateMicrorealmDriverState::Faulted {
                committed_frames,
                fault: CorePrivateMicrorealmDriverFault {
                    kind: CorePrivateMicrorealmFaultKind::RouteAuthority,
                    message: Arc::from(message.clone()),
                    last_committed_tick: final_tick,
                },
            });
            let _ = result_tx.send(Err(message));
            wait_for_shutdown(
                shutdown_rx,
                handoff_rx,
                fixed_advance_rx,
                "fixed dungeon cannot advance after conversion failed",
            )
            .await;
            driver_task_exit(
                ingress,
                committed_frames,
                final_tick,
                skipped_deadlines,
                CorePrivateMicrorealmDriverOutcome::Faulted,
            )
        }
    }
}

enum FixedDungeonDriverEvent {
    Shutdown,
    Handoff(Option<CorePrivateMicrorealmHandoffRequest>),
    Advance(Option<CorePrivateFixedDungeonAdvanceRequest>),
    Frame(Instant),
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn run_fixed_dungeon(
    mut runtime: CorePrivateFixedDungeonRuntime,
    mut ready: CorePrivateFixedDungeonDriverReady,
    ingress: &Arc<SharedIngress>,
    observation_tx: &watch::Sender<CorePrivateMicrorealmDriverState>,
    mut committed_frames: u64,
    mut final_tick: Tick,
    mut skipped_deadlines: u64,
    shutdown_rx: &mut watch::Receiver<bool>,
    handoff_rx: &mut mpsc::Receiver<CorePrivateMicrorealmHandoffRequest>,
    fixed_advance_rx: &mut mpsc::Receiver<CorePrivateFixedDungeonAdvanceRequest>,
) -> CorePrivateMicrorealmDriverTaskExit {
    let mut retained_rx = ingress.retained_tx.subscribe();
    let mut interval = fixed_driver_interval().await;
    let mut outcome = CorePrivateMicrorealmDriverOutcome::FixedDungeonReady;

    loop {
        let event = tokio::select! {
            biased;
            changed = shutdown_rx.changed() => {
                let _ = changed;
                FixedDungeonDriverEvent::Shutdown
            }
            request = handoff_rx.recv() => FixedDungeonDriverEvent::Handoff(request),
            request = fixed_advance_rx.recv() => FixedDungeonDriverEvent::Advance(request),
            deadline = interval.tick(), if runtime.room_phase().is_some() => {
                FixedDungeonDriverEvent::Frame(deadline)
            }
        };

        match event {
            FixedDungeonDriverEvent::Shutdown => break,
            FixedDungeonDriverEvent::Handoff(Some(request)) => {
                let _ = request.ready_tx.send(Err(()));
            }
            FixedDungeonDriverEvent::Handoff(None) | FixedDungeonDriverEvent::Advance(None) => {
                break;
            }
            FixedDungeonDriverEvent::Advance(Some(request)) => match runtime.advance().await {
                Ok(advance) => {
                    ready.node = advance.transition.to;
                    if runtime.room_phase().is_some() {
                        ingress.neutralize_for_scene_transition();
                        ingress.resume_accepting();
                        interval.reset();
                    } else {
                        ingress.stop_accepting();
                    }
                    observation_tx.send_replace(
                        CorePrivateMicrorealmDriverState::FixedDungeonReady { ready },
                    );
                    outcome = CorePrivateMicrorealmDriverOutcome::Shutdown;
                    let _ = request.result_tx.send(Ok(advance));
                }
                Err(error) => {
                    let message = error.to_string();
                    let fatal = fixed_advance_error_is_fatal(&error);
                    let _ = request.result_tx.send(Err(message));
                    if fatal {
                        outcome = CorePrivateMicrorealmDriverOutcome::Faulted;
                        ingress.stop_accepting();
                        observation_tx.send_replace(CorePrivateMicrorealmDriverState::Faulted {
                            committed_frames,
                            fault: fixed_runtime_fault(&error, final_tick),
                        });
                        wait_for_shutdown(
                            shutdown_rx,
                            handoff_rx,
                            fixed_advance_rx,
                            "fixed dungeon cannot advance after a route fault",
                        )
                        .await;
                        break;
                    }
                }
            },
            FixedDungeonDriverEvent::Frame(deadline) => {
                let lateness = Instant::now().saturating_duration_since(deadline);
                let missed = lateness.as_nanos() / u128::from(DRIVER_TICK_NANOS);
                skipped_deadlines =
                    skipped_deadlines.saturating_add(u64::try_from(missed).unwrap_or(u64::MAX));
                let retained = *retained_rx.borrow_and_update();
                match runtime.step_live_room(retained.runtime_input()).await {
                    Ok(frame) => {
                        committed_frames = committed_frames.saturating_add(1);
                        final_tick = frame.tick;
                        let frame = Arc::new(frame);
                        if frame.player_died {
                            outcome =
                                CorePrivateMicrorealmDriverOutcome::FixedDungeonTerminalPending;
                            ingress.stop_accepting();
                            observation_tx.send_replace(
                                CorePrivateMicrorealmDriverState::FixedDungeonTerminalPending {
                                    committed_frames,
                                    lethal_frame: frame,
                                },
                            );
                            wait_for_shutdown(
                                shutdown_rx,
                                handoff_rx,
                                fixed_advance_rx,
                                "fixed dungeon cannot advance after a terminal frame",
                            )
                            .await;
                            break;
                        }
                        observation_tx.send_replace(
                            CorePrivateMicrorealmDriverState::FixedDungeonRunning {
                                committed_frames,
                                frame,
                            },
                        );
                    }
                    Err(error) => {
                        outcome = CorePrivateMicrorealmDriverOutcome::Faulted;
                        ingress.stop_accepting();
                        observation_tx.send_replace(CorePrivateMicrorealmDriverState::Faulted {
                            committed_frames,
                            fault: fixed_runtime_fault(&error, final_tick),
                        });
                        wait_for_shutdown(
                            shutdown_rx,
                            handoff_rx,
                            fixed_advance_rx,
                            "fixed dungeon cannot advance after a driver fault",
                        )
                        .await;
                        break;
                    }
                }
            }
        }
    }

    ingress.stop_accepting();
    driver_task_exit(
        ingress,
        committed_frames,
        final_tick,
        skipped_deadlines,
        outcome,
    )
}

fn driver_task_exit(
    ingress: &SharedIngress,
    committed_frames: u64,
    final_tick: Tick,
    skipped_deadlines: u64,
    outcome: CorePrivateMicrorealmDriverOutcome,
) -> CorePrivateMicrorealmDriverTaskExit {
    CorePrivateMicrorealmDriverTaskExit {
        report: CorePrivateMicrorealmDriverReport {
            committed_frames,
            final_tick,
            skipped_deadlines,
            accepted_input_updates: ingress
                .metrics
                .accepted_input_updates
                .load(Ordering::Relaxed),
            accepted_ability_presses: ingress
                .metrics
                .accepted_ability_presses
                .load(Ordering::Relaxed),
            link_lost_neutralizations: ingress
                .metrics
                .link_lost_neutralizations
                .load(Ordering::Relaxed),
            outcome,
            task_joined: false,
            driver_task_live_after_join: true,
            active_driver_tasks_after_join: active_core_microrealm_driver_tasks(),
        },
    }
}

enum HandoffControlOutcome {
    Continue,
    Convert(Box<CorePrivateFixedDungeonConversionRequest>),
    Indeterminate,
    Shutdown,
}

#[allow(
    clippy::too_many_arguments,
    reason = "the frame-boundary pause borrows each independent owner without creating a second context owner"
)]
async fn handle_handoff_request(
    handoff_ready: bool,
    ingress: &Arc<SharedIngress>,
    observation_tx: &watch::Sender<CorePrivateMicrorealmDriverState>,
    request: CorePrivateMicrorealmHandoffRequest,
    committed_frames: u64,
    final_tick: Tick,
    shutdown_rx: &mut watch::Receiver<bool>,
    fixed_advance_rx: &mut mpsc::Receiver<CorePrivateFixedDungeonAdvanceRequest>,
) -> HandoffControlOutcome {
    if !handoff_ready {
        let _ = request.ready_tx.send(Err(()));
        return HandoffControlOutcome::Continue;
    }
    ingress.stop_accepting();
    let ready = CorePrivateMicrorealmHandoffReady {
        committed_frames,
        final_tick,
    };
    if request.ready_tx.send(Ok(ready)).is_err() {
        ingress.resume_accepting();
        return HandoffControlOutcome::Continue;
    }
    let previous_state =
        observation_tx.send_replace(CorePrivateMicrorealmDriverState::BellResolutionPending {
            committed_frames,
            final_tick,
        });
    match await_handoff_decision(request.decision_rx, shutdown_rx, fixed_advance_rx).await {
        HandoffWaitOutcome::Abort(resumed_tx) => {
            ingress.resume_accepting();
            observation_tx.send_replace(previous_state);
            let _ = resumed_tx.send(());
            HandoffControlOutcome::Continue
        }
        HandoffWaitOutcome::DecisionDropped => HandoffControlOutcome::Indeterminate,
        HandoffWaitOutcome::Convert(request) => HandoffControlOutcome::Convert(request),
        HandoffWaitOutcome::Shutdown => HandoffControlOutcome::Shutdown,
    }
}

enum HandoffWaitOutcome {
    Abort(oneshot::Sender<()>),
    DecisionDropped,
    Convert(Box<CorePrivateFixedDungeonConversionRequest>),
    Shutdown,
}

async fn await_handoff_decision(
    decision_rx: oneshot::Receiver<CorePrivateMicrorealmHandoffDecision>,
    shutdown_rx: &mut watch::Receiver<bool>,
    fixed_advance_rx: &mut mpsc::Receiver<CorePrivateFixedDungeonAdvanceRequest>,
) -> HandoffWaitOutcome {
    let mut decision_rx = decision_rx;
    loop {
        tokio::select! {
            biased;
            changed = shutdown_rx.changed() => {
                let _ = changed;
                return HandoffWaitOutcome::Shutdown;
            }
            decision = &mut decision_rx => return match decision {
                Ok(CorePrivateMicrorealmHandoffDecision::Abort(resumed_tx)) => {
                    HandoffWaitOutcome::Abort(resumed_tx)
                }
                Ok(CorePrivateMicrorealmHandoffDecision::Convert(request)) => {
                    HandoffWaitOutcome::Convert(request)
                }
                Err(_) => HandoffWaitOutcome::DecisionDropped,
            },
            request = fixed_advance_rx.recv() => {
                let Some(request) = request else {
                    return HandoffWaitOutcome::Shutdown;
                };
                reject_fixed_advance(
                    request,
                    "fixed dungeon is unavailable while Bell resolution is pending",
                );
            }
        }
    }
}

async fn wait_for_shutdown(
    shutdown_rx: &mut watch::Receiver<bool>,
    handoff_rx: &mut mpsc::Receiver<CorePrivateMicrorealmHandoffRequest>,
    fixed_advance_rx: &mut mpsc::Receiver<CorePrivateFixedDungeonAdvanceRequest>,
    fixed_advance_error: &'static str,
) {
    while !*shutdown_rx.borrow() {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() {
                    break;
                }
            }
            request = handoff_rx.recv() => {
                let Some(request) = request else {
                    break;
                };
                let _ = request.ready_tx.send(Err(()));
            }
            request = fixed_advance_rx.recv() => {
                let Some(request) = request else {
                    break;
                };
                reject_fixed_advance(request, fixed_advance_error);
            }
        }
    }
}

fn reject_fixed_advance(request: CorePrivateFixedDungeonAdvanceRequest, message: &'static str) {
    let _ = request.result_tx.send(Err(message.to_owned()));
}

fn runtime_fault(
    error: &CorePrivateMicrorealmRuntimeError,
    last_committed_tick: Tick,
) -> CorePrivateMicrorealmDriverFault {
    let kind = match &error {
        CorePrivateMicrorealmRuntimeError::RouteAuthorityMismatch
        | CorePrivateMicrorealmRuntimeError::Route(_) => {
            CorePrivateMicrorealmFaultKind::RouteAuthority
        }
        CorePrivateMicrorealmRuntimeError::TickExhausted => {
            CorePrivateMicrorealmFaultKind::TickExhausted
        }
        _ => CorePrivateMicrorealmFaultKind::Simulation,
    };
    CorePrivateMicrorealmDriverFault {
        kind,
        message: Arc::from(error.to_string()),
        last_committed_tick,
    }
}

fn fixed_advance_error_is_fatal(error: &CorePrivateFixedDungeonRuntimeError) -> bool {
    !matches!(
        error,
        CorePrivateFixedDungeonRuntimeError::Dungeon(
            sim_content::CoreFixedDungeonError::AdvanceUnavailable { .. }
                | sim_content::CoreFixedDungeonError::RestResolutionRequired,
        )
    )
}

fn fixed_runtime_fault(
    error: &CorePrivateFixedDungeonRuntimeError,
    last_committed_tick: Tick,
) -> CorePrivateMicrorealmDriverFault {
    let kind = match error {
        CorePrivateFixedDungeonRuntimeError::RouteAuthorityMismatch
        | CorePrivateFixedDungeonRuntimeError::Route(_) => {
            CorePrivateMicrorealmFaultKind::RouteAuthority
        }
        CorePrivateFixedDungeonRuntimeError::TickExhausted => {
            CorePrivateMicrorealmFaultKind::TickExhausted
        }
        _ => CorePrivateMicrorealmFaultKind::Simulation,
    };
    CorePrivateMicrorealmDriverFault {
        kind,
        message: Arc::from(error.to_string()),
        last_committed_tick,
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex as StdMutex};

    use protocol::{
        CORE_PRIVATE_ROUTE_SCHEMA_VERSION, CorePrivateRouteContentRevisionV1,
        CorePrivateRoutePhaseV1, CorePrivateRouteReadinessV1, CorePrivateRouteSceneV1,
        CorePrivateRouteStateV1, ManifestHash,
    };
    use tokio::sync::Notify;

    use super::*;

    fn hash(byte: char) -> ManifestHash {
        ManifestHash::new(byte.to_string().repeat(64)).expect("valid hash")
    }

    fn template_step(tick: u64) -> CorePrivateMicrorealmStep {
        CorePrivateMicrorealmStep {
            input_sequence: 0,
            tick: Tick(tick),
            player_position: sim_core::TilePoint::new(24_000, 24_000),
            phase: sim_core::CoreMicrorealmPhase::Dormant,
            route: CorePrivateRouteStateV1 {
                schema_version: CORE_PRIVATE_ROUTE_SCHEMA_VERSION,
                character_id: [0x22; 16],
                actor_generation: 1,
                character_version: 1,
                content_revision: CorePrivateRouteContentRevisionV1 {
                    records_blake3: hash('a'),
                    assets_blake3: hash('b'),
                    localization_blake3: hash('c'),
                },
                instance_lineage_id: Some([0x33; 16]),
                scene: CorePrivateRouteSceneV1::CoreMicrorealm,
                room: None,
                phase: CorePrivateRoutePhaseV1::MicrorealmDormant,
                readiness: CorePrivateRouteReadinessV1::canonical(
                    CorePrivateRoutePhaseV1::MicrorealmDormant,
                ),
                state_version: 1,
            },
            events: Vec::new(),
            movement: sim_core::MovementStep {
                position: sim_core::SimulationVector::new(24.0, 24.0),
                velocity: sim_core::SimulationVector::new(0.0, 0.0),
                collided: false,
            },
            combat: sim_core::CombatStep::default(),
            wave: None,
            pack_clear: None,
            player_died: false,
            bell_portal_in_range: false,
        }
    }

    struct ScriptedRuntime {
        tick: u64,
        received: Arc<StdMutex<Vec<CorePrivateMicrorealmInput>>>,
        terminal_at: Option<u64>,
        fault_at: Option<u64>,
        entered: Option<Arc<Notify>>,
        release: Option<Arc<Notify>>,
        scripted: VecDeque<CorePrivateMicrorealmStep>,
        handoff_ready: bool,
    }

    impl ScriptedRuntime {
        fn ordinary(received: Arc<StdMutex<Vec<CorePrivateMicrorealmInput>>>) -> Self {
            Self {
                tick: 0,
                received,
                terminal_at: None,
                fault_at: None,
                entered: None,
                release: None,
                scripted: VecDeque::new(),
                handoff_ready: false,
            }
        }
    }

    impl MicrorealmFrameRuntime for ScriptedRuntime {
        async fn step_frame(
            &mut self,
            input: CorePrivateMicrorealmInput,
        ) -> Result<CorePrivateMicrorealmStep, CorePrivateMicrorealmRuntimeError> {
            self.tick += 1;
            self.received.lock().expect("received").push(input);
            if let Some(entered) = &self.entered {
                entered.notify_one();
            }
            if let Some(release) = &self.release {
                release.notified().await;
            }
            if self.fault_at == Some(self.tick) {
                return Err(CorePrivateMicrorealmRuntimeError::RouteAuthorityMismatch);
            }
            let mut step = self
                .scripted
                .pop_front()
                .unwrap_or_else(|| template_step(self.tick));
            step.tick = Tick(self.tick);
            step.input_sequence = input.input_sequence;
            step.player_died = self.terminal_at == Some(self.tick);
            Ok(step)
        }

        fn handoff_ready(&self) -> bool {
            self.handoff_ready
        }
    }

    async fn advance_one_frame(observer: &mut CorePrivateMicrorealmDriverObserver) {
        tokio::time::advance(DRIVER_TICK_DURATION).await;
        let _ = observer.changed().await.expect("driver observation");
    }

    #[tokio::test(start_paused = true)]
    async fn fixed_advance_is_rejected_without_blocking_before_bell_conversion() {
        let received = Arc::new(StdMutex::new(Vec::new()));
        let driver = spawn_driver(ScriptedRuntime::ordinary(received));
        tokio::task::yield_now().await;

        assert!(matches!(
            driver.handle().advance_fixed_dungeon().await,
            Err(CorePrivateMicrorealmDriverError::FixedDungeonAdvance(message))
                if message == "fixed dungeon is not installed"
        ));
        let report = driver.shutdown().await.expect("joined shutdown");
        assert_eq!(report.committed_frames, 0);
        assert!(report.task_joined);
    }

    #[tokio::test(start_paused = true)]
    async fn independent_clock_commits_thirty_frames_and_retains_one_latest_input() {
        let received = Arc::new(StdMutex::new(Vec::new()));
        let driver = spawn_driver(ScriptedRuntime::ordinary(Arc::clone(&received)));
        let handle = driver.handle();
        let mut observer = handle.observe();
        tokio::task::yield_now().await;

        handle
            .submit_latest_input(CorePrivateMicrorealmRetainedInput {
                input_sequence: 1,
                movement: MovementAction::new(-1, 0),
                aim: AimDirection::east(),
                primary_held: false,
                primary_sequence: 0,
            })
            .expect("first compact state");
        handle
            .submit_latest_input(CorePrivateMicrorealmRetainedInput {
                input_sequence: 2,
                movement: MovementAction::new(1, 0),
                aim: AimDirection::east(),
                primary_held: true,
                primary_sequence: 1,
            })
            .expect("coalesced compact state");

        for _ in 0..30 {
            advance_one_frame(&mut observer).await;
        }
        let state = observer.latest();
        assert!(matches!(
            state,
            CorePrivateMicrorealmDriverState::Running {
                committed_frames: 30,
                ..
            }
        ));
        {
            let inputs = received.lock().expect("received");
            assert_eq!(inputs.len(), 30);
            assert!(inputs.iter().all(|input| input.input_sequence == 2));
            assert!(inputs.iter().all(|input| input.primary_sequence == 1));
        }

        let report = driver.shutdown().await.expect("joined shutdown");
        assert_eq!(report.committed_frames, 30);
        assert_eq!(report.accepted_input_updates, 2);
        assert!(report.task_joined);
        assert!(!report.driver_task_live_after_join);
    }

    #[tokio::test(start_paused = true)]
    async fn ingress_rejects_regressions_and_reliable_presses_advance_once() {
        let received = Arc::new(StdMutex::new(Vec::new()));
        let driver = spawn_driver(ScriptedRuntime::ordinary(Arc::clone(&received)));
        let handle = driver.handle();
        let mut observer = handle.observe();
        tokio::task::yield_now().await;
        let accepted = CorePrivateMicrorealmRetainedInput {
            input_sequence: 4,
            movement: MovementAction::new(1, 0),
            aim: AimDirection::east(),
            primary_held: true,
            primary_sequence: 3,
        };
        handle
            .submit_latest_input(accepted)
            .expect("accepted input");
        let released = CorePrivateMicrorealmRetainedInput {
            input_sequence: 5,
            primary_held: false,
            primary_sequence: 0,
            ..accepted
        };
        handle
            .submit_latest_input(released)
            .expect("legacy release sequence is normalized");
        assert_eq!(handle.latest_retained_input().primary_sequence, 3);
        assert!(!handle.latest_retained_input().primary_held);
        assert_eq!(
            handle.submit_latest_input(CorePrivateMicrorealmRetainedInput {
                input_sequence: 6,
                primary_held: true,
                primary_sequence: 2,
                ..accepted
            }),
            Err(
                CorePrivateMicrorealmIngressError::PrimarySequenceRegressed {
                    last: 3,
                    received: 2,
                }
            )
        );
        assert_eq!(
            handle.submit_latest_input(accepted),
            Err(CorePrivateMicrorealmIngressError::StaleInputSequence {
                last: 5,
                received: 4,
            })
        );
        let press = CorePrivateMicrorealmAbilityPress {
            action_sequence: 7,
            ability: CorePrivateMicrorealmAbility::Ability2,
        };
        handle.submit_ability_press(press).expect("accepted press");
        assert_eq!(
            handle.submit_ability_press(press),
            Err(CorePrivateMicrorealmIngressError::StaleActionSequence {
                last: 7,
                received: 7,
            })
        );

        advance_one_frame(&mut observer).await;
        advance_one_frame(&mut observer).await;
        {
            let inputs = received.lock().expect("received");
            assert_eq!(inputs.len(), 2);
            assert!(inputs.iter().all(|input| input.ability_2_sequence == 1));
            assert!(inputs.iter().all(|input| input.ability_1_sequence == 0));
        }
        let report = driver.shutdown().await.expect("shutdown");
        assert_eq!(report.accepted_ability_presses, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn link_lost_neutralizes_continuous_intent_without_stopping_danger_ticks() {
        let received = Arc::new(StdMutex::new(Vec::new()));
        let driver = spawn_driver(ScriptedRuntime::ordinary(Arc::clone(&received)));
        let handle = driver.handle();
        let mut observer = handle.observe();
        tokio::task::yield_now().await;
        handle
            .submit_latest_input(CorePrivateMicrorealmRetainedInput {
                input_sequence: 9,
                movement: MovementAction::new(1, 1),
                aim: AimDirection::east(),
                primary_held: true,
                primary_sequence: 2,
            })
            .expect("live input");
        handle
            .submit_ability_press(CorePrivateMicrorealmAbilityPress {
                action_sequence: 1,
                ability: CorePrivateMicrorealmAbility::Ability1,
            })
            .expect("reliable press");
        advance_one_frame(&mut observer).await;
        handle
            .neutralize_for_link_lost()
            .expect("LinkLost neutralization");
        for _ in 0..5 {
            advance_one_frame(&mut observer).await;
        }

        {
            let inputs = received.lock().expect("received");
            assert_eq!(inputs.len(), 6);
            assert_ne!(inputs[0].movement, MovementAction::default());
            assert!(inputs[0].primary_held);
            assert!(
                inputs[1..]
                    .iter()
                    .all(|input| input.movement == MovementAction::default() && !input.primary_held)
            );
            assert!(inputs.iter().all(|input| input.aim == AimDirection::east()));
            assert!(inputs.iter().all(|input| input.ability_1_sequence == 1));
        }
        let report = driver.shutdown().await.expect("shutdown");
        assert_eq!(report.committed_frames, 6);
        assert_eq!(report.link_lost_neutralizations, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn prepared_handoff_freezes_between_frames_and_abort_resumes_exact_owner() {
        let received = Arc::new(StdMutex::new(Vec::new()));
        let mut runtime = ScriptedRuntime::ordinary(Arc::clone(&received));
        runtime.handoff_ready = true;
        let driver = spawn_driver(runtime);
        let handle = driver.handle();
        let mut observer = handle.observe();
        tokio::task::yield_now().await;
        advance_one_frame(&mut observer).await;

        let prepared = driver.prepare_handoff().await.expect("prepared handoff");
        assert_eq!(
            prepared.ready(),
            CorePrivateMicrorealmHandoffReady {
                committed_frames: 1,
                final_tick: Tick(1),
            }
        );
        assert_eq!(
            handle.submit_latest_input(CorePrivateMicrorealmRetainedInput {
                input_sequence: 1,
                movement: MovementAction::new(1, 0),
                aim: AimDirection::east(),
                primary_held: false,
                primary_sequence: 0,
            }),
            Err(CorePrivateMicrorealmIngressError::DriverFrozen)
        );
        tokio::time::advance(DRIVER_TICK_DURATION * 5).await;
        tokio::task::yield_now().await;
        assert_eq!(received.lock().expect("received").len(), 1);
        assert!(matches!(
            handle.advance_fixed_dungeon().await,
            Err(CorePrivateMicrorealmDriverError::FixedDungeonAdvance(message))
                if message == "fixed dungeon is unavailable while Bell resolution is pending"
        ));

        prepared.abort().await.expect("abort handoff");
        handle
            .submit_latest_input(CorePrivateMicrorealmRetainedInput {
                input_sequence: 1,
                movement: MovementAction::new(1, 0),
                aim: AimDirection::east(),
                primary_held: false,
                primary_sequence: 0,
            })
            .expect("resumed ingress");
        advance_one_frame(&mut observer).await;
        assert!(received.lock().expect("received").len() >= 2);
        let report = driver.shutdown().await.expect("shutdown");
        assert_eq!(report.outcome, CorePrivateMicrorealmDriverOutcome::Shutdown);
    }

    #[tokio::test(start_paused = true)]
    async fn handoff_rejects_non_bell_state_without_freezing_ingress() {
        let received = Arc::new(StdMutex::new(Vec::new()));
        let driver = spawn_driver(ScriptedRuntime::ordinary(received));
        tokio::task::yield_now().await;
        assert!(matches!(
            driver.prepare_handoff().await,
            Err(CorePrivateMicrorealmDriverError::HandoffNotReady)
        ));
        driver
            .handle()
            .submit_latest_input(CorePrivateMicrorealmRetainedInput {
                input_sequence: 1,
                movement: MovementAction::default(),
                aim: AimDirection::east(),
                primary_held: false,
                primary_sequence: 0,
            })
            .expect("ingress remains live");
        driver.shutdown().await.expect("shutdown");
    }

    #[tokio::test(start_paused = true)]
    async fn dropped_prepared_handoff_freezes_unknown_durable_outcome_until_shutdown() {
        let received = Arc::new(StdMutex::new(Vec::new()));
        let mut runtime = ScriptedRuntime::ordinary(Arc::clone(&received));
        runtime.handoff_ready = true;
        let driver = spawn_driver(runtime);
        let handle = driver.handle();
        let mut observer = handle.observe();
        tokio::task::yield_now().await;
        advance_one_frame(&mut observer).await;

        let prepared = driver.prepare_handoff().await.expect("prepare");
        drop(prepared);
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        assert!(matches!(
            observer.latest(),
            CorePrivateMicrorealmDriverState::BellResolutionPending {
                committed_frames: 1,
                final_tick: Tick(1),
            }
        ));
        assert_eq!(
            handle.submit_latest_input(CorePrivateMicrorealmRetainedInput {
                input_sequence: 1,
                movement: MovementAction::default(),
                aim: AimDirection::east(),
                primary_held: false,
                primary_sequence: 0,
            }),
            Err(CorePrivateMicrorealmIngressError::DriverFrozen)
        );
        tokio::time::advance(DRIVER_TICK_DURATION * 5).await;
        tokio::task::yield_now().await;
        assert_eq!(received.lock().expect("received").len(), 1);
        let report = driver.shutdown().await.expect("shutdown");
        assert_eq!(
            report.outcome,
            CorePrivateMicrorealmDriverOutcome::BellResolutionPending
        );
    }

    #[tokio::test(start_paused = true)]
    async fn lethal_frame_freezes_exactly_until_joined_shutdown() {
        let received = Arc::new(StdMutex::new(Vec::new()));
        let mut runtime = ScriptedRuntime::ordinary(received);
        runtime.terminal_at = Some(2);
        let driver = spawn_driver(runtime);
        let handle = driver.handle();
        let mut observer = handle.observe();
        tokio::task::yield_now().await;
        advance_one_frame(&mut observer).await;
        advance_one_frame(&mut observer).await;
        assert!(matches!(
            observer.latest(),
            CorePrivateMicrorealmDriverState::TerminalPending {
                committed_frames: 2,
                ref lethal_step,
            } if lethal_step.tick == Tick(2)
        ));
        assert_eq!(
            handle.neutralize_for_link_lost(),
            Err(CorePrivateMicrorealmIngressError::DriverFrozen)
        );
        tokio::time::advance(DRIVER_TICK_DURATION * 10).await;
        tokio::task::yield_now().await;
        assert_eq!(
            observer.latest().latest_step().expect("lethal step").tick,
            Tick(2)
        );
        let report = driver.shutdown().await.expect("shutdown");
        assert_eq!(report.committed_frames, 2);
        assert_eq!(
            report.outcome,
            CorePrivateMicrorealmDriverOutcome::TerminalPending
        );
    }

    #[tokio::test(start_paused = true)]
    async fn route_fault_is_fail_closed_and_shutdown_finishes_an_in_flight_frame() {
        let received = Arc::new(StdMutex::new(Vec::new()));
        let mut faulting = ScriptedRuntime::ordinary(Arc::clone(&received));
        faulting.fault_at = Some(1);
        let fault_driver = spawn_driver(faulting);
        let mut fault_observer = fault_driver.handle().observe();
        tokio::task::yield_now().await;
        advance_one_frame(&mut fault_observer).await;
        assert!(matches!(
            fault_observer.latest(),
            CorePrivateMicrorealmDriverState::Faulted {
                committed_frames: 0,
                fault: CorePrivateMicrorealmDriverFault {
                    kind: CorePrivateMicrorealmFaultKind::RouteAuthority,
                    last_committed_tick: Tick(0),
                    ..
                }
            }
        ));
        let fault_report = fault_driver.shutdown().await.expect("fault shutdown");
        assert_eq!(
            fault_report.outcome,
            CorePrivateMicrorealmDriverOutcome::Faulted
        );

        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let mut blocked = ScriptedRuntime::ordinary(received);
        blocked.entered = Some(Arc::clone(&entered));
        blocked.release = Some(Arc::clone(&release));
        let driver = spawn_driver(blocked);
        tokio::task::yield_now().await;
        tokio::time::advance(DRIVER_TICK_DURATION).await;
        entered.notified().await;
        let shutdown = tokio::spawn(driver.shutdown());
        tokio::task::yield_now().await;
        assert!(!shutdown.is_finished());
        release.notify_one();
        let report = shutdown.await.expect("shutdown task").expect("driver join");
        assert_eq!(report.committed_frames, 1);
        assert_eq!(report.final_tick, Tick(1));
        assert!(report.task_joined);
        assert!(!report.driver_task_live_after_join);
    }
}
