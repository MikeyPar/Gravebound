//! Disposable native presentation for the exact unpromoted `GB-M03-03C` scenes.

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    path::PathBuf,
};

use anyhow::{Context, Result};
use bevy::{
    camera::ScalingMode,
    prelude::*,
    render::view::screenshot::Screenshot,
    sprite::Anchor,
    window::{PresentMode, PrimaryWindow, WindowResolution},
};
use sim_content::{CoreDevelopmentWorldFlow, load_core_development_world_flow};
use sim_core::{
    CoreMicrorealmEvent, CoreMicrorealmInput, CoreMicrorealmPhase, CoreMicrorealmSimulation,
    MILLI_TILES_PER_TILE, SceneAccessContext, SceneDisplacement, SceneInteractionEvent,
    SceneInteractionRejection, SceneInteractionSession, SceneObjectCondition, SceneObjectGeometry,
    Tick, TilePoint, TileRectangle, WorldSceneDefinition, WorldSceneKind, WorldScenePlayer,
};

const VIEW_HEIGHT_TILES: f32 = 20.0;
const Z_FLOOR: f32 = 0.0;
const Z_ROAD: f32 = 1.0;
const Z_GRID: f32 = 2.0;
const Z_SOLID: f32 = 3.0;
const Z_OBJECT: f32 = 4.0;
const Z_LABEL: f32 = 5.0;
const Z_PLAYER: f32 = 6.0;
const LABEL_SCALE: f32 = 0.024;
const EVIDENCE_SETTLE_FRAMES: u8 = 30;
const CARDINAL_STEP_MILLI_TILES: i32 = 170;
const DIAGONAL_STEP_MILLI_TILES: i32 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreWorldShowcaseScene {
    Hall,
    Microrealm,
}

