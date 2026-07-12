//! Bounded authoritative instance ownership and scheduler diagnostics for `GB-M02-08`.

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU64;
use std::path::Path;
use std::time::Instant;

use protocol::{
    ControlEvent, InputFrame, ReliableEvent, ReliableEventFrame, SessionControlFrame,
    SessionControlRequest, SessionControlResultCode, SnapshotChunk, WireMessage, WireText,
};
use thiserror::Error;

use crate::{
    DirectoryTickOutput, InputDisposition, LifecycleError, LifecycleResponse, ManagedSession,
    SessionDirectory, SessionOwnerId, TransportId,
};

pub const M02_ARENA_CAPACITY: usize = 16;
pub const M02_SOAK_BOT_COUNT: usize = 16;
pub const M02_SOAK_DURATION_TICKS: u64 = 2 * 60 * 60 * 30;
pub const SERVER_TICK_BUDGET_MICROS: u64 = 33_333;
const M02_P95_LIMIT_MICROS: u64 = 20_000;
const M02_P99_LIMIT_MICROS: u64 = 30_000;
const REQUIRED_HEADROOM_BASIS_POINTS: u16 = 3_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HostedInstanceId(NonZeroU64);

impl HostedInstanceId {
    pub fn new(value: u64) -> Result<Self, InstanceError> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(InstanceError::ZeroInstanceIdentity)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceKind {
    CombatArena,
    Realm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArenaInstancePhase {
    Allocating,
    Active,
    Draining,
    Closed,
}

#[derive(Debug)]
struct HostedInstance {
    kind: InstanceKind,
    content_bundle_version: WireText<32>,
    content: sim_content::AuthorityCombatTestContent,
    phase: ArenaInstancePhase,
    directory: SessionDirectory,
}

impl HostedInstance {
    fn allocating(content_root: &Path) -> Result<Self, InstanceError> {
        let (package, report) = sim_content::load_and_validate(content_root)
            .map_err(|error| InstanceError::Content(error.to_string()))?;
        if report.content_version != "fp.1.0.0" {
            return Err(InstanceError::UnsupportedContentVersion(
                report.content_version,
            ));
        }
        let content = sim_content::first_playable_authority_combat_test(&package)
            .map_err(|error| InstanceError::Content(error.to_string()))?;
        Ok(Self {
            kind: InstanceKind::CombatArena,
            content_bundle_version: WireText::new(report.content_version)
                .map_err(|_| InstanceError::ContentVersionEncoding)?,
            content,
            phase: ArenaInstancePhase::Allocating,
            directory: SessionDirectory::default(),
        })
    }

    fn has_capacity(&self) -> bool {
        matches!(self.phase, ArenaInstancePhase::Active)
            && self.directory.len() < M02_ARENA_CAPACITY
    }
}

#[derive(Debug)]
pub struct InstanceControlResponse {
    pub instance_id: HostedInstanceId,
    pub lifecycle: LifecycleResponse,
}

#[derive(Debug)]
pub struct SchedulerSnapshotBatch {
    pub instance_id: HostedInstanceId,
    pub owner: SessionOwnerId,
    pub snapshots: Vec<SnapshotChunk>,
}

#[derive(Debug)]
pub struct SchedulerFrame {
    pub scheduler_tick: u64,
    pub session_steps: usize,
    pub elapsed_micros: u64,
    pub snapshot_batches: Vec<SchedulerSnapshotBatch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickTimingReport {
    pub sample_count: usize,
    pub mean_micros: u64,
    pub p95_micros: u64,
    pub p99_micros: u64,
    pub maximum_micros: u64,
    pub mean_headroom_basis_points: u16,
    pub passes_m02_limits: bool,
}

#[derive(Debug)]
pub struct InstanceDiagnostics {
    pub scheduler_frames: u64,
    pub session_steps: u64,
    pub admissions: u64,
    pub reconnects: u64,
    pub retired_sessions: u64,
    pub allocated_instances: u64,
    pub closed_instances: u64,
    pub invalid_states: u64,
    pub simulation_stalls: u64,
    tick_micros: Vec<u64>,
    tick_sample_count: usize,
    tick_sample_cursor: usize,
    total_tick_micros: u128,
}

impl Default for InstanceDiagnostics {
    fn default() -> Self {
        let sample_capacity = usize::try_from(M02_SOAK_DURATION_TICKS)
            .expect("M02 soak duration fits the supported platform");
        Self {
            scheduler_frames: 0,
            session_steps: 0,
            admissions: 0,
            reconnects: 0,
            retired_sessions: 0,
            allocated_instances: 0,
            closed_instances: 0,
            invalid_states: 0,
            simulation_stalls: 0,
            tick_micros: vec![0; sample_capacity],
            tick_sample_count: 0,
            tick_sample_cursor: 0,
            total_tick_micros: 0,
        }
    }
}

impl InstanceDiagnostics {
    fn increment(value: &mut u64) -> Result<(), InstanceError> {
        *value = value
            .checked_add(1)
            .ok_or(InstanceError::DiagnosticCounterExhausted)?;
        Ok(())
    }

    fn add(value: &mut u64, amount: usize) -> Result<(), InstanceError> {
        let amount =
            u64::try_from(amount).map_err(|_| InstanceError::DiagnosticCounterExhausted)?;
        *value = value
            .checked_add(amount)
            .ok_or(InstanceError::DiagnosticCounterExhausted)?;
        Ok(())
    }

    fn record_tick(&mut self, elapsed_micros: u64) -> Result<(), InstanceError> {
        if self.tick_micros.is_empty() {
            return Err(InstanceError::TickSampleStorageUnavailable);
        }
        let index = if self.tick_sample_count < self.tick_micros.len() {
            let index = self.tick_sample_count;
            self.tick_sample_count = self
                .tick_sample_count
                .checked_add(1)
                .ok_or(InstanceError::TickTimingOverflow)?;
            index
        } else {
            let index = self.tick_sample_cursor;
            self.total_tick_micros = self
                .total_tick_micros
                .checked_sub(u128::from(self.tick_micros[index]))
                .ok_or(InstanceError::TickTimingOverflow)?;
            self.tick_sample_cursor = (self.tick_sample_cursor + 1) % self.tick_micros.len();
            index
        };
        self.tick_micros[index] = elapsed_micros;
        self.total_tick_micros = self
            .total_tick_micros
            .checked_add(u128::from(elapsed_micros))
            .ok_or(InstanceError::TickTimingOverflow)?;
        Self::increment(&mut self.scheduler_frames)
    }

    pub fn timing_report(&self) -> Result<Option<TickTimingReport>, InstanceError> {
        if self.tick_sample_count == 0 {
            return Ok(None);
        }
        let mut sorted = self.tick_micros[..self.tick_sample_count].to_vec();
        sorted.sort_unstable();
        let count = u128::try_from(sorted.len()).map_err(|_| InstanceError::TickTimingOverflow)?;
        let mean = self
            .total_tick_micros
            .checked_add(count - 1)
            .ok_or(InstanceError::TickTimingOverflow)?
            / count;
        let mean_micros = u64::try_from(mean).map_err(|_| InstanceError::TickTimingOverflow)?;
        let consumed_basis_points = mean
            .checked_mul(10_000)
            .and_then(|value| value.checked_add(u128::from(SERVER_TICK_BUDGET_MICROS) - 1))
            .ok_or(InstanceError::TickTimingOverflow)?
            / u128::from(SERVER_TICK_BUDGET_MICROS);
        let consumed_basis_points = u16::try_from(consumed_basis_points.min(10_000))
            .map_err(|_| InstanceError::TickTimingOverflow)?;
        let mean_headroom_basis_points = 10_000_u16.saturating_sub(consumed_basis_points);
        let p95_micros = nearest_rank(&sorted, 95);
        let p99_micros = nearest_rank(&sorted, 99);
        let maximum_micros = *sorted.last().ok_or(InstanceError::TickTimingOverflow)?;
        Ok(Some(TickTimingReport {
            sample_count: sorted.len(),
            mean_micros,
            p95_micros,
            p99_micros,
            maximum_micros,
            mean_headroom_basis_points,
            passes_m02_limits: p95_micros <= M02_P95_LIMIT_MICROS
                && p99_micros <= M02_P99_LIMIT_MICROS
                && mean_headroom_basis_points >= REQUIRED_HEADROOM_BASIS_POINTS,
        }))
    }
}

#[derive(Debug)]
pub struct InstanceScheduler {
    instances: BTreeMap<HostedInstanceId, HostedInstance>,
    owner_index: BTreeMap<SessionOwnerId, HostedInstanceId>,
    next_instance_id: u64,
    scheduler_tick: u64,
    accepting: bool,
    diagnostics: InstanceDiagnostics,
}

impl Default for InstanceScheduler {
    fn default() -> Self {
        Self {
            instances: BTreeMap::new(),
            owner_index: BTreeMap::new(),
            next_instance_id: 1,
            scheduler_tick: 0,
            accepting: true,
            diagnostics: InstanceDiagnostics::default(),
        }
    }
}

impl InstanceScheduler {
    #[must_use]
    pub const fn is_accepting(&self) -> bool {
        self.accepting
    }

    #[must_use]
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }

    #[must_use]
    pub fn owner_count(&self) -> usize {
        self.owner_index.len()
    }

    #[must_use]
    pub const fn scheduler_tick(&self) -> u64 {
        self.scheduler_tick
    }

    #[must_use]
    pub const fn diagnostics(&self) -> &InstanceDiagnostics {
        &self.diagnostics
    }

    #[must_use]
    pub fn instance_for_owner(&self, owner: SessionOwnerId) -> Option<HostedInstanceId> {
        self.owner_index.get(&owner).copied()
    }

    #[must_use]
    pub fn instance_phase(&self, id: HostedInstanceId) -> Option<ArenaInstancePhase> {
        self.instances.get(&id).map(|instance| instance.phase)
    }

    #[must_use]
    pub fn instance_kind(&self, id: HostedInstanceId) -> Option<InstanceKind> {
        self.instances.get(&id).map(|instance| instance.kind)
    }

    #[must_use]
    pub fn instance_content_version(&self, id: HostedInstanceId) -> Option<&WireText<32>> {
        self.instances
            .get(&id)
            .map(|instance| &instance.content_bundle_version)
    }

    pub fn admit_or_route_control(
        &mut self,
        owner: SessionOwnerId,
        transport: TransportId,
        frame: &SessionControlFrame,
        content_root: &Path,
        server_monotonic_micros: u64,
    ) -> Result<InstanceControlResponse, InstanceError> {
        if !self.accepting {
            return Err(InstanceError::SchedulerDraining);
        }
        if let Some(instance_id) = self.owner_index.get(&owner).copied() {
            let instance = self
                .instances
                .get_mut(&instance_id)
                .ok_or(InstanceError::OwnerIndexDrift)?;
            let lifecycle = instance.directory.handle_control_with_compiled_content(
                owner,
                transport,
                frame,
                &instance.content,
                server_monotonic_micros,
            )?;
            if control_code(&lifecycle.event)? == SessionControlResultCode::Reattached {
                InstanceDiagnostics::increment(&mut self.diagnostics.reconnects)?;
            }
            return Ok(InstanceControlResponse {
                instance_id,
                lifecycle,
            });
        }
        if !matches!(frame.request, SessionControlRequest::Join) {
            return Err(InstanceError::OwnerNotAssigned);
        }
        let candidate = self
            .instances
            .iter()
            .find_map(|(id, instance)| instance.has_capacity().then_some(*id));
        if let Some(instance_id) = candidate {
            let instance = self
                .instances
                .get_mut(&instance_id)
                .ok_or(InstanceError::InstanceNotFound)?;
            let lifecycle = instance.directory.handle_control_with_compiled_content(
                owner,
                transport,
                frame,
                &instance.content,
                server_monotonic_micros,
            )?;
            require_joined(&lifecycle.event)?;
            self.owner_index.insert(owner, instance_id);
            InstanceDiagnostics::increment(&mut self.diagnostics.admissions)?;
            return Ok(InstanceControlResponse {
                instance_id,
                lifecycle,
            });
        }
        let instance_id = HostedInstanceId::new(self.next_instance_id)?;
        let next_instance_id = self
            .next_instance_id
            .checked_add(1)
            .ok_or(InstanceError::InstanceIdentityExhausted)?;
        let mut instance = HostedInstance::allocating(content_root)?;
        let lifecycle = instance.directory.handle_control_with_compiled_content(
            owner,
            transport,
            frame,
            &instance.content,
            server_monotonic_micros,
        )?;
        require_joined(&lifecycle.event)?;
        instance.phase = ArenaInstancePhase::Active;
        self.instances.insert(instance_id, instance);
        self.owner_index.insert(owner, instance_id);
        self.next_instance_id = next_instance_id;
        InstanceDiagnostics::increment(&mut self.diagnostics.allocated_instances)?;
        InstanceDiagnostics::increment(&mut self.diagnostics.admissions)?;
        Ok(InstanceControlResponse {
            instance_id,
            lifecycle,
        })
    }

    pub fn submit_input(
        &mut self,
        owner: SessionOwnerId,
        transport: TransportId,
        frame: &InputFrame,
    ) -> Result<InputDisposition, InstanceError> {
        self.session_mut(owner)?
            .submit_input(transport, frame)
            .map_err(Into::into)
    }

    pub fn handle_gameplay_reliable(
        &mut self,
        owner: SessionOwnerId,
        transport: TransportId,
        message: WireMessage,
    ) -> Result<WireMessage, InstanceError> {
        self.session_mut(owner)?
            .handle_gameplay_reliable(transport, message)
            .map_err(Into::into)
    }

    pub fn transport_lost(
        &mut self,
        owner: SessionOwnerId,
        transport: TransportId,
    ) -> Result<(), InstanceError> {
        self.session_mut(owner)?
            .transport_lost(transport)
            .map_err(Into::into)
    }

    pub fn tick(&mut self) -> Result<SchedulerFrame, InstanceError> {
        let started = Instant::now();
        let scheduler_tick = self
            .scheduler_tick
            .checked_add(1)
            .ok_or(InstanceError::SchedulerTickExhausted)?;
        let mut snapshot_batches = Vec::new();
        let mut session_steps = 0_usize;
        for (instance_id, instance) in &mut self.instances {
            if !matches!(
                instance.phase,
                ArenaInstancePhase::Active | ArenaInstancePhase::Draining
            ) {
                continue;
            }
            for output in instance.directory.tick_simulation_active()? {
                validate_tick_step(&output, &mut self.diagnostics)?;
                session_steps = session_steps
                    .checked_add(1)
                    .ok_or(InstanceError::DiagnosticCounterExhausted)?;
                if !output.snapshots.is_empty() {
                    snapshot_batches.push(SchedulerSnapshotBatch {
                        instance_id: *instance_id,
                        owner: output.owner,
                        snapshots: output.snapshots,
                    });
                }
            }
        }
        let elapsed_micros = u64::try_from(started.elapsed().as_micros())
            .map_err(|_| InstanceError::TickTimingOverflow)?;
        self.scheduler_tick = scheduler_tick;
        InstanceDiagnostics::add(&mut self.diagnostics.session_steps, session_steps)?;
        self.diagnostics.record_tick(elapsed_micros)?;
        Ok(SchedulerFrame {
            scheduler_tick,
            session_steps,
            elapsed_micros,
            snapshot_batches,
        })
    }

    /// Retires terminal sessions only after callers have delivered the frame's final snapshots.
    pub fn retire_resolved(&mut self) -> Result<Vec<SessionOwnerId>, InstanceError> {
        self.retire_resolved_excluding(&BTreeSet::new())
    }

    /// Retains selected terminal owners while a runtime still needs their reconnect tombstone.
    pub fn retire_resolved_excluding(
        &mut self,
        retained_owners: &BTreeSet<SessionOwnerId>,
    ) -> Result<Vec<SessionOwnerId>, InstanceError> {
        let retirement_plan: Vec<_> = self
            .instances
            .iter()
            .map(|(instance_id, instance)| {
                let owners: Vec<_> = instance
                    .directory
                    .resolved_owner_ids()
                    .into_iter()
                    .filter(|owner| !retained_owners.contains(owner))
                    .collect();
                (*instance_id, owners)
            })
            .filter(|(_, owners)| !owners.is_empty())
            .collect();

        // Validate the complete cross-index plan before mutating either owner store.
        // This keeps an invariant failure from leaving membership half-retired.
        if retirement_plan.iter().any(|(instance_id, owners)| {
            owners
                .iter()
                .any(|owner| self.owner_index.get(owner) != Some(instance_id))
        }) {
            self.diagnostics.invalid_states = self.diagnostics.invalid_states.saturating_add(1);
            return Err(InstanceError::OwnerIndexDrift);
        }

        let retired_count = retirement_plan
            .iter()
            .try_fold(0_usize, |total, (_, owners)| {
                total.checked_add(owners.len())
            })
            .ok_or(InstanceError::DiagnosticCounterExhausted)?;
        let closed_instances: Vec<_> = retirement_plan
            .iter()
            .filter_map(|(instance_id, owners)| {
                let instance = self
                    .instances
                    .get(instance_id)
                    .expect("retirement plan references a hosted instance");
                (matches!(instance.phase, ArenaInstancePhase::Active)
                    && instance.directory.len() == owners.len())
                .then_some(*instance_id)
            })
            .collect();
        let retired_total = self
            .diagnostics
            .retired_sessions
            .checked_add(
                u64::try_from(retired_count)
                    .map_err(|_| InstanceError::DiagnosticCounterExhausted)?,
            )
            .ok_or(InstanceError::DiagnosticCounterExhausted)?;
        let closed_total = self
            .diagnostics
            .closed_instances
            .checked_add(
                u64::try_from(closed_instances.len())
                    .map_err(|_| InstanceError::DiagnosticCounterExhausted)?,
            )
            .ok_or(InstanceError::DiagnosticCounterExhausted)?;

        let mut retired = Vec::with_capacity(retired_count);
        for (instance_id, owners) in retirement_plan {
            let instance = self
                .instances
                .get_mut(&instance_id)
                .expect("validated retirement plan references a hosted instance");
            for owner in owners {
                let removed = instance.directory.remove_resolved(owner);
                debug_assert!(removed, "validated resolved session must remain removable");
                self.owner_index.remove(&owner);
                retired.push(owner);
            }
        }
        for instance_id in &closed_instances {
            self.instances.remove(instance_id);
        }
        self.diagnostics.retired_sessions = retired_total;
        self.diagnostics.closed_instances = closed_total;
        Ok(retired)
    }

    pub fn begin_shutdown(
        &mut self,
    ) -> Result<Vec<(HostedInstanceId, TransportId, ReliableEventFrame)>, InstanceError> {
        if !self.accepting {
            return Ok(Vec::new());
        }
        self.accepting = false;
        let mut events = Vec::new();
        for (instance_id, instance) in &mut self.instances {
            instance.phase = ArenaInstancePhase::Draining;
            for (transport, event) in instance.directory.begin_shutdown()? {
                events.push((*instance_id, transport, event));
            }
        }
        Ok(events)
    }

    pub fn finish_shutdown(&mut self) -> Result<(), InstanceError> {
        if self.accepting {
            return Err(InstanceError::ShutdownNotStarted);
        }
        let instance_count = self.instances.len();
        for instance in self.instances.values_mut() {
            instance.directory.finish_shutdown()?;
            instance.phase = ArenaInstancePhase::Closed;
        }
        self.instances.clear();
        self.owner_index.clear();
        InstanceDiagnostics::add(&mut self.diagnostics.closed_instances, instance_count)?;
        Ok(())
    }

    fn session_mut(&mut self, owner: SessionOwnerId) -> Result<&mut ManagedSession, InstanceError> {
        let instance_id = self
            .owner_index
            .get(&owner)
            .copied()
            .ok_or(InstanceError::OwnerNotAssigned)?;
        self.instances
            .get_mut(&instance_id)
            .ok_or(InstanceError::OwnerIndexDrift)?
            .directory
            .session_mut(owner)
            .ok_or(InstanceError::OwnerIndexDrift)
    }
}

fn validate_tick_step(
    output: &DirectoryTickOutput,
    diagnostics: &mut InstanceDiagnostics,
) -> Result<(), InstanceError> {
    let expected = output
        .before_tick
        .checked_add(1)
        .ok_or(InstanceError::SchedulerTickExhausted)?;
    if output.after_tick != expected {
        diagnostics.simulation_stalls = diagnostics.simulation_stalls.saturating_add(1);
        return Err(InstanceError::SimulationTickDrift {
            owner: output.owner,
            before: output.before_tick,
            after: output.after_tick,
        });
    }
    Ok(())
}

fn control_code(event: &ReliableEventFrame) -> Result<SessionControlResultCode, InstanceError> {
    let ReliableEvent::Control(ControlEvent::SessionResult(result)) = &event.event else {
        return Err(InstanceError::UnexpectedControlResult);
    };
    Ok(result.code)
}

fn require_joined(event: &ReliableEventFrame) -> Result<(), InstanceError> {
    if control_code(event)? != SessionControlResultCode::Joined {
        return Err(InstanceError::UnexpectedControlResult);
    }
    Ok(())
}

fn nearest_rank(sorted: &[u64], percentile: usize) -> u64 {
    let rank = sorted.len().saturating_mul(percentile).div_ceil(100);
    sorted[rank.saturating_sub(1)]
}

#[derive(Debug, Error)]
pub enum InstanceError {
    #[error("hosted instance identity must be nonzero")]
    ZeroInstanceIdentity,
    #[error("hosted instance identity exhausted")]
    InstanceIdentityExhausted,
    #[error("hosted instance was not found")]
    InstanceNotFound,
    #[error("session owner is not assigned to a hosted instance")]
    OwnerNotAssigned,
    #[error("owner-to-instance routing index disagrees with instance membership")]
    OwnerIndexDrift,
    #[error("instance scheduler is draining and rejects admission")]
    SchedulerDraining,
    #[error("instance content version could not be encoded")]
    ContentVersionEncoding,
    #[error("M02 instance requires fp.1.0.0, received {0}")]
    UnsupportedContentVersion(String),
    #[error("instance content compilation failed: {0}")]
    Content(String),
    #[error("instance scheduler tick exhausted")]
    SchedulerTickExhausted,
    #[error("tick diagnostics rolling storage is unavailable")]
    TickSampleStorageUnavailable,
    #[error("tick timing arithmetic overflowed")]
    TickTimingOverflow,
    #[error("instance diagnostic counter exhausted")]
    DiagnosticCounterExhausted,
    #[error("instance scheduler shutdown must begin before finish")]
    ShutdownNotStarted,
    #[error("lifecycle returned an unexpected Control result")]
    UnexpectedControlResult,
    #[error(
        "logical session {owner:?} did not advance exactly once: before={before}, after={after}"
    )]
    SimulationTickDrift {
        owner: SessionOwnerId,
        before: u64,
        after: u64,
    },
    #[error(transparent)]
    Lifecycle(#[from] LifecycleError),
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use protocol::{ActionFrame, ActionKind};

    use super::*;

    fn content_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn owner(value: u64) -> SessionOwnerId {
        SessionOwnerId::new(value).unwrap()
    }

    fn transport(value: u64) -> TransportId {
        TransportId::new(value).unwrap()
    }

    fn join(sequence: u32) -> SessionControlFrame {
        SessionControlFrame {
            sequence,
            client_tick: 0,
            client_monotonic_micros: u64::from(sequence),
            request: SessionControlRequest::Join,
        }
    }

    fn input(sequence: u32) -> InputFrame {
        InputFrame {
            sequence,
            client_tick: u64::from(sequence),
            movement_x_milli: 0,
            movement_y_milli: 0,
            aim_x_milli: 1_000,
            aim_y_milli: 0,
            held_primary: false,
            primary_sequence: 0,
            ability_1_sequence: 0,
            ability_2_sequence: 0,
        }
    }

    #[test]
    fn identity_capacity_and_deterministic_oldest_instance_admission_are_exact() {
        assert!(matches!(
            HostedInstanceId::new(0),
            Err(InstanceError::ZeroInstanceIdentity)
        ));
        let root = content_root();
        let mut scheduler = InstanceScheduler::default();
        let instance_ids = (1..=M02_ARENA_CAPACITY)
            .map(|ordinal| {
                scheduler
                    .admit_or_route_control(
                        owner(u64::try_from(ordinal).unwrap()),
                        transport(u64::try_from(ordinal).unwrap()),
                        &join(1),
                        &root,
                        1,
                    )
                    .unwrap()
                    .instance_id
            })
            .collect::<Vec<_>>();
        let first_id = instance_ids[0];
        assert!(instance_ids.iter().all(|id| *id == first_id));
        assert_eq!(scheduler.instance_count(), 1);
        assert_eq!(scheduler.owner_count(), M02_ARENA_CAPACITY);
        assert_eq!(
            scheduler.instance_kind(first_id),
            Some(InstanceKind::CombatArena)
        );
        assert_eq!(
            scheduler
                .instance_content_version(first_id)
                .unwrap()
                .as_str(),
            "fp.1.0.0"
        );
        let overflow_owner = owner(17);
        let overflow = scheduler
            .admit_or_route_control(overflow_owner, transport(17), &join(1), &root, 2)
            .unwrap();
        assert_ne!(overflow.instance_id, first_id);
        assert_eq!(scheduler.instance_count(), 2);
        assert_eq!(
            scheduler.instance_for_owner(overflow_owner),
            Some(overflow.instance_id)
        );
        assert_eq!(scheduler.diagnostics().allocated_instances, 2);
        assert_eq!(scheduler.diagnostics().admissions, 17);
    }

    #[test]
    fn reconnect_routes_to_same_instance_and_each_frame_steps_once() {
        let root = content_root();
        let mut scheduler = InstanceScheduler::default();
        let initial = scheduler
            .admit_or_route_control(owner(1), transport(1), &join(1), &root, 1)
            .unwrap();
        scheduler
            .submit_input(owner(1), transport(1), &input(1))
            .unwrap();
        let first = scheduler.tick().unwrap();
        assert_eq!(first.scheduler_tick, 1);
        assert_eq!(first.session_steps, 1);
        assert!(first.snapshot_batches.is_empty());
        let reattached = scheduler
            .admit_or_route_control(owner(1), transport(2), &join(2), &root, 2)
            .unwrap();
        assert_eq!(reattached.instance_id, initial.instance_id);
        assert_eq!(
            control_code(&reattached.lifecycle.event).unwrap(),
            SessionControlResultCode::Reattached
        );
        assert_eq!(scheduler.diagnostics().reconnects, 1);
        scheduler
            .submit_input(owner(1), transport(2), &input(2))
            .unwrap();
        let second = scheduler.tick().unwrap();
        assert_eq!(second.scheduler_tick, 2);
        assert_eq!(second.session_steps, 1);
        assert_eq!(second.snapshot_batches.len(), 1);
        assert_eq!(second.snapshot_batches[0].owner, owner(1));
        assert_eq!(second.snapshot_batches[0].snapshots[0].server_tick, 2);
        assert_eq!(scheduler.diagnostics().simulation_stalls, 0);
    }

    #[test]
    fn resolved_retirement_closes_empty_instance_without_membership_residue() {
        let root = content_root();
        let mut scheduler = InstanceScheduler::default();
        let admitted = scheduler
            .admit_or_route_control(owner(1), transport(1), &join(1), &root, 1)
            .unwrap();
        let recall = scheduler
            .handle_gameplay_reliable(
                owner(1),
                transport(1),
                WireMessage::ActionFrame(ActionFrame {
                    sequence: 1,
                    client_tick: 0,
                    action: ActionKind::RecallStart,
                }),
            )
            .unwrap();
        assert!(matches!(
            recall,
            WireMessage::ReliableEvent(ReliableEventFrame {
                event: ReliableEvent::ActionResult {
                    code: protocol::ActionResultCode::Accepted,
                    ..
                },
                ..
            })
        ));
        let mut terminal = None;
        for _ in 0..sim_core::EMERGENCY_RECALL_CHANNEL_TICKS {
            terminal = Some(scheduler.tick().unwrap());
        }
        let terminal = terminal.unwrap();
        assert_eq!(terminal.snapshot_batches.len(), 1);
        assert_eq!(
            terminal.snapshot_batches[0].snapshots[0].server_tick,
            sim_core::EMERGENCY_RECALL_CHANNEL_TICKS
        );
        assert_eq!(
            scheduler.instance_phase(admitted.instance_id),
            Some(ArenaInstancePhase::Active)
        );
        assert_eq!(scheduler.retire_resolved().unwrap(), vec![owner(1)]);
        assert_eq!(scheduler.instance_count(), 0);
        assert_eq!(scheduler.owner_count(), 0);
        assert_eq!(scheduler.diagnostics().retired_sessions, 1);
        assert_eq!(scheduler.diagnostics().closed_instances, 1);
    }

    #[test]
    fn resolved_retirement_rejects_index_drift_before_mutating_membership() {
        let root = content_root();
        let mut scheduler = InstanceScheduler::default();
        let admitted = scheduler
            .admit_or_route_control(owner(1), transport(1), &join(1), &root, 1)
            .unwrap();
        scheduler
            .handle_gameplay_reliable(
                owner(1),
                transport(1),
                WireMessage::ActionFrame(ActionFrame {
                    sequence: 1,
                    client_tick: 0,
                    action: ActionKind::RecallStart,
                }),
            )
            .unwrap();
        for _ in 0..sim_core::EMERGENCY_RECALL_CHANNEL_TICKS {
            scheduler.tick().unwrap();
        }

        scheduler.owner_index.remove(&owner(1));
        assert!(matches!(
            scheduler.retire_resolved(),
            Err(InstanceError::OwnerIndexDrift)
        ));
        let instance = scheduler.instances.get(&admitted.instance_id).unwrap();
        assert_eq!(instance.directory.len(), 1);
        assert_eq!(instance.phase, ArenaInstancePhase::Active);
        assert_eq!(scheduler.diagnostics().retired_sessions, 0);
        assert_eq!(scheduler.diagnostics().closed_instances, 0);
        assert_eq!(scheduler.diagnostics().invalid_states, 1);
    }

    #[test]
    fn resolved_retirement_can_retain_a_routable_terminal_tombstone() {
        let root = content_root();
        let mut scheduler = InstanceScheduler::default();
        scheduler
            .admit_or_route_control(owner(1), transport(1), &join(1), &root, 1)
            .unwrap();
        scheduler
            .handle_gameplay_reliable(
                owner(1),
                transport(1),
                WireMessage::ActionFrame(ActionFrame {
                    sequence: 1,
                    client_tick: 0,
                    action: ActionKind::RecallStart,
                }),
            )
            .unwrap();
        for _ in 0..sim_core::EMERGENCY_RECALL_CHANNEL_TICKS {
            scheduler.tick().unwrap();
        }

        let retained = BTreeSet::from([owner(1)]);
        assert!(
            scheduler
                .retire_resolved_excluding(&retained)
                .unwrap()
                .is_empty()
        );
        assert_eq!(scheduler.owner_count(), 1);
        assert_eq!(scheduler.retire_resolved().unwrap(), vec![owner(1)]);
        assert_eq!(scheduler.owner_count(), 0);
    }

    #[test]
    fn timing_report_uses_nearest_rank_and_exact_m02_boundaries() {
        let mut diagnostics = InstanceDiagnostics::default();
        for sample in 1..=100 {
            diagnostics.record_tick(sample).unwrap();
        }
        let report = diagnostics.timing_report().unwrap().unwrap();
        assert_eq!(report.sample_count, 100);
        assert_eq!(report.mean_micros, 51);
        assert_eq!(report.p95_micros, 95);
        assert_eq!(report.p99_micros, 99);
        assert_eq!(report.maximum_micros, 100);
        assert!(report.passes_m02_limits);

        let mut boundary = InstanceDiagnostics::default();
        for _ in 0..95 {
            boundary.record_tick(M02_P95_LIMIT_MICROS).unwrap();
        }
        for _ in 0..5 {
            boundary.record_tick(M02_P99_LIMIT_MICROS).unwrap();
        }
        let report = boundary.timing_report().unwrap().unwrap();
        assert_eq!(report.p95_micros, M02_P95_LIMIT_MICROS);
        assert_eq!(report.p99_micros, M02_P99_LIMIT_MICROS);
        assert!(report.passes_m02_limits);

        let mut failed = InstanceDiagnostics::default();
        for _ in 0..100 {
            failed.record_tick(M02_P95_LIMIT_MICROS + 1).unwrap();
        }
        assert!(!failed.timing_report().unwrap().unwrap().passes_m02_limits);
    }

    #[test]
    fn tick_samples_roll_forever_and_shutdown_is_fail_closed_and_idempotent() {
        let mut full = InstanceDiagnostics::default();
        full.tick_sample_count = full.tick_micros.len();
        full.record_tick(42).unwrap();
        assert_eq!(full.tick_sample_count, full.tick_micros.len());
        assert_eq!(full.tick_sample_cursor, 1);
        assert_eq!(full.tick_micros[0], 42);
        assert_eq!(full.total_tick_micros, 42);

        let root = content_root();
        let mut scheduler = InstanceScheduler::default();
        for ordinal in 1..=2 {
            scheduler
                .admit_or_route_control(
                    owner(ordinal),
                    transport(ordinal),
                    &join(1),
                    &root,
                    ordinal,
                )
                .unwrap();
        }
        let events = scheduler.begin_shutdown().unwrap();
        assert_eq!(events.len(), 2);
        assert!(!scheduler.is_accepting());
        assert!(scheduler.begin_shutdown().unwrap().is_empty());
        assert!(matches!(
            scheduler.admit_or_route_control(owner(3), transport(3), &join(1), &root, 3),
            Err(InstanceError::SchedulerDraining)
        ));
        scheduler.finish_shutdown().unwrap();
        scheduler.finish_shutdown().unwrap();
        assert_eq!(scheduler.instance_count(), 0);
        assert_eq!(scheduler.owner_count(), 0);
        assert_eq!(scheduler.diagnostics().closed_instances, 1);
    }

    #[test]
    fn unassigned_control_and_gameplay_fail_without_allocating() {
        let root = content_root();
        let mut scheduler = InstanceScheduler::default();
        let reconnect = SessionControlFrame {
            sequence: 1,
            client_tick: 0,
            client_monotonic_micros: 0,
            request: SessionControlRequest::Reconnect {
                prior_session_id: WireText::new("missing").unwrap(),
            },
        };
        assert!(matches!(
            scheduler.admit_or_route_control(owner(1), transport(1), &reconnect, &root, 0),
            Err(InstanceError::OwnerNotAssigned)
        ));
        assert!(matches!(
            scheduler.submit_input(owner(1), transport(1), &input(1)),
            Err(InstanceError::OwnerNotAssigned)
        ));
        assert_eq!(scheduler.instance_count(), 0);
        assert_eq!(scheduler.owner_count(), 0);
    }
}
