//! Reusable evidence compiler for the assembled `GB-M03-03` ordinary private-life route.
//!
//! Authorities:
//! - `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-021`, `QA-005`, and `QA-101`;
//! - `Gravebound_Content_Production_Spec_v1.md`: Core starter custody, Bell Sepulcher B0-B6,
//!   Sir Caldus, stable exit, and Hall-return authority;
//! - `Gravebound_Development_Roadmap_v1.md`: 25 scripted full-loop journeys plus login,
//!   death-to-successor, and successor-to-combat gates.
//!
//! This module compiles observations from the authenticated public QUIC route. It never creates
//! gameplay state, supplies destinations, advances simulation, or substitutes automated
//! reachability for the separate human private-cohort metric.

use std::{
    collections::BTreeSet,
    fs,
    io::Write as _,
    path::{Path, PathBuf},
};

use serde::Serialize;
use thiserror::Error;

pub const REQUIRED_PRIVATE_LIFE_JOURNEY_COUNT: usize = 25;
const LOGIN_TO_CONTROL_MEDIAN_LIMIT_MICROS: u64 = 30_000_000;
const DEATH_TO_CONTROL_MEDIAN_LIMIT_MICROS: u64 = 15_000_000;
const DEATH_TO_CONTROL_P95_LIMIT_MICROS: u64 = 30_000_000;
const SUMMARY_TO_COMBAT_LIMIT_MICROS: u64 = 120_000_000;
const REQUIRED_COMBAT_PERCENT: usize = 70;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PrivateLifeDurationStatsV1 {
    pub sample_count: u32,
    pub median_micros: u64,
    pub p95_micros: u64,
    pub maximum_micros: u64,
}

