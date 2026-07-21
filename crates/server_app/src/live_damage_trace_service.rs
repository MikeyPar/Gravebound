//! Server-owned live damage-trace ingestion and restart recovery for `GB-M03-06B`.
//!
//! This internal boundary follows `Gravebound_Production_GDD_v1_Canonical.md` `DTH-001`,
//! `DTH-020`, and `TECH-021`-`023`; the promoted Core authority in
//! `Gravebound_Content_Production_Spec_v1.md`; `Gravebound_Development_Roadmap_v1.md`
//! `GB-M03-06`/`GB-M03-13` restart and atomicity gates; and accepted `SPEC-CONFLICT-009`.
//! Clients never provide trace identities, aggregate versions, danger roots, content authority,
//! or terminal material. A lethal tick is staged here for the later `06C` single-writer death
//! transaction and is never submitted to the standalone live-trace repository.

use std::{collections::BTreeSet, future::Future, sync::Arc};

use persistence::{
    CORE_ITEM_CONTENT_REVISION, DurableDeathPresentationAuthorityV1,
    LIVE_DAMAGE_TRACE_WINDOW_TICKS_V1, LiveDamageTraceCauseV1, LiveDamageTraceContentAuthorityV1,
    LiveDamageTraceDamageTypeV1, LiveDamageTraceDangerAuthorityV1, LiveDamageTraceEntryV1,
    LiveDamageTraceHeadV1, LiveDamageTraceNetworkStateV1, LiveDamageTraceRecallStateV1,
    LiveDamageTraceStatusV1, LiveDamageTraceTickCommandV1, LiveDamageTraceTickRequestV1,
    LiveDamageTraceTickTransactionV1, MAX_LIVE_DAMAGE_TRACE_ENTRIES_V1, PersistenceError,
    PostgresPersistence, StoredLiveDamageTraceSnapshotEntryV1, StoredLiveDamageTraceSnapshotV1,
    StoredLiveDamageTraceTickV1,
};
use sim_content::CoreDevelopmentDeathView;
use sim_core::{
    AuthoritativeDeathCauseKind, DEATH_AUTHORITY_SCHEMA_VERSION, DamageTraceAggregate,
    DamageTraceCheckpointV1, DamageTraceEntry, DamageTraceObservation, DamageType,
    DeathAuthorityError, DeathTraceNetworkState, DeathTraceRecallState, EntityId,
};
use thiserror::Error;

use crate::{DeathEntityIdentityAuthority, TerminalBinding};

/// Exact authenticated and server-journaled aggregate binding for one active danger root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveDamageTraceBinding {
    account_id: [u8; 16],
    character_id: [u8; 16],
    character_version: u64,
    danger: LiveDamageTraceDangerAuthorityV1,
    content: LiveDamageTraceContentAuthorityV1,
}

impl LiveDamageTraceBinding {
    pub fn new(
        account_id: [u8; 16],
        character_id: [u8; 16],
        character_version: u64,
        danger: LiveDamageTraceDangerAuthorityV1,
        content: LiveDamageTraceContentAuthorityV1,
    ) -> Result<Self, LiveDamageTraceServiceError> {
        let binding = Self {
            account_id,
            character_id,
            character_version,
            danger,
            content,
        };
        binding.validate()?;
        Ok(binding)
    }

    #[must_use]
    pub const fn character_version(&self) -> u64 {
        self.character_version
    }

    #[must_use]
    pub const fn danger(&self) -> &LiveDamageTraceDangerAuthorityV1 {
        &self.danger
    }

    fn validate(&self) -> Result<(), LiveDamageTraceServiceError> {
        if self.account_id == [0; 16]
            || self.character_id == [0; 16]
            || self.character_version == 0
            || self.danger.lineage_id == [0; 16]
            || self.danger.restore_point_id == [0; 16]
            || i64::try_from(self.character_version).is_err()
            || i64::try_from(self.danger.checkpoint_tick).is_err()
            || self.content != LiveDamageTraceContentAuthorityV1::core()
        {
            return Err(LiveDamageTraceServiceError::InvalidBinding);
        }
        Ok(())
    }
}

/// Server-generated identity and version authority for one complete simulation tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveDamageTraceMutationAuthority {
    trace_tick_id: [u8; 16],
    expected_character_version: u64,
    danger: LiveDamageTraceDangerAuthorityV1,
    issued_at_unix_ms: u64,
}

impl LiveDamageTraceMutationAuthority {
    pub fn new(
        trace_tick_id: [u8; 16],
        expected_character_version: u64,
        danger: LiveDamageTraceDangerAuthorityV1,
        issued_at_unix_ms: u64,
    ) -> Result<Self, LiveDamageTraceServiceError> {
        let authority = Self {
            trace_tick_id,
            expected_character_version,
            danger,
            issued_at_unix_ms,
        };
        authority.validate_shape()?;
        Ok(authority)
    }

    fn validate_shape(&self) -> Result<(), LiveDamageTraceServiceError> {
        if self.trace_tick_id == [0; 16]
            || self.expected_character_version == 0
            || self.danger.lineage_id == [0; 16]
            || self.danger.restore_point_id == [0; 16]
            || self.issued_at_unix_ms == 0
            || i64::try_from(self.expected_character_version).is_err()
            || i64::try_from(self.danger.checkpoint_tick).is_err()
            || i64::try_from(self.issued_at_unix_ms).is_err()
        {
            return Err(LiveDamageTraceServiceError::InvalidMutationAuthority);
        }
        Ok(())
    }
}

/// Material held until the `GB-M03-06C` transaction atomically promotes it into durable death.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedTerminalLiveDamageTrace {
    request: LiveDamageTraceTickRequestV1,
    aggregate: DamageTraceAggregate,
    terminal_snapshot: sim_core::DeathTraceTerminalSnapshot,
    full_window: Vec<StoredLiveDamageTraceSnapshotEntryV1>,
    entity_identities: DeathEntityIdentityAuthority,
}

impl PreparedTerminalLiveDamageTrace {
    #[must_use]
    pub const fn request(&self) -> &LiveDamageTraceTickRequestV1 {
        &self.request
    }

    #[must_use]
    pub const fn aggregate(&self) -> &DamageTraceAggregate {
        &self.aggregate
    }

    #[must_use]
    pub const fn terminal_snapshot(&self) -> &sim_core::DeathTraceTerminalSnapshot {
        &self.terminal_snapshot
    }

    #[must_use]
    pub fn full_window(&self) -> &[StoredLiveDamageTraceSnapshotEntryV1] {
        &self.full_window
    }

    /// Immutable simulation-to-journal mapping accumulated by the same trace owner that staged
    /// the lethal tick. Death planning cannot substitute a separately authored identity map.
    #[must_use]
    pub const fn entity_identities(&self) -> &DeathEntityIdentityAuthority {
        &self.entity_identities
    }

    #[cfg(test)]
    pub(crate) fn from_test_authority(
        request: LiveDamageTraceTickRequestV1,
        aggregate: DamageTraceAggregate,
        full_window: Vec<StoredLiveDamageTraceSnapshotEntryV1>,
        entity_identities: DeathEntityIdentityAuthority,
    ) -> Result<Self, LiveDamageTraceServiceError> {
        let terminal_snapshot = aggregate
            .terminal_snapshot()
            .map_err(LiveDamageTraceServiceError::Simulation)?;
        Ok(Self {
            request,
            aggregate,
            terminal_snapshot,
            full_window,
            entity_identities,
        })
    }
}

/// Successful state transition from one complete authoritative tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveDamageTraceIngestOutcome {
    EmptyTick,
    Committed(StoredLiveDamageTraceTickV1),
    Replayed(StoredLiveDamageTraceTickV1),
    TerminalPrepared(Box<PreparedTerminalLiveDamageTrace>),
}

