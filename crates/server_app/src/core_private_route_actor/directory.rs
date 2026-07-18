use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex, MutexGuard},
};

use protocol::{
    CorePrivateRouteContentRevisionV1, CorePrivateRoutePhaseV1, CorePrivateRouteRoomV1,
    CorePrivateRouteSceneV1, CorePrivateRouteStateV1, WorldFlowContentRevisionV1,
};
use thiserror::Error;
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

use super::{CorePrivateRouteActor, CorePrivateRouteActorAdvance, CorePrivateRouteActorError};
use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CoreBellPortalAbortReason,
    CoreBellPortalAuthority, CoreBellPortalBinding, CoreBellPortalPermit,
    CoreBellPortalPermitLease, CoreBellPortalRejection, CoreBellPortalTransition,
};

pub const CORE_PRIVATE_ROUTE_ACTOR_MAILBOX_CAPACITY: usize = 64;

const EXTRACTION_PERMIT_CONTEXT: &str =
    "gravebound.core-private-route.extraction-terminal-permit.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CorePrivateRouteActorKey {
    account_id: [u8; 16],
    character_id: [u8; 16],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateRouteActorLease {
    key: CorePrivateRouteActorKey,
    actor_generation: u64,
}

/// Durable Hall-to-microrealm transition material presented only after the world-flow write has
/// committed. The actor lease supplies account, character, and generation authority; this value
/// binds the exact receipt outcome that the in-memory actor must converge on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CorePrivateRouteEnterMicrorealmTransition {
    pub transfer_id: [u8; 16],
    pub source_character_version: u64,
    pub destination_character_version: u64,
    pub instance_lineage_id: [u8; 16],
    pub content_revision: WorldFlowContentRevisionV1,
}

/// Durable Hall-to-Character-Select transition material. Character Select has no route actor, so
/// successful reconciliation retires the exact Hall generation and retains an in-process replay
/// tombstone for that generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CorePrivateRouteReturnToCharacterSelectTransition {
    pub transfer_id: [u8; 16],
    pub source_character_version: u64,
    pub destination_character_version: u64,
    pub content_revision: WorldFlowContentRevisionV1,
}

impl CorePrivateRouteActorLease {
    #[must_use]
    pub const fn account_id(self) -> [u8; 16] {
        self.key.account_id
    }

    #[must_use]
    pub const fn character_id(self) -> [u8; 16] {
        self.key.character_id
    }

    #[must_use]
    pub const fn actor_generation(self) -> u64 {
        self.actor_generation
    }
}

/// Stable server-issued Sir Caldus exit identities admitted by the private-route actor.
///
/// Fields remain private so client material cannot be promoted into exit authority. The caller
/// must first obtain these identities from the committed reward/exit owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateRouteExtractionExitBinding {
    encounter: [u8; 16],
    exit_instance: [u8; 16],
    extraction_request: [u8; 16],
    extraction_receipt: [u8; 16],
    terminal: [u8; 16],
}

impl CorePrivateRouteExtractionExitBinding {
    pub fn new(
        encounter_id: [u8; 16],
        exit_instance_id: [u8; 16],
        extraction_request_id: [u8; 16],
        extraction_receipt_id: [u8; 16],
        terminal_id: [u8; 16],
    ) -> Result<Self, CorePrivateRouteRuntimeError> {
        let identities = [
            encounter_id,
            exit_instance_id,
            extraction_request_id,
            extraction_receipt_id,
            terminal_id,
        ];
        if identities.contains(&[0; 16]) || !pairwise_distinct(&identities) {
            return Err(CorePrivateRouteRuntimeError::InvalidExtractionBinding);
        }
        Ok(Self {
            encounter: encounter_id,
            exit_instance: exit_instance_id,
            extraction_request: extraction_request_id,
            extraction_receipt: extraction_receipt_id,
            terminal: terminal_id,
        })
    }

    #[must_use]
    pub const fn encounter_id(&self) -> [u8; 16] {
        self.encounter
    }

    #[must_use]
    pub const fn exit_instance_id(&self) -> [u8; 16] {
        self.exit_instance
    }

    #[must_use]
    pub const fn extraction_request_id(&self) -> [u8; 16] {
        self.extraction_request
    }

    #[must_use]
    pub const fn extraction_receipt_id(&self) -> [u8; 16] {
        self.extraction_receipt
    }

    #[must_use]
    pub const fn terminal_id(&self) -> [u8; 16] {
        self.terminal
    }
}

/// Complete pre-terminal actor and durable-exit authority supplied by server-owned systems.
///
/// The directory compares the complete route snapshot and its paired world-flow revision under
/// the actor lock. A caller cannot mix a stale generation, state version, content revision, or
/// lineage with a current exit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePrivateRouteExtractionBinding {
    account_id: [u8; 16],
    accepted_route: CorePrivateRouteStateV1,
    world_flow_revision: WorldFlowContentRevisionV1,
    entry_restore_point_id: [u8; 16],
    exit: CorePrivateRouteExtractionExitBinding,
}

impl CorePrivateRouteExtractionBinding {
    pub fn new(
        account_id: [u8; 16],
        accepted_route: CorePrivateRouteStateV1,
        world_flow_revision: WorldFlowContentRevisionV1,
        entry_restore_point_id: [u8; 16],
        exit: CorePrivateRouteExtractionExitBinding,
    ) -> Result<Self, CorePrivateRouteRuntimeError> {
        if account_id == [0; 16]
            || entry_restore_point_id == [0; 16]
            || accepted_route.validate().is_err()
            || accepted_route.scene != CorePrivateRouteSceneV1::BellSepulcher
            || accepted_route.room != Some(CorePrivateRouteRoomV1::CaldusArenaB6)
            || accepted_route.phase != CorePrivateRoutePhaseV1::BossExitReady
            || !accepted_route.readiness.extraction_available.is_available()
            || accepted_route
                .instance_lineage_id
                .is_none_or(|lineage| lineage == [0; 16])
            || zero_world_flow_revision(&world_flow_revision)
        {
            return Err(CorePrivateRouteRuntimeError::InvalidExtractionBinding);
        }
        Ok(Self {
            account_id,
            accepted_route,
            world_flow_revision,
            entry_restore_point_id,
            exit,
        })
    }

    #[must_use]
    pub const fn account_id(&self) -> [u8; 16] {
        self.account_id
    }

    #[must_use]
    pub const fn accepted_route(&self) -> &CorePrivateRouteStateV1 {
        &self.accepted_route
    }

    #[must_use]
    pub const fn world_flow_revision(&self) -> &WorldFlowContentRevisionV1 {
        &self.world_flow_revision
    }

    #[must_use]
    pub const fn entry_restore_point_id(&self) -> [u8; 16] {
        self.entry_restore_point_id
    }

    #[must_use]
    pub const fn exit(&self) -> &CorePrivateRouteExtractionExitBinding {
        &self.exit
    }
}

/// Opaque reservation proving one exact actor generation entered `TerminalPending`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePrivateRouteExtractionPermit {
    permit_id: [u8; 16],
    binding: CorePrivateRouteExtractionBinding,
    actor_generation: u64,
    accepted_route_state_version: u64,
    terminal_pending_route_state_version: u64,
    route_content_revision: CorePrivateRouteContentRevisionV1,
    world_flow_revision: WorldFlowContentRevisionV1,
}

impl CorePrivateRouteExtractionPermit {
    #[must_use]
    pub const fn permit_id(&self) -> [u8; 16] {
        self.permit_id
    }

    #[must_use]
    pub const fn binding(&self) -> &CorePrivateRouteExtractionBinding {
        &self.binding
    }

    #[must_use]
    pub const fn actor_generation(&self) -> u64 {
        self.actor_generation
    }

    #[must_use]
    pub const fn accepted_route_state_version(&self) -> u64 {
        self.accepted_route_state_version
    }

    #[must_use]
    pub const fn terminal_pending_route_state_version(&self) -> u64 {
        self.terminal_pending_route_state_version
    }

    #[must_use]
    pub const fn route_content_revision(&self) -> &CorePrivateRouteContentRevisionV1 {
        &self.route_content_revision
    }

