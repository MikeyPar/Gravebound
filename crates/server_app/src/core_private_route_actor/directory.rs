use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex, MutexGuard},
};

use protocol::{CorePrivateRoutePhaseV1, CorePrivateRouteSceneV1, CorePrivateRouteStateV1};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateRouteRuntimeReport {
    pub served_actor_commands: u64,
    pub abandoned_actor_commands: u64,
    pub remaining_actor_tasks: usize,
    pub remaining_registered_actors: usize,
    pub remaining_portal_reservations: usize,
    pub zero_residue: bool,
}

struct CorePrivateRouteActorControl {
    actor: CorePrivateRouteActor,
    reservation: Option<CoreBellPortalPermit>,
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
            reservation: None,
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

    pub async fn set_bell_portal_in_range(
        &self,
        lease: CorePrivateRouteActorLease,
        in_range: bool,
    ) -> CorePrivateRouteActorReply {
        self.actor_handle(lease)?
            .set_bell_portal_range(in_range)
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
                if control.reservation.is_some() {
                    return Err(CorePrivateRouteRuntimeError::TransferInProgress);
                }
                control.retired = true;
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

    /// Closes admission and retires every actor before connection workers are joined. Any Bell
    /// permit still held by a world-flow task becomes unusable and will reconcile from the durable
    /// receipt after restart rather than preserving an in-memory danger actor.
    pub fn begin_shutdown(&self) {
        let mut state = lock(&self.inner.state);
        state.accepting = false;
        state.shutdown_started = true;
        let actors = std::mem::take(&mut state.actors);
        state.active_account.clear();
        for (key, mut entry) in actors {
            let generation = {
                let mut control = lock(&entry.control);
                control.retired = true;
                control.reservation = None;
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
            .filter(|entry| lock(&entry.control).reservation.is_some())
            .count();
        let report = CorePrivateRouteRuntimeReport {
            served_actor_commands: state.served_actor_commands,
            abandoned_actor_commands: state.abandoned_actor_commands,
            remaining_actor_tasks: state.retired_tasks.len(),
            remaining_registered_actors: state.actors.len(),
            remaining_portal_reservations,
            zero_residue: state.retired_tasks.is_empty()
                && state.actors.is_empty()
                && state.active_account.is_empty()
                && remaining_portal_reservations == 0,
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

    fn control_for_binding(
        &self,
        binding: &CoreBellPortalBinding,
    ) -> Result<Arc<Mutex<CorePrivateRouteActorControl>>, CoreBellPortalRejection> {
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
        state
            .actors
            .get(key)
            .map(|entry| Arc::clone(&entry.control))
            .ok_or(CoreBellPortalRejection::InstanceUnavailable)
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
        let mut control = lock(&self.control);
        if control.reservation.as_ref() == Some(&self.permit) {
            control.reservation = None;
        }
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
        let control = self.control_for_binding(&binding)?;
        let mut actor = lock(&control);
        if actor.retired {
            return Err(CoreBellPortalRejection::InstanceUnavailable);
        }
        let projection = actor.actor.projection();
        if actor.actor.world_flow_revision() != &binding.content_revision {
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
        if !actor.actor.bell_portal_in_range() {
            return Err(CoreBellPortalRejection::OutOfRange);
        }
        if actor.reservation.is_some() {
            return Err(CoreBellPortalRejection::TransferInProgress);
        }
        let permit_id = derive_permit_id(projection, &binding);
        let permit = CoreBellPortalPermit {
            binding,
            permit_id,
            actor_generation: projection.actor_generation,
            route_state_version: projection.state_version,
        };
        actor.reservation = Some(permit.clone());
        drop(actor);
        // No suspension occurs between installing the reservation and constructing this guard.
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
        let current = self.control_for_binding(&transition.binding)?;
        if !Arc::ptr_eq(&current, &lease.control) {
            return Err(CoreBellPortalRejection::InstanceUnavailable);
        }
        let mut control = lock(&current);
        if control.retired
            || control.reservation.as_ref() != Some(&lease.permit)
            || control.actor.projection().actor_generation != lease.permit.actor_generation
            || control.actor.projection().state_version != lease.permit.route_state_version
        {
            return Err(CoreBellPortalRejection::InstanceUnavailable);
        }
        control
            .actor
            .commit_bell_portal(transition.destination_character_version)
            .map_err(|_| CoreBellPortalRejection::ServiceUnavailable)?;
        control.reservation = None;
        lease.armed = false;
        Ok(())
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
        let control = self.control_for_binding(&transition.binding)?;
        let mut control = lock(&control);
        if control.retired
            || control.actor.world_flow_revision() != &transition.binding.content_revision
        {
            return Err(CoreBellPortalRejection::InstanceUnavailable);
        }
        if let Some(reservation) = &control.reservation {
            if reservation.binding != transition.binding {
                return Err(CoreBellPortalRejection::TransferInProgress);
            }
            control.reservation = None;
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
        }
    }
    let mut abandoned = 0_u64;
    while let Ok(command) = inbox.try_recv() {
        abandoned = abandoned.saturating_add(1);
        match command {
            CorePrivateRouteActorCommand::Advance { reply, .. }
            | CorePrivateRouteActorCommand::SetBellPortalRange { reply, .. } => {
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
    if control.reservation.is_some() {
        return Err(CorePrivateRouteRuntimeError::TransferInProgress);
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
    if control.reservation.is_some() {
        return Err(CorePrivateRouteRuntimeError::TransferInProgress);
    }
    control.actor.set_bell_portal_in_range(in_range)?;
    Ok(control.actor.projection().clone())
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
