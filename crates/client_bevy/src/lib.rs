//! Gravebound native client presentation boundary.

mod accessibility;
mod arena_view;
mod combat;
mod consumable;
mod death;
mod debug_overlay;
mod developer_tools;
mod encounter;
mod enemies;
mod inventory;
mod item_showcase;
mod network_prediction;
mod network_session;
mod player;
mod stress_benchmark;
mod telemetry;

use std::{env, fs, io::Read, path::PathBuf};

use anyhow::{Context, Result, bail};
use bevy::{
    log::{error, info},
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
    window::{PresentMode, WindowResolution},
};
use sim_content::{
    ContentPackage, ValidationReport, first_playable_arena, first_playable_bell_proctor,
    first_playable_bell_reed, first_playable_chain_sentry, first_playable_drowned_pilgrim,
    first_playable_equipment_catalog, first_playable_grave_mark, first_playable_red_tonic,
    first_playable_red_tonic_with_undertaker_knot, first_playable_reward_catalog,
    first_playable_slipstep, first_playable_stillness, first_playable_weapon, load_and_validate,
};
use sim_core::{
    ArenaGeometry, EnemyLabDefinitions, PlayerCombatState, PlayerMovementState,
    ProjectileCollisionWorld, StillnessDefinition, StillnessDefinitionParameters,
};

pub use arena_view::{
    ArenaRenderPlan, DEFAULT_VIEW_HEIGHT_TILES, DEFAULT_VIEW_WIDTH_AT_16_9_TILES, RenderRectangle,
    authored_point_to_render, build_render_plan, render_point_to_simulation,
    simulation_point_to_render, visible_width_for_aspect,
};
pub use combat::AbilityTwoBindings;
pub use combat::{AbilityOneBindings, CombatInputGate, PrimaryFireBindings};
pub use network_prediction::{
    CompleteSnapshot, CorrectionClass, CorrectionSignal, DeterministicProjectilePresentation,
    InterpolatedEntity, NativeNetworkPresentation, NetworkCorrectionDiagnostics,
    NetworkPredictionError, PredictedMovementInput, ReconciliationEvent, RemoteClientRuntime,
    RemoteSnapshotInbox, SnapshotApplication, SnapshotAssembler,
};
pub use network_session::{
    CLIENT_LINK_LOST_MS, ClientConnectionLifecycle, ClientConnectionPhase,
    ClientSessionLifecycleError,
};
pub use player::{CAMERA_RESPONSE_SECONDS, MovementBindings, critically_damped_step};