#[derive(Debug, Error)]
pub enum LiveDamageTraceServiceError {
    #[error("live damage-trace binding is invalid or outside promoted Core authority")]
    InvalidBinding,
    #[error("live damage-trace mutation identity, version, or time is invalid")]
    InvalidMutationAuthority,
    #[error("a different live damage-trace tick is pending durable acknowledgement")]
    PendingMutationConflict,
    #[error("terminal damage evidence is pending the atomic death transaction")]
    TerminalResolutionPending,
    #[error("simulation entity {0} has no current durable server identity")]
    MissingEntityIdentity(u64),
    #[error("stored simulation entity {0} does not match its durable server identity")]
    EntityIdentityMismatch(u64),
    #[error("live damage-trace presentation content is missing or incompatible: {0}")]
    PresentationContentMismatch(&'static str),
    #[error("stored live damage-trace snapshot is corrupt or only partially authoritative")]
    CorruptSnapshot,
    #[error("stored live damage-trace result does not match the exact sealed request")]
    StoredResultMismatch,
    #[error("live damage-trace authority must be reloaded from durable state")]
    AuthorityRefreshRequired,
    #[error("simulation entity {0} conflicts with immutable journal identity authority")]
    EntityIdentityConflict(u64),
    #[error("live damage-trace identity authority exceeds its bounded window capacity")]
    EntityIdentityCapacityExceeded,
    #[error("there is no ambiguous live damage-trace request to retry")]
    NoPendingMutation,
    #[error("authoritative simulation rejected the complete trace tick")]
    Simulation(#[source] DeathAuthorityError),
    #[error("live damage-trace persistence rejected the mutation")]
    Persistence(#[source] PersistenceError),
}

/// Narrow storage seam. It cannot become a second gameplay writer because it accepts only the
/// sealed DTO assembled from server simulation and binding authority.
pub trait LiveDamageTraceRepository: Send + Sync {
    fn transact_tick(
        &self,
        request: &LiveDamageTraceTickRequestV1,
    ) -> impl Future<Output = Result<LiveDamageTraceTickTransactionV1, PersistenceError>> + Send;

    fn load_snapshot(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> impl Future<Output = Result<StoredLiveDamageTraceSnapshotV1, PersistenceError>> + Send;
}

impl LiveDamageTraceRepository for PostgresPersistence {
    async fn transact_tick(
        &self,
        request: &LiveDamageTraceTickRequestV1,
    ) -> Result<LiveDamageTraceTickTransactionV1, PersistenceError> {
        self.transact_live_damage_trace_tick_v1(request).await
    }

    async fn load_snapshot(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> Result<StoredLiveDamageTraceSnapshotV1, PersistenceError> {
        self.load_live_damage_trace_snapshot_v1(account_id, character_id)
            .await
    }
}

#[derive(Debug, Clone)]
struct PendingLiveDamageTraceTick {
    request: LiveDamageTraceTickRequestV1,
    aggregate: DamageTraceAggregate,
}

/// One selected character's bounded live trace. The aggregate advances only after a validated
/// durable acknowledgement, so response loss is indistinguishable from an ordinary retry.
#[derive(Debug)]
pub struct LiveDamageTraceService<Repository> {
    repository: Repository,
    binding: LiveDamageTraceBinding,
    presentation: Arc<CoreDevelopmentDeathView>,
    identities: DeathEntityIdentityAuthority,
    aggregate: DamageTraceAggregate,
    /// Exact normalized receipt identity retained beside every aggregate entry. Simulation state
    /// alone cannot reconstruct historical trace-tick IDs after acknowledgement or restart.
    window: Vec<StoredLiveDamageTraceSnapshotEntryV1>,
    head: Option<LiveDamageTraceHeadV1>,
    pending: Option<PendingLiveDamageTraceTick>,
    terminal: Option<PreparedTerminalLiveDamageTrace>,
    reload_required: bool,
}

impl<Repository> LiveDamageTraceService<Repository> {
    #[must_use]
    pub fn checkpoint(&self) -> DamageTraceCheckpointV1 {
        self.aggregate.checkpoint()
    }

    #[must_use]
    pub const fn pending_request(&self) -> Option<&LiveDamageTraceTickRequestV1> {
        match &self.pending {
            Some(pending) => Some(&pending.request),
            None => None,
        }
    }

    #[must_use]
    pub const fn prepared_terminal(&self) -> Option<&PreparedTerminalLiveDamageTrace> {
        self.terminal.as_ref()
    }

    #[must_use]
    pub const fn requires_reload(&self) -> bool {
        self.reload_required
    }

    #[must_use]
    pub const fn head(&self) -> Option<&LiveDamageTraceHeadV1> {
        self.head.as_ref()
    }

    #[must_use]
    pub const fn identities(&self) -> &DeathEntityIdentityAuthority {
        &self.identities
    }

    /// Monotonically extends the simulation-to-journal identity authority. Historical mappings
    /// are retained for the complete trace window and cannot be remapped or reused.
    pub fn register_entity_identities(
        &mut self,
        identities: &DeathEntityIdentityAuthority,
    ) -> Result<(), LiveDamageTraceServiceError> {
        if self.reload_required {
            return Err(LiveDamageTraceServiceError::AuthorityRefreshRequired);
        }
        if self.terminal.is_some() {
            return Err(LiveDamageTraceServiceError::TerminalResolutionPending);
        }
        merge_identity_authority(&mut self.identities, identities)
    }

    /// Applies an already authenticated server-journal authority advance between trace writes.
    /// Pending, terminal, and reload-required states are immutable until resolved or reopened.
    pub fn advance_authority(
        &mut self,
        character_version: u64,
        danger: LiveDamageTraceDangerAuthorityV1,
    ) -> Result<(), LiveDamageTraceServiceError> {
        if self.reload_required {
            return Err(LiveDamageTraceServiceError::AuthorityRefreshRequired);
        }
        if self.pending.is_some() {
            return Err(LiveDamageTraceServiceError::PendingMutationConflict);
        }
        if self.terminal.is_some() {
            return Err(LiveDamageTraceServiceError::TerminalResolutionPending);
        }
        if character_version < self.binding.character_version
            || danger.lineage_id != self.binding.danger.lineage_id
            || danger.restore_point_id != self.binding.danger.restore_point_id
            || danger.checkpoint_tick < self.binding.danger.checkpoint_tick
            || i64::try_from(character_version).is_err()
            || i64::try_from(danger.checkpoint_tick).is_err()
        {
            return Err(LiveDamageTraceServiceError::InvalidMutationAuthority);
        }
        self.binding.character_version = character_version;
        self.binding.danger = danger;
        Ok(())
    }
}

impl<Repository> LiveDamageTraceService<Repository>
where
    Repository: LiveDamageTraceRepository,
{
    /// Opens the currently committed danger trace without requiring callers to reconstruct the
    /// database-owned checkpoint tick or latest character version. The opaque terminal binding
    /// still pins account, character, lineage, and restore point, while the repository supplies
    /// the remaining current authority under one read transaction.
    pub async fn start_or_resume_current(
        repository: Repository,
        terminal: TerminalBinding,
        minimum_character_version: u64,
        identities: DeathEntityIdentityAuthority,
        presentation: Arc<CoreDevelopmentDeathView>,
    ) -> Result<Self, LiveDamageTraceServiceError> {
        if minimum_character_version == 0 {
            return Err(LiveDamageTraceServiceError::InvalidBinding);
        }
        validate_identity_authority(&identities)?;
        validate_presentation_authority(&presentation)?;
        let snapshot = repository
            .load_snapshot(*terminal.account_id(), *terminal.character_id())
            .await
            .map_err(LiveDamageTraceServiceError::Persistence)?;
        if snapshot.character_version < minimum_character_version
            || snapshot.danger.lineage_id != *terminal.lineage_id()
            || snapshot.danger.restore_point_id != *terminal.restore_point_id()
            || snapshot.content != LiveDamageTraceContentAuthorityV1::core()
        {
            return Err(LiveDamageTraceServiceError::InvalidBinding);
        }
        let binding = LiveDamageTraceBinding::new(
            *terminal.account_id(),
            *terminal.character_id(),
            snapshot.character_version,
            snapshot.danger.clone(),
            snapshot.content.clone(),
        )?;
        Self::from_snapshot(repository, binding, identities, presentation, snapshot)
    }

    /// Opens the one authoritative trace state for this danger root. The repository returns the
    /// locked empty state for a fresh root or the complete retained window for a resumed root;
    /// callers cannot accidentally start a second empty aggregate over existing evidence.
    pub async fn start_or_resume(
        repository: Repository,
        binding: LiveDamageTraceBinding,
        identities: DeathEntityIdentityAuthority,
        presentation: Arc<CoreDevelopmentDeathView>,
    ) -> Result<Self, LiveDamageTraceServiceError> {
        binding.validate()?;
        validate_identity_authority(&identities)?;
        validate_presentation_authority(&presentation)?;
        let snapshot = repository
            .load_snapshot(binding.account_id, binding.character_id)
            .await
            .map_err(LiveDamageTraceServiceError::Persistence)?;
        Self::from_snapshot(repository, binding, identities, presentation, snapshot)
    }

    fn from_snapshot(
        repository: Repository,
        binding: LiveDamageTraceBinding,
        mut identities: DeathEntityIdentityAuthority,
        presentation: Arc<CoreDevelopmentDeathView>,
        snapshot: StoredLiveDamageTraceSnapshotV1,
    ) -> Result<Self, LiveDamageTraceServiceError> {
        let aggregate = reconstruct_snapshot(&snapshot, &binding, &mut identities, &presentation)?;
        Ok(Self {
            repository,
            binding,
            presentation,
            identities,
            aggregate,
            window: snapshot.entries,
            head: snapshot.head,
            pending: None,
            terminal: None,
            reload_required: false,
        })
    }

    /// Stages one complete simulation tick. Empty ticks are explicit no-ops. A nonlethal tick
    /// remains sealed across repository errors; an exact retry is the only input admitted until
    /// its committed or replayed result validates. A lethal tick never calls `transact_tick`.
    pub async fn ingest_tick(
        &mut self,
        mutation: LiveDamageTraceMutationAuthority,
        observations: Vec<DamageTraceObservation>,
    ) -> Result<LiveDamageTraceIngestOutcome, LiveDamageTraceServiceError> {
        if self.reload_required {
            return Err(LiveDamageTraceServiceError::AuthorityRefreshRequired);
        }
        if self.terminal.is_some() {
            return Err(LiveDamageTraceServiceError::TerminalResolutionPending);
        }
        if self.pending.is_some() {
            return Err(LiveDamageTraceServiceError::PendingMutationConflict);
        }
        validate_mutation(&mutation, &self.binding)?;

        if observations.is_empty() {
            return Ok(LiveDamageTraceIngestOutcome::EmptyTick);
        }
        validate_observations(&observations, &self.presentation)?;

        let staged = stage_tick(
            &self.aggregate,
            &self.binding,
            &self.identities,
            self.head.clone(),
            mutation,
            observations,
        )?;

        if staged
            .request
            .command
            .entries
            .last()
            .is_some_and(|entry| entry.lethal)
        {
            let terminal_snapshot = staged
                .aggregate
                .terminal_snapshot()
                .map_err(LiveDamageTraceServiceError::Simulation)?;
            let full_window = append_stored_window(&self.window, &staged.request.command)?;
            let prepared = PreparedTerminalLiveDamageTrace {
                request: staged.request,
                aggregate: staged.aggregate,
                terminal_snapshot,
                full_window,
                entity_identities: self.identities.clone(),
            };
            self.terminal = Some(prepared.clone());
            return Ok(LiveDamageTraceIngestOutcome::TerminalPrepared(Box::new(
                prepared,
            )));
        }

        self.pending = Some(staged);
        self.retry_pending().await
    }

    /// Retries only the exact sealed request retained after an ambiguous database failure. The
    /// simulation caller does not recreate observations, timestamps, or identity material.
    pub async fn retry_pending(
        &mut self,
    ) -> Result<LiveDamageTraceIngestOutcome, LiveDamageTraceServiceError> {
        if self.reload_required {
            return Err(LiveDamageTraceServiceError::AuthorityRefreshRequired);
        }
        let pending = self
            .pending
            .clone()
            .ok_or(LiveDamageTraceServiceError::NoPendingMutation)?;
        let transaction = match self.repository.transact_tick(&pending.request).await {
            Ok(transaction) => transaction,
            Err(error) if error.may_have_ambiguous_commit_outcome() => {
                return Err(LiveDamageTraceServiceError::Persistence(error));
            }
            Err(error) => {
                self.pending = None;
                self.reload_required = true;
                return Err(LiveDamageTraceServiceError::Persistence(error));
            }
        };
        let (replayed, stored) = match transaction {
            LiveDamageTraceTickTransactionV1::Committed(stored) => (false, stored),
            LiveDamageTraceTickTransactionV1::Replayed(stored) => (true, stored),
        };
        if let Err(error) = validate_stored_result(&pending.request, &stored) {
            self.pending = None;
            self.reload_required = true;
            return Err(error);
        }
        self.window = append_stored_window(&self.window, &stored.command)?;
        self.aggregate = pending.aggregate;
        self.binding.character_version = stored.command.expected_character_version;
        self.binding.danger = stored.command.danger.clone();
        self.head = Some(stored.head());
        self.pending = None;
        if replayed {
            Ok(LiveDamageTraceIngestOutcome::Replayed(stored))
        } else {
            Ok(LiveDamageTraceIngestOutcome::Committed(stored))
        }
    }
}

fn append_stored_window(
    current: &[StoredLiveDamageTraceSnapshotEntryV1],
    command: &LiveDamageTraceTickCommandV1,
) -> Result<Vec<StoredLiveDamageTraceSnapshotEntryV1>, LiveDamageTraceServiceError> {
    let cutoff = command
        .event_tick
        .saturating_sub(LIVE_DAMAGE_TRACE_WINDOW_TICKS_V1);
    let mut window = current
        .iter()
        .filter(|entry| entry.event_tick >= cutoff)
        .cloned()
        .collect::<Vec<_>>();
    if window
        .last()
        .is_some_and(|entry| entry.event_tick >= command.event_tick)
        || window
            .len()
            .checked_add(command.entries.len())
            .is_none_or(|count| count > MAX_LIVE_DAMAGE_TRACE_ENTRIES_V1)
    {
        return Err(LiveDamageTraceServiceError::CorruptSnapshot);
    }
    window.extend(command.entries.iter().cloned().map(|entry| {
        StoredLiveDamageTraceSnapshotEntryV1 {
            trace_tick_id: command.trace_tick_id,
            event_tick: command.event_tick,
            entry,
        }
    }));
    Ok(window)
}

fn validate_mutation(
    mutation: &LiveDamageTraceMutationAuthority,
    binding: &LiveDamageTraceBinding,
) -> Result<(), LiveDamageTraceServiceError> {
    mutation.validate_shape()?;
    if mutation.expected_character_version != binding.character_version
        || mutation.danger != binding.danger
    {
        return Err(LiveDamageTraceServiceError::InvalidMutationAuthority);
    }
    Ok(())
}

fn validate_identity_authority(
    identities: &DeathEntityIdentityAuthority,
) -> Result<(), LiveDamageTraceServiceError> {
    let mut durable_ids = BTreeSet::new();
    if identities
        .by_sim_entity
        .values()
        .any(|id| *id == [0; 16] || !durable_ids.insert(*id))
        || identities.by_sim_entity.len() > MAX_LIVE_DAMAGE_TRACE_ENTRIES_V1
    {
        return Err(LiveDamageTraceServiceError::InvalidBinding);
    }
    Ok(())
}

fn validate_presentation_authority(
    presentation: &CoreDevelopmentDeathView,
) -> Result<(), LiveDamageTraceServiceError> {
    let expected = DurableDeathPresentationAuthorityV1::core();
    let actual = presentation.hashes();
    if presentation.item_content_revision() != CORE_ITEM_CONTENT_REVISION
        || actual.records_blake3 != expected.records_blake3
        || actual.assets_blake3 != expected.assets_blake3
        || actual.localization_blake3 != expected.localization_blake3
    {
        return Err(LiveDamageTraceServiceError::PresentationContentMismatch(
            "catalog revision",
        ));
    }
    Ok(())
}

fn validate_observations(
    observations: &[DamageTraceObservation],
    presentation: &CoreDevelopmentDeathView,
) -> Result<(), LiveDamageTraceServiceError> {
    for observation in observations {
        validate_trace_presentation_ids(
            presentation,
            &observation.source_content_id,
            observation.pattern_id.as_deref(),
            &observation.attack_id,
            observation
                .statuses
                .iter()
                .map(|status| status.status_id.as_str()),
        )?;
    }
    Ok(())
}

fn validate_stored_entry_presentation(
    entry: &LiveDamageTraceEntryV1,
    presentation: &CoreDevelopmentDeathView,
) -> Result<(), LiveDamageTraceServiceError> {
    validate_trace_presentation_ids(
        presentation,
        &entry.source_content_id,
        entry.pattern_id.as_deref(),
        &entry.attack_id,
        entry
            .statuses
            .iter()
            .map(|status| status.status_id.as_str()),
    )
}

fn validate_trace_presentation_ids<'a>(
    presentation: &CoreDevelopmentDeathView,
    source_content_id: &str,
    pattern_id: Option<&str>,
    attack_id: &str,
    status_ids: impl Iterator<Item = &'a str>,
) -> Result<(), LiveDamageTraceServiceError> {
    if presentation.resolve_source(source_content_id).is_none()
        || presentation.resolve_attack(attack_id).is_none()
        || pattern_id.is_some_and(|id| presentation.resolve_pattern(id).is_none())
        || status_ids
            .into_iter()
            .any(|id| presentation.resolve_status(id).is_none())
    {
        return Err(LiveDamageTraceServiceError::PresentationContentMismatch(
            "combat producer IDs",
        ));
    }
    Ok(())
}

fn merge_identity_authority(
    target: &mut DeathEntityIdentityAuthority,
    incoming: &DeathEntityIdentityAuthority,
) -> Result<(), LiveDamageTraceServiceError> {
    validate_identity_authority(incoming)?;
    let mut merged = target.clone();
    for (&sim_id, &durable_id) in &incoming.by_sim_entity {
        if merged
            .by_sim_entity
            .get(&sim_id)
            .is_some_and(|existing| *existing != durable_id)
            || merged
                .by_sim_entity
                .iter()
                .any(|(existing_sim, existing_durable)| {
                    *existing_sim != sim_id && *existing_durable == durable_id
                })
        {
            return Err(LiveDamageTraceServiceError::EntityIdentityConflict(
                sim_id.get(),
            ));
        }
        merged.by_sim_entity.insert(sim_id, durable_id);
    }
    if merged.by_sim_entity.len() > MAX_LIVE_DAMAGE_TRACE_ENTRIES_V1 {
        return Err(LiveDamageTraceServiceError::EntityIdentityCapacityExceeded);
    }
    *target = merged;
    Ok(())
}

fn stage_tick(
    aggregate: &DamageTraceAggregate,
    binding: &LiveDamageTraceBinding,
    identities: &DeathEntityIdentityAuthority,
    expected_previous: Option<LiveDamageTraceHeadV1>,
    mutation: LiveDamageTraceMutationAuthority,
    observations: Vec<DamageTraceObservation>,
) -> Result<PendingLiveDamageTraceTick, LiveDamageTraceServiceError> {
    let mut staged = aggregate.clone();
    let compiled = staged
        .record_tick(observations)
        .map_err(LiveDamageTraceServiceError::Simulation)?;
    let event_tick = compiled
        .first()
        .ok_or(LiveDamageTraceServiceError::CorruptSnapshot)?
        .tick
        .0;
    let entries = compiled
        .iter()
        .map(|entry| map_entry_to_persistence(entry, identities))
        .collect::<Result<Vec<_>, _>>()?;
    let command = LiveDamageTraceTickCommandV1 {
        account_id: binding.account_id,
        character_id: binding.character_id,
        trace_tick_id: mutation.trace_tick_id,
        expected_previous,
        expected_character_version: mutation.expected_character_version,
        event_tick,
        danger: mutation.danger,
        content: binding.content.clone(),
        entries,
        issued_at_unix_ms: mutation.issued_at_unix_ms,
    };
    let request = LiveDamageTraceTickRequestV1::seal(command)
        .map_err(LiveDamageTraceServiceError::Persistence)?;
    Ok(PendingLiveDamageTraceTick {
        request,
        aggregate: staged,
    })
}

fn map_entry_to_persistence(
    entry: &DamageTraceEntry,
    identities: &DeathEntityIdentityAuthority,
) -> Result<LiveDamageTraceEntryV1, LiveDamageTraceServiceError> {
    let (source_entity_id, source_sim_entity_id) = match entry.source_entity_id {
        Some(sim_id) => {
            let durable = identities
                .by_sim_entity
                .get(&sim_id)
                .copied()
                .filter(|id| *id != [0; 16])
                .ok_or(LiveDamageTraceServiceError::MissingEntityIdentity(
                    sim_id.get(),
                ))?;
            (Some(durable), Some(sim_id.get()))
        }
        None => (None, None),
    };
    let statuses = entry
        .statuses
        .iter()
        .enumerate()
        .map(|(ordinal, status)| {
            Ok(LiveDamageTraceStatusV1 {
                status_ordinal: u8::try_from(ordinal)
                    .map_err(|_| LiveDamageTraceServiceError::CorruptSnapshot)?,
                status_id: status.status_id.clone(),
                remaining_ticks: status.remaining_ticks,
                stack_count: status.stack_count,
            })
        })
        .collect::<Result<Vec<_>, LiveDamageTraceServiceError>>()?;
    Ok(LiveDamageTraceEntryV1 {
        event_ordinal: entry.event_ordinal,
        cause: map_cause(entry.cause_kind),
        source_content_id: entry.source_content_id.clone(),
        source_entity_id,
        source_sim_entity_id,
        pattern_id: entry.pattern_id.clone(),
        attack_id: entry.attack_id.clone(),
        raw_damage: entry.raw_damage,
        final_damage: entry.final_damage,
        damage_type: map_damage_type(entry.damage_type),
        pre_health: entry.pre_health,
        post_health: entry.post_health,
        source_x_milli_tiles: entry.source_x_milli_tiles,
        source_y_milli_tiles: entry.source_y_milli_tiles,
        network_state: map_network_state(entry.network_state),
        recall_state: map_recall_state(entry.recall_state),
        lethal: entry.lethal,
        statuses,
    })
}

fn reconstruct_snapshot(
    snapshot: &StoredLiveDamageTraceSnapshotV1,
    binding: &LiveDamageTraceBinding,
    identities: &mut DeathEntityIdentityAuthority,
    presentation: &CoreDevelopmentDeathView,
) -> Result<DamageTraceAggregate, LiveDamageTraceServiceError> {
    if snapshot.character_version != binding.character_version
        || snapshot.danger != binding.danger
        || snapshot.content != binding.content
    {
        return Err(LiveDamageTraceServiceError::CorruptSnapshot);
    }
    if snapshot.head.is_none() {
        return if snapshot.through_tick == 0 && snapshot.entries.is_empty() {
            Ok(DamageTraceAggregate::new())
        } else {
            Err(LiveDamageTraceServiceError::CorruptSnapshot)
        };
    }
    if snapshot.through_tick == 0
        || snapshot.entries.is_empty()
        || snapshot
            .head
            .as_ref()
            .is_none_or(|head| head.event_tick != snapshot.through_tick)
        || snapshot.entries.last().is_none_or(|entry| {
            entry.event_tick != snapshot.through_tick
                || snapshot
                    .head
                    .as_ref()
                    .is_none_or(|head| head.trace_tick_id != entry.trace_tick_id)
        })
    {
        return Err(LiveDamageTraceServiceError::CorruptSnapshot);
    }
    let cutoff = snapshot
        .through_tick
        .saturating_sub(LIVE_DAMAGE_TRACE_WINDOW_TICKS_V1);
    let mut previous = None;
    let mut entries = Vec::with_capacity(snapshot.entries.len());
    for stored in &snapshot.entries {
        validate_stored_entry_presentation(&stored.entry, presentation)?;
        if stored.event_tick < cutoff || stored.event_tick > snapshot.through_tick {
            return Err(LiveDamageTraceServiceError::CorruptSnapshot);
        }
        let key = (stored.event_tick, stored.entry.event_ordinal);
        if previous.is_some_and(|value| value >= key) {
            return Err(LiveDamageTraceServiceError::CorruptSnapshot);
        }
        previous = Some(key);
        entries.push(map_entry_from_persistence(stored, identities)?);
    }
    let expected = entries.clone();
    let aggregate = DamageTraceAggregate::from_checkpoint(DamageTraceCheckpointV1 {
        schema_version: DEATH_AUTHORITY_SCHEMA_VERSION,
        entries,
    })
    .map_err(|_| LiveDamageTraceServiceError::CorruptSnapshot)?;
    if aggregate.entries() != expected || aggregate.terminal_snapshot().is_ok() {
        return Err(LiveDamageTraceServiceError::CorruptSnapshot);
    }
    Ok(aggregate)
}

fn map_entry_from_persistence(
    stored: &StoredLiveDamageTraceSnapshotEntryV1,
    identities: &mut DeathEntityIdentityAuthority,
) -> Result<DamageTraceEntry, LiveDamageTraceServiceError> {
    let entry = &stored.entry;
    let source_entity_id = match (entry.source_sim_entity_id, entry.source_entity_id) {
        (Some(sim), Some(durable)) => {
            let sim_id = EntityId::new(sim).ok_or(LiveDamageTraceServiceError::CorruptSnapshot)?;
            merge_identity_authority(
                identities,
                &DeathEntityIdentityAuthority {
                    by_sim_entity: [(sim_id, durable)].into_iter().collect(),
                },
            )
            .map_err(|_| LiveDamageTraceServiceError::EntityIdentityMismatch(sim))?;
            Some(sim_id)
        }
        (None, None) => None,
        _ => return Err(LiveDamageTraceServiceError::CorruptSnapshot),
    };
    let statuses = entry
        .statuses
        .iter()
        .enumerate()
        .map(|(ordinal, status)| {
            if usize::from(status.status_ordinal) != ordinal {
                return Err(LiveDamageTraceServiceError::CorruptSnapshot);
            }
            Ok(sim_core::DeathTraceStatus {
                status_id: status.status_id.clone(),
                remaining_ticks: status.remaining_ticks,
                stack_count: status.stack_count,
            })
        })
        .collect::<Result<Vec<_>, LiveDamageTraceServiceError>>()?;
    Ok(DamageTraceEntry {
        tick: sim_core::Tick(stored.event_tick),
        event_ordinal: entry.event_ordinal,
        cause_kind: unmap_cause(entry.cause),
        source_content_id: entry.source_content_id.clone(),
        source_entity_id,
        pattern_id: entry.pattern_id.clone(),
        attack_id: entry.attack_id.clone(),
        raw_damage: entry.raw_damage,
        final_damage: entry.final_damage,
        damage_type: unmap_damage_type(entry.damage_type),
        pre_health: entry.pre_health,
        post_health: entry.post_health,
        source_x_milli_tiles: entry.source_x_milli_tiles,
        source_y_milli_tiles: entry.source_y_milli_tiles,
        statuses,
        network_state: unmap_network_state(entry.network_state),
        recall_state: unmap_recall_state(entry.recall_state),
        lethal: entry.lethal,
    })
}

fn validate_stored_result(
    request: &LiveDamageTraceTickRequestV1,
    stored: &StoredLiveDamageTraceTickV1,
) -> Result<(), LiveDamageTraceServiceError> {
    stored
        .validate_for_request(request)
        .map_err(|_| LiveDamageTraceServiceError::StoredResultMismatch)
}

const fn map_cause(value: AuthoritativeDeathCauseKind) -> LiveDamageTraceCauseV1 {
    match value {
        AuthoritativeDeathCauseKind::DirectHit => LiveDamageTraceCauseV1::DirectHit,
        AuthoritativeDeathCauseKind::DamageOverTime => LiveDamageTraceCauseV1::DamageOverTime,
        AuthoritativeDeathCauseKind::Environment => LiveDamageTraceCauseV1::Environment,
        AuthoritativeDeathCauseKind::Disconnect => LiveDamageTraceCauseV1::Disconnect,
    }
}

const fn unmap_cause(value: LiveDamageTraceCauseV1) -> AuthoritativeDeathCauseKind {
    match value {
        LiveDamageTraceCauseV1::DirectHit => AuthoritativeDeathCauseKind::DirectHit,
        LiveDamageTraceCauseV1::DamageOverTime => AuthoritativeDeathCauseKind::DamageOverTime,
        LiveDamageTraceCauseV1::Environment => AuthoritativeDeathCauseKind::Environment,
        LiveDamageTraceCauseV1::Disconnect => AuthoritativeDeathCauseKind::Disconnect,
    }
}

const fn map_damage_type(value: DamageType) -> LiveDamageTraceDamageTypeV1 {
    match value {
        DamageType::Physical => LiveDamageTraceDamageTypeV1::Physical,
        DamageType::Veil => LiveDamageTraceDamageTypeV1::Veil,
    }
}

const fn unmap_damage_type(value: LiveDamageTraceDamageTypeV1) -> DamageType {
    match value {
        LiveDamageTraceDamageTypeV1::Physical => DamageType::Physical,
        LiveDamageTraceDamageTypeV1::Veil => DamageType::Veil,
    }
}

const fn map_network_state(value: DeathTraceNetworkState) -> LiveDamageTraceNetworkStateV1 {
    match value {
        DeathTraceNetworkState::Connected => LiveDamageTraceNetworkStateV1::Connected,
        DeathTraceNetworkState::Degraded => LiveDamageTraceNetworkStateV1::Degraded,
        DeathTraceNetworkState::LinkLost => LiveDamageTraceNetworkStateV1::LinkLost,
        DeathTraceNetworkState::Reattached => LiveDamageTraceNetworkStateV1::Reattached,
    }
}

const fn unmap_network_state(value: LiveDamageTraceNetworkStateV1) -> DeathTraceNetworkState {
    match value {
        LiveDamageTraceNetworkStateV1::Connected => DeathTraceNetworkState::Connected,
        LiveDamageTraceNetworkStateV1::Degraded => DeathTraceNetworkState::Degraded,
        LiveDamageTraceNetworkStateV1::LinkLost => DeathTraceNetworkState::LinkLost,
        LiveDamageTraceNetworkStateV1::Reattached => DeathTraceNetworkState::Reattached,
    }
}

const fn map_recall_state(value: DeathTraceRecallState) -> LiveDamageTraceRecallStateV1 {
    match value {
        DeathTraceRecallState::Inactive => LiveDamageTraceRecallStateV1::Inactive,
        DeathTraceRecallState::Channeling => LiveDamageTraceRecallStateV1::Channeling,
        DeathTraceRecallState::CompletionPending => LiveDamageTraceRecallStateV1::CompletionPending,
    }
}

const fn unmap_recall_state(value: LiveDamageTraceRecallStateV1) -> DeathTraceRecallState {
    match value {
        LiveDamageTraceRecallStateV1::Inactive => DeathTraceRecallState::Inactive,
        LiveDamageTraceRecallStateV1::Channeling => DeathTraceRecallState::Channeling,
        LiveDamageTraceRecallStateV1::CompletionPending => DeathTraceRecallState::CompletionPending,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, HashMap},
        sync::{Arc, Mutex},
    };

    use sim_core::{DeathTraceStatus, SimulationVector, Tick};

    use super::*;

    #[derive(Debug, Default)]
    struct FakeState {
        calls: usize,
        response_loss_once: bool,
        committed: HashMap<[u8; 16], StoredLiveDamageTraceTickV1>,
        snapshot: Option<StoredLiveDamageTraceSnapshotV1>,
    }

    #[derive(Debug, Clone, Default)]
    struct FakeRepository(Arc<Mutex<FakeState>>);

    impl FakeRepository {
        fn calls(&self) -> usize {
            self.0.lock().unwrap().calls
        }

        fn lose_first_response(&self) {
            self.0.lock().unwrap().response_loss_once = true;
        }

        fn set_snapshot(&self, snapshot: StoredLiveDamageTraceSnapshotV1) {
            self.0.lock().unwrap().snapshot = Some(snapshot);
        }
    }

    impl LiveDamageTraceRepository for FakeRepository {
        async fn transact_tick(
            &self,
            request: &LiveDamageTraceTickRequestV1,
        ) -> Result<LiveDamageTraceTickTransactionV1, PersistenceError> {
            let mut state = self.0.lock().unwrap();
            state.calls += 1;
            if let Some(stored) = state.committed.get(&request.command.trace_tick_id) {
                return if stored.request_hash == request.request_hash {
                    Ok(LiveDamageTraceTickTransactionV1::Replayed(stored.clone()))
                } else {
                    Err(PersistenceError::LiveDamageTraceIdempotencyConflict)
                };
            }
            let committed_at_unix_ms = request.command.issued_at_unix_ms + 1;
            let stored =
                StoredLiveDamageTraceTickV1::for_committed_request(request, committed_at_unix_ms)?;
            state
                .committed
                .insert(request.command.trace_tick_id, stored.clone());
            if state.response_loss_once {
                state.response_loss_once = false;
                return Err(PersistenceError::Database(sqlx::Error::Io(
                    std::io::Error::new(
                        std::io::ErrorKind::ConnectionReset,
                        "simulated lost commit acknowledgement",
                    ),
                )));
            }
            Ok(LiveDamageTraceTickTransactionV1::Committed(stored))
        }

        async fn load_snapshot(
            &self,
            _account_id: [u8; 16],
            _character_id: [u8; 16],
        ) -> Result<StoredLiveDamageTraceSnapshotV1, PersistenceError> {
            self.0
                .lock()
                .unwrap()
                .snapshot
                .clone()
                .map_or_else(|| Ok(empty_snapshot()), Ok)
        }
    }

    fn entity(value: u64) -> EntityId {
        EntityId::new(value).unwrap()
    }

    fn identities() -> DeathEntityIdentityAuthority {
        DeathEntityIdentityAuthority {
            by_sim_entity: BTreeMap::from([(entity(42), [42; 16]), (entity(77), [77; 16])]),
        }
    }

    fn presentation() -> Arc<CoreDevelopmentDeathView> {
        Arc::new(
            sim_content::load_core_development_death_view(
                &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content"),
            )
            .expect("valid checked-in Core death-presentation catalog"),
        )
    }

    fn binding() -> LiveDamageTraceBinding {
        LiveDamageTraceBinding {
            account_id: [1; 16],
            character_id: [2; 16],
            character_version: 7,
            danger: LiveDamageTraceDangerAuthorityV1 {
                lineage_id: [3; 16],
                restore_point_id: [4; 16],
                checkpoint_tick: 50,
            },
            content: LiveDamageTraceContentAuthorityV1::core(),
        }
    }

    fn terminal_binding() -> TerminalBinding {
        TerminalBinding::new([1; 16], [2; 16], [3; 16], [4; 16]).unwrap()
    }

    #[tokio::test]
    async fn current_danger_open_uses_repository_checkpoint_and_rejects_foreign_root() {
        let repository = FakeRepository::default();
        let service = LiveDamageTraceService::start_or_resume_current(
            repository.clone(),
            terminal_binding(),
            6,
            identities(),
            presentation(),
        )
        .await
        .expect("current durable checkpoint opens");
        assert_eq!(service.binding.character_version, 7);
        assert_eq!(service.binding.danger.checkpoint_tick, 50);

        let foreign = TerminalBinding::new([1; 16], [2; 16], [3; 16], [9; 16]).unwrap();
        assert!(matches!(
            LiveDamageTraceService::start_or_resume_current(
                repository,
                foreign,
                6,
                identities(),
                presentation(),
            )
            .await,
            Err(LiveDamageTraceServiceError::InvalidBinding)
        ));
    }

    fn mutation(id: u8) -> LiveDamageTraceMutationAuthority {
        LiveDamageTraceMutationAuthority {
            trace_tick_id: [id; 16],
            expected_character_version: 7,
            danger: binding().danger,
            issued_at_unix_ms: 1_000 + u64::from(id),
        }
    }

    fn observation(
        tick: u64,
        ordinal: u32,
        sim_entity: Option<u64>,
        pre_health: u32,
        final_damage: u32,
    ) -> DamageTraceObservation {
        DamageTraceObservation {
            tick: Tick(tick),
            event_ordinal: ordinal,
            cause_kind: AuthoritativeDeathCauseKind::DirectHit,
            source_content_id: "enemy.bell_acolyte".to_owned(),
            source_entity_id: sim_entity.map(entity),
            pattern_id: Some("pattern.enemy.bell_acolyte.alternating_fan".to_owned()),
            attack_id: "pattern.enemy.bell_acolyte.alternating_fan".to_owned(),
            raw_damage: final_damage,
            final_damage,
            damage_type: DamageType::Veil,
            pre_health,
            post_health: pre_health.saturating_sub(final_damage),
            source_position: SimulationVector::new(12.125, -7.501),
            statuses: vec![
                DeathTraceStatus {
                    status_id: "status.marked".to_owned(),
                    remaining_ticks: 20,
                    stack_count: 2,
                },
                DeathTraceStatus {
                    status_id: "status.bleed".to_owned(),
                    remaining_ticks: 10,
                    stack_count: 1,
                },
            ],
            network_state: DeathTraceNetworkState::Degraded,
            recall_state: DeathTraceRecallState::Channeling,
        }
    }

    fn snapshot_from_entries(
        through_tick: u64,
        entries: Vec<StoredLiveDamageTraceSnapshotEntryV1>,
    ) -> StoredLiveDamageTraceSnapshotV1 {
        let head_tick_id = entries.last().map(|entry| entry.trace_tick_id);
        StoredLiveDamageTraceSnapshotV1 {
            character_version: binding().character_version,
            danger: binding().danger,
            content: LiveDamageTraceContentAuthorityV1::core(),
            through_tick,
            entries,
            head: (through_tick > 0).then_some(LiveDamageTraceHeadV1 {
                trace_tick_id: head_tick_id.unwrap_or([8; 16]),
                event_tick: through_tick,
                result_digest: [9; 32],
            }),
        }
    }

    fn empty_snapshot() -> StoredLiveDamageTraceSnapshotV1 {
        snapshot_from_entries(0, Vec::new())
    }

    fn stored_entry(
        tick: u64,
        observation: DamageTraceObservation,
    ) -> StoredLiveDamageTraceSnapshotEntryV1 {
        let mut aggregate = DamageTraceAggregate::new();
        let compiled = aggregate.record_tick([observation]).unwrap().remove(0);
        StoredLiveDamageTraceSnapshotEntryV1 {
            trace_tick_id: [u8::try_from(tick % 251 + 1).unwrap(); 16],
            event_tick: tick,
            entry: map_entry_to_persistence(&compiled, &identities()).unwrap(),
        }
    }

    #[tokio::test]
    async fn unordered_tick_is_canonical_and_preserves_sparse_ordinals_and_fixed_point() {
        let repository = FakeRepository::default();
        let mut service = LiveDamageTraceService::start_or_resume(
            repository,
            binding(),
            identities(),
            presentation(),
        )
        .await
        .unwrap();
        let outcome = service
            .ingest_tick(
                mutation(5),
                vec![
                    observation(100, 7, Some(77), 90, 10),
                    observation(100, 3, Some(42), 100, 10),
                ],
            )
            .await
            .unwrap();
        let LiveDamageTraceIngestOutcome::Committed(stored) = outcome else {
            panic!("expected committed tick")
        };
        assert_eq!(stored.command.entries[0].event_ordinal, 3);
        assert_eq!(stored.command.entries[1].event_ordinal, 7);
        assert_eq!(stored.command.entries[0].source_sim_entity_id, Some(42));
        assert_eq!(stored.command.entries[0].source_entity_id, Some([42; 16]));
        assert_eq!(stored.command.entries[0].source_x_milli_tiles, 12_125);
        assert_eq!(stored.command.entries[0].source_y_milli_tiles, -7_501);
        assert_eq!(
            stored.command.entries[0].statuses[0].status_id,
            "status.bleed"
        );
        assert_eq!(service.checkpoint().entries.len(), 2);
    }

    #[tokio::test]
    async fn empty_tick_is_noop_and_missing_entity_fails_before_repository() {
        let repository = FakeRepository::default();
        let mut service = LiveDamageTraceService::start_or_resume(
            repository.clone(),
            binding(),
            identities(),
            presentation(),
        )
        .await
        .unwrap();
        assert_eq!(
            service.ingest_tick(mutation(5), vec![]).await.unwrap(),
            LiveDamageTraceIngestOutcome::EmptyTick
        );
        let error = service
            .ingest_tick(mutation(6), vec![observation(100, 1, Some(99), 100, 10)])
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            LiveDamageTraceServiceError::MissingEntityIdentity(99)
        ));
        assert_eq!(repository.calls(), 0);
        assert!(service.checkpoint().entries.is_empty());
    }

    #[tokio::test]
    async fn response_loss_keeps_exact_request_and_retry_replays_before_promotion() {
        let repository = FakeRepository::default();
        repository.lose_first_response();
        let mut service = LiveDamageTraceService::start_or_resume(
            repository.clone(),
            binding(),
            identities(),
            presentation(),
        )
        .await
        .unwrap();
        let observations = vec![observation(100, 3, Some(42), 100, 10)];
        assert!(matches!(
            service.ingest_tick(mutation(5), observations.clone()).await,
            Err(LiveDamageTraceServiceError::Persistence(_))
        ));
        assert!(service.checkpoint().entries.is_empty());
        assert!(service.pending_request().is_some());

        let mut changed = observations.clone();
        changed[0].raw_damage += 1;
        assert!(matches!(
            service.ingest_tick(mutation(5), changed).await,
            Err(LiveDamageTraceServiceError::PendingMutationConflict)
        ));
        assert!(matches!(
            service
                .ingest_tick(mutation(6), vec![observation(101, 1, Some(42), 90, 10)])
                .await,
            Err(LiveDamageTraceServiceError::PendingMutationConflict)
        ));
        assert_eq!(repository.calls(), 1);

        let outcome = service.retry_pending().await.unwrap();
        assert!(matches!(outcome, LiveDamageTraceIngestOutcome::Replayed(_)));
        assert_eq!(repository.calls(), 2);
        assert!(service.pending_request().is_none());
        assert_eq!(service.checkpoint().entries.len(), 1);
    }

    #[tokio::test]
    async fn acknowledged_tick_advances_current_version_and_checkpoint_authority() {
        let repository = FakeRepository::default();
        let mut service = LiveDamageTraceService::start_or_resume(
            repository,
            binding(),
            identities(),
            presentation(),
        )
        .await
        .unwrap();
        service
            .ingest_tick(mutation(5), vec![observation(100, 3, Some(42), 100, 10)])
            .await
            .unwrap();
        let mut advanced = mutation(6);
        advanced.expected_character_version = 9;
        advanced.danger.checkpoint_tick = 100;
        service
            .advance_authority(9, advanced.danger.clone())
            .unwrap();
        let outcome = service
            .ingest_tick(advanced, vec![observation(101, 7, Some(77), 90, 10)])
            .await
            .unwrap();
        let LiveDamageTraceIngestOutcome::Committed(stored) = outcome else {
            panic!("expected committed tick")
        };
        assert_eq!(stored.command.expected_character_version, 9);
        assert_eq!(stored.command.danger.checkpoint_tick, 100);
        assert_eq!(service.binding.character_version, 9);
        assert_eq!(service.binding.danger.checkpoint_tick, 100);
    }

    #[tokio::test]
    async fn lethal_tick_is_prepared_exactly_and_never_calls_standalone_repository() {
        let repository = FakeRepository::default();
        let mut service = LiveDamageTraceService::start_or_resume(
            repository.clone(),
            binding(),
            identities(),
            presentation(),
        )
        .await
        .unwrap();
        let outcome = service
            .ingest_tick(mutation(5), vec![observation(100, 7, Some(42), 10, 10)])
            .await
            .unwrap();
        let LiveDamageTraceIngestOutcome::TerminalPrepared(prepared) = outcome else {
            panic!("expected terminal preparation")
        };
        assert!(prepared.request().command.entries[0].lethal);
        assert_eq!(
            prepared.terminal_snapshot().trace,
            prepared.aggregate().entries()
        );
        assert_eq!(
            prepared
                .terminal_snapshot()
                .cause
                .lethal_entry
                .event_ordinal,
            7
        );
        assert_eq!(prepared.full_window().len(), 1);
        assert_eq!(repository.calls(), 0);
        assert!(matches!(
            service
                .ingest_tick(mutation(6), vec![observation(101, 1, Some(42), 10, 1)])
                .await,
            Err(LiveDamageTraceServiceError::TerminalResolutionPending)
        ));
        assert_eq!(repository.calls(), 0);
    }

    #[tokio::test]
    async fn resume_reconstructs_exact_window_and_accepts_inclusive_300_tick_boundary() {
        let repository = FakeRepository::default();
        let first = stored_entry(100, observation(100, 3, Some(42), 100, 10));
        let last = stored_entry(400, observation(400, 7, Some(77), 90, 10));
        repository.set_snapshot(snapshot_from_entries(400, vec![first, last]));
        let service = LiveDamageTraceService::start_or_resume(
            repository,
            binding(),
            identities(),
            presentation(),
        )
        .await
        .unwrap();
        let checkpoint = service.checkpoint();
        assert_eq!(checkpoint.entries.len(), 2);
        assert_eq!(checkpoint.entries[0].tick, Tick(100));
        assert_eq!(checkpoint.entries[1].tick, Tick(400));
        assert_eq!(checkpoint.entries[1].source_entity_id, Some(entity(77)));
    }

    #[tokio::test]
    async fn resume_rejects_corrupt_identity_root_content_order_and_window() {
        let cases = {
            let base = snapshot_from_entries(
                400,
                vec![
                    stored_entry(100, observation(100, 3, Some(42), 100, 10)),
                    stored_entry(400, observation(400, 7, Some(77), 90, 10)),
                ],
            );
            let mut identity = base.clone();
            identity.entries[0].entry.source_entity_id = Some([99; 16]);
            let mut root = base.clone();
            root.danger.lineage_id = [9; 16];
            let mut content = base.clone();
            content.content.records_blake3 = "changed".to_owned();
            let mut order = base.clone();
            order.entries.reverse();
            let mut outside_window = base;
            outside_window.entries[0].event_tick = 99;
            vec![identity, root, content, order, outside_window]
        };
        for snapshot in cases {
            let repository = FakeRepository::default();
            repository.set_snapshot(snapshot);
            let error = LiveDamageTraceService::start_or_resume(
                repository,
                binding(),
                identities(),
                presentation(),
            )
            .await
            .unwrap_err();
            assert!(matches!(
                error,
                LiveDamageTraceServiceError::CorruptSnapshot
                    | LiveDamageTraceServiceError::EntityIdentityMismatch(_)
            ));
        }
    }

    #[tokio::test]
    async fn corrupt_stored_acknowledgement_does_not_promote_pending_aggregate() {
        #[derive(Debug)]
        struct CorruptRepository;
        impl LiveDamageTraceRepository for CorruptRepository {
            async fn transact_tick(
                &self,
                request: &LiveDamageTraceTickRequestV1,
            ) -> Result<LiveDamageTraceTickTransactionV1, PersistenceError> {
                let mut stored = StoredLiveDamageTraceTickV1::for_committed_request(
                    request,
                    request.command.issued_at_unix_ms + 1,
                )?;
                stored.result_digest = [2; 32];
                Ok(LiveDamageTraceTickTransactionV1::Committed(stored))
            }
            async fn load_snapshot(
                &self,
                _account_id: [u8; 16],
                _character_id: [u8; 16],
            ) -> Result<StoredLiveDamageTraceSnapshotV1, PersistenceError> {
                Ok(empty_snapshot())
            }
        }

        let mut service = LiveDamageTraceService::start_or_resume(
            CorruptRepository,
            binding(),
            identities(),
            presentation(),
        )
        .await
        .unwrap();
        assert!(matches!(
            service
                .ingest_tick(mutation(5), vec![observation(100, 3, Some(42), 100, 10)])
                .await,
            Err(LiveDamageTraceServiceError::StoredResultMismatch)
        ));
        assert!(service.checkpoint().entries.is_empty());
        assert!(service.pending_request().is_none());
        assert!(service.requires_reload());
    }

    #[tokio::test]
    async fn deterministic_rejection_requires_authoritative_reopen() {
        #[derive(Debug)]
        struct RejectedRepository;
        impl LiveDamageTraceRepository for RejectedRepository {
            async fn transact_tick(
                &self,
                _request: &LiveDamageTraceTickRequestV1,
            ) -> Result<LiveDamageTraceTickTransactionV1, PersistenceError> {
                Err(PersistenceError::LiveDamageTraceCharacterVersionMismatch {
                    expected: 7,
                    actual: 8,
                })
            }

            async fn load_snapshot(
                &self,
                _account_id: [u8; 16],
                _character_id: [u8; 16],
            ) -> Result<StoredLiveDamageTraceSnapshotV1, PersistenceError> {
                Ok(empty_snapshot())
            }
        }

        let mut service = LiveDamageTraceService::start_or_resume(
            RejectedRepository,
            binding(),
            identities(),
            presentation(),
        )
        .await
        .unwrap();
        assert!(matches!(
            service
                .ingest_tick(mutation(5), vec![observation(100, 3, Some(42), 100, 10)])
                .await,
            Err(LiveDamageTraceServiceError::Persistence(
                PersistenceError::LiveDamageTraceCharacterVersionMismatch { .. }
            ))
        ));
        assert!(service.pending_request().is_none());
        assert!(service.requires_reload());
        assert!(matches!(
            service.retry_pending().await,
            Err(LiveDamageTraceServiceError::AuthorityRefreshRequired)
        ));
    }

    #[tokio::test]
    async fn dynamic_identity_registration_is_monotonic_and_terminal_provenance_is_complete() {
        let repository = FakeRepository::default();
        let mut initial = identities();
        initial.by_sim_entity.remove(&entity(77));
        let mut service =
            LiveDamageTraceService::start_or_resume(repository, binding(), initial, presentation())
                .await
                .unwrap();
        service
            .register_entity_identities(&DeathEntityIdentityAuthority {
                by_sim_entity: BTreeMap::from([(entity(77), [77; 16])]),
            })
            .unwrap();
        service
            .ingest_tick(mutation(5), vec![observation(100, 3, Some(77), 100, 10)])
            .await
            .unwrap();

        assert!(matches!(
            service.register_entity_identities(&DeathEntityIdentityAuthority {
                by_sim_entity: BTreeMap::from([(entity(77), [88; 16])]),
            }),
            Err(LiveDamageTraceServiceError::EntityIdentityConflict(77))
        ));
        assert!(matches!(
            service.register_entity_identities(&DeathEntityIdentityAuthority {
                by_sim_entity: BTreeMap::from([(entity(88), [77; 16])]),
            }),
            Err(LiveDamageTraceServiceError::EntityIdentityConflict(88))
        ));

        let outcome = service
            .ingest_tick(mutation(6), vec![observation(101, 7, Some(77), 90, 90)])
            .await
            .unwrap();
        let LiveDamageTraceIngestOutcome::TerminalPrepared(prepared) = outcome else {
            panic!("expected terminal preparation")
        };
        assert_eq!(prepared.full_window().len(), 2);
        assert!(
            prepared
                .full_window()
                .iter()
                .all(|entry| entry.entry.source_entity_id == Some([77; 16]))
        );
        assert_eq!(prepared.entity_identities(), service.identities());
    }

    #[tokio::test]
    async fn restart_learns_historical_identities_and_lethal_keeps_the_complete_window() {
        let repository = FakeRepository::default();
        repository.set_snapshot(snapshot_from_entries(
            400,
            vec![
                stored_entry(101, observation(101, 3, Some(42), 100, 10)),
                stored_entry(400, observation(400, 7, Some(77), 90, 10)),
            ],
        ));
        let mut service = LiveDamageTraceService::start_or_resume(
            repository,
            binding(),
            DeathEntityIdentityAuthority::default(),
            presentation(),
        )
        .await
        .unwrap();
        assert_eq!(service.identities(), &identities());

        let outcome = service
            .ingest_tick(mutation(9), vec![observation(401, 9, Some(77), 80, 80)])
            .await
            .unwrap();
        let LiveDamageTraceIngestOutcome::TerminalPrepared(prepared) = outcome else {
            panic!("expected terminal preparation")
        };
        assert_eq!(prepared.full_window().len(), 3);
        assert_eq!(prepared.full_window()[0].event_tick, 101);
        assert_eq!(prepared.full_window()[2].event_tick, 401);
    }

    #[tokio::test]
    async fn restart_rejects_stale_or_future_character_version() {
        for character_version in [6, 8] {
            let repository = FakeRepository::default();
            let mut snapshot = empty_snapshot();
            snapshot.character_version = character_version;
            repository.set_snapshot(snapshot);
            assert!(matches!(
                LiveDamageTraceService::start_or_resume(
                    repository,
                    binding(),
                    identities(),
                    presentation(),
                )
                .await,
                Err(LiveDamageTraceServiceError::CorruptSnapshot)
            ));
        }
    }

    #[tokio::test]
    async fn unknown_producer_ids_fail_before_trace_state_or_persistence_changes() {
        let repository = FakeRepository::default();
        let mut service = LiveDamageTraceService::start_or_resume(
            repository.clone(),
            binding(),
            identities(),
            presentation(),
        )
        .await
        .unwrap();

        let mut invalid = Vec::new();
        let mut source = observation(100, 1, Some(42), 100, 10);
        source.source_content_id = "source.core.unknown".into();
        invalid.push(source);
        let mut pattern = observation(100, 1, Some(42), 100, 10);
        pattern.pattern_id = Some("pattern.core.unknown".into());
        invalid.push(pattern);
        let mut attack = observation(100, 1, Some(42), 100, 10);
        attack.attack_id = "attack.core.unknown".into();
        invalid.push(attack);
        let mut status = observation(100, 1, Some(42), 100, 10);
        status.statuses[0].status_id = "status.core.unknown".into();
        invalid.push(status);

        for observation in invalid {
            assert!(matches!(
                service.ingest_tick(mutation(5), vec![observation]).await,
                Err(LiveDamageTraceServiceError::PresentationContentMismatch(
                    "combat producer IDs"
                ))
            ));
            assert!(service.checkpoint().entries.is_empty());
            assert!(service.pending_request().is_none());
            assert!(service.prepared_terminal().is_none());
        }
        assert_eq!(repository.calls(), 0);
    }

    #[tokio::test]
    async fn resumed_trace_revalidates_stored_producer_ids_before_acceptance() {
        let repository = FakeRepository::default();
        let mut retained = stored_entry(100, observation(100, 1, Some(42), 100, 10));
        retained.entry.source_content_id = "source.core.unknown".into();
        repository.set_snapshot(snapshot_from_entries(100, vec![retained]));

        assert!(matches!(
            LiveDamageTraceService::start_or_resume(
                repository.clone(),
                binding(),
                identities(),
                presentation(),
            )
            .await,
            Err(LiveDamageTraceServiceError::PresentationContentMismatch(
                "combat producer IDs"
            ))
        ));
        assert_eq!(repository.calls(), 0);
    }
}
