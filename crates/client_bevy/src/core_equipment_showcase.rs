//! Native `GB-M03-04E` inventory, comparison, confirmation, and icon-review surface.

use std::{collections::BTreeMap, env, fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use bevy::{
    asset::io::{AssetSourceBuilder, AssetSourceId, file::FileAssetReader},
    prelude::*,
    render::view::screenshot::Screenshot,
    window::{PresentMode, PrimaryWindow, WindowResolution},
};
use content_schema::CoreWorldFlowCopyFile;
use sim_content::{
    CoreEquipmentAxis, CoreEquipmentComparison, compare_core_equipment,
    load_core_development_items, resolve_core_equipment_presentation,
};
use sim_core::EquipmentRarity;

const ICON_RUNTIME_PATH: &str = "core/items/core_item_icons.runtime.png";
const ICON_SOURCE_BLAKE3: &str = "19d49b684fd2b78c84b7aee67b0f94dcc9f8f061acff0ec9c81882bddd2cf9f5";
const ICON_RUNTIME_BLAKE3: &str =
    "c48daa7c1e7d7e054dd94480031e636a7a892af19d25c5b5091e0b03c55b8da7";
const EVIDENCE_SETTLE_FRAMES: u8 = 90;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreEquipmentShowcaseState {
    Comparison,
    IconMatrix,
}

#[derive(Debug, Clone)]
pub struct CoreEquipmentShowcaseConfig {
    pub content_root: PathBuf,
    pub reduced_effects: bool,
    pub state: CoreEquipmentShowcaseState,
}

#[derive(Debug, Resource)]
struct EquipmentShowcaseModel {
    state: CoreEquipmentShowcaseState,
    reduced_effects: bool,
    revision: String,
    names: BTreeMap<String, String>,
    item_ids: Vec<String>,
    comparison: CoreEquipmentComparison,
}

#[derive(Debug, Resource)]
struct ScreenshotRequest(PathBuf);

#[derive(Debug, Default)]
struct CaptureProgress {
    settled_frames: u8,
    queued: bool,
}

pub fn run_core_equipment_showcase(config: &CoreEquipmentShowcaseConfig) -> Result<()> {
    let content_root = fs::canonicalize(&config.content_root).with_context(|| {
        format!(
            "could not resolve content root {}",
            config.content_root.display()
        )
    })?;
    let catalog = load_core_development_items(&content_root)
        .context("unpromoted Core item content failed validation")?;
    let repository_root = content_root
        .parent()
        .context("content root has no repository parent")?;
    let source = repository_root.join("assets/core/items/core_item_icons.svg");
    let asset_root = repository_root.join("assets");
    let runtime = asset_root.join(ICON_RUNTIME_PATH);
    validate_icon_artifacts(&source, &runtime)?;
    let names = load_names(&content_root)?;
    let item_ids = catalog.items().keys().cloned().collect::<Vec<_>>();
    let current = resolve_core_equipment_presentation(
        &catalog,
        "item.weapon.crossbow.pine_crossbow",
        1,
        EquipmentRarity::Worn,
    )?;
    let incoming = resolve_core_equipment_presentation(
        &catalog,
        "item.weapon.crossbow.grave_repeater",
        4,
        EquipmentRarity::Forged,
    )?;
    let comparison = compare_core_equipment(Some(&current), &incoming)?;
    let model = EquipmentShowcaseModel {
        state: config.state,
        reduced_effects: config.reduced_effects,
        revision: catalog.revision_label().to_owned(),
        names,
        item_ids,
        comparison,
    };
    let (width, height) = crate::configured_window_size()?;
    let screenshot = env::var_os("GRAVEBOUND_SCREENSHOT_PATH").map(PathBuf::from);
    let mut app = App::new();
    let asset_reader_root = asset_root.clone();
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::new(move || Box::new(FileAssetReader::new(asset_reader_root.clone()))),
    )
    .insert_resource(ClearColor(Color::srgb_u8(7, 9, 12)))
    .insert_resource(model)
    .add_plugins(
        crate::gravebound_default_plugins()
            .set(ImagePlugin::default_nearest())
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Gravebound - GB-M03-04E Field Equipment".to_owned(),
                    resolution: WindowResolution::new(width, height),
                    present_mode: PresentMode::AutoVsync,
                    ..default()
                }),
                ..default()
            }),
    )
    .add_systems(Startup, spawn_surface);
    if let Some(path) = screenshot {
        app.insert_resource(ScreenshotRequest(path))
            .add_systems(Update, capture_evidence);
    }
    app.run();
    Ok(())
}

