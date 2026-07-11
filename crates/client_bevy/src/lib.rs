//! Gravebound native client presentation boundary.

mod arena_view;
mod player;

use std::{env, path::PathBuf};

use anyhow::{Context, Result, bail};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
    window::WindowResolution,
};
use sim_content::{ValidationReport, first_playable_arena, load_and_validate};
use sim_core::{ArenaGeometry, PlayerMovementState};

pub use arena_view::{
    ArenaRenderPlan, DEFAULT_VIEW_HEIGHT_TILES, DEFAULT_VIEW_WIDTH_AT_16_9_TILES, RenderRectangle,
    authored_point_to_render, build_render_plan, visible_width_for_aspect,
};
pub use player::{CAMERA_RESPONSE_SECONDS, MovementBindings, critically_damped_step};

const WINDOW_TITLE: &str = "Gravebound - LocalLab";
const DEFAULT_CONTENT_ROOT: &str = "content";

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
    let player_state = PlayerMovementState::at_arena_spawn(&arena)
        .context("failed to construct the Grave Arbalist movement state")?;

    let screenshot_request = env::var_os("GRAVEBOUND_SCREENSHOT_PATH").map(PathBuf::from);
    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb_u8(7, 10, 14)))
        .insert_resource(LoadedArena(arena))
        .insert_resource(player::PlayerSimulation::new(player_state))
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
        .add_systems(Startup, arena_view::spawn_arena_view)
        .add_systems(Update, capture_requested_screenshot);
    player::configure(&mut app);
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
    if *rendered_frames == 10 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(request.0.clone()));
    }
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
