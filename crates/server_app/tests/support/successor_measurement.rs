//! Reusable `GB-M03-07` successor-recovery timing and cleanup evidence.
//!
//! Authorities:
//! - `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-020`, `DTH-021`, `UI-007`-
//!   `009`, `TECH-021`-`023`, and `QA-101`;
//! - `Gravebound_Content_Production_Spec_v1.md`: `CONT-CATALOG-003`;
//! - `Gravebound_Development_Roadmap_v1.md`: `GB-M03-07` and the M03 exit gate.
//!
//! This module compiles evidence from the disposable authenticated route. It does not create a
//! successor, infer player behavior, or become an alternate gameplay writer.

use std::{
    collections::BTreeSet,
    fs,
    io::Write as _,
    path::{Path, PathBuf},
};

use serde::Serialize;
use thiserror::Error;

use crate::death_measurement::PostgresResidueSnapshotV1;

pub const REQUIRED_SUCCESSOR_JOURNEY_COUNT: usize = 25;
const DEATH_TO_CONTROL_MEDIAN_LIMIT_MICROS: u64 = 15_000_000;
const DEATH_TO_CONTROL_P95_LIMIT_MICROS: u64 = 30_000_000;
const SUMMARY_TO_COMBAT_LIMIT_MICROS: u64 = 120_000_000;
const REQUIRED_COMBAT_PERCENT: usize = 70;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SuccessorDurationStatsV1 {
    pub sample_count: u32,
    pub median_micros: u64,
    pub p95_micros: u64,
    pub maximum_micros: u64,
}

