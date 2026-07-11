use std::f32::consts::FRAC_PI_4;

use bevy::{camera::ScalingMode, log::info, prelude::*, sprite::Anchor};
use sim_core::{ArenaGeometry, ArenaGeometryError, MILLI_TILES_PER_TILE, TilePoint, TileRectangle};

use crate::{LoadedArena, PackageDiagnostics};

/// `SIM-002` default vertical camera extent.
pub const DEFAULT_VIEW_HEIGHT_TILES: f32 = 13.5;
/// `SIM-002` default horizontal extent at 16:9.
pub const DEFAULT_VIEW_WIDTH_AT_16_9_TILES: f32 = 24.0;
const GRID_LINE_THICKNESS_TILES: f32 = 0.018;
const WORLD_LABEL_SCALE: f32 = 0.024;

const Z_FLOOR: f32 = 0.0;
const Z_GRID: f32 = 1.0;
const Z_SOLID: f32 = 2.0;
const Z_MARKER: f32 = 3.0;
const Z_LABEL: f32 = 4.0;

/// Render-space rectangle created from authoritative geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderRectangle {
    pub center: Vec2,
    pub size: Vec2,
}

/// Pure presentation plan used by the Bevy spawner and deterministic unit tests.
#[derive(Debug, Clone, PartialEq)]
pub struct ArenaRenderPlan {
    pub floor: RenderRectangle,
    pub shell: [RenderRectangle; 4],
    pub pillars: Vec<RenderRectangle>,
    pub player_spawn: Vec2,
    pub boss_spawn: Vec2,
    pub anchors: Vec<(String, Vec2)>,
}

/// Semantic category attached to every debug primitive for inspection and future toggles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub enum ArenaVisual {
    Floor,
    Grid,
    Shell,
    Pillar,
    PlayerSpawn,
    BossSpawn,
    WaveAnchor,
    RewardAnchor,
    TonicAnchor,
    Label,
}

/// Converts a northwest-authored point to centered Bevy world space.
#[must_use]
pub fn authored_point_to_render(point: TilePoint, arena: &ArenaGeometry) -> Vec2 {
    let half_width = milli_to_tiles(arena.width_milli_tiles) * 0.5;
    let half_height = milli_to_tiles(arena.height_milli_tiles) * 0.5;
    Vec2::new(
        milli_to_tiles(point.x_milli_tiles) - half_width,
        half_height - milli_to_tiles(point.y_milli_tiles),
    )
}

fn authored_rectangle_to_render(
    rectangle: TileRectangle,
    arena: &ArenaGeometry,
) -> RenderRectangle {
    let width = milli_to_tiles(rectangle.width_milli_tiles);
    let height = milli_to_tiles(rectangle.height_milli_tiles);
    let center = TilePoint::new(
        rectangle.x_milli_tiles + rectangle.width_milli_tiles / 2,
        rectangle.y_milli_tiles + rectangle.height_milli_tiles / 2,
    );
    RenderRectangle {
        center: authored_point_to_render(center, arena),
        size: Vec2::new(width, height),
    }
}

/// Builds all geometry-derived render primitives without consulting source/display pixels.
pub fn build_render_plan(arena: &ArenaGeometry) -> Result<ArenaRenderPlan, ArenaGeometryError> {
    let floor = authored_rectangle_to_render(
        TileRectangle::new(0, 0, arena.width_milli_tiles, arena.height_milli_tiles),
        arena,
    );
    let shell = arena
        .shell_rectangles()?
        .map(|rectangle| authored_rectangle_to_render(rectangle, arena));
    let pillars = arena
        .pillars
        .iter()
        .copied()
        .map(|rectangle| authored_rectangle_to_render(rectangle, arena))
        .collect();
    let anchors = arena
        .anchors
        .iter()
        .map(|anchor| {
            (
                anchor.id.clone(),
                authored_point_to_render(anchor.point, arena),
            )
        })
        .collect();
    Ok(ArenaRenderPlan {
        floor,
        shell,
        pillars,
        player_spawn: authored_point_to_render(arena.player_spawn, arena),
        boss_spawn: authored_point_to_render(arena.boss_spawn, arena),
        anchors,
    })
}

