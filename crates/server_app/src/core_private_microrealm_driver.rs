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
    CoreBellPortalTransition, CoreDurableB3Resolution, CoreDurableBargainRestResolution,
    CoreDurableCaldusResolution, CorePrivateCaldusDefeatHandoff, CorePrivateCaldusFrame,
    CorePrivateCaldusRewardCommit, CorePrivateCaldusRuntime, CorePrivateCaldusRuntimeError,
    CorePrivateCaldusRuntimeInput, CorePrivateFixedDungeonAdvance,
    CorePrivateFixedDungeonB3RewardCommit, CorePrivateFixedDungeonLiveRoomFrame,
    CorePrivateFixedDungeonRestCommit, CorePrivateFixedDungeonRuntime,
    CorePrivateFixedDungeonRuntimeError, CorePrivateMicrorealmInput, CorePrivateMicrorealmRuntime,
    CorePrivateMicrorealmRuntimeError, CorePrivateMicrorealmStep, CorePrivatePlayerDamageFactV1,
    CorePrivateTerminalFeedError, CorePrivateTerminalFrameDisposition,
    CorePrivateTerminalFrameSender, CorePrivateTerminalRouteControlAuthorityV1,
    CorePrivateTerminalSceneV1,
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

#[derive(Debug, Clone, Copy, PartialEq)]
struct RetainedFrameInput {
    continuous: CorePrivateMicrorealmRetainedInput,
    ability_1_sequence: u32,
    ability_2_sequence: u32,
    reward_session_active: bool,
    reward_trust_valid: bool,
    reward_activity_sequence: u64,
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
            reward_session_active: self.reward_session_active,
            reward_trust_valid: self.reward_trust_valid,
            reward_activity_sequence: self.reward_activity_sequence,
        }
    }
}

impl Default for RetainedFrameInput {
    fn default() -> Self {
        Self {
            continuous: CorePrivateMicrorealmRetainedInput::default(),
            ability_1_sequence: 0,
            ability_2_sequence: 0,
            reward_session_active: true,
            reward_trust_valid: true,
            reward_activity_sequence: 1,
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
    authoritative_tick: AtomicU64,
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
    fixed_advance_tx: mpsc::Sender<CorePrivateFixedDungeonControlRequest>,
}

impl CorePrivateMicrorealmDriverHandle {
    /// Returns only the latest successfully committed frame tick. Scheduled deadlines and failed
    /// simulation/route attempts never advance this value; zero means the first frame has not
    /// committed yet.
    #[must_use]
    pub(crate) fn authoritative_tick(&self) -> Option<std::num::NonZeroU64> {
        std::num::NonZeroU64::new(self.ingress.authoritative_tick.load(Ordering::Acquire))
    }

    #[must_use]
    pub(crate) fn shares_driver_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.ingress, &other.ingress)
    }

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
            .send(CorePrivateFixedDungeonControlRequest::Advance { result_tx })
            .await
            .map_err(|_| CorePrivateMicrorealmDriverError::FixedDungeonControlClosed)?;
        result_rx
            .await
            .map_err(|_| CorePrivateMicrorealmDriverError::FixedDungeonControlClosed)?
            .map_err(CorePrivateMicrorealmDriverError::FixedDungeonAdvance)
    }

    /// Applies one opaque durable Bargain/no-offer result to B4. The caller cannot author the
    /// resolution fields; only the persistence authority can construct the proof value.
    pub async fn resolve_fixed_dungeon_rest(
        &self,
        durable: CoreDurableBargainRestResolution,
    ) -> Result<CorePrivateFixedDungeonRestCommit, CorePrivateMicrorealmDriverError> {
        let (result_tx, result_rx) = oneshot::channel();
        self.fixed_advance_tx
            .send(CorePrivateFixedDungeonControlRequest::ResolveRest { durable, result_tx })
            .await
            .map_err(|_| CorePrivateMicrorealmDriverError::FixedDungeonControlClosed)?;
        result_rx
            .await
            .map_err(|_| CorePrivateMicrorealmDriverError::FixedDungeonControlClosed)?
            .map_err(CorePrivateMicrorealmDriverError::FixedDungeonRestResolution)
    }

    /// Acknowledges the exact B3 simulation handoff only after both durable personal-item and
    /// progression/milestone terminals exist. The proof is opaque and remains task-owned.
    pub async fn commit_fixed_dungeon_b3_reward(
        &self,
        durable: CoreDurableB3Resolution,
    ) -> Result<CorePrivateFixedDungeonB3RewardCommit, CorePrivateMicrorealmDriverError> {
        let (result_tx, result_rx) = oneshot::channel();
        self.fixed_advance_tx
            .send(CorePrivateFixedDungeonControlRequest::CommitB3Reward {
                durable: Box::new(durable),
                result_tx,
            })
            .await
            .map_err(|_| CorePrivateMicrorealmDriverError::FixedDungeonControlClosed)?;
        result_rx
            .await
            .map_err(|_| CorePrivateMicrorealmDriverError::FixedDungeonControlClosed)?
            .map_err(CorePrivateMicrorealmDriverError::FixedDungeonB3Reward)
    }

    /// Acknowledges the exact frozen Caldus defeat only after personal reward, progression, and
    /// victory-exit terminals are durable. The compiled presentation authority is process-owned.
    pub async fn commit_caldus_reward(
        &self,
        durable: CoreDurableCaldusResolution,
    ) -> Result<CorePrivateCaldusRewardCommit, CorePrivateMicrorealmDriverError> {
        let (result_tx, result_rx) = oneshot::channel();
        self.fixed_advance_tx
            .send(CorePrivateFixedDungeonControlRequest::CommitCaldusReward {
                durable: Box::new(durable),
                result_tx,
            })
            .await
            .map_err(|_| CorePrivateMicrorealmDriverError::FixedDungeonControlClosed)?;
        result_rx
            .await
            .map_err(|_| CorePrivateMicrorealmDriverError::FixedDungeonControlClosed)?
            .map_err(CorePrivateMicrorealmDriverError::CaldusReward)
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
        let previous = reducer.retained.continuous;
        // Release frames in the established session wire contract may carry zero. Preserve the
        // server's maximum accepted sequence so a release cannot re-arm an already consumed shot.
        input.primary_sequence = input.primary_sequence.max(maximum_primary_sequence);
        let meaningful_activity = input.movement != previous.movement
            || input.aim != previous.aim
            || input.primary_held != previous.primary_held
            || input.primary_sequence > previous.primary_sequence;
        reducer.retained.continuous = input;
        if meaningful_activity {
            reducer.retained.reward_activity_sequence = reducer
                .retained
                .reward_activity_sequence
                .checked_add(1)
                .ok_or(CorePrivateMicrorealmIngressError::ActivitySequenceExhausted)?;
        }
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
        reducer.retained.reward_activity_sequence = reducer
            .retained
            .reward_activity_sequence
            .checked_add(1)
            .ok_or(CorePrivateMicrorealmIngressError::ActivitySequenceExhausted)?;
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
        let was_accepting = reducer.accepting;
        reducer.retained.continuous.movement = MovementAction::default();
        reducer.retained.continuous.primary_held = false;
        reducer.retained.reward_session_active = false;
        reducer.retained.reward_trust_valid = false;
        self.ingress.publish_locked(&reducer);
        self.ingress
            .metrics
            .link_lost_neutralizations
            .fetch_add(1, Ordering::Relaxed);
        if was_accepting {
            Ok(())
        } else {
            Err(CorePrivateMicrorealmIngressError::DriverFrozen)
        }
    }

    /// Records a newly authenticated winning transport before its retained danger owner becomes
    /// visible. Reconnect itself is a fresh activity edge; gameplay sequence watermarks remain.
    pub(crate) fn mark_reward_session_active(&self) {
        let mut reducer = self
            .ingress
            .reducer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reducer.retained.reward_session_active = true;
        reducer.retained.reward_trust_valid = true;
        reducer.retained.reward_activity_sequence =
            reducer.retained.reward_activity_sequence.saturating_add(1);
        self.ingress.publish_locked(&reducer);
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

    #[cfg(test)]
    pub(crate) fn from_receiver_for_test(
        receiver: watch::Receiver<CorePrivateMicrorealmDriverState>,
    ) -> Self {
        Self { receiver }
    }
}

/// Fail-closed reason retained when the authoritative frame owner stops advancing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePrivateMicrorealmFaultKind {
    RouteAuthority,
    TickExhausted,
    Simulation,
    TerminalAuthority,
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
    FixedDungeonRewardPending {
        committed_frames: u64,
        frame: Arc<CorePrivateFixedDungeonLiveRoomFrame>,
        reward_handoff: Arc<sim_content::CoreB3RewardHandoff>,
    },
    FixedDungeonTerminalPending {
        committed_frames: u64,
        lethal_frame: Arc<CorePrivateFixedDungeonLiveRoomFrame>,
    },
    CaldusRunning {
        committed_frames: u64,
        frame: Arc<CorePrivateCaldusFrame>,
    },
    CaldusRewardPending {
        committed_frames: u64,
        frame: Arc<CorePrivateCaldusFrame>,
        reward_handoff: Arc<CorePrivateCaldusDefeatHandoff>,
    },
    CaldusTerminalPending {
        committed_frames: u64,
        lethal_frame: Arc<CorePrivateCaldusFrame>,
    },
    CaldusExitReady {
        committed_frames: u64,
        commit: Arc<CorePrivateCaldusRewardCommit>,
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
            | Self::FixedDungeonRewardPending { .. }
            | Self::FixedDungeonTerminalPending { .. }
            | Self::CaldusRunning { .. }
            | Self::CaldusRewardPending { .. }
            | Self::CaldusTerminalPending { .. }
            | Self::CaldusExitReady { .. }
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
    CaldusRewardPending,
    CaldusTerminalPending,
    CaldusExitReady,
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
    caldus_content: Arc<sim_content::CoreDevelopmentCaldus>,
    result_tx: oneshot::Sender<Result<CorePrivateFixedDungeonDriverReady, String>>,
}