    #[must_use]
    pub const fn world_flow_revision(&self) -> &WorldFlowContentRevisionV1 {
        &self.world_flow_revision
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateRouteRuntimeReport {
    pub served_actor_commands: u64,
    pub abandoned_actor_commands: u64,
    pub remaining_actor_tasks: usize,
    pub remaining_registered_actors: usize,
    pub remaining_portal_reservations: usize,
    pub remaining_terminal_reservations: usize,
    pub remaining_transition_reconciliations: usize,
    pub zero_residue: bool,
}

struct CorePrivateRouteActorControl {
    actor: CorePrivateRouteActor,
    bell_reservation: Option<CoreBellPortalPermit>,
    terminal_reservation: Option<CorePrivateRouteExtractionPermit>,
    enter_microrealm_reconciliation: Option<CorePrivateRouteEnterMicrorealmTransition>,
    retired: bool,
}

#[derive(Clone)]
struct CorePrivateRouteActorHandle {
    commands: mpsc::Sender<CorePrivateRouteActorCommand>,
}

enum CorePrivateRouteActorCommand {
    Advance {
        advance: CorePrivateRouteActorAdvance,
        reply: oneshot::Sender<CorePrivateRouteActorReply>,
    },
    SetBellPortalRange {
        in_range: bool,
        reply: oneshot::Sender<CorePrivateRouteActorReply>,
    },
    ApplyMicrorealmAuthority {
        expected_state_version: u64,
        phase: CorePrivateRoutePhaseV1,
        bell_portal_in_range: bool,
        reply: oneshot::Sender<CorePrivateRouteActorReply>,
    },
    ApplyFixedDungeonAuthority {
        expected_state_version: u64,
        room: CorePrivateRouteRoomV1,
        phase: CorePrivateRoutePhaseV1,
        reply: oneshot::Sender<CorePrivateRouteActorReply>,
    },
    PrepareBellPortal {
        binding: CoreBellPortalBinding,
        reply: oneshot::Sender<Result<CoreBellPortalPermit, CoreBellPortalRejection>>,
    },
    CommitBellPortal {
        permit: CoreBellPortalPermit,
        transition: CoreBellPortalTransition,
        reply: oneshot::Sender<Result<(), CoreBellPortalRejection>>,
    },
    ReconcileBellPortal {
        transition: CoreBellPortalTransition,
        reply: oneshot::Sender<Result<(), CoreBellPortalRejection>>,
    },
    ReconcileEnterMicrorealm {
        transition: CorePrivateRouteEnterMicrorealmTransition,
        reply: oneshot::Sender<CorePrivateRouteActorReply>,
    },
    PrepareExtractionTerminal {
        binding: CorePrivateRouteExtractionBinding,
        reply:
            oneshot::Sender<Result<CorePrivateRouteExtractionPermit, CorePrivateRouteRuntimeError>>,
    },
    RevalidateExtractionTerminal {
        permit: CorePrivateRouteExtractionPermit,
        reply: oneshot::Sender<Result<(), CorePrivateRouteRuntimeError>>,
    },
}

type CorePrivateRouteActorReply = Result<CorePrivateRouteStateV1, CorePrivateRouteRuntimeError>;

impl CorePrivateRouteActorHandle {
    async fn advance(&self, advance: CorePrivateRouteActorAdvance) -> CorePrivateRouteActorReply {
        let (reply, receive) = oneshot::channel();
        self.commands
            .send(CorePrivateRouteActorCommand::Advance { advance, reply })
            .await
            .map_err(|_| CorePrivateRouteRuntimeError::ActorUnavailable)?;
        receive
            .await
            .map_err(|_| CorePrivateRouteRuntimeError::ActorUnavailable)?
    }

    async fn set_bell_portal_range(&self, in_range: bool) -> CorePrivateRouteActorReply {
        let (reply, receive) = oneshot::channel();
        self.commands
            .send(CorePrivateRouteActorCommand::SetBellPortalRange { in_range, reply })
            .await
            .map_err(|_| CorePrivateRouteRuntimeError::ActorUnavailable)?;
        receive
            .await
            .map_err(|_| CorePrivateRouteRuntimeError::ActorUnavailable)?
    }

    async fn apply_microrealm_authority(
        &self,
        expected_state_version: u64,
        phase: CorePrivateRoutePhaseV1,
        bell_portal_in_range: bool,
    ) -> CorePrivateRouteActorReply {
        let (reply, receive) = oneshot::channel();
        self.commands
            .send(CorePrivateRouteActorCommand::ApplyMicrorealmAuthority {
                expected_state_version,
                phase,
                bell_portal_in_range,
                reply,
            })
            .await
            .map_err(|_| CorePrivateRouteRuntimeError::ActorUnavailable)?;
        receive
            .await
            .map_err(|_| CorePrivateRouteRuntimeError::ActorUnavailable)?
    }

    async fn apply_fixed_dungeon_authority(
        &self,
        expected_state_version: u64,
        room: CorePrivateRouteRoomV1,
        phase: CorePrivateRoutePhaseV1,
    ) -> CorePrivateRouteActorReply {
        let (reply, receive) = oneshot::channel();
        self.commands
            .send(CorePrivateRouteActorCommand::ApplyFixedDungeonAuthority {
                expected_state_version,
                room,
                phase,
                reply,
            })
            .await
            .map_err(|_| CorePrivateRouteRuntimeError::ActorUnavailable)?;
        receive
            .await
            .map_err(|_| CorePrivateRouteRuntimeError::ActorUnavailable)?
    }

    async fn prepare_bell_portal(
        &self,
        binding: CoreBellPortalBinding,
    ) -> Result<CoreBellPortalPermit, CoreBellPortalRejection> {
        let (reply, receive) = oneshot::channel();
        self.commands
            .send(CorePrivateRouteActorCommand::PrepareBellPortal { binding, reply })
            .await
            .map_err(|_| CoreBellPortalRejection::InstanceUnavailable)?;
        receive
            .await
            .map_err(|_| CoreBellPortalRejection::InstanceUnavailable)?
    }

    async fn commit_bell_portal(
        &self,
        permit: CoreBellPortalPermit,
        transition: CoreBellPortalTransition,
    ) -> Result<(), CoreBellPortalRejection> {
        let (reply, receive) = oneshot::channel();
        self.commands
            .send(CorePrivateRouteActorCommand::CommitBellPortal {
                permit,
                transition,
                reply,
            })
            .await
            .map_err(|_| CoreBellPortalRejection::InstanceUnavailable)?;
        receive
            .await
            .map_err(|_| CoreBellPortalRejection::InstanceUnavailable)?
    }

    async fn reconcile_bell_portal(
        &self,
        transition: CoreBellPortalTransition,
    ) -> Result<(), CoreBellPortalRejection> {
        let (reply, receive) = oneshot::channel();
        self.commands
            .send(CorePrivateRouteActorCommand::ReconcileBellPortal { transition, reply })
            .await
            .map_err(|_| CoreBellPortalRejection::InstanceUnavailable)?;
        receive
            .await
            .map_err(|_| CoreBellPortalRejection::InstanceUnavailable)?
    }

    async fn reconcile_enter_microrealm(
        &self,
        transition: CorePrivateRouteEnterMicrorealmTransition,
    ) -> CorePrivateRouteActorReply {
        let (reply, receive) = oneshot::channel();
        self.commands
            .send(CorePrivateRouteActorCommand::ReconcileEnterMicrorealm { transition, reply })
            .await
            .map_err(|_| CorePrivateRouteRuntimeError::ActorUnavailable)?;
        receive
            .await
            .map_err(|_| CorePrivateRouteRuntimeError::ActorUnavailable)?
    }

    async fn prepare_extraction_terminal(
        &self,
        binding: CorePrivateRouteExtractionBinding,
    ) -> Result<CorePrivateRouteExtractionPermit, CorePrivateRouteRuntimeError> {
        let (reply, receive) = oneshot::channel();
        self.commands
            .send(CorePrivateRouteActorCommand::PrepareExtractionTerminal { binding, reply })
            .await
            .map_err(|_| CorePrivateRouteRuntimeError::ActorUnavailable)?;
        receive
            .await
            .map_err(|_| CorePrivateRouteRuntimeError::ActorUnavailable)?
    }

    async fn revalidate_extraction_terminal(
        &self,
        permit: CorePrivateRouteExtractionPermit,
    ) -> Result<(), CorePrivateRouteRuntimeError> {
        let (reply, receive) = oneshot::channel();
        self.commands
            .send(CorePrivateRouteActorCommand::RevalidateExtractionTerminal { permit, reply })
            .await
            .map_err(|_| CorePrivateRouteRuntimeError::ActorUnavailable)?;
        receive
            .await
            .map_err(|_| CorePrivateRouteRuntimeError::ActorUnavailable)?
    }
}

struct CorePrivateRouteActorEntry {
    authenticated: AuthenticatedAccount,
    control: Arc<Mutex<CorePrivateRouteActorControl>>,
    handle: CorePrivateRouteActorHandle,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<CorePrivateRouteActorTaskReport>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CorePrivateRouteActorTaskReport {
    served: u64,
    abandoned: u64,
}

struct CorePrivateRouteDirectoryState {
    accepting: bool,
    shutdown_started: bool,
    actors: BTreeMap<CorePrivateRouteActorKey, CorePrivateRouteActorEntry>,
    active_account: BTreeMap<[u8; 16], CorePrivateRouteActorKey>,
    generation_floors: BTreeMap<CorePrivateRouteActorKey, u64>,
    character_select_reconciliations: BTreeMap<
        (CorePrivateRouteActorKey, u64),
        CorePrivateRouteReturnToCharacterSelectTransition,
    >,
    retired_tasks: Vec<JoinHandle<CorePrivateRouteActorTaskReport>>,
    served_actor_commands: u64,
    abandoned_actor_commands: u64,
}

struct CorePrivateRouteDirectoryInner {
    state: Mutex<CorePrivateRouteDirectoryState>,
}

#[derive(Clone)]
pub struct CorePrivateRouteActorDirectory {
    inner: Arc<CorePrivateRouteDirectoryInner>,
}

impl fmt::Debug for CorePrivateRouteActorDirectory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = lock(&self.inner.state);
        formatter
            .debug_struct("CorePrivateRouteActorDirectory")
            .field("accepting", &state.accepting)
            .field("registered_actors", &state.actors.len())
            .finish_non_exhaustive()
    }
}

impl Default for CorePrivateRouteActorDirectory {
    fn default() -> Self {
        Self::new()
    }
}

impl CorePrivateRouteActorDirectory {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(CorePrivateRouteDirectoryInner {
                state: Mutex::new(CorePrivateRouteDirectoryState {
                    accepting: true,
                    shutdown_started: false,
                    actors: BTreeMap::new(),
                    active_account: BTreeMap::new(),
                    generation_floors: BTreeMap::new(),
                    character_select_reconciliations: BTreeMap::new(),
                    retired_tasks: Vec::new(),
                    served_actor_commands: 0,
                    abandoned_actor_commands: 0,
                }),
            }),
        }
    }

    /// Registers one actor generation allocated by the persistent composition root. The directory
    /// enforces monotonicity again in memory, but deliberately does not invent a process-local
    /// generation that could create an ABA after restart.
    pub fn register_actor(
        &self,
        authenticated: AuthenticatedAccount,
        seed: super::CorePrivateRouteActorSeed,
        actor_generation: u64,
    ) -> Result<CorePrivateRouteActorLease, CorePrivateRouteRuntimeError> {
        if authenticated.namespace != AuthenticatedNamespace::WipeableTest {
            return Err(CorePrivateRouteRuntimeError::InvalidActorBinding);
        }
        let key = CorePrivateRouteActorKey {
            account_id: authenticated.account_id.as_bytes(),
            character_id: seed.character_id,
        };
        let actor = CorePrivateRouteActor::new(seed, actor_generation)?;
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| CorePrivateRouteRuntimeError::RuntimeUnavailable)?;
        let mut state = lock(&self.inner.state);
        if !state.accepting {
            return Err(CorePrivateRouteRuntimeError::Retired);
        }
        if state.active_account.contains_key(&key.account_id) {
            return Err(CorePrivateRouteRuntimeError::AccountAlreadyActive);
        }
        if state.actors.contains_key(&key) {
            return Err(CorePrivateRouteRuntimeError::ActorAlreadyRegistered);
        }
        if state
            .generation_floors
            .get(&key)
            .is_some_and(|floor| actor_generation <= *floor)
        {
            return Err(CorePrivateRouteRuntimeError::StaleGeneration);
        }
        let control = Arc::new(Mutex::new(CorePrivateRouteActorControl {
            actor,
            bell_reservation: None,
            terminal_reservation: None,
            enter_microrealm_reconciliation: None,
            retired: false,
        }));
        let (commands, inbox) = mpsc::channel(CORE_PRIVATE_ROUTE_ACTOR_MAILBOX_CAPACITY);
        let (shutdown, shutdown_receive) = oneshot::channel();
        let task_control = Arc::clone(&control);
        let task = runtime.spawn(serve_actor_mailbox(inbox, task_control, shutdown_receive));
        state.active_account.insert(key.account_id, key);
        state.actors.insert(
            key,
            CorePrivateRouteActorEntry {
                authenticated,
                control,
                handle: CorePrivateRouteActorHandle { commands },
                shutdown: Some(shutdown),
                task: Some(task),
            },
        );
        Ok(CorePrivateRouteActorLease {
            key,
            actor_generation,
        })
    }

    pub async fn advance(
        &self,
        lease: CorePrivateRouteActorLease,
        advance: CorePrivateRouteActorAdvance,
    ) -> CorePrivateRouteActorReply {
        self.actor_handle(lease)?.advance(advance).await
    }

    /// Converges one retained Hall actor on an already committed microrealm entry. Exact replay
    /// is a no-op even after the microrealm phase has advanced; changed lineage, content,
    /// generation, or character-version authority fails closed.
    pub(crate) async fn reconcile_enter_microrealm(
        &self,
        lease: CorePrivateRouteActorLease,
        transition: CorePrivateRouteEnterMicrorealmTransition,
    ) -> CorePrivateRouteActorReply {
        validate_enter_microrealm_transition(&transition)?;
        self.actor_handle(lease)?
            .reconcile_enter_microrealm(transition)
            .await
    }

    /// Retires the exact Hall actor after a committed return to Character Select. The transition
    /// is recorded before the actor task is joined, so an exact response-loss replay cannot retire
    /// a later generation. Changed replay material remains a typed conflict.
    pub(crate) async fn reconcile_return_to_character_select(
        &self,
        lease: CorePrivateRouteActorLease,
        transition: CorePrivateRouteReturnToCharacterSelectTransition,
    ) -> Result<(), CorePrivateRouteRuntimeError> {
        validate_return_to_character_select_transition(&transition)?;
        let reconciliation_key = (lease.key, lease.actor_generation);
        let mut entry = {
            let mut state = lock(&self.inner.state);
            if !state.accepting {
                return Err(CorePrivateRouteRuntimeError::Retired);
            }
            if let Some(stored) = state
                .character_select_reconciliations
                .get(&reconciliation_key)
            {
                return if stored == &transition {
                    Ok(())
                } else {
                    Err(CorePrivateRouteRuntimeError::StaleRouteState)
                };
            }
            let control = state
                .actors
                .get(&lease.key)
                .ok_or(CorePrivateRouteRuntimeError::ActorUnavailable)?
                .control
                .clone();
            {
                let mut control = lock(&control);
                let projection = control.actor.projection();
                if projection.actor_generation != lease.actor_generation {
                    return Err(CorePrivateRouteRuntimeError::StaleGeneration);
                }
                if control.actor.world_flow_revision() != &transition.content_revision {
                    return Err(CorePrivateRouteRuntimeError::ContentAuthorityMismatch);
                }
                if control.bell_reservation.is_some() {
                    return Err(CorePrivateRouteRuntimeError::TransferInProgress);
                }
                if control.terminal_reservation.is_some() {
                    return Err(CorePrivateRouteRuntimeError::TerminalInProgress);
                }
                if projection.character_version != transition.source_character_version
                    || projection.scene != CorePrivateRouteSceneV1::LanternHalls
                    || projection.room.is_some()
                    || projection.instance_lineage_id.is_some()
                    || projection.phase != CorePrivateRoutePhaseV1::Hall
                {
                    return Err(CorePrivateRouteRuntimeError::StaleRouteState);
                }
                control.retired = true;
                control.enter_microrealm_reconciliation = None;
            }
            let entry = state
                .actors
                .remove(&lease.key)
                .ok_or(CorePrivateRouteRuntimeError::ActorUnavailable)?;
            state.active_account.remove(&lease.key.account_id);
            state
                .generation_floors
                .entry(lease.key)
                .and_modify(|floor| *floor = (*floor).max(lease.actor_generation))
                .or_insert(lease.actor_generation);
            state
                .character_select_reconciliations
                .insert(reconciliation_key, transition);
            entry
        };
        if let Some(shutdown) = entry.shutdown.take() {
            let _ = shutdown.send(());
        }
        let report = entry
            .task
            .take()
            .ok_or(CorePrivateRouteRuntimeError::ActorUnavailable)?
            .await
            .map_err(CorePrivateRouteRuntimeError::ActorTaskFailed)?;
        let mut state = lock(&self.inner.state);
        state.served_actor_commands = state.served_actor_commands.saturating_add(report.served);
        state.abandoned_actor_commands = state
            .abandoned_actor_commands
            .saturating_add(report.abandoned);
        Ok(())
    }

    pub async fn set_bell_portal_in_range(
        &self,
        lease: CorePrivateRouteActorLease,
        in_range: bool,
    ) -> CorePrivateRouteActorReply {
        self.actor_handle(lease)?
            .set_bell_portal_range(in_range)
            .await
    }

    /// Atomically applies one server-owned microrealm simulation result. The caller supplies the
    /// exact route version it staged against; phase and Bell range either advance together under
    /// the actor lock or neither changes.
    pub(crate) async fn apply_microrealm_authority(
        &self,
        lease: CorePrivateRouteActorLease,
        expected_state_version: u64,
        phase: CorePrivateRoutePhaseV1,
        bell_portal_in_range: bool,
    ) -> CorePrivateRouteActorReply {
        self.actor_handle(lease)?
            .apply_microrealm_authority(expected_state_version, phase, bell_portal_in_range)
            .await
    }

    /// Atomically converges one server-owned fixed-dungeon simulation result. A single frame may
    /// legitimately lock a participant and close a clear doorway, or activate and immediately
    /// clear the final hostile; the complete canonical phase path commits under one actor lock.
    pub(crate) async fn apply_fixed_dungeon_authority(
        &self,
        lease: CorePrivateRouteActorLease,
        expected_state_version: u64,
        room: CorePrivateRouteRoomV1,
        phase: CorePrivateRoutePhaseV1,
    ) -> CorePrivateRouteActorReply {
        self.actor_handle(lease)?
            .apply_fixed_dungeon_authority(expected_state_version, room, phase)
            .await
    }

    /// Atomically reserves one exact successful-extraction authority and revokes ordinary route
    /// control. Exact replay returns the same permit; changed material cannot replace it.
    pub async fn prepare_extraction_terminal(
        &self,
        lease: CorePrivateRouteActorLease,
        binding: CorePrivateRouteExtractionBinding,
    ) -> Result<CorePrivateRouteExtractionPermit, CorePrivateRouteRuntimeError> {
        validate_extraction_lease_binding(lease, &binding)?;
        self.actor_handle(lease)?
            .prepare_extraction_terminal(binding)
            .await
    }

    /// Revalidates the opaque reservation immediately before an in-process prepared candidate or
    /// committed result is published. Retirement, replacement, content drift, or state movement
    /// invalidates the permit.
    pub async fn revalidate_extraction_terminal(
        &self,
        lease: CorePrivateRouteActorLease,
        permit: &CorePrivateRouteExtractionPermit,
    ) -> Result<(), CorePrivateRouteRuntimeError> {
        validate_extraction_lease_binding(lease, permit.binding())?;
        self.actor_handle(lease)?
            .revalidate_extraction_terminal(permit.clone())
            .await
    }

    pub fn snapshot(
        &self,
        lease: CorePrivateRouteActorLease,
    ) -> Result<CorePrivateRouteStateV1, CorePrivateRouteRuntimeError> {
        let control = self.actor_control(lease)?;
        let control = lock(&control);
        if control.retired || control.actor.projection().actor_generation != lease.actor_generation
        {
            return Err(CorePrivateRouteRuntimeError::StaleGeneration);
        }
        Ok(control.actor.projection().clone())
    }

    pub async fn retire_actor(
        &self,
        lease: CorePrivateRouteActorLease,
    ) -> Result<(), CorePrivateRouteRuntimeError> {
        let mut entry = {
            let mut state = lock(&self.inner.state);
            let control = state
                .actors
                .get(&lease.key)
                .ok_or(CorePrivateRouteRuntimeError::ActorUnavailable)?
                .control
                .clone();
            {
                let mut control = lock(&control);
                if control.actor.projection().actor_generation != lease.actor_generation {
                    return Err(CorePrivateRouteRuntimeError::StaleGeneration);
                }
                if control.bell_reservation.is_some() {
                    return Err(CorePrivateRouteRuntimeError::TransferInProgress);
                }
                control.retired = true;
                control.terminal_reservation = None;
                control.enter_microrealm_reconciliation = None;
            }
            let entry = state
                .actors
                .remove(&lease.key)
                .ok_or(CorePrivateRouteRuntimeError::ActorUnavailable)?;
            state.active_account.remove(&lease.key.account_id);
            state
                .generation_floors
                .entry(lease.key)
                .and_modify(|floor| *floor = (*floor).max(lease.actor_generation))
                .or_insert(lease.actor_generation);
            entry
        };
        if let Some(shutdown) = entry.shutdown.take() {
            let _ = shutdown.send(());
        }
        let report = entry
            .task
            .take()
            .ok_or(CorePrivateRouteRuntimeError::ActorUnavailable)?
            .await
            .map_err(CorePrivateRouteRuntimeError::ActorTaskFailed)?;
        let mut state = lock(&self.inner.state);
        state.served_actor_commands = state.served_actor_commands.saturating_add(report.served);
        state.abandoned_actor_commands = state
            .abandoned_actor_commands
            .saturating_add(report.abandoned);
        Ok(())
    }

    /// Closes admission and retires every actor before connection workers are joined. Any Bell or
    /// terminal permit becomes unusable; durable outcomes reconcile after restart, while an
    /// uncommitted terminal intent follows the ordinary crash-restore contract.
    pub fn begin_shutdown(&self) {
        let mut state = lock(&self.inner.state);
        state.accepting = false;
        state.shutdown_started = true;
        let actors = std::mem::take(&mut state.actors);
        state.active_account.clear();
        state.character_select_reconciliations.clear();
        for (key, mut entry) in actors {
            let generation = {
                let mut control = lock(&entry.control);
                control.retired = true;
                control.bell_reservation = None;
                control.terminal_reservation = None;
                control.enter_microrealm_reconciliation = None;
                control.actor.projection().actor_generation
            };
            state
                .generation_floors
                .entry(key)
                .and_modify(|floor| *floor = (*floor).max(generation))
                .or_insert(generation);
            if let Some(shutdown) = entry.shutdown.take() {
                let _ = shutdown.send(());
            }
            if let Some(task) = entry.task.take() {
                state.retired_tasks.push(task);
            }
        }
    }

    pub async fn finish_shutdown(
        &self,
    ) -> Result<CorePrivateRouteRuntimeReport, CorePrivateRouteRuntimeError> {
        let tasks = {
            let mut state = lock(&self.inner.state);
            if !state.shutdown_started {
                return Err(CorePrivateRouteRuntimeError::ShutdownNotStarted);
            }
            std::mem::take(&mut state.retired_tasks)
        };
        let mut served = 0_u64;
        let mut abandoned = 0_u64;
        for task in tasks {
            let report = task
                .await
                .map_err(CorePrivateRouteRuntimeError::ActorTaskFailed)?;
            served = served.saturating_add(report.served);
            abandoned = abandoned.saturating_add(report.abandoned);
        }
        let mut state = lock(&self.inner.state);
        state.served_actor_commands = state.served_actor_commands.saturating_add(served);
        state.abandoned_actor_commands = state.abandoned_actor_commands.saturating_add(abandoned);
        let remaining_portal_reservations = state
            .actors
            .values()
            .filter(|entry| lock(&entry.control).bell_reservation.is_some())
            .count();
        let remaining_terminal_reservations = state
            .actors
            .values()
            .filter(|entry| lock(&entry.control).terminal_reservation.is_some())
            .count();
        let report = CorePrivateRouteRuntimeReport {
            served_actor_commands: state.served_actor_commands,
            abandoned_actor_commands: state.abandoned_actor_commands,
            remaining_actor_tasks: state.retired_tasks.len(),
            remaining_registered_actors: state.actors.len(),
            remaining_portal_reservations,
            remaining_terminal_reservations,
            remaining_transition_reconciliations: state.character_select_reconciliations.len(),
            zero_residue: state.retired_tasks.is_empty()
                && state.actors.is_empty()
                && state.active_account.is_empty()
                && state.character_select_reconciliations.is_empty()
                && remaining_portal_reservations == 0
                && remaining_terminal_reservations == 0,
        };
        Ok(report)
    }

    fn actor_handle(
        &self,
        lease: CorePrivateRouteActorLease,
    ) -> Result<CorePrivateRouteActorHandle, CorePrivateRouteRuntimeError> {
        let state = lock(&self.inner.state);
        if !state.accepting {
            return Err(CorePrivateRouteRuntimeError::Retired);
        }
        let entry = state
            .actors
            .get(&lease.key)
            .ok_or(CorePrivateRouteRuntimeError::ActorUnavailable)?;
        if entry.authenticated.account_id.as_bytes() != lease.key.account_id
            || lock(&entry.control).actor.projection().actor_generation != lease.actor_generation
        {
            return Err(CorePrivateRouteRuntimeError::StaleGeneration);
        }
        Ok(entry.handle.clone())
    }

    fn actor_control(
        &self,
        lease: CorePrivateRouteActorLease,
    ) -> Result<Arc<Mutex<CorePrivateRouteActorControl>>, CorePrivateRouteRuntimeError> {
        let state = lock(&self.inner.state);
        let entry = state
            .actors
            .get(&lease.key)
            .ok_or(CorePrivateRouteRuntimeError::ActorUnavailable)?;
        Ok(Arc::clone(&entry.control))
    }

    fn actor_for_binding(
        &self,
        binding: &CoreBellPortalBinding,
    ) -> Result<
        (
            Arc<Mutex<CorePrivateRouteActorControl>>,
            CorePrivateRouteActorHandle,
        ),
        CoreBellPortalRejection,
    > {
        let state = lock(&self.inner.state);
        if !state.accepting {
            return Err(CoreBellPortalRejection::InstanceUnavailable);
        }
        let key = state
            .active_account
            .get(&binding.account_id)
            .ok_or(CoreBellPortalRejection::InstanceUnavailable)?;
        if key.character_id != binding.character_id {
            return Err(CoreBellPortalRejection::InstanceUnavailable);
        }
        state.actors.get(key).map_or_else(
            || Err(CoreBellPortalRejection::InstanceUnavailable),
            |entry| Ok((Arc::clone(&entry.control), entry.handle.clone())),
        )
    }
}