/// Returns horizontal camera extent while preserving one unit on both render axes.
#[must_use]
pub fn visible_width_for_aspect(aspect_ratio: f32) -> f32 {
    DEFAULT_VIEW_HEIGHT_TILES * aspect_ratio
}

#[allow(clippy::cast_precision_loss)]
fn milli_to_tiles(value: i32) -> f32 {
    value as f32 / MILLI_TILES_PER_TILE as f32
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
pub(crate) fn spawn_arena_view(
    mut commands: Commands,
    arena: Res<LoadedArena>,
    diagnostics: Res<PackageDiagnostics>,
) {
    let plan = build_render_plan(&arena.0).expect("validated arena must produce a render plan");
    commands.spawn((
        Name::new("LocalLab Camera"),
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: DEFAULT_VIEW_HEIGHT_TILES,
            },
            ..OrthographicProjection::default_2d()
        }),
        Transform::from_xyz(plan.player_spawn.x, plan.player_spawn.y, 100.0),
    ));

    spawn_rectangle(
        &mut commands,
        "Walkable floor",
        plan.floor,
        Color::srgb_u8(24, 31, 37),
        Z_FLOOR,
        ArenaVisual::Floor,
    );
    spawn_grid(&mut commands, &arena.0);
    for (index, rectangle) in plan.shell.iter().copied().enumerate() {
        spawn_rectangle(
            &mut commands,
            format!("Shell {index}"),
            rectangle,
            Color::srgb_u8(52, 58, 61),
            Z_SOLID,
            ArenaVisual::Shell,
        );
    }
    for (index, rectangle) in plan.pillars.iter().copied().enumerate() {
        spawn_rectangle(
            &mut commands,
            format!("Pillar {index}"),
            rectangle,
            Color::srgb_u8(67, 72, 72),
            Z_SOLID,
            ArenaVisual::Pillar,
        );
        spawn_rectangle_outline(&mut commands, rectangle, Color::srgb_u8(111, 103, 83));
    }

    spawn_marker(
        &mut commands,
        "PLAYER SPAWN",
        plan.player_spawn,
        Color::srgb_u8(82, 211, 178),
        0.58,
        FRAC_PI_4,
        ArenaVisual::PlayerSpawn,
    );
    spawn_marker(
        &mut commands,
        "BOSS SPAWN",
        plan.boss_spawn,
        Color::srgb_u8(196, 103, 112),
        0.72,
        0.0,
        ArenaVisual::BossSpawn,
    );
    for (id, point) in &plan.anchors {
        let (color, size, kind) = match id.as_str() {
            "reward_pedestal" => (
                Color::srgb_u8(224, 184, 92),
                0.48,
                ArenaVisual::RewardAnchor,
            ),
            "tonic_refill" => (
                Color::srgb_u8(103, 181, 210),
                0.48,
                ArenaVisual::TonicAnchor,
            ),
            _ => (Color::srgb_u8(178, 155, 103), 0.28, ArenaVisual::WaveAnchor),
        };
        spawn_marker(&mut commands, id, *point, color, size, FRAC_PI_4, kind);
    }
    spawn_hud(&mut commands, &arena.0, &diagnostics);

    info!(
        feature_id = "GB-M01-01A",
        arena_id = %arena.0.id,
        content_version = %diagnostics.content_version,
        content_hash = %diagnostics.package_hash_blake3,
        content_root = %diagnostics.content_root.display(),
        pillars = arena.0.pillars.len(),
        anchors = arena.0.anchors.len(),
        "Bell Laboratory presentation initialized"
    );
}