#[derive(Debug, Clone)]
pub struct CoreWorldShowcaseConfig {
    pub content_root: PathBuf,
    pub scene: CoreWorldShowcaseScene,
    pub reduced_motion: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct RenderRectangle {
    center: Vec2,
    size: Vec2,
}

#[derive(Debug, Clone, PartialEq)]
struct RenderCircle {
    center: Vec2,
    radius: f32,
}

#[derive(Debug, Clone, PartialEq)]
enum ObjectRenderGeometry {
    Point(Vec2),
    Circle(RenderCircle),
    Rectangle(RenderRectangle),
}

#[derive(Debug, Clone, PartialEq)]
struct ObjectRenderPlan {
    id: String,
    geometry: ObjectRenderGeometry,
    integration_gated: bool,
    condition: SceneObjectCondition,
    label_lane: u16,
}

#[derive(Debug, Clone, Copy)]
struct ObjectLabelLayout {
    position: Vec2,
    anchor: Anchor,
    justify: Justify,
}

#[derive(Debug, Clone, PartialEq)]
struct WorldShowcaseRenderPlan {
    floor: RenderRectangle,
    shell: [RenderRectangle; 4],
    solids: Vec<RenderRectangle>,
    roads: Vec<RenderRectangle>,
    objects: Vec<ObjectRenderPlan>,
    player_spawn: Vec2,
}

#[derive(Debug, Clone)]
struct SceneLabel {
    name: String,
    description: String,
}

#[derive(Debug, Resource)]
struct ShowcaseScene {
    definition: WorldSceneDefinition,
    labels: BTreeMap<String, SceneLabel>,
    revision: String,
    reduced_motion: bool,
}

#[derive(Debug, Resource)]
struct ShowcaseRuntime {
    player: WorldScenePlayer,
    interaction: SceneInteractionSession,
    microrealm: Option<CoreMicrorealmSimulation>,
    tick: Tick,
    prompt: String,
    state: String,
    faulted: bool,
}

#[derive(Debug, Component)]
struct ShowcasePlayer;

#[derive(Debug, Component)]
struct ShowcaseCamera;

#[derive(Debug, Component)]
struct SafeRingMarker;

#[derive(Debug, Component)]
struct ShowcaseObjectLabel {
    focus: Vec2,
}

#[derive(Debug, Component)]
struct ShowcasePromptText;

#[derive(Debug, Component)]
struct ShowcaseStateText;

#[derive(Debug, Resource)]
struct ShowcaseScreenshotRequest(PathBuf);

#[derive(Debug, Default)]
struct CaptureProgress {
    settled_frames: u8,
    queued: bool,
}

#[allow(clippy::needless_pass_by_value)] // Matches the other owned native-client launch configs.
pub fn run_core_world_showcase(config: CoreWorldShowcaseConfig) -> Result<()> {
    let compiled = load_core_development_world_flow(&config.content_root)
        .context("unpromoted Core world-flow content failed validation")?;
    let definition = match config.scene {
        CoreWorldShowcaseScene::Hall => compiled.compile_hall_scene()?,
        CoreWorldShowcaseScene::Microrealm => compiled.compile_microrealm_scene()?,
    };
    let labels = scene_labels(&compiled, &definition)?;
    let scene_name = labels
        .get(&definition.id)
        .map_or_else(|| definition.id.clone(), |label| label.name.clone());
    let revision = compiled
        .hashes()
        .records_blake3
        .get(..12)
        .unwrap_or(&compiled.hashes().records_blake3)
        .to_owned();
    let screenshot_request = env::var_os("GRAVEBOUND_SCREENSHOT_PATH").map(PathBuf::from);
    let (window_width, window_height) = crate::configured_window_size()?;
    let mut app = App::new();
    let player = WorldScenePlayer::new(
        &definition,
        definition.player_spawn,
        CARDINAL_STEP_MILLI_TILES,
    )
    .context("Core showcase player spawn is invalid")?;
    let microrealm = (definition.kind == WorldSceneKind::PrivateDanger)
        .then(|| CoreMicrorealmSimulation::new(definition.player_spawn));
    app.insert_resource(ClearColor(Color::srgb_u8(7, 9, 12)))
        .insert_resource(Time::<Fixed>::from_hz(f64::from(
            sim_core::TICKS_PER_SECOND,
        )))
        .insert_resource(ShowcaseScene {
            definition,
            labels,
            revision,
            reduced_motion: config.reduced_motion,
        })
        .insert_resource(ShowcaseRuntime {
            player,
            interaction: SceneInteractionSession::default(),
            microrealm,
            tick: Tick(0),
            prompt: "MOVE WITH WASD OR ARROWS".to_owned(),
            state: "AUTHORITATIVE SCENE READY".to_owned(),
            faulted: false,
        })
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: format!("Gravebound - {scene_name} [Disposable Core Showcase]"),
                        resolution: WindowResolution::new(window_width, window_height),
                        present_mode: PresentMode::AutoVsync,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_systems(Startup, spawn_world_showcase)
        .add_systems(FixedUpdate, step_world_showcase)
        .add_systems(
            Update,
            (
                sync_showcase_presentation,
                sync_showcase_copy,
                animate_safe_markers,
            )
                .chain(),
        );
    if let Some(path) = screenshot_request {
        app.insert_resource(ShowcaseScreenshotRequest(path))
            .add_systems(
                Update,
                capture_world_showcase_evidence
                    .after(sync_showcase_presentation)
                    .after(sync_showcase_copy),
            );
    }
    app.run();
    Ok(())
}

fn scene_labels(
    compiled: &CoreDevelopmentWorldFlow,
    scene: &WorldSceneDefinition,
) -> Result<BTreeMap<String, SceneLabel>> {
    std::iter::once(scene.id.as_str())
        .chain(scene.objects.iter().map(|object| object.id.as_str()))
        .map(|id| {
            let name_key = format!("{id}.name");
            let description_key = format!("{id}.description");
            Ok((
                id.to_owned(),
                SceneLabel {
                    name: compiled
                        .localized(&name_key)
                        .with_context(|| format!("missing localized scene name {name_key}"))?
                        .to_owned(),
                    description: compiled
                        .localized(&description_key)
                        .with_context(|| {
                            format!("missing localized scene description {description_key}")
                        })?
                        .to_owned(),
                },
            ))
        })
        .collect()
}

fn build_render_plan(scene: &WorldSceneDefinition) -> WorldShowcaseRenderPlan {
    let floor = authored_rectangle_to_render(
        TileRectangle::new(0, 0, scene.width_milli_tiles, scene.height_milli_tiles),
        scene,
    );
    let shell =
        shell_rectangles(scene).map(|rectangle| authored_rectangle_to_render(rectangle, scene));
    let solids = scene
        .solid_rectangles
        .iter()
        .copied()
        .map(|rectangle| authored_rectangle_to_render(rectangle, scene))
        .collect();
    let roads = scene
        .roads
        .iter()
        .flat_map(|road| {
            road.points
                .windows(2)
                .map(|pair| road_segment_to_render(pair[0], pair[1], road.width_milli_tiles, scene))
        })
        .collect();
    let mut label_lanes = BTreeMap::<(u8, i32, i32), u16>::new();
    let objects = scene
        .objects
        .iter()
        .map(|object| {
            let label_key = match object.geometry {
                SceneObjectGeometry::Point(point)
                | SceneObjectGeometry::PointInteractable { point, .. } => {
                    (0, point.x_milli_tiles, point.y_milli_tiles)
                }
                SceneObjectGeometry::Circle { center, .. } => {
                    (1, center.x_milli_tiles, center.y_milli_tiles)
                }
                SceneObjectGeometry::Rectangle(rectangle) => (
                    2,
                    rectangle.x_milli_tiles + rectangle.width_milli_tiles / 2,
                    rectangle.y_milli_tiles + rectangle.height_milli_tiles / 2,
                ),
            };
            let label_lane = label_lanes.entry(label_key).or_default();
            let assigned_label_lane = *label_lane;
            *label_lane = label_lane.saturating_add(1);
            ObjectRenderPlan {
                id: object.id.clone(),
                geometry: match object.geometry {
                    SceneObjectGeometry::Point(point)
                    | SceneObjectGeometry::PointInteractable { point, .. } => {
                        ObjectRenderGeometry::Point(authored_point_to_render(point, scene))
                    }
                    SceneObjectGeometry::Circle {
                        center,
                        radius_milli_tiles,
                    } => ObjectRenderGeometry::Circle(RenderCircle {
                        center: authored_point_to_render(center, scene),
                        radius: milli_to_tiles(radius_milli_tiles),
                    }),
                    SceneObjectGeometry::Rectangle(rectangle) => ObjectRenderGeometry::Rectangle(
                        authored_rectangle_to_render(rectangle, scene),
                    ),
                },
                integration_gated: object.integration_gate.is_some(),
                condition: object.condition,
                label_lane: assigned_label_lane,
            }
        })
        .collect();
    WorldShowcaseRenderPlan {
        floor,
        shell,
        solids,
        roads,
        objects,
        player_spawn: authored_point_to_render(scene.player_spawn, scene),
    }
}

fn shell_rectangles(scene: &WorldSceneDefinition) -> [TileRectangle; 4] {
    let thickness = scene.shell_thickness_milli_tiles;
    [
        TileRectangle::new(0, 0, scene.width_milli_tiles, thickness),
        TileRectangle::new(
            0,
            scene.height_milli_tiles - thickness,
            scene.width_milli_tiles,
            thickness,
        ),
        TileRectangle::new(
            0,
            thickness,
            thickness,
            scene.height_milli_tiles - 2 * thickness,
        ),
        TileRectangle::new(
            scene.width_milli_tiles - thickness,
            thickness,
            thickness,
            scene.height_milli_tiles - 2 * thickness,
        ),
    ]
}

fn road_segment_to_render(
    start: TilePoint,
    end: TilePoint,
    width_milli_tiles: i32,
    scene: &WorldSceneDefinition,
) -> RenderRectangle {
    let center = TilePoint::new(
        start.x_milli_tiles.midpoint(end.x_milli_tiles),
        start.y_milli_tiles.midpoint(end.y_milli_tiles),
    );
    let horizontal = start.y_milli_tiles == end.y_milli_tiles;
    RenderRectangle {
        center: authored_point_to_render(center, scene),
        size: if horizontal {
            Vec2::new(
                milli_to_tiles((end.x_milli_tiles - start.x_milli_tiles).abs() + width_milli_tiles),
                milli_to_tiles(width_milli_tiles),
            )
        } else {
            Vec2::new(
                milli_to_tiles(width_milli_tiles),
                milli_to_tiles((end.y_milli_tiles - start.y_milli_tiles).abs() + width_milli_tiles),
            )
        },
    }
}

fn authored_rectangle_to_render(
    rectangle: TileRectangle,
    scene: &WorldSceneDefinition,
) -> RenderRectangle {
    let center = TilePoint::new(
        rectangle.x_milli_tiles + rectangle.width_milli_tiles / 2,
        rectangle.y_milli_tiles + rectangle.height_milli_tiles / 2,
    );
    RenderRectangle {
        center: authored_point_to_render(center, scene),
        size: Vec2::new(
            milli_to_tiles(rectangle.width_milli_tiles),
            milli_to_tiles(rectangle.height_milli_tiles),
        ),
    }
}

fn authored_point_to_render(point: TilePoint, scene: &WorldSceneDefinition) -> Vec2 {
    Vec2::new(
        milli_to_tiles(point.x_milli_tiles) - milli_to_tiles(scene.width_milli_tiles) * 0.5,
        milli_to_tiles(scene.height_milli_tiles) * 0.5 - milli_to_tiles(point.y_milli_tiles),
    )
}

#[allow(clippy::cast_precision_loss)]
fn milli_to_tiles(value: i32) -> f32 {
    value as f32 / MILLI_TILES_PER_TILE as f32
}

#[allow(clippy::needless_pass_by_value)]
fn spawn_world_showcase(mut commands: Commands, scene: Res<ShowcaseScene>) {
    let plan = build_render_plan(&scene.definition);
    commands.spawn((
        Name::new("Core world showcase camera"),
        ShowcaseCamera,
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: VIEW_HEIGHT_TILES,
            },
            ..OrthographicProjection::default_2d()
        }),
        Transform::from_xyz(plan.player_spawn.x, plan.player_spawn.y, 100.0),
    ));
    spawn_rectangle(
        &mut commands,
        "Scene floor",
        &plan.floor,
        if scene.definition.kind == WorldSceneKind::SafeHub {
            Color::srgb_u8(28, 31, 35)
        } else {
            Color::srgb_u8(42, 43, 35)
        },
        Z_FLOOR,
    );
    spawn_grid(&mut commands, &scene.definition);
    for (index, road) in plan.roads.iter().enumerate() {
        spawn_rectangle(
            &mut commands,
            format!("Road segment {index}"),
            road,
            Color::srgb_u8(79, 73, 57),
            Z_ROAD,
        );
    }
    for (index, shell) in plan.shell.iter().enumerate() {
        spawn_stone_block(&mut commands, format!("Shell {index}"), shell);
    }
    for (index, solid) in plan.solids.iter().enumerate() {
        spawn_stone_block(&mut commands, format!("Hall solid {index}"), solid);
    }
    for object in &plan.objects {
        spawn_scene_object(&mut commands, object, &scene);
    }
    spawn_player(&mut commands, plan.player_spawn);
    spawn_hud(&mut commands, &scene);
}

