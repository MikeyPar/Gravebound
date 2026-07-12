//! Privacy-safe, deterministic `LocalLab` telemetry for `GB-M01-10B`.
//!
//! The First Playable has no account system. The common TEL-001 account field therefore carries a
//! fixed sentinel and never a tester identity. An opaque, locally assigned tester ID is kept in the
//! event envelope solely to join the consented blind-test records. Free-form source answers are not
//! accepted: researchers must enter a redacted summary through [`PrivacySafeSurveySummary`].

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const TELEMETRY_SCHEMA_VERSION: u16 = 1;
pub const LOCAL_ACCOUNT_SENTINEL: &str = "local_lab_no_account";
pub const LOCAL_REGION: &str = "local";
pub const LOCAL_ENVIRONMENT: &str = "local_lab";
const LOCAL_PLATFORM: &str = "windows_native";
const MAX_REDACTED_SUMMARY_BYTES: usize = 280;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalTelemetryContext {
    pub tester_id: String,
    pub session_id: String,
    pub build_id: String,
    pub content_bundle_version: String,
    pub cohort_eligibility: CohortEligibility,
    pub genre_familiarity: GenreFamiliarity,
    pub metric_eligibility: MetricEligibility,
}

impl LocalTelemetryContext {
    pub fn new(
        tester_id: impl Into<String>,
        session_id: impl Into<String>,
        build_id: impl Into<String>,
        content_bundle_version: impl Into<String>,
        cohort_eligibility: CohortEligibility,
        genre_familiarity: GenreFamiliarity,
        metric_eligibility: MetricEligibility,
    ) -> Result<Self, LocalTelemetryError> {
        let value = Self {
            tester_id: tester_id.into(),
            session_id: session_id.into(),
            build_id: build_id.into(),
            content_bundle_version: content_bundle_version.into(),
            cohort_eligibility,
            genre_familiarity,
            metric_eligibility,
        };
        validate_prefixed_hex_id("tester_id", &value.tester_id, "tester-")?;
        validate_prefixed_hex_id("session_id", &value.session_id, "session-")?;
        validate_opaque_id("build_id", &value.build_id)?;
        validate_content_id("content_bundle_version", &value.content_bundle_version)?;
        Ok(value)
    }