fn spawn_grid(commands: &mut Commands, arena: &ArenaGeometry) {
    let width_tiles = arena.width_milli_tiles / MILLI_TILES_PER_TILE;
    let height_tiles = arena.height_milli_tiles / MILLI_TILES_PER_TILE;
    let floor_width = milli_to_tiles(arena.width_milli_tiles);
    let floor_height = milli_to_tiles(arena.height_milli_tiles);
    let color = Color::srgba_u8(109, 125, 126, 38);
    for x in 0..=width_tiles {
        let center = authored_point_to_render(
            TilePoint::new(x * MILLI_TILES_PER_TILE, arena.height_milli_tiles / 2),
            arena,
        );
        spawn_rectangle(
            commands,
            format!("Grid vertical {x}"),
            RenderRectangle {
                center,
                size: Vec2::new(GRID_LINE_THICKNESS_TILES, floor_height),
            },
            color,
            Z_GRID,
            ArenaVisual::Grid,
        );
    }
    for y in 0..=height_tiles {
        let center = authored_point_to_render(
            TilePoint::new(arena.width_milli_tiles / 2, y * MILLI_TILES_PER_TILE),
            arena,
        );
        spawn_rectangle(
            commands,
            format!("Grid horizontal {y}"),
            RenderRectangle {
                center,
                size: Vec2::new(floor_width, GRID_LINE_THICKNESS_TILES),
            },
            color,
            Z_GRID,
            ArenaVisual::Grid,
        );
    }
}

fn spawn_rectangle(
    commands: &mut Commands,
    name: impl Into<String>,
    rectangle: RenderRectangle,
    color: Color,
    z: f32,
    kind: ArenaVisual,
) {
    commands.spawn((
        Name::new(name.into()),
        kind,
        Sprite::from_color(color, rectangle.size),
        Transform::from_xyz(rectangle.center.x, rectangle.center.y, z),
    ));
}

fn spawn_rectangle_outline(commands: &mut Commands, rectangle: RenderRectangle, color: Color) {
    const THICKNESS: f32 = 0.06;
    for (index, edge) in [
        RenderRectangle {
            center: rectangle.center + Vec2::new(0.0, rectangle.size.y * 0.5),
            size: Vec2::new(rectangle.size.x, THICKNESS),
        },
        RenderRectangle {
            center: rectangle.center - Vec2::new(0.0, rectangle.size.y * 0.5),
            size: Vec2::new(rectangle.size.x, THICKNESS),
        },
        RenderRectangle {
            center: rectangle.center - Vec2::new(rectangle.size.x * 0.5, 0.0),
            size: Vec2::new(THICKNESS, rectangle.size.y),
        },
        RenderRectangle {
            center: rectangle.center + Vec2::new(rectangle.size.x * 0.5, 0.0),
            size: Vec2::new(THICKNESS, rectangle.size.y),
        },
    ]
    .into_iter()
    .enumerate()
    {
        spawn_rectangle(
            commands,
            format!("Pillar outline {index}"),
            edge,
            color,
            Z_SOLID + 0.1,
            ArenaVisual::Pillar,
        );
    }
}

fn spawn_marker(
    commands: &mut Commands,
    label: &str,
    position: Vec2,
    color: Color,
    size: f32,
    rotation_radians: f32,
    kind: ArenaVisual,
) {
    commands.spawn((
        Name::new(label.to_owned()),
        kind,
        Sprite::from_color(color, Vec2::splat(size)),
        Transform::from_xyz(position.x, position.y, Z_MARKER)
            .with_rotation(Quat::from_rotation_z(rotation_radians)),
    ));
    commands.spawn((
        Name::new(format!("{label} label")),
        ArenaVisual::Label,
        Text2d::new(label),
        TextFont::from_font_size(13.0),
        TextColor(Color::srgb_u8(224, 219, 202)),
        Transform::from_xyz(position.x + 0.34, position.y + 0.28, Z_LABEL)
            .with_scale(Vec3::splat(WORLD_LABEL_SCALE)),
        Anchor::BOTTOM_LEFT,
    ));
}