fn validate_icon_artifacts(source: &std::path::Path, runtime: &std::path::Path) -> Result<()> {
    let source_bytes = fs::read(source).with_context(|| format!("missing {}", source.display()))?;
    let runtime_bytes =
        fs::read(runtime).with_context(|| format!("missing {}", runtime.display()))?;
    let source_hash = blake3::hash(&source_bytes).to_hex().to_string();
    let runtime_hash = blake3::hash(&runtime_bytes).to_hex().to_string();
    if source_hash != ICON_SOURCE_BLAKE3 || runtime_hash != ICON_RUNTIME_BLAKE3 {
        bail!(
            "Core icon source/runtime artifact hash mismatch: source={source_hash}, runtime={runtime_hash}"
        );
    }
    Ok(())
}

fn load_names(content_root: &std::path::Path) -> Result<BTreeMap<String, String>> {
    let bytes = fs::read(content_root.join("core_dev/items.en-US.json"))?;
    let copy: CoreWorldFlowCopyFile = serde_json::from_slice(&bytes)?;
    Ok(copy
        .entries
        .into_iter()
        .filter_map(|entry| {
            entry
                .key
                .as_str()
                .strip_suffix(".name")
                .map(|id| (id.to_owned(), entry.value))
        })
        .collect())
}

#[allow(clippy::needless_pass_by_value)]
fn spawn_surface(
    mut commands: Commands,
    model: Res<EquipmentShowcaseModel>,
    assets: Res<AssetServer>,
    mut atlases: ResMut<Assets<TextureAtlasLayout>>,
) {
    commands.spawn(Camera2d);
    let texture = assets.load(ICON_RUNTIME_PATH);
    let atlas = atlases.add(TextureAtlasLayout::from_grid(
        UVec2::splat(64),
        6,
        3,
        None,
        None,
    ));
    spawn_ambient_world(&mut commands, model.reduced_effects);
    match model.state {
        CoreEquipmentShowcaseState::Comparison => {
            spawn_comparison_surface(&mut commands, &model, &texture, &atlas);
        }
        CoreEquipmentShowcaseState::IconMatrix => {
            spawn_icon_matrix(&mut commands, &model, &texture, &atlas);
        }
    }
}

fn spawn_ambient_world(commands: &mut Commands, reduced_effects: bool) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(8, 11, 14)),
            GlobalZIndex(-10),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: percent(7),
                    top: percent(13),
                    width: percent(46),
                    height: percent(68),
                    border: UiRect::all(px(1)),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(13, 19, 20)),
                BorderColor::all(Color::srgba_u8(105, 92, 63, 130)),
            ));
            root.spawn((
                Text::new("LANTERN HALLS  /  FIELD LOADOUT REVIEW"),
                TextFont::from_font_size(16.0),
                TextColor(Color::srgb_u8(166, 154, 126)),
                Node {
                    position_type: PositionType::Absolute,
                    left: percent(9),
                    top: percent(16),
                    ..default()
                },
            ));
            root.spawn((
                Text::new(if reduced_effects {
                    "REDUCED EFFECTS  /  WORLD CONTINUES"
                } else {
                    "STANDARD EFFECTS  /  WORLD CONTINUES"
                }),
                TextFont::from_font_size(14.0),
                TextColor(Color::srgb_u8(118, 145, 137)),
                Node {
                    position_type: PositionType::Absolute,
                    left: percent(9),
                    bottom: percent(16),
                    ..default()
                },
            ));
        });
}