    #[must_use]
    pub fn gate_eligible(&self) -> bool {
        self.cohort_eligibility == CohortEligibility::EligibleBlind
            && self.metric_eligibility == MetricEligibility::Eligible
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CohortEligibility {
    EligibleBlind,
    ExcludedFeatureContributor,
    ExcludedIncompleteConsent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenreFamiliarity {
    NewToBoth,
    ActionRpgOnly,
    BulletHellOnly,
    ActionRpgAndBulletHell,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricEligibility {
    Eligible,
    ExcludedDeveloperTools,
    ExcludedNonStandardTimeScale,
    ExcludedDebugInvulnerability,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryEnvelope {
    pub event_id: String,
    pub event_name: String,
    pub event_schema_version: u16,
    /// UTC Unix epoch milliseconds. Tests provide this value explicitly, so exports are replayable.
    pub occurred_at_utc: i64,
    pub pseudonymous_account_id: String,
    pub local_tester_id: String,
    pub session_id: String,
    pub build_id: String,
    pub content_bundle_version: String,
    pub platform: String,
    pub region_id: String,
    pub environment: String,
    pub cohort_tags: Vec<String>,
    pub sequence: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryRecord {
    #[serde(flatten)]
    pub envelope: TelemetryEnvelope,
    #[serde(flatten)]
    pub event: TelemetryEvent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "event_payload_type",
    content = "event_payload",
    rename_all = "snake_case"
)]
pub enum TelemetryEvent {
    SessionStarted,
    SessionEnded,
    RunStarted {
        run_id: String,
    },
    BossStarted {
        run_id: String,
        boss_id: String,
    },
    BossPhaseChanged(BossPhaseTelemetry),
    BossDefeated {
        run_id: String,
        boss_id: String,
        clear_ticks: u32,
    },
    DamageReceived(DamageTelemetry),
    CharacterDied(DeathTelemetry),
    ItemLifecycle(ItemLifecycleTelemetry),
    RunRestarted(RestartTelemetry),
    ClientCrash {
        crash_code: String,
        run_id: Option<String>,
    },
    ObservationRecorded(ObservationTelemetry),
    KillerResponseRecorded(KillerResponseTelemetry),
    DeathTraceRevealed {
        death_id: String,
    },
    SurveyCompleted(SurveyTelemetry),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TelemetryEventKind {
    SessionStarted,
    SessionEnded,
    RunStarted,
    BossStarted,
    BossPhaseChanged,
    BossDefeated,
    DamageReceived,
    CharacterDied,
    ItemPickedUp,
    ItemEquipped,
    ItemDestroyed,
    RunRestarted,
    ClientCrash,
    ObservationRecorded,
    KillerResponseRecorded,
    DeathTraceRevealed,
    SurveyCompleted,
}

impl TelemetryEvent {
    #[must_use]
    pub const fn kind(&self) -> TelemetryEventKind {
        match self {
            Self::SessionStarted => TelemetryEventKind::SessionStarted,
            Self::SessionEnded => TelemetryEventKind::SessionEnded,
            Self::RunStarted { .. } => TelemetryEventKind::RunStarted,
            Self::BossStarted { .. } => TelemetryEventKind::BossStarted,
            Self::BossPhaseChanged(_) => TelemetryEventKind::BossPhaseChanged,
            Self::BossDefeated { .. } => TelemetryEventKind::BossDefeated,
            Self::DamageReceived(_) => TelemetryEventKind::DamageReceived,
            Self::CharacterDied(_) => TelemetryEventKind::CharacterDied,
            Self::ItemLifecycle(item) => match item.action {
                ItemLifecycleAction::PickedUp => TelemetryEventKind::ItemPickedUp,
                ItemLifecycleAction::Equipped => TelemetryEventKind::ItemEquipped,
                ItemLifecycleAction::Destroyed => TelemetryEventKind::ItemDestroyed,
            },
            Self::RunRestarted(_) => TelemetryEventKind::RunRestarted,
            Self::ClientCrash { .. } => TelemetryEventKind::ClientCrash,
            Self::ObservationRecorded(_) => TelemetryEventKind::ObservationRecorded,
            Self::KillerResponseRecorded(_) => TelemetryEventKind::KillerResponseRecorded,
            Self::DeathTraceRevealed { .. } => TelemetryEventKind::DeathTraceRevealed,
            Self::SurveyCompleted(_) => TelemetryEventKind::SurveyCompleted,
        }
    }

    fn event_name(&self) -> &'static str {
        match self.kind() {
            TelemetryEventKind::SessionStarted => "session_started",
            TelemetryEventKind::SessionEnded => "session_ended",
            TelemetryEventKind::RunStarted => "run_started",
            TelemetryEventKind::BossStarted => "boss_started",
            TelemetryEventKind::BossPhaseChanged => "boss_phase_changed",
            TelemetryEventKind::BossDefeated => "boss_defeated",
            TelemetryEventKind::DamageReceived => "damage_received",
            TelemetryEventKind::CharacterDied => "character_died",
            TelemetryEventKind::ItemPickedUp => "item_picked_up",
            TelemetryEventKind::ItemEquipped => "item_equipped",
            TelemetryEventKind::ItemDestroyed => "item_destroyed",
            TelemetryEventKind::RunRestarted => "run_restarted",
            TelemetryEventKind::ClientCrash => "client_crash",
            TelemetryEventKind::ObservationRecorded => "observation_recorded",
            TelemetryEventKind::KillerResponseRecorded => "killer_response_recorded",
            TelemetryEventKind::DeathTraceRevealed => "death_trace_revealed",
            TelemetryEventKind::SurveyCompleted => "survey_completed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BossPhaseTelemetry {
    pub run_id: String,
    pub boss_id: String,
    pub from_phase: u8,
    pub to_phase: u8,
    pub boss_health: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DamageTelemetry {
    pub run_id: String,
    pub source_id: String,
    pub pattern_id: String,
    pub damage_type: String,
    pub raw_damage: u32,
    pub final_damage: u32,
    pub pre_hit_health: u32,
    pub post_hit_health: u32,
    pub target_state: String,
    pub simulation_tick: u64,
    pub latency_ms: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeathTelemetry {
    pub run_id: String,
    pub death_id: String,
    pub class_id: String,
    pub level: u8,
    pub oath_id: Option<String>,
    pub active_bargain_ids: Vec<String>,
    pub lifetime_ticks: u64,
    pub session_duration_ticks: u64,
    pub killer_id: String,
    pub pattern_id: String,
    pub damage_type: String,
    pub raw_damage: u32,
    pub final_damage: u32,
    pub pre_hit_health: u32,
    pub status_ids: Vec<String>,
    pub room_id: String,
    pub boss_phase: Option<u8>,
    pub party_size: u8,
    pub contribution_basis_points: u16,
    pub item_power_band: String,
    pub ping_ms: u16,
    pub jitter_ms: u16,
    pub loss_basis_points: u16,
    pub correction_count: u16,
    pub recall_state: String,
    pub cause: DeathCauseTelemetry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeathCauseTelemetry {
    DirectHit,
    DamageOverTime,
    Environment,
    Disconnect,
    ServerFault,
    AdministrativeRestore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemLifecycleAction {
    PickedUp,
    Equipped,
    Destroyed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemLifecycleTelemetry {
    pub run_id: String,
    pub item_instance_id: String,
    pub item_content_id: String,
    pub action: ItemLifecycleAction,
    pub reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestartReasonTelemetry {
    Death,
    BossVictory,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestartTelemetry {
    pub previous_run_id: String,
    pub new_run_id: String,
    pub reason: RestartReasonTelemetry,
    pub death_id: Option<String>,
    pub elapsed_ticks: u32,
    pub voluntarily_activated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationMoment {
    FirstConfusion,
    FirstDamage,
    FirstItem,
    FirstDeath,
    FirstRestart,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationTelemetry {
    pub moment: ObservationMoment,
    pub run_id: String,
    pub simulation_tick: u64,
    pub researcher_summary: PrivacySafeSurveySummary,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KillerResponseTelemetry {
    pub death_id: String,
    pub selected_killer_id: String,
    pub selected_pattern_id: String,
    pub matched_authoritative_cause: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rating(u8);

impl Rating {
    pub fn new(value: u8) -> Result<Self, LocalTelemetryError> {
        if (1..=5).contains(&value) {
            Ok(Self(value))
        } else {
            Err(LocalTelemetryError::RatingOutOfRange(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenQuestion {
    WhatFeltDistinctive,
    WhatWouldMakeYouStop,
    WhatDoYouWantToDoNext,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenSurveyAnswer {
    pub question: OpenQuestion,
    pub redacted_summary: PrivacySafeSurveySummary,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurveyTelemetry {
    pub movement: Rating,
    pub shooting: Rating,
    pub dodging: Rating,
    pub overall_combat_feel: Rating,
    pub wants_another_attempt: bool,
    pub answers: Vec<OpenSurveyAnswer>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PrivacySafeSurveySummary(String);

impl PrivacySafeSurveySummary {
    /// Accepts a researcher-authored summary only after obvious direct identifiers were removed.
    /// Raw interview transcription must never be passed to this API.
    pub fn from_redacted_summary(value: impl Into<String>) -> Result<Self, LocalTelemetryError> {
        let value = value.into();
        let trimmed = value.trim();
        if trimmed.is_empty() || trimmed.len() > MAX_REDACTED_SUMMARY_BYTES {
            return Err(LocalTelemetryError::InvalidRedactedSummaryLength(
                trimmed.len(),
            ));
        }
        if trimmed.chars().any(char::is_control) || contains_direct_identifier_marker(trimmed) {
            return Err(LocalTelemetryError::PotentialPersonalIdentifier);
        }
        Ok(Self(trimmed.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalTelemetryLog {
    context: LocalTelemetryContext,
    records: Vec<TelemetryRecord>,
    active_run_id: Option<String>,
    known_run_ids: BTreeSet<String>,
    known_death_ids: BTreeSet<String>,
    boss_started_runs: BTreeSet<String>,
    boss_defeated_runs: BTreeSet<String>,
    observed_moments: BTreeSet<ObservationMoment>,
    killer_response_deaths: BTreeSet<String>,
    trace_revealed_deaths: BTreeSet<String>,
    session_started: bool,
    session_ended: bool,
}

impl LocalTelemetryLog {
    #[must_use]
    pub fn new(context: LocalTelemetryContext) -> Self {
        Self {
            context,
            records: Vec::new(),
            active_run_id: None,
            known_run_ids: BTreeSet::new(),
            known_death_ids: BTreeSet::new(),
            boss_started_runs: BTreeSet::new(),
            boss_defeated_runs: BTreeSet::new(),
            observed_moments: BTreeSet::new(),
            killer_response_deaths: BTreeSet::new(),
            trace_revealed_deaths: BTreeSet::new(),
            session_started: false,
            session_ended: false,
        }
    }

    pub fn record(
        &mut self,
        occurred_at_utc: i64,
        event: TelemetryEvent,
    ) -> Result<&TelemetryRecord, LocalTelemetryError> {
        if occurred_at_utc < 0 {
            return Err(LocalTelemetryError::InvalidUtcTimestamp(occurred_at_utc));
        }
        if let Some(previous) = self.records.last()
            && occurred_at_utc < previous.envelope.occurred_at_utc
        {
            return Err(LocalTelemetryError::TimestampRegressed);
        }
        self.validate_and_apply(&event)?;
        let sequence = u32::try_from(self.records.len() + 1)
            .map_err(|_| LocalTelemetryError::SequenceExhausted)?;
        let event_name = event.event_name();
        let record = TelemetryRecord {
            envelope: TelemetryEnvelope {
                event_id: format!("{}:{sequence:08}", self.context.session_id),
                event_name: event_name.to_owned(),
                event_schema_version: TELEMETRY_SCHEMA_VERSION,
                occurred_at_utc,
                pseudonymous_account_id: LOCAL_ACCOUNT_SENTINEL.to_owned(),
                local_tester_id: self.context.tester_id.clone(),
                session_id: self.context.session_id.clone(),
                build_id: self.context.build_id.clone(),
                content_bundle_version: self.context.content_bundle_version.clone(),
                platform: LOCAL_PLATFORM.to_owned(),
                region_id: LOCAL_REGION.to_owned(),
                environment: LOCAL_ENVIRONMENT.to_owned(),
                cohort_tags: cohort_tags(&self.context),
                sequence,
            },
            event,
        };
        self.records.push(record);
        Ok(self.records.last().expect("record was just pushed"))
    }

    #[must_use]
    pub fn records(&self) -> &[TelemetryRecord] {
        &self.records
    }

    pub fn export_json_lines(&self) -> Result<String, LocalTelemetryError> {
        let mut output = String::new();
        for record in &self.records {
            output.push_str(&serde_json::to_string(record)?);
            output.push('\n');
        }
        Ok(output)
    }

    fn validate_and_apply(&mut self, event: &TelemetryEvent) -> Result<(), LocalTelemetryError> {
        if self.session_ended {
            return Err(LocalTelemetryError::SessionAlreadyEnded);
        }
        if !self.session_started && !matches!(event, TelemetryEvent::SessionStarted) {
            return Err(LocalTelemetryError::SessionMustStartFirst);
        }
        match event {
            TelemetryEvent::SessionStarted => {
                if self.session_started {
                    return Err(LocalTelemetryError::DuplicateSessionStart);
                }
                self.session_started = true;
            }
            TelemetryEvent::SessionEnded => {
                self.session_ended = true;
                self.active_run_id = None;
            }
            TelemetryEvent::RunStarted { run_id } => self.start_run(run_id)?,
            TelemetryEvent::BossStarted { run_id, boss_id } => {
                self.require_active_run(run_id)?;
                validate_content_id("boss_id", boss_id)?;
                if !self.boss_started_runs.insert(run_id.clone()) {
                    return Err(LocalTelemetryError::DuplicateBossStart(run_id.clone()));
                }
            }
            TelemetryEvent::BossPhaseChanged(value) => {
                self.require_active_boss(&value.run_id)?;
                validate_content_id("boss_id", &value.boss_id)?;
                if value.from_phase == value.to_phase || !(1..=3).contains(&value.to_phase) {
                    return Err(LocalTelemetryError::InvalidBossPhase);
                }
            }
            TelemetryEvent::BossDefeated {
                run_id, boss_id, ..
            } => {
                self.require_active_boss(run_id)?;
                validate_content_id("boss_id", boss_id)?;
                if !self.boss_defeated_runs.insert(run_id.clone()) {
                    return Err(LocalTelemetryError::DuplicateBossDefeat(run_id.clone()));
                }
            }
            TelemetryEvent::DamageReceived(value) => {
                self.require_active_run(&value.run_id)?;
                validate_damage(value)?;
            }
            TelemetryEvent::CharacterDied(value) => {
                self.require_active_run(&value.run_id)?;
                validate_death(value)?;
                if value.cause == DeathCauseTelemetry::ServerFault {
                    return Err(LocalTelemetryError::FinalServerFaultDeath);
                }
                if !self.known_death_ids.insert(value.death_id.clone()) {
                    return Err(LocalTelemetryError::DuplicateDeath(value.death_id.clone()));
                }
            }
            TelemetryEvent::ItemLifecycle(value) => {
                self.require_active_run(&value.run_id)?;
                validate_opaque_id("item_instance_id", &value.item_instance_id)?;
                validate_content_id("item_content_id", &value.item_content_id)?;
                if let Some(reason) = &value.reason {
                    validate_opaque_id("item_lifecycle_reason", reason)?;
                }
            }
            TelemetryEvent::RunRestarted(value) => self.apply_restart(value)?,
            TelemetryEvent::ClientCrash { crash_code, run_id } => {
                validate_opaque_id("crash_code", crash_code)?;
                if let Some(run_id) = run_id {
                    self.require_known_run(run_id)?;
                }
            }
            TelemetryEvent::ObservationRecorded(value) => {
                self.require_known_run(&value.run_id)?;
                if !self.observed_moments.insert(value.moment) {
                    return Err(LocalTelemetryError::DuplicateObservation(value.moment));
                }
            }
            TelemetryEvent::KillerResponseRecorded(value) => {
                self.require_known_death(&value.death_id)?;
                validate_content_id("selected_killer_id", &value.selected_killer_id)?;
                validate_content_id("selected_pattern_id", &value.selected_pattern_id)?;
                if self.trace_revealed_deaths.contains(&value.death_id) {
                    return Err(LocalTelemetryError::KillerResponseAfterTrace);
                }
                if !self.killer_response_deaths.insert(value.death_id.clone()) {
                    return Err(LocalTelemetryError::DuplicateKillerResponse(
                        value.death_id.clone(),
                    ));
                }
            }
            TelemetryEvent::DeathTraceRevealed { death_id } => {
                self.require_known_death(death_id)?;
                if !self.killer_response_deaths.contains(death_id) {
                    return Err(LocalTelemetryError::TraceBeforeKillerResponse);
                }
                self.trace_revealed_deaths.insert(death_id.clone());
            }
            TelemetryEvent::SurveyCompleted(value) => validate_survey(value)?,
        }
        Ok(())
    }

    fn start_run(&mut self, run_id: &str) -> Result<(), LocalTelemetryError> {
        validate_opaque_id("run_id", run_id)?;
        if self.active_run_id.is_some() {
            return Err(LocalTelemetryError::ActiveRunExists);
        }
        if !self.known_run_ids.insert(run_id.to_owned()) {
            return Err(LocalTelemetryError::DuplicateRun(run_id.to_owned()));
        }
        self.active_run_id = Some(run_id.to_owned());
        Ok(())
    }

    fn apply_restart(&mut self, value: &RestartTelemetry) -> Result<(), LocalTelemetryError> {
        self.require_active_run(&value.previous_run_id)?;
        validate_opaque_id("new_run_id", &value.new_run_id)?;
        if value.previous_run_id == value.new_run_id
            || self.known_run_ids.contains(&value.new_run_id)
        {
            return Err(LocalTelemetryError::InvalidRestartCorrelation);
        }
        match value.reason {
            RestartReasonTelemetry::Death => {
                let death_id = value
                    .death_id
                    .as_deref()
                    .ok_or(LocalTelemetryError::RestartDeathMissing)?;
                self.require_known_death(death_id)?;
            }
            RestartReasonTelemetry::BossVictory => {
                if value.death_id.is_some()
                    || !self.boss_defeated_runs.contains(&value.previous_run_id)
                {
                    return Err(LocalTelemetryError::InvalidVictoryRestart);
                }
            }
        }
        self.known_run_ids.insert(value.new_run_id.clone());
        self.active_run_id = Some(value.new_run_id.clone());
        Ok(())
    }

    fn require_active_run(&self, run_id: &str) -> Result<(), LocalTelemetryError> {
        if self.active_run_id.as_deref() == Some(run_id) {
            Ok(())
        } else {
            Err(LocalTelemetryError::RunNotActive(run_id.to_owned()))
        }
    }

    fn require_known_run(&self, run_id: &str) -> Result<(), LocalTelemetryError> {
        if self.known_run_ids.contains(run_id) {
            Ok(())
        } else {
            Err(LocalTelemetryError::UnknownRun(run_id.to_owned()))
        }
    }

    fn require_active_boss(&self, run_id: &str) -> Result<(), LocalTelemetryError> {
        self.require_active_run(run_id)?;
        if self.boss_started_runs.contains(run_id) {
            Ok(())
        } else {
            Err(LocalTelemetryError::BossNotStarted(run_id.to_owned()))
        }
    }

    fn require_known_death(&self, death_id: &str) -> Result<(), LocalTelemetryError> {
        if self.known_death_ids.contains(death_id) {
            Ok(())
        } else {
            Err(LocalTelemetryError::UnknownDeath(death_id.to_owned()))
        }
    }
}

fn validate_damage(value: &DamageTelemetry) -> Result<(), LocalTelemetryError> {
    validate_content_id("source_id", &value.source_id)?;
    validate_content_id("pattern_id", &value.pattern_id)?;
    validate_content_id("damage_type", &value.damage_type)?;
    validate_opaque_id("target_state", &value.target_state)?;
    if value.final_damage > value.raw_damage || value.post_hit_health > value.pre_hit_health {
        return Err(LocalTelemetryError::InvalidDamagePayload);
    }
    Ok(())
}

fn validate_death(value: &DeathTelemetry) -> Result<(), LocalTelemetryError> {
    validate_opaque_id("death_id", &value.death_id)?;
    validate_content_id("class_id", &value.class_id)?;
    validate_content_id("killer_id", &value.killer_id)?;
    validate_content_id("pattern_id", &value.pattern_id)?;
    validate_content_id("damage_type", &value.damage_type)?;
    validate_content_id("room_id", &value.room_id)?;
    validate_opaque_id("item_power_band", &value.item_power_band)?;
    validate_opaque_id("recall_state", &value.recall_state)?;
    if let Some(oath_id) = &value.oath_id {
        validate_content_id("oath_id", oath_id)?;
    }
    for bargain_id in &value.active_bargain_ids {
        validate_content_id("active_bargain_id", bargain_id)?;
    }
    for status_id in &value.status_ids {
        validate_content_id("status_id", status_id)?;
    }
    if value.level == 0
        || value.party_size == 0
        || value.contribution_basis_points > 10_000
        || value.loss_basis_points > 10_000
        || value.final_damage > value.raw_damage
    {
        return Err(LocalTelemetryError::InvalidDeathPayload);
    }
    Ok(())
}

fn validate_survey(value: &SurveyTelemetry) -> Result<(), LocalTelemetryError> {
    let questions: BTreeSet<_> = value
        .answers
        .iter()
        .map(|answer| answer.question as u8)
        .collect();
    if value.answers.len() != 3 || questions.len() != 3 {
        return Err(LocalTelemetryError::IncompleteOpenSurvey);
    }
    Ok(())
}

fn validate_opaque_id(field: &'static str, value: &str) -> Result<(), LocalTelemetryError> {
    if !(3..=80).contains(&value.len())
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_:".contains(&byte)
        })
    {
        return Err(LocalTelemetryError::InvalidIdentifier(field));
    }
    Ok(())
}

fn validate_prefixed_hex_id(
    field: &'static str,
    value: &str,
    prefix: &str,
) -> Result<(), LocalTelemetryError> {
    let Some(suffix) = value.strip_prefix(prefix) else {
        return Err(LocalTelemetryError::InvalidIdentifier(field));
    };
    if suffix.len() != 16
        || !suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(LocalTelemetryError::InvalidIdentifier(field));
    }
    Ok(())
}

fn validate_content_id(field: &'static str, value: &str) -> Result<(), LocalTelemetryError> {
    if !(2..=128).contains(&value.len())
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_.:".contains(&byte)
        })
    {
        return Err(LocalTelemetryError::InvalidIdentifier(field));
    }
    Ok(())
}

fn contains_direct_identifier_marker(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "@", "http://", "https://", "email:", "name:", "phone:", "discord:", "steam:",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
        || value
            .split(|character: char| !character.is_ascii_digit())
            .any(|digits| digits.len() >= 7)
}

fn cohort_tags(context: &LocalTelemetryContext) -> Vec<String> {
    vec![
        match context.cohort_eligibility {
            CohortEligibility::EligibleBlind => "cohort:eligible_blind",
            CohortEligibility::ExcludedFeatureContributor => "cohort:excluded_feature_contributor",
            CohortEligibility::ExcludedIncompleteConsent => "cohort:excluded_incomplete_consent",
        }
        .to_owned(),
        match context.genre_familiarity {
            GenreFamiliarity::NewToBoth => "genre:new_to_both",
            GenreFamiliarity::ActionRpgOnly => "genre:action_rpg_only",
            GenreFamiliarity::BulletHellOnly => "genre:bullet_hell_only",
            GenreFamiliarity::ActionRpgAndBulletHell => "genre:action_rpg_and_bullet_hell",
        }
        .to_owned(),
        match context.metric_eligibility {
            MetricEligibility::Eligible => "metrics:eligible",
            MetricEligibility::ExcludedDeveloperTools => "metrics:excluded_developer_tools",
            MetricEligibility::ExcludedNonStandardTimeScale => {
                "metrics:excluded_non_standard_time_scale"
            }
            MetricEligibility::ExcludedDebugInvulnerability => {
                "metrics:excluded_debug_invulnerability"
            }
        }
        .to_owned(),
    ]
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LocalTelemetryError {
    #[error("invalid telemetry identifier in {0}")]
    InvalidIdentifier(&'static str),
    #[error("UTC timestamp {0} is invalid")]
    InvalidUtcTimestamp(i64),
    #[error("telemetry timestamps must not regress")]
    TimestampRegressed,
    #[error("session_started must be the first event")]
    SessionMustStartFirst,
    #[error("session has already started")]
    DuplicateSessionStart,
    #[error("session has already ended")]
    SessionAlreadyEnded,
    #[error("an active run already exists")]
    ActiveRunExists,
    #[error("run {0} has already been used")]
    DuplicateRun(String),
    #[error("run {0} is not active")]
    RunNotActive(String),
    #[error("run {0} is unknown")]
    UnknownRun(String),
    #[error("boss has already started in run {0}")]
    DuplicateBossStart(String),
    #[error("boss has not started in run {0}")]
    BossNotStarted(String),
    #[error("boss phase payload is invalid")]
    InvalidBossPhase,
    #[error("boss has already been defeated in run {0}")]
    DuplicateBossDefeat(String),
    #[error("damage payload is internally inconsistent")]
    InvalidDamagePayload,
    #[error("death payload is internally inconsistent")]
    InvalidDeathPayload,
    #[error("server_fault cannot remain a final death result")]
    FinalServerFaultDeath,
    #[error("death {0} has already been recorded")]
    DuplicateDeath(String),
    #[error("death {0} is unknown")]
    UnknownDeath(String),
    #[error("restart after death must reference a death")]
    RestartDeathMissing,
    #[error("restart run correlation is invalid")]
    InvalidRestartCorrelation,
    #[error("victory restart requires a boss defeat and no death reference")]
    InvalidVictoryRestart,
    #[error("observation {0:?} has already been recorded")]
    DuplicateObservation(ObservationMoment),
    #[error("killer response must be recorded before revealing the trace")]
    KillerResponseAfterTrace,
    #[error("killer response for death {0} has already been recorded")]
    DuplicateKillerResponse(String),
    #[error("the detailed death trace cannot be revealed before the killer response")]
    TraceBeforeKillerResponse,
    #[error("rating {0} must be between 1 and 5")]
    RatingOutOfRange(u8),
    #[error("all three open survey prompts require one answer")]
    IncompleteOpenSurvey,
    #[error("redacted summary length {0} is outside 1..={MAX_REDACTED_SUMMARY_BYTES}")]
    InvalidRedactedSummaryLength(usize),
    #[error("summary contains a potential direct personal identifier")]
    PotentialPersonalIdentifier,
    #[error("telemetry event sequence exhausted")]
    SequenceExhausted,
    #[error("telemetry JSON export failed: {0}")]
    Json(String),
}

impl From<serde_json::Error> for LocalTelemetryError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_784_000_000_000;

    fn context() -> LocalTelemetryContext {
        LocalTelemetryContext::new(
            "tester-0000000000000001",
            "session-0000000000000001",
            "build-m01-fixture",
            "fp.1.0.0",
            CohortEligibility::EligibleBlind,
            GenreFamiliarity::ActionRpgAndBulletHell,
            MetricEligibility::Eligible,
        )
        .unwrap()
    }

    fn death(run_id: &str) -> DeathTelemetry {
        DeathTelemetry {
            run_id: run_id.to_owned(),
            death_id: "death-0001".to_owned(),
            class_id: "class.grave_arbalist".to_owned(),
            level: 1,
            oath_id: None,
            active_bargain_ids: Vec::new(),
            lifetime_ticks: 900,
            session_duration_ticks: 900,
            killer_id: "enemy.bell_reed".to_owned(),
            pattern_id: "pattern.prototype.bell_reed.gap_ring".to_owned(),
            damage_type: "veil".to_owned(),
            raw_damage: 10,
            final_damage: 8,
            pre_hit_health: 8,
            status_ids: Vec::new(),
            room_id: "arena.prototype.bell_laboratory_01".to_owned(),
            boss_phase: None,
            party_size: 1,
            contribution_basis_points: 10_000,
            item_power_band: "prototype".to_owned(),
            ping_ms: 0,
            jitter_ms: 0,
            loss_basis_points: 0,
            correction_count: 0,
            recall_state: "unavailable_local_lab".to_owned(),
            cause: DeathCauseTelemetry::DirectHit,
        }
    }

    fn summary(value: &str) -> PrivacySafeSurveySummary {
        PrivacySafeSurveySummary::from_redacted_summary(value).unwrap()
    }

    #[test]
    fn envelope_uses_local_sentinel_and_never_substitutes_tester_identity() {
        let mut log = LocalTelemetryLog::new(context());
        let record = log.record(T0, TelemetryEvent::SessionStarted).unwrap();
        assert_eq!(
            record.envelope.pseudonymous_account_id,
            LOCAL_ACCOUNT_SENTINEL
        );
        assert_eq!(record.envelope.local_tester_id, "tester-0000000000000001");
        assert_eq!(record.envelope.region_id, LOCAL_REGION);
        assert_eq!(record.envelope.environment, LOCAL_ENVIRONMENT);
        assert_ne!(
            record.envelope.pseudonymous_account_id,
            record.envelope.local_tester_id
        );
        assert!(context().gate_eligible());
    }

    #[test]
    fn ordering_requires_session_run_boss_and_killer_response_before_trace() {
        let mut log = LocalTelemetryLog::new(context());
        assert_eq!(
            log.record(
                T0,
                TelemetryEvent::RunStarted {
                    run_id: "run-1".to_owned()
                }
            ),
            Err(LocalTelemetryError::SessionMustStartFirst)
        );
        log.record(T0, TelemetryEvent::SessionStarted).unwrap();
        log.record(
            T0 + 1,
            TelemetryEvent::RunStarted {
                run_id: "run-1".to_owned(),
            },
        )
        .unwrap();
        assert_eq!(
            log.record(
                T0 + 2,
                TelemetryEvent::BossPhaseChanged(BossPhaseTelemetry {
                    run_id: "run-1".to_owned(),
                    boss_id: "boss.prototype.bell_proctor".to_owned(),
                    from_phase: 1,
                    to_phase: 2,
                    boss_health: 2_100,
                })
            ),
            Err(LocalTelemetryError::BossNotStarted("run-1".to_owned()))
        );
        log.record(T0 + 2, TelemetryEvent::CharacterDied(death("run-1")))
            .unwrap();
        assert_eq!(
            log.record(
                T0 + 3,
                TelemetryEvent::DeathTraceRevealed {
                    death_id: "death-0001".to_owned()
                }
            ),
            Err(LocalTelemetryError::TraceBeforeKillerResponse)
        );
        log.record(
            T0 + 3,
            TelemetryEvent::KillerResponseRecorded(KillerResponseTelemetry {
                death_id: "death-0001".to_owned(),
                selected_killer_id: "enemy.bell_reed".to_owned(),
                selected_pattern_id: "pattern.prototype.bell_reed.gap_ring".to_owned(),
                matched_authoritative_cause: true,
            }),
        )
        .unwrap();
        log.record(
            T0 + 4,
            TelemetryEvent::DeathTraceRevealed {
                death_id: "death-0001".to_owned(),
            },
        )
        .unwrap();
    }

    #[test]
    fn restart_atomically_correlates_previous_new_run_and_death() {
        let mut log = LocalTelemetryLog::new(context());
        log.record(T0, TelemetryEvent::SessionStarted).unwrap();
        log.record(
            T0 + 1,
            TelemetryEvent::RunStarted {
                run_id: "run-1".to_owned(),
            },
        )
        .unwrap();
        log.record(T0 + 2, TelemetryEvent::CharacterDied(death("run-1")))
            .unwrap();
        log.record(
            T0 + 3,
            TelemetryEvent::RunRestarted(RestartTelemetry {
                previous_run_id: "run-1".to_owned(),
                new_run_id: "run-2".to_owned(),
                reason: RestartReasonTelemetry::Death,
                death_id: Some("death-0001".to_owned()),
                elapsed_ticks: 30,
                voluntarily_activated: true,
            }),
        )
        .unwrap();
        log.record(
            T0 + 4,
            TelemetryEvent::DamageReceived(DamageTelemetry {
                run_id: "run-2".to_owned(),
                source_id: "enemy.drowned_pilgrim".to_owned(),
                pattern_id: "pattern.prototype.drowned_pilgrim.aimed_fan".to_owned(),
                damage_type: "physical".to_owned(),
                raw_damage: 8,
                final_damage: 6,
                pre_hit_health: 128,
                post_hit_health: 122,
                target_state: "active".to_owned(),
                simulation_tick: 1,
                latency_ms: 0,
            }),
        )
        .unwrap();
        assert_eq!(log.records()[3].envelope.event_name, "run_restarted");
    }

    #[test]
    fn survey_keeps_observation_and_opinion_separate_and_complete() {
        let mut log = LocalTelemetryLog::new(context());
        log.record(T0, TelemetryEvent::SessionStarted).unwrap();
        log.record(
            T0 + 1,
            TelemetryEvent::RunStarted {
                run_id: "run-1".to_owned(),
            },
        )
        .unwrap();
        log.record(
            T0 + 2,
            TelemetryEvent::ObservationRecorded(ObservationTelemetry {
                moment: ObservationMoment::FirstConfusion,
                run_id: "run-1".to_owned(),
                simulation_tick: 14,
                researcher_summary: summary("Paused at the first reward choice."),
            }),
        )
        .unwrap();
        log.record(
            T0 + 3,
            TelemetryEvent::SurveyCompleted(SurveyTelemetry {
                movement: Rating::new(4).unwrap(),
                shooting: Rating::new(5).unwrap(),
                dodging: Rating::new(4).unwrap(),
                overall_combat_feel: Rating::new(4).unwrap(),
                wants_another_attempt: true,
                answers: vec![
                    OpenSurveyAnswer {
                        question: OpenQuestion::WhatFeltDistinctive,
                        redacted_summary: summary("Readable ring gaps."),
                    },
                    OpenSurveyAnswer {
                        question: OpenQuestion::WhatWouldMakeYouStop,
                        redacted_summary: summary("Unclear damage sources."),
                    },
                    OpenSurveyAnswer {
                        question: OpenQuestion::WhatDoYouWantToDoNext,
                        redacted_summary: summary("Try another weapon."),
                    },
                ],
            }),
        )
        .unwrap();
        assert_eq!(log.records()[2].envelope.event_name, "observation_recorded");
        assert_eq!(log.records()[3].envelope.event_name, "survey_completed");
    }

    #[test]
    fn privacy_boundary_rejects_raw_identifier_markers_and_invalid_ids() {
        assert_eq!(
            PrivacySafeSurveySummary::from_redacted_summary("email: person@example.test"),
            Err(LocalTelemetryError::PotentialPersonalIdentifier)
        );
        assert_eq!(
            PrivacySafeSurveySummary::from_redacted_summary("Call 5551234567"),
            Err(LocalTelemetryError::PotentialPersonalIdentifier)
        );
        assert!(
            LocalTelemetryContext::new(
                "A Real Name",
                "session-1",
                "build-1",
                "fp.1.0.0",
                CohortEligibility::EligibleBlind,
                GenreFamiliarity::NewToBoth,
                MetricEligibility::Eligible,
            )
            .is_err()
        );
        assert_eq!(
            Rating::new(0),
            Err(LocalTelemetryError::RatingOutOfRange(0))
        );
        assert_eq!(
            Rating::new(6),
            Err(LocalTelemetryError::RatingOutOfRange(6))
        );
    }

    #[test]
    fn required_envelope_fields_and_item_event_names_are_schema_stable() {
        let mut log = LocalTelemetryLog::new(context());
        log.record(T0, TelemetryEvent::SessionStarted).unwrap();
        log.record(
            T0 + 1,
            TelemetryEvent::RunStarted {
                run_id: "run-1".to_owned(),
            },
        )
        .unwrap();
        for (index, action) in [
            ItemLifecycleAction::PickedUp,
            ItemLifecycleAction::Equipped,
            ItemLifecycleAction::Destroyed,
        ]
        .into_iter()
        .enumerate()
        {
            log.record(
                T0 + 2 + i64::try_from(index).unwrap(),
                TelemetryEvent::ItemLifecycle(ItemLifecycleTelemetry {
                    run_id: "run-1".to_owned(),
                    item_instance_id: "item-1".to_owned(),
                    item_content_id: "item.prototype.weapon.pine_crossbow".to_owned(),
                    action,
                    reason: (action == ItemLifecycleAction::Destroyed)
                        .then(|| "run-restart".to_owned()),
                }),
            )
            .unwrap();
        }
        let names: Vec<_> = log.records()[2..]
            .iter()
            .map(|record| record.envelope.event_name.as_str())
            .collect();
        assert_eq!(names, ["item_picked_up", "item_equipped", "item_destroyed"]);

        let value = serde_json::to_value(&log.records()[0]).unwrap();
        for field in [
            "event_id",
            "event_name",
            "event_schema_version",
            "occurred_at_utc",
            "pseudonymous_account_id",
            "local_tester_id",
            "session_id",
            "build_id",
            "content_bundle_version",
            "platform",
            "region_id",
            "environment",
            "cohort_tags",
            "sequence",
        ] {
            assert!(value.get(field).is_some(), "missing required field {field}");
        }
    }

    #[test]
    fn deterministic_export_fixture_is_byte_identical_and_contains_required_events() {
        fn fixture() -> String {
            let mut log = LocalTelemetryLog::new(context());
            log.record(T0, TelemetryEvent::SessionStarted).unwrap();
            log.record(
                T0 + 1,
                TelemetryEvent::RunStarted {
                    run_id: "run-1".to_owned(),
                },
            )
            .unwrap();
            log.record(
                T0 + 2,
                TelemetryEvent::BossStarted {
                    run_id: "run-1".to_owned(),
                    boss_id: "boss.prototype.bell_proctor".to_owned(),
                },
            )
            .unwrap();
            log.record(
                T0 + 3,
                TelemetryEvent::BossPhaseChanged(BossPhaseTelemetry {
                    run_id: "run-1".to_owned(),
                    boss_id: "boss.prototype.bell_proctor".to_owned(),
                    from_phase: 1,
                    to_phase: 2,
                    boss_health: 2_100,
                }),
            )
            .unwrap();
            log.record(
                T0 + 4,
                TelemetryEvent::BossDefeated {
                    run_id: "run-1".to_owned(),
                    boss_id: "boss.prototype.bell_proctor".to_owned(),
                    clear_ticks: 2_700,
                },
            )
            .unwrap();
            log.record(
                T0 + 5,
                TelemetryEvent::ItemLifecycle(ItemLifecycleTelemetry {
                    run_id: "run-1".to_owned(),
                    item_instance_id: "item-1".to_owned(),
                    item_content_id: "item.prototype.weapon.pine_crossbow".to_owned(),
                    action: ItemLifecycleAction::PickedUp,
                    reason: None,
                }),
            )
            .unwrap();
            log.record(
                T0 + 6,
                TelemetryEvent::ClientCrash {
                    crash_code: "none-fixture".to_owned(),
                    run_id: Some("run-1".to_owned()),
                },
            )
            .unwrap();
            log.export_json_lines().unwrap()
        }
        let first = fixture();
        let second = fixture();
        assert_eq!(first, second);
        assert_eq!(
            blake3::hash(first.as_bytes()).to_hex().as_str(),
            "9688cab59dd8cf880473932d34af352dd9bc70e52f55858f3422646a42c0f961"
        );
        for name in [
            "session_started",
            "run_started",
            "boss_started",
            "boss_phase_changed",
            "boss_defeated",
            "item_picked_up",
            "client_crash",
        ] {
            assert!(first.contains(name), "missing {name}");
        }
        for forbidden in ["email", "platform_id", "ip_address", "real_name"] {
            assert!(!first.contains(forbidden));
        }
    }
}