fn spawn_grid(commands: &mut Commands, scene: &WorldSceneDefinition) {
    let width = milli_to_tiles(scene.width_milli_tiles);
    let height = milli_to_tiles(scene.height_milli_tiles);
    let width_tiles = scene.width_milli_tiles / MILLI_TILES_PER_TILE;
    let height_tiles = scene.height_milli_tiles / MILLI_TILES_PER_TILE;
    let color = Color::srgba_u8(157, 143, 112, 20);
    for x in (4..width_tiles).step_by(4) {
        spawn_rectangle(
            commands,
            format!("Grid x{x}"),
            &RenderRectangle {
                center: Vec2::new(milli_to_tiles(x * MILLI_TILES_PER_TILE) - width * 0.5, 0.0),
                size: Vec2::new(0.025, height),
            },
            color,
            Z_GRID,
        );
    }
    for y in (4..height_tiles).step_by(4) {
        spawn_rectangle(
            commands,
            format!("Grid y{y}"),
            &RenderRectangle {
                center: Vec2::new(0.0, height * 0.5 - milli_to_tiles(y * MILLI_TILES_PER_TILE)),
                size: Vec2::new(width, 0.025),
            },
            color,
            Z_GRID,
        );
    }
}

fn spawn_rectangle(
    commands: &mut Commands,
    name: impl Into<String>,
    rectangle: &RenderRectangle,
    color: Color,
    z: f32,
) {
    commands.spawn((
        Name::new(name.into()),
        Sprite::from_color(color, rectangle.size),
        Transform::from_xyz(rectangle.center.x, rectangle.center.y, z),
    ));
}