#[allow(clippy::too_many_lines)] // Linear hierarchy keeps the review surface's reading order explicit.
fn spawn_comparison_surface(
    commands: &mut Commands,
    model: &EquipmentShowcaseModel,
    texture: &Handle<Image>,
    atlas: &Handle<TextureAtlasLayout>,
) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(24),
                top: px(24),
                bottom: px(24),
                width: percent(43),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(18)),
                row_gap: px(10),
                border: UiRect::all(px(2)),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            BackgroundColor(Color::srgba_u8(12, 15, 18, 248)),
            BorderColor::all(Color::srgb_u8(171, 137, 72)),
            GlobalZIndex(10),
        ))
        .with_children(|panel| {
            label(
                panel,
                "FIELD INVENTORY",
                24.0,
                Color::srgb_u8(235, 218, 171),
            );
            label(
                panel,
                "I / TAB  /  ONLINE WORLD DOES NOT PAUSE",
                14.0,
                Color::srgb_u8(133, 158, 151),
            );
            label(panel, "EQUIPPED", 16.0, Color::srgb_u8(188, 178, 151));
            spawn_icon_row(
                panel,
                texture,
                atlas,
                &[17, 11, usize::MAX, usize::MAX],
                58.0,
            );
            label(
                panel,
                "PENDING BACKPACK  1 / 8",
                16.0,
                Color::srgb_u8(207, 185, 145),
            );
            spawn_icon_row(
                panel,
                texture,
                atlas,
                &[
                    14,
                    usize::MAX,
                    usize::MAX,
                    usize::MAX,
                    usize::MAX,
                    usize::MAX,
                    usize::MAX,
                    usize::MAX,
                ],
                52.0,
            );
            label(
                panel,
                "LOST ON DEATH OR EMERGENCY RECALL",
                14.0,
                Color::srgb_u8(224, 166, 112),
            );
            label(
                panel,
                "GRAVE REPEATER  ->  WEAPON",
                19.0,
                Color::srgb_u8(238, 224, 188),
            );
            label(panel, "BEHAVIOR", 14.0, Color::srgb_u8(140, 177, 161));
            label(
                panel,
                "Single-bolt crossbow / faster cadence / shorter reach",
                15.0,
                Color::srgb_u8(224, 218, 199),
            );
            for line in comparison_lines(&model.comparison) {
                label(panel, &line, 14.0, Color::srgb_u8(203, 202, 188));
            }
            label(panel, "SWAP PREVIEW", 14.0, Color::srgb_u8(140, 177, 161));
            label(
                panel,
                "Pine Crossbow -> Backpack 1 (vacated source)",
                15.0,
                Color::srgb_u8(238, 198, 122),
            );
            panel.spawn((
                Text::new("CONFIRM EQUIP   [ENTER / A]"),
                TextFont::from_font_size(16.0),
                TextColor(Color::srgb_u8(244, 235, 205)),
                Node {
                    padding: UiRect::axes(px(18), px(12)),
                    border: UiRect::all(px(2)),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(32, 46, 43)),
                BorderColor::all(Color::srgb_u8(190, 158, 87)),
            ));
            label(
                panel,
                "Cancel [ESC / B]  /  Confirmation locks while in flight",
                14.0,
                Color::srgb_u8(145, 151, 147),
            );
            label(
                panel,
                &format!("AUTHORITY  {}...", &model.revision[..32]),
                14.0,
                Color::srgb_u8(108, 125, 122),
            );
        });
}