const WINDOW_TITLE: &str = "Gravebound - LocalLab";
const DEFAULT_CONTENT_ROOT: &str = "content";
const WINDOW_SIZE_ENV: &str = "GRAVEBOUND_WINDOW_SIZE";
const DEFAULT_EVIDENCE_CAPTURE_RENDER_FRAMES: u8 = 60;
const EVIDENCE_SETTLE_RENDER_FRAMES: u8 = 60;
const SLIPSTEP_EVIDENCE_SETTLE_RENDER_FRAMES: u8 = 12;
const RED_TONIC_EVIDENCE_SETTLE_RENDER_FRAMES: u8 = 6;
const ENEMY_EVIDENCE_SETTLE_RENDER_FRAMES: u8 = 60;
const ENEMY_DEATH_EVIDENCE_SETTLE_RENDER_FRAMES: u8 = 12;
const LETHAL_EVIDENCE_SETTLE_RENDER_FRAMES: u8 = 180;
const DEATH_RESTART_EVIDENCE_SETTLE_RENDER_FRAMES: u8 = 6;
const DEATH_RECAP_EVIDENCE_SETTLE_RENDER_FRAMES: u8 = 12;
const ITEM_CATALOG_EVIDENCE_SETTLE_RENDER_FRAMES: u8 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
enum FixedSimulationSet {
    Movement,
    Developer,
    Hostile,
    Combat,
    Encounter,
    Consumable,
    Inventory,
    Telemetry,
    Death,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
enum FrameSet {
    CameraFollow,
    InputSample,
    Presentation,
}

#[derive(Resource)]
struct LoadedArena(ArenaGeometry);

#[derive(Resource)]
struct PackageDiagnostics {
    build_id: String,
    content_version: String,
    record_count: usize,
    package_hash_blake3: String,
    content_root: PathBuf,
}

#[derive(Resource)]
struct ScreenshotRequest(PathBuf);

#[derive(Default)]
struct CaptureProgress {
    rendered_frames: u8,
    ready_frames: u8,
    capture_queued: bool,
}

impl PackageDiagnostics {
    fn from_report(report: ValidationReport, content_root: PathBuf, build_id: String) -> Self {
        Self {
            build_id,
            content_version: report.content_version,
            record_count: report.record_count,
            package_hash_blake3: report.package_hash_blake3,
            content_root,
        }
    }
}

/// Validates the immutable content package, constructs the arena, and runs `LocalLab`.
#[allow(clippy::too_many_lines)] // App assembly remains linear so plugin and set order are reviewable.
pub fn run_local_lab() -> Result<()> {
    let build_id = executable_build_id()?;
    let content_root = resolve_content_root()?;
    let (package, report) = load_and_validate(&content_root).with_context(|| {
        format!(
            "content validation failed at {}; set GRAVEBOUND_CONTENT_ROOT when launching outside the repository",
            content_root.display()
        )
    })?;
    let arena = first_playable_arena(&package).context("failed to compile Bell Laboratory")?;
    let screenshot_request = env::var_os("GRAVEBOUND_SCREENSHOT_PATH").map(PathBuf::from);
    let (window_width, window_height) = configured_window_size()?;
    let evidence_scenario =
        combat::EvidenceScenario::from_environment(screenshot_request.is_some())?;
    let equipment_catalog = first_playable_equipment_catalog(&package)
        .context("failed to compile the 12 prototype equipment templates")?;
    let reward_catalog = first_playable_reward_catalog(&package)
        .context("failed to compile the five prototype reward tables")?;
    let weapon = if evidence_scenario == combat::EvidenceScenario::ItemCatalogShowcase {
        equipment_catalog
            .crossbow("item.prototype.weapon.scatterbow")
            .context("failed to compile Scatterbow showcase")?
    } else {
        first_playable_weapon(&package).context("failed to compile Pine Crossbow")?
    };
    let grave_mark = first_playable_grave_mark(&package).context("failed to compile Grave Mark")?;
    let slipstep = first_playable_slipstep(&package).context("failed to compile Slipstep")?;
    let base_stillness =
        first_playable_stillness(&package).context("failed to compile Stillness")?;
    let stillness = if evidence_scenario == combat::EvidenceScenario::ItemCatalogShowcase {
        StillnessDefinition::new(StillnessDefinitionParameters {
            content_id: "item.prototype.charm.still_eye".to_owned(),
            activation_ticks: 12,
            movement_threshold_basis_points: base_stillness.movement_threshold_basis_points(),
            projectile_speed_bonus_basis_points: 1_000,
            primary_damage_bonus_basis_points: 600,
            break_on_damage: base_stillness.break_on_damage(),
            break_on_slipstep: base_stillness.break_on_slipstep(),
        })
        .context("failed to compile Still Eye showcase")?
    } else {
        base_stillness
    };
    let player_state = if evidence_scenario == combat::EvidenceScenario::DamageGraceShowcase {
        PlayerMovementState::new(sim_core::SimulationVector::new(5.5, 12.0), &arena)
            .context("failed to construct the damage-grace evidence position")?
    } else {
        PlayerMovementState::at_arena_spawn(&arena)
            .context("failed to construct the Grave Arbalist movement state")?
    };
    let combat_state = PlayerCombatState::new(weapon, grave_mark, slipstep, stillness)
        .context("failed to construct the Grave Arbalist combat state")?;
    let debug_hurtboxes = combat::first_playable_debug_hurtboxes()
        .context("failed to construct LocalLab debug enemy hurtboxes")?;
    let collision_world = ProjectileCollisionWorld::new(&arena, debug_hurtboxes)
        .context("failed to construct the LocalLab projectile collision world")?;

    let definitions = build_enemy_definitions(&package)?;
    let boss_definition =
        first_playable_bell_proctor(&package).context("failed to compile Bell Proctor")?;
    let red_tonic = if evidence_scenario == combat::EvidenceScenario::ItemCatalogShowcase {
        first_playable_red_tonic_with_undertaker_knot(&package)
            .context("failed to compile Undertaker Knot Tonic override")?
    } else {
        first_playable_red_tonic(&package).context("failed to compile Red Tonic")?
    };
    let current_health = match evidence_scenario {
        combat::EvidenceScenario::RedTonicShowcase
        | combat::EvidenceScenario::ItemCatalogShowcase => 70,
        combat::EvidenceScenario::DamageLethalShowcase
        | combat::EvidenceScenario::DeathRestartShowcase
        | combat::EvidenceScenario::DeathRecapShowcase => 8,
        _ => 128,
    };
    let run_stats = if evidence_scenario == combat::EvidenceScenario::ItemCatalogShowcase {
        death::LocalRunStats {
            movement_speed_tiles_per_second: 5.1 * 0.98,
            maximum_health: 140,
            armor: 2,
            resistance_basis_points: 0,
        }
    } else {
        death::LocalRunStats::default()
    };
    let run_factory = death::LocalRunFactory::new(
        arena.clone(),
        definitions,
        boss_definition,
        combat_state,
        red_tonic,
        run_stats,
        matches!(
            evidence_scenario,
            combat::EvidenceScenario::None
                | combat::EvidenceScenario::DebugOverlayShowcase
                | combat::EvidenceScenario::DebugToolsShowcase
                | combat::EvidenceScenario::BossShowcase
                | combat::EvidenceScenario::BossCompletionShowcase
                | combat::EvidenceScenario::StressFull
                | combat::EvidenceScenario::StressReduced
        ),
    );
    let (player_simulation, enemy_runtime) = run_factory
        .build_run(1, player_state.position(), current_health)
        .context("failed to construct the three-role enemy laboratory")?;
    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb_u8(7, 10, 14)))
        .insert_resource(LoadedArena(arena))
        .insert_resource(player_simulation)
        .insert_resource(combat::CombatCollisionWorld::new(collision_world))
        .insert_resource(evidence_scenario)
        .insert_resource(enemy_runtime)
        .insert_resource(run_factory)
        .insert_resource(item_showcase::ItemShowcaseCatalog::new(
            equipment_catalog,
            reward_catalog,
            report.content_version.clone(),
        )?)
        .insert_resource(Time::<Fixed>::from_hz(f64::from(
            sim_core::TICKS_PER_SECOND,
        )))
        .insert_resource(PackageDiagnostics::from_report(
            report,
            content_root,
            build_id,
        ))
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: WINDOW_TITLE.to_owned(),
                        resolution: WindowResolution::new(window_width, window_height),
                        present_mode: if matches!(
                            evidence_scenario,
                            combat::EvidenceScenario::StressFull
                                | combat::EvidenceScenario::StressReduced
                        ) {
                            PresentMode::AutoNoVsync
                        } else {
                            PresentMode::AutoVsync
                        },
                        // A benchmark report must describe the exact swapchain size for its
                        // entire sample window. Ordinary LocalLab windows remain resizable.
                        resizable: !matches!(
                            evidence_scenario,
                            combat::EvidenceScenario::StressFull
                                | combat::EvidenceScenario::StressReduced
                        ),
                        ..default()
                    }),
                    ..default()
                }),
        )
        .configure_sets(
            FixedUpdate,
            (
                FixedSimulationSet::Movement,
                FixedSimulationSet::Developer,
                FixedSimulationSet::Hostile,
                FixedSimulationSet::Combat,
                FixedSimulationSet::Encounter,
                FixedSimulationSet::Consumable,
                FixedSimulationSet::Inventory,
                FixedSimulationSet::Telemetry,
                FixedSimulationSet::Death,
            )
                .chain(),
        )
        .configure_sets(
            Update,
            (
                FrameSet::CameraFollow,
                FrameSet::InputSample,
                FrameSet::Presentation,
            )
                .chain(),
        )
        .add_systems(Startup, arena_view::spawn_arena_view)
        .add_systems(
            Update,
            capture_requested_screenshot.after(FrameSet::Presentation),
        );
    player::configure(&mut app);
    network_prediction::configure(&mut app);
    accessibility::configure(&mut app);
    enemies::configure(&mut app);
    combat::configure(&mut app);
    encounter::configure(&mut app);
    consumable::configure(&mut app);
    death::configure(&mut app)?;
    inventory::configure(&mut app);
    item_showcase::configure(&mut app);
    debug_overlay::configure(&mut app);
    developer_tools::configure(&mut app, evidence_scenario);
    stress_benchmark::configure(&mut app, evidence_scenario, window_width, window_height)?;
    telemetry::configure(&mut app)?;
    if let Some(path) = screenshot_request {
        app.insert_resource(ScreenshotRequest(path));
    }
    app.run();
    Ok(())
}

