//! Real rendered `GB-M01-09` benchmark and evidence adapter.

use std::{
    env, fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use bevy::prelude::*;
use sim_core::{
    EffectMode, FrameSampleKind, MemorySample, PerformanceEvidenceInput, PerformanceEvidenceReport,
    StressFixture, StressFixtureConfig, TARGET_ENEMY_COUNT, TARGET_HOSTILE_PROJECTILE_COUNT,
    TargetHardware,
};
use sysinfo::{Pid, ProcessesToUpdate, System, get_current_pid};

use crate::{
    FixedSimulationSet, FrameSet, LoadedArena, PackageDiagnostics,
    arena_view::simulation_point_to_render, combat::EvidenceScenario,
};

const DURATION_ENV: &str = "GRAVEBOUND_STRESS_DURATION_SECONDS";
const REPORT_PATH_ENV: &str = "GRAVEBOUND_PERFORMANCE_REPORT_PATH";
const TARGET_VERIFIED_ENV: &str = "GRAVEBOUND_TARGET_CLASS_VERIFIED";
const TARGET_GPU_ENV: &str = "GRAVEBOUND_TARGET_GPU";
const WARMUP_SECONDS: u64 = 5;
const MEMORY_SAMPLE_SECONDS: u64 = 10;
const HOSTILE_Z: f32 = 7.2;
const ENEMY_Z: f32 = 5.4;
const TELEGRAPH_Z: f32 = 4.6;
const FRIENDLY_EFFECT_Z: f32 = 5.9;
const DECORATIVE_Z: f32 = 2.0;

#[derive(Component)]
struct StressProjectileVisual(usize);

#[derive(Component)]
struct StressEnemyVisual(usize);

#[derive(Component)]
struct StressBenchmarkOverlay;

#[derive(Resource)]
pub(crate) struct StressBenchmarkState {
    fixture: StressFixture,
    mode: EffectMode,
    report_path: PathBuf,
    target_hardware: TargetHardware,
    target_class_verified: bool,
    measurement_duration: Duration,
    warmup_elapsed: Duration,
    measurement_elapsed: Duration,
    frame_times_micros: Vec<u64>,
    memory_samples: Vec<MemorySample>,
    next_memory_sample: Duration,
    memory: ProcessMemorySampler,
    report: Option<PerformanceEvidenceReport>,
}

impl StressBenchmarkState {
    pub(crate) const fn report_ready(&self) -> bool {
        self.report.is_some()
    }
}

struct ProcessMemorySampler {
    system: System,
    pid: Pid,
}

impl ProcessMemorySampler {
    fn new() -> Result<Self> {
        Ok(Self {
            system: System::new(),
            pid: get_current_pid().map_err(|error| {
                anyhow::anyhow!("failed to identify benchmark process: {error}")
            })?,
        })
    }

    fn resident_bytes(&mut self) -> Result<u64> {
        self.system
            .refresh_processes(ProcessesToUpdate::Some(&[self.pid]), true);
        self.system
            .process(self.pid)
            .map(sysinfo::Process::memory)
            .context("benchmark process disappeared from the resident-memory sampler")
    }
}

pub(crate) fn configure(
    app: &mut App,
    scenario: EvidenceScenario,
    window_width: u32,
    window_height: u32,
) -> Result<()> {
    let mode = match scenario {
        EvidenceScenario::StressFull => EffectMode::Full,
        EvidenceScenario::StressReduced => EffectMode::Reduced,
        _ => return Ok(()),
    };
    let duration_seconds = env::var(DURATION_ENV)
        .unwrap_or_else(|_| "60".to_owned())
        .parse::<u64>()
        .with_context(|| format!("{DURATION_ENV} must be an integer"))?;
    if !(1..=7_200).contains(&duration_seconds) {
        bail!("{DURATION_ENV} must be within 1..=7200")
    }
    let report_path = PathBuf::from(
        env::var_os(REPORT_PATH_ENV)
            .context("stress evidence requires GRAVEBOUND_PERFORMANCE_REPORT_PATH")?,
    );
    let target_class_verified = match env::var(TARGET_VERIFIED_ENV).as_deref() {
        Ok("1") => true,
        Ok("0") | Err(_) => false,
        Ok(other) => bail!("{TARGET_VERIFIED_ENV} must be 0 or 1, got `{other}`"),
    };
    let mut system = System::new_all();
    system.refresh_cpu_all();
    let cpu = system
        .cpus()
        .first()
        .map_or("unavailable", |cpu| cpu.brand())
        .to_owned();
    let target_hardware = TargetHardware {
        operating_system: System::long_os_version().unwrap_or_else(|| "Windows".to_owned()),
        cpu,
        memory_bytes: system.total_memory(),
        gpu: env::var(TARGET_GPU_ENV).unwrap_or_else(|_| "unverified".to_owned()),
        width_pixels: window_width,
        height_pixels: window_height,
    };
    let seed = StressFixtureConfig::default().seed;
    let (build_id, bundle_id) = {
        let package = app.world().resource::<PackageDiagnostics>();
        (package.build_id.clone(), package.content_version.clone())
    };
    app.insert_resource(StressBenchmarkState {
        fixture: StressFixture::new(StressFixtureConfig {
            seed,
            effect_mode: mode,
        }),
        mode,
        report_path,
        target_hardware,
        target_class_verified,
        measurement_duration: Duration::from_secs(duration_seconds),
        warmup_elapsed: Duration::ZERO,
        measurement_elapsed: Duration::ZERO,
        frame_times_micros: Vec::with_capacity(
            usize::try_from(duration_seconds.saturating_mul(90)).unwrap_or(usize::MAX),
        ),
        memory_samples: Vec::new(),
        next_memory_sample: Duration::ZERO,
        memory: ProcessMemorySampler::new()?,
        report: None,
    })
    .insert_resource(StressBuildIdentity {
        build_id,
        bundle_id,
        fixture_seed: seed,
    })
    .add_systems(Startup, spawn_stress_benchmark)
    .add_systems(
        FixedUpdate,
        advance_stress_fixture.in_set(FixedSimulationSet::Hostile),
    )
    .add_systems(
        Update,
        (
            sync_stress_visuals,
            collect_render_measurements,
            update_stress_overlay,
        )
            .chain()
            .in_set(FrameSet::Presentation),
    );
    Ok(())
}

#[derive(Resource)]
struct StressBuildIdentity {
    build_id: String,
    bundle_id: String,
    fixture_seed: u64,
}

#[allow(
    clippy::cast_precision_loss,
    clippy::needless_pass_by_value,
    clippy::too_many_lines
)]
fn spawn_stress_benchmark(
    mut commands: Commands,
    arena: Res<LoadedArena>,
    state: Res<StressBenchmarkState>,
) {
    for index in 0..TARGET_HOSTILE_PROJECTILE_COUNT {
        let position = stress_position(
            state
                .fixture
                .projectile_position_milli_tiles(index)
                .expect("stress projectile index is exact"),
            &arena.0,
        );
        let priority = state
            .fixture
            .projectile_threat_priority(index)
            .expect("stress projectile priority is exact");
        let outer = if index % 3 == 0 {
            Vec2::new(0.24, 0.11)
        } else {
            Vec2::splat(0.17)
        };
        let color = if priority >= 10 {
            Color::srgb_u8(251, 104, 82)
        } else {
            Color::srgb_u8(202, 105, 231)
        };
        commands
            .spawn((
                Name::new(format!("Stress hostile projectile {}", index + 1)),
                StressProjectileVisual(index),
                Sprite::from_color(Color::srgb_u8(248, 243, 220), outer),
                Transform::from_xyz(position.x, position.y, HOSTILE_Z).with_rotation(
                    Quat::from_rotation_z(if index % 2 == 0 {
                        std::f32::consts::FRAC_PI_4
                    } else {
                        0.0
                    }),
                ),
            ))
            .with_child((
                Sprite::from_color(color, outer * 0.58),
                Transform::from_xyz(0.0, 0.0, 0.1),
            ));
    }
    for index in 0..TARGET_ENEMY_COUNT {
        let position = stress_position(
            state
                .fixture
                .enemy_position_milli_tiles(index)
                .expect("stress enemy index is exact"),
            &arena.0,
        );
        commands.spawn((
            Name::new(format!("Stress hostile telegraph {}", index + 1)),
            Sprite::from_color(Color::srgba_u8(244, 184, 92, 90), Vec2::splat(1.25)),
            Transform::from_xyz(position.x, position.y, TELEGRAPH_Z)
                .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
        ));
        commands
            .spawn((
                Name::new(format!("Stress enemy {}", index + 1)),
                StressEnemyVisual(index),
                Sprite::from_color(Color::srgb_u8(112, 143, 157), Vec2::splat(0.62)),
                Transform::from_xyz(position.x, position.y, ENEMY_Z)
                    .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
            ))
            .with_child((
                Sprite::from_color(Color::srgb_u8(236, 225, 197), Vec2::splat(0.28)),
                Transform::from_xyz(0.0, 0.0, 0.1),
            ));
    }
    if state.mode == EffectMode::Full {
        for index in 0..400 {
            let column = index % 40;
            let row = index / 40;
            commands.spawn((
                Name::new("Stress priority-5 ambience"),
                Sprite::from_color(Color::srgba_u8(94, 119, 108, 55), Vec2::splat(0.08)),
                Transform::from_xyz(
                    -15.5 + column as f32 * 0.8,
                    -10.0 + row as f32 * 2.1,
                    DECORATIVE_Z,
                ),
            ));
        }
        for index in 0..160 {
            let column = index % 20;
            let row = index / 20;
            commands.spawn((
                Name::new("Stress priority-4 remote friendly effect"),
                Sprite::from_color(Color::srgba_u8(82, 211, 178, 90), Vec2::new(0.22, 0.06)),
                Transform::from_xyz(
                    -15.0 + column as f32 * 1.55,
                    -9.0 + row as f32 * 2.5,
                    FRIENDLY_EFFECT_Z,
                ),
            ));
        }
    }
    commands.spawn((
        Name::new("Stress benchmark overlay"),
        StressBenchmarkOverlay,
        Text::new("STRESS BENCHMARK INITIALIZING"),
        TextFont::from_font_size(14.0),
        TextColor(Color::srgb_u8(240, 235, 213)),
        Node {
            position_type: PositionType::Absolute,
            right: px(18),
            top: px(92),
            width: px(470),
            border: UiRect::all(px(2)),
            padding: UiRect::all(px(10)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(5, 9, 13, 235)),
        BorderColor::all(Color::srgba_u8(238, 226, 150, 230)),
    ));
}

fn advance_stress_fixture(mut state: ResMut<StressBenchmarkState>) {
    if state.report.is_none() {
        state.fixture.advance();
    }
}

#[allow(clippy::needless_pass_by_value)]
fn sync_stress_visuals(
    state: Res<StressBenchmarkState>,
    arena: Res<LoadedArena>,
    mut projectiles: Query<(&StressProjectileVisual, &mut Transform)>,
    mut enemies: Query<(&StressEnemyVisual, &mut Transform), Without<StressProjectileVisual>>,
) {
    for (visual, mut transform) in &mut projectiles {
        let position = stress_position(
            state
                .fixture
                .projectile_position_milli_tiles(visual.0)
                .expect("stress projectile visual index is exact"),
            &arena.0,
        );
        transform.translation.x = position.x;
        transform.translation.y = position.y;
    }
    for (visual, mut transform) in &mut enemies {
        let position = stress_position(
            state
                .fixture
                .enemy_position_milli_tiles(visual.0)
                .expect("stress enemy visual index is exact"),
            &arena.0,
        );
        transform.translation.x = position.x;
        transform.translation.y = position.y;
    }
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_lines)]
fn collect_render_measurements(
    time: Res<Time<Real>>,
    identity: Res<StressBuildIdentity>,
    mut state: ResMut<StressBenchmarkState>,
) {
    if state.report.is_some() {
        return;
    }
    let delta = time.delta();
    if state.warmup_elapsed < Duration::from_secs(WARMUP_SECONDS) {
        state.warmup_elapsed = state.warmup_elapsed.saturating_add(delta);
        return;
    }
    if state.memory_samples.is_empty() {
        let resident_bytes = state
            .memory
            .resident_bytes()
            .expect("resident-memory evidence must remain available");
        state.memory_samples.push(MemorySample {
            elapsed_ms: 0,
            resident_bytes,
        });
        state.next_memory_sample = Duration::from_secs(MEMORY_SAMPLE_SECONDS);
    }
    state.measurement_elapsed = state.measurement_elapsed.saturating_add(delta);
    state
        .frame_times_micros
        .push(u64::try_from(delta.as_micros()).unwrap_or(u64::MAX).max(1));
    if state.measurement_elapsed >= state.next_memory_sample {
        let elapsed_ms = u64::try_from(state.measurement_elapsed.as_millis()).unwrap_or(u64::MAX);
        let resident_bytes = state
            .memory
            .resident_bytes()
            .expect("resident-memory evidence must remain available");
        state.memory_samples.push(MemorySample {
            elapsed_ms,
            resident_bytes,
        });
        state.next_memory_sample = state
            .next_memory_sample
            .saturating_add(Duration::from_secs(MEMORY_SAMPLE_SECONDS));
    }
    if state.measurement_elapsed < state.measurement_duration {
        return;
    }
    let final_elapsed_ms = u64::try_from(state.measurement_elapsed.as_millis()).unwrap_or(u64::MAX);
    if state
        .memory_samples
        .last()
        .is_none_or(|sample| sample.elapsed_ms < final_elapsed_ms)
    {
        let resident_bytes = state
            .memory
            .resident_bytes()
            .expect("resident-memory evidence must remain available");
        state.memory_samples.push(MemorySample {
            elapsed_ms: final_elapsed_ms,
            resident_bytes,
        });
    }
    let report = PerformanceEvidenceReport::compile(PerformanceEvidenceInput {
        build_id: identity.build_id.clone(),
        bundle_id: identity.bundle_id.clone(),
        fixture_seed: identity.fixture_seed,
        duration_ms: final_elapsed_ms,
        peak_hostile_projectiles: TARGET_HOSTILE_PROJECTILE_COUNT,
        peak_enemies: TARGET_ENEMY_COUNT,
        effect_mode: state.mode,
        frame_sample_kind: FrameSampleKind::RenderedFrame,
        frame_times_micros: std::mem::take(&mut state.frame_times_micros),
        memory_samples: state.memory_samples.clone(),
        target_hardware: state.target_hardware.clone(),
        target_class_verified: state.target_class_verified,
        hostile_telegraphs_retained: true,
        culled_effect_priorities: match state.mode {
            EffectMode::Full => Vec::new(),
            EffectMode::Reduced => vec![5, 4],
        },
    })
    .expect("validated real rendered benchmark report must compile");
    publish_report(&state.report_path, &report)
        .expect("performance evidence report must publish atomically");
    state.report = Some(report);
}

