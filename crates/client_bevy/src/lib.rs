//! Gravebound native client presentation boundary.

mod arena_view;
mod combat;
mod player;

use std::{env, fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use bevy::{
    log::{error, info},
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
    window::WindowResolution,
};
use sim_content::{
    ValidationReport, first_playable_arena, first_playable_weapon, load_and_validate,
};
use sim_core::{ArenaGeometry, PlayerCombatState, PlayerMovementState};

pub use arena_view::{
    ArenaRenderPlan, DEFAULT_VIEW_HEIGHT_TILES, DEFAULT_VIEW_WIDTH_AT_16_9_TILES, RenderRectangle,
    authored_point_to_render, build_render_plan, render_point_to_simulation,
    simulation_point_to_render, visible_width_for_aspect,
};
pub use combat::{CombatInputGate, PrimaryFireBindings};
pub use player::{CAMERA_RESPONSE_SECONDS, MovementBindings, critically_damped_step};

const WINDOW_TITLE: &str = "Gravebound - LocalLab";
const DEFAULT_CONTENT_ROOT: &str = "content";
const EVIDENCE_CAPTURE_RENDER_FRAMES: u8 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
enum FixedSimulationSet {
    Movement,
    Combat,
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
    content_version: String,
    record_count: usize,
    package_hash_blake3: String,
    content_root: PathBuf,
}

#[derive(Resource)]
struct ScreenshotRequest(PathBuf);

impl PackageDiagnostics {
    fn from_report(report: ValidationReport, content_root: PathBuf) -> Self {
        Self {
            content_version: report.content_version,
            record_count: report.record_count,
            package_hash_blake3: report.package_hash_blake3,
            content_root,
        }
    }
}

/// Validates the immutable content package, constructs the arena, and runs `LocalLab`.
pub fn run_local_lab() -> Result<()> {
    let content_root = resolve_content_root()?;
    let (package, report) = load_and_validate(&content_root).with_context(|| {
        format!(
            "content validation failed at {}; set GRAVEBOUND_CONTENT_ROOT when launching outside the repository",
            content_root.display()
        )
    })?;
    let arena = first_playable_arena(&package).context("failed to compile Bell Laboratory")?;
    let weapon = first_playable_weapon(&package).context("failed to compile Pine Crossbow")?;
    let player_state = PlayerMovementState::at_arena_spawn(&arena)
        .context("failed to construct the Grave Arbalist movement state")?;
    let combat_state = PlayerCombatState::new(weapon)
        .context("failed to construct the Grave Arbalist combat state")?;

    let screenshot_request = env::var_os("GRAVEBOUND_SCREENSHOT_PATH").map(PathBuf::from);
    let evidence_scenario =
        combat::EvidenceScenario::from_environment(screenshot_request.is_some())?;
    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb_u8(7, 10, 14)))
        .insert_resource(LoadedArena(arena))
        .insert_resource(player::PlayerSimulation::new(player_state))
        .insert_resource(combat::CombatSimulation::new(combat_state))
        .insert_resource(evidence_scenario)
        .insert_resource(Time::<Fixed>::from_hz(f64::from(
            sim_core::TICKS_PER_SECOND,
        )))
        .insert_resource(PackageDiagnostics::from_report(report, content_root))
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: WINDOW_TITLE.to_owned(),
                        resolution: WindowResolution::new(1280, 720),
                        resizable: true,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .configure_sets(
            FixedUpdate,
            (FixedSimulationSet::Movement, FixedSimulationSet::Combat).chain(),
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
        .add_systems(Update, capture_requested_screenshot);
    player::configure(&mut app);
    combat::configure(&mut app);
    if let Some(path) = screenshot_request {
        app.insert_resource(ScreenshotRequest(path));
    }
    app.run();
    Ok(())
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn capture_requested_screenshot(
    mut commands: Commands,
    request: Option<Res<ScreenshotRequest>>,
    mut rendered_frames: Local<u8>,
) {
    let Some(request) = request else {
        return;
    };
    *rendered_frames = rendered_frames.saturating_add(1);
    if *rendered_frames == EVIDENCE_CAPTURE_RENDER_FRAMES {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_screenshot_atomically(request.0.clone()));
    }
}

fn save_screenshot_atomically(path: PathBuf) -> impl FnMut(On<ScreenshotCaptured>) {
    let temporary_path = temporary_screenshot_path(&path);
    let mut save_temporary = save_to_disk(temporary_path.clone());
    move |captured| {
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
            Ok(()) => info!("Screenshot atomically published to {}", path.display()),
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
}