fn build_enemy_definitions(package: &ContentPackage) -> Result<EnemyLabDefinitions> {
    Ok(EnemyLabDefinitions {
        drowned_pilgrim: first_playable_drowned_pilgrim(package)
            .context("failed to compile Drowned Pilgrim")?,
        bell_reed: first_playable_bell_reed(package).context("failed to compile Bell Reed")?,
        chain_sentry: first_playable_chain_sentry(package)
            .context("failed to compile Chain Sentry")?,
    })
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)] // Bevy system parameters are wrapper values.
fn capture_requested_screenshot(
    mut commands: Commands,
    request: Option<Res<ScreenshotRequest>>,
    scenario: Res<combat::EvidenceScenario>,
    collision_diagnostics: Res<combat::CollisionDiagnostics>,
    consumable_diagnostics: Res<consumable::ConsumableDiagnostics>,
    enemy_runtime: Res<enemies::EnemyLabRuntime>,
    enemy_presentation: Res<enemies::EnemyPresentationState>,
    death_runtime: Res<death::LocalDeathRuntime>,
    inventory_diagnostics: Res<inventory::InventoryDiagnostics>,
    item_catalog: Res<item_showcase::ItemShowcaseCatalog>,
    player_simulation: Res<player::PlayerSimulation>,
    debug_overlay: Res<debug_overlay::DebugOverlayState>,
    developer_tools: Res<developer_tools::DeveloperToolsState>,
    stress: Option<Res<stress_benchmark::StressBenchmarkState>>,
    mut progress: Local<CaptureProgress>,
) {
    let Some(request) = request else {
        return;
    };
    if progress.capture_queued {
        return;
    }
    progress.rendered_frames = progress.rendered_frames.saturating_add(1);
    let ready = if *scenario == combat::EvidenceScenario::CollisionShowcase {
        collision_diagnostics.showcase_ready()
    } else if *scenario == combat::EvidenceScenario::GraveMarkShowcase {
        collision_diagnostics.grave_mark_showcase_ready()
    } else if *scenario == combat::EvidenceScenario::SlipstepShowcase {
        collision_diagnostics.slipstep_showcase_ready()
    } else if *scenario == combat::EvidenceScenario::StillnessShowcase {
        collision_diagnostics.stillness_showcase_ready()
    } else if *scenario == combat::EvidenceScenario::RedTonicShowcase {
        consumable_diagnostics.showcase_ready()
    } else if *scenario == combat::EvidenceScenario::EnemyShowcase {
        enemy_runtime.showcase_ready()
            && enemy_runtime.active_lane_is_clear()
            && enemy_presentation.readability_showcase_ready()
    } else if *scenario == combat::EvidenceScenario::EnemyDeathShowcase {
        enemy_presentation.death_showcase_ready()
    } else if *scenario == combat::EvidenceScenario::DamageLethalShowcase {
        enemy_presentation.lethal_showcase_ready() && collision_diagnostics.later_action_rejected()
    } else if *scenario == combat::EvidenceScenario::DamageGraceShowcase {
        enemy_presentation.grace_showcase_ready()
    } else if *scenario == combat::EvidenceScenario::DeathRestartShowcase {
        death_runtime.evidence_ready()
    } else if *scenario == combat::EvidenceScenario::DeathRecapShowcase {
        death_runtime.death_evidence_ready()
    } else if *scenario == combat::EvidenceScenario::InventoryShowcase {
        inventory_diagnostics.evidence_ready()
    } else if *scenario == combat::EvidenceScenario::ItemCatalogShowcase {
        item_catalog.evidence_ready(&enemy_runtime, &player_simulation, &consumable_diagnostics)
            && collision_diagnostics.item_showcase_ready()
    } else if *scenario == combat::EvidenceScenario::DebugOverlayShowcase {
        debug_overlay.evidence_ready()
            && !matches!(
                death_runtime.encounter().state(),
                sim_core::EncounterState::AwaitingFirstActivity
                    | sim_core::EncounterState::FirstWaveDelay { .. }
            )
            && !enemy_runtime.normal_snapshots().is_empty()
    } else if *scenario == combat::EvidenceScenario::DebugToolsShowcase {
        developer_tools.evidence_ready()
            && !matches!(
                death_runtime.encounter().state(),
                sim_core::EncounterState::AwaitingFirstActivity
                    | sim_core::EncounterState::FirstWaveDelay { .. }
            )
    } else if *scenario == combat::EvidenceScenario::BossShowcase {
        enemy_runtime.boss_snapshot().is_some_and(|snapshot| {
            snapshot.local_tick.0 >= 188
                && matches!(
                    snapshot.state,
                    sim_core::BellProctorStateKind::Active(sim_core::BellProctorPhase::Phase1)
                )
        }) && debug_overlay.evidence_ready()
    } else if *scenario == combat::EvidenceScenario::BossCompletionShowcase {
        matches!(
            death_runtime.encounter().state(),
            sim_core::EncounterState::CompletionSummary
        ) && debug_overlay.evidence_ready()
    } else if matches!(
        *scenario,
        combat::EvidenceScenario::StressFull | combat::EvidenceScenario::StressReduced
    ) {
        stress
            .as_deref()
            .is_some_and(stress_benchmark::StressBenchmarkState::report_ready)
    } else {
        progress.rendered_frames >= DEFAULT_EVIDENCE_CAPTURE_RENDER_FRAMES
    };
    if ready {
        progress.ready_frames = progress.ready_frames.saturating_add(1);
    }
    let required_ready_frames = match *scenario {
        combat::EvidenceScenario::SlipstepShowcase => SLIPSTEP_EVIDENCE_SETTLE_RENDER_FRAMES,
        combat::EvidenceScenario::RedTonicShowcase => RED_TONIC_EVIDENCE_SETTLE_RENDER_FRAMES,
        combat::EvidenceScenario::EnemyShowcase => ENEMY_EVIDENCE_SETTLE_RENDER_FRAMES,
        combat::EvidenceScenario::EnemyDeathShowcase => ENEMY_DEATH_EVIDENCE_SETTLE_RENDER_FRAMES,
        combat::EvidenceScenario::DamageLethalShowcase
        | combat::EvidenceScenario::DamageGraceShowcase => LETHAL_EVIDENCE_SETTLE_RENDER_FRAMES,
        combat::EvidenceScenario::DeathRestartShowcase => {
            DEATH_RESTART_EVIDENCE_SETTLE_RENDER_FRAMES
        }
        combat::EvidenceScenario::DeathRecapShowcase => DEATH_RECAP_EVIDENCE_SETTLE_RENDER_FRAMES,
        combat::EvidenceScenario::ItemCatalogShowcase => ITEM_CATALOG_EVIDENCE_SETTLE_RENDER_FRAMES,
        _ => EVIDENCE_SETTLE_RENDER_FRAMES,
    };
    if progress.ready_frames >= required_ready_frames {
        progress.capture_queued = true;
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_screenshot_atomically(request.0.clone()));
    }
}