pub struct CorePrivateRouteBellPermitLease {
    control: Arc<Mutex<CorePrivateRouteActorControl>>,
    permit: CoreBellPortalPermit,
    armed: bool,
}

impl CorePrivateRouteBellPermitLease {
    fn release(&mut self) {
        if !self.armed {
            return;
        }
        release_bell_reservation(&self.control, &self.permit);
        self.armed = false;
    }
}

impl fmt::Debug for CorePrivateRouteBellPermitLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CorePrivateRouteBellPermitLease")
            .field("permit", &self.permit)
            .field("armed", &self.armed)
            .finish_non_exhaustive()
    }
}

impl Drop for CorePrivateRouteBellPermitLease {
    fn drop(&mut self) {
        self.release();
    }
}

impl CoreBellPortalPermitLease for CorePrivateRouteBellPermitLease {
    fn permit(&self) -> &CoreBellPortalPermit {
        &self.permit
    }
}

impl CoreBellPortalAuthority for CorePrivateRouteActorDirectory {
    type PermitLease = CorePrivateRouteBellPermitLease;

    async fn prepare_bell_portal(
        &self,
        binding: CoreBellPortalBinding,
    ) -> Result<Self::PermitLease, CoreBellPortalRejection> {
        let (control, handle) = self.actor_for_binding(&binding)?;
        let permit = handle.prepare_bell_portal(binding).await?;
        Ok(CorePrivateRouteBellPermitLease {
            control,
            permit,
            armed: true,
        })
    }

