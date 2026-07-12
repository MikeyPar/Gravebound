//! Deterministic `GB-M01-09` stress and reliability fixtures.
//!
//! This module owns workload construction and evidence arithmetic, not rendering or operating-
//! system instrumentation. A report is accepted only when its caller supplies rendered-frame and
//! resident-memory samples collected on the canonical target hardware.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    AimVector, BellProctorSimulation, BellProctorStateKind, BossEvent, BossInput, TICKS_PER_SECOND,
    Tick,
};

pub const TARGET_HOSTILE_PROJECTILE_COUNT: usize = 800;
pub const TARGET_ENEMY_COUNT: usize = 40;
pub const BOSS_REPLAY_TICKS: u64 = TICKS_PER_SECOND as u64 * 60;
pub const BOSS_RELIABILITY_RUN_COUNT: usize = 20;
const TARGET_MEMORY_BYTES: u64 = 1_500_000_000;
const REQUIRED_MEMORY_DURATION_MS: u64 = 30 * 60 * 1_000;
pub const MONOTONIC_GROWTH_FLOOR_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectMode {
    Full,
    Reduced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameSampleKind {
    RenderedFrame,
    SimulationTick,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetHardware {
    pub operating_system: String,
    pub cpu: String,
    pub memory_bytes: u64,
    pub gpu: String,
    pub width_pixels: u32,
    pub height_pixels: u32,
}

impl TargetHardware {
    #[must_use]
    pub fn canonical_description() -> Self {
        Self {
            operating_system: "Windows 10/11".to_owned(),
            cpu: "4-core 3.0 GHz-class CPU".to_owned(),
            memory_bytes: 8_000_000_000,
            gpu: "GTX 1050-class GPU".to_owned(),
            width_pixels: 1_920,
            height_pixels: 1_080,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StressFixtureConfig {
    pub seed: u64,
    pub effect_mode: EffectMode,
}

impl Default for StressFixtureConfig {
    fn default() -> Self {
        Self {
            seed: 0x4742_4D30_312D_3039,
            effect_mode: EffectMode::Full,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StressProjectile {
    id: u32,
    x_milli_tiles: i32,
    y_milli_tiles: i32,
    velocity_x_milli_tiles_per_tick: i16,
    velocity_y_milli_tiles_per_tick: i16,
    threat_priority: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StressEnemy {
    id: u16,
    x_milli_tiles: i32,
    y_milli_tiles: i32,
    phase: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StressFixture {
    seed: u64,
    effect_mode: EffectMode,
    tick: Tick,
    projectiles: Vec<StressProjectile>,
    enemies: Vec<StressEnemy>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StressFixtureSnapshot {
    pub tick: Tick,
    pub hostile_projectiles: usize,
    pub enemies: usize,
    pub hostile_telegraphs_retained: bool,
    pub state_hash_blake3: String,
}

impl StressFixture {
    #[must_use]
    pub fn new(config: StressFixtureConfig) -> Self {
        let mut state = config.seed;
        let projectiles = (0..TARGET_HOSTILE_PROJECTILE_COUNT)
            .map(|ordinal| {
                let random = splitmix64(&mut state);
                let x = 1_000 + i32::try_from(random % 30_000).expect("bounded x");
                let y = 1_000 + i32::try_from((random >> 16) % 22_000).expect("bounded y");
                let speed_x = 35 + i16::try_from((random >> 32) % 90).expect("bounded vx");
                let speed_y = 25 + i16::try_from((random >> 48) % 70).expect("bounded vy");
                StressProjectile {
                    id: u32::try_from(ordinal + 1).expect("projectile ID"),
                    x_milli_tiles: x,
                    y_milli_tiles: y,
                    velocity_x_milli_tiles_per_tick: if ordinal % 2 == 0 {
                        speed_x
                    } else {
                        -speed_x
                    },
                    velocity_y_milli_tiles_per_tick: if ordinal % 3 == 0 {
                        speed_y
                    } else {
                        -speed_y
                    },
                    threat_priority: u8::try_from(ordinal % 13).expect("threat") + 1,
                }
            })
            .collect();
        let enemies = (0..TARGET_ENEMY_COUNT)
            .map(|ordinal| {
                let column = ordinal % 10;
                let row = ordinal / 10;
                StressEnemy {
                    id: u16::try_from(ordinal + 1).expect("enemy ID"),
                    x_milli_tiles: 2_000 + i32::try_from(column * 3_000).expect("enemy x"),
                    y_milli_tiles: 3_000 + i32::try_from(row * 5_000).expect("enemy y"),
                    phase: u16::try_from(ordinal * 97).expect("enemy phase"),
                }
            })
            .collect();
        Self {
            seed: config.seed,
            effect_mode: config.effect_mode,
            tick: Tick(0),
            projectiles,
            enemies,
        }
    }

    pub fn advance(&mut self) {
        self.tick = Tick(self.tick.0.saturating_add(1));
        for projectile in &mut self.projectiles {
            projectile.x_milli_tiles += i32::from(projectile.velocity_x_milli_tiles_per_tick);
            projectile.y_milli_tiles += i32::from(projectile.velocity_y_milli_tiles_per_tick);
            wrap_coordinate(&mut projectile.x_milli_tiles, 1_000, 31_000);
            wrap_coordinate(&mut projectile.y_milli_tiles, 1_000, 23_000);
        }
        for enemy in &mut self.enemies {
            enemy.phase = enemy.phase.wrapping_add(1);
            let direction = if (enemy.phase / 90) % 2 == 0 { 1 } else { -1 };
            enemy.x_milli_tiles += direction * 3;
        }
    }

    #[must_use]
    pub const fn effect_mode(&self) -> EffectMode {
        self.effect_mode
    }

    #[must_use]
    pub fn projectile_position_milli_tiles(&self, index: usize) -> Option<(i32, i32)> {
        self.projectiles
            .get(index)
            .map(|projectile| (projectile.x_milli_tiles, projectile.y_milli_tiles))
    }

    #[must_use]
    pub fn projectile_threat_priority(&self, index: usize) -> Option<u8> {
        self.projectiles
            .get(index)
            .map(|projectile| projectile.threat_priority)
    }

    #[must_use]
    pub fn enemy_position_milli_tiles(&self, index: usize) -> Option<(i32, i32)> {
        self.enemies
            .get(index)
            .map(|enemy| (enemy.x_milli_tiles, enemy.y_milli_tiles))
    }

    #[must_use]
    pub fn snapshot(&self) -> StressFixtureSnapshot {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"gravebound-gb-m01-09-stress-v1\0");
        hasher.update(&self.seed.to_le_bytes());
        hasher.update(&[match self.effect_mode {
            EffectMode::Full => 0,
            EffectMode::Reduced => 1,
        }]);
        hasher.update(&self.tick.0.to_le_bytes());
        for enemy in &self.enemies {
            hasher.update(&enemy.id.to_le_bytes());
            hasher.update(&enemy.x_milli_tiles.to_le_bytes());
            hasher.update(&enemy.y_milli_tiles.to_le_bytes());
            hasher.update(&enemy.phase.to_le_bytes());
        }
        for projectile in &self.projectiles {
            hasher.update(&projectile.id.to_le_bytes());
            hasher.update(&projectile.x_milli_tiles.to_le_bytes());
            hasher.update(&projectile.y_milli_tiles.to_le_bytes());
            hasher.update(&projectile.velocity_x_milli_tiles_per_tick.to_le_bytes());
            hasher.update(&projectile.velocity_y_milli_tiles_per_tick.to_le_bytes());
            hasher.update(&[projectile.threat_priority]);
        }
        StressFixtureSnapshot {
            tick: self.tick,
            hostile_projectiles: self.projectiles.len(),
            enemies: self.enemies.len(),
            hostile_telegraphs_retained: true,
            state_hash_blake3: hasher.finalize().to_hex().to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemorySample {
    pub elapsed_ms: u64,
    pub resident_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryAssessment {
    InsufficientDuration,
    OverBudget,
    MonotonicGrowth,
    Pass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceAcceptance {
    Pass,
    FrameBudgetFailed,
    MemoryFailed,
    UnverifiedTargetHardware,
    UnverifiedSimulationOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerformanceEvidenceInput {
    pub build_id: String,
    pub bundle_id: String,
    pub fixture_seed: u64,
    pub duration_ms: u64,
    pub peak_hostile_projectiles: usize,
    pub peak_enemies: usize,
    pub effect_mode: EffectMode,
    pub frame_sample_kind: FrameSampleKind,
    pub frame_times_micros: Vec<u64>,
    pub memory_samples: Vec<MemorySample>,
    pub target_hardware: TargetHardware,
    /// Set only after the operator verifies the recorded machine meets or exceeds TECH-070.
    pub target_class_verified: bool,
    pub hostile_telegraphs_retained: bool,
    pub culled_effect_priorities: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerformanceEvidenceReport {
    pub report_schema: String,
    pub build_id: String,
    pub bundle_id: String,
    pub fixture_seed: u64,
    pub duration_ms: u64,
    pub peak_hostile_projectiles: usize,
    pub peak_enemies: usize,
    pub effect_mode: EffectMode,
    pub frame_sample_kind: FrameSampleKind,
    pub frame_sample_count: usize,
    pub measured_fps_milli: u64,
    pub p95_frame_time_micros: u64,
    pub p99_frame_time_micros: u64,
    pub memory_samples: Vec<MemorySample>,
    pub peak_resident_bytes: u64,
    pub memory_assessment: MemoryAssessment,
    pub target_hardware: TargetHardware,
    pub target_class_verified: bool,
    pub hostile_telegraphs_retained: bool,
    pub culled_effect_priorities: Vec<u8>,
    pub acceptance: PerformanceAcceptance,
    pub raw_report_hash_blake3: String,
}

impl PerformanceEvidenceReport {
    pub fn compile(input: PerformanceEvidenceInput) -> Result<Self, PerformanceReportError> {
        validate_evidence_input(&input)?;
        let mut sorted_frame_times = input.frame_times_micros.clone();
        sorted_frame_times.sort_unstable();
        let p95 = nearest_rank(&sorted_frame_times, 95);
        let p99 = nearest_rank(&sorted_frame_times, 99);
        let total_micros: u128 = sorted_frame_times
            .iter()
            .map(|value| u128::from(*value))
            .sum();
        let frame_count = u128::try_from(sorted_frame_times.len()).expect("frame count");
        let measured_fps_milli = u64::try_from(
            frame_count
                .saturating_mul(1_000_000_000)
                .checked_div(total_micros)
                .ok_or(PerformanceReportError::ZeroFrameDuration)?,
        )
        .map_err(|_| PerformanceReportError::ArithmeticOverflow)?;
        let peak_resident_bytes = input
            .memory_samples
            .iter()
            .map(|sample| sample.resident_bytes)
            .max()
            .unwrap_or(0);
        let memory_assessment = assess_memory(&input.memory_samples, peak_resident_bytes);
        let frames_pass = measured_fps_milli >= 60_000 && p95 <= 16_700 && p99 <= 33_300;
        let acceptance = if input.frame_sample_kind != FrameSampleKind::RenderedFrame {
            PerformanceAcceptance::UnverifiedSimulationOnly
        } else if !input.target_class_verified
            || input.target_hardware.width_pixels != 1_920
            || input.target_hardware.height_pixels != 1_080
        {
            PerformanceAcceptance::UnverifiedTargetHardware
        } else if memory_assessment != MemoryAssessment::Pass {
            PerformanceAcceptance::MemoryFailed
        } else if !frames_pass
            || input.peak_hostile_projectiles < TARGET_HOSTILE_PROJECTILE_COUNT
            || input.peak_enemies < TARGET_ENEMY_COUNT
            || !input.hostile_telegraphs_retained
        {
            PerformanceAcceptance::FrameBudgetFailed
        } else {
            PerformanceAcceptance::Pass
        };
        let mut report = Self {
            report_schema: "gravebound.performance.gb-m01-09.v1".to_owned(),
            build_id: input.build_id,
            bundle_id: input.bundle_id,
            fixture_seed: input.fixture_seed,
            duration_ms: input.duration_ms,
            peak_hostile_projectiles: input.peak_hostile_projectiles,
            peak_enemies: input.peak_enemies,
            effect_mode: input.effect_mode,
            frame_sample_kind: input.frame_sample_kind,
            frame_sample_count: input.frame_times_micros.len(),
            measured_fps_milli,
            p95_frame_time_micros: p95,
            p99_frame_time_micros: p99,
            memory_samples: input.memory_samples,
            peak_resident_bytes,
            memory_assessment,
            target_hardware: input.target_hardware,
            target_class_verified: input.target_class_verified,
            hostile_telegraphs_retained: input.hostile_telegraphs_retained,
            culled_effect_priorities: input.culled_effect_priorities,
            acceptance,
            raw_report_hash_blake3: String::new(),
        };
        report.raw_report_hash_blake3 = hash_report(&report)?;
        Ok(report)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BossReliabilityReport {
    pub replay_ticks: u64,
    pub replay_hash_blake3: String,
    pub completed_runs: usize,
    pub unique_run_hashes: usize,
    pub reference_run_hash_blake3: String,
}

pub fn run_bell_proctor_reliability_fixture()
-> Result<BossReliabilityReport, PerformanceReportError> {
    let replay_hash = run_boss_schedule(BOSS_REPLAY_TICKS, false)?;
    if replay_hash != run_boss_schedule(BOSS_REPLAY_TICKS, false)? {
        return Err(PerformanceReportError::BossReplayDiverged);
    }
    let hashes = (0..BOSS_RELIABILITY_RUN_COUNT)
        .map(|_| run_boss_schedule(2_700, true))
        .collect::<Result<Vec<_>, _>>()?;
    let unique = hashes.iter().collect::<BTreeSet<_>>().len();
    if unique != 1 {
        return Err(PerformanceReportError::BossRunsDiverged);
    }
    Ok(BossReliabilityReport {
        replay_ticks: BOSS_REPLAY_TICKS,
        replay_hash_blake3: replay_hash,
        completed_runs: hashes.len(),
        unique_run_hashes: unique,
        reference_run_hash_blake3: hashes[0].clone(),
    })
}

fn run_boss_schedule(ticks: u64, defeat_at_end: bool) -> Result<String, PerformanceReportError> {
    let mut simulation = BellProctorSimulation::first_playable();
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"gravebound-gb-m01-09-boss-replay-v1\0");
    let mut defeated = false;
    for tick in 0..ticks {
        let health = if defeat_at_end && tick + 1 == ticks {
            0
        } else if tick >= 1_800 {
            550
        } else if tick >= 1_200 {
            1_000
        } else if tick >= 600 {
            2_000
        } else {
            3_000
        };
        let events = simulation
            .advance(BossInput {
                current_health: health,
                target_aim: AimVector::EAST,
            })
            .map_err(|error| PerformanceReportError::BossRuntime(error.to_string()))?;
        hasher.update(&tick.to_le_bytes());
        hasher.update(&health.to_le_bytes());
        for event in events {
            if matches!(event, BossEvent::BossDefeated { .. }) {
                defeated = true;
            }
            hasher.update(format!("{event:?}").as_bytes());
            hasher.update(&[0]);
        }
    }
    if defeat_at_end && (!defeated || simulation.state() != BellProctorStateKind::Defeated) {
        return Err(PerformanceReportError::BossDidNotComplete);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn validate_evidence_input(input: &PerformanceEvidenceInput) -> Result<(), PerformanceReportError> {
    if input.build_id.trim().is_empty() || input.bundle_id.trim().is_empty() {
        return Err(PerformanceReportError::MissingIdentity);
    }
    if input.duration_ms == 0 {
        return Err(PerformanceReportError::ZeroDuration);
    }
    if input.frame_times_micros.is_empty() {
        return Err(PerformanceReportError::MissingFrameSamples);
    }
    if input.frame_times_micros.contains(&0) {
        return Err(PerformanceReportError::ZeroFrameDuration);
    }
    if input
        .memory_samples
        .windows(2)
        .any(|pair| pair[0].elapsed_ms >= pair[1].elapsed_ms)
    {
        return Err(PerformanceReportError::MemorySamplesOutOfOrder);
    }
    if input
        .memory_samples
        .last()
        .is_some_and(|sample| sample.elapsed_ms > input.duration_ms)
    {
        return Err(PerformanceReportError::MemorySampleOutsideDuration);
    }
    let valid_culling = match input.effect_mode {
        EffectMode::Full => input.culled_effect_priorities.is_empty(),
        EffectMode::Reduced => {
            matches!(input.culled_effect_priorities.as_slice(), [] | [5] | [5, 4])
        }
    };
    if !valid_culling {
        return Err(PerformanceReportError::InvalidEffectCulling);
    }
    Ok(())
}

fn assess_memory(samples: &[MemorySample], peak: u64) -> MemoryAssessment {
    let duration = samples.last().map_or(0, |sample| sample.elapsed_ms)
        - samples.first().map_or(0, |sample| sample.elapsed_ms);
    if duration < REQUIRED_MEMORY_DURATION_MS {
        return MemoryAssessment::InsufficientDuration;
    }
    if peak > TARGET_MEMORY_BYTES {
        return MemoryAssessment::OverBudget;
    }
    let growth = samples
        .last()
        .map_or(0, |sample| sample.resident_bytes)
        .saturating_sub(samples.first().map_or(0, |sample| sample.resident_bytes));
    if growth >= MONOTONIC_GROWTH_FLOOR_BYTES
        && samples
            .windows(2)
            .all(|pair| pair[0].resident_bytes < pair[1].resident_bytes)
    {
        return MemoryAssessment::MonotonicGrowth;
    }
    MemoryAssessment::Pass
}

fn nearest_rank(sorted_values: &[u64], percentile: usize) -> u64 {
    let rank = sorted_values.len().saturating_mul(percentile).div_ceil(100);
    sorted_values[rank.saturating_sub(1)]
}

fn hash_report(report: &PerformanceEvidenceReport) -> Result<String, PerformanceReportError> {
    let mut canonical = report.clone();
    canonical.raw_report_hash_blake3.clear();
    let bytes = serde_json::to_vec(&canonical)
        .map_err(|error| PerformanceReportError::Serialization(error.to_string()))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut value = *state;
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

fn wrap_coordinate(value: &mut i32, minimum: i32, maximum: i32) {
    if *value < minimum {
        *value = maximum - (minimum - *value - 1);
    } else if *value > maximum {
        *value = minimum + (*value - maximum - 1);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PerformanceReportError {
    #[error("build ID and bundle ID are required")]
    MissingIdentity,
    #[error("report duration must be positive")]
    ZeroDuration,
    #[error("at least one frame sample is required")]
    MissingFrameSamples,
    #[error("frame duration must be positive")]
    ZeroFrameDuration,
    #[error("memory samples must be strictly ordered by elapsed time")]
    MemorySamplesOutOfOrder,
    #[error("memory sample elapsed time exceeds report duration")]
    MemorySampleOutsideDuration,
    #[error("reduced effects may cull only priority 5 and then priority 4")]
    InvalidEffectCulling,
    #[error("performance arithmetic overflowed")]
    ArithmeticOverflow,
    #[error("report serialization failed: {0}")]
    Serialization(String),
    #[error("Bell Proctor runtime failed: {0}")]
    BossRuntime(String),
    #[error("60-second Bell Proctor replay diverged")]
    BossReplayDiverged,
    #[error("20 Bell Proctor runs produced inconsistent hashes")]
    BossRunsDiverged,
    #[error("a Bell Proctor reliability run did not complete")]
    BossDidNotComplete,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stress_fixture_is_exact_and_replays_for_sixty_seconds() {
        let mut first = StressFixture::new(StressFixtureConfig::default());
        let mut second = StressFixture::new(StressFixtureConfig::default());
        assert_eq!(first.snapshot().hostile_projectiles, 800);
        assert_eq!(first.snapshot().enemies, 40);
        for _ in 0..BOSS_REPLAY_TICKS {
            first.advance();
            second.advance();
        }
        let snapshot = first.snapshot();
        assert_eq!(snapshot, second.snapshot());
        assert_eq!(snapshot.tick, Tick(BOSS_REPLAY_TICKS));
        assert!(snapshot.hostile_telegraphs_retained);
        assert_eq!(
            snapshot.state_hash_blake3,
            "7dac4876dea54c1e12d5a86febf8f2f33206e4f2ffa6f78c27195442bd3b975a"
        );
    }

    #[test]
    fn report_computes_nearest_rank_and_refuses_simulation_only_acceptance() {
        let report = PerformanceEvidenceReport::compile(PerformanceEvidenceInput {
            build_id: "local-test".to_owned(),
            bundle_id: "fp.1.0.0".to_owned(),
            fixture_seed: StressFixtureConfig::default().seed,
            duration_ms: 1_000,
            peak_hostile_projectiles: 800,
            peak_enemies: 40,
            effect_mode: EffectMode::Full,
            frame_sample_kind: FrameSampleKind::SimulationTick,
            frame_times_micros: vec![10_000, 11_000, 12_000, 13_000, 14_000],
            memory_samples: Vec::new(),
            target_hardware: TargetHardware::canonical_description(),
            target_class_verified: true,
            hostile_telegraphs_retained: true,
            culled_effect_priorities: Vec::new(),
        })
        .expect("report");
        assert_eq!(report.p95_frame_time_micros, 14_000);
        assert_eq!(report.p99_frame_time_micros, 14_000);
        assert_eq!(
            report.acceptance,
            PerformanceAcceptance::UnverifiedSimulationOnly
        );
        assert_eq!(report.raw_report_hash_blake3.len(), 64);
    }

    #[test]
    fn canonical_render_and_memory_samples_can_pass() {
        let report = PerformanceEvidenceReport::compile(PerformanceEvidenceInput {
            build_id: "immutable-build".to_owned(),
            bundle_id: "fp.1.0.0".to_owned(),
            fixture_seed: 7,
            duration_ms: REQUIRED_MEMORY_DURATION_MS,
            peak_hostile_projectiles: 800,
            peak_enemies: 40,
            effect_mode: EffectMode::Full,
            frame_sample_kind: FrameSampleKind::RenderedFrame,
            frame_times_micros: vec![16_000; 120],
            memory_samples: vec![
                MemorySample {
                    elapsed_ms: 0,
                    resident_bytes: 400_000_000,
                },
                MemorySample {
                    elapsed_ms: 900_000,
                    resident_bytes: 420_000_000,
                },
                MemorySample {
                    elapsed_ms: REQUIRED_MEMORY_DURATION_MS,
                    resident_bytes: 415_000_000,
                },
            ],
            target_hardware: TargetHardware::canonical_description(),
            target_class_verified: true,
            hostile_telegraphs_retained: true,
            culled_effect_priorities: Vec::new(),
        })
        .expect("report");
        assert_eq!(report.memory_assessment, MemoryAssessment::Pass);
        assert_eq!(report.acceptance, PerformanceAcceptance::Pass);
    }

    #[test]
    fn memory_gate_detects_short_runs_budget_and_monotonic_growth() {
        assert_eq!(
            assess_memory(
                &[
                    MemorySample {
                        elapsed_ms: 0,
                        resident_bytes: 100,
                    },
                    MemorySample {
                        elapsed_ms: 10,
                        resident_bytes: 100,
                    },
                ],
                100,
            ),
            MemoryAssessment::InsufficientDuration
        );
        let monotonic = [
            MemorySample {
                elapsed_ms: 0,
                resident_bytes: 100_000_000,
            },
            MemorySample {
                elapsed_ms: 900_000,
                resident_bytes: 110_000_000,
            },
            MemorySample {
                elapsed_ms: REQUIRED_MEMORY_DURATION_MS,
                resident_bytes: 120_000_000,
            },
        ];
        assert_eq!(
            assess_memory(&monotonic, 120_000_000),
            MemoryAssessment::MonotonicGrowth
        );
        assert_eq!(
            assess_memory(&monotonic, TARGET_MEMORY_BYTES + 1),
            MemoryAssessment::OverBudget
        );
    }

    #[test]
    fn reduced_effects_enforce_priority_order_and_target_attestation() {
        let base = PerformanceEvidenceInput {
            build_id: "immutable-build".to_owned(),
            bundle_id: "fp.1.0.0".to_owned(),
            fixture_seed: 7,
            duration_ms: REQUIRED_MEMORY_DURATION_MS,
            peak_hostile_projectiles: 800,
            peak_enemies: 40,
            effect_mode: EffectMode::Reduced,
            frame_sample_kind: FrameSampleKind::RenderedFrame,
            frame_times_micros: vec![16_000; 120],
            memory_samples: vec![
                MemorySample {
                    elapsed_ms: 0,
                    resident_bytes: 400_000_000,
                },
                MemorySample {
                    elapsed_ms: REQUIRED_MEMORY_DURATION_MS,
                    resident_bytes: 400_000_000,
                },
            ],
            target_hardware: TargetHardware::canonical_description(),
            target_class_verified: false,
            hostile_telegraphs_retained: true,
            culled_effect_priorities: vec![5, 4],
        };
        let report = PerformanceEvidenceReport::compile(base.clone()).expect("report");
        assert_eq!(
            report.acceptance,
            PerformanceAcceptance::UnverifiedTargetHardware
        );

        let invalid = PerformanceEvidenceInput {
            culled_effect_priorities: vec![4, 5],
            ..base
        };
        assert_eq!(
            PerformanceEvidenceReport::compile(invalid),
            Err(PerformanceReportError::InvalidEffectCulling)
        );
    }

    #[test]
    fn sixty_second_replay_and_twenty_complete_boss_runs_match() {
        let report = run_bell_proctor_reliability_fixture().expect("reliability fixture");
        assert_eq!(report.replay_ticks, 1_800);
        assert_eq!(report.completed_runs, 20);
        assert_eq!(report.unique_run_hashes, 1);
        assert_eq!(report.replay_hash_blake3.len(), 64);
        assert_eq!(report.reference_run_hash_blake3.len(), 64);
        assert_eq!(
            report.replay_hash_blake3,
            "68558b94dfed325a84b0074ac16ac6a298d7fa063ce28a99607046d8ab643546"
        );
        assert_eq!(
            report.reference_run_hash_blake3,
            "534e36440d915778945ff42b1937041bf1a2ad8809430fda428c29815a34400f"
        );
    }
}
