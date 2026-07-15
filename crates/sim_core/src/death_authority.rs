//! Deterministic permanent-death inputs for `GB-M03-06B`.
//!
//! This module owns only pure simulation state. Durable identity, `PostgreSQL` transactions,
//! protocol messages, item destruction, memorials, and Echo projection remain outside
//! `sim_core`.

use std::collections::{BTreeMap, VecDeque};

use thiserror::Error;

use crate::{DamageType, EntityId, SimulationVector, TICKS_PER_SECOND, Tick};

pub const DEATH_AUTHORITY_SCHEMA_VERSION: u16 = 1;
#[allow(clippy::cast_lossless)]
pub const DEATH_TRACE_WINDOW_TICKS: u64 = 10 * TICKS_PER_SECOND as u64;
pub const MAX_DEATH_TRACE_ENTRIES: usize = 4_096;
pub const MAX_DEATH_TRACE_STATUSES: usize = 32;
pub const MAX_DEATH_TRACE_STATUS_TICKS: u32 = 108_000;
#[allow(clippy::cast_lossless)]
pub const ECHO_COMBAT_ELIGIBILITY_TICKS: u64 = 10 * 60 * TICKS_PER_SECOND as u64;
pub const LINK_LOST_VULNERABILITY_TICKS: u32 = 3 * TICKS_PER_SECOND;
pub const RECALL_CHANNEL_TICKS: u32 = 12;

pub const DEED_NONE_ID: &str = "deed.none";
pub const DEED_NONE_EN_US: &str = "No final deed recorded.";
pub const DEED_SEPULCHER_KNIGHT_DEFEATED_ID: &str = "deed.core.sepulcher_knight_defeated";
pub const DEED_SIR_CALDUS_DEFEATED_ID: &str = "deed.core.sir_caldus_defeated";

/// One authoritative character-state tick presented to the life-clock aggregate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifeClockTickState {
    CharacterSelect,
    Loading,
    Offline,
    HallControllable,
    /// Transfer/load work after durable danger entry; combat time counts, lifetime does not.
    DangerLoading,
    /// Instance staging after load; combat time counts, lifetime does not.
    DangerStaging,
    DangerControllable,
    DangerLinkLost,
}

impl LifeClockTickState {
    const fn counts_lifetime(self) -> bool {
        matches!(
            self,
            Self::HallControllable | Self::DangerControllable | Self::DangerLinkLost
        )
    }