fn save_screenshot_atomically(
    path: PathBuf,
) -> impl FnMut(On<ScreenshotCaptured>, MessageWriter<AppExit>) {
    let temporary_path = temporary_screenshot_path(&path);
    let mut save_temporary = save_to_disk(temporary_path.clone());
    move |captured, mut app_exit: MessageWriter<AppExit>| {
        save_temporary(captured);
        if !temporary_path.is_file() {
            error!(
                "Screenshot temporary file was not created at {}",
                temporary_path.display()
            );
            return;
        }
        let sync_result = fs::OpenOptions::new()
            .write(true)
            .open(&temporary_path)
            .and_then(|file| file.sync_all());
        if let Err(error) = sync_result {
            error!(
                "Cannot flush screenshot temporary file {}: {error}",
                temporary_path.display()
            );
            return;
        }
        match fs::rename(&temporary_path, &path) {
            Ok(()) => {
                info!("Screenshot atomically published to {}", path.display());
                app_exit.write(AppExit::Success);
            }
            Err(error) => error!(
                "Cannot atomically publish screenshot {}: {error}",
                path.display()
            ),
        }
    }
}

fn temporary_screenshot_path(path: &std::path::Path) -> PathBuf {
    let extension = path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("png");
    let stem = path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("gravebound-screenshot");
    path.with_file_name(format!("{stem}.partial.{extension}"))
}