#[derive(Debug)]
enum CorePrivateFixedDungeonControlRequest {
    Advance {
        result_tx: oneshot::Sender<Result<CorePrivateFixedDungeonAdvance, String>>,
    },
    ResolveRest {
        durable: CoreDurableBargainRestResolution,
        result_tx: oneshot::Sender<Result<CorePrivateFixedDungeonRestCommit, String>>,
    },
    CommitB3Reward {
        durable: Box<CoreDurableB3Resolution>,
        result_tx: oneshot::Sender<Result<CorePrivateFixedDungeonB3RewardCommit, String>>,
    },
    CommitCaldusReward {
        durable: Box<CoreDurableCaldusResolution>,
        result_tx: oneshot::Sender<Result<CorePrivateCaldusRewardCommit, String>>,
    },
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
        caldus_content: Arc<sim_content::CoreDevelopmentCaldus>,
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
                    caldus_content,
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
    /// Unit-test-only path for driver mechanics that do not exercise terminal delivery. No
    /// production or persistent-session build can construct an ownerless danger driver.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn spawn_without_terminal_owner(runtime: CorePrivateMicrorealmRuntime) -> Self {
        spawn_driver(runtime)
    }

    /// Installs the lossless terminal-history consumer before the first danger frame. The
    /// ordinary `spawn` path remains fail closed for damage-bearing frames until the complete
    /// private-life authority builder supplies this binding.
    #[must_use]
    pub fn spawn_with_terminal_feed(
        runtime: CorePrivateMicrorealmRuntime,
        terminal_feed: CorePrivateTerminalFrameSender,
    ) -> Self {
        spawn_driver_with_terminal_feed(runtime, terminal_feed)
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
    #[error("server-owned reward activity sequence exhausted")]
    ActivitySequenceExhausted,
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
    #[error("Core fixed-dungeon rest resolution failed: {0}")]
    FixedDungeonRestResolution(String),
    #[error("Core fixed-dungeon B3 reward commit failed: {0}")]
    FixedDungeonB3Reward(String),
    #[error("Core Sir Caldus reward commit failed: {0}")]
    CaldusReward(String),
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

#[cfg(test)]
fn spawn_driver<R>(runtime: R) -> CorePrivateMicrorealmDriver
where
    R: MicrorealmFrameRuntime,
{
    spawn_driver_with_terminal_feed(runtime, CorePrivateTerminalFrameSender::unbound())
}

fn spawn_driver_with_terminal_feed<R>(
    runtime: R,
    terminal_feed: CorePrivateTerminalFrameSender,
) -> CorePrivateMicrorealmDriver
where
    R: MicrorealmFrameRuntime,
{
    let (retained_tx, retained_rx) = watch::channel(RetainedFrameInput::default());
    let ingress = Arc::new(SharedIngress {
        reducer: Mutex::new(IngressReducer::default()),
        retained_tx,
        metrics: SharedMetrics::default(),
        authoritative_tick: AtomicU64::new(0),
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
            terminal_feed,
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
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "one select loop owns the full frame-boundary handoff and fail-closed shutdown order"
)]
async fn run_driver<R>(
    mut runtime: R,
    mut terminal_feed: CorePrivateTerminalFrameSender,
    ingress: Arc<SharedIngress>,
    mut retained_rx: watch::Receiver<RetainedFrameInput>,
    observation_tx: watch::Sender<CorePrivateMicrorealmDriverState>,
    mut shutdown_rx: watch::Receiver<bool>,
    mut handoff_rx: mpsc::Receiver<CorePrivateMicrorealmHandoffRequest>,
    mut fixed_advance_rx: mpsc::Receiver<CorePrivateFixedDungeonControlRequest>,
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
                            &mut terminal_feed,
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
                if step.player_died {
                    ingress.stop_accepting();
                }
                let terminal_disposition = acknowledge_terminal_frame(
                    &mut terminal_feed,
                    CorePrivateTerminalFrameView {
                        scene: CorePrivateTerminalSceneV1::Microrealm,
                        route: &step.route,
                        tick: step.tick,
                        player_position: step.player_position,
                        player_died: step.player_died,
                        facts: &step.player_damage,
                    },
                    &mut shutdown_rx,
                )
                .await;
                let terminal_disposition = match terminal_disposition {
                    Ok(disposition) => disposition,
                    Err(error) => {
                        committed_frames = committed_frames.saturating_add(1);
                        final_tick = step.tick;
                        outcome = CorePrivateMicrorealmDriverOutcome::Faulted;
                        ingress.stop_accepting();
                        observation_tx.send_replace(CorePrivateMicrorealmDriverState::Faulted {
                            committed_frames,
                            fault: terminal_feed_fault(&error, final_tick),
                        });
                        wait_for_shutdown(
                            &mut shutdown_rx,
                            &mut handoff_rx,
                            &mut fixed_advance_rx,
                            "private terminal ingestion failed after a microrealm frame commit",
                        )
                        .await;
                        break;
                    }
                };
                committed_frames = committed_frames.saturating_add(1);
                final_tick = step.tick;
                ingress
                    .authoritative_tick
                    .store(final_tick.0, Ordering::Release);
                let step = Arc::new(step);
                if matches!(
                    terminal_disposition,
                    CorePrivateTerminalFrameDisposition::TerminalOwned { .. }
                ) {
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

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "the consuming Bell conversion keeps acknowledgement, publication, and shutdown ordering in one owner"
)]
async fn convert_runtime_and_wait<R>(
    runtime: R,
    request: Box<CorePrivateFixedDungeonConversionRequest>,
    terminal_feed: &mut CorePrivateTerminalFrameSender,
    ingress: &Arc<SharedIngress>,
    observation_tx: &watch::Sender<CorePrivateMicrorealmDriverState>,
    committed_frames: u64,
    final_tick: Tick,
    skipped_deadlines: u64,
    shutdown_rx: &mut watch::Receiver<bool>,
    handoff_rx: &mut mpsc::Receiver<CorePrivateMicrorealmHandoffRequest>,
    fixed_advance_rx: &mut mpsc::Receiver<CorePrivateFixedDungeonControlRequest>,
) -> CorePrivateMicrorealmDriverTaskExit
where
    R: MicrorealmFrameRuntime,
{
    ingress.stop_accepting();
    let CorePrivateFixedDungeonConversionRequest {
        transition,
        expected_content_revision,
        encounters,
        caldus_content,
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
            let route = match fixed_dungeon.route_snapshot() {
                Ok(route) => route,
                Err(error) => {
                    let message = error.to_string();
                    observation_tx.send_replace(CorePrivateMicrorealmDriverState::Faulted {
                        committed_frames,
                        fault: fixed_runtime_fault(&error, final_tick),
                    });
                    let _ = result_tx.send(Err(message));
                    wait_for_shutdown(
                        shutdown_rx,
                        handoff_rx,
                        fixed_advance_rx,
                        "fixed dungeon route snapshot failed after Bell conversion",
                    )
                    .await;
                    return driver_task_exit(
                        ingress,
                        committed_frames,
                        final_tick,
                        skipped_deadlines,
                        CorePrivateMicrorealmDriverOutcome::Faulted,
                    );
                }
            };
            if let Err(error) = acknowledge_terminal_route_control(
                terminal_feed,
                CorePrivateTerminalRouteControlAuthorityV1::BellDungeonEntered {
                    transition: transition.clone(),
                },
                route,
                final_tick,
                shutdown_rx,
            )
            .await
            {
                let message = error.to_string();
                observation_tx.send_replace(CorePrivateMicrorealmDriverState::Faulted {
                    committed_frames,
                    fault: terminal_feed_fault(&error, final_tick),
                });
                let _ = result_tx.send(Err(message));
                wait_for_shutdown(
                    shutdown_rx,
                    handoff_rx,
                    fixed_advance_rx,
                    "Bell conversion committed without terminal control acknowledgement",
                )
                .await;
                return driver_task_exit(
                    ingress,
                    committed_frames,
                    final_tick,
                    skipped_deadlines,
                    CorePrivateMicrorealmDriverOutcome::Faulted,
                );
            }
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
            Box::pin(run_fixed_dungeon(
                fixed_dungeon,
                caldus_content,
                ready,
                terminal_feed,
                ingress,
                observation_tx,
                committed_frames,
                final_tick,
                skipped_deadlines,
                shutdown_rx,
                handoff_rx,
                fixed_advance_rx,
            ))
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
    Control(Option<CorePrivateFixedDungeonControlRequest>),
    Frame(Instant),
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn run_fixed_dungeon(
    mut runtime: CorePrivateFixedDungeonRuntime,
    caldus_content: Arc<sim_content::CoreDevelopmentCaldus>,
    mut ready: CorePrivateFixedDungeonDriverReady,
    terminal_feed: &mut CorePrivateTerminalFrameSender,
    ingress: &Arc<SharedIngress>,
    observation_tx: &watch::Sender<CorePrivateMicrorealmDriverState>,
    mut committed_frames: u64,
    mut final_tick: Tick,
    mut skipped_deadlines: u64,
    shutdown_rx: &mut watch::Receiver<bool>,
    handoff_rx: &mut mpsc::Receiver<CorePrivateMicrorealmHandoffRequest>,
    fixed_advance_rx: &mut mpsc::Receiver<CorePrivateFixedDungeonControlRequest>,
) -> CorePrivateMicrorealmDriverTaskExit {
    let mut retained_rx = ingress.retained_tx.subscribe();
    let mut interval = fixed_driver_interval().await;
    let mut outcome = CorePrivateMicrorealmDriverOutcome::FixedDungeonReady;
    let mut b3_reward_pending = false;
    let mut last_fixed_frame: Option<Arc<CorePrivateFixedDungeonLiveRoomFrame>> = None;

    loop {
        let event = tokio::select! {
            biased;
            changed = shutdown_rx.changed() => {
                let _ = changed;
                FixedDungeonDriverEvent::Shutdown
            }
            request = handoff_rx.recv() => FixedDungeonDriverEvent::Handoff(request),
            request = fixed_advance_rx.recv() => FixedDungeonDriverEvent::Control(request),
            deadline = interval.tick(), if runtime.room_phase().is_some() && !b3_reward_pending => {
                FixedDungeonDriverEvent::Frame(deadline)
            }
        };

        match event {
            FixedDungeonDriverEvent::Shutdown => break,
            FixedDungeonDriverEvent::Handoff(Some(request)) => {
                let _ = request.ready_tx.send(Err(()));
            }
            FixedDungeonDriverEvent::Handoff(None) | FixedDungeonDriverEvent::Control(None) => {
                break;
            }
            FixedDungeonDriverEvent::Control(Some(
                CorePrivateFixedDungeonControlRequest::Advance { result_tx },
            )) => {
                ingress.stop_accepting();
                match runtime.advance().await {
                    Ok(advance) => {
                        if let Err(error) = acknowledge_terminal_route_control(
                            terminal_feed,
                            CorePrivateTerminalRouteControlAuthorityV1::FixedDungeonAdvanced {
                                transition: advance.transition,
                            },
                            advance.route.clone(),
                            final_tick,
                            shutdown_rx,
                        )
                        .await
                        {
                            let message = error.to_string();
                            ingress.stop_accepting();
                            observation_tx.send_replace(
                                CorePrivateMicrorealmDriverState::Faulted {
                                    committed_frames,
                                    fault: terminal_feed_fault(&error, final_tick),
                                },
                            );
                            let _ = result_tx.send(Err(message));
                            wait_for_shutdown(
                                shutdown_rx,
                                handoff_rx,
                                fixed_advance_rx,
                                "fixed route control committed without terminal acknowledgement",
                            )
                            .await;
                            return driver_task_exit(
                                ingress,
                                committed_frames,
                                final_tick,
                                skipped_deadlines,
                                CorePrivateMicrorealmDriverOutcome::Faulted,
                            );
                        }
                        ready.node = advance.transition.to;
                        if ready.node == sim_content::CoreFixedDungeonNode::CaldusArenaB6 {
                            let caldus = runtime
                                .into_caldus_staging_handoff()
                                .map_err(|error| error.to_string())
                                .and_then(|handoff| {
                                    CorePrivateCaldusRuntime::from_staging_handoff(handoff)
                                        .map_err(|error| error.to_string())
                                });
                            return match caldus {
                                Ok(caldus) => {
                                    final_tick = caldus.tick();
                                    ingress.neutralize_for_scene_transition();
                                    ingress.resume_accepting();
                                    observation_tx.send_replace(
                                        CorePrivateMicrorealmDriverState::FixedDungeonReady {
                                            ready,
                                        },
                                    );
                                    let _ = result_tx.send(Ok(advance));
                                    run_caldus(
                                        caldus,
                                        caldus_content,
                                        terminal_feed,
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
                                    ingress.stop_accepting();
                                    observation_tx.send_replace(
                                        CorePrivateMicrorealmDriverState::Faulted {
                                            committed_frames,
                                            fault: CorePrivateMicrorealmDriverFault {
                                                kind:
                                                    CorePrivateMicrorealmFaultKind::RouteAuthority,
                                                message: Arc::from(message.clone()),
                                                last_committed_tick: final_tick,
                                            },
                                        },
                                    );
                                    let _ = result_tx.send(Err(message));
                                    wait_for_shutdown(
                                        shutdown_rx,
                                        handoff_rx,
                                        fixed_advance_rx,
                                        "Caldus conversion failed after B6 route commit",
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
                            };
                        }
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
                        let _ = result_tx.send(Ok(advance));
                    }
                    Err(error) => {
                        let message = error.to_string();
                        let fatal = fixed_advance_error_is_fatal(&error);
                        let _ = result_tx.send(Err(message));
                        if fatal {
                            outcome = CorePrivateMicrorealmDriverOutcome::Faulted;
                            ingress.stop_accepting();
                            observation_tx.send_replace(
                                CorePrivateMicrorealmDriverState::Faulted {
                                    committed_frames,
                                    fault: fixed_runtime_fault(&error, final_tick),
                                },
                            );
                            wait_for_shutdown(
                                shutdown_rx,
                                handoff_rx,
                                fixed_advance_rx,
                                "fixed dungeon cannot advance after a route fault",
                            )
                            .await;
                            break;
                        } else if runtime.room_phase().is_some() {
                            ingress.resume_accepting();
                        }
                    }
                }
            }
            FixedDungeonDriverEvent::Control(Some(
                CorePrivateFixedDungeonControlRequest::ResolveRest { durable, result_tx },
            )) => match runtime.resolve_rest(&durable).await {
                Ok(commit) => {
                    if commit.receipt == sim_content::CoreFixedDungeonRestReceipt::Committed
                        && let Err(error) = acknowledge_terminal_route_control(
                            terminal_feed,
                            CorePrivateTerminalRouteControlAuthorityV1::B4RestResolved {
                                durable: durable.clone(),
                                commit: commit.clone(),
                            },
                            commit.route.clone(),
                            final_tick,
                            shutdown_rx,
                        )
                        .await
                    {
                        let message = error.to_string();
                        ingress.stop_accepting();
                        observation_tx.send_replace(CorePrivateMicrorealmDriverState::Faulted {
                            committed_frames,
                            fault: terminal_feed_fault(&error, final_tick),
                        });
                        let _ = result_tx.send(Err(message));
                        wait_for_shutdown(
                            shutdown_rx,
                            handoff_rx,
                            fixed_advance_rx,
                            "B4 result committed without terminal acknowledgement",
                        )
                        .await;
                        return driver_task_exit(
                            ingress,
                            committed_frames,
                            final_tick,
                            skipped_deadlines,
                            CorePrivateMicrorealmDriverOutcome::Faulted,
                        );
                    }
                    observation_tx.send_replace(
                        CorePrivateMicrorealmDriverState::FixedDungeonReady { ready },
                    );
                    let _ = result_tx.send(Ok(commit));
                }
                Err(error) => {
                    let message = error.to_string();
                    let fatal = fixed_rest_error_is_fatal(&error);
                    let _ = result_tx.send(Err(message));
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
                            "fixed dungeon cannot resolve B4 after a route fault",
                        )
                        .await;
                        break;
                    }
                }
            },
            FixedDungeonDriverEvent::Control(Some(
                CorePrivateFixedDungeonControlRequest::CommitB3Reward { durable, result_tx },
            )) => match runtime.commit_b3_reward(&durable).await {
                Ok(commit) => {
                    if commit.receipt == sim_content::CoreB3RewardReceipt::Committed
                        && let Err(error) = acknowledge_terminal_route_control(
                            terminal_feed,
                            CorePrivateTerminalRouteControlAuthorityV1::B3RewardCommitted {
                                durable: (*durable).clone(),
                                commit: commit.clone(),
                            },
                            commit.route.clone(),
                            final_tick,
                            shutdown_rx,
                        )
                        .await
                    {
                        let message = error.to_string();
                        ingress.stop_accepting();
                        observation_tx.send_replace(CorePrivateMicrorealmDriverState::Faulted {
                            committed_frames,
                            fault: terminal_feed_fault(&error, final_tick),
                        });
                        let _ = result_tx.send(Err(message));
                        wait_for_shutdown(
                            shutdown_rx,
                            handoff_rx,
                            fixed_advance_rx,
                            "B3 reward committed without terminal acknowledgement",
                        )
                        .await;
                        return driver_task_exit(
                            ingress,
                            committed_frames,
                            final_tick,
                            skipped_deadlines,
                            CorePrivateMicrorealmDriverOutcome::Faulted,
                        );
                    }
                    b3_reward_pending = false;
                    ingress.resume_accepting();
                    interval.reset();
                    if let Some(frame) = last_fixed_frame.as_deref().cloned() {
                        let mut frame = frame;
                        frame.route = commit.route.clone();
                        let frame = Arc::new(frame);
                        last_fixed_frame = Some(Arc::clone(&frame));
                        observation_tx.send_replace(
                            CorePrivateMicrorealmDriverState::FixedDungeonRunning {
                                committed_frames,
                                frame,
                            },
                        );
                    }
                    let _ = result_tx.send(Ok(commit));
                }
                Err(error) => {
                    let message = error.to_string();
                    let fatal = fixed_b3_reward_error_is_fatal(&error);
                    let _ = result_tx.send(Err(message));
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
                            "fixed dungeon cannot commit B3 reward after a route fault",
                        )
                        .await;
                        break;
                    }
                }
            },
            FixedDungeonDriverEvent::Control(Some(
                CorePrivateFixedDungeonControlRequest::CommitCaldusReward { result_tx, .. },
            )) => {
                let _ = result_tx.send(Err("Sir Caldus is not installed".to_owned()));
            }
            FixedDungeonDriverEvent::Frame(deadline) => {
                let lateness = Instant::now().saturating_duration_since(deadline);
                let missed = lateness.as_nanos() / u128::from(DRIVER_TICK_NANOS);
                skipped_deadlines =
                    skipped_deadlines.saturating_add(u64::try_from(missed).unwrap_or(u64::MAX));
                let retained = *retained_rx.borrow_and_update();
                match runtime.step_live_room(retained.runtime_input()).await {
                    Ok(frame) => {
                        if frame.player_died {
                            ingress.stop_accepting();
                        }
                        let terminal_disposition = acknowledge_terminal_frame(
                            terminal_feed,
                            CorePrivateTerminalFrameView {
                                scene: CorePrivateTerminalSceneV1::FixedDungeon,
                                route: &frame.route,
                                tick: frame.tick,
                                player_position: frame.player_position,
                                player_died: frame.player_died,
                                facts: &frame.player_damage,
                            },
                            shutdown_rx,
                        )
                        .await;
                        let terminal_disposition = match terminal_disposition {
                            Ok(disposition) => disposition,
                            Err(error) => {
                                committed_frames = committed_frames.saturating_add(1);
                                final_tick = frame.tick;
                                outcome = CorePrivateMicrorealmDriverOutcome::Faulted;
                                ingress.stop_accepting();
                                observation_tx.send_replace(
                                    CorePrivateMicrorealmDriverState::Faulted {
                                        committed_frames,
                                        fault: terminal_feed_fault(&error, final_tick),
                                    },
                                );
                                wait_for_shutdown(
                                    shutdown_rx,
                                    handoff_rx,
                                    fixed_advance_rx,
                                    "private terminal ingestion failed after a fixed-room frame commit",
                                )
                                .await;
                                break;
                            }
                        };
                        committed_frames = committed_frames.saturating_add(1);
                        final_tick = frame.tick;
                        ingress
                            .authoritative_tick
                            .store(final_tick.0, Ordering::Release);
                        let frame = Arc::new(frame);
                        if matches!(
                            terminal_disposition,
                            CorePrivateTerminalFrameDisposition::TerminalOwned { .. }
                        ) {
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
                        last_fixed_frame = Some(Arc::clone(&frame));
                        if let Some(reward) = runtime.pending_b3_reward_handoff()
                            && frame.tick >= reward.reward_due_tick
                        {
                            b3_reward_pending = true;
                            ingress.stop_accepting();
                            observation_tx.send_replace(
                                CorePrivateMicrorealmDriverState::FixedDungeonRewardPending {
                                    committed_frames,
                                    frame,
                                    reward_handoff: Arc::new(reward.clone()),
                                },
                            );
                        } else {
                            observation_tx.send_replace(
                                CorePrivateMicrorealmDriverState::FixedDungeonRunning {
                                    committed_frames,
                                    frame,
                                },
                            );
                        }
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

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn run_caldus(
    mut runtime: CorePrivateCaldusRuntime,
    content: Arc<sim_content::CoreDevelopmentCaldus>,
    terminal_feed: &mut CorePrivateTerminalFrameSender,
    ingress: &Arc<SharedIngress>,
    observation_tx: &watch::Sender<CorePrivateMicrorealmDriverState>,
    mut committed_frames: u64,
    mut final_tick: Tick,
    mut skipped_deadlines: u64,
    shutdown_rx: &mut watch::Receiver<bool>,
    handoff_rx: &mut mpsc::Receiver<CorePrivateMicrorealmHandoffRequest>,
    fixed_advance_rx: &mut mpsc::Receiver<CorePrivateFixedDungeonControlRequest>,
) -> CorePrivateMicrorealmDriverTaskExit {
    let mut retained_rx = ingress.retained_tx.subscribe();
    let mut interval = fixed_driver_interval().await;
    let mut outcome = CorePrivateMicrorealmDriverOutcome::Shutdown;
    let mut reward_pending = false;
    let mut exit_ready = false;

    loop {
        let event = tokio::select! {
            biased;
            changed = shutdown_rx.changed() => {
                let _ = changed;
                FixedDungeonDriverEvent::Shutdown
            }
            request = handoff_rx.recv() => FixedDungeonDriverEvent::Handoff(request),
            request = fixed_advance_rx.recv() => FixedDungeonDriverEvent::Control(request),
            deadline = interval.tick(), if !reward_pending && !exit_ready => {
                FixedDungeonDriverEvent::Frame(deadline)
            }
        };

        match event {
            FixedDungeonDriverEvent::Shutdown => break,
            FixedDungeonDriverEvent::Handoff(Some(request)) => {
                let _ = request.ready_tx.send(Err(()));
            }
            FixedDungeonDriverEvent::Handoff(None) | FixedDungeonDriverEvent::Control(None) => {
                break;
            }
            FixedDungeonDriverEvent::Control(Some(
                CorePrivateFixedDungeonControlRequest::CommitCaldusReward { durable, result_tx },
            )) => match runtime
                .commit_reward_resolution(&content, (*durable).clone())
                .await
            {
                Ok(commit) => {
                    if commit.disposition
                        == crate::CorePrivateCaldusRewardCommitDisposition::Committed
                        && let Err(error) = acknowledge_terminal_route_control(
                            terminal_feed,
                            CorePrivateTerminalRouteControlAuthorityV1::CaldusRewardCommitted {
                                durable: *durable,
                                commit: commit.clone(),
                            },
                            commit.route.clone(),
                            final_tick,
                            shutdown_rx,
                        )
                        .await
                    {
                        let message = error.to_string();
                        ingress.stop_accepting();
                        observation_tx.send_replace(CorePrivateMicrorealmDriverState::Faulted {
                            committed_frames,
                            fault: terminal_feed_fault(&error, final_tick),
                        });
                        let _ = result_tx.send(Err(message));
                        wait_for_shutdown(
                            shutdown_rx,
                            handoff_rx,
                            fixed_advance_rx,
                            "Caldus reward committed without terminal acknowledgement",
                        )
                        .await;
                        return driver_task_exit(
                            ingress,
                            committed_frames,
                            final_tick,
                            skipped_deadlines,
                            CorePrivateMicrorealmDriverOutcome::Faulted,
                        );
                    }
                    reward_pending = false;
                    exit_ready = true;
                    outcome = CorePrivateMicrorealmDriverOutcome::CaldusExitReady;
                    ingress.stop_accepting();
                    let commit = Arc::new(commit);
                    observation_tx.send_replace(
                        CorePrivateMicrorealmDriverState::CaldusExitReady {
                            committed_frames,
                            commit: Arc::clone(&commit),
                        },
                    );
                    let _ = result_tx.send(Ok((*commit).clone()));
                }
                Err(error) => {
                    let _ = result_tx.send(Err(error.to_string()));
                    if caldus_reward_error_is_fatal(&error) {
                        outcome = CorePrivateMicrorealmDriverOutcome::Faulted;
                        ingress.stop_accepting();
                        observation_tx.send_replace(CorePrivateMicrorealmDriverState::Faulted {
                            committed_frames,
                            fault: caldus_runtime_fault(&error, final_tick),
                        });
                        wait_for_shutdown(
                            shutdown_rx,
                            handoff_rx,
                            fixed_advance_rx,
                            "Caldus cannot advance after reward acknowledgement failed",
                        )
                        .await;
                        break;
                    }
                }
            },
            FixedDungeonDriverEvent::Control(Some(request)) => {
                reject_fixed_advance(request, "only Caldus reward acknowledgement is available");
            }
            FixedDungeonDriverEvent::Frame(deadline) => {
                let lateness = Instant::now().saturating_duration_since(deadline);
                let missed = lateness.as_nanos() / u128::from(DRIVER_TICK_NANOS);
                skipped_deadlines =
                    skipped_deadlines.saturating_add(u64::try_from(missed).unwrap_or(u64::MAX));
                let retained = *retained_rx.borrow_and_update();
                match runtime.step(caldus_runtime_input(retained)).await {
                    Ok(frame) => {
                        if frame.player_died {
                            ingress.stop_accepting();
                        }
                        let terminal_disposition = acknowledge_terminal_frame(
                            terminal_feed,
                            CorePrivateTerminalFrameView {
                                scene: CorePrivateTerminalSceneV1::Caldus,
                                route: &frame.route,
                                tick: frame.tick,
                                player_position: frame.player_position,
                                player_died: frame.player_died,
                                facts: &frame.player_damage,
                            },
                            shutdown_rx,
                        )
                        .await;
                        let terminal_disposition = match terminal_disposition {
                            Ok(disposition) => disposition,
                            Err(error) => {
                                committed_frames = committed_frames.saturating_add(1);
                                final_tick = frame.tick;
                                outcome = CorePrivateMicrorealmDriverOutcome::Faulted;
                                ingress.stop_accepting();
                                observation_tx.send_replace(
                                    CorePrivateMicrorealmDriverState::Faulted {
                                        committed_frames,
                                        fault: terminal_feed_fault(&error, final_tick),
                                    },
                                );
                                wait_for_shutdown(
                                    shutdown_rx,
                                    handoff_rx,
                                    fixed_advance_rx,
                                    "private terminal ingestion failed after a Caldus frame commit",
                                )
                                .await;
                                break;
                            }
                        };
                        committed_frames = committed_frames.saturating_add(1);
                        final_tick = frame.tick;
                        ingress
                            .authoritative_tick
                            .store(final_tick.0, Ordering::Release);
                        let frame = Arc::new(frame);
                        if matches!(
                            terminal_disposition,
                            CorePrivateTerminalFrameDisposition::TerminalOwned { .. }
                        ) {
                            outcome = CorePrivateMicrorealmDriverOutcome::CaldusTerminalPending;
                            ingress.stop_accepting();
                            observation_tx.send_replace(
                                CorePrivateMicrorealmDriverState::CaldusTerminalPending {
                                    committed_frames,
                                    lethal_frame: frame,
                                },
                            );
                            wait_for_shutdown(
                                shutdown_rx,
                                handoff_rx,
                                fixed_advance_rx,
                                "Caldus cannot advance after a terminal frame",
                            )
                            .await;
                            break;
                        }
                        if let Some(handoff) = runtime.pending_reward_handoff() {
                            reward_pending = true;
                            outcome = CorePrivateMicrorealmDriverOutcome::CaldusRewardPending;
                            ingress.stop_accepting();
                            observation_tx.send_replace(
                                CorePrivateMicrorealmDriverState::CaldusRewardPending {
                                    committed_frames,
                                    frame,
                                    reward_handoff: Arc::new(handoff.clone()),
                                },
                            );
                        } else {
                            observation_tx.send_replace(
                                CorePrivateMicrorealmDriverState::CaldusRunning {
                                    committed_frames,
                                    frame,
                                },
                            );
                        }
                    }
                    Err(error) => {
                        outcome = CorePrivateMicrorealmDriverOutcome::Faulted;
                        ingress.stop_accepting();
                        observation_tx.send_replace(CorePrivateMicrorealmDriverState::Faulted {
                            committed_frames,
                            fault: caldus_runtime_fault(&error, final_tick),
                        });
                        wait_for_shutdown(
                            shutdown_rx,
                            handoff_rx,
                            fixed_advance_rx,
                            "Caldus cannot advance after a driver fault",
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

fn caldus_runtime_input(retained: RetainedFrameInput) -> CorePrivateCaldusRuntimeInput {
    CorePrivateCaldusRuntimeInput {
        action: retained.runtime_input(),
        connection: if retained.reward_session_active {
            sim_core::CoreBossConnectionState::ConnectedLoaded
        } else {
            sim_core::CoreBossConnectionState::Disconnected
        },
    }
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
    fixed_advance_rx: &mut mpsc::Receiver<CorePrivateFixedDungeonControlRequest>,
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
    fixed_advance_rx: &mut mpsc::Receiver<CorePrivateFixedDungeonControlRequest>,
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
    fixed_advance_rx: &mut mpsc::Receiver<CorePrivateFixedDungeonControlRequest>,
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

fn reject_fixed_advance(request: CorePrivateFixedDungeonControlRequest, message: &'static str) {
    match request {
        CorePrivateFixedDungeonControlRequest::Advance { result_tx } => {
            let _ = result_tx.send(Err(message.to_owned()));
        }
        CorePrivateFixedDungeonControlRequest::ResolveRest { result_tx, .. } => {
            let _ = result_tx.send(Err(message.to_owned()));
        }
        CorePrivateFixedDungeonControlRequest::CommitB3Reward { result_tx, .. } => {
            let _ = result_tx.send(Err(message.to_owned()));
        }
        CorePrivateFixedDungeonControlRequest::CommitCaldusReward { result_tx, .. } => {
            let _ = result_tx.send(Err(message.to_owned()));
        }
    }
}

struct CorePrivateTerminalFrameView<'a> {
    scene: CorePrivateTerminalSceneV1,
    route: &'a protocol::CorePrivateRouteStateV1,
    tick: Tick,
    player_position: sim_core::TilePoint,
    player_died: bool,
    facts: &'a [CorePrivatePlayerDamageFactV1],
}

async fn acknowledge_terminal_frame(
    feed: &mut CorePrivateTerminalFrameSender,
    frame: CorePrivateTerminalFrameView<'_>,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> Result<CorePrivateTerminalFrameDisposition, CorePrivateTerminalFeedError> {
    let delivery = feed.deliver(
        frame.scene,
        frame.route.clone(),
        frame.tick,
        frame.player_position,
        frame.facts.to_vec(),
        frame.player_died,
    );
    tokio::pin!(delivery);
    tokio::select! {
        biased;
        result = &mut delivery => result,
        changed = shutdown_rx.changed() => {
            let _ = changed;
            Err(CorePrivateTerminalFeedError::ShutdownWithUnresolvedFrame)
        }
    }
}

async fn acknowledge_terminal_route_control(
    feed: &mut CorePrivateTerminalFrameSender,
    authority: CorePrivateTerminalRouteControlAuthorityV1,
    route: protocol::CorePrivateRouteStateV1,
    tick: Tick,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> Result<(), CorePrivateTerminalFeedError> {
    let delivery = feed.deliver_route_control(authority, route, tick);
    tokio::pin!(delivery);
    let disposition = tokio::select! {
        biased;
        result = &mut delivery => result?,
        changed = shutdown_rx.changed() => {
            let _ = changed;
            return Err(CorePrivateTerminalFeedError::ShutdownWithUnresolvedFrame);
        }
    };
    if disposition == CorePrivateTerminalFrameDisposition::Continue {
        Ok(())
    } else {
        Err(CorePrivateTerminalFeedError::InvalidDisposition)
    }
}

fn terminal_feed_fault(
    error: &CorePrivateTerminalFeedError,
    final_tick: Tick,
) -> CorePrivateMicrorealmDriverFault {
    CorePrivateMicrorealmDriverFault {
        kind: CorePrivateMicrorealmFaultKind::TerminalAuthority,
        message: Arc::from(error.to_string()),
        last_committed_tick: final_tick,
    }
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
                | sim_content::CoreFixedDungeonError::RestResolutionRequired
                | sim_content::CoreFixedDungeonError::FixedRoom(
                    sim_content::CoreFixedRoomEncounterError::RoomHandoffUnavailable,
                ),
        )
    )
}

fn fixed_b3_reward_error_is_fatal(error: &CorePrivateFixedDungeonRuntimeError) -> bool {
    !matches!(
        error,
        CorePrivateFixedDungeonRuntimeError::B3RewardAuthorityMismatch
            | CorePrivateFixedDungeonRuntimeError::Dungeon(
                sim_content::CoreFixedDungeonError::B3RewardUnavailable
                    | sim_content::CoreFixedDungeonError::FixedRoom(
                        sim_content::CoreFixedRoomEncounterError::B3RewardUnavailable
                            | sim_content::CoreFixedRoomEncounterError::B3RewardConflict,
                    ),
            )
    )
}

fn fixed_rest_error_is_fatal(error: &CorePrivateFixedDungeonRuntimeError) -> bool {
    !matches!(
        error,
        CorePrivateFixedDungeonRuntimeError::BargainAuthorityMismatch
            | CorePrivateFixedDungeonRuntimeError::Dungeon(
                sim_content::CoreFixedDungeonError::RestResolutionUnavailable
                    | sim_content::CoreFixedDungeonError::RestResolutionConflict,
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

fn caldus_runtime_fault(
    error: &CorePrivateCaldusRuntimeError,
    last_committed_tick: Tick,
) -> CorePrivateMicrorealmDriverFault {
    let kind = match error {
        CorePrivateCaldusRuntimeError::RouteAuthorityMismatch
        | CorePrivateCaldusRuntimeError::RewardAuthorityMismatch
        | CorePrivateCaldusRuntimeError::Route(_) => CorePrivateMicrorealmFaultKind::RouteAuthority,
        CorePrivateCaldusRuntimeError::TickExhausted => {
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

fn caldus_reward_error_is_fatal(error: &CorePrivateCaldusRuntimeError) -> bool {
    !matches!(
        error,
        CorePrivateCaldusRuntimeError::RewardResolutionUnavailable
            | CorePrivateCaldusRuntimeError::RewardResolutionConflict
            | CorePrivateCaldusRuntimeError::RewardAuthorityMismatch
    )
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex as StdMutex};

    use crate::{CorePrivateTerminalFrameReceiver, TerminalBinding, TerminalKind};
    use protocol::{
        CORE_PRIVATE_ROUTE_SCHEMA_VERSION, CorePrivateRouteContentRevisionV1,
        CorePrivateRoutePhaseV1, CorePrivateRouteReadinessV1, CorePrivateRouteRoomV1,
        CorePrivateRouteSceneV1, CorePrivateRouteStateV1, ManifestHash, WorldFlowContentRevisionV1,
    };
    use tokio::sync::Notify;

    use super::*;

    fn hash(byte: char) -> ManifestHash {
        ManifestHash::new(byte.to_string().repeat(64)).expect("valid hash")
    }

    fn spawn_caldus_test_driver(
        runtime: CorePrivateCaldusRuntime,
        content: Arc<sim_content::CoreDevelopmentCaldus>,
    ) -> CorePrivateMicrorealmDriver {
        let (retained_tx, _) = watch::channel(RetainedFrameInput::default());
        let ingress = Arc::new(SharedIngress {
            reducer: Mutex::new(IngressReducer::default()),
            retained_tx,
            metrics: SharedMetrics::default(),
            authoritative_tick: AtomicU64::new(0),
            task_live: AtomicBool::new(true),
        });
        let (observation_tx, observation_rx) =
            watch::channel(CorePrivateMicrorealmDriverState::Starting);
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let (handoff_tx, mut handoff_rx) = mpsc::channel(1);
        let (fixed_advance_tx, mut fixed_advance_rx) = mpsc::channel(1);
        ACTIVE_CORE_MICROREALM_DRIVER_TASKS.fetch_add(1, Ordering::AcqRel);
        let task_ingress = Arc::clone(&ingress);
        let final_tick = runtime.tick();
        let join = tokio::spawn(async move {
            let _task_guard = ActiveDriverTaskGuard {
                ingress: Arc::clone(&task_ingress),
            };
            let mut terminal_feed = CorePrivateTerminalFrameSender::unbound();
            run_caldus(
                runtime,
                content,
                &mut terminal_feed,
                &task_ingress,
                &observation_tx,
                0,
                final_tick,
                0,
                &mut shutdown_rx,
                &mut handoff_rx,
                &mut fixed_advance_rx,
            )
            .await
        });
        CorePrivateMicrorealmDriver {
            handle: CorePrivateMicrorealmDriverHandle {
                ingress,
                observation_rx,
                handoff_tx,
                fixed_advance_tx,
            },
            shutdown_tx,
            join: Some(join),
        }
    }

    fn no_offer_authority() -> CoreDurableBargainRestResolution {
        CoreDurableBargainRestResolution::from_no_offer_milestone(
            crate::AuthenticatedAccount {
                account_id: crate::AccountId::new([0x11; 16]).unwrap(),
                namespace: crate::AuthenticatedNamespace::WipeableTest,
            },
            &persistence::StoredBargainMilestoneResult {
                account_id: [0x11; 16],
                character_id: [0x22; 16],
                source_reward_event_id: [0x44; 16],
                payload_hash: [0x45; 32],
                result_code: 2,
                pre_oath_bargain_version: 1,
                post_oath_bargain_version: 1,
                pre_earned_bargain_slots: 0,
                post_earned_bargain_slots: 0,
                offer_id: None,
                ash_mutation_id: Some([0x44; 16]),
                milestone_id: persistence::CORE_BARGAIN_MILESTONE_ID.into(),
                source_content_id: persistence::CORE_BARGAIN_SOURCE_ID.into(),
                source_layout_id: persistence::CORE_BARGAIN_LAYOUT_ID.into(),
                instance_lineage_id: [0x33; 16],
                entry_restore_point_id: [0x46; 16],
                result_payload: vec![1],
            },
        )
        .unwrap()
    }

    #[tokio::test(start_paused = true)]
    async fn same_driver_task_advances_route_bound_caldus_at_thirty_hertz() {
        let (directory, runtime) =
            crate::core_private_caldus_runtime::core_private_caldus_runtime_test_fixture();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let content = Arc::new(sim_content::load_core_development_caldus(&root).unwrap());
        let driver = spawn_caldus_test_driver(runtime, content);
        let mut observer = driver.handle().observe();

        tokio::time::advance(Duration::from_millis(34)).await;
        let state = observer.changed().await.expect("first Caldus frame");
        assert!(matches!(
            state,
            CorePrivateMicrorealmDriverState::CaldusRunning {
                committed_frames: 1,
                ref frame,
            } if frame.tick == Tick(1)
                && frame.route.phase == protocol::CorePrivateRoutePhaseV1::BossReadyCountdown
        ));

        let report = driver.shutdown().await.expect("joined driver");
        assert_eq!(report.committed_frames, 1);
        assert_eq!(report.final_tick, Tick(1));
        assert_eq!(report.outcome, CorePrivateMicrorealmDriverOutcome::Shutdown);
        directory.begin_shutdown();
        assert!(directory.finish_shutdown().await.unwrap().zero_residue);
    }

    #[test]
    fn stale_caldus_reward_acknowledgements_remain_nonfatal() {
        assert!(!caldus_reward_error_is_fatal(
            &CorePrivateCaldusRuntimeError::RewardResolutionUnavailable
        ));
        assert!(!caldus_reward_error_is_fatal(
            &CorePrivateCaldusRuntimeError::RewardResolutionConflict
        ));
        assert!(!caldus_reward_error_is_fatal(
            &CorePrivateCaldusRuntimeError::RewardAuthorityMismatch
        ));
        assert!(caldus_reward_error_is_fatal(
            &CorePrivateCaldusRuntimeError::InvalidComposition
        ));
    }

    fn b3_reward_authority() -> CoreDurableB3Resolution {
        let authenticated = crate::AuthenticatedAccount {
            account_id: crate::AccountId::new([0x11; 16]).unwrap(),
            namespace: crate::AuthenticatedNamespace::WipeableTest,
        };
        crate::CoreDurableB3RewardCommit::test_fixture(
            authenticated,
            [0x22; 16],
            [0x33; 16],
            sim_content::CoreB3RewardHandoff {
                activation_ordinal: 1,
                instance_id: sim_core::SpawnInstanceId {
                    run_ordinal: 1,
                    spawn_ordinal: 51,
                },
                actor_id: sim_core::EntityId::new(100).unwrap(),
                participant_id: sim_core::EntityId::new(900).unwrap(),
                death_tick: Tick(100),
                reward_due_tick: Tick(108),
                reward_profile_id: "reward.miniboss_t1".into(),
                xp_profile_id: "xp.miniboss_t1".into(),
                active_ticks: 100,
                present_ticks: 100,
                direct_damage: 1_600,
                reference_health: 1_600,
                longest_inactivity_ticks: 0,
                life_state: sim_core::RewardLifeState::Living,
                recall_state: sim_core::RewardRecallState::Eligible,
                trust_state: sim_core::RewardTrustState::Valid,
            },
        )
        .into()
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
            player_damage: Vec::new(),
            pack_clear: None,
            player_died: false,
            bell_portal_in_range: false,
        }
    }

    fn lethal_damage_fact(tick: Tick) -> CorePrivatePlayerDamageFactV1 {
        CorePrivatePlayerDamageFactV1 {
            tick,
            event_ordinal: 0,
            cause_kind: sim_core::AuthoritativeDeathCauseKind::DirectHit,
            source_content_id: "enemy.drowned_pilgrim",
            source_entity_id: sim_core::EntityId::new(10).unwrap(),
            target_entity_id: sim_core::EntityId::new(20).unwrap(),
            pattern_id: "pattern.enemy.drowned_pilgrim.fan",
            attack_id: "pattern.enemy.drowned_pilgrim.fan",
            raw_damage: 10,
            final_damage: 10,
            damage_type: sim_core::DamageType::Physical,
            pre_health: 10,
            post_health: 0,
            source_position: sim_core::SimulationVector::new(4.0, 6.0),
        }
    }

    fn terminal_frame_channel() -> (
        CorePrivateTerminalFrameSender,
        CorePrivateTerminalFrameReceiver,
    ) {
        let binding = TerminalBinding::new([0x11; 16], [0x22; 16], [0x33; 16], [0x44; 16])
            .expect("terminal binding");
        let route_lease = crate::CorePrivateRouteActorLease::for_test([0x11; 16], [0x22; 16], 1);
        let content_revision = template_step(1).route.content_revision;
        let binding = crate::CorePrivateTerminalFeedBinding::new(
            binding,
            route_lease,
            content_revision,
            [0x44; 16],
        )
        .expect("valid feed binding");
        CorePrivateTerminalFrameReceiver::channel(binding)
    }

    fn lethal_terminal_receipt(tick: Tick) -> crate::StoredTerminalReceipt {
        crate::StoredTerminalReceipt::from_storage(&crate::StoredTerminalReceiptV1 {
            schema_version: crate::STORED_TERMINAL_RECEIPT_SCHEMA_V1,
            account_id: [0x11; 16],
            character_id: [0x22; 16],
            lineage_id: [0x33; 16],
            restore_point_id: [0x44; 16],
            terminal_id: [0x51; 16],
            mutation_id: [0x52; 16],
            payload_hash: [0x53; 32],
            server_plan_hash: [0x54; 32],
            result_hash: [0x55; 32],
            expected_state_version: 1,
            post_state_version: 2,
            observed_tick: tick.0,
            committed_tick: tick.0,
            terminal_kind_code: TerminalKind::LethalDeath.stable_code(),
        })
        .expect("valid lethal receipt")
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
            if step.player_died {
                step.player_damage = vec![lethal_damage_fact(step.tick)];
            }
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
        assert!(matches!(
            driver
                .handle()
                .resolve_fixed_dungeon_rest(no_offer_authority())
                .await,
            Err(CorePrivateMicrorealmDriverError::FixedDungeonRestResolution(message))
                if message == "fixed dungeon is not installed"
        ));
        assert!(matches!(
            driver
                .handle()
                .commit_fixed_dungeon_b3_reward(b3_reward_authority())
                .await,
            Err(CorePrivateMicrorealmDriverError::FixedDungeonB3Reward(message))
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
    async fn no_op_packet_cadence_does_not_count_as_reward_activity() {
        let received = Arc::new(StdMutex::new(Vec::new()));
        let driver = spawn_driver(ScriptedRuntime::ordinary(Arc::clone(&received)));
        let handle = driver.handle();
        let mut observer = handle.observe();
        tokio::task::yield_now().await;

        // The production client samples every fixed update. More than the SOC-010 twenty-second
        // limit of no-op packets must leave the server-owned activity watermark unchanged.
        for sequence in 1..=605 {
            handle
                .submit_latest_input(CorePrivateMicrorealmRetainedInput {
                    input_sequence: sequence,
                    ..CorePrivateMicrorealmRetainedInput::default()
                })
                .expect("fresh no-op packet");
            advance_one_frame(&mut observer).await;
        }
        handle
            .submit_latest_input(CorePrivateMicrorealmRetainedInput {
                input_sequence: 606,
                movement: MovementAction::new(1, 0),
                ..CorePrivateMicrorealmRetainedInput::default()
            })
            .expect("meaningful movement packet");
        advance_one_frame(&mut observer).await;

        {
            let inputs = received.lock().expect("received");
            assert_eq!(inputs.len(), 606);
            assert!(
                inputs[..605]
                    .iter()
                    .all(|input| input.reward_activity_sequence == 1)
            );
            assert_eq!(inputs[605].reward_activity_sequence, 2);
        }
        let report = driver.shutdown().await.expect("shutdown");
        assert_eq!(report.accepted_input_updates, 606);
        assert_eq!(report.committed_frames, 606);
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
        handle.mark_reward_session_active();
        advance_one_frame(&mut observer).await;

        {
            let inputs = received.lock().expect("received");
            assert_eq!(inputs.len(), 7);
            assert_ne!(inputs[0].movement, MovementAction::default());
            assert!(inputs[0].primary_held);
            assert!(
                inputs[1..6]
                    .iter()
                    .all(|input| input.movement == MovementAction::default() && !input.primary_held)
            );
            assert!(inputs.iter().all(|input| input.aim == AimDirection::east()));
            assert!(inputs.iter().all(|input| input.ability_1_sequence == 1));
            assert!(inputs[0].reward_session_active);
            assert!(inputs[0].reward_trust_valid);
            assert!(
                inputs[1..6]
                    .iter()
                    .all(|input| !input.reward_session_active && !input.reward_trust_valid)
            );
            assert!(inputs[1..6].iter().all(|input| {
                input.reward_activity_sequence == inputs[0].reward_activity_sequence
            }));
            assert!(inputs[6].reward_session_active);
            assert!(inputs[6].reward_trust_valid);
            assert_eq!(
                inputs[6].reward_activity_sequence,
                inputs[0].reward_activity_sequence + 1
            );
        }
        let report = driver.shutdown().await.expect("shutdown");
        assert_eq!(report.committed_frames, 7);
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
        let (terminal_sender, mut terminal_receiver) = terminal_frame_channel();
        let terminal_owner = tokio::spawn(async move {
            let first = terminal_receiver.receive().await.expect("first frame");
            let first_frame = first.frame().expect("frame delivery");
            assert_eq!(first_frame.tick, Tick(1));
            assert!(first_frame.damage.is_empty());
            first.acknowledge_continue().expect("first acknowledgement");
            let lethal = terminal_receiver.receive().await.expect("lethal frame");
            let lethal_frame = lethal.frame().expect("frame delivery");
            assert_eq!(lethal_frame.delivery_sequence.get(), 2);
            assert_eq!(lethal_frame.tick, Tick(2));
            assert_eq!(lethal_frame.damage.len(), 1);
            let receipt = lethal_terminal_receipt(Tick(2));
            lethal
                .acknowledge_terminal_owned(&receipt)
                .expect("lethal ownership");
        });
        let driver = spawn_driver_with_terminal_feed(runtime, terminal_sender);
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
        terminal_owner.await.expect("terminal owner");
    }

    #[tokio::test(start_paused = true)]
    async fn terminal_ack_precedes_tick_and_presentation_publication() {
        let received = Arc::new(StdMutex::new(Vec::new()));
        let (terminal_sender, mut terminal_receiver) = terminal_frame_channel();
        let driver =
            spawn_driver_with_terminal_feed(ScriptedRuntime::ordinary(received), terminal_sender);
        let handle = driver.handle();
        let mut observer = handle.observe();

        tokio::time::advance(DRIVER_TICK_DURATION).await;
        let delivery = terminal_receiver.receive().await.expect("terminal frame");
        let frame = delivery.frame().expect("frame delivery");
        assert_eq!(frame.delivery_sequence.get(), 1);
        assert_eq!(frame.tick, Tick(1));
        assert!(matches!(
            observer.latest(),
            CorePrivateMicrorealmDriverState::Starting
        ));
        assert_eq!(handle.authoritative_tick(), None);

        delivery
            .acknowledge_continue()
            .expect("terminal acknowledgement");
        let state = observer.changed().await.expect("published frame");
        assert!(matches!(
            state,
            CorePrivateMicrorealmDriverState::Running {
                committed_frames: 1,
                ref step,
            } if step.tick == Tick(1)
        ));
        assert_eq!(handle.authoritative_tick().unwrap().get(), 1);
        let report = driver.shutdown().await.expect("shutdown");
        assert_eq!(report.committed_frames, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn terminal_feed_loss_faults_without_publishing_the_committed_tick() {
        let received = Arc::new(StdMutex::new(Vec::new()));
        let (terminal_sender, terminal_receiver) = terminal_frame_channel();
        drop(terminal_receiver);
        let driver =
            spawn_driver_with_terminal_feed(ScriptedRuntime::ordinary(received), terminal_sender);
        let handle = driver.handle();
        let mut observer = handle.observe();
        tokio::task::yield_now().await;
        advance_one_frame(&mut observer).await;
        assert!(matches!(
            observer.latest(),
            CorePrivateMicrorealmDriverState::Faulted {
                committed_frames: 1,
                fault: CorePrivateMicrorealmDriverFault {
                    kind: CorePrivateMicrorealmFaultKind::TerminalAuthority,
                    last_committed_tick: Tick(1),
                    ..
                }
            }
        ));
        assert_eq!(handle.authoritative_tick(), None);
        let report = driver.shutdown().await.expect("shutdown");
        assert_eq!(report.committed_frames, 1);
        assert_eq!(report.final_tick, Tick(1));
        assert_eq!(report.outcome, CorePrivateMicrorealmDriverOutcome::Faulted);
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_cancels_an_undrained_terminal_delivery_as_ambiguous() {
        let received = Arc::new(StdMutex::new(Vec::new()));
        let (terminal_sender, mut terminal_receiver) = terminal_frame_channel();
        let driver =
            spawn_driver_with_terminal_feed(ScriptedRuntime::ordinary(received), terminal_sender);
        let handle = driver.handle();
        tokio::task::yield_now().await;
        for _ in 0..4 {
            if terminal_receiver.pending_deliveries() > 0 {
                break;
            }
            tokio::time::advance(DRIVER_TICK_DURATION).await;
            tokio::task::yield_now().await;
        }
        assert_eq!(terminal_receiver.pending_deliveries(), 1);
        let report = driver.shutdown().await.expect("bounded shutdown");
        assert_eq!(report.committed_frames, 1);
        assert_eq!(report.final_tick, Tick(1));
        assert_eq!(report.outcome, CorePrivateMicrorealmDriverOutcome::Faulted);
        assert_eq!(handle.authoritative_tick(), None);
        let delivery = terminal_receiver
            .receive()
            .await
            .expect("ambiguous committed delivery retained");
        assert_eq!(delivery.frame().expect("frame delivery").tick, Tick(1));
        assert_eq!(
            delivery.acknowledge_continue(),
            Err(crate::CorePrivateTerminalAcknowledgementError::DriverGone)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_cancels_a_received_unacknowledged_terminal_delivery() {
        let received = Arc::new(StdMutex::new(Vec::new()));
        let (terminal_sender, mut terminal_receiver) = terminal_frame_channel();
        let driver =
            spawn_driver_with_terminal_feed(ScriptedRuntime::ordinary(received), terminal_sender);
        let handle = driver.handle();
        tokio::task::yield_now().await;
        for _ in 0..4 {
            if terminal_receiver.pending_deliveries() > 0 {
                break;
            }
            tokio::time::advance(DRIVER_TICK_DURATION).await;
            tokio::task::yield_now().await;
        }
        assert_eq!(terminal_receiver.pending_deliveries(), 1);
        let delivery = terminal_receiver.receive().await.expect("received frame");
        let report = driver.shutdown().await.expect("bounded shutdown");
        assert_eq!(report.committed_frames, 1);
        assert_eq!(report.final_tick, Tick(1));
        assert_eq!(report.outcome, CorePrivateMicrorealmDriverOutcome::Faulted);
        assert_eq!(handle.authoritative_tick(), None);
        assert_eq!(
            delivery.acknowledge_continue(),
            Err(crate::CorePrivateTerminalAcknowledgementError::DriverGone)
        );
    }

    #[tokio::test]
    async fn terminal_feed_rejects_foreign_route_generation_before_delivery() {
        let binding = TerminalBinding::new([0x11; 16], [0x22; 16], [0x33; 16], [0x44; 16])
            .expect("terminal binding");
        let foreign_lease = crate::CorePrivateRouteActorLease::for_test([0x11; 16], [0x22; 16], 2);
        let step = template_step(1);
        let feed_binding = crate::CorePrivateTerminalFeedBinding::new(
            binding,
            foreign_lease,
            step.route.content_revision.clone(),
            [0x44; 16],
        )
        .expect("structurally valid foreign generation binding");
        let (mut sender, _receiver) = CorePrivateTerminalFrameReceiver::channel(feed_binding);
        assert_eq!(
            sender
                .deliver(
                    CorePrivateTerminalSceneV1::Microrealm,
                    step.route,
                    step.tick,
                    step.player_position,
                    step.player_damage,
                    step.player_died,
                )
                .await,
            Err(CorePrivateTerminalFeedError::ForeignBinding)
        );
    }

    #[tokio::test]
    #[allow(
        clippy::too_many_lines,
        reason = "one linear assertion proves the exact frame/control/frame delivery sequence"
    )]
    async fn terminal_feed_totally_orders_bell_b0_and_first_b1_frame() {
        let (mut sender, mut receiver) = terminal_frame_channel();
        let mut final_microrealm = template_step(1);
        final_microrealm.route.phase = CorePrivateRoutePhaseV1::MicrorealmCleared;
        final_microrealm.route.readiness =
            CorePrivateRouteReadinessV1::canonical(CorePrivateRoutePhaseV1::MicrorealmCleared);

        let frame_delivery = async {
            let delivery = receiver.receive().await.expect("final microrealm frame");
            assert_eq!(
                delivery
                    .frame()
                    .expect("simulation frame")
                    .delivery_sequence
                    .get(),
                1
            );
            delivery
                .acknowledge_continue()
                .expect("frame acknowledgement");
        };
        let frame_send = sender.deliver(
            CorePrivateTerminalSceneV1::Microrealm,
            final_microrealm.route.clone(),
            final_microrealm.tick,
            final_microrealm.player_position,
            Vec::new(),
            false,
        );
        let (frame_result, ()) = tokio::join!(frame_send, frame_delivery);
        assert_eq!(
            frame_result.expect("frame accepted"),
            CorePrivateTerminalFrameDisposition::Continue
        );

        let transition = CoreBellPortalTransition {
            binding: crate::CoreBellPortalBinding {
                account_id: [0x11; 16],
                character_id: [0x22; 16],
                mutation_id: [0x61; 16],
                instance_lineage_id: [0x33; 16],
                entry_restore_point_id: [0x44; 16],
                character_version: 1,
                content_revision: WorldFlowContentRevisionV1 {
                    records_blake3: hash('d'),
                    assets_blake3: hash('e'),
                    localization_blake3: hash('f'),
                },
            },
            transfer_id: [0x62; 16],
            destination_character_version: 2,
        };
        let mut b0 = final_microrealm.route.clone();
        b0.character_version = 2;
        b0.state_version = 2;
        b0.scene = CorePrivateRouteSceneV1::BellSepulcher;
        b0.room = Some(CorePrivateRouteRoomV1::BellVestibuleB0);
        b0.phase = CorePrivateRoutePhaseV1::DungeonVestibule;
        b0.readiness = CorePrivateRouteReadinessV1::canonical(b0.phase);
        let bell_delivery = async {
            let delivery = receiver.receive().await.expect("Bell control");
            let bell = delivery.route_control().expect("route control");
            assert_eq!(bell.delivery_sequence.get(), 2);
            assert_eq!(bell.simulation_tick, Tick(1));
            assert_eq!(
                bell.authority.kind(),
                crate::CorePrivateTerminalRouteControlKindV1::BellDungeonEntered
            );
            delivery
                .acknowledge_continue()
                .expect("Bell acknowledgement");
        };
        let bell_send = sender.deliver_route_control(
            CorePrivateTerminalRouteControlAuthorityV1::BellDungeonEntered {
                transition: transition.clone(),
            },
            b0.clone(),
            Tick(1),
        );
        let (bell_result, ()) = tokio::join!(bell_send, bell_delivery);
        assert_eq!(
            bell_result.expect("Bell accepted"),
            CorePrivateTerminalFrameDisposition::Continue
        );

        let advance = sim_content::CoreFixedDungeonAdvance {
            from: sim_content::CoreFixedDungeonNode::BellVestibuleB0,
            to: sim_content::CoreFixedDungeonNode::BellCrossB1,
            rest_resolution: None,
        };
        let mut b1 = b0.clone();
        b1.state_version = 3;
        b1.room = Some(CorePrivateRouteRoomV1::BellCrossB1);
        b1.phase = CorePrivateRoutePhaseV1::RoomDormant;
        b1.readiness = CorePrivateRouteReadinessV1::canonical(b1.phase);
        let advance_delivery = async {
            let delivery = receiver.receive().await.expect("B0/B1 control");
            let control = delivery.route_control().expect("route control");
            assert_eq!(control.delivery_sequence.get(), 3);
            assert_eq!(control.simulation_tick, Tick(1));
            delivery
                .acknowledge_continue()
                .expect("B0/B1 acknowledgement");
        };
        let advance_send = sender.deliver_route_control(
            CorePrivateTerminalRouteControlAuthorityV1::FixedDungeonAdvanced {
                transition: advance,
            },
            b1.clone(),
            Tick(1),
        );
        let (advance_result, ()) = tokio::join!(advance_send, advance_delivery);
        assert_eq!(
            advance_result.expect("advance accepted"),
            CorePrivateTerminalFrameDisposition::Continue
        );

        let first_b1_delivery = async {
            let delivery = receiver.receive().await.expect("first B1 frame");
            let first_b1 = delivery.frame().expect("simulation frame");
            assert_eq!(first_b1.delivery_sequence.get(), 4);
            assert_eq!(first_b1.tick, Tick(2));
            delivery
                .acknowledge_continue()
                .expect("first B1 acknowledgement");
        };
        let first_b1_send = sender.deliver(
            CorePrivateTerminalSceneV1::FixedDungeon,
            b1,
            Tick(2),
            final_microrealm.player_position,
            Vec::new(),
            false,
        );
        let (first_b1_result, ()) = tokio::join!(first_b1_send, first_b1_delivery);
        assert_eq!(
            first_b1_result.expect("first B1 accepted"),
            CorePrivateTerminalFrameDisposition::Continue
        );
    }

    #[tokio::test]
    async fn terminal_feed_accepts_one_exact_equal_version_b3_commit() {
        let (mut sender, mut receiver) = terminal_frame_channel();
        let mut route = template_step(1).route;
        route.scene = CorePrivateRouteSceneV1::BellSepulcher;
        route.room = Some(CorePrivateRouteRoomV1::BellKnightB3);
        route.phase = CorePrivateRoutePhaseV1::RoomCleared;
        route.readiness = CorePrivateRouteReadinessV1::canonical(route.phase);
        route.state_version = 20;

        let frame_receiver = async {
            let delivery = receiver.receive().await.expect("B3 frame");
            delivery
                .acknowledge_continue()
                .expect("frame acknowledgement");
        };
        let frame_sender = sender.deliver(
            CorePrivateTerminalSceneV1::FixedDungeon,
            route.clone(),
            Tick(1),
            sim_core::TilePoint::new(10_000, 10_000),
            Vec::new(),
            false,
        );
        let (frame_result, ()) = tokio::join!(frame_sender, frame_receiver);
        assert_eq!(
            frame_result.expect("frame accepted"),
            CorePrivateTerminalFrameDisposition::Continue
        );

        let durable = b3_reward_authority();
        let commit = CorePrivateFixedDungeonB3RewardCommit {
            route: route.clone(),
            receipt: sim_content::CoreB3RewardReceipt::Committed,
            disposition: durable.disposition(),
            reward_event_id: durable.reward_event_id(),
            reward_result_hash: durable.reward_result_hash(),
            progression_payload_hash: durable.progression_payload_hash(),
            bargain_offer_id: durable.bargain_offer_id(),
            has_no_offer_resolution: durable.has_no_offer_resolution(),
        };
        let authority =
            CorePrivateTerminalRouteControlAuthorityV1::B3RewardCommitted { durable, commit };
        let control_receiver = async {
            let delivery = receiver.receive().await.expect("B3 control");
            let control = delivery.route_control().expect("route control");
            assert_eq!(control.delivery_sequence.get(), 2);
            assert_eq!(control.route.state_version, 20);
            delivery
                .acknowledge_continue()
                .expect("control acknowledgement");
        };
        let control_sender =
            sender.deliver_route_control(authority.clone(), route.clone(), Tick(1));
        let (control_result, ()) = tokio::join!(control_sender, control_receiver);
        assert_eq!(
            control_result.expect("control accepted"),
            CorePrivateTerminalFrameDisposition::Continue
        );
        assert_eq!(
            sender
                .deliver_route_control(authority, route, Tick(1))
                .await,
            Err(CorePrivateTerminalFeedError::DuplicateRouteControl)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn route_fault_is_fail_closed_and_shutdown_finishes_an_in_flight_frame() {
        let received = Arc::new(StdMutex::new(Vec::new()));
        let mut faulting = ScriptedRuntime::ordinary(Arc::clone(&received));
        faulting.fault_at = Some(1);
        let fault_driver = spawn_driver(faulting);
        let fault_handle = fault_driver.handle();
        let mut fault_observer = fault_handle.observe();
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
        assert_eq!(fault_handle.authoritative_tick(), None);
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
        let handle = driver.handle();
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
        assert_eq!(handle.authoritative_tick().unwrap().get(), 1);
        assert!(report.task_joined);
        assert!(!report.driver_task_live_after_join);
    }
}