    const fn requires_danger_entry(self) -> bool {
        matches!(
            self,
            Self::DangerLoading
                | Self::DangerStaging
                | Self::DangerControllable
                | Self::DangerLinkLost
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DangerTerminalOutcome {
    Death,
    Extraction,
    EmergencyRecall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DangerEntryClockSnapshot {
    pub permadeath_combat_ticks: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeathClockCheckpointV1 {
    pub schema_version: u16,
    pub lifetime_ticks: u64,
    pub permadeath_combat_ticks: u64,
    pub danger_entry: Option<DangerEntryClockSnapshot>,
    pub link_lost_ticks: u32,
    pub dead: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeathClockSnapshot {
    pub lifetime_ticks: u64,
    pub lifetime_ms: u64,
    pub permadeath_combat_ticks: u64,
    pub echo_time_eligible: bool,
    pub danger_active: bool,
    pub link_lost_ticks: u32,
    pub dead: bool,
}

/// Checked 30 Hz lifetime and permadeath-combat clocks.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeathClockAggregate {
    lifetime_ticks: u64,
    permadeath_combat_ticks: u64,
    danger_entry: Option<DangerEntryClockSnapshot>,
    link_lost_ticks: u32,
    dead: bool,
}

impl DeathClockAggregate {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            lifetime_ticks: 0,
            permadeath_combat_ticks: 0,
            danger_entry: None,
            link_lost_ticks: 0,
            dead: false,
        }
    }

    pub fn from_checkpoint(
        checkpoint: DeathClockCheckpointV1,
    ) -> Result<Self, DeathAuthorityError> {
        if checkpoint.schema_version != DEATH_AUTHORITY_SCHEMA_VERSION
            || (checkpoint.dead && checkpoint.danger_entry.is_some())
            || checkpoint.danger_entry.is_some_and(|entry| {
                entry.permadeath_combat_ticks > checkpoint.permadeath_combat_ticks
            })
            || checkpoint.link_lost_ticks > LINK_LOST_VULNERABILITY_TICKS
            || (checkpoint.link_lost_ticks > 0 && checkpoint.danger_entry.is_none())
        {
            return Err(DeathAuthorityError::CorruptClockCheckpoint);
        }
        Ok(Self {
            lifetime_ticks: checkpoint.lifetime_ticks,
            permadeath_combat_ticks: checkpoint.permadeath_combat_ticks,
            danger_entry: checkpoint.danger_entry,
            link_lost_ticks: checkpoint.link_lost_ticks,
            dead: checkpoint.dead,
        })
    }

    #[must_use]
    pub const fn checkpoint(&self) -> DeathClockCheckpointV1 {
        DeathClockCheckpointV1 {
            schema_version: DEATH_AUTHORITY_SCHEMA_VERSION,
            lifetime_ticks: self.lifetime_ticks,
            permadeath_combat_ticks: self.permadeath_combat_ticks,
            danger_entry: self.danger_entry,
            link_lost_ticks: self.link_lost_ticks,
            dead: self.dead,
        }
    }

    pub fn enter_danger(&mut self) -> Result<DangerEntryClockSnapshot, DeathAuthorityError> {
        if self.dead {
            return Err(DeathAuthorityError::CharacterAlreadyDead);
        }
        if self.danger_entry.is_some() {
            return Err(DeathAuthorityError::DangerAlreadyActive);
        }
        let entry = DangerEntryClockSnapshot {
            permadeath_combat_ticks: self.permadeath_combat_ticks,
        };
        self.danger_entry = Some(entry);
        self.link_lost_ticks = 0;
        Ok(entry)
    }

    /// Applies one authoritative 30 Hz tick without partially mutating on overflow.
    pub fn advance(&mut self, state: LifeClockTickState) -> Result<(), DeathAuthorityError> {
        if self.dead {
            return Err(DeathAuthorityError::CharacterAlreadyDead);
        }
        if state.requires_danger_entry() && self.danger_entry.is_none() {
            return Err(DeathAuthorityError::DangerEntryRequired);
        }
        if !state.requires_danger_entry() && self.danger_entry.is_some() {
            return Err(DeathAuthorityError::DangerStillActive);
        }

        let next_lifetime = if state.counts_lifetime() {
            self.lifetime_ticks
                .checked_add(1)
                .ok_or(DeathAuthorityError::ClockOverflow)?
        } else {
            self.lifetime_ticks
        };
        let next_combat = if state.requires_danger_entry() {
            self.permadeath_combat_ticks
                .checked_add(1)
                .ok_or(DeathAuthorityError::ClockOverflow)?
        } else {
            self.permadeath_combat_ticks
        };
        let next_link_lost = if state == LifeClockTickState::DangerLinkLost {
            self.link_lost_ticks
                .checked_add(1)
                .filter(|ticks| *ticks <= LINK_LOST_VULNERABILITY_TICKS)
                .ok_or(DeathAuthorityError::LinkLostWindowExpired)?
        } else {
            0
        };
        self.lifetime_ticks = next_lifetime;
        self.permadeath_combat_ticks = next_combat;
        self.link_lost_ticks = next_link_lost;
        Ok(())
    }

    pub fn resolve_danger(
        &mut self,
        outcome: DangerTerminalOutcome,
    ) -> Result<(), DeathAuthorityError> {
        if self.dead {
            return Err(DeathAuthorityError::CharacterAlreadyDead);
        }
        if self.danger_entry.is_none() {
            return Err(DeathAuthorityError::DangerEntryRequired);
        }
        self.danger_entry = None;
        self.link_lost_ticks = 0;
        self.dead = outcome == DangerTerminalOutcome::Death;
        Ok(())
    }

    /// Applies TECH-023: keep actual lifetime, roll Echo combat time back to danger entry.
    pub fn restore_after_uncommitted_crash(&mut self) -> Result<(), DeathAuthorityError> {
        if self.dead {
            return Err(DeathAuthorityError::CommittedDeathCannotRestore);
        }
        let entry = self
            .danger_entry
            .take()
            .ok_or(DeathAuthorityError::DangerEntryRequired)?;
        self.permadeath_combat_ticks = entry.permadeath_combat_ticks;
        self.link_lost_ticks = 0;
        Ok(())
    }

    pub fn snapshot(&self) -> Result<DeathClockSnapshot, DeathAuthorityError> {
        Ok(DeathClockSnapshot {
            lifetime_ticks: self.lifetime_ticks,
            lifetime_ms: ticks_to_milliseconds(self.lifetime_ticks)?,
            permadeath_combat_ticks: self.permadeath_combat_ticks,
            echo_time_eligible: self.permadeath_combat_ticks >= ECHO_COMBAT_ELIGIBILITY_TICKS,
            danger_active: self.danger_entry.is_some(),
            link_lost_ticks: self.link_lost_ticks,
            dead: self.dead,
        })
    }
}

/// `floor(ticks * 1000 / 30)` without overflowing the intermediate product.
pub fn ticks_to_milliseconds(ticks: u64) -> Result<u64, DeathAuthorityError> {
    let whole_seconds = ticks / u64::from(TICKS_PER_SECOND);
    let remainder_ticks = ticks % u64::from(TICKS_PER_SECOND);
    whole_seconds
        .checked_mul(1_000)
        .and_then(|whole| {
            whole.checked_add(
                remainder_ticks
                    .checked_mul(1_000)?
                    .checked_div(u64::from(TICKS_PER_SECOND))?,
            )
        })
        .ok_or(DeathAuthorityError::ClockOverflow)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeedCompletionKind {
    DungeonBoss,
    MajorRealmEvent,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeedCompletionMode {
    Normal,
    Practice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeedLifeState {
    Living,
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeedRecallState {
    Present,
    Recalled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeedCompletionObservation {
    pub completion_id: String,
    pub deed_id: String,
    pub achieved_tick: Tick,
    pub kind: DeedCompletionKind,
    pub mode: DeedCompletionMode,
    pub life_state: DeedLifeState,
    pub recall_state: DeedRecallState,
    pub reward_qualified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewardQualifiedDeed {
    pub completion_id: String,
    pub deed_id: String,
    pub achieved_tick: Tick,
    pub kind: DeedCompletionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeedIneligibilityReason {
    Practice,
    Dead,
    Recalled,
    RewardIneligible,
    UnsupportedCompletion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeedRecordOutcome {
    Recorded,
    IdempotentReplay,
    Ignored(DeedIneligibilityReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalDeed {
    pub deed_id: String,
    pub achieved_tick: Option<Tick>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeedCheckpointV1 {
    pub schema_version: u16,
    pub completions: Vec<RewardQualifiedDeed>,
}

/// Idempotent, reward-qualified deeds for one mortal life.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeedAggregate {
    completions: BTreeMap<String, RewardQualifiedDeed>,
}

impl DeedAggregate {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            completions: BTreeMap::new(),
        }
    }

    pub fn from_checkpoint(checkpoint: DeedCheckpointV1) -> Result<Self, DeathAuthorityError> {
        if checkpoint.schema_version != DEATH_AUTHORITY_SCHEMA_VERSION {
            return Err(DeathAuthorityError::CorruptDeedCheckpoint);
        }
        let mut aggregate = Self::new();
        let mut previous_completion_id: Option<String> = None;
        for completion in checkpoint.completions {
            validate_stable_id(&completion.completion_id)?;
            validate_stable_id(&completion.deed_id)?;
            if completion.achieved_tick.0 == 0
                || !matches!(
                    completion.kind,
                    DeedCompletionKind::DungeonBoss | DeedCompletionKind::MajorRealmEvent
                )
                || previous_completion_id.as_deref().is_some_and(|previous| {
                    previous.as_bytes() >= completion.completion_id.as_bytes()
                })
                || aggregate
                    .completions
                    .insert(completion.completion_id.clone(), completion)
                    .is_some()
            {
                return Err(DeathAuthorityError::CorruptDeedCheckpoint);
            }
            previous_completion_id = aggregate
                .completions
                .last_key_value()
                .map(|(id, _)| id.clone());
        }
        Ok(aggregate)
    }

    #[must_use]
    pub fn checkpoint(&self) -> DeedCheckpointV1 {
        DeedCheckpointV1 {
            schema_version: DEATH_AUTHORITY_SCHEMA_VERSION,
            completions: self.completions.values().cloned().collect(),
        }
    }

    pub fn record(
        &mut self,
        observation: DeedCompletionObservation,
    ) -> Result<DeedRecordOutcome, DeathAuthorityError> {
        validate_stable_id(&observation.completion_id)?;
        validate_stable_id(&observation.deed_id)?;
        if observation.achieved_tick.0 == 0 {
            return Err(DeathAuthorityError::InvalidAchievedTick);
        }

        if let Some(existing) = self.completions.get(&observation.completion_id) {
            return if observation.mode == DeedCompletionMode::Normal
                && observation.life_state == DeedLifeState::Living
                && observation.recall_state == DeedRecallState::Present
                && observation.reward_qualified
                && existing.deed_id == observation.deed_id
                && existing.achieved_tick == observation.achieved_tick
                && existing.kind == observation.kind
            {
                Ok(DeedRecordOutcome::IdempotentReplay)
            } else {
                Err(DeathAuthorityError::CompletionIdConflict(
                    observation.completion_id,
                ))
            };
        }

        let ignored = if observation.mode == DeedCompletionMode::Practice {
            Some(DeedIneligibilityReason::Practice)
        } else if observation.life_state == DeedLifeState::Dead {
            Some(DeedIneligibilityReason::Dead)
        } else if observation.recall_state == DeedRecallState::Recalled {
            Some(DeedIneligibilityReason::Recalled)
        } else if !observation.reward_qualified {
            Some(DeedIneligibilityReason::RewardIneligible)
        } else if observation.kind == DeedCompletionKind::Other {
            Some(DeedIneligibilityReason::UnsupportedCompletion)
        } else {
            None
        };
        if let Some(reason) = ignored {
            return Ok(DeedRecordOutcome::Ignored(reason));
        }

        let completion = RewardQualifiedDeed {
            completion_id: observation.completion_id.clone(),
            deed_id: observation.deed_id,
            achieved_tick: observation.achieved_tick,
            kind: observation.kind,
        };
        self.completions
            .insert(observation.completion_id, completion);
        Ok(DeedRecordOutcome::Recorded)
    }

    #[must_use]
    pub fn echo_deed_eligible(&self) -> bool {
        let bosses = self
            .completions
            .values()
            .filter(|entry| entry.kind == DeedCompletionKind::DungeonBoss)
            .count();
        let major_events = self
            .completions
            .values()
            .filter(|entry| entry.kind == DeedCompletionKind::MajorRealmEvent)
            .map(|entry| entry.deed_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        bosses >= 1 || major_events.len() >= 2
    }

    #[must_use]
    pub fn final_deed(&self) -> FinalDeed {
        self.completions
            .values()
            .max_by(|left, right| {
                (left.achieved_tick, left.deed_id.as_bytes())
                    .cmp(&(right.achieved_tick, right.deed_id.as_bytes()))
            })
            .map_or_else(
                || FinalDeed {
                    deed_id: DEED_NONE_ID.to_owned(),
                    achieved_tick: None,
                },
                |entry| FinalDeed {
                    deed_id: entry.deed_id.clone(),
                    achieved_tick: Some(entry.achieved_tick),
                },
            )
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.completions.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.completions.is_empty()
    }
}

/// Presentation-only Core fallback copy. Durable rows store only the deed ID.
#[must_use]
pub const fn core_deed_en_us(deed_id: &str) -> Option<&'static str> {
    match deed_id.as_bytes() {
        b"deed.none" => Some(DEED_NONE_EN_US),
        b"deed.core.sepulcher_knight_defeated" => Some("Defeated the Sepulcher Knight."),
        b"deed.core.sir_caldus_defeated" => Some("Defeated Sir Caldus."),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthoritativeDeathCauseKind {
    DirectHit,
    DamageOverTime,
    Environment,
    Disconnect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathTraceNetworkState {
    Connected,
    Degraded,
    LinkLost,
    Reattached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathTraceRecallState {
    Inactive,
    Channeling,
    CompletionPending,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DeathTraceStatus {
    pub status_id: String,
    pub remaining_ticks: u32,
    pub stack_count: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DamageTraceObservation {
    pub tick: Tick,
    pub event_ordinal: u32,
    pub cause_kind: AuthoritativeDeathCauseKind,
    pub source_content_id: String,
    pub source_entity_id: Option<EntityId>,
    pub pattern_id: Option<String>,
    pub attack_id: String,
    pub raw_damage: u32,
    pub final_damage: u32,
    pub damage_type: DamageType,
    pub pre_health: u32,
    pub post_health: u32,
    pub source_position: SimulationVector,
    pub statuses: Vec<DeathTraceStatus>,
    pub network_state: DeathTraceNetworkState,
    pub recall_state: DeathTraceRecallState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageTraceEntry {
    pub tick: Tick,
    pub event_ordinal: u32,
    pub cause_kind: AuthoritativeDeathCauseKind,
    pub source_content_id: String,
    pub source_entity_id: Option<EntityId>,
    pub pattern_id: Option<String>,
    pub attack_id: String,
    pub raw_damage: u32,
    pub final_damage: u32,
    pub damage_type: DamageType,
    pub pre_health: u32,
    pub post_health: u32,
    pub source_x_milli_tiles: i32,
    pub source_y_milli_tiles: i32,
    pub statuses: Vec<DeathTraceStatus>,
    pub network_state: DeathTraceNetworkState,
    pub recall_state: DeathTraceRecallState,
    pub lethal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoritativeDeathCause {
    pub kind: AuthoritativeDeathCauseKind,
    pub lethal_entry: DamageTraceEntry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathTraceTerminalSnapshot {
    pub cause: AuthoritativeDeathCause,
    pub trace: Vec<DamageTraceEntry>,
    pub last_five: Vec<DamageTraceEntry>,
    pub canonical_hash_blake3: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageTraceCheckpointV1 {
    pub schema_version: u16,
    pub entries: Vec<DamageTraceEntry>,
}

/// Bounded authoritative ten-second damage window, ordered by `(tick, event_ordinal)`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DamageTraceAggregate {
    entries: VecDeque<DamageTraceEntry>,
    terminal: bool,
}

impl DamageTraceAggregate {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            terminal: false,
        }
    }

    pub fn from_checkpoint(
        checkpoint: DamageTraceCheckpointV1,
    ) -> Result<Self, DeathAuthorityError> {
        if checkpoint.schema_version != DEATH_AUTHORITY_SCHEMA_VERSION {
            return Err(DeathAuthorityError::CorruptTraceCheckpoint);
        }
        let expected_entries = checkpoint.entries.clone();
        let mut aggregate = Self::new();
        for entry in checkpoint.entries {
            aggregate.push_compiled(entry)?;
        }
        if aggregate.entries.iter().ne(expected_entries.iter()) {
            return Err(DeathAuthorityError::CorruptTraceCheckpoint);
        }
        Ok(aggregate)
    }

    #[must_use]
    pub fn checkpoint(&self) -> DamageTraceCheckpointV1 {
        DamageTraceCheckpointV1 {
            schema_version: DEATH_AUTHORITY_SCHEMA_VERSION,
            entries: self.entries.iter().cloned().collect(),
        }
    }

    /// Accepts one complete authoritative tick; caller order cannot affect same-tick ordering.
    pub fn record_tick(
        &mut self,
        observations: impl IntoIterator<Item = DamageTraceObservation>,
    ) -> Result<Vec<DamageTraceEntry>, DeathAuthorityError> {
        if self.terminal {
            return Err(DeathAuthorityError::TraceAlreadyTerminal);
        }
        let mut compiled = observations
            .into_iter()
            .map(compile_trace_observation)
            .collect::<Result<Vec<_>, _>>()?;
        if compiled.is_empty() {
            return Ok(Vec::new());
        }
        let tick = compiled[0].tick;
        if compiled.iter().any(|entry| entry.tick != tick) {
            return Err(DeathAuthorityError::MixedTraceTicks);
        }
        compiled.sort_by_key(|entry| entry.event_ordinal);
        if let Some(duplicate) = compiled
            .windows(2)
            .find(|pair| pair[0].event_ordinal == pair[1].event_ordinal)
        {
            return Err(DeathAuthorityError::DuplicateTraceOrdinal {
                tick,
                event_ordinal: duplicate[0].event_ordinal,
            });
        }
        if compiled.iter().filter(|entry| entry.lethal).count() > 1
            || compiled
                .iter()
                .position(|entry| entry.lethal)
                .is_some_and(|index| index + 1 != compiled.len())
        {
            return Err(DeathAuthorityError::InconsistentLethality);
        }

        let mut staged = self.clone();
        for entry in &compiled {
            staged.push_compiled(entry.clone())?;
        }
        *self = staged;
        Ok(compiled)
    }

    pub fn terminal_snapshot(&self) -> Result<DeathTraceTerminalSnapshot, DeathAuthorityError> {
        if !self.terminal {
            return Err(DeathAuthorityError::LethalEntryRequired);
        }
        let trace: Vec<_> = self.entries.iter().cloned().collect();
        let lethal_entries: Vec<_> = trace.iter().filter(|entry| entry.lethal).collect();
        if lethal_entries.len() != 1 || !trace.last().is_some_and(|entry| entry.lethal) {
            return Err(DeathAuthorityError::InconsistentLethality);
        }
        let lethal_entry = lethal_entries[0].clone();
        let last_five = trace
            .iter()
            .skip(trace.len().saturating_sub(5))
            .cloned()
            .collect();
        Ok(DeathTraceTerminalSnapshot {
            cause: AuthoritativeDeathCause {
                kind: lethal_entry.cause_kind,
                lethal_entry,
            },
            canonical_hash_blake3: hash_trace(&trace),
            trace,
            last_five,
        })
    }

    #[must_use]
    pub fn entries(&self) -> Vec<DamageTraceEntry> {
        self.entries.iter().cloned().collect()
    }

    fn push_compiled(&mut self, entry: DamageTraceEntry) -> Result<(), DeathAuthorityError> {
        validate_compiled_entry(&entry)?;
        if self.terminal {
            return Err(DeathAuthorityError::TraceAlreadyTerminal);
        }
        if let Some(previous) = self.entries.back()
            && (entry.tick, entry.event_ordinal) <= (previous.tick, previous.event_ordinal)
        {
            return Err(DeathAuthorityError::TraceOrderRegression {
                previous_tick: previous.tick,
                previous_ordinal: previous.event_ordinal,
                actual_tick: entry.tick,
                actual_ordinal: entry.event_ordinal,
            });
        }

        while self.entries.front().is_some_and(|front| {
            entry.tick.0.saturating_sub(front.tick.0) > DEATH_TRACE_WINDOW_TICKS
        }) {
            self.entries.pop_front();
        }
        if self.entries.len() >= MAX_DEATH_TRACE_ENTRIES {
            return Err(DeathAuthorityError::TraceCapacityExceeded);
        }
        self.terminal = entry.lethal;
        self.entries.push_back(entry);
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoritativeDeathInputs {
    pub clocks: DeathClockSnapshot,
    pub final_deed: FinalDeed,
    pub echo_deed_eligible: bool,
    pub cause: AuthoritativeDeathCause,
    pub trace: Vec<DamageTraceEntry>,
    pub last_five: Vec<DamageTraceEntry>,
    pub trace_digest: [u8; 32],
}

pub fn compile_authoritative_death_inputs(
    clocks: &DeathClockAggregate,
    deeds: &DeedAggregate,
    trace: &DamageTraceAggregate,
) -> Result<AuthoritativeDeathInputs, DeathAuthorityError> {
    let clock_snapshot = clocks.snapshot()?;
    if !clock_snapshot.dead {
        return Err(DeathAuthorityError::CommittedDeathRequired);
    }
    let trace_snapshot = trace.terminal_snapshot()?;
    Ok(AuthoritativeDeathInputs {
        clocks: clock_snapshot,
        final_deed: deeds.final_deed(),
        echo_deed_eligible: deeds.echo_deed_eligible(),
        cause: trace_snapshot.cause,
        trace: trace_snapshot.trace,
        last_five: trace_snapshot.last_five,
        trace_digest: trace_snapshot.canonical_hash_blake3,
    })
}

fn compile_trace_observation(
    mut observation: DamageTraceObservation,
) -> Result<DamageTraceEntry, DeathAuthorityError> {
    validate_stable_id(&observation.source_content_id)?;
    if let Some(pattern_id) = observation.pattern_id.as_deref() {
        validate_stable_id(pattern_id)?;
    }
    validate_stable_id(&observation.attack_id)?;
    if !observation.source_position.is_finite() {
        return Err(DeathAuthorityError::NonFiniteSourcePosition);
    }
    let source_position_milli = (
        quantize_milli(observation.source_position.x)?,
        quantize_milli(observation.source_position.y)?,
    );

    observation
        .statuses
        .sort_by(|left, right| left.status_id.as_bytes().cmp(right.status_id.as_bytes()));
    if observation.statuses.len() > MAX_DEATH_TRACE_STATUSES {
        return Err(DeathAuthorityError::TooManyTraceStatuses);
    }
    for (index, status) in observation.statuses.iter().enumerate() {
        validate_stable_id(&status.status_id)?;
        if status.remaining_ticks > MAX_DEATH_TRACE_STATUS_TICKS
            || !(1..=255).contains(&status.stack_count)
        {
            return Err(DeathAuthorityError::InvalidTraceStatus);
        }
        if index > 0 && observation.statuses[index - 1].status_id == status.status_id {
            return Err(DeathAuthorityError::DuplicateTraceStatus(
                status.status_id.clone(),
            ));
        }
    }

    let entry = DamageTraceEntry {
        tick: observation.tick,
        event_ordinal: observation.event_ordinal,
        cause_kind: observation.cause_kind,
        source_content_id: observation.source_content_id,
        source_entity_id: observation.source_entity_id,
        pattern_id: observation.pattern_id,
        attack_id: observation.attack_id,
        raw_damage: observation.raw_damage,
        final_damage: observation.final_damage,
        damage_type: observation.damage_type,
        pre_health: observation.pre_health,
        post_health: observation.post_health,
        source_x_milli_tiles: source_position_milli.0,
        source_y_milli_tiles: source_position_milli.1,
        statuses: observation.statuses,
        network_state: observation.network_state,
        recall_state: observation.recall_state,
        lethal: observation.post_health == 0,
    };
    validate_compiled_entry(&entry)?;
    Ok(entry)
}

fn validate_compiled_entry(entry: &DamageTraceEntry) -> Result<(), DeathAuthorityError> {
    if entry.tick.0 == 0 {
        return Err(DeathAuthorityError::InvalidTraceTick);
    }
    validate_stable_id(&entry.source_content_id)?;
    if let Some(pattern_id) = entry.pattern_id.as_deref() {
        validate_stable_id(pattern_id)?;
    }
    validate_stable_id(&entry.attack_id)?;
    if entry.pre_health == 0 {
        return Err(DeathAuthorityError::InvalidTraceDamage);
    }
    if entry.post_health != entry.pre_health.saturating_sub(entry.final_damage) {
        return Err(DeathAuthorityError::HealthArithmeticMismatch);
    }
    if entry.lethal != (entry.post_health == 0) {
        return Err(DeathAuthorityError::InconsistentLethality);
    }
    if entry.statuses.len() > MAX_DEATH_TRACE_STATUSES {
        return Err(DeathAuthorityError::TooManyTraceStatuses);
    }
    for (index, status) in entry.statuses.iter().enumerate() {
        validate_stable_id(&status.status_id)?;
        if status.remaining_ticks > MAX_DEATH_TRACE_STATUS_TICKS
            || !(1..=255).contains(&status.stack_count)
        {
            return Err(DeathAuthorityError::InvalidTraceStatus);
        }
        if index > 0
            && entry.statuses[index - 1].status_id.as_bytes() >= status.status_id.as_bytes()
        {
            return Err(DeathAuthorityError::CorruptTraceCheckpoint);
        }
    }
    Ok(())
}

fn quantize_milli(value: f32) -> Result<i32, DeathAuthorityError> {
    let scaled = f64::from(value) * 1_000.0;
    if !scaled.is_finite() || scaled < f64::from(i32::MIN) || scaled > f64::from(i32::MAX) {
        return Err(DeathAuthorityError::SourcePositionOutOfRange);
    }
    #[allow(clippy::cast_possible_truncation)]
    Ok(scaled.round() as i32)
}

fn hash_trace(trace: &[DamageTraceEntry]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"gravebound-death-trace-v1\0");
    hash_u64(&mut hasher, trace.len() as u64);
    for entry in trace {
        hash_u64(&mut hasher, entry.tick.0);
        hasher.update(&entry.event_ordinal.to_le_bytes());
        hasher.update(&[cause_tag(entry.cause_kind)]);
        hash_bytes(&mut hasher, entry.source_content_id.as_bytes());
        match entry.source_entity_id {
            Some(entity_id) => {
                hasher.update(&[1]);
                hash_u64(&mut hasher, entity_id.get());
            }
            None => {
                hasher.update(&[0]);
            }
        }
        match entry.pattern_id.as_deref() {
            Some(pattern_id) => {
                hasher.update(&[1]);
                hash_bytes(&mut hasher, pattern_id.as_bytes());
            }
            None => {
                hasher.update(&[0]);
            }
        }
        hash_bytes(&mut hasher, entry.attack_id.as_bytes());
        hasher.update(&entry.raw_damage.to_le_bytes());
        hasher.update(&entry.final_damage.to_le_bytes());
        hasher.update(&[damage_type_tag(entry.damage_type)]);
        hasher.update(&entry.pre_health.to_le_bytes());
        hasher.update(&entry.post_health.to_le_bytes());
        hasher.update(&entry.source_x_milli_tiles.to_le_bytes());
        hasher.update(&entry.source_y_milli_tiles.to_le_bytes());
        hasher.update(&[network_tag(entry.network_state)]);
        hasher.update(&[recall_tag(entry.recall_state)]);
        hasher.update(&[u8::from(entry.lethal)]);
        hash_u64(&mut hasher, entry.statuses.len() as u64);
        for status in &entry.statuses {
            hash_bytes(&mut hasher, status.status_id.as_bytes());
            hasher.update(&status.remaining_ticks.to_le_bytes());
            hasher.update(&status.stack_count.to_le_bytes());
        }
    }
    *hasher.finalize().as_bytes()
}

fn hash_bytes(hasher: &mut blake3::Hasher, value: &[u8]) {
    hash_u64(hasher, value.len() as u64);
    hasher.update(value);
}

fn hash_u64(hasher: &mut blake3::Hasher, value: u64) {
    hasher.update(&value.to_le_bytes());
}

const fn cause_tag(value: AuthoritativeDeathCauseKind) -> u8 {
    match value {
        AuthoritativeDeathCauseKind::DirectHit => 0,
        AuthoritativeDeathCauseKind::DamageOverTime => 1,
        AuthoritativeDeathCauseKind::Environment => 2,
        AuthoritativeDeathCauseKind::Disconnect => 3,
    }
}

const fn damage_type_tag(value: DamageType) -> u8 {
    match value {
        DamageType::Physical => 0,
        DamageType::Veil => 1,
    }
}

const fn network_tag(value: DeathTraceNetworkState) -> u8 {
    match value {
        DeathTraceNetworkState::Connected => 0,
        DeathTraceNetworkState::Degraded => 1,
        DeathTraceNetworkState::LinkLost => 2,
        DeathTraceNetworkState::Reattached => 3,
    }
}

const fn recall_tag(value: DeathTraceRecallState) -> u8 {
    match value {
        DeathTraceRecallState::Inactive => 0,
        DeathTraceRecallState::Channeling => 1,
        DeathTraceRecallState::CompletionPending => 2,
    }
}

fn validate_stable_id(value: &str) -> Result<(), DeathAuthorityError> {
    if !(3..=96).contains(&value.len())
        || !value.split('.').all(|segment| {
            !segment.is_empty()
                && segment.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'_'
                        || byte == b'-'
                })
        })
    {
        return Err(DeathAuthorityError::InvalidStableId(value.to_owned()));
    }
    Ok(())
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DeathAuthorityError {
    #[error("death-authority clock arithmetic overflowed")]
    ClockOverflow,
    #[error("the character is already dead")]
    CharacterAlreadyDead,
    #[error("danger entry is already active")]
    DangerAlreadyActive,
    #[error("a committed danger entry is required")]
    DangerEntryRequired,
    #[error("danger must resolve before Hall control resumes")]
    DangerStillActive,
    #[error("the 90-tick vulnerable LinkLost window has expired and must resolve")]
    LinkLostWindowExpired,
    #[error("a committed death cannot use crash restoration")]
    CommittedDeathCannotRestore,
    #[error("the clock checkpoint is corrupt or unsupported")]
    CorruptClockCheckpoint,
    #[error("the deed checkpoint is corrupt or unsupported")]
    CorruptDeedCheckpoint,
    #[error("the trace checkpoint is corrupt or unsupported")]
    CorruptTraceCheckpoint,
    #[error("stable ID is invalid: {0}")]
    InvalidStableId(String),
    #[error("a deed achieved tick must be nonzero")]
    InvalidAchievedTick,
    #[error("completion ID was replayed with changed authority: {0}")]
    CompletionIdConflict(String),
    #[error("the trace tick must be nonzero")]
    InvalidTraceTick,
    #[error("a trace batch mixed authoritative ticks")]
    MixedTraceTicks,
    #[error("duplicate event ordinal {event_ordinal} at {tick:?}")]
    DuplicateTraceOrdinal { tick: Tick, event_ordinal: u32 },
    #[error(
        "trace order regressed from ({previous_tick:?}, {previous_ordinal}) to ({actual_tick:?}, {actual_ordinal})"
    )]
    TraceOrderRegression {
        previous_tick: Tick,
        previous_ordinal: u32,
        actual_tick: Tick,
        actual_ordinal: u32,
    },
    #[error("the ten-second trace exceeded its bounded capacity")]
    TraceCapacityExceeded,
    #[error("the trace already contains its unique lethal event")]
    TraceAlreadyTerminal,
    #[error("terminal staging requires exactly one final lethal entry")]
    LethalEntryRequired,
    #[error("trace lethality is inconsistent")]
    InconsistentLethality,
    #[error("trace pre-health must be nonzero")]
    InvalidTraceDamage,
    #[error("trace pre-health, damage, and post-health disagree")]
    HealthArithmeticMismatch,
    #[error("source position contains nonfinite data")]
    NonFiniteSourcePosition,
    #[error("source position cannot be represented in fixed-point milli-tiles")]
    SourcePositionOutOfRange,
    #[error("a trace entry contains too many statuses")]
    TooManyTraceStatuses,
    #[error("trace status bounds are invalid")]
    InvalidTraceStatus,
    #[error("trace status is duplicated: {0}")]
    DuplicateTraceStatus(String),
    #[error("authoritative death inputs require a committed death")]
    CommittedDeathRequired,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: u64) -> EntityId {
        EntityId::new(value).expect("nonzero entity")
    }

    fn deed(
        completion_id: &str,
        deed_id: &str,
        tick: u64,
        kind: DeedCompletionKind,
    ) -> DeedCompletionObservation {
        DeedCompletionObservation {
            completion_id: completion_id.to_owned(),
            deed_id: deed_id.to_owned(),
            achieved_tick: Tick(tick),
            kind,
            mode: DeedCompletionMode::Normal,
            life_state: DeedLifeState::Living,
            recall_state: DeedRecallState::Present,
            reward_qualified: true,
        }
    }

    fn damage(tick: u64, ordinal: u32, pre: u32, final_damage: u32) -> DamageTraceObservation {
        DamageTraceObservation {
            tick: Tick(tick),
            event_ordinal: ordinal,
            cause_kind: AuthoritativeDeathCauseKind::DirectHit,
            source_content_id: "enemy.core.bell_acolyte".to_owned(),
            source_entity_id: Some(id(7)),
            pattern_id: Some("pattern.core.bell_acolyte.fan".to_owned()),
            attack_id: "attack.core.bell_acolyte.fan".to_owned(),
            raw_damage: final_damage,
            final_damage,
            damage_type: DamageType::Physical,
            pre_health: pre,
            post_health: pre.saturating_sub(final_damage),
            source_position: SimulationVector::new(12.125, 7.5),
            statuses: vec![DeathTraceStatus {
                status_id: "status.frostbind".to_owned(),
                remaining_ticks: 30,
                stack_count: 1,
            }],
            network_state: DeathTraceNetworkState::Connected,
            recall_state: DeathTraceRecallState::Inactive,
        }
    }

    #[test]
    fn clocks_apply_exact_hall_danger_loading_offline_and_link_lost_boundaries() {
        let mut clocks = DeathClockAggregate::new();
        for state in [
            LifeClockTickState::CharacterSelect,
            LifeClockTickState::Loading,
            LifeClockTickState::Offline,
            LifeClockTickState::HallControllable,
        ] {
            clocks.advance(state).expect("safe clock tick");
        }
        assert_eq!(clocks.snapshot().expect("snapshot").lifetime_ticks, 1);
        assert_eq!(
            clocks.advance(LifeClockTickState::DangerControllable),
            Err(DeathAuthorityError::DangerEntryRequired)
        );

        clocks.enter_danger().expect("entry");
        for state in [
            LifeClockTickState::DangerLoading,
            LifeClockTickState::DangerStaging,
            LifeClockTickState::DangerControllable,
            LifeClockTickState::DangerLinkLost,
        ] {
            clocks.advance(state).expect("danger clock tick");
        }
        let snapshot = clocks.snapshot().expect("snapshot");
        assert_eq!(snapshot.lifetime_ticks, 3);
        assert_eq!(snapshot.permadeath_combat_ticks, 4);
        assert_eq!(snapshot.lifetime_ms, 100);
        assert_eq!(snapshot.link_lost_ticks, 1);
        assert_eq!(
            clocks.advance(LifeClockTickState::HallControllable),
            Err(DeathAuthorityError::DangerStillActive)
        );
        clocks
            .resolve_danger(DangerTerminalOutcome::Extraction)
            .expect("extract");
        clocks
            .advance(LifeClockTickState::HallControllable)
            .expect("hall after extract");
        assert_eq!(clocks.snapshot().expect("snapshot").lifetime_ticks, 4);
        assert_eq!(LINK_LOST_VULNERABILITY_TICKS, 90);
        assert_eq!(RECALL_CHANNEL_TICKS, 12);
    }

    #[test]
    fn link_lost_counts_exactly_90_vulnerable_ticks_then_requires_resolution() {
        let mut clocks = DeathClockAggregate::new();
        clocks.enter_danger().expect("entry");
        for _ in 0..LINK_LOST_VULNERABILITY_TICKS {
            clocks
                .advance(LifeClockTickState::DangerLinkLost)
                .expect("vulnerable tick");
        }
        let at_boundary = clocks.clone();
        assert_eq!(
            clocks.advance(LifeClockTickState::DangerLinkLost),
            Err(DeathAuthorityError::LinkLostWindowExpired)
        );
        assert_eq!(clocks, at_boundary);
        clocks
            .resolve_danger(DangerTerminalOutcome::EmergencyRecall)
            .expect("automatic Recall");
        assert_eq!(clocks.snapshot().expect("snapshot").link_lost_ticks, 0);
    }

    #[test]
    fn combat_threshold_is_exactly_17_999_and_18_000() {
        let mut clocks = DeathClockAggregate::new();
        clocks.enter_danger().expect("entry");
        for _ in 0..17_999 {
            clocks
                .advance(LifeClockTickState::DangerControllable)
                .expect("tick");
        }
        assert!(!clocks.snapshot().expect("snapshot").echo_time_eligible);
        clocks
            .advance(LifeClockTickState::DangerControllable)
            .expect("threshold tick");
        assert!(clocks.snapshot().expect("snapshot").echo_time_eligible);
        assert_eq!(ECHO_COMBAT_ELIGIBILITY_TICKS, 18_000);
    }

    #[test]
    fn extraction_recall_and_death_stop_combat_at_the_committed_boundary() {
        for outcome in [
            DangerTerminalOutcome::Extraction,
            DangerTerminalOutcome::EmergencyRecall,
        ] {
            let mut clocks = DeathClockAggregate::new();
            clocks.enter_danger().expect("entry");
            clocks
                .advance(LifeClockTickState::DangerControllable)
                .expect("combat");
            clocks.resolve_danger(outcome).expect("resolution");
            clocks
                .advance(LifeClockTickState::HallControllable)
                .expect("hall");
            assert_eq!(
                clocks.snapshot().expect("snapshot").permadeath_combat_ticks,
                1
            );
        }

        let mut dead = DeathClockAggregate::new();
        dead.enter_danger().expect("entry");
        dead.resolve_danger(DangerTerminalOutcome::Death)
            .expect("death");
        assert_eq!(
            dead.advance(LifeClockTickState::DangerControllable),
            Err(DeathAuthorityError::CharacterAlreadyDead)
        );
    }

    #[test]
    fn crash_restores_entry_combat_clock_but_preserves_actual_lifetime() {
        let mut clocks = DeathClockAggregate::new();
        clocks
            .advance(LifeClockTickState::HallControllable)
            .expect("hall");
        clocks.enter_danger().expect("entry");
        for _ in 0..60 {
            clocks
                .advance(LifeClockTickState::DangerLinkLost)
                .expect("link lost");
        }
        clocks
            .restore_after_uncommitted_crash()
            .expect("crash restore");
        let restored = clocks.snapshot().expect("snapshot");
        assert_eq!(restored.lifetime_ticks, 61);
        assert_eq!(restored.permadeath_combat_ticks, 0);
        assert!(!restored.danger_active);
    }

    #[test]
    fn clock_checkpoint_rejects_corruption_and_replays_identically() {
        let checkpoint = DeathClockCheckpointV1 {
            schema_version: DEATH_AUTHORITY_SCHEMA_VERSION,
            lifetime_ticks: 200,
            permadeath_combat_ticks: 100,
            danger_entry: Some(DangerEntryClockSnapshot {
                permadeath_combat_ticks: 80,
            }),
            link_lost_ticks: 4,
            dead: false,
        };
        let restored = DeathClockAggregate::from_checkpoint(checkpoint).expect("checkpoint");
        assert_eq!(restored.checkpoint(), checkpoint);
        assert_eq!(
            DeathClockAggregate::from_checkpoint(DeathClockCheckpointV1 {
                danger_entry: Some(DangerEntryClockSnapshot {
                    permadeath_combat_ticks: 101,
                }),
                ..checkpoint
            }),
            Err(DeathAuthorityError::CorruptClockCheckpoint)
        );
    }

    #[test]
    fn checked_tick_to_millisecond_conversion_has_exact_floor_and_overflow() {
        assert_eq!(ticks_to_milliseconds(1), Ok(33));
        assert_eq!(ticks_to_milliseconds(29), Ok(966));
        assert_eq!(ticks_to_milliseconds(30), Ok(1_000));
        assert_eq!(
            ticks_to_milliseconds(u64::MAX),
            Err(DeathAuthorityError::ClockOverflow)
        );
    }

    #[test]
    fn clock_overflow_is_transactional() {
        let checkpoint = DeathClockCheckpointV1 {
            schema_version: DEATH_AUTHORITY_SCHEMA_VERSION,
            lifetime_ticks: u64::MAX,
            permadeath_combat_ticks: 0,
            danger_entry: None,
            link_lost_ticks: 0,
            dead: false,
        };
        let mut clocks = DeathClockAggregate::from_checkpoint(checkpoint).expect("maximum clock");
        assert_eq!(
            clocks.advance(LifeClockTickState::HallControllable),
            Err(DeathAuthorityError::ClockOverflow)
        );
        assert_eq!(clocks.checkpoint(), checkpoint);
    }

    #[test]
    fn deeds_are_reward_qualified_idempotent_and_conflict_safe() {
        let mut deeds = DeedAggregate::new();
        let completion = deed(
            "completion.core.caldus.0001",
            DEED_SIR_CALDUS_DEFEATED_ID,
            500,
            DeedCompletionKind::DungeonBoss,
        );
        assert_eq!(
            deeds.record(completion.clone()),
            Ok(DeedRecordOutcome::Recorded)
        );
        assert_eq!(
            deeds.record(completion.clone()),
            Ok(DeedRecordOutcome::IdempotentReplay)
        );
        let mut changed = completion;
        changed.achieved_tick = Tick(501);
        assert_eq!(
            deeds.record(changed),
            Err(DeathAuthorityError::CompletionIdConflict(
                "completion.core.caldus.0001".to_owned()
            ))
        );
        let mut changed_authority = deed(
            "completion.core.caldus.0001",
            DEED_SIR_CALDUS_DEFEATED_ID,
            500,
            DeedCompletionKind::DungeonBoss,
        );
        changed_authority.mode = DeedCompletionMode::Practice;
        assert!(matches!(
            deeds.record(changed_authority),
            Err(DeathAuthorityError::CompletionIdConflict(_))
        ));
        assert_eq!(deeds.len(), 1);
        assert!(deeds.echo_deed_eligible());
    }

    #[test]
    fn practice_dead_recalled_ineligible_and_other_rewards_do_not_count() {
        fn make_practice(entry: &mut DeedCompletionObservation) {
            entry.mode = DeedCompletionMode::Practice;
        }
        fn make_dead(entry: &mut DeedCompletionObservation) {
            entry.life_state = DeedLifeState::Dead;
        }
        fn make_recalled(entry: &mut DeedCompletionObservation) {
            entry.recall_state = DeedRecallState::Recalled;
        }
        fn make_reward_ineligible(entry: &mut DeedCompletionObservation) {
            entry.reward_qualified = false;
        }

        let mut deeds = DeedAggregate::new();
        let cases = [
            (
                make_practice as fn(&mut DeedCompletionObservation),
                DeedIneligibilityReason::Practice,
            ),
            (make_dead, DeedIneligibilityReason::Dead),
            (make_recalled, DeedIneligibilityReason::Recalled),
            (
                make_reward_ineligible,
                DeedIneligibilityReason::RewardIneligible,
            ),
        ];
        for (index, (mutate, reason)) in cases.into_iter().enumerate() {
            let mut entry = deed(
                &format!("completion.core.rejected.{index}"),
                DEED_SEPULCHER_KNIGHT_DEFEATED_ID,
                10 + index as u64,
                DeedCompletionKind::DungeonBoss,
            );
            mutate(&mut entry);
            assert_eq!(deeds.record(entry), Ok(DeedRecordOutcome::Ignored(reason)));
        }
        let other = deed(
            "completion.core.minor.0001",
            "deed.core.minor",
            20,
            DeedCompletionKind::Other,
        );
        assert_eq!(
            deeds.record(other),
            Ok(DeedRecordOutcome::Ignored(
                DeedIneligibilityReason::UnsupportedCompletion
            ))
        );
        assert!(deeds.is_empty());
        assert!(!deeds.echo_deed_eligible());
    }

    #[test]
    fn one_boss_or_two_distinct_major_events_qualify_echo_deeds() {
        let mut events = DeedAggregate::new();
        events
            .record(deed(
                "completion.event.bell.0001",
                "deed.event.bell",
                100,
                DeedCompletionKind::MajorRealmEvent,
            ))
            .expect("first event");
        assert!(!events.echo_deed_eligible());
        events
            .record(deed(
                "completion.event.bell.0002",
                "deed.event.ashen_procession",
                101,
                DeedCompletionKind::MajorRealmEvent,
            ))
            .expect("second event");
        assert!(events.echo_deed_eligible());

        let mut duplicate_event = DeedAggregate::new();
        for ordinal in 1..=2 {
            duplicate_event
                .record(deed(
                    &format!("completion.event.bell.{ordinal:04}"),
                    "deed.event.bell",
                    100 + ordinal,
                    DeedCompletionKind::MajorRealmEvent,
                ))
                .expect("repeated event completion");
        }
        assert!(!duplicate_event.echo_deed_eligible());

        let mut boss = DeedAggregate::new();
        boss.record(deed(
            "completion.boss.knight.0001",
            DEED_SEPULCHER_KNIGHT_DEFEATED_ID,
            200,
            DeedCompletionKind::DungeonBoss,
        ))
        .expect("boss");
        assert!(boss.echo_deed_eligible());
    }

    #[test]
    fn final_deed_uses_latest_tick_then_utf8_id_and_exact_fallback_copy() {
        let empty = DeedAggregate::new();
        assert_eq!(
            empty.final_deed(),
            FinalDeed {
                deed_id: DEED_NONE_ID.to_owned(),
                achieved_tick: None
            }
        );
        assert_eq!(core_deed_en_us(DEED_NONE_ID), Some(DEED_NONE_EN_US));

        let mut deeds = DeedAggregate::new();
        deeds
            .record(deed(
                "completion.core.a",
                DEED_SEPULCHER_KNIGHT_DEFEATED_ID,
                300,
                DeedCompletionKind::DungeonBoss,
            ))
            .expect("first");
        deeds
            .record(deed(
                "completion.core.b",
                DEED_SIR_CALDUS_DEFEATED_ID,
                300,
                DeedCompletionKind::DungeonBoss,
            ))
            .expect("second");
        assert_eq!(deeds.final_deed().deed_id, DEED_SIR_CALDUS_DEFEATED_ID);
    }

    #[test]
    fn deed_checkpoint_is_canonical_and_corruption_rejects() {
        let mut deeds = DeedAggregate::new();
        deeds
            .record(deed(
                "completion.core.b",
                DEED_SIR_CALDUS_DEFEATED_ID,
                20,
                DeedCompletionKind::DungeonBoss,
            ))
            .expect("b");
        deeds
            .record(deed(
                "completion.core.a",
                DEED_SEPULCHER_KNIGHT_DEFEATED_ID,
                10,
                DeedCompletionKind::DungeonBoss,
            ))
            .expect("a");
        let checkpoint = deeds.checkpoint();
        assert_eq!(checkpoint.completions[0].completion_id, "completion.core.a");
        assert_eq!(
            DeedAggregate::from_checkpoint(checkpoint.clone())
                .expect("restore")
                .checkpoint(),
            checkpoint
        );
        let mut corrupt = checkpoint;
        corrupt.completions[0].kind = DeedCompletionKind::Other;
        assert_eq!(
            DeedAggregate::from_checkpoint(corrupt),
            Err(DeathAuthorityError::CorruptDeedCheckpoint)
        );

        let mut reversed = deeds.checkpoint();
        reversed.completions.reverse();
        assert_eq!(
            DeedAggregate::from_checkpoint(reversed),
            Err(DeathAuthorityError::CorruptDeedCheckpoint)
        );
    }

    #[test]
    fn same_tick_entries_use_event_ordinal_independent_of_input_order() {
        let mut first = DamageTraceAggregate::new();
        first
            .record_tick([damage(100, 2, 90, 10), damage(100, 1, 100, 10)])
            .expect("same tick");
        let mut second = DamageTraceAggregate::new();
        second
            .record_tick([damage(100, 1, 100, 10), damage(100, 2, 90, 10)])
            .expect("same tick");
        assert_eq!(first, second);
        assert_eq!(first.entries()[0].event_ordinal, 1);
        assert_eq!(first.entries()[1].event_ordinal, 2);
    }

    #[test]
    fn trace_window_keeps_exact_300_tick_edge_and_evicts_301() {
        let mut trace = DamageTraceAggregate::new();
        trace.record_tick([damage(1, 0, 100, 1)]).expect("first");
        trace
            .record_tick([damage(301, 0, 99, 1)])
            .expect("inclusive edge");
        assert_eq!(trace.entries().len(), 2);
        trace
            .record_tick([damage(302, 0, 98, 1)])
            .expect("eviction");
        assert_eq!(trace.entries().len(), 2);
        assert_eq!(trace.entries()[0].tick, Tick(301));
    }

    #[test]
    fn zero_effect_trace_entries_remain_valid_investigation_evidence() {
        let mut trace = DamageTraceAggregate::new();
        let mut resisted = damage(10, 0, 20, 0);
        resisted.raw_damage = 0;
        trace.record_tick([resisted]).expect("zero-effect evidence");
        let stored = trace.entries();
        assert_eq!(stored[0].pre_health, 20);
        assert_eq!(stored[0].post_health, 20);
        assert!(!stored[0].lethal);
    }

    #[test]
    fn trace_capacity_is_bounded_and_overflow_is_transactional() {
        let mut trace = DamageTraceAggregate::new();
        let observations = (0..MAX_DEATH_TRACE_ENTRIES)
            .map(|ordinal| damage(10, u32::try_from(ordinal).expect("bounded ordinal"), 100, 1))
            .collect::<Vec<_>>();
        trace.record_tick(observations).expect("bounded trace");
        let full = trace.clone();
        assert_eq!(
            trace.record_tick([damage(
                10,
                u32::try_from(MAX_DEATH_TRACE_ENTRIES).expect("bounded ordinal"),
                100,
                1,
            )]),
            Err(DeathAuthorityError::TraceCapacityExceeded)
        );
        assert_eq!(trace, full);
    }

    #[test]
    fn lethal_event_is_unique_final_and_deterministically_selects_cause() {
        let mut trace = DamageTraceAggregate::new();
        trace.record_tick([damage(10, 0, 30, 10)]).expect("chip");
        let mut lethal = damage(11, 4, 20, 25);
        lethal.cause_kind = AuthoritativeDeathCauseKind::DamageOverTime;
        lethal.network_state = DeathTraceNetworkState::LinkLost;
        lethal.recall_state = DeathTraceRecallState::Channeling;
        trace.record_tick([lethal]).expect("lethal");
        let snapshot = trace.terminal_snapshot().expect("terminal");
        assert_eq!(
            snapshot.cause.kind,
            AuthoritativeDeathCauseKind::DamageOverTime
        );
        assert_eq!(snapshot.cause.lethal_entry.tick, Tick(11));
        assert_eq!(
            snapshot.trace.iter().filter(|entry| entry.lethal).count(),
            1
        );
        assert_eq!(
            trace.record_tick([damage(12, 0, 1, 1)]),
            Err(DeathAuthorityError::TraceAlreadyTerminal)
        );
    }

    #[test]
    fn last_five_projection_is_oldest_to_newest_for_short_and_long_traces() {
        let mut short = DamageTraceAggregate::new();
        short.record_tick([damage(1, 0, 20, 5)]).expect("first");
        short.record_tick([damage(2, 0, 15, 15)]).expect("lethal");
        assert_eq!(short.terminal_snapshot().expect("short").last_five.len(), 2);

        let mut long = DamageTraceAggregate::new();
        for tick in 1..=6 {
            long.record_tick([damage(tick, 0, 70 - u32::try_from(tick).expect("small"), 1)])
                .expect("chip");
        }
        long.record_tick([damage(7, 0, 10, 10)]).expect("lethal");
        let last = long.terminal_snapshot().expect("long").last_five;
        assert_eq!(
            last.iter().map(|entry| entry.tick.0).collect::<Vec<_>>(),
            vec![3, 4, 5, 6, 7]
        );
    }

    #[test]
    fn invalid_position_ids_statuses_health_and_order_reject_without_mutation() {
        let mut trace = DamageTraceAggregate::new();
        let before = trace.clone();
        let mut invalid = damage(1, 0, 10, 1);
        invalid.source_position.x = f32::NAN;
        assert_eq!(
            trace.record_tick([invalid]),
            Err(DeathAuthorityError::NonFiniteSourcePosition)
        );
        assert_eq!(trace, before);

        let mut invalid = damage(1, 0, 10, 1);
        invalid.attack_id = "UNKNOWN ATTACK".to_owned();
        assert!(matches!(
            trace.record_tick([invalid]),
            Err(DeathAuthorityError::InvalidStableId(_))
        ));
        let mut invalid = damage(1, 0, 10, 1);
        invalid.statuses.push(invalid.statuses[0].clone());
        assert!(matches!(
            trace.record_tick([invalid]),
            Err(DeathAuthorityError::DuplicateTraceStatus(_))
        ));
        let mut invalid = damage(1, 0, 10, 1);
        invalid.statuses[0].stack_count = 0;
        assert_eq!(
            trace.record_tick([invalid]),
            Err(DeathAuthorityError::InvalidTraceStatus)
        );
        let mut invalid = damage(1, 0, 10, 1);
        invalid.source_position.x = f32::MAX;
        assert_eq!(
            trace.record_tick([invalid]),
            Err(DeathAuthorityError::SourcePositionOutOfRange)
        );
        let mut invalid = damage(1, 0, 10, 1);
        invalid.post_health = 8;
        assert_eq!(
            trace.record_tick([invalid]),
            Err(DeathAuthorityError::HealthArithmeticMismatch)
        );

        trace.record_tick([damage(2, 1, 10, 1)]).expect("accepted");
        let accepted = trace.clone();
        assert!(matches!(
            trace.record_tick([damage(1, 2, 9, 1)]),
            Err(DeathAuthorityError::TraceOrderRegression { .. })
        ));
        assert_eq!(trace, accepted);
    }

    #[test]
    fn duplicate_ordinals_and_nonfinal_lethal_batches_reject_transactionally() {
        let mut trace = DamageTraceAggregate::new();
        assert!(matches!(
            trace.record_tick([damage(10, 1, 20, 1), damage(10, 1, 19, 1)]),
            Err(DeathAuthorityError::DuplicateTraceOrdinal { .. })
        ));
        assert!(trace.entries().is_empty());
        assert_eq!(
            trace.record_tick([damage(10, 1, 1, 1), damage(10, 2, 20, 1)]),
            Err(DeathAuthorityError::InconsistentLethality)
        );
        assert!(trace.entries().is_empty());
    }

    #[test]
    fn trace_checkpoint_restart_preserves_terminal_hash_and_rejects_corruption() {
        let mut trace = DamageTraceAggregate::new();
        trace.record_tick([damage(50, 0, 25, 5)]).expect("first");
        trace.record_tick([damage(51, 0, 20, 20)]).expect("lethal");
        let checkpoint = trace.checkpoint();
        let restored = DamageTraceAggregate::from_checkpoint(checkpoint.clone()).expect("restore");
        assert_eq!(restored, trace);
        assert_eq!(
            restored
                .terminal_snapshot()
                .expect("restored terminal")
                .canonical_hash_blake3,
            trace
                .terminal_snapshot()
                .expect("terminal")
                .canonical_hash_blake3
        );

        let mut corrupt = checkpoint;
        corrupt.entries[0].lethal = true;
        assert_eq!(
            DamageTraceAggregate::from_checkpoint(corrupt),
            Err(DeathAuthorityError::InconsistentLethality)
        );

        let mut stale = trace.checkpoint();
        stale.entries[0].tick = Tick(1);
        stale.entries[1].tick = Tick(400);
        assert_eq!(
            DamageTraceAggregate::from_checkpoint(stale),
            Err(DeathAuthorityError::CorruptTraceCheckpoint)
        );
    }

    #[test]
    fn canonical_trace_hash_is_pinned_and_changes_with_authority() {
        let mut trace = DamageTraceAggregate::new();
        trace.record_tick([damage(100, 0, 30, 10)]).expect("first");
        trace.record_tick([damage(101, 0, 20, 20)]).expect("lethal");
        let digest = trace
            .terminal_snapshot()
            .expect("terminal")
            .canonical_hash_blake3;
        assert_eq!(
            blake3::Hash::from_bytes(digest).to_hex().as_str(),
            "fa8f932d1f7999edc957d1f2ddad67df94d244d4c86a181a6de24a91ccb030ed"
        );

        let checkpoint = trace.checkpoint();
        let mut changed = checkpoint.clone();
        changed.entries[0].network_state = DeathTraceNetworkState::Degraded;
        let changed = DamageTraceAggregate::from_checkpoint(changed).expect("changed authority");
        assert_ne!(
            changed
                .terminal_snapshot()
                .expect("changed terminal")
                .canonical_hash_blake3,
            digest
        );
    }

    #[test]
    fn complete_inputs_require_both_committed_death_and_terminal_trace() {
        let mut clocks = DeathClockAggregate::new();
        clocks.enter_danger().expect("entry");
        let deeds = DeedAggregate::new();
        let mut trace = DamageTraceAggregate::new();
        trace.record_tick([damage(1, 0, 1, 1)]).expect("lethal");
        assert_eq!(
            compile_authoritative_death_inputs(&clocks, &deeds, &trace),
            Err(DeathAuthorityError::CommittedDeathRequired)
        );
        clocks
            .resolve_danger(DangerTerminalOutcome::Death)
            .expect("death");
        let inputs =
            compile_authoritative_death_inputs(&clocks, &deeds, &trace).expect("complete inputs");
        assert_eq!(inputs.final_deed.deed_id, DEED_NONE_ID);
        assert_eq!(inputs.last_five.len(), 1);
        assert!(!inputs.echo_deed_eligible);
    }
}