fn configured_window_size() -> Result<(u32, u32)> {
    env::var(WINDOW_SIZE_ENV).map_or(Ok((1280, 720)), |value| parse_window_size(&value))
}

fn executable_build_id() -> Result<String> {
    let path = env::current_exe().context("failed to resolve the Gravebound executable")?;
    let mut file = fs::File::open(&path)
        .with_context(|| format!("failed to open Gravebound executable {}", path.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let count = file
            .read(&mut buffer)
            .with_context(|| format!("failed to hash Gravebound executable {}", path.display()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("release-{}", hasher.finalize().to_hex()))
}

fn parse_window_size(value: &str) -> Result<(u32, u32)> {
    let Some((width, height)) = value.split_once('x') else {
        bail!("{WINDOW_SIZE_ENV} must use WIDTHxHEIGHT")
    };
    let width: u32 = width
        .parse()
        .with_context(|| format!("invalid {WINDOW_SIZE_ENV} width"))?;
    let height: u32 = height
        .parse()
        .with_context(|| format!("invalid {WINDOW_SIZE_ENV} height"))?;
    if !(1280..=7680).contains(&width) || !(720..=4320).contains(&height) {
        bail!("{WINDOW_SIZE_ENV} must remain within 1280x720..7680x4320")
    }
    Ok((width, height))
}

fn resolve_content_root() -> Result<PathBuf> {
    if let Some(configured) = env::var_os("GRAVEBOUND_CONTENT_ROOT") {
        return Ok(PathBuf::from(configured));
    }
    let current_directory = env::current_dir().context("failed to resolve current directory")?;
    let current_candidate = current_directory.join(DEFAULT_CONTENT_ROOT);
    if is_content_root(&current_candidate) {
        return Ok(current_candidate);
    }
    let executable = env::current_exe().context("failed to resolve LocalLab executable")?;
    for ancestor in executable.ancestors().skip(1) {
        let candidate = ancestor.join(DEFAULT_CONTENT_ROOT);
        if is_content_root(&candidate) {
            return Ok(candidate);
        }
    }
    bail!(
        "could not locate the content package from {} or executable {}; set GRAVEBOUND_CONTENT_ROOT",
        current_directory.display(),
        executable.display()
    )
}

fn is_content_root(path: &std::path::Path) -> bool {
    path.join("manifests/fp.1.0.0.json").is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_temporary_path_preserves_image_format() {
        assert_eq!(
            temporary_screenshot_path(std::path::Path::new("tmp/evidence.png")),
            PathBuf::from("tmp/evidence.partial.png")
        );
        assert_eq!(
            temporary_screenshot_path(std::path::Path::new("tmp/evidence.jpg")),
            PathBuf::from("tmp/evidence.partial.jpg")
        );
    }

    #[test]
    fn evidence_window_size_is_strict_and_preserves_supported_bounds() {
        assert_eq!(parse_window_size("1280x720").unwrap(), (1280, 720));
        assert_eq!(parse_window_size("1920x1080").unwrap(), (1920, 1080));
        assert!(parse_window_size("1920X1080").is_err());
        assert!(parse_window_size("1279x720").is_err());
        assert!(parse_window_size("1920x719").is_err());
        assert!(parse_window_size("wide").is_err());
    }
}