impl SuccessorDurationStatsV1 {
    fn compile(samples: &[u64]) -> Result<Self, SuccessorMeasurementError> {
        if samples.is_empty() {
            return Err(SuccessorMeasurementError::MissingDurationSamples);
        }
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        Ok(Self {
            sample_count: u32::try_from(sorted.len())
                .map_err(|_| SuccessorMeasurementError::SampleCountOverflow)?,
            median_micros: sorted[sorted.len() / 2],
            p95_micros: nearest_rank(&sorted, 95),
            maximum_micros: sorted[sorted.len() - 1],
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SuccessorJourneySampleV1 {
    pub journey_ordinal: u8,
    pub death_id: [u8; 16],
    pub successor_id: [u8; 16],
    pub receipt_id: [u8; 16],
    pub starter_item_uids: [[u8; 16]; 4],
    pub terminal_commit_micros: u64,
    pub successor_create_round_trip_micros: u64,
    pub death_to_control_micros: u64,
    pub summary_to_control_micros: u64,
    pub control_to_combat_micros: u64,
    pub summary_to_combat_micros: u64,
    pub confirmations: u8,
    pub fresh_successor_result: bool,
    pub exact_durable_graph: bool,
    pub transport_and_task_zero_residue: bool,
    pub database_residue: PostgresResidueSnapshotV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuccessorJourneyAuthorityV1 {
    pub build_id: String,
    pub successor_content_revision: String,
    pub world_records_blake3: String,
    pub world_assets_blake3: String,
    pub world_localization_blake3: String,
    pub death_view_records_blake3: String,
    pub death_view_assets_blake3: String,
    pub death_view_localization_blake3: String,
}

impl SuccessorJourneyAuthorityV1 {
    fn validate(&self) -> Result<(), SuccessorMeasurementError> {
        if self.build_id.is_empty() || self.successor_content_revision.is_empty() {
            return Err(SuccessorMeasurementError::InvalidBuildAuthority);
        }
        for hash in [
            &self.world_records_blake3,
            &self.world_assets_blake3,
            &self.world_localization_blake3,
            &self.death_view_records_blake3,
            &self.death_view_assets_blake3,
            &self.death_view_localization_blake3,
        ] {
            if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(SuccessorMeasurementError::InvalidBuildAuthority);
            }
        }
        Ok(())
    }
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "the report retains independent timing, identity, graph, cleanup, and scope gates"
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SuccessorJourneyEvidenceV1 {
    pub report_schema: &'static str,
    pub feature_id: &'static str,
    pub sample_scope: &'static str,
    pub behavioral_cohort_scope: &'static str,
    pub build_id: String,
    pub successor_content_revision: String,
    pub world_records_blake3: String,
    pub world_assets_blake3: String,
    pub world_localization_blake3: String,
    pub death_view_records_blake3: String,
    pub death_view_assets_blake3: String,
    pub death_view_localization_blake3: String,
    pub required_sample_count: usize,
    pub sample_count: usize,
    pub samples: Vec<SuccessorJourneySampleV1>,
    pub terminal_commit_latency: SuccessorDurationStatsV1,
    pub successor_create_round_trip_latency: SuccessorDurationStatsV1,
    pub death_to_control_latency: SuccessorDurationStatsV1,
    pub summary_to_control_latency: SuccessorDurationStatsV1,
    pub control_to_combat_latency: SuccessorDurationStatsV1,
    pub summary_to_combat_latency: SuccessorDurationStatsV1,
    pub death_to_control_median_under_fifteen_seconds: bool,
    pub death_to_control_p95_under_thirty_seconds: bool,
    pub successor_combat_within_two_minutes_count: usize,
    pub successor_combat_within_two_minutes_percent: u8,
    pub successor_combat_route_meets_seventy_percent: bool,
    pub every_journey_used_two_confirmations: bool,
    pub every_successor_result_fresh: bool,
    pub all_death_successor_receipt_and_starter_ids_unique: bool,
    pub every_durable_graph_exact: bool,
    pub zero_transport_task_session_transaction_and_lock_residue: bool,
    pub accepted: bool,
    pub raw_report_hash_blake3: String,
}

impl SuccessorJourneyEvidenceV1 {
    #[allow(
        clippy::too_many_lines,
        reason = "the versioned report derives every timing, identity, route, and cleanup gate in one hashable compilation boundary"
    )]
    pub fn compile(
        mut samples: Vec<SuccessorJourneySampleV1>,
        authority: SuccessorJourneyAuthorityV1,
    ) -> Result<Self, SuccessorMeasurementError> {
        authority.validate()?;
        samples.sort_by_key(|sample| sample.journey_ordinal);
        let exact_ordinals = samples.len() == REQUIRED_SUCCESSOR_JOURNEY_COUNT
            && samples.iter().enumerate().all(|(index, sample)| {
                sample.journey_ordinal == u8::try_from(index + 1).unwrap_or(0)
            });
        let durations = |field: fn(&SuccessorJourneySampleV1) -> u64| {
            samples.iter().map(field).collect::<Vec<_>>()
        };
        let terminal_commit_latency =
            SuccessorDurationStatsV1::compile(&durations(|sample| sample.terminal_commit_micros))?;
        let successor_create_round_trip_latency =
            SuccessorDurationStatsV1::compile(&durations(|sample| {
                sample.successor_create_round_trip_micros
            }))?;
        let death_to_control_latency =
            SuccessorDurationStatsV1::compile(&durations(|sample| sample.death_to_control_micros))?;
        let summary_to_control_latency = SuccessorDurationStatsV1::compile(&durations(|sample| {
            sample.summary_to_control_micros
        }))?;
        let control_to_combat_latency = SuccessorDurationStatsV1::compile(&durations(|sample| {
            sample.control_to_combat_micros
        }))?;
        let summary_to_combat_latency = SuccessorDurationStatsV1::compile(&durations(|sample| {
            sample.summary_to_combat_micros
        }))?;
        let positive_durations = samples.iter().all(|sample| {
            sample.terminal_commit_micros > 0
                && sample.successor_create_round_trip_micros > 0
                && sample.death_to_control_micros > 0
                && sample.summary_to_control_micros > 0
                && sample.control_to_combat_micros > 0
                && sample.summary_to_combat_micros > 0
                && sample.death_to_control_micros >= sample.summary_to_control_micros
                && sample.summary_to_combat_micros >= sample.summary_to_control_micros
                && sample.summary_to_combat_micros >= sample.control_to_combat_micros
        });
        let death_to_control_median_under_fifteen_seconds =
            death_to_control_latency.median_micros < DEATH_TO_CONTROL_MEDIAN_LIMIT_MICROS;
        let death_to_control_p95_under_thirty_seconds =
            death_to_control_latency.p95_micros < DEATH_TO_CONTROL_P95_LIMIT_MICROS;
        let successor_combat_within_two_minutes_count = samples
            .iter()
            .filter(|sample| sample.summary_to_combat_micros <= SUMMARY_TO_COMBAT_LIMIT_MICROS)
            .count();
        let successor_combat_route_meets_seventy_percent = !samples.is_empty()
            && successor_combat_within_two_minutes_count.saturating_mul(100)
                >= samples.len().saturating_mul(REQUIRED_COMBAT_PERCENT);
        let successor_combat_within_two_minutes_percent = if samples.is_empty() {
            0
        } else {
            u8::try_from(
                successor_combat_within_two_minutes_count.saturating_mul(100) / samples.len(),
            )
            .unwrap_or(100)
        };
        let every_journey_used_two_confirmations =
            samples.iter().all(|sample| sample.confirmations == 2);
        let every_successor_result_fresh =
            samples.iter().all(|sample| sample.fresh_successor_result);
        let every_durable_graph_exact = samples.iter().all(|sample| sample.exact_durable_graph);
        let zero_residue = samples.iter().all(|sample| {
            sample.transport_and_task_zero_residue && sample.database_residue.is_zero()
        });
        let all_ids_unique = ids_are_unique(&samples);
        let accepted = exact_ordinals
            && positive_durations
            && death_to_control_median_under_fifteen_seconds
            && death_to_control_p95_under_thirty_seconds
            && successor_combat_route_meets_seventy_percent
            && every_journey_used_two_confirmations
            && every_successor_result_fresh
            && all_ids_unique
            && every_durable_graph_exact
            && zero_residue;
        let mut report = Self {
            report_schema: "gravebound.performance.gb-m03-07.successor-recovery.v1",
            feature_id: "GB-M03-07",
            sample_scope: "automated-disposable-death-summary-successor-control-and-danger-route",
            behavioral_cohort_scope: "not-measured-human-private-cohort-remains-a-separate-m03-gate",
            build_id: authority.build_id,
            successor_content_revision: authority.successor_content_revision,
            world_records_blake3: authority.world_records_blake3,
            world_assets_blake3: authority.world_assets_blake3,
            world_localization_blake3: authority.world_localization_blake3,
            death_view_records_blake3: authority.death_view_records_blake3,
            death_view_assets_blake3: authority.death_view_assets_blake3,
            death_view_localization_blake3: authority.death_view_localization_blake3,
            required_sample_count: REQUIRED_SUCCESSOR_JOURNEY_COUNT,
            sample_count: samples.len(),
            samples,
            terminal_commit_latency,
            successor_create_round_trip_latency,
            death_to_control_latency,
            summary_to_control_latency,
            control_to_combat_latency,
            summary_to_combat_latency,
            death_to_control_median_under_fifteen_seconds,
            death_to_control_p95_under_thirty_seconds,
            successor_combat_within_two_minutes_count,
            successor_combat_within_two_minutes_percent,
            successor_combat_route_meets_seventy_percent,
            every_journey_used_two_confirmations,
            every_successor_result_fresh,
            all_death_successor_receipt_and_starter_ids_unique: all_ids_unique,
            every_durable_graph_exact,
            zero_transport_task_session_transaction_and_lock_residue: zero_residue,
            accepted,
            raw_report_hash_blake3: String::new(),
        };
        report.raw_report_hash_blake3 = report_hash(&report)?;
        Ok(report)
    }

    pub fn write_json_atomically(&self, path: &Path) -> Result<(), SuccessorMeasurementError> {
        if self.raw_report_hash_blake3 != report_hash(self)? {
            return Err(SuccessorMeasurementError::ReportHashMismatch);
        }
        if path.exists() {
            return Err(SuccessorMeasurementError::EvidenceIo(format!(
                "destination already exists: {}",
                path.display()
            )));
        }
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .map_err(|error| SuccessorMeasurementError::EvidenceIo(error.to_string()))?;
        }
        let temporary = partial_path(path);
        let result = (|| -> Result<(), SuccessorMeasurementError> {
            let bytes = serde_json::to_vec_pretty(self)
                .map_err(|error| SuccessorMeasurementError::Serialization(error.to_string()))?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|error| SuccessorMeasurementError::EvidenceIo(error.to_string()))?;
            file.write_all(&bytes)
                .map_err(|error| SuccessorMeasurementError::EvidenceIo(error.to_string()))?;
            file.sync_all()
                .map_err(|error| SuccessorMeasurementError::EvidenceIo(error.to_string()))?;
            fs::rename(&temporary, path)
                .map_err(|error| SuccessorMeasurementError::EvidenceIo(error.to_string()))
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}

fn ids_are_unique(samples: &[SuccessorJourneySampleV1]) -> bool {
    let nonzero = |id: &[u8; 16]| *id != [0; 16];
    let deaths = samples
        .iter()
        .map(|sample| sample.death_id)
        .collect::<BTreeSet<_>>();
    let successors = samples
        .iter()
        .map(|sample| sample.successor_id)
        .collect::<BTreeSet<_>>();
    let receipts = samples
        .iter()
        .map(|sample| sample.receipt_id)
        .collect::<BTreeSet<_>>();
    let starter_items = samples
        .iter()
        .flat_map(|sample| sample.starter_item_uids)
        .collect::<BTreeSet<_>>();
    deaths.len() == samples.len()
        && successors.len() == samples.len()
        && receipts.len() == samples.len()
        && starter_items.len() == samples.len().saturating_mul(4)
        && deaths.iter().all(nonzero)
        && successors.iter().all(nonzero)
        && receipts.iter().all(nonzero)
        && starter_items.iter().all(nonzero)
}

fn report_hash(report: &SuccessorJourneyEvidenceV1) -> Result<String, SuccessorMeasurementError> {
    let mut hashable = report.clone();
    hashable.raw_report_hash_blake3.clear();
    let bytes = serde_json::to_vec(&hashable)
        .map_err(|error| SuccessorMeasurementError::Serialization(error.to_string()))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn partial_path(path: &Path) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(".partial");
    PathBuf::from(value)
}

fn nearest_rank(sorted_values: &[u64], percentile: usize) -> u64 {
    let rank = sorted_values.len().saturating_mul(percentile).div_ceil(100);
    sorted_values[rank.saturating_sub(1)]
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SuccessorMeasurementError {
    #[error("at least one successor duration sample is required")]
    MissingDurationSamples,
    #[error("successor duration sample count exceeds the evidence representation")]
    SampleCountOverflow,
    #[error("successor build/content authority is invalid")]
    InvalidBuildAuthority,
    #[error("successor report serialization failed: {0}")]
    Serialization(String),
    #[error("successor report hash does not match its payload")]
    ReportHashMismatch,
    #[error("successor evidence I/O failed: {0}")]
    EvidenceIo(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authority() -> SuccessorJourneyAuthorityV1 {
        SuccessorJourneyAuthorityV1 {
            build_id: "core-dev".into(),
            successor_content_revision: "core-dev.blake3.abc".into(),
            world_records_blake3: "a".repeat(64),
            world_assets_blake3: "b".repeat(64),
            world_localization_blake3: "c".repeat(64),
            death_view_records_blake3: "d".repeat(64),
            death_view_assets_blake3: "e".repeat(64),
            death_view_localization_blake3: "f".repeat(64),
        }
    }

    fn id(kind: u8, ordinal: u8) -> [u8; 16] {
        let mut id = [kind; 16];
        id[0] = kind;
        id[1] = ordinal;
        id
    }

    fn accepted_samples() -> Vec<SuccessorJourneySampleV1> {
        (1_u8..=25)
            .map(|ordinal| SuccessorJourneySampleV1 {
                journey_ordinal: ordinal,
                death_id: id(1, ordinal),
                successor_id: id(2, ordinal),
                receipt_id: id(3, ordinal),
                starter_item_uids: [
                    id(4, ordinal),
                    id(5, ordinal),
                    id(6, ordinal),
                    id(7, ordinal),
                ],
                terminal_commit_micros: 10,
                successor_create_round_trip_micros: 20,
                death_to_control_micros: 1_000_000,
                summary_to_control_micros: 800_000,
                control_to_combat_micros: 200_000,
                summary_to_combat_micros: 1_000_000,
                confirmations: 2,
                fresh_successor_result: true,
                exact_durable_graph: true,
                transport_and_task_zero_residue: true,
                database_residue: PostgresResidueSnapshotV1 {
                    active_transactions: 0,
                    idle_transactions: 0,
                    aborted_transactions: 0,
                    waiting_locks: 0,
                    granted_locks: 0,
                },
            })
            .collect()
    }

    #[test]
    fn report_requires_exact_unique_technical_journeys_and_hashes_them() {
        let accepted =
            SuccessorJourneyEvidenceV1::compile(accepted_samples(), authority()).unwrap();
        assert!(accepted.accepted);
        assert_eq!(accepted.sample_count, REQUIRED_SUCCESSOR_JOURNEY_COUNT);
        assert_eq!(accepted.successor_combat_within_two_minutes_percent, 100);
        assert_eq!(accepted.raw_report_hash_blake3.len(), 64);
        assert_eq!(
            accepted.raw_report_hash_blake3,
            SuccessorJourneyEvidenceV1::compile(accepted_samples(), authority())
                .unwrap()
                .raw_report_hash_blake3
        );

        let mut duplicate = accepted_samples();
        duplicate[24].starter_item_uids[3] = duplicate[0].starter_item_uids[0];
        assert!(
            !SuccessorJourneyEvidenceV1::compile(duplicate, authority())
                .unwrap()
                .accepted
        );

        let mut slow = accepted_samples();
        for sample in &mut slow {
            sample.death_to_control_micros = DEATH_TO_CONTROL_MEDIAN_LIMIT_MICROS;
        }
        assert!(
            !SuccessorJourneyEvidenceV1::compile(slow, authority())
                .unwrap()
                .accepted
        );
    }

    #[test]
    fn report_publication_is_atomic_new_only_and_hash_checked() {
        let report = SuccessorJourneyEvidenceV1::compile(accepted_samples(), authority()).unwrap();
        let root = std::env::temp_dir().join(format!(
            "gravebound-successor-report-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = root.join("report.json");
        report.write_json_atomically(&path).unwrap();
        assert!(!partial_path(&path).exists());
        assert!(matches!(
            report.write_json_atomically(&path),
            Err(SuccessorMeasurementError::EvidenceIo(_))
        ));

        let mut tampered = report;
        tampered.sample_count = 24;
        assert_eq!(
            tampered.write_json_atomically(&root.join("tampered.json")),
            Err(SuccessorMeasurementError::ReportHashMismatch)
        );
        fs::remove_dir_all(root).unwrap();
    }
}