fn spawn_hud(commands: &mut Commands, arena: &ArenaGeometry, diagnostics: &PackageDiagnostics) {
    let short_hash = diagnostics
        .package_hash_blake3
        .get(..12)
        .unwrap_or(&diagnostics.package_hash_blake3);
    commands.spawn((
        Name::new("Foundation diagnostics"),
        Text::new(format!(
            "GRAVEBOUND  /  LOCAL LAB\nGB-M01-01A  |  {}\n{}  |  {} Hz  |  content {}  |  {} records  |  hash {}",
            arena.id,
            "24 x 13.5 TILE ORTHOGRAPHIC VIEW",
            sim_core::TICKS_PER_SECOND,
            diagnostics.content_version,
            diagnostics.record_count,
            short_hash
        )),
        TextFont::from_font_size(15.0),
        TextColor(Color::srgb_u8(232, 225, 203)),
        Node {
            position_type: PositionType::Absolute,
            top: px(14),
            left: px(14),
            border: UiRect::all(px(1)),
            padding: UiRect::all(px(10)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(8, 12, 16, 225)),
        BorderColor::all(Color::srgba_u8(169, 142, 82, 180)),
    ));
    commands.spawn((
        Name::new("Arena legend"),
        Text::new("[P] PLAYER   [B] BOSS   [W] WAVE   [R] GOLD REWARD   [T] BLUE TONIC\nNW AUTHORED ORIGIN  /  +X EAST  /  +Y SOUTH  /  1 TILE = 1 WORLD UNIT"),
        TextFont::from_font_size(13.0),
        TextColor(Color::srgb_u8(197, 203, 196)),
        Node {
            position_type: PositionType::Absolute,
            bottom: px(14),
            left: px(14),
            border: UiRect::all(px(1)),
            padding: UiRect::all(px(8)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(8, 12, 16, 220)),
        BorderColor::all(Color::srgba_u8(109, 125, 126, 140)),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{ArenaAnchor, TileRectangle};

    fn exact_arena() -> ArenaGeometry {
        ArenaGeometry {
            id: "arena.prototype.bell_laboratory_01".to_owned(),
            width_milli_tiles: 32_000,
            height_milli_tiles: 24_000,
            shell_thickness_milli_tiles: 1_000,
            player_spawn: TilePoint::new(4_000, 12_000),
            boss_spawn: TilePoint::new(24_000, 12_000),
            pillars: vec![TileRectangle::new(10_000, 5_000, 2_000, 3_000)],
            anchors: vec![ArenaAnchor {
                id: "N1".to_owned(),
                point: TilePoint::new(8_000, 3_000),
            }],
        }
        .validated()
        .expect("arena")
    }

    #[test]
    fn northwest_coordinates_map_to_centered_render_space() {
        let arena = exact_arena();
        assert_eq!(
            authored_point_to_render(TilePoint::new(0, 0), &arena),
            Vec2::new(-16.0, 12.0)
        );
        assert_eq!(
            authored_point_to_render(TilePoint::new(32_000, 24_000), &arena),
            Vec2::new(16.0, -12.0)
        );
        assert_eq!(
            authored_point_to_render(arena.player_spawn, &arena),
            Vec2::new(-12.0, 0.0)
        );
    }

    #[test]
    fn plan_preserves_authored_sizes_and_locations() {
        let plan = build_render_plan(&exact_arena()).expect("plan");
        assert_eq!(plan.floor.size, Vec2::new(32.0, 24.0));
        assert_eq!(plan.shell.len(), 4);
        assert_eq!(plan.pillars[0].size, Vec2::new(2.0, 3.0));
        assert_eq!(plan.pillars[0].center, Vec2::new(-5.0, 5.5));
        assert_eq!(plan.player_spawn, Vec2::new(-12.0, 0.0));
        assert_eq!(plan.boss_spawn, Vec2::new(8.0, 0.0));
        assert_eq!(plan.anchors[0].1, Vec2::new(-8.0, 9.0));
    }

    #[test]
    fn fixed_vertical_projection_is_exact_at_sixteen_by_nine() {
        let width = visible_width_for_aspect(16.0 / 9.0);
        assert!((width - DEFAULT_VIEW_WIDTH_AT_16_9_TILES).abs() < f32::EPSILON);
    }
}