fn spawn_icon_matrix(
    commands: &mut Commands,
    model: &EquipmentShowcaseModel,
    texture: &Handle<Image>,
    atlas: &Handle<TextureAtlasLayout>,
) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(14),
                right: percent(14),
                top: percent(9),
                bottom: percent(9),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(20)),
                row_gap: px(10),
                border: UiRect::all(px(2)),
                ..default()
            },
            BackgroundColor(Color::srgba_u8(11, 14, 17, 249)),
            BorderColor::all(Color::srgb_u8(171, 137, 72)),
            GlobalZIndex(10),
        ))
        .with_children(|panel| {
            label(
                panel,
                "CORE ITEM ICON REVIEW  /  18 / 18",
                24.0,
                Color::srgb_u8(235, 218, 171),
            );
            label(
                panel,
                "64x64 SOURCE CELLS  /  MINIMUM-SCALE INVENTORY CONTEXT",
                14.0,
                Color::srgb_u8(137, 162, 155),
            );
            panel
                .spawn(Node {
                    width: percent(100),
                    flex_grow: 1.0,
                    flex_wrap: FlexWrap::Wrap,
                    align_content: AlignContent::Center,
                    justify_content: JustifyContent::Center,
                    column_gap: px(16),
                    row_gap: px(12),
                    ..default()
                })
                .with_children(|grid| {
                    for (index, item_id) in model.item_ids.iter().enumerate() {
                        grid.spawn(Node {
                            width: percent(14),
                            min_width: px(108),
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            row_gap: px(4),
                            ..default()
                        })
                        .with_children(|cell| {
                            spawn_icon(cell, texture.clone(), atlas.clone(), index, 64.0);
                            label(
                                cell,
                                model.names.get(item_id).map_or(item_id, String::as_str),
                                14.0,
                                Color::srgb_u8(218, 212, 194),
                            );
                        });
                    }
                });
            label(
                panel,
                "NON-COLOR SILHOUETTE CHECK  /  SOURCE HASH VERIFIED  /  STATIC / NO MOTION",
                14.0,
                Color::srgb_u8(139, 154, 148),
            );
        });
}

fn spawn_icon_row(
    parent: &mut ChildSpawnerCommands,
    texture: &Handle<Image>,
    atlas: &Handle<TextureAtlasLayout>,
    indices: &[usize],
    size: f32,
) {
    parent
        .spawn(Node {
            width: percent(100),
            column_gap: px(8),
            ..default()
        })
        .with_children(|row| {
            for &index in indices {
                if index == usize::MAX {
                    row.spawn((
                        Node {
                            width: px(size),
                            height: px(size),
                            border: UiRect::all(px(1)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb_u8(18, 21, 24)),
                        BorderColor::all(Color::srgb_u8(65, 65, 61)),
                    ));
                } else {
                    spawn_icon(row, texture.clone(), atlas.clone(), index, size);
                }
            }
        });
}

fn spawn_icon(
    parent: &mut ChildSpawnerCommands,
    texture: Handle<Image>,
    atlas: Handle<TextureAtlasLayout>,
    index: usize,
    size: f32,
) {
    parent.spawn((
        ImageNode::from_atlas_image(
            texture,
            TextureAtlas {
                layout: atlas,
                index,
            },
        ),
        Node {
            width: px(size),
            height: px(size),
            border: UiRect::all(px(1)),
            ..default()
        },
        BorderColor::all(Color::srgb_u8(173, 142, 77)),
    ));
}

fn label(parent: &mut ChildSpawnerCommands, value: &str, size: f32, color: Color) {
    parent.spawn((
        Text::new(value),
        TextFont::from_font_size(size),
        TextColor(color),
    ));
}

fn comparison_lines(comparison: &CoreEquipmentComparison) -> Vec<String> {
    comparison
        .changes
        .iter()
        .filter(|change| !change.advanced)
        .take(4)
        .map(|change| {
            let axis = match change.axis {
                CoreEquipmentAxis::WeaponDamage => "Displayed hit damage",
                CoreEquipmentAxis::AttackIntervalMicros => "Attack interval",
                CoreEquipmentAxis::RangeMilliTiles => "Range",
                CoreEquipmentAxis::BoltCount => "Bolts per release",
                _ => "Resolved behavior axis",
            };
            format!(
                "{axis}: {}  ->  {}",
                change.before.unwrap_or_default(),
                change.after.unwrap_or_default()
            )
        })
        .collect()
}

#[allow(clippy::needless_pass_by_value)]
fn capture_evidence(
    mut commands: Commands,
    request: Res<ScreenshotRequest>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut progress: Local<CaptureProgress>,
) {
    if progress.queued || windows.single().is_err() {
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
    use super::*;

    #[test]
    fn checked_in_runtime_sheet_matches_the_approved_source_and_all_items() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        validate_icon_artifacts(
            &root.join("assets/core/items/core_item_icons.svg"),
            &root.join("assets/core/items/core_item_icons.runtime.png"),
        )
        .unwrap();
        let catalog = load_core_development_items(&root.join("content")).unwrap();
        let names = load_names(&root.join("content")).unwrap();
        assert_eq!(catalog.items().len(), 18);
        assert_eq!(names.len(), 18);
    }
}