    async fn commit_bell_portal(
        &self,
        mut lease: Self::PermitLease,
        transition: CoreBellPortalTransition,
    ) -> Result<(), CoreBellPortalRejection> {
        if !valid_transition(&lease.permit.binding, &transition) {
            return Err(CoreBellPortalRejection::ServiceUnavailable);
        }
        let (current, handle) = self.actor_for_binding(&transition.binding)?;
        if !Arc::ptr_eq(&current, &lease.control) {
            return Err(CoreBellPortalRejection::InstanceUnavailable);
        }
        let result = handle
            .commit_bell_portal(lease.permit.clone(), transition)
            .await;
        if result.is_ok() {
            lease.armed = false;
        }
        result
    }

    async fn abort_bell_portal(
        &self,
        mut lease: Self::PermitLease,
        _reason: CoreBellPortalAbortReason,
    ) {
        lease.release();
    }

    async fn reconcile_bell_portal(
        &self,
        transition: CoreBellPortalTransition,
    ) -> Result<(), CoreBellPortalRejection> {
        if !valid_transition(&transition.binding, &transition) {
            return Err(CoreBellPortalRejection::ServiceUnavailable);
        }
        let (_, handle) = self.actor_for_binding(&transition.binding)?;
        handle.reconcile_bell_portal(transition).await
    }
}

