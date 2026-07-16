//! Reusable `GB-M03-06E` timing, cleanup, and memory evidence primitives.
//!
//! Authorities:
//! - `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-001`, `DTH-021`, `TECH-022`,
//!   and `TECH-023`;
//! - `Gravebound_Content_Production_Spec_v1.md`: `CONT-ECHO-009` and `CONT-HUB-002`;
//! - `Gravebound_Development_Roadmap_v1.md`: `GB-M03-06`, `GB-M03-13`, and the M03 exit gate.
//!
//! This module performs evidence arithmetic and bounded inspection only. It does not create a
//! death, infer an Echo outcome, or become an alternate gameplay writer.

use std::{
    collections::BTreeSet,
    fs,
    io::Write as _,
    path::{Path, PathBuf},
    time::Duration,
};

use persistence::PostgresPersistence;
use serde::Serialize;
use sqlx::Row;
use sysinfo::{Pid, ProcessesToUpdate, System, get_current_pid};
use thiserror::Error;

const REQUIRED_MEMORY_DURATION_MS: u64 = 30 * 60 * 1_000;
const SUPPLEMENTAL_COMBINED_PROCESS_MEMORY_CEILING_BYTES: u64 = 1_500_000_000;
pub const DEATH_SOAK_RECONNECT_INTERVAL_JOURNEYS: u64 = 20;
pub const DEATH_VIEW_QUERIES_PER_SOAK_JOURNEY: u64 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DurationStatsV1 {
    pub sample_count: u32,
    pub median_micros: u64,
    pub p95_micros: u64,
    pub maximum_micros: u64,
}