impl PrivateLifeDurationStatsV1 {
    fn compile(samples: &[u64]) -> Result<Self, PrivateLifeMeasurementError> {
        if samples.is_empty() {
            return Err(PrivateLifeMeasurementError::MissingDurationSamples);
        }
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        Ok(Self {
            sample_count: u32::try_from(sorted.len())
                .map_err(|_| PrivateLifeMeasurementError::SampleCountOverflow)?,
            median_micros: sorted[sorted.len() / 2],
            p95_micros: nearest_rank(&sorted, 95),
            maximum_micros: sorted[sorted.len() - 1],
        })
    }
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "the raw sample retains independent route, telemetry, and cleanup observations"
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrivateLifeJourneySampleV1 {
    pub journey_ordinal: u8,
    pub initial_character_id: [u8; 16],
    pub extraction_request_id: [u8; 16],
    pub extraction_receipt_id: [u8; 16],
    pub death_id: [u8; 16],
    pub successor_id: [u8; 16],
    pub successor_receipt_id: [u8; 16],
    pub successor_starter_item_uids: [[u8; 16]; 4],
    pub login_to_control_micros: u64,
    pub full_loop_micros: u64,
    pub death_to_successor_control_micros: u64,
    pub summary_to_successor_control_micros: u64,
    pub successor_control_to_combat_micros: u64,
    pub summary_to_combat_micros: u64,
    pub extraction_branch_complete: bool,
    pub death_summary_memorial_trace_complete: bool,
    pub successor_confirmations: u8,
    pub successor_combat_input_confirmed: bool,
    pub telemetry_session_closed: bool,
    pub runtime_zero_residue: bool,
    pub database_zero_residue: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateLifeJourneyAuthorityV1 {
    pub build_id: String,
    pub source_revision: String,
    pub schema_version: i64,
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub package_manifest_blake3: String,
    pub world_records_blake3: String,
    pub world_assets_blake3: String,
    pub world_localization_blake3: String,
    pub route_records_blake3: String,
    pub route_assets_blake3: String,
    pub route_localization_blake3: String,
    pub bargain_records_blake3: String,
    pub bargain_assets_blake3: String,
    pub bargain_localization_blake3: String,
    pub death_view_records_blake3: String,
    pub death_view_assets_blake3: String,
    pub death_view_localization_blake3: String,
}

impl PrivateLifeJourneyAuthorityV1 {
    fn validate(&self) -> Result<(), PrivateLifeMeasurementError> {
        if self.build_id.is_empty()
            || self.schema_version <= 0
            || self.protocol_major == 0
            || !valid_source_revision(&self.source_revision)
        {
            return Err(PrivateLifeMeasurementError::InvalidBuildAuthority);
        }
        for hash in self.hashes() {
            if !valid_blake3(hash) {
                return Err(PrivateLifeMeasurementError::InvalidBuildAuthority);
            }
        }
        Ok(())
    }

    fn hashes(&self) -> [&str; 13] {
        [
            &self.package_manifest_blake3,
            &self.world_records_blake3,
            &self.world_assets_blake3,
            &self.world_localization_blake3,
            &self.route_records_blake3,
            &self.route_assets_blake3,
            &self.route_localization_blake3,
            &self.bargain_records_blake3,
            &self.bargain_assets_blake3,
            &self.bargain_localization_blake3,
            &self.death_view_records_blake3,
            &self.death_view_assets_blake3,
            &self.death_view_localization_blake3,
        ]
    }
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "the report exposes each independent Roadmap gate instead of collapsing failures"
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrivateLifeJourneyEvidenceV1 {
    pub report_schema: &'static str,
    pub feature_id: &'static str,
    pub sample_scope: &'static str,
    pub behavioral_cohort_scope: &'static str,
    pub build_id: String,
    pub source_revision: String,
    pub schema_version: i64,
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub package_manifest_blake3: String,
    pub world_records_blake3: String,
    pub world_assets_blake3: String,
    pub world_localization_blake3: String,
    pub route_records_blake3: String,
    pub route_assets_blake3: String,
    pub route_localization_blake3: String,
    pub bargain_records_blake3: String,
    pub bargain_assets_blake3: String,
    pub bargain_localization_blake3: String,
    pub death_view_records_blake3: String,
    pub death_view_assets_blake3: String,
    pub death_view_localization_blake3: String,
    pub required_sample_count: usize,
    pub sample_count: usize,
    pub samples: Vec<PrivateLifeJourneySampleV1>,
    pub login_to_control_latency: PrivateLifeDurationStatsV1,
    pub full_loop_latency: PrivateLifeDurationStatsV1,
    pub death_to_successor_control_latency: PrivateLifeDurationStatsV1,
    pub summary_to_successor_control_latency: PrivateLifeDurationStatsV1,
    pub successor_control_to_combat_latency: PrivateLifeDurationStatsV1,
    pub summary_to_combat_latency: PrivateLifeDurationStatsV1,
    pub login_to_control_median_under_thirty_seconds: bool,
    pub death_to_successor_control_median_under_fifteen_seconds: bool,
    pub death_to_successor_control_p95_under_thirty_seconds: bool,
    pub successor_combat_within_two_minutes_count: usize,
    pub successor_combat_within_two_minutes_percent: u8,
    pub technical_successor_combat_meets_seventy_percent: bool,
    pub all_route_branches_complete: bool,
    pub every_successor_used_two_confirmations: bool,
    pub all_durable_identities_unique: bool,
    pub every_journey_zero_residue: bool,
    pub accepted: bool,
    pub raw_report_hash_blake3: String,
}

impl PrivateLifeJourneyEvidenceV1 {
    #[allow(
        clippy::too_many_lines,
        reason = "one hashable compiler derives every timing, route, identity, and cleanup gate"
    )]
    pub fn compile(
        mut samples: Vec<PrivateLifeJourneySampleV1>,
        authority: PrivateLifeJourneyAuthorityV1,
    ) -> Result<Self, PrivateLifeMeasurementError> {
        authority.validate()?;
        samples.sort_by_key(|sample| sample.journey_ordinal);
        let exact_ordinals = samples.len() == REQUIRED_PRIVATE_LIFE_JOURNEY_COUNT
            && samples.iter().enumerate().all(|(index, sample)| {
                sample.journey_ordinal == u8::try_from(index + 1).unwrap_or(0)
            });
        let durations = |field: fn(&PrivateLifeJourneySampleV1) -> u64| {
            samples.iter().map(field).collect::<Vec<_>>()
        };
        let login_to_control_latency = PrivateLifeDurationStatsV1::compile(&durations(|sample| {
            sample.login_to_control_micros
        }))?;
        let full_loop_latency = PrivateLifeDurationStatsV1::compile(&durations(|sample| {
            sample.full_loop_micros
        }))?;
        let death_to_successor_control_latency =
            PrivateLifeDurationStatsV1::compile(&durations(|sample| {
                sample.death_to_successor_control_micros
            }))?;
        let summary_to_successor_control_latency =
            PrivateLifeDurationStatsV1::compile(&durations(|sample| {
                sample.summary_to_successor_control_micros
            }))?;
        let successor_control_to_combat_latency =
            PrivateLifeDurationStatsV1::compile(&durations(|sample| {
                sample.successor_control_to_combat_micros
            }))?;
        let summary_to_combat_latency =
            PrivateLifeDurationStatsV1::compile(&durations(|sample| {
                sample.summary_to_combat_micros
            }))?;
        let positive_ordered_durations = samples.iter().all(|sample| {
            sample.login_to_control_micros > 0
                && sample.full_loop_micros >= sample.login_to_control_micros
                && sample.death_to_successor_control_micros > 0
                && sample.summary_to_successor_control_micros > 0
                && sample.successor_control_to_combat_micros > 0
                && sample.summary_to_combat_micros > 0
                && sample.death_to_successor_control_micros
                    >= sample.summary_to_successor_control_micros
                && sample.summary_to_combat_micros
                    >= sample.summary_to_successor_control_micros
                && sample.summary_to_combat_micros
                    >= sample.successor_control_to_combat_micros
        });
        let login_to_control_median_under_thirty_seconds =
            login_to_control_latency.median_micros < LOGIN_TO_CONTROL_MEDIAN_LIMIT_MICROS;
        let death_to_successor_control_median_under_fifteen_seconds =
            death_to_successor_control_latency.median_micros
                < DEATH_TO_CONTROL_MEDIAN_LIMIT_MICROS;
        let death_to_successor_control_p95_under_thirty_seconds =
            death_to_successor_control_latency.p95_micros < DEATH_TO_CONTROL_P95_LIMIT_MICROS;
        let successor_combat_within_two_minutes_count = samples
            .iter()
            .filter(|sample| sample.summary_to_combat_micros <= SUMMARY_TO_COMBAT_LIMIT_MICROS)
            .count();
        let technical_successor_combat_meets_seventy_percent = !samples.is_empty()
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
        let all_route_branches_complete = samples.iter().all(|sample| {
            sample.extraction_branch_complete
                && sample.death_summary_memorial_trace_complete
                && sample.successor_combat_input_confirmed
                && sample.telemetry_session_closed
        });
        let every_successor_used_two_confirmations = samples
            .iter()
            .all(|sample| sample.successor_confirmations == 2);
        let all_durable_identities_unique = ids_are_unique(&samples);
        let every_journey_zero_residue = samples
            .iter()
            .all(|sample| sample.runtime_zero_residue && sample.database_zero_residue);
        let accepted = exact_ordinals
            && positive_ordered_durations
            && login_to_control_median_under_thirty_seconds
            && death_to_successor_control_median_under_fifteen_seconds
            && death_to_successor_control_p95_under_thirty_seconds
            && technical_successor_combat_meets_seventy_percent
            && all_route_branches_complete
            && every_successor_used_two_confirmations
            && all_durable_identities_unique
            && every_journey_zero_residue;
        let mut report = Self {
            report_schema: "gravebound.performance.gb-m03.private-life-full-loop.v1",
            feature_id: "GB-M03-03",
            sample_scope: "automated-authenticated-public-quic-character-select-hall-b0-b6-extraction-death-memorial-successor-combat",
            behavioral_cohort_scope: "not-measured-human-private-cohort-remains-a-separate-m03-gate",
            build_id: authority.build_id,
            source_revision: authority.source_revision,
            schema_version: authority.schema_version,
            protocol_major: authority.protocol_major,
            protocol_minor: authority.protocol_minor,
            package_manifest_blake3: authority.package_manifest_blake3,
            world_records_blake3: authority.world_records_blake3,
            world_assets_blake3: authority.world_assets_blake3,
            world_localization_blake3: authority.world_localization_blake3,
            route_records_blake3: authority.route_records_blake3,
            route_assets_blake3: authority.route_assets_blake3,
            route_localization_blake3: authority.route_localization_blake3,
            bargain_records_blake3: authority.bargain_records_blake3,
            bargain_assets_blake3: authority.bargain_assets_blake3,
            bargain_localization_blake3: authority.bargain_localization_blake3,
            death_view_records_blake3: authority.death_view_records_blake3,
            death_view_assets_blake3: authority.death_view_assets_blake3,
            death_view_localization_blake3: authority.death_view_localization_blake3,
            required_sample_count: REQUIRED_PRIVATE_LIFE_JOURNEY_COUNT,
            sample_count: samples.len(),
            samples,
            login_to_control_latency,
            full_loop_latency,
            death_to_successor_control_latency,
            summary_to_successor_control_latency,
            successor_control_to_combat_latency,
            summary_to_combat_latency,
            login_to_control_median_under_thirty_seconds,
            death_to_successor_control_median_under_fifteen_seconds,
            death_to_successor_control_p95_under_thirty_seconds,
            successor_combat_within_two_minutes_count,
            successor_combat_within_two_minutes_percent,
            technical_successor_combat_meets_seventy_percent,
            all_route_branches_complete,
            every_successor_used_two_confirmations,
            all_durable_identities_unique,
            every_journey_zero_residue,
            accepted,
            raw_report_hash_blake3: String::new(),
        };
        report.raw_report_hash_blake3 = report_hash(&report)?;
        Ok(report)
    }

    pub fn write_json_atomically(
        &self,
        path: &Path,
    ) -> Result<(), PrivateLifeMeasurementError> {
        if self.raw_report_hash_blake3 != report_hash(self)? {
            return Err(PrivateLifeMeasurementError::ReportHashMismatch);
        }
        if path.exists() {
            return Err(PrivateLifeMeasurementError::EvidenceIo(format!(
                "destination already exists: {}",
                path.display()
            )));
        }
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .map_err(|error| PrivateLifeMeasurementError::EvidenceIo(error.to_string()))?;
        }
        let temporary = partial_path(path);
        let result = (|| -> Result<(), PrivateLifeMeasurementError> {
            let bytes = serde_json::to_vec_pretty(self)
                .map_err(|error| PrivateLifeMeasurementError::Serialization(error.to_string()))?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|error| PrivateLifeMeasurementError::EvidenceIo(error.to_string()))?;
            file.write_all(&bytes)
                .map_err(|error| PrivateLifeMeasurementError::EvidenceIo(error.to_string()))?;
            file.sync_all()
                .map_err(|error| PrivateLifeMeasurementError::EvidenceIo(error.to_string()))?;
            fs::rename(&temporary, path)
                .map_err(|error| PrivateLifeMeasurementError::EvidenceIo(error.to_string()))
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}

fn ids_are_unique(samples: &[PrivateLifeJourneySampleV1]) -> bool {
    let expected = samples.len().saturating_mul(10);
    let ids = samples
        .iter()
        .flat_map(|sample| {
            [
                sample.initial_character_id,
                sample.extraction_request_id,
                sample.extraction_receipt_id,
                sample.death_id,
                sample.successor_id,
                sample.successor_receipt_id,
                sample.successor_starter_item_uids[0],
                sample.successor_starter_item_uids[1],
                sample.successor_starter_item_uids[2],
                sample.successor_starter_item_uids[3],
            ]
        })
        .collect::<BTreeSet<_>>();
    ids.len() == expected && ids.iter().all(|id| *id != [0; 16])
}

fn report_hash(
    report: &PrivateLifeJourneyEvidenceV1,
) -> Result<String, PrivateLifeMeasurementError> {
    let mut hashable = report.clone();
    hashable.raw_report_hash_blake3.clear();
    let bytes = serde_json::to_vec(&hashable)
        .map_err(|error| PrivateLifeMeasurementError::Serialization(error.to_string()))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn valid_source_revision(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_blake3(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
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
pub enum PrivateLifeMeasurementError {
    #[error("at least one private-life duration sample is required")]
    MissingDurationSamples,
    #[error("private-life duration sample count exceeds the evidence representation")]
    SampleCountOverflow,
    #[error("private-life build/content authority is invalid")]
    InvalidBuildAuthority,
    #[error("private-life report serialization failed: {0}")]
    Serialization(String),
    #[error("private-life report hash does not match its payload")]
    ReportHashMismatch,
    #[error("private-life evidence I/O failed: {0}")]
    EvidenceIo(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authority() -> PrivateLifeJourneyAuthorityV1 {
        PrivateLifeJourneyAuthorityV1 {
            build_id: "core-dev".into(),
            source_revision: "1".repeat(40),
            schema_version: 79,
            protocol_major: 1,
            protocol_minor: 25,
            package_manifest_blake3: "0".repeat(64),
            world_records_blake3: "1".repeat(64),
            world_assets_blake3: "2".repeat(64),
            world_localization_blake3: "3".repeat(64),
            route_records_blake3: "4".repeat(64),
            route_assets_blake3: "5".repeat(64),
            route_localization_blake3: "6".repeat(64),
            bargain_records_blake3: "7".repeat(64),
            bargain_assets_blake3: "8".repeat(64),
            bargain_localization_blake3: "9".repeat(64),
            death_view_records_blake3: "a".repeat(64),
            death_view_assets_blake3: "b".repeat(64),
            death_view_localization_blake3: "c".repeat(64),
        }
    }

    fn id(kind: u8, ordinal: u8) -> [u8; 16] {
        let mut id = [kind; 16];
        id[0] = kind;
        id[1] = ordinal;
        id
    }

    fn accepted_samples() -> Vec<PrivateLifeJourneySampleV1> {
        (1_u8..=25)
            .map(|ordinal| PrivateLifeJourneySampleV1 {
                journey_ordinal: ordinal,
                initial_character_id: id(1, ordinal),
                extraction_request_id: id(2, ordinal),
                extraction_receipt_id: id(3, ordinal),
                death_id: id(4, ordinal),
                successor_id: id(5, ordinal),
                successor_receipt_id: id(6, ordinal),
                successor_starter_item_uids: [
                    id(7, ordinal),
                    id(8, ordinal),
                    id(9, ordinal),
                    id(10, ordinal),
                ],
                login_to_control_micros: 1_000_000,
                full_loop_micros: 60_000_000,
                death_to_successor_control_micros: 2_000_000,
                summary_to_successor_control_micros: 1_500_000,
                successor_control_to_combat_micros: 1_000_000,
                summary_to_combat_micros: 2_500_000,
                extraction_branch_complete: true,
                death_summary_memorial_trace_complete: true,
                successor_confirmations: 2,
                successor_combat_input_confirmed: true,
                telemetry_session_closed: true,
                runtime_zero_residue: true,
                database_zero_residue: true,
            })
            .collect()
    }

    #[test]
    fn report_requires_exact_unique_full_loops_and_hashes_them() {
        let accepted =
            PrivateLifeJourneyEvidenceV1::compile(accepted_samples(), authority()).unwrap();
        assert!(accepted.accepted);
        assert_eq!(
            accepted.sample_count,
            REQUIRED_PRIVATE_LIFE_JOURNEY_COUNT
        );
        assert_eq!(accepted.successor_combat_within_two_minutes_percent, 100);
        assert_eq!(accepted.raw_report_hash_blake3.len(), 64);
        assert_eq!(
            accepted.raw_report_hash_blake3,
            PrivateLifeJourneyEvidenceV1::compile(accepted_samples(), authority())
                .unwrap()
                .raw_report_hash_blake3
        );

        let mut duplicate = accepted_samples();
        duplicate[24].death_id = duplicate[0].death_id;
        assert!(
            !PrivateLifeJourneyEvidenceV1::compile(duplicate, authority())
                .unwrap()
                .accepted
        );

        let mut slow = accepted_samples();
        for sample in &mut slow {
            sample.login_to_control_micros = LOGIN_TO_CONTROL_MEDIAN_LIMIT_MICROS;
        }
        assert!(
            !PrivateLifeJourneyEvidenceV1::compile(slow, authority())
                .unwrap()
                .accepted
        );
    }

    #[test]
    fn report_publication_is_atomic_new_only_and_hash_checked() {
        let report =
            PrivateLifeJourneyEvidenceV1::compile(accepted_samples(), authority()).unwrap();
        let root = std::env::temp_dir().join(format!(
            "gravebound-private-life-report-{}-{}",
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
            Err(PrivateLifeMeasurementError::EvidenceIo(_))
        ));

        let mut tampered = report;
        tampered.sample_count = 24;
        assert_eq!(
            tampered.write_json_atomically(&root.join("tampered.json")),
            Err(PrivateLifeMeasurementError::ReportHashMismatch)
        );
        fs::remove_dir_all(root).unwrap();
    }
}