async fn serve_actor_mailbox(
    mut inbox: mpsc::Receiver<CorePrivateRouteActorCommand>,
    control: Arc<Mutex<CorePrivateRouteActorControl>>,
    mut shutdown: oneshot::Receiver<()>,
) -> CorePrivateRouteActorTaskReport {
    let mut served = 0_u64;
    loop {
        let command = tokio::select! {
            biased;
            _ = &mut shutdown => None,
            command = inbox.recv() => command,
        };
        let Some(command) = command else {
            break;
        };
        served = served.saturating_add(1);
        match command {
            CorePrivateRouteActorCommand::Advance { advance, reply } => {
                let _ = reply.send(handle_advance(&control, advance));
            }
            CorePrivateRouteActorCommand::SetBellPortalRange { in_range, reply } => {
                let _ = reply.send(handle_portal_range(&control, in_range));
            }
            CorePrivateRouteActorCommand::ApplyMicrorealmAuthority {
                expected_state_version,
                phase,
                bell_portal_in_range,
                reply,
            } => {
                let _ = reply.send(handle_microrealm_authority(
                    &control,
                    expected_state_version,
                    phase,
                    bell_portal_in_range,
                ));
            }
            CorePrivateRouteActorCommand::ApplyFixedDungeonAuthority {
                expected_state_version,
                room,
                phase,
                reply,
            } => {
                reply_fixed_dungeon_authority(&control, expected_state_version, room, phase, reply);
            }
            CorePrivateRouteActorCommand::PrepareBellPortal { binding, reply } => {
                let result = handle_prepare_bell_portal(&control, binding);
                let prepared = result.as_ref().ok().cloned();
                if reply.send(result).is_err()
                    && let Some(permit) = prepared
                {
                    release_bell_reservation(&control, &permit);
                }
            }
            CorePrivateRouteActorCommand::CommitBellPortal {
                permit,
                transition,
                reply,
            } => {
                let _ = reply.send(handle_commit_bell_portal(&control, &permit, &transition));
            }
            CorePrivateRouteActorCommand::ReconcileBellPortal { transition, reply } => {
                let _ = reply.send(handle_reconcile_bell_portal(&control, &transition));
            }
            CorePrivateRouteActorCommand::ReconcileEnterMicrorealm { transition, reply } => {
                let _ = reply.send(handle_reconcile_enter_microrealm(&control, &transition));
            }
            CorePrivateRouteActorCommand::PrepareExtractionTerminal { binding, reply } => {
                // A lost transport response must not reopen control. Exact replay reconstructs the
                // same permit from the actor-owned reservation.
                let _ = reply.send(handle_prepare_extraction_terminal(&control, binding));
            }
            CorePrivateRouteActorCommand::RevalidateExtractionTerminal { permit, reply } => {
                let _ = reply.send(handle_revalidate_extraction_terminal(&control, &permit));
            }
        }
    }
    let mut abandoned = 0_u64;
    while let Ok(command) = inbox.try_recv() {
        abandoned = abandoned.saturating_add(1);
        match command {
            CorePrivateRouteActorCommand::Advance { reply, .. }
            | CorePrivateRouteActorCommand::SetBellPortalRange { reply, .. }
            | CorePrivateRouteActorCommand::ApplyMicrorealmAuthority { reply, .. }
            | CorePrivateRouteActorCommand::ApplyFixedDungeonAuthority { reply, .. }
            | CorePrivateRouteActorCommand::ReconcileEnterMicrorealm { reply, .. } => {
                let _ = reply.send(Err(CorePrivateRouteRuntimeError::Retired));
            }
            CorePrivateRouteActorCommand::PrepareBellPortal { reply, .. } => {
                let _ = reply.send(Err(CoreBellPortalRejection::InstanceUnavailable));
            }
            CorePrivateRouteActorCommand::CommitBellPortal { reply, .. }
            | CorePrivateRouteActorCommand::ReconcileBellPortal { reply, .. } => {
                let _ = reply.send(Err(CoreBellPortalRejection::InstanceUnavailable));
            }
            CorePrivateRouteActorCommand::PrepareExtractionTerminal { reply, .. } => {
                let _ = reply.send(Err(CorePrivateRouteRuntimeError::Retired));
            }
            CorePrivateRouteActorCommand::RevalidateExtractionTerminal { reply, .. } => {
                let _ = reply.send(Err(CorePrivateRouteRuntimeError::Retired));
            }
        }
    }
    CorePrivateRouteActorTaskReport { served, abandoned }
}

fn handle_advance(
    control: &Mutex<CorePrivateRouteActorControl>,
    advance: CorePrivateRouteActorAdvance,
) -> CorePrivateRouteActorReply {
    let mut control = lock(control);
    if control.retired {
        return Err(CorePrivateRouteRuntimeError::Retired);
    }
    if control.bell_reservation.is_some() {
        return Err(CorePrivateRouteRuntimeError::TransferInProgress);
    }
    if control.terminal_reservation.is_some() {
        return Err(CorePrivateRouteRuntimeError::TerminalInProgress);
    }
    control.actor.advance(advance).cloned().map_err(Into::into)
}

fn handle_portal_range(
    control: &Mutex<CorePrivateRouteActorControl>,
    in_range: bool,
) -> CorePrivateRouteActorReply {
    let mut control = lock(control);
    if control.retired {
        return Err(CorePrivateRouteRuntimeError::Retired);
    }
    if control.bell_reservation.is_some() {
        return Err(CorePrivateRouteRuntimeError::TransferInProgress);
    }
    if control.terminal_reservation.is_some() {
        return Err(CorePrivateRouteRuntimeError::TerminalInProgress);
    }
    control.actor.set_bell_portal_in_range(in_range)?;
    Ok(control.actor.projection().clone())
}

fn handle_microrealm_authority(
    control: &Mutex<CorePrivateRouteActorControl>,
    expected_state_version: u64,
    phase: CorePrivateRoutePhaseV1,
    bell_portal_in_range: bool,
) -> CorePrivateRouteActorReply {
    let mut control = lock(control);
    if control.retired {
        return Err(CorePrivateRouteRuntimeError::Retired);
    }
    if control.bell_reservation.is_some() {
        return Err(CorePrivateRouteRuntimeError::TransferInProgress);
    }
    if control.terminal_reservation.is_some() {
        return Err(CorePrivateRouteRuntimeError::TerminalInProgress);
    }
    let projection = control.actor.projection();
    if projection.state_version != expected_state_version
        || projection.scene != CorePrivateRouteSceneV1::CoreMicrorealm
        || projection.room.is_some()
        || (phase != CorePrivateRoutePhaseV1::MicrorealmCleared && bell_portal_in_range)
    {
        return Err(CorePrivateRouteRuntimeError::StaleRouteState);
    }
    let advance = match (projection.phase, phase) {
        (current, target) if current == target => None,
        (
            CorePrivateRoutePhaseV1::MicrorealmDormant,
            CorePrivateRoutePhaseV1::MicrorealmWaiting,
        ) => Some(CorePrivateRouteActorAdvance::MicrorealmWaiting),
        (CorePrivateRoutePhaseV1::MicrorealmWaiting, CorePrivateRoutePhaseV1::MicrorealmActive) => {
            Some(CorePrivateRouteActorAdvance::MicrorealmActive)
        }
        (CorePrivateRoutePhaseV1::MicrorealmActive, CorePrivateRoutePhaseV1::MicrorealmCleared) => {
            Some(CorePrivateRouteActorAdvance::MicrorealmCleared)
        }
        _ => return Err(CorePrivateRouteRuntimeError::StaleRouteState),
    };
    if let Some(advance) = advance {
        control.actor.advance(advance)?;
    }
    control
        .actor
        .set_bell_portal_in_range(bell_portal_in_range)?;
    Ok(control.actor.projection().clone())
}

