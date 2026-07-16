//! Reusable `GB-M03-06E` timing, cleanup, and memory evidence primitives.
//!
//! Authorities:
//! - canonical GDD `DTH-001`, `DTH-021`, `TECH-022`, and `TECH-023`;
//! - Content Production Spec `CONT-ECHO-009` and `CONT-HUB-002`;
//! - Development Roadmap `GB-M03-06`, `GB-M03-13`, and the M03 exit gate.
//!
//! This module performs evidence arithmetic and bounded inspection only. It does not create a
//! death, infer an Echo outcome, or become an alternate gameplay writer.

use std::time::Duration;

use persistence::PostgresPersistence;
use serde::Serialize;
use sqlx::Row;
use sysinfo::{Pid, ProcessesToUpdate, System, get_current_pid};
use thiserror::Error;

const REQUIRED_MEMORY_DURATION_MS: u64 = 30 * 60 * 1_000;

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
                 AND xact_start IS NOT NULL) AS active_transactions,\
             (SELECT count(*) FROM pg_stat_activity \
               WHERE datname=current_database() AND pid<>pg_backend_pid() \
                 AND state='idle in transaction') AS idle_transactions,\
             (SELECT count(*) FROM pg_stat_activity \
               WHERE datname=current_database() AND pid<>pg_backend_pid() \
                 AND state='idle in transaction (aborted)') AS aborted_transactions,\
             (SELECT count(*) FROM pg_locks AS held \
               JOIN pg_stat_activity AS activity ON activity.pid=held.pid \
               WHERE activity.datname=current_database() AND held.pid<>pg_backend_pid() \
                 AND NOT held.granted) AS waiting_locks,\
             (SELECT count(*) FROM pg_locks AS held \
               JOIN pg_stat_activity AS activity ON activity.pid=held.pid \
               WHERE activity.datname=current_database() AND held.pid<>pg_backend_pid() \
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