impl DurationStatsV1 {
    pub fn compile(samples: &[Duration]) -> Result<Self, DeathMeasurementError> {
        if samples.is_empty() {
            return Err(DeathMeasurementError::MissingDurationSamples);
        }
        let mut micros = samples
            .iter()
            .map(|sample| {
                u64::try_from(sample.as_micros())
                    .map_err(|_| DeathMeasurementError::DurationOverflow)
            })
            .collect::<Result<Vec<_>, _>>()?;
        micros.sort_unstable();
        let sample_count =
            u32::try_from(micros.len()).map_err(|_| DeathMeasurementError::SampleCountOverflow)?;
        Ok(Self {
            sample_count,
            median_micros: micros[micros.len() / 2],
            p95_micros: nearest_rank(&micros, 95),
            maximum_micros: micros[micros.len() - 1],
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DeathLatencySampleV1 {
    pub terminal_commit: Duration,
    pub exact_replay: Duration,
    pub canonical_signature_query: Duration,
    pub latest_round_trip: Duration,
    pub summary_round_trip: Duration,
    pub acknowledgement_to_interactive: Duration,
    pub zero_residue: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeathLatencyEvidenceV1 {
    pub report_schema: &'static str,
    pub feature_id: &'static str,
    pub sample_scope: &'static str,
    pub build_id: String,
    pub death_view_records_blake3: String,
    pub death_view_assets_blake3: String,
    pub death_view_localization_blake3: String,
    pub sample_count: usize,
    pub terminal_commit_latency: DurationStatsV1,
    pub exact_replay_latency: DurationStatsV1,
    pub canonical_signature_query_latency: DurationStatsV1,
    pub latest_round_trip_latency: DurationStatsV1,
    pub summary_round_trip_latency: DurationStatsV1,
    pub acknowledgement_to_interactive_latency: DurationStatsV1,
    pub every_summary_interactive_under_two_seconds: bool,
    pub zero_transport_task_session_transaction_and_lock_residue: bool,
    pub accepted: bool,
    pub raw_report_hash_blake3: String,
}

impl DeathLatencyEvidenceV1 {
    pub fn compile(
        samples: &[DeathLatencySampleV1],
        build_id: impl Into<String>,
        death_view_records_blake3: impl Into<String>,
        death_view_assets_blake3: impl Into<String>,
        death_view_localization_blake3: impl Into<String>,
    ) -> Result<Self, DeathMeasurementError> {
        let collect = |field: fn(&DeathLatencySampleV1) -> Duration| {
            samples.iter().map(field).collect::<Vec<_>>()
        };
        let acknowledgement_to_interactive =
            collect(|sample| sample.acknowledgement_to_interactive);
        let every_summary_interactive_under_two_seconds = acknowledgement_to_interactive
            .iter()
            .all(|sample| *sample < Duration::from_secs(2));
        let zero_residue = samples.iter().all(|sample| sample.zero_residue);
        let mut report = Self {
            report_schema: "gravebound.performance.gb-m03-06e.latency.v1",
            feature_id: "GB-M03-06E",
            sample_scope: "death-performance-not-final-25-full-loop-journeys",
            build_id: build_id.into(),
            death_view_records_blake3: death_view_records_blake3.into(),
            death_view_assets_blake3: death_view_assets_blake3.into(),
            death_view_localization_blake3: death_view_localization_blake3.into(),
            sample_count: samples.len(),
            terminal_commit_latency: DurationStatsV1::compile(&collect(|sample| {
                sample.terminal_commit
            }))?,
            exact_replay_latency: DurationStatsV1::compile(&collect(|sample| sample.exact_replay))?,
            canonical_signature_query_latency: DurationStatsV1::compile(&collect(|sample| {
                sample.canonical_signature_query
            }))?,
            latest_round_trip_latency: DurationStatsV1::compile(&collect(|sample| {
                sample.latest_round_trip
            }))?,
            summary_round_trip_latency: DurationStatsV1::compile(&collect(|sample| {
                sample.summary_round_trip
            }))?,
            acknowledgement_to_interactive_latency: DurationStatsV1::compile(
                &acknowledgement_to_interactive,
            )?,
            every_summary_interactive_under_two_seconds,
            zero_transport_task_session_transaction_and_lock_residue: zero_residue,
            accepted: every_summary_interactive_under_two_seconds && zero_residue,
            raw_report_hash_blake3: String::new(),
        };
        report.raw_report_hash_blake3 = report_hash(&report)?;
        Ok(report)
    }
}

fn report_hash(report: &DeathLatencyEvidenceV1) -> Result<String, DeathMeasurementError> {
    let mut hashable = report.clone();
    hashable.raw_report_hash_blake3.clear();
    let bytes = serde_json::to_vec(&hashable)
        .map_err(|error| DeathMeasurementError::Serialization(error.to_string()))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PostgresResidueSnapshotV1 {
    pub active_transactions: u64,
    pub idle_transactions: u64,
    pub aborted_transactions: u64,
    pub waiting_locks: u64,
    pub granted_locks: u64,
}

impl PostgresResidueSnapshotV1 {
    pub async fn capture(persistence: &PostgresPersistence) -> Result<Self, DeathMeasurementError> {
        let mut transaction = persistence
            .begin_transaction()
            .await
            .map_err(|error| DeathMeasurementError::PostgresInspection(error.to_string()))?;
        let row = sqlx::query(
            "SELECT \
             (SELECT count(*) FROM pg_stat_activity \
               WHERE datname=current_database() AND pid<>pg_backend_pid() \
                 AND backend_type='client backend' AND usename=current_user \
                 AND xact_start IS NOT NULL) AS active_transactions,\
             (SELECT count(*) FROM pg_stat_activity \
               WHERE datname=current_database() AND pid<>pg_backend_pid() \
                 AND backend_type='client backend' AND usename=current_user \
                 AND state='idle in transaction') AS idle_transactions,\
             (SELECT count(*) FROM pg_stat_activity \
               WHERE datname=current_database() AND pid<>pg_backend_pid() \
                 AND backend_type='client backend' AND usename=current_user \
                 AND state='idle in transaction (aborted)') AS aborted_transactions,\
             (SELECT count(*) FROM pg_locks AS held \
               JOIN pg_stat_activity AS activity ON activity.pid=held.pid \
               WHERE activity.datname=current_database() AND held.pid<>pg_backend_pid() \
                 AND activity.backend_type='client backend' \
                 AND activity.usename=current_user \
                 AND NOT held.granted) AS waiting_locks,\
             (SELECT count(*) FROM pg_locks AS held \
               JOIN pg_stat_activity AS activity ON activity.pid=held.pid \
               WHERE activity.datname=current_database() AND held.pid<>pg_backend_pid() \
                 AND activity.backend_type='client backend' \
                 AND activity.usename=current_user \
                 AND held.granted \
                 AND held.locktype IN ('relation','tuple','transactionid','advisory')) \
               AS granted_locks",
        )
        .fetch_one(transaction.connection())
        .await
        .map_err(|error| DeathMeasurementError::PostgresInspection(error.to_string()))?;
        transaction
            .rollback()
            .await
            .map_err(|error| DeathMeasurementError::PostgresInspection(error.to_string()))?;
        Ok(Self {
            active_transactions: nonnegative_count(&row, "active_transactions")?,
            idle_transactions: nonnegative_count(&row, "idle_transactions")?,
            aborted_transactions: nonnegative_count(&row, "aborted_transactions")?,
            waiting_locks: nonnegative_count(&row, "waiting_locks")?,
            granted_locks: nonnegative_count(&row, "granted_locks")?,
        })
    }

    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.active_transactions == 0
            && self.idle_transactions == 0
            && self.aborted_transactions == 0
            && self.waiting_locks == 0
            && self.granted_locks == 0
    }
}

fn nonnegative_count(
    row: &sqlx::postgres::PgRow,
    column: &'static str,
) -> Result<u64, DeathMeasurementError> {
    let count: i64 = row.get(column);
    u64::try_from(count).map_err(|_| DeathMeasurementError::NegativePostgresCount(column))
}

pub struct ProcessMemorySampler {
    system: System,
    pid: Pid,
}

impl ProcessMemorySampler {
    pub fn new() -> Result<Self, DeathMeasurementError> {
        Ok(Self {
            system: System::new(),
            pid: get_current_pid()
                .map_err(|error| DeathMeasurementError::ProcessMemory(error.to_string()))?,
        })
    }

    pub fn resident_bytes(&mut self) -> Result<u64, DeathMeasurementError> {
        self.system
            .refresh_processes(ProcessesToUpdate::Some(&[self.pid]), true);
        self.system
            .process(self.pid)
            .map(sysinfo::Process::memory)
            .ok_or(DeathMeasurementError::ProcessUnavailable)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ResidentMemorySampleV1 {
    pub elapsed_ms: u64,
    pub resident_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidentMemoryAssessmentKindV1 {
    InsufficientDuration,
    MonotonicGrowth,
    Stable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ResidentMemoryAssessmentV1 {
    pub kind: ResidentMemoryAssessmentKindV1,
    pub peak_resident_bytes: u64,
    pub post_warmup_growth_bytes: u64,
}

impl ResidentMemoryAssessmentV1 {
    pub fn compile(samples: &[ResidentMemorySampleV1]) -> Result<Self, DeathMeasurementError> {
        if samples.len() < 2 {
            return Err(DeathMeasurementError::MissingMemorySamples);
        }
        if samples
            .windows(2)
            .any(|pair| pair[0].elapsed_ms >= pair[1].elapsed_ms)
        {
            return Err(DeathMeasurementError::MemorySamplesOutOfOrder);
        }
        let duration = samples[samples.len() - 1]
            .elapsed_ms
            .saturating_sub(samples[0].elapsed_ms);
        let peak_resident_bytes = samples
            .iter()
            .map(|sample| sample.resident_bytes)
            .max()
            .unwrap_or(0);
        let post_warmup = &samples[1..];
        let post_warmup_growth_bytes = post_warmup[post_warmup.len() - 1]
            .resident_bytes
            .saturating_sub(post_warmup[0].resident_bytes);
        let monotonic_growth = post_warmup.len() >= 2
            && post_warmup_growth_bytes >= sim_core::MONOTONIC_GROWTH_FLOOR_BYTES
            && post_warmup
                .windows(2)
                .all(|pair| pair[0].resident_bytes < pair[1].resident_bytes);
        let kind = if duration < REQUIRED_MEMORY_DURATION_MS {
            ResidentMemoryAssessmentKindV1::InsufficientDuration
        } else if monotonic_growth {
            ResidentMemoryAssessmentKindV1::MonotonicGrowth
        } else {
            ResidentMemoryAssessmentKindV1::Stable
        };
        Ok(Self {
            kind,
            peak_resident_bytes,
            post_warmup_growth_bytes,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DeathRuntimeResidueV1 {
    pub accepted_connections: u64,
    pub rejected_connections: u64,
    pub combat_sessions_admitted: u64,
    pub completed_connection_tasks: u64,
    pub failed_connection_tasks: u64,
    pub remaining_connection_tasks: usize,
    pub remaining_open_connections: usize,
    pub zero_residue: bool,
    pub persistence_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathMemorySoakInputV1 {
    pub build_id: String,
    pub death_view_records_blake3: String,
    pub death_view_assets_blake3: String,
    pub death_view_localization_blake3: String,
    pub measured_duration_ms: u64,
    pub query_journeys: u64,
    pub death_view_queries: u64,
    pub connection_generations: u64,
    pub exact_replays: u64,
    pub canonical_signature_checks: u64,
    pub resident_memory_samples: Vec<ResidentMemorySampleV1>,
    pub canonical_signature_unchanged: bool,
    pub final_database_residue: PostgresResidueSnapshotV1,
    pub runtime_residue: DeathRuntimeResidueV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeathMemorySoakEvidenceV1 {
    pub report_schema: &'static str,
    pub feature_id: &'static str,
    pub sample_scope: &'static str,
    pub build_id: String,
    pub death_view_records_blake3: String,
    pub death_view_assets_blake3: String,
    pub death_view_localization_blake3: String,
    pub required_duration_ms: u64,
    pub measured_duration_ms: u64,
    pub query_journeys: u64,
    pub death_view_queries: u64,
    pub connection_generations: u64,
    pub completed_reconnects: u64,
    pub exact_replays: u64,
    pub canonical_signature_checks: u64,
    pub resident_memory_samples: Vec<ResidentMemorySampleV1>,
    pub resident_memory_assessment: ResidentMemoryAssessmentV1,
    pub supplemental_combined_process_memory_ceiling_bytes: u64,
    pub combined_process_peak_within_supplemental_ceiling: bool,
    pub canonical_signature_unchanged: bool,
    pub final_database_residue: PostgresResidueSnapshotV1,
    pub runtime_residue: DeathRuntimeResidueV1,
    pub accepted: bool,
    pub raw_report_hash_blake3: String,
}

impl DeathMemorySoakEvidenceV1 {
    pub fn compile(input: DeathMemorySoakInputV1) -> Result<Self, DeathMeasurementError> {
        let expected_query_count = input
            .query_journeys
            .checked_mul(DEATH_VIEW_QUERIES_PER_SOAK_JOURNEY)
            .ok_or(DeathMeasurementError::CounterOverflow)?;
        let expected_exact_replays = input.query_journeys / DEATH_SOAK_RECONNECT_INTERVAL_JOURNEYS;
        let expected_connection_generations = input
            .query_journeys
            .div_ceil(DEATH_SOAK_RECONNECT_INTERVAL_JOURNEYS);
        let completed_reconnects = input.connection_generations.saturating_sub(1);
        let resident_memory_assessment =
            ResidentMemoryAssessmentV1::compile(&input.resident_memory_samples)?;
        let combined_process_peak_within_supplemental_ceiling = resident_memory_assessment
            .peak_resident_bytes
            <= SUPPLEMENTAL_COMBINED_PROCESS_MEMORY_CEILING_BYTES;
        let runtime_clean = input.runtime_residue.zero_residue
            && input.runtime_residue.persistence_enabled
            && input.runtime_residue.accepted_connections == input.connection_generations
            && input.runtime_residue.rejected_connections == 0
            && input.runtime_residue.combat_sessions_admitted == 0
            && input.runtime_residue.completed_connection_tasks == input.connection_generations
            && input.runtime_residue.failed_connection_tasks == 0
            && input.runtime_residue.remaining_connection_tasks == 0
            && input.runtime_residue.remaining_open_connections == 0;
        let accepted = input.measured_duration_ms >= REQUIRED_MEMORY_DURATION_MS
            && input.query_journeys > DEATH_SOAK_RECONNECT_INTERVAL_JOURNEYS
            && input.death_view_queries == expected_query_count
            && input.connection_generations == expected_connection_generations
            && completed_reconnects > 0
            && input.exact_replays == expected_exact_replays
            && input.canonical_signature_checks == expected_exact_replays
            && resident_memory_assessment.kind == ResidentMemoryAssessmentKindV1::Stable
            && combined_process_peak_within_supplemental_ceiling
            && input.canonical_signature_unchanged
            && input.final_database_residue.is_zero()
            && runtime_clean;
        let mut report = Self {
            report_schema: "gravebound.performance.gb-m03-06e.death-memory-soak.v1",
            feature_id: "GB-M03-06E",
            sample_scope: "death-read-reconnect-replay-stability-not-final-private-loop-cohort",
            build_id: input.build_id,
            death_view_records_blake3: input.death_view_records_blake3,
            death_view_assets_blake3: input.death_view_assets_blake3,
            death_view_localization_blake3: input.death_view_localization_blake3,
            required_duration_ms: REQUIRED_MEMORY_DURATION_MS,
            measured_duration_ms: input.measured_duration_ms,
            query_journeys: input.query_journeys,
            death_view_queries: input.death_view_queries,
            connection_generations: input.connection_generations,
            completed_reconnects,
            exact_replays: input.exact_replays,
            canonical_signature_checks: input.canonical_signature_checks,
            resident_memory_samples: input.resident_memory_samples,
            resident_memory_assessment,
            supplemental_combined_process_memory_ceiling_bytes:
                SUPPLEMENTAL_COMBINED_PROCESS_MEMORY_CEILING_BYTES,
            combined_process_peak_within_supplemental_ceiling,
            canonical_signature_unchanged: input.canonical_signature_unchanged,
            final_database_residue: input.final_database_residue,
            runtime_residue: input.runtime_residue,
            accepted,
            raw_report_hash_blake3: String::new(),
        };
        report.raw_report_hash_blake3 = memory_report_hash(&report)?;
        Ok(report)
    }
}

#[allow(
    dead_code,
    reason = "shared measurement support is compiled separately by non-matrix integration targets"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeathBranchKindV1 {
    LevelBelowTen,
    CombatBelowThreshold,
    MissingQualifyingDeed,
    VerifiedServerIncident,
    EligibleSelfPromotion,
    EligibleExistingAvailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeathBranchEchoOutcomeV1 {
    NotEligible,
    Dormant,
    Available,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DeathBranchSampleV1 {
    pub branch: DeathBranchKindV1,
    pub echo_outcome: DeathBranchEchoOutcomeV1,
    pub terminal_commit_micros: u64,
    pub exact_replay_micros: u64,
    pub canonical_signature_query_micros: u64,
    pub latest_round_trip_micros: u64,
    pub summary_round_trip_micros: u64,
    pub post_commit_to_client_model_ready_micros: u64,
    pub target_echo_records: u32,
    pub target_echo_transitions: u32,
    pub target_outbox_events: u32,
    pub account_available_echoes: u32,
    pub account_dormant_echoes: u32,
    pub canonical_signature_unchanged: bool,
    pub database_residue: PostgresResidueSnapshotV1,
    pub runtime_residue: DeathRuntimeResidueV1,
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "the report retains four independent machine-readable audit gates"
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeathBranchMatrixEvidenceV1 {
    pub report_schema: &'static str,
    pub feature_id: &'static str,
    pub sample_scope: &'static str,
    pub build_id: String,
    pub death_view_records_blake3: String,
    pub death_view_assets_blake3: String,
    pub death_view_localization_blake3: String,
    pub branch_count: usize,
    pub branches: Vec<DeathBranchSampleV1>,
    pub exact_required_branch_set: bool,
    pub every_client_model_ready_under_two_seconds: bool,
    pub every_signature_stable_and_zero_residue: bool,
    pub accepted: bool,
    pub raw_report_hash_blake3: String,
}

impl DeathBranchMatrixEvidenceV1 {
    pub fn compile(
        mut branches: Vec<DeathBranchSampleV1>,
        build_id: impl Into<String>,
        death_view_records_blake3: impl Into<String>,
        death_view_assets_blake3: impl Into<String>,
        death_view_localization_blake3: impl Into<String>,
    ) -> Result<Self, DeathMeasurementError> {
        branches.sort_by_key(|sample| sample.branch);
        let actual = branches
            .iter()
            .map(|sample| sample.branch)
            .collect::<BTreeSet<_>>();
        let required = BTreeSet::from([
            DeathBranchKindV1::LevelBelowTen,
            DeathBranchKindV1::CombatBelowThreshold,
            DeathBranchKindV1::MissingQualifyingDeed,
            DeathBranchKindV1::VerifiedServerIncident,
            DeathBranchKindV1::EligibleSelfPromotion,
            DeathBranchKindV1::EligibleExistingAvailable,
        ]);
        let exact_required_branch_set = branches.len() == required.len() && actual == required;
        let every_client_model_ready_under_two_seconds = branches
            .iter()
            .all(|sample| sample.post_commit_to_client_model_ready_micros < 2_000_000);
        let every_signature_stable_and_zero_residue = branches.iter().all(|sample| {
            sample.canonical_signature_unchanged
                && sample.database_residue.is_zero()
                && branch_runtime_residue_is_exact(sample.runtime_residue)
        });
        let exact_outcomes = branches.iter().all(branch_sample_is_exact);
        let mut report = Self {
            report_schema: "gravebound.performance.gb-m03-06e.death-branch-matrix.v1",
            feature_id: "GB-M03-06E",
            sample_scope: "reachable-death-eligibility-echo-availability-client-model-not-native-frame",
            build_id: build_id.into(),
            death_view_records_blake3: death_view_records_blake3.into(),
            death_view_assets_blake3: death_view_assets_blake3.into(),
            death_view_localization_blake3: death_view_localization_blake3.into(),
            branch_count: branches.len(),
            branches,
            exact_required_branch_set,
            every_client_model_ready_under_two_seconds,
            every_signature_stable_and_zero_residue,
            accepted: exact_required_branch_set
                && every_client_model_ready_under_two_seconds
                && every_signature_stable_and_zero_residue
                && exact_outcomes,
            raw_report_hash_blake3: String::new(),
        };
        report.raw_report_hash_blake3 = branch_matrix_report_hash(&report)?;
        Ok(report)
    }

    pub fn write_json_atomically(&self, path: &Path) -> Result<(), DeathMeasurementError> {
        if self.raw_report_hash_blake3 != branch_matrix_report_hash(self)? {
            return Err(DeathMeasurementError::ReportHashMismatch);
        }
        if path.exists() {
            return Err(DeathMeasurementError::EvidenceIo(format!(
                "destination already exists: {}",
                path.display()
            )));
        }
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .map_err(|error| DeathMeasurementError::EvidenceIo(error.to_string()))?;
        }
        let temporary = partial_path(path);
        let result = (|| -> Result<(), DeathMeasurementError> {
            let bytes = serde_json::to_vec_pretty(self)
                .map_err(|error| DeathMeasurementError::Serialization(error.to_string()))?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|error| DeathMeasurementError::EvidenceIo(error.to_string()))?;
            file.write_all(&bytes)
                .map_err(|error| DeathMeasurementError::EvidenceIo(error.to_string()))?;
            file.sync_all()
                .map_err(|error| DeathMeasurementError::EvidenceIo(error.to_string()))?;
            fs::rename(&temporary, path)
                .map_err(|error| DeathMeasurementError::EvidenceIo(error.to_string()))
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}

fn partial_path(path: &Path) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(".partial");
    PathBuf::from(value)
}

const fn branch_runtime_residue_is_exact(residue: DeathRuntimeResidueV1) -> bool {
    residue.accepted_connections == 2
        && residue.rejected_connections == 0
        && residue.combat_sessions_admitted == 0
        && residue.completed_connection_tasks == 2
        && residue.failed_connection_tasks == 0
        && residue.remaining_connection_tasks == 0
        && residue.remaining_open_connections == 0
        && residue.zero_residue
        && residue.persistence_enabled
}

fn branch_sample_is_exact(sample: &DeathBranchSampleV1) -> bool {
    let positive_measurements = sample.terminal_commit_micros > 0
        && sample.exact_replay_micros > 0
        && sample.canonical_signature_query_micros > 0
        && sample.latest_round_trip_micros > 0
        && sample.summary_round_trip_micros > 0
        && sample.post_commit_to_client_model_ready_micros > 0;
    let outcome_exact = match sample.branch {
        DeathBranchKindV1::LevelBelowTen
        | DeathBranchKindV1::CombatBelowThreshold
        | DeathBranchKindV1::MissingQualifyingDeed
        | DeathBranchKindV1::VerifiedServerIncident => {
            sample.echo_outcome == DeathBranchEchoOutcomeV1::NotEligible
                && sample.target_echo_records == 0
                && sample.target_echo_transitions == 0
                && sample.target_outbox_events == 1
                && sample.account_available_echoes == 0
                && sample.account_dormant_echoes == 0
        }
        DeathBranchKindV1::EligibleSelfPromotion => {
            sample.echo_outcome == DeathBranchEchoOutcomeV1::Available
                && sample.target_echo_records == 1
                && sample.target_echo_transitions == 2
                && sample.target_outbox_events == 3
                && sample.account_available_echoes == 1
                && sample.account_dormant_echoes == 0
        }
        DeathBranchKindV1::EligibleExistingAvailable => {
            sample.echo_outcome == DeathBranchEchoOutcomeV1::Dormant
                && sample.target_echo_records == 1
                && sample.target_echo_transitions == 1
                && sample.target_outbox_events == 2
                && sample.account_available_echoes == 1
                && sample.account_dormant_echoes == 1
        }
    };
    positive_measurements && outcome_exact
}

fn branch_matrix_report_hash(
    report: &DeathBranchMatrixEvidenceV1,
) -> Result<String, DeathMeasurementError> {
    let mut hashable = report.clone();
    hashable.raw_report_hash_blake3.clear();
    let bytes = serde_json::to_vec(&hashable)
        .map_err(|error| DeathMeasurementError::Serialization(error.to_string()))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn memory_report_hash(report: &DeathMemorySoakEvidenceV1) -> Result<String, DeathMeasurementError> {
    let mut hashable = report.clone();
    hashable.raw_report_hash_blake3.clear();
    let bytes = serde_json::to_vec(&hashable)
        .map_err(|error| DeathMeasurementError::Serialization(error.to_string()))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn nearest_rank(sorted_values: &[u64], percentile: usize) -> u64 {
    let rank = sorted_values.len().saturating_mul(percentile).div_ceil(100);
    sorted_values[rank.saturating_sub(1)]
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DeathMeasurementError {
    #[error("at least one duration sample is required")]
    MissingDurationSamples,
    #[error("duration exceeds the evidence representation")]
    DurationOverflow,
    #[error("duration sample count exceeds the evidence representation")]
    SampleCountOverflow,
    #[error("measurement counter arithmetic overflowed")]
    CounterOverflow,
    #[error("at least two resident-memory samples are required")]
    MissingMemorySamples,
    #[error("resident-memory samples must be strictly ordered")]
    MemorySamplesOutOfOrder,
    #[error("PostgreSQL residue count was negative for {0}")]
    NegativePostgresCount(&'static str),
    #[error("PostgreSQL residue inspection failed: {0}")]
    PostgresInspection(String),
    #[error("process-memory inspection failed: {0}")]
    ProcessMemory(String),
    #[error("the measured process disappeared")]
    ProcessUnavailable,
    #[error("measurement report serialization failed: {0}")]
    Serialization(String),
    #[error("measurement report hash does not match its payload")]
    ReportHashMismatch,
    #[error("measurement evidence I/O failed: {0}")]
    EvidenceIo(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_stats_use_nearest_rank_and_exact_maximum() {
        let samples = (1_u64..=20).map(Duration::from_micros).collect::<Vec<_>>();
        let stats = DurationStatsV1::compile(&samples).unwrap();
        assert_eq!(stats.sample_count, 20);
        assert_eq!(stats.median_micros, 11);
        assert_eq!(stats.p95_micros, 19);
        assert_eq!(stats.maximum_micros, 20);
    }

    #[test]
    fn duration_stats_reject_an_empty_sample_set() {
        assert_eq!(
            DurationStatsV1::compile(&[]),
            Err(DeathMeasurementError::MissingDurationSamples)
        );
    }

    #[test]
    fn latency_report_hash_is_stable_and_sensitive() {
        let baseline = DeathLatencySampleV1 {
            terminal_commit: Duration::from_micros(10),
            exact_replay: Duration::from_micros(5),
            canonical_signature_query: Duration::from_micros(4),
            latest_round_trip: Duration::from_micros(3),
            summary_round_trip: Duration::from_micros(2),
            acknowledgement_to_interactive: Duration::from_micros(8),
            zero_residue: true,
        };
        let compile = |sample| {
            DeathLatencyEvidenceV1::compile(
                &[sample],
                "core-dev",
                "a".repeat(64),
                "b".repeat(64),
                "c".repeat(64),
            )
            .unwrap()
        };
        let first = compile(baseline);
        let same = compile(baseline);
        assert_eq!(first.raw_report_hash_blake3, same.raw_report_hash_blake3);
        assert_eq!(first.raw_report_hash_blake3.len(), 64);

        let changed = compile(DeathLatencySampleV1 {
            acknowledgement_to_interactive: Duration::from_micros(9),
            ..baseline
        });
        assert_ne!(first.raw_report_hash_blake3, changed.raw_report_hash_blake3);
    }

    #[test]
    fn memory_assessment_rejects_unordered_samples() {
        assert_eq!(
            ResidentMemoryAssessmentV1::compile(&[
                ResidentMemorySampleV1 {
                    elapsed_ms: 10,
                    resident_bytes: 1,
                },
                ResidentMemorySampleV1 {
                    elapsed_ms: 10,
                    resident_bytes: 2,
                },
            ]),
            Err(DeathMeasurementError::MemorySamplesOutOfOrder)
        );
    }

    #[test]
    fn memory_assessment_distinguishes_short_stable_and_monotonic_runs() {
        let short = ResidentMemoryAssessmentV1::compile(&[
            ResidentMemorySampleV1 {
                elapsed_ms: 0,
                resident_bytes: 100,
            },
            ResidentMemorySampleV1 {
                elapsed_ms: 10_000,
                resident_bytes: 101,
            },
        ])
        .unwrap();
        assert_eq!(
            short.kind,
            ResidentMemoryAssessmentKindV1::InsufficientDuration
        );

        let stable = ResidentMemoryAssessmentV1::compile(&[
            ResidentMemorySampleV1 {
                elapsed_ms: 0,
                resident_bytes: 100,
            },
            ResidentMemorySampleV1 {
                elapsed_ms: 600_000,
                resident_bytes: 110,
            },
            ResidentMemorySampleV1 {
                elapsed_ms: 1_200_000,
                resident_bytes: 108,
            },
            ResidentMemorySampleV1 {
                elapsed_ms: 1_800_000,
                resident_bytes: 109,
            },
        ])
        .unwrap();
        assert_eq!(stable.kind, ResidentMemoryAssessmentKindV1::Stable);

        let floor = sim_core::MONOTONIC_GROWTH_FLOOR_BYTES;
        let monotonic = ResidentMemoryAssessmentV1::compile(&[
            ResidentMemorySampleV1 {
                elapsed_ms: 0,
                resident_bytes: 100,
            },
            ResidentMemorySampleV1 {
                elapsed_ms: 600_000,
                resident_bytes: 1_000,
            },
            ResidentMemorySampleV1 {
                elapsed_ms: 1_200_000,
                resident_bytes: 1_000 + floor,
            },
            ResidentMemorySampleV1 {
                elapsed_ms: 1_800_000,
                resident_bytes: 1_001 + floor,
            },
        ])
        .unwrap();
        assert_eq!(
            monotonic.kind,
            ResidentMemoryAssessmentKindV1::MonotonicGrowth
        );
    }

    fn accepted_memory_soak_input() -> DeathMemorySoakInputV1 {
        DeathMemorySoakInputV1 {
            build_id: "core-dev".to_owned(),
            death_view_records_blake3: "a".repeat(64),
            death_view_assets_blake3: "b".repeat(64),
            death_view_localization_blake3: "c".repeat(64),
            measured_duration_ms: REQUIRED_MEMORY_DURATION_MS,
            query_journeys: 21,
            death_view_queries: 84,
            connection_generations: 2,
            exact_replays: 1,
            canonical_signature_checks: 1,
            resident_memory_samples: vec![
                ResidentMemorySampleV1 {
                    elapsed_ms: 0,
                    resident_bytes: 100,
                },
                ResidentMemorySampleV1 {
                    elapsed_ms: 600_000,
                    resident_bytes: 110,
                },
                ResidentMemorySampleV1 {
                    elapsed_ms: 1_200_000,
                    resident_bytes: 105,
                },
                ResidentMemorySampleV1 {
                    elapsed_ms: REQUIRED_MEMORY_DURATION_MS,
                    resident_bytes: 108,
                },
            ],
            canonical_signature_unchanged: true,
            final_database_residue: PostgresResidueSnapshotV1 {
                active_transactions: 0,
                idle_transactions: 0,
                aborted_transactions: 0,
                waiting_locks: 0,
                granted_locks: 0,
            },
            runtime_residue: DeathRuntimeResidueV1 {
                accepted_connections: 2,
                rejected_connections: 0,
                combat_sessions_admitted: 0,
                completed_connection_tasks: 2,
                failed_connection_tasks: 0,
                remaining_connection_tasks: 0,
                remaining_open_connections: 0,
                zero_residue: true,
                persistence_enabled: true,
            },
        }
    }

    #[test]
    fn memory_soak_report_is_hashed_and_fails_closed() {
        let accepted = DeathMemorySoakEvidenceV1::compile(accepted_memory_soak_input()).unwrap();
        assert!(accepted.accepted);
        assert_eq!(accepted.raw_report_hash_blake3.len(), 64);

        let same = DeathMemorySoakEvidenceV1::compile(accepted_memory_soak_input()).unwrap();
        assert_eq!(accepted.raw_report_hash_blake3, same.raw_report_hash_blake3);

        let mut short = accepted_memory_soak_input();
        short.measured_duration_ms = REQUIRED_MEMORY_DURATION_MS - 1;
        let short = DeathMemorySoakEvidenceV1::compile(short).unwrap();
        assert!(!short.accepted);
        assert_ne!(
            accepted.raw_report_hash_blake3,
            short.raw_report_hash_blake3
        );

        let mut wrong_query_count = accepted_memory_soak_input();
        wrong_query_count.death_view_queries = 83;
        assert!(
            !DeathMemorySoakEvidenceV1::compile(wrong_query_count)
                .unwrap()
                .accepted
        );

        let mut no_completed_reconnect = accepted_memory_soak_input();
        no_completed_reconnect.query_journeys = DEATH_SOAK_RECONNECT_INTERVAL_JOURNEYS;
        no_completed_reconnect.death_view_queries =
            DEATH_SOAK_RECONNECT_INTERVAL_JOURNEYS * DEATH_VIEW_QUERIES_PER_SOAK_JOURNEY;
        no_completed_reconnect.connection_generations = 1;
        no_completed_reconnect.runtime_residue.accepted_connections = 1;
        no_completed_reconnect
            .runtime_residue
            .completed_connection_tasks = 1;
        assert!(
            !DeathMemorySoakEvidenceV1::compile(no_completed_reconnect)
                .unwrap()
                .accepted
        );
    }

    fn branch_sample(
        branch: DeathBranchKindV1,
        echo_outcome: DeathBranchEchoOutcomeV1,
        target_echo_records: u32,
        target_echo_transitions: u32,
        target_outbox_events: u32,
        account_available_echoes: u32,
        account_dormant_echoes: u32,
    ) -> DeathBranchSampleV1 {
        DeathBranchSampleV1 {
            branch,
            echo_outcome,
            terminal_commit_micros: 10,
            exact_replay_micros: 5,
            canonical_signature_query_micros: 4,
            latest_round_trip_micros: 3,
            summary_round_trip_micros: 2,
            post_commit_to_client_model_ready_micros: 8,
            target_echo_records,
            target_echo_transitions,
            target_outbox_events,
            account_available_echoes,
            account_dormant_echoes,
            canonical_signature_unchanged: true,
            database_residue: PostgresResidueSnapshotV1 {
                active_transactions: 0,
                idle_transactions: 0,
                aborted_transactions: 0,
                waiting_locks: 0,
                granted_locks: 0,
            },
            runtime_residue: DeathRuntimeResidueV1 {
                accepted_connections: 2,
                rejected_connections: 0,
                combat_sessions_admitted: 0,
                completed_connection_tasks: 2,
                failed_connection_tasks: 0,
                remaining_connection_tasks: 0,
                remaining_open_connections: 0,
                zero_residue: true,
                persistence_enabled: true,
            },
        }
    }

    fn accepted_branch_matrix() -> Vec<DeathBranchSampleV1> {
        let ineligible =
            |branch| branch_sample(branch, DeathBranchEchoOutcomeV1::NotEligible, 0, 0, 1, 0, 0);
        vec![
            ineligible(DeathBranchKindV1::LevelBelowTen),
            ineligible(DeathBranchKindV1::CombatBelowThreshold),
            ineligible(DeathBranchKindV1::MissingQualifyingDeed),
            ineligible(DeathBranchKindV1::VerifiedServerIncident),
            branch_sample(
                DeathBranchKindV1::EligibleSelfPromotion,
                DeathBranchEchoOutcomeV1::Available,
                1,
                2,
                3,
                1,
                0,
            ),
            branch_sample(
                DeathBranchKindV1::EligibleExistingAvailable,
                DeathBranchEchoOutcomeV1::Dormant,
                1,
                1,
                2,
                1,
                1,
            ),
        ]
    }

    #[test]
    fn branch_matrix_report_requires_every_exact_outcome_and_hashes_it() {
        let compile = |branches| {
            DeathBranchMatrixEvidenceV1::compile(
                branches,
                "core-dev",
                "a".repeat(64),
                "b".repeat(64),
                "c".repeat(64),
            )
            .unwrap()
        };
        let accepted = compile(accepted_branch_matrix());
        assert!(accepted.accepted);
        assert_eq!(accepted.raw_report_hash_blake3.len(), 64);
        assert_eq!(
            accepted.raw_report_hash_blake3,
            compile(accepted_branch_matrix()).raw_report_hash_blake3
        );

        let mut missing = accepted_branch_matrix();
        missing.pop();
        assert!(!compile(missing).accepted);

        let mut wrong = accepted_branch_matrix();
        wrong[0].target_outbox_events = 2;
        let wrong = compile(wrong);
        assert!(!wrong.accepted);
        assert_ne!(
            accepted.raw_report_hash_blake3,
            wrong.raw_report_hash_blake3
        );
    }

    #[test]
    fn branch_matrix_report_publication_is_atomic_new_only_and_hash_checked() {
        let report = DeathBranchMatrixEvidenceV1::compile(
            accepted_branch_matrix(),
            "core-dev",
            "a".repeat(64),
            "b".repeat(64),
            "c".repeat(64),
        )
        .unwrap();
        let ordinal = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "gravebound-death-branch-report-{}-{ordinal}",
            std::process::id()
        ));
        let path = root.join("report.json");
        report.write_json_atomically(&path).unwrap();
        let encoded = fs::read(&path).unwrap();
        let decoded = serde_json::from_slice::<serde_json::Value>(&encoded).unwrap();
        assert_eq!(
            decoded["raw_report_hash_blake3"],
            report.raw_report_hash_blake3
        );
        assert!(!partial_path(&path).exists());
        assert!(matches!(
            report.write_json_atomically(&path),
            Err(DeathMeasurementError::EvidenceIo(_))
        ));

        let mut tampered = report;
        tampered.branch_count += 1;
        assert_eq!(
            tampered.write_json_atomically(&root.join("tampered.json")),
            Err(DeathMeasurementError::ReportHashMismatch)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn process_memory_sampler_observes_the_current_test_process() {
        assert!(
            ProcessMemorySampler::new()
                .unwrap()
                .resident_bytes()
                .unwrap()
                > 0
        );
    }
}