fn handle_fixed_dungeon_authority(
    control: &Mutex<CorePrivateRouteActorControl>,
    expected_state_version: u64,
    room: CorePrivateRouteRoomV1,
    phase: CorePrivateRoutePhaseV1,
) -> CorePrivateRouteActorReply {
    let mut control = lock(control);
    if control.retired {
        return Err(CorePrivateRouteRuntimeError::Retired);
    }
    if control.bell_reservation.is_some() {
        return Err(CorePrivateRouteRuntimeError::TransferInProgress);
    }
    if control.terminal_reservation.is_some() {
        return Err(CorePrivateRouteRuntimeError::TerminalInProgress);
    }
    let projection = control.actor.projection();
    if projection.state_version != expected_state_version
        || projection.scene != CorePrivateRouteSceneV1::BellSepulcher
        || projection.instance_lineage_id.is_none()
    {
        return Err(CorePrivateRouteRuntimeError::StaleRouteState);
    }
    let advances = fixed_dungeon_advances(projection.room, projection.phase, room, phase)
        .ok_or(CorePrivateRouteRuntimeError::StaleRouteState)?;
    for advance in advances {
        control.actor.advance(advance)?;
    }
    Ok(control.actor.projection().clone())
}

fn reply_fixed_dungeon_authority(
    control: &Mutex<CorePrivateRouteActorControl>,
    expected_state_version: u64,
    room: CorePrivateRouteRoomV1,
    phase: CorePrivateRoutePhaseV1,
    reply: oneshot::Sender<CorePrivateRouteActorReply>,
) {
    let _ = reply.send(handle_fixed_dungeon_authority(
        control,
        expected_state_version,
        room,
        phase,
    ));
}

fn fixed_dungeon_advances(
    current_room: Option<CorePrivateRouteRoomV1>,
    current_phase: CorePrivateRoutePhaseV1,
    target_room: CorePrivateRouteRoomV1,
    target_phase: CorePrivateRoutePhaseV1,
) -> Option<Vec<CorePrivateRouteActorAdvance>> {
    use CorePrivateRouteActorAdvance as Advance;
    use CorePrivateRoutePhaseV1 as Phase;
    use CorePrivateRouteRoomV1 as Room;

    let same = current_room == Some(target_room) && current_phase == target_phase;
    if same && canonical_fixed_dungeon_position(target_room, target_phase) {
        return Some(Vec::new());
    }
    if current_room == Some(Room::CaldusArenaB6) && target_room == Room::CaldusArenaB6 {
        return boss_advances(current_phase, target_phase);
    }
    let advances = match (current_room, current_phase, target_room, target_phase) {
        (
            Some(Room::BellVestibuleB0),
            Phase::DungeonVestibule,
            Room::BellCrossB1,
            Phase::RoomDormant,
        ) => {
            vec![Advance::EnterCombatRoom(Room::BellCrossB1)]
        }
        (Some(Room::BellCrossB1), Phase::RoomCleared, Room::BellNaveB2, Phase::RoomDormant) => {
            vec![Advance::EnterCombatRoom(Room::BellNaveB2)]
        }
        (Some(Room::BellNaveB2), Phase::RoomCleared, Room::BellKnightB3, Phase::RoomDormant) => {
            vec![Advance::EnterCombatRoom(Room::BellKnightB3)]
        }
        (Some(Room::BellKnightB3), Phase::RoomCleared, Room::BellRestB4, Phase::Rest) => {
            vec![Advance::EnterRest]
        }
        (Some(Room::BellRestB4), Phase::Rest, Room::BellBridgeB5, Phase::RoomDormant) => {
            vec![Advance::EnterCombatRoom(Room::BellBridgeB5)]
        }
        (Some(Room::BellBridgeB5), Phase::RoomCleared, Room::CaldusArenaB6, Phase::BossStaging) => {
            vec![Advance::EnterBoss]
        }
        (Some(current), Phase::RoomDormant, target, Phase::RoomAwaitingDoorSafety)
            if current == target && is_fixed_combat_room(target) =>
        {
            vec![Advance::RoomAwaitingDoorSafety]
        }
        (Some(current), Phase::RoomDormant, target, Phase::RoomSpawnWarning)
            if current == target && is_fixed_combat_room(target) =>
        {
            vec![Advance::RoomAwaitingDoorSafety, Advance::RoomSpawnWarning]
        }
        (Some(current), Phase::RoomAwaitingDoorSafety, target, Phase::RoomSpawnWarning)
            if current == target && is_fixed_combat_room(target) =>
        {
            vec![Advance::RoomSpawnWarning]
        }
        (Some(current), Phase::RoomSpawnWarning, target, Phase::RoomActive)
            if current == target && is_fixed_combat_room(target) =>
        {
            vec![Advance::RoomActive]
        }
        (Some(current), Phase::RoomSpawnWarning, target, Phase::RoomQuiet)
            if current == target && is_fixed_combat_room(target) =>
        {
            vec![Advance::RoomActive, Advance::RoomQuiet]
        }
        (Some(current), Phase::RoomActive, target, Phase::RoomQuiet)
            if current == target && is_fixed_combat_room(target) =>
        {
            vec![Advance::RoomQuiet]
        }
        (Some(current), Phase::RoomQuiet, target, Phase::RoomCleared)
            if current == target && is_fixed_combat_room(target) =>
        {
            vec![Advance::RoomCleared]
        }
        (Some(current), current_phase, target, Phase::RoomDormant)
            if current == target
                && is_fixed_combat_room(target)
                && matches!(
                    current_phase,
                    Phase::RoomAwaitingDoorSafety | Phase::RoomSpawnWarning | Phase::RoomActive
                ) =>
        {
            vec![Advance::RoomReset]
        }
        _ => return None,
    };
    Some(advances)
}

fn boss_advances(
    current: CorePrivateRoutePhaseV1,
    target: CorePrivateRoutePhaseV1,
) -> Option<Vec<CorePrivateRouteActorAdvance>> {
    use CorePrivateRouteActorAdvance as Advance;
    use CorePrivateRoutePhaseV1 as Phase;

    let advance = match (current, target) {
        (Phase::BossStaging, Phase::BossReadyCountdown) => Advance::BossReadyCountdown,
        (Phase::BossReadyCountdown, Phase::BossIntroduction) => Advance::BossIntroduction,
        (Phase::BossIntroduction, Phase::BossPhaseOne) => Advance::BossPhaseOne,
        (Phase::BossPhaseOne, Phase::BossBreakToTwo) => Advance::BossBreakToTwo,
        (Phase::BossBreakToTwo, Phase::BossPhaseTwo) => Advance::BossPhaseTwo,
        (Phase::BossPhaseTwo, Phase::BossBreakToThree) => Advance::BossBreakToThree,
        (Phase::BossBreakToThree, Phase::BossPhaseThree) => Advance::BossPhaseThree,
        (
            Phase::BossPhaseOne
            | Phase::BossBreakToTwo
            | Phase::BossPhaseTwo
            | Phase::BossBreakToThree
            | Phase::BossPhaseThree,
            Phase::BossDefeated,
        ) => Advance::BossDefeated,
        (Phase::BossDefeated, Phase::BossExitReady) => Advance::BossExitReady,
        (
            Phase::BossReadyCountdown
            | Phase::BossIntroduction
            | Phase::BossPhaseOne
            | Phase::BossBreakToTwo
            | Phase::BossPhaseTwo
            | Phase::BossBreakToThree
            | Phase::BossPhaseThree,
            Phase::BossStaging,
        ) => Advance::BossReset,
        _ => return None,
    };
    Some(vec![advance])
}

const fn canonical_fixed_dungeon_position(
    room: CorePrivateRouteRoomV1,
    phase: CorePrivateRoutePhaseV1,
) -> bool {
    use CorePrivateRoutePhaseV1 as Phase;
    use CorePrivateRouteRoomV1 as Room;
    match room {
        Room::BellVestibuleB0 => matches!(phase, Phase::DungeonVestibule),
        Room::BellCrossB1 | Room::BellNaveB2 | Room::BellKnightB3 | Room::BellBridgeB5 => {
            matches!(
                phase,
                Phase::RoomDormant
                    | Phase::RoomAwaitingDoorSafety
                    | Phase::RoomSpawnWarning
                    | Phase::RoomActive
                    | Phase::RoomQuiet
                    | Phase::RoomCleared
            )
        }
        Room::BellRestB4 => matches!(phase, Phase::Rest),
        Room::CaldusArenaB6 => matches!(
            phase,
            Phase::BossStaging
                | Phase::BossReadyCountdown
                | Phase::BossIntroduction
                | Phase::BossPhaseOne
                | Phase::BossBreakToTwo
                | Phase::BossPhaseTwo
                | Phase::BossBreakToThree
                | Phase::BossPhaseThree
                | Phase::BossDefeated
                | Phase::BossExitReady
        ),
    }
}

const fn is_fixed_combat_room(room: CorePrivateRouteRoomV1) -> bool {
    matches!(
        room,
        CorePrivateRouteRoomV1::BellCrossB1
            | CorePrivateRouteRoomV1::BellNaveB2
            | CorePrivateRouteRoomV1::BellKnightB3
            | CorePrivateRouteRoomV1::BellBridgeB5
    )
}