fn spawn_stone_block(
    commands: &mut Commands,
    name: impl Into<String>,
    rectangle: &RenderRectangle,
) {
    let name = name.into();
    spawn_rectangle(
        commands,
        name.clone(),
        rectangle,
        Color::srgb_u8(18, 20, 24),
        Z_SOLID,
    );
    spawn_rectangle_outline(commands, &name, rectangle, Color::srgb_u8(101, 83, 54));
}

fn spawn_rectangle_outline(
    commands: &mut Commands,
    name: &str,
    rectangle: &RenderRectangle,
    color: Color,
) {
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
            format!("{name} edge {index}"),
            &edge,
            color,
            Z_SOLID + 0.1,
        );
    }
}

fn spawn_scene_object(commands: &mut Commands, object: &ObjectRenderPlan, scene: &ShowcaseScene) {
    let label = scene
        .labels
        .get(&object.id)
        .expect("validated localized object");
    match &object.geometry {
        ObjectRenderGeometry::Point(position) => {
            spawn_object_marker(commands, object, *position, label);
        }
        ObjectRenderGeometry::Circle(circle) => {
            spawn_circle_markers(commands, object, circle);
            spawn_object_label(commands, object, label);
        }
        ObjectRenderGeometry::Rectangle(rectangle) => {
            let color = object_color(&object.id, object.integration_gated);
            spawn_rectangle(commands, &object.id, rectangle, color, Z_OBJECT);
            spawn_rectangle_outline(
                commands,
                &object.id,
                rectangle,
                Color::srgb_u8(212, 177, 91),
            );
            spawn_object_label(commands, object, label);
        }
    }
}

fn spawn_object_marker(
    commands: &mut Commands,
    object: &ObjectRenderPlan,
    position: Vec2,
    label: &SceneLabel,
) {
    let size = if object.id == "station.realm_gate" {
        Vec2::new(1.25, 0.65)
    } else {
        Vec2::splat(0.82)
    };
    commands.spawn((
        Name::new(object.id.clone()),
        Sprite::from_color(object_color(&object.id, object.integration_gated), size),
        Transform::from_xyz(position.x, position.y, Z_OBJECT).with_rotation(
            if object.id == "station.oath_shrine" {
                Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)
            } else {
                Quat::IDENTITY
            },
        ),
    ));
    spawn_object_label(commands, object, label);
}

fn spawn_circle_markers(commands: &mut Commands, object: &ObjectRenderPlan, circle: &RenderCircle) {
    let color = object_color(&object.id, object.integration_gated);
    for index in 0_u8..24 {
        let angle = f32::from(index) * std::f32::consts::TAU / 24.0;
        let position = circle.center + Vec2::new(angle.cos(), angle.sin()) * circle.radius;
        commands.spawn((
            Name::new(format!("{} ring {index}", object.id)),
            SafeRingMarker,
            Sprite::from_color(color, Vec2::splat(0.20)),
            Transform::from_xyz(position.x, position.y, Z_OBJECT),
        ));
    }
}

fn spawn_object_label(commands: &mut Commands, object: &ObjectRenderPlan, label: &SceneLabel) {
    let id = object.id.as_str();
    let layout = object_label_layout(object);
    commands.spawn((
        Name::new(format!("{id} label")),
        ShowcaseObjectLabel {
            focus: object_label_focus(object),
        },
        Text2d::new(if id.starts_with("station.") {
            format!("{}\nAVAILABLE IN A LATER TEST", label.name)
        } else {
            label.name.clone()
        }),
        TextFont::from_font_size(14.0),
        TextColor(if id.starts_with("station.") {
            Color::srgb_u8(184, 184, 178)
        } else {
            Color::srgb_u8(230, 218, 181)
        }),
        TextLayout::justify(layout.justify),
        Transform::from_xyz(layout.position.x, layout.position.y, Z_LABEL)
            .with_scale(Vec3::splat(LABEL_SCALE)),
        layout.anchor,
    ));
}

fn object_label_focus(object: &ObjectRenderPlan) -> Vec2 {
    match object.geometry {
        ObjectRenderGeometry::Point(position) => position,
        ObjectRenderGeometry::Circle(ref circle) => circle.center,
        ObjectRenderGeometry::Rectangle(ref rectangle) => rectangle.center,
    }
}