fn publish_report(path: &Path, report: &PerformanceEvidenceReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create report directory {}", parent.display()))?;
    }
    let temporary = path.with_extension("partial.json");
    let bytes = serde_json::to_vec_pretty(report).context("failed to serialize report")?;
    fs::write(&temporary, bytes)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    fs::rename(&temporary, path)
        .with_context(|| format!("failed to publish {}", path.display()))?;
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn update_stress_overlay(
    state: Res<StressBenchmarkState>,
    mut overlay: Single<&mut Text, With<StressBenchmarkOverlay>>,
) {
    let snapshot = state.fixture.snapshot();
    let (status, fps, p95, p99, peak_memory) = state.report.as_ref().map_or_else(
        || {
            (
                "MEASURING".to_owned(),
                0,
                0,
                0,
                state
                    .memory_samples
                    .iter()
                    .map(|sample| sample.resident_bytes)
                    .max()
                    .unwrap_or(0),
            )
        },
        |report| {
            (
                format!("REPORT {:?}", report.acceptance),
                report.measured_fps_milli,
                report.p95_frame_time_micros,
                report.p99_frame_time_micros,
                report.peak_resident_bytes,
            )
        },
    );
    overlay.0 = format!(
        "GB-M01-09 RENDERED STRESS | {status}\nMODE {:?} | 1920x1080 | TARGET VERIFIED {}\nHOSTILE PROJECTILES {} / 800 | ENEMIES {} / 40\nHOSTILE TELEGRAPHS RETAINED | CULL {:?}\nWARMUP {:.1}/5.0S | MEASURE {:.1}/{:.1}S | FRAMES {}\nFPS {}.{:03} | P95 {}US | P99 {}US | PEAK RSS {} MIB\nSEED {:016X} | TICK {} | HASH {}",
        state.mode,
        state.target_class_verified,
        snapshot.hostile_projectiles,
        snapshot.enemies,
        match state.mode {
            EffectMode::Full => Vec::<u8>::new(),
            EffectMode::Reduced => vec![5, 4],
        },
        state.warmup_elapsed.as_secs_f64(),
        state.measurement_elapsed.as_secs_f64(),
        state.measurement_duration.as_secs_f64(),
        state
            .report
            .as_ref()
            .map_or(state.frame_times_micros.len(), |report| report
                .frame_sample_count),
        fps / 1_000,
        fps % 1_000,
        p95,
        p99,
        peak_memory / (1024 * 1024),
        StressFixtureConfig::default().seed,
        snapshot.tick.0,
        &snapshot.state_hash_blake3[..12],
    );
}

#[allow(clippy::cast_precision_loss)]
fn stress_position((x, y): (i32, i32), arena: &sim_core::ArenaGeometry) -> Vec2 {
    simulation_point_to_render(
        sim_core::SimulationVector::new(x as f32 / 1_000.0, y as f32 / 1_000.0),
        arena,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_and_culling_contract_are_explicit() {
        assert_eq!(WARMUP_SECONDS, 5);
        assert_eq!(MEMORY_SAMPLE_SECONDS, 10);
        assert_eq!(TARGET_HOSTILE_PROJECTILE_COUNT, 800);
        assert_eq!(TARGET_ENEMY_COUNT, 40);
    }
}