fn handle_prepare_bell_portal(
    control: &Mutex<CorePrivateRouteActorControl>,
    binding: CoreBellPortalBinding,
) -> Result<CoreBellPortalPermit, CoreBellPortalRejection> {
    let mut control = lock(control);
    if control.retired {
        return Err(CoreBellPortalRejection::InstanceUnavailable);
    }
    let projection = control.actor.projection();
    if control.actor.world_flow_revision() != &binding.content_revision {
        return Err(CoreBellPortalRejection::ServiceUnavailable);
    }
    if projection.character_id != binding.character_id
        || projection.character_version != binding.character_version
        || projection.instance_lineage_id != Some(binding.instance_lineage_id)
    {
        return Err(CoreBellPortalRejection::InstanceUnavailable);
    }
    if projection.scene != CorePrivateRouteSceneV1::CoreMicrorealm
        || projection.phase != CorePrivateRoutePhaseV1::MicrorealmCleared
    {
        return Err(CoreBellPortalRejection::NotCleared);
    }
    if !control.actor.bell_portal_in_range() {
        return Err(CoreBellPortalRejection::OutOfRange);
    }
    if control.bell_reservation.is_some() || control.terminal_reservation.is_some() {
        return Err(CoreBellPortalRejection::TransferInProgress);
    }
    let permit = CoreBellPortalPermit {
        permit_id: derive_permit_id(projection, &binding),
        actor_generation: projection.actor_generation,
        route_state_version: projection.state_version,
        binding,
    };
    control.bell_reservation = Some(permit.clone());
    Ok(permit)
}

fn handle_commit_bell_portal(
    control: &Mutex<CorePrivateRouteActorControl>,
    permit: &CoreBellPortalPermit,
    transition: &CoreBellPortalTransition,
) -> Result<(), CoreBellPortalRejection> {
    if !valid_transition(&permit.binding, transition) {
        return Err(CoreBellPortalRejection::ServiceUnavailable);
    }
    let mut control = lock(control);
    if control.retired
        || control.bell_reservation.as_ref() != Some(permit)
        || control.terminal_reservation.is_some()
        || control.actor.projection().actor_generation != permit.actor_generation
        || control.actor.projection().state_version != permit.route_state_version
    {
        return Err(CoreBellPortalRejection::InstanceUnavailable);
    }
    control
        .actor
        .commit_bell_portal(transition.destination_character_version)
        .map_err(|_| CoreBellPortalRejection::ServiceUnavailable)?;
    control.bell_reservation = None;
    Ok(())
}

fn handle_reconcile_bell_portal(
    control: &Mutex<CorePrivateRouteActorControl>,
    transition: &CoreBellPortalTransition,
) -> Result<(), CoreBellPortalRejection> {
    if !valid_transition(&transition.binding, transition) {
        return Err(CoreBellPortalRejection::ServiceUnavailable);
    }
    let mut control = lock(control);
    if control.retired
        || control.actor.world_flow_revision() != &transition.binding.content_revision
    {
        return Err(CoreBellPortalRejection::InstanceUnavailable);
    }
    if control.terminal_reservation.is_some() {
        return Err(CoreBellPortalRejection::InstanceUnavailable);
    }
    if let Some(reservation) = &control.bell_reservation {
        if reservation.binding != transition.binding {
            return Err(CoreBellPortalRejection::TransferInProgress);
        }
        control.bell_reservation = None;
    }
    let projection = control.actor.projection();
    if projection.character_version > transition.destination_character_version {
        return Ok(());
    }
    if projection.character_version == transition.destination_character_version
        && projection.scene == CorePrivateRouteSceneV1::BellSepulcher
        && projection.instance_lineage_id == Some(transition.binding.instance_lineage_id)
    {
        return Ok(());
    }
    if projection.character_version != transition.binding.character_version
        || projection.character_id != transition.binding.character_id
        || projection.instance_lineage_id != Some(transition.binding.instance_lineage_id)
    {
        return Err(CoreBellPortalRejection::InstanceUnavailable);
    }
    control
        .actor
        .reconcile_bell_portal(transition.destination_character_version)
        .map_err(|_| CoreBellPortalRejection::ServiceUnavailable)?;
    Ok(())
}

fn handle_reconcile_enter_microrealm(
    control: &Mutex<CorePrivateRouteActorControl>,
    transition: &CorePrivateRouteEnterMicrorealmTransition,
) -> CorePrivateRouteActorReply {
    let mut control = lock(control);
    if control.retired {
        return Err(CorePrivateRouteRuntimeError::Retired);
    }
    if control.actor.world_flow_revision() != &transition.content_revision {
        return Err(CorePrivateRouteRuntimeError::ContentAuthorityMismatch);
    }
    let projection = control.actor.projection();
    if projection.character_version == transition.destination_character_version
        && projection.scene == CorePrivateRouteSceneV1::CoreMicrorealm
        && projection.room.is_none()
        && projection.instance_lineage_id == Some(transition.instance_lineage_id)
        && matches!(
            projection.phase,
            CorePrivateRoutePhaseV1::MicrorealmDormant
                | CorePrivateRoutePhaseV1::MicrorealmWaiting
                | CorePrivateRoutePhaseV1::MicrorealmActive
                | CorePrivateRoutePhaseV1::MicrorealmCleared
        )
    {
        if let Some(stored) = &control.enter_microrealm_reconciliation
            && stored != transition
        {
            return Err(CorePrivateRouteRuntimeError::StaleRouteState);
        }
        let projection = projection.clone();
        control.enter_microrealm_reconciliation = Some(transition.clone());
        return Ok(projection);
    }
    if control.bell_reservation.is_some() {
        return Err(CorePrivateRouteRuntimeError::TransferInProgress);
    }
    if control.terminal_reservation.is_some() {
        return Err(CorePrivateRouteRuntimeError::TerminalInProgress);
    }
    if projection.character_version != transition.source_character_version
        || projection.scene != CorePrivateRouteSceneV1::LanternHalls
        || projection.room.is_some()
        || projection.instance_lineage_id.is_some()
        || projection.phase != CorePrivateRoutePhaseV1::Hall
    {
        return Err(CorePrivateRouteRuntimeError::StaleRouteState);
    }
    let projection = control
        .actor
        .advance(CorePrivateRouteActorAdvance::EnterMicrorealm {
            instance_lineage_id: transition.instance_lineage_id,
            destination_character_version: transition.destination_character_version,
        })
        .cloned()
        .map_err(CorePrivateRouteRuntimeError::from)?;
    control.enter_microrealm_reconciliation = Some(transition.clone());
    Ok(projection)
}

fn handle_prepare_extraction_terminal(
    control: &Mutex<CorePrivateRouteActorControl>,
    binding: CorePrivateRouteExtractionBinding,
) -> Result<CorePrivateRouteExtractionPermit, CorePrivateRouteRuntimeError> {
    let mut control = lock(control);
    if control.retired {
        return Err(CorePrivateRouteRuntimeError::Retired);
    }
    if let Some(existing) = control.terminal_reservation.as_ref() {
        if existing.binding() != &binding {
            return Err(CorePrivateRouteRuntimeError::TerminalReservationConflict);
        }
        validate_extraction_permit(&control, existing)?;
        return Ok(existing.clone());
    }
    if control.bell_reservation.is_some() {
        return Err(CorePrivateRouteRuntimeError::TransferInProgress);
    }

    let projection = control.actor.projection();
    let accepted = binding.accepted_route();
    if projection.actor_generation != accepted.actor_generation {
        return Err(CorePrivateRouteRuntimeError::StaleGeneration);
    }
    if projection.content_revision != accepted.content_revision
        || control.actor.world_flow_revision() != binding.world_flow_revision()
    {
        return Err(CorePrivateRouteRuntimeError::ContentAuthorityMismatch);
    }
    if projection.state_version != accepted.state_version || projection != accepted {
        return Err(CorePrivateRouteRuntimeError::StaleRouteState);
    }
    if projection.scene != CorePrivateRouteSceneV1::BellSepulcher
        || projection.room != Some(CorePrivateRouteRoomV1::CaldusArenaB6)
        || projection.phase != CorePrivateRoutePhaseV1::BossExitReady
        || !projection.readiness.extraction_available.is_available()
    {
        return Err(CorePrivateRouteRuntimeError::ExtractionNotReady);
    }

    let actor_generation = projection.actor_generation;
    let accepted_route_state_version = projection.state_version;
    let character_version = projection.character_version;
    let route_content_revision = projection.content_revision.clone();
    let world_flow_revision = control.actor.world_flow_revision().clone();
    let terminal_pending_route_state_version = accepted_route_state_version
        .checked_add(1)
        .ok_or(CorePrivateRouteRuntimeError::StaleRouteState)?;
    let actor_before_terminal = control.actor.clone();
    let pending = match control.actor.begin_extraction_terminal() {
        Ok(pending) => pending.clone(),
        Err(error) => {
            control.actor = actor_before_terminal;
            return Err(error.into());
        }
    };
    if pending.actor_generation != actor_generation
        || pending.state_version != terminal_pending_route_state_version
        || pending.character_version != character_version
        || pending.character_id != binding.accepted_route().character_id
        || pending.instance_lineage_id != binding.accepted_route().instance_lineage_id
        || pending.scene != CorePrivateRouteSceneV1::BellSepulcher
        || pending.room != Some(CorePrivateRouteRoomV1::CaldusArenaB6)
        || pending.phase != CorePrivateRoutePhaseV1::TerminalPending
    {
        control.actor = actor_before_terminal;
        return Err(CorePrivateRouteRuntimeError::StaleRouteState);
    }
    let permit = CorePrivateRouteExtractionPermit {
        permit_id: derive_extraction_permit_id(
            &binding,
            actor_generation,
            accepted_route_state_version,
            terminal_pending_route_state_version,
        ),
        binding,
        actor_generation,
        accepted_route_state_version,
        terminal_pending_route_state_version,
        route_content_revision,
        world_flow_revision,
    };
    control.terminal_reservation = Some(permit.clone());
    Ok(permit)
}