fn object_label_layout(object: &ObjectRenderPlan) -> ObjectLabelLayout {
    const MARKER_GAP: f32 = 0.55;
    const OUTLINE_GAP: f32 = 0.35;
    const LABEL_LANE_HEIGHT: f32 = 0.52;
    let lane_offset = f32::from(object.label_lane) * LABEL_LANE_HEIGHT;
    match object.geometry {
        ObjectRenderGeometry::Point(position) if position.x > 0.0 => ObjectLabelLayout {
            position: position + Vec2::new(-MARKER_GAP, MARKER_GAP + lane_offset),
            anchor: Anchor::BOTTOM_RIGHT,
            justify: Justify::Right,
        },
        ObjectRenderGeometry::Point(position) => ObjectLabelLayout {
            position: position + Vec2::new(MARKER_GAP, MARKER_GAP + lane_offset),
            anchor: Anchor::BOTTOM_LEFT,
            justify: Justify::Left,
        },
        ObjectRenderGeometry::Circle(ref circle) => ObjectLabelLayout {
            position: circle.center + Vec2::new(0.0, circle.radius + OUTLINE_GAP + lane_offset),
            anchor: Anchor::BOTTOM_CENTER,
            justify: Justify::Center,
        },
        ObjectRenderGeometry::Rectangle(ref rectangle) => ObjectLabelLayout {
            position: rectangle.center
                + Vec2::new(0.0, rectangle.size.y * 0.5 + OUTLINE_GAP + lane_offset),
            anchor: Anchor::BOTTOM_CENTER,
            justify: Justify::Center,
        },
    }
}

fn object_color(id: &str, gated: bool) -> Color {
    if gated {
        return Color::srgb_u8(91, 92, 91);
    }
    if id.contains("lantern_fork") {
        Color::srgb_u8(225, 174, 72)
    } else if id.contains("realm_gate") {
        Color::srgb_u8(177, 139, 69)
    } else if id.contains("bell_sepulcher") {
        Color::srgb_u8(107, 79, 131)
    } else {
        Color::srgb_u8(139, 126, 94)
    }
}

fn spawn_player(commands: &mut Commands, position: Vec2) {
    commands
        .spawn((
            Name::new("Grave Arbalist showcase player"),
            ShowcasePlayer,
            Transform::from_xyz(position.x, position.y, Z_PLAYER),
            Visibility::Visible,
        ))
        .with_children(|player| {
            player.spawn((
                Sprite::from_color(Color::srgb_u8(202, 211, 204), Vec2::new(0.48, 0.62)),
                Transform::from_xyz(0.0, 0.08, 0.0),
            ));
            player.spawn((
                Sprite::from_color(Color::srgb_u8(99, 119, 112), Vec2::new(0.72, 0.20)),
                Transform::from_xyz(0.0, -0.22, 0.1),
            ));
            player.spawn((
                Sprite::from_color(Color::srgb_u8(173, 141, 79), Vec2::new(0.68, 0.10)),
                Transform::from_xyz(0.34, 0.02, 0.2),
            ));
        });
}