fn handle_revalidate_extraction_terminal(
    control: &Mutex<CorePrivateRouteActorControl>,
    permit: &CorePrivateRouteExtractionPermit,
) -> Result<(), CorePrivateRouteRuntimeError> {
    let control = lock(control);
    validate_extraction_permit(&control, permit)
}

fn validate_extraction_permit(
    control: &CorePrivateRouteActorControl,
    permit: &CorePrivateRouteExtractionPermit,
) -> Result<(), CorePrivateRouteRuntimeError> {
    if control.retired {
        return Err(CorePrivateRouteRuntimeError::Retired);
    }
    if control.terminal_reservation.as_ref() != Some(permit) {
        return Err(CorePrivateRouteRuntimeError::TerminalReservationConflict);
    }
    let projection = control.actor.projection();
    if projection.actor_generation != permit.actor_generation {
        return Err(CorePrivateRouteRuntimeError::StaleGeneration);
    }
    if projection.content_revision != permit.route_content_revision
        || control.actor.world_flow_revision() != &permit.world_flow_revision
    {
        return Err(CorePrivateRouteRuntimeError::ContentAuthorityMismatch);
    }
    if projection.state_version != permit.terminal_pending_route_state_version
        || permit.accepted_route_state_version.checked_add(1)
            != Some(permit.terminal_pending_route_state_version)
        || projection.character_id != permit.binding.accepted_route().character_id
        || projection.character_version != permit.binding.accepted_route().character_version
        || projection.instance_lineage_id != permit.binding.accepted_route().instance_lineage_id
        || projection.scene != CorePrivateRouteSceneV1::BellSepulcher
        || projection.room != Some(CorePrivateRouteRoomV1::CaldusArenaB6)
        || projection.phase != CorePrivateRoutePhaseV1::TerminalPending
    {
        return Err(CorePrivateRouteRuntimeError::StaleRouteState);
    }
    Ok(())
}

fn release_bell_reservation(
    control: &Mutex<CorePrivateRouteActorControl>,
    permit: &CoreBellPortalPermit,
) {
    let mut control = lock(control);
    if control.bell_reservation.as_ref() == Some(permit) {
        control.bell_reservation = None;
    }
}

fn validate_extraction_lease_binding(
    lease: CorePrivateRouteActorLease,
    binding: &CorePrivateRouteExtractionBinding,
) -> Result<(), CorePrivateRouteRuntimeError> {
    if binding.account_id() != lease.account_id()
        || binding.accepted_route().character_id != lease.character_id()
    {
        return Err(CorePrivateRouteRuntimeError::InvalidExtractionBinding);
    }
    if binding.accepted_route().actor_generation != lease.actor_generation() {
        return Err(CorePrivateRouteRuntimeError::StaleGeneration);
    }
    Ok(())
}

fn derive_extraction_permit_id(
    binding: &CorePrivateRouteExtractionBinding,
    actor_generation: u64,
    accepted_route_state_version: u64,
    terminal_pending_route_state_version: u64,
) -> [u8; 16] {
    let mut hasher = blake3::Hasher::new_derive_key(EXTRACTION_PERMIT_CONTEXT);
    let lineage_id = binding
        .accepted_route()
        .instance_lineage_id
        .expect("validated extraction binding has one lineage");
    let actor_generation = actor_generation.to_le_bytes();
    let accepted_route_state_version = accepted_route_state_version.to_le_bytes();
    let terminal_pending_route_state_version = terminal_pending_route_state_version.to_le_bytes();
    let account_id = binding.account_id();
    let character_id = binding.accepted_route().character_id;
    let entry_restore_point_id = binding.entry_restore_point_id();
    let encounter_id = binding.exit().encounter_id();
    let exit_instance_id = binding.exit().exit_instance_id();
    let extraction_request_id = binding.exit().extraction_request_id();
    let extraction_receipt_id = binding.exit().extraction_receipt_id();
    let terminal_id = binding.exit().terminal_id();
    for part in [
        account_id.as_slice(),
        character_id.as_slice(),
        lineage_id.as_slice(),
        entry_restore_point_id.as_slice(),
        encounter_id.as_slice(),
        exit_instance_id.as_slice(),
        extraction_request_id.as_slice(),
        extraction_receipt_id.as_slice(),
        terminal_id.as_slice(),
        actor_generation.as_slice(),
        accepted_route_state_version.as_slice(),
        terminal_pending_route_state_version.as_slice(),
        binding
            .accepted_route()
            .content_revision
            .records_blake3
            .as_str()
            .as_bytes(),
        binding
            .accepted_route()
            .content_revision
            .assets_blake3
            .as_str()
            .as_bytes(),
        binding
            .accepted_route()
            .content_revision
            .localization_blake3
            .as_str()
            .as_bytes(),
        binding
            .world_flow_revision()
            .records_blake3
            .as_str()
            .as_bytes(),
        binding
            .world_flow_revision()
            .assets_blake3
            .as_str()
            .as_bytes(),
        binding
            .world_flow_revision()
            .localization_blake3
            .as_str()
            .as_bytes(),
    ] {
        hasher.update(&(part.len() as u64).to_le_bytes());
        hasher.update(part);
    }
    let mut permit_id = [0; 16];
    permit_id.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
    if permit_id == [0; 16] {
        permit_id[15] = 1;
    }
    permit_id
}

fn pairwise_distinct(identities: &[[u8; 16]]) -> bool {
    identities
        .iter()
        .enumerate()
        .all(|(index, identity)| !identities[index + 1..].contains(identity))
}

fn zero_world_flow_revision(revision: &WorldFlowContentRevisionV1) -> bool {
    [
        &revision.records_blake3,
        &revision.assets_blake3,
        &revision.localization_blake3,
    ]
    .into_iter()
    .any(|hash| hash.as_str().bytes().all(|byte| byte == b'0'))
}

fn derive_permit_id(
    projection: &CorePrivateRouteStateV1,
    binding: &CoreBellPortalBinding,
) -> [u8; 16] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"gravebound/core-private-route/bell-permit/v1\0");
    hasher.update(&binding.account_id);
    hasher.update(&binding.character_id);
    hasher.update(&binding.mutation_id);
    hasher.update(&binding.instance_lineage_id);
    hasher.update(&binding.entry_restore_point_id);
    hasher.update(&projection.actor_generation.to_le_bytes());
    hasher.update(&projection.state_version.to_le_bytes());
    let hash = hasher.finalize();
    let mut permit_id = [0_u8; 16];
    permit_id.copy_from_slice(&hash.as_bytes()[..16]);
    if permit_id.iter().all(|byte| *byte == 0) {
        permit_id[15] = 1;
    }
    permit_id
}

fn valid_transition(
    binding: &CoreBellPortalBinding,
    transition: &CoreBellPortalTransition,
) -> bool {
    transition.binding == *binding
        && transition.transfer_id.iter().any(|byte| *byte != 0)
        && binding.character_version.checked_add(1)
            == Some(transition.destination_character_version)
}

fn validate_enter_microrealm_transition(
    transition: &CorePrivateRouteEnterMicrorealmTransition,
) -> Result<(), CorePrivateRouteRuntimeError> {
    if transition.transfer_id == [0; 16]
        || transition.instance_lineage_id == [0; 16]
        || transition.source_character_version == 0
        || transition.source_character_version.checked_add(1)
            != Some(transition.destination_character_version)
        || zero_world_flow_revision(&transition.content_revision)
    {
        return Err(CorePrivateRouteRuntimeError::InvalidActorBinding);
    }
    Ok(())
}

fn validate_return_to_character_select_transition(
    transition: &CorePrivateRouteReturnToCharacterSelectTransition,
) -> Result<(), CorePrivateRouteRuntimeError> {
    if transition.transfer_id == [0; 16]
        || transition.source_character_version == 0
        || transition.source_character_version.checked_add(1)
            != Some(transition.destination_character_version)
        || zero_world_flow_revision(&transition.content_revision)
    {
        return Err(CorePrivateRouteRuntimeError::InvalidActorBinding);
    }
    Ok(())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Debug, Error)]
pub enum CorePrivateRouteRuntimeError {
    #[error("private-route runtime is retired")]
    Retired,
    #[error("private-route actor binding is invalid")]
    InvalidActorBinding,
    #[error("an actor is already active for this account")]
    AccountAlreadyActive,
    #[error("private-route actor is already registered")]
    ActorAlreadyRegistered,
    #[error("private-route actor is unavailable")]
    ActorUnavailable,
    #[error("private-route actor generation is stale")]
    StaleGeneration,
    #[error("private-route extraction binding is invalid")]
    InvalidExtractionBinding,
    #[error("private-route content authority does not match the actor")]
    ContentAuthorityMismatch,
    #[error("private-route state authority is stale")]
    StaleRouteState,
    #[error("private-route extraction is not ready")]
    ExtractionNotReady,
    #[error("a terminal operation pins the current actor generation")]
    TerminalInProgress,
    #[error("another terminal reservation already owns this actor generation")]
    TerminalReservationConflict,
    #[error("a Bell transfer pins the current actor generation")]
    TransferInProgress,
    #[error("private-route runtime requires an active Tokio runtime")]
    RuntimeUnavailable,
    #[error("private-route shutdown has not started")]
    ShutdownNotStarted,
    #[error("private-route actor task failed")]
    ActorTaskFailed(#[source] tokio::task::JoinError),
    #[error(transparent)]
    Actor(#[from] CorePrivateRouteActorError),
}