fn spawn_hud(commands: &mut Commands, scene: &ShowcaseScene) {
    let label = scene.labels.get(&scene.definition.id).expect("scene label");
    let safety = if scene.definition.kind == WorldSceneKind::SafeHub {
        "SAFE NONCOMBAT  /  HOSTILE • DAMAGE • PROJECTILE • PICKUP • DROP CREATION PROHIBITED"
    } else {
        "PRIVATE DANGER  /  CAPACITY 1  /  MACRO CYCLE • SIEGE • RETIREMENT DISABLED"
    };
    commands.spawn((
        Name::new("World showcase header"),
        Text::new(format!(
            "GRAVEBOUND  /  GB-M03-03C  /  {}\n{}\n{}\n{}  /  records {}  /  {}",
            label.name.to_uppercase(),
            label.description,
            safety,
            scene.definition.id,
            scene.revision,
            if scene.reduced_motion {
                "REDUCED MOTION"
            } else {
                "STANDARD MOTION"
            }
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
        BackgroundColor(Color::srgba_u8(8, 11, 14, 232)),
        BorderColor::all(Color::srgba_u8(169, 142, 82, 190)),
    ));
    commands.spawn((
        Name::new("World showcase footer"),
        Text::new("DISPOSABLE CORE SHOWCASE  •  NORMAL PLAYER ROUTE DISABLED\nWASD / ARROWS MOVE  •  E INTERACT  •  ESC CLOSE  •  NORTHWEST AUTHORED ORIGIN"),
        TextFont::from_font_size(13.0),
        TextColor(Color::srgb_u8(199, 203, 196)),
        Node {
            position_type: PositionType::Absolute,
            bottom: px(14),
            left: px(14),
            border: UiRect::all(px(1)),
            padding: UiRect::all(px(8)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(8, 11, 14, 220)),
        BorderColor::all(Color::srgba_u8(101, 83, 54, 190)),
    ));
    commands.spawn((
        Name::new("World showcase state"),
        ShowcaseStateText,
        Text::new("AUTHORITATIVE SCENE READY"),
        TextFont::from_font_size(14.0),
        TextColor(Color::srgb_u8(226, 186, 91)),
        Node {
            position_type: PositionType::Absolute,
            top: px(120),
            right: px(14),
            border: UiRect::all(px(1)),
            padding: UiRect::all(px(9)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(8, 11, 14, 225)),
        BorderColor::all(Color::srgba_u8(169, 142, 82, 190)),
    ));
    commands.spawn((
        Name::new("World showcase interaction prompt"),
        ShowcasePromptText,
        Text::new("MOVE WITH WASD OR ARROWS"),
        TextFont::from_font_size(14.0),
        TextColor(Color::srgb_u8(232, 225, 203)),
        Node {
            position_type: PositionType::Absolute,
            bottom: px(14),
            right: px(14),
            border: UiRect::all(px(1)),
            padding: UiRect::all(px(9)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(8, 11, 14, 225)),
        BorderColor::all(Color::srgba_u8(126, 112, 76, 190)),
    ));
}

#[allow(clippy::needless_pass_by_value)]
fn step_world_showcase(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    scene: Res<ShowcaseScene>,
    mut runtime: ResMut<ShowcaseRuntime>,
) {
    if runtime.faulted {
        return;
    }
    let Some(next_tick) = runtime.tick.checked_next() else {
        fail_showcase(&mut runtime, "authoritative tick overflow");
        return;
    };
    runtime.tick = next_tick;
    if runtime.interaction.open_panel_object_id().is_none() {
        let horizontal = key_axis(
            &keyboard,
            [KeyCode::KeyA, KeyCode::ArrowLeft],
            [KeyCode::KeyD, KeyCode::ArrowRight],
        );
        let vertical = key_axis(
            &keyboard,
            [KeyCode::KeyW, KeyCode::ArrowUp],
            [KeyCode::KeyS, KeyCode::ArrowDown],
        );
        let diagonal = horizontal != 0 && vertical != 0;
        let step = if diagonal {
            DIAGONAL_STEP_MILLI_TILES
        } else {
            CARDINAL_STEP_MILLI_TILES
        };
        if runtime
            .player
            .step_movement(
                &scene.definition,
                SceneDisplacement::new(horizontal * step, vertical * step),
            )
            .is_err()
        {
            fail_showcase(&mut runtime, "movement authority rejected a bounded step");
            return;
        }
    }

    step_microrealm(&keyboard, &mouse, &mut runtime);
    let gates = BTreeSet::new();
    let Ok(projection) = runtime.player.nearest_interaction(
        &scene.definition,
        SceneAccessContext {
            enabled_integration_gates: &gates,
            microrealm_cleared: runtime
                .microrealm
                .as_ref()
                .is_some_and(CoreMicrorealmSimulation::bell_portal_available),
        },
    ) else {
        fail_showcase(&mut runtime, "interaction projection failed closed");
        return;
    };
    let interact_held = keyboard.pressed(KeyCode::KeyE) || keyboard.pressed(KeyCode::Enter);
    let close_panel = keyboard.just_pressed(KeyCode::Escape);
    let Ok(interaction_events) =
        runtime
            .interaction
            .step(projection.as_ref(), interact_held, close_panel)
    else {
        fail_showcase(&mut runtime, "interaction authority overflowed");
        return;
    };
    update_interaction_copy(
        &scene,
        &mut runtime,
        projection.as_ref(),
        &interaction_events,
    );
}

fn key_axis(
    keyboard: &ButtonInput<KeyCode>,
    negative: [KeyCode; 2],
    positive: [KeyCode; 2],
) -> i32 {
    let negative = negative.into_iter().any(|key| keyboard.pressed(key));
    let positive = positive.into_iter().any(|key| keyboard.pressed(key));
    i32::from(positive) - i32::from(negative)
}

fn step_microrealm(
    keyboard: &ButtonInput<KeyCode>,
    mouse: &ButtonInput<MouseButton>,
    runtime: &mut ShowcaseRuntime,
) {
    let Some(simulation) = &mut runtime.microrealm else {
        "SAFE NONCOMBAT AUTHORITY".clone_into(&mut runtime.state);
        return;
    };
    let pack_cleared =
        simulation.phase() == CoreMicrorealmPhase::Active && keyboard.just_pressed(KeyCode::KeyC);
    let input = CoreMicrorealmInput {
        entrant_position: runtime.player.position(),
        primary_released: mouse.just_released(MouseButton::Left),
        living_participants: 1,
        pack_cleared,
    };
    match simulation.step(runtime.tick, input) {
        Ok(events) => {
            for event in events {
                runtime.state = match event {
                    CoreMicrorealmEvent::BeginPackWarning { warning_ticks } => format!(
                        "PACK.BELL.01 WARNING REQUESTED  /  {warning_ticks} TICKS  /  03D ENTITY SPAWN DEFERRED"
                    ),
                    CoreMicrorealmEvent::ResetPack => "MICROREALM RESET TO DORMANT".to_owned(),
                    CoreMicrorealmEvent::Cleared => {
                        "MICROREALM CLEARED  /  BELL PORTAL CONDITION SATISFIED".to_owned()
                    }
                };
            }
            if simulation.phase() == CoreMicrorealmPhase::Active && !pack_cleared {
                "PACK.BELL.01 ACTIVE SEAM  /  PRESS C TO SUPPLY DISPOSABLE 03D CLEAR"
                    .clone_into(&mut runtime.state);
            } else if simulation.phase() == CoreMicrorealmPhase::Waiting {
                "ENTRY TRIGGERED  /  1 SECOND PACK DELAY".clone_into(&mut runtime.state);
            } else if simulation.phase() == CoreMicrorealmPhase::Dormant {
                "DORMANT  /  MOVE BEYOND 1 TILE OR RELEASE PRIMARY TO TRIGGER"
                    .clone_into(&mut runtime.state);
            }
        }
        Err(_) => fail_showcase(runtime, "microrealm lifecycle failed closed"),
    }
}

fn update_interaction_copy(
    scene: &ShowcaseScene,
    runtime: &mut ShowcaseRuntime,
    projection: Option<&sim_core::SceneInteractionProjection>,
    events: &[SceneInteractionEvent],
) {
    if let Some(event) = events.last() {
        runtime.prompt = match event {
            SceneInteractionEvent::Progress {
                object_id,
                held_ticks,
                required_ticks,
            } => format!(
                "HOLD E  /  {}  /  {held_ticks} OF {required_ticks} TICKS",
                scene_name(scene, object_id)
            ),
            SceneInteractionEvent::Opened { object_id } => format!(
                "{}  /  ESC CLOSE",
                scene
                    .labels
                    .get(object_id)
                    .map_or(object_id.as_str(), |label| label.description.as_str())
            ),
            SceneInteractionEvent::Closed { object_id } => {
                format!("{} CLOSED WITHOUT MUTATION", scene_name(scene, object_id))
            }
            SceneInteractionEvent::Rejected { object_id, reason } => format!(
                "{}  /  {}",
                scene_name(scene, object_id),
                match reason {
                    SceneInteractionRejection::StageDisabled => {
                        "AVAILABLE IN A LATER TEST  /  STAGE_DISABLED"
                    }
                    SceneInteractionRejection::ConditionUnmet => {
                        "CONDITION NOT MET  /  CONTENT_DISABLED"
                    }
                }
            ),
        };
    } else if let Some(projection) = projection {
        runtime.prompt = format!("E  /  {}", scene_name(scene, &projection.object_id));
    } else {
        "WASD / ARROWS MOVE  /  E INTERACT  /  ESC CLOSE".clone_into(&mut runtime.prompt);
    }
}

fn scene_name<'a>(scene: &'a ShowcaseScene, object_id: &'a str) -> &'a str {
    scene
        .labels
        .get(object_id)
        .map_or(object_id, |label| label.name.as_str())
}

fn fail_showcase(runtime: &mut ShowcaseRuntime, reason: &str) {
    runtime.faulted = true;
    "SCENE AUTHORITY HALTED".clone_into(&mut runtime.state);
    runtime.prompt = format!("SERVICE_UNAVAILABLE  /  {reason}");
}

#[allow(clippy::needless_pass_by_value)]
fn sync_showcase_presentation(
    scene: Res<ShowcaseScene>,
    runtime: Res<ShowcaseRuntime>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut player: Single<&mut Transform, (With<ShowcasePlayer>, Without<ShowcaseCamera>)>,
    mut camera: Single<&mut Transform, (With<ShowcaseCamera>, Without<ShowcasePlayer>)>,
    mut object_labels: Query<(&ShowcaseObjectLabel, &mut Visibility)>,
) {
    let position = authored_point_to_render(runtime.player.position(), &scene.definition);
    player.translation.x = position.x;
    player.translation.y = position.y;
    let camera_center = clamp_camera_center(
        position,
        &scene.definition,
        window.width() / window.height(),
    );
    camera.translation.x = camera_center.x;
    camera.translation.y = camera_center.y;
    for (label, mut visibility) in &mut object_labels {
        *visibility =
            if label_focus_is_visible(label.focus, camera_center, window.width() / window.height())
            {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
    }
}

#[allow(clippy::needless_pass_by_value)]
fn sync_showcase_copy(
    runtime: Res<ShowcaseRuntime>,
    mut prompt: Single<&mut Text, (With<ShowcasePromptText>, Without<ShowcaseStateText>)>,
    mut state: Single<&mut Text, (With<ShowcaseStateText>, Without<ShowcasePromptText>)>,
) {
    prompt.0.clone_from(&runtime.prompt);
    state.0.clone_from(&runtime.state);
}

fn clamp_camera_center(desired: Vec2, scene: &WorldSceneDefinition, aspect_ratio: f32) -> Vec2 {
    let half_scene = Vec2::new(
        milli_to_tiles(scene.width_milli_tiles) * 0.5,
        milli_to_tiles(scene.height_milli_tiles) * 0.5,
    );
    let half_view = camera_half_view(aspect_ratio);
    Vec2::new(
        clamp_axis(desired.x, half_scene.x, half_view.x),
        clamp_axis(desired.y, half_scene.y, half_view.y),
    )
}

fn camera_half_view(aspect_ratio: f32) -> Vec2 {
    Vec2::new(
        VIEW_HEIGHT_TILES * aspect_ratio * 0.5,
        VIEW_HEIGHT_TILES * 0.5,
    )
}

fn label_focus_is_visible(focus: Vec2, camera_center: Vec2, aspect_ratio: f32) -> bool {
    const EDGE_INSET_TILES: f32 = 0.25;
    let visible_half_extent = camera_half_view(aspect_ratio) - Vec2::splat(EDGE_INSET_TILES);
    let delta = (focus - camera_center).abs();
    delta.x <= visible_half_extent.x && delta.y <= visible_half_extent.y
}

fn clamp_axis(desired: f32, half_scene: f32, half_view: f32) -> f32 {
    if half_view >= half_scene {
        0.0
    } else {
        desired.clamp(-half_scene + half_view, half_scene - half_view)
    }
}

#[allow(clippy::needless_pass_by_value)]
fn animate_safe_markers(
    time: Res<Time>,
    scene: Res<ShowcaseScene>,
    mut markers: Query<&mut Transform, With<SafeRingMarker>>,
) {
    if scene.reduced_motion {
        return;
    }
    let scale = 1.0 + (time.elapsed_secs() * 1.8).sin() * 0.08;
    for mut transform in &mut markers {
        transform.scale = Vec3::splat(scale);
    }
}

#[allow(clippy::needless_pass_by_value)]
fn capture_world_showcase_evidence(
    mut commands: Commands,
    request: Res<ShowcaseScreenshotRequest>,
    mut progress: Local<CaptureProgress>,
) {
    if progress.queued {
        return;
    }
    progress.settled_frames = progress.settled_frames.saturating_add(1);
    if progress.settled_frames >= EVIDENCE_SETTLE_FRAMES {
        progress.queued = true;
        commands
            .spawn(Screenshot::primary_window())
            .observe(crate::save_screenshot_atomically(request.0.clone()));
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn compiled() -> CoreDevelopmentWorldFlow {
        load_core_development_world_flow(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content"),
        )
        .expect("world flow")
    }

    #[test]
    fn hall_plan_is_derived_from_exact_compiled_geometry() {
        let scene = compiled().compile_hall_scene().expect("Hall");
        let plan = build_render_plan(&scene);
        assert_eq!(plan.floor.size, Vec2::new(64.0, 48.0));
        assert_eq!(plan.shell.len(), 4);
        assert_eq!(plan.solids.len(), 5);
        assert!(plan.roads.is_empty());
        assert_eq!(plan.objects.len(), 6);
        assert_eq!(plan.player_spawn, Vec2::new(0.0, -18.0));
    }

    #[test]
    fn microrealm_plan_preserves_road_and_semantic_objects() {
        let scene = compiled().compile_microrealm_scene().expect("microrealm");
        let plan = build_render_plan(&scene);
        assert_eq!(plan.floor.size, Vec2::new(48.0, 48.0));
        assert_eq!(plan.roads.len(), 4);
        assert_eq!(plan.objects.len(), 4);
        assert!(plan.objects.iter().any(|object| {
            object.id == "portal.dungeon.bell_sepulcher"
                && object.condition == SceneObjectCondition::RequiresMicrorealmCleared
                && object.integration_gated
        }));

        let realm_gate = plan
            .objects
            .iter()
            .find(|object| object.id == "landmark.realm_gate")
            .expect("Realm Gate landmark");
        let hall_return = plan
            .objects
            .iter()
            .find(|object| object.id == "portal.return.lantern_halls")
            .expect("Hall return portal");
        assert_eq!((realm_gate.label_lane, hall_return.label_lane), (0, 1));
        let realm_gate_label = object_label_layout(realm_gate);
        let hall_return_label = object_label_layout(hall_return);
        assert!(
            (hall_return_label.position.y - realm_gate_label.position.y - 0.52).abs() < 0.000_1
        );
    }

    #[test]
    fn every_rendered_object_has_exact_localized_copy() {
        let compiled = compiled();
        for scene in [
            compiled.compile_hall_scene().expect("Hall"),
            compiled.compile_microrealm_scene().expect("microrealm"),
        ] {
            let labels = scene_labels(&compiled, &scene).expect("labels");
            assert_eq!(labels.len(), scene.objects.len() + 1);
            assert!(labels.values().all(|label| {
                !label.name.trim().is_empty() && !label.description.trim().is_empty()
            }));
        }
    }

    #[test]
    fn camera_clamps_to_scene_bounds_at_edge_arrivals() {
        let compiled = compiled();
        let hall = compiled.compile_hall_scene().expect("Hall");
        let hall_spawn = authored_point_to_render(hall.player_spawn, &hall);
        assert_eq!(
            clamp_camera_center(hall_spawn, &hall, 16.0 / 9.0),
            Vec2::new(0.0, -14.0)
        );

        let microrealm = compiled.compile_microrealm_scene().expect("microrealm");
        let microrealm_spawn = authored_point_to_render(microrealm.player_spawn, &microrealm);
        let clamped = clamp_camera_center(microrealm_spawn, &microrealm, 16.0 / 9.0);
        assert!((clamped.x - (-6.222_222)).abs() < 0.000_1);
        assert!((clamped.y - (-14.0)).abs() < 0.000_1);
    }

    #[test]
    fn semantic_labels_are_culled_by_object_focus_at_view_edges() {
        let camera = Vec2::new(0.0, -14.0);
        let aspect = 16.0 / 9.0;
        assert!(label_focus_is_visible(
            Vec2::new(-16.0, -12.0),
            camera,
            aspect
        ));
        assert!(!label_focus_is_visible(
            Vec2::new(-28.0, -12.0),
            camera,
            aspect
        ));
        assert!(!label_focus_is_visible(Vec2::new(0.0, 0.0), camera, aspect));
    }
}
