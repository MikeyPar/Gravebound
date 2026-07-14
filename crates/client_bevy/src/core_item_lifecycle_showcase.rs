//! Disposable native inspection surface for the unpromoted `GB-M03-04G` item lifecycle.
//!
//! This module is deliberately read-only. It presents one deterministic `04A`-`04F`
//! lifecycle signature using validated Core item content, but exposes no station input,
//! world admission, or gameplay mutation path.

use std::{
    collections::BTreeMap,
    env,
    fmt::Write as _,
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use bevy::{
    app::AppExit,
    log::{error, warn},
    prelude::*,
    render::{
        render_resource::TextureFormat,
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::{PresentMode, PrimaryWindow, WindowResolution},
};
use content_schema::CoreWorldFlowCopyFile;
use sim_content::load_core_development_items;

const EVIDENCE_SETTLE_FRAMES: u8 = 90;
const EVIDENCE_SETTLE_TIME: Duration = Duration::from_secs(8);
const MAX_CAPTURE_ATTEMPTS: u8 = 4;
const EQUIPMENT_CAPACITY: u16 = 4;
const BELT_CAPACITY: u16 = 2;
const RUN_BACKPACK_CAPACITY: u16 = 8;
const CHARACTER_SAFE_CAPACITY: u16 = 8;
const VAULT_CAPACITY: u16 = 160;
const PLAYFIELD_WIDTH_PERCENT: f32 = 49.0;
const INSPECTION_WIDTH_PERCENT: f32 = 49.0;
const STATIC_EFFECT_PROFILE: &str = "STATIC  /  REDUCED-SAFE";

#[derive(Debug, Clone)]
pub struct CoreItemLifecycleShowcaseConfig {
    pub content_root: PathBuf,
    pub reduced_effects: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LifecycleCapacities {
    equipment: u16,
    belt: u16,
    run_backpack: u16,
    character_safe: u16,
    vault: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AggregateVersions {
    account: u64,
    character: u64,
    world: u64,
    progression: u64,
    inventory: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReceiptCounts {
    starter: u16,
    xp: u16,
    first_clear: u16,
    reward: u16,
    equipment: u16,
    safe_transfer: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LifecycleItem {
    uid: [u8; 16],
    template_id: &'static str,
    localized_name: String,
    provenance: &'static str,
    security: &'static str,
    location: &'static str,
    item_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LifecycleInformation {
    selected_character: &'static str,
    character_id: [u8; 16],
    class_name: &'static str,
    content_revision: String,
    level: u16,
    total_xp: u32,
    capacities: LifecycleCapacities,
    versions: AggregateVersions,
    receipts: ReceiptCounts,
    items: Vec<LifecycleItem>,
    ledger_transitions: Vec<&'static str>,
    occupied_equipment: Vec<&'static str>,
    occupied_belt: Vec<u16>,
    occupied_backpack: Vec<u16>,
    occupied_character_safe: Vec<u16>,
    occupied_vault: Vec<u16>,
}

#[derive(Debug, Resource)]
struct LifecycleShowcaseModel {
    information: LifecycleInformation,
}

#[derive(Debug, Resource)]
struct ScreenshotRequest(PathBuf);

#[derive(Debug, Default, Resource)]
struct CaptureProgress {
    started_at: Option<Instant>,
    settled_frames: u8,
    attempts: u8,
    queued: bool,
}

/// Opens the deterministic, noninteractive item-lifecycle inspection surface.
pub fn run_core_item_lifecycle_showcase(config: &CoreItemLifecycleShowcaseConfig) -> Result<()> {
    let content_root = fs::canonicalize(&config.content_root).with_context(|| {
        format!(
            "could not resolve content root {}",
            config.content_root.display()
        )
    })?;
    let model = build_model(&content_root)?;
    let (width, height) = crate::configured_window_size()?;
    let screenshot = env::var_os("GRAVEBOUND_SCREENSHOT_PATH").map(PathBuf::from);

    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb_u8(6, 8, 10)))
        .insert_resource(model)
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: if config.reduced_effects {
                    "Gravebound - GB-M03-04G Item Lifecycle - Reduced Effects".to_owned()
                } else {
                    "Gravebound - GB-M03-04G Item Lifecycle - Standard Effects".to_owned()
                },
                resolution: WindowResolution::new(width, height),
                present_mode: PresentMode::AutoVsync,
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, spawn_surface);
    if let Some(path) = screenshot {
        app.insert_resource(ScreenshotRequest(path))
            .insert_resource(CaptureProgress::default())
            .add_systems(Update, capture_evidence);
    }
    app.run();
    Ok(())
}

fn build_model(content_root: &std::path::Path) -> Result<LifecycleShowcaseModel> {
    let catalog = load_core_development_items(content_root)
        .context("unpromoted Core item content failed validation")?;
    let names = load_names(content_root)?;
    let items = build_items(catalog.items(), &names)?;
    Ok(LifecycleShowcaseModel {
        information: build_information(catalog.revision_label(), items),
    })
}

fn build_items(
    catalog_items: &BTreeMap<String, content_schema::ProductionItemTemplateRecord>,
    names: &BTreeMap<String, String>,
) -> Result<Vec<LifecycleItem>> {
    let specs = [
        (
            [0xe2; 16],
            "item.weapon.crossbow.grave_repeater",
            "Drop",
            "AtRiskEquipped",
            "Equipped.Weapon",
            2,
        ),
        (
            [0x21; 16],
            "item.relic.arbalist.cracked_mark_lens",
            "Starter",
            "AtRiskEquipped",
            "Equipped.Relic",
            1,
        ),
        (
            [0x31; 16],
            "consumable.red_tonic",
            "Starter",
            "Safe",
            "Belt.0",
            1,
        ),
        (
            [0x32; 16],
            "consumable.red_tonic",
            "Starter",
            "Safe",
            "Belt.0",
            1,
        ),
        (
            [0x41; 16],
            "item.weapon.crossbow.pine_crossbow",
            "Starter",
            "AtRiskPending",
            "RunBackpack.0",
            2,
        ),
        (
            [0xe3; 16],
            "item.armor.ashplate.t1",
            "Drop",
            "AtRiskPending",
            "RunBackpack.1",
            1,
        ),
        (
            [0xe4; 16],
            "consumable.red_tonic",
            "Drop",
            "AtRiskPending",
            "RunBackpack.2",
            1,
        ),
        (
            [0xe5; 16],
            "consumable.red_tonic",
            "Drop",
            "AtRiskPending",
            "RunBackpack.2",
            1,
        ),
        (
            [0xd4; 16],
            "item.weapon.crossbow.pine_crossbow",
            "Starter",
            "Safe",
            "Vault.0",
            2,
        ),
    ];
    specs
        .into_iter()
        .map(
            |(uid, template_id, provenance, security, location, item_version)| {
                if !catalog_items.contains_key(template_id) {
                    bail!("lifecycle evidence references disabled Core item {template_id}");
                }
                let localized_name = names
                    .get(template_id)
                    .with_context(|| format!("missing en-US item name for {template_id}"))?
                    .clone();
                Ok(LifecycleItem {
                    uid,
                    template_id,
                    localized_name,
                    provenance,
                    security,
                    location,
                    item_version,
                })
            },
        )
        .collect()
}

fn build_information(content_revision: &str, items: Vec<LifecycleItem>) -> LifecycleInformation {
    LifecycleInformation {
        selected_character: "Morrow Vale",
        character_id: [0xd3; 16],
        class_name: "Grave Arbalist",
        content_revision: content_revision.to_owned(),
        level: 4,
        total_xp: 675,
        capacities: LifecycleCapacities {
            equipment: EQUIPMENT_CAPACITY,
            belt: BELT_CAPACITY,
            run_backpack: RUN_BACKPACK_CAPACITY,
            character_safe: CHARACTER_SAFE_CAPACITY,
            vault: VAULT_CAPACITY,
        },
        versions: AggregateVersions {
            account: 2,
            character: 1,
            world: 1,
            progression: 2,
            inventory: 5,
        },
        receipts: ReceiptCounts {
            starter: 1,
            xp: 1,
            first_clear: 1,
            reward: 1,
            equipment: 1,
            safe_transfer: 1,
        },
        items,
        ledger_transitions: vec![
            "01  STARTER  Created -> Equipped / Belt  x4",
            "02  CALDUS   XP 0 -> 675  /  Level 1 -> 4",
            "03  REWARD   2 equipment + 2 tonic units -> Backpack",
            "04  EQUIP    Backpack.0 <-> Equipped.Weapon",
            "05  SAFE     CharacterSafe.0 -> Vault.0",
        ],
        occupied_equipment: vec!["Weapon", "Relic"],
        occupied_belt: vec![0],
        occupied_backpack: vec![0, 1, 2],
        occupied_character_safe: vec![],
        occupied_vault: vec![0],
    }
}

fn load_names(content_root: &std::path::Path) -> Result<BTreeMap<String, String>> {
    let path = content_root.join("core_dev/items.en-US.json");
    let bytes = fs::read(&path).with_context(|| format!("missing {}", path.display()))?;
    let copy: CoreWorldFlowCopyFile =
        serde_json::from_slice(&bytes).with_context(|| format!("invalid {}", path.display()))?;
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

#[allow(clippy::needless_pass_by_value)] // Bevy systems require owned system parameters.
fn spawn_surface(mut commands: Commands, model: Res<LifecycleShowcaseModel>) {
    commands.spawn((Name::new("Item lifecycle evidence camera"), Camera2d));
    spawn_hall_corridor(&mut commands);
    spawn_inspection_panel(&mut commands, &model);
}

fn spawn_hall_corridor(commands: &mut Commands) {
    commands
        .spawn((
            Name::new("Protected Hall playfield corridor"),
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(PLAYFIELD_WIDTH_PERCENT),
                height: percent(100),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(8, 11, 13)),
        ))
        .with_children(|world| {
            spawn_corridor_geometry(world);
            spawn_corridor_lights(world);
            spawn_corridor_copy(world);
        });
}

fn spawn_corridor_geometry(world: &mut ChildSpawnerCommands) {
    world.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: percent(8),
            right: percent(8),
            top: percent(11),
            bottom: percent(11),
            border: UiRect::all(px(1)),
            ..default()
        },
        BackgroundColor(Color::srgb_u8(13, 18, 19)),
        BorderColor::all(Color::srgb_u8(79, 69, 49)),
    ));
    world.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: percent(36),
            top: percent(11),
            width: percent(28),
            height: percent(78),
            border: UiRect::axes(px(1), px(0)),
            ..default()
        },
        BackgroundColor(Color::srgb_u8(22, 27, 25)),
        BorderColor::all(Color::srgb_u8(111, 91, 54)),
    ));
    for top in [24.0, 43.0, 62.0, 81.0] {
        world.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(18),
                top: percent(top),
                width: percent(64),
                height: px(1),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(67, 61, 46)),
        ));
    }
}

fn spawn_corridor_lights(world: &mut ChildSpawnerCommands) {
    let light = Color::srgb_u8(174, 124, 55);
    for (left, top) in [(25.0, 26.0), (72.0, 45.0), (25.0, 64.0)] {
        world.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(left),
                top: percent(top),
                width: px(7),
                height: px(18),
                ..default()
            },
            BackgroundColor(light),
        ));
    }
}

fn spawn_corridor_copy(world: &mut ChildSpawnerCommands) {
    world.spawn((
        Text::new("LANTERN HALLS  /  SAFE CORRIDOR"),
        TextFont::from_font_size(15.0),
        TextColor(Color::srgb_u8(187, 169, 128)),
        Node {
            position_type: PositionType::Absolute,
            left: percent(10),
            top: percent(5),
            ..default()
        },
    ));
    world.spawn((
        Text::new(STATIC_EFFECT_PROFILE),
        TextFont::from_font_size(12.0),
        TextColor(Color::srgb_u8(123, 153, 143)),
        Node {
            position_type: PositionType::Absolute,
            right: percent(10),
            top: percent(5),
            ..default()
        },
    ));
    world
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(10),
                right: percent(10),
                bottom: percent(4),
                padding: UiRect::axes(px(12), px(8)),
                column_gap: px(16),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgba_u8(8, 11, 12, 235)),
            BorderColor::all(Color::srgb_u8(87, 76, 55)),
        ))
        .with_children(|notice| {
            compact_label(
                notice,
                "DISPOSABLE INSPECTION  /  NO INTERACTION",
                13.0,
                Color::srgb_u8(221, 205, 165),
            );
            compact_label(
                notice,
                "REALM GATE + VAULT STATION REMAIN LOCKED",
                11.0,
                Color::srgb_u8(191, 129, 88),
            );
        });
}

#[allow(clippy::too_many_lines)] // Linear construction preserves the evidence reading order.
fn spawn_inspection_panel(commands: &mut Commands, model: &LifecycleShowcaseModel) {
    let info = &model.information;
    commands
        .spawn((
            Name::new("Read-only lifecycle signature panel"),
            Node {
                position_type: PositionType::Absolute,
                right: px(12),
                top: px(12),
                bottom: px(12),
                width: percent(INSPECTION_WIDTH_PERCENT),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(12)),
                row_gap: px(4),
                border: UiRect::all(px(2)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgba_u8(10, 13, 16, 250)),
            BorderColor::all(Color::srgb_u8(153, 122, 67)),
            GlobalZIndex(10),
        ))
        .with_children(|panel| {
            compact_label(
                panel,
                "ITEM + VAULT  /  DURABLE LIFECYCLE",
                20.0,
                Color::srgb_u8(238, 221, 179),
            );
            compact_label(
                panel,
                "GB-M03-04G  ·  READ ONLY  ·  04A—04F COMPOSED",
                11.0,
                Color::srgb_u8(127, 164, 151),
            );
            compact_label(
                panel,
                &format!("CONTENT  {}", info.content_revision),
                9.0,
                Color::srgb_u8(103, 120, 117),
            );
            divider(panel);
            spawn_identity_summary(panel, info);
            divider(panel);
            spawn_capacity_summary(panel, info);
            divider(panel);
            compact_label(
                panel,
                "ITEM INSTANCES  /  UID · PROVENANCE · SECURITY · LOCATION · VERSION",
                11.0,
                Color::srgb_u8(194, 181, 149),
            );
            for item in &info.items {
                spawn_item_row(panel, item);
            }
            divider(panel);
            spawn_receipt_summary(panel, info);
            divider(panel);
            compact_label(
                panel,
                "MUTATION LEDGER  /  CANONICAL ORDER",
                11.0,
                Color::srgb_u8(194, 181, 149),
            );
            for transition in &info.ledger_transitions {
                compact_label(panel, transition, 10.0, Color::srgb_u8(192, 198, 188));
            }
            compact_label(
                panel,
                "RECONNECT = MATCH  /  RESTART = MATCH  /  NORMAL ROUTE DISABLED",
                10.0,
                Color::srgb_u8(195, 151, 91),
            );
        });
}

fn spawn_identity_summary(parent: &mut ChildSpawnerCommands, info: &LifecycleInformation) {
    parent
        .spawn(Node {
            width: percent(100),
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        })
        .with_children(|row| {
            row.spawn(Node {
                width: percent(49),
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(|left| {
                compact_label(
                    left,
                    "SELECTED CHARACTER",
                    10.0,
                    Color::srgb_u8(121, 149, 141),
                );
                compact_label(
                    left,
                    &format!("{}  /  {}", info.selected_character, info.class_name),
                    14.0,
                    Color::srgb_u8(231, 219, 190),
                );
                compact_label(
                    left,
                    &format!("CHAR UID  {}", uid_hex(info.character_id)),
                    9.0,
                    Color::srgb_u8(111, 126, 123),
                );
            });
            row.spawn(Node {
                width: percent(49),
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(|right| {
                compact_label(
                    right,
                    "PROGRESSION / AGGREGATE VERSIONS",
                    10.0,
                    Color::srgb_u8(121, 149, 141),
                );
                compact_label(
                    right,
                    &format!("LEVEL {}  /  {:04} XP", info.level, info.total_xp),
                    14.0,
                    Color::srgb_u8(231, 219, 190),
                );
                compact_label(
                    right,
                    &format!(
                        "A{} C{} W{} P{} I{}",
                        info.versions.account,
                        info.versions.character,
                        info.versions.world,
                        info.versions.progression,
                        info.versions.inventory
                    ),
                    10.0,
                    Color::srgb_u8(180, 157, 105),
                );
            });
        });
}

fn spawn_capacity_summary(parent: &mut ChildSpawnerCommands, info: &LifecycleInformation) {
    let occupied_equipment = info.occupied_equipment.join(", ");
    compact_label(
        parent,
        &format!(
            "EQUIPMENT  {}/{}  [{}]  /  empty [Armor, Charm]",
            info.occupied_equipment.len(),
            info.capacities.equipment,
            occupied_equipment
        ),
        10.0,
        Color::srgb_u8(211, 205, 185),
    );
    compact_label(
        parent,
        &format!(
            "BELT {}/{}  occupied {:?}  /  BACKPACK {}/{}  occupied {:?}",
            info.occupied_belt.len(),
            info.capacities.belt,
            info.occupied_belt,
            info.occupied_backpack.len(),
            info.capacities.run_backpack,
            info.occupied_backpack
        ),
        10.0,
        Color::srgb_u8(211, 205, 185),
    );
    compact_label(
        parent,
        &format!(
            "CHARACTER SAFE {}/{}  occupied {:?}  /  VAULT {}/{}  occupied {:?}",
            info.occupied_character_safe.len(),
            info.capacities.character_safe,
            info.occupied_character_safe,
            info.occupied_vault.len(),
            info.capacities.vault,
            info.occupied_vault
        ),
        10.0,
        Color::srgb_u8(211, 205, 185),
    );
}

fn spawn_item_row(parent: &mut ChildSpawnerCommands, item: &LifecycleItem) {
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(5), px(2)),
                ..default()
            },
            BackgroundColor(Color::srgba_u8(20, 24, 26, 190)),
        ))
        .with_children(|row| {
            compact_label(
                row,
                &format!(
                    "{}  /  {}  /  {}  /  {}  /  v{}",
                    item.localized_name,
                    item.provenance,
                    item.security,
                    item.location,
                    item.item_version
                ),
                10.0,
                Color::srgb_u8(221, 215, 196),
            );
            compact_label(
                row,
                &format!("UID {}  ·  {}", uid_hex(item.uid), item.template_id),
                8.0,
                Color::srgb_u8(105, 124, 120),
            );
        });
}

fn spawn_receipt_summary(parent: &mut ChildSpawnerCommands, info: &LifecycleInformation) {
    compact_label(
        parent,
        "DURABLE RECEIPTS  /  EXACT REPLAY RETURNS STORED OUTCOME",
        11.0,
        Color::srgb_u8(194, 181, 149),
    );
    compact_label(
        parent,
        &format!(
            "STARTER {}  ·  XP {}  ·  FIRST CLEAR {}  ·  REWARD {}  ·  EQUIP {}  ·  SAFE {}",
            info.receipts.starter,
            info.receipts.xp,
            info.receipts.first_clear,
            info.receipts.reward,
            info.receipts.equipment,
            info.receipts.safe_transfer
        ),
        10.0,
        Color::srgb_u8(212, 199, 166),
    );
}

fn divider(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        Node {
            width: percent(100),
            height: px(1),
            ..default()
        },
        BackgroundColor(Color::srgb_u8(58, 55, 46)),
    ));
}

fn compact_label(parent: &mut ChildSpawnerCommands, value: &str, size: f32, color: Color) {
    parent.spawn((
        Text::new(value),
        TextFont::from_font_size(size),
        TextColor(color),
    ));
}

fn uid_hex(uid: [u8; 16]) -> String {
    let mut encoded = String::with_capacity(32);
    for byte in uid {
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems require owned system parameters.
fn capture_evidence(
    mut commands: Commands,
    request: Res<ScreenshotRequest>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut progress: ResMut<CaptureProgress>,
) {
    if progress.queued || windows.single().is_err() {
        return;
    }
    let elapsed = progress
        .started_at
        .get_or_insert_with(Instant::now)
        .elapsed();
    progress.settled_frames = progress.settled_frames.saturating_add(1);
    if progress.settled_frames >= EVIDENCE_SETTLE_FRAMES && elapsed >= EVIDENCE_SETTLE_TIME {
        progress.attempts = progress.attempts.saturating_add(1);
        progress.queued = true;
        commands
            .spawn(Screenshot::primary_window())
            .observe(validate_and_save_screenshot(request.0.clone()));
    }
}

fn validate_and_save_screenshot(
    path: PathBuf,
) -> impl FnMut(On<ScreenshotCaptured>, ResMut<CaptureProgress>, MessageWriter<AppExit>) {
    let mut save = crate::save_screenshot_atomically(path);
    move |captured, mut progress, mut app_exit| {
        if screenshot_has_complete_surface(&captured.image) {
            save(captured, app_exit);
        } else if progress.attempts >= MAX_CAPTURE_ATTEMPTS {
            error!(
                "item-lifecycle evidence remained incomplete after {} attempts",
                progress.attempts
            );
            app_exit.write(AppExit::from_code(2));
        } else {
            warn!(
                "discarding incomplete item-lifecycle evidence frame {}/{}; retrying",
                progress.attempts, MAX_CAPTURE_ATTEMPTS
            );
            progress.started_at = Some(Instant::now());
            progress.settled_frames = 0;
            progress.queued = false;
        }
    }
}

fn screenshot_has_complete_surface(image: &Image) -> bool {
    if !matches!(
        image.texture_descriptor.format,
        TextureFormat::Rgba8Unorm
            | TextureFormat::Rgba8UnormSrgb
            | TextureFormat::Bgra8Unorm
            | TextureFormat::Bgra8UnormSrgb
    ) {
        return false;
    }
    image.data.as_deref().is_some_and(rgba_surface_is_complete)
}

fn rgba_surface_is_complete(data: &[u8]) -> bool {
    if data.is_empty() || !data.len().is_multiple_of(4) {
        return false;
    }
    let total = data.len() / 4;
    let surfaced = data
        .chunks_exact(4)
        .filter(|pixel| pixel[..3].iter().copied().max().unwrap_or_default() >= 12)
        .count();
    surfaced.saturating_mul(100) >= total.saturating_mul(80)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    #[test]
    fn model_preserves_the_canonical_storage_capacities_and_visible_occupancy() {
        let model = build_model(&content_root()).unwrap();
        let info = &model.information;
        assert_eq!(info.capacities.equipment, 4);
        assert_eq!(info.capacities.belt, 2);
        assert_eq!(info.capacities.run_backpack, 8);
        assert_eq!(info.capacities.character_safe, 8);
        assert_eq!(info.capacities.vault, 160);
        assert_eq!(info.occupied_equipment, ["Weapon", "Relic"]);
        assert_eq!(info.occupied_belt, [0]);
        assert_eq!(info.occupied_backpack, [0, 1, 2]);
        assert!(info.occupied_character_safe.is_empty());
        assert_eq!(info.occupied_vault, [0]);
        assert_eq!(
            info.versions,
            AggregateVersions {
                account: 2,
                character: 1,
                world: 1,
                progression: 2,
                inventory: 5,
            }
        );
    }

    #[test]
    fn static_effect_profile_is_information_invariant_and_reduced_safe() {
        let first = build_model(&content_root()).unwrap();
        let second = build_model(&content_root()).unwrap();
        assert_eq!(first.information, second.information);
        assert!(STATIC_EFFECT_PROFILE.contains("REDUCED-SAFE"));
    }

    #[test]
    fn every_visible_item_uses_validated_core_content_and_a_durable_identity() {
        let model = build_model(&content_root()).unwrap();
        assert_eq!(model.information.items.len(), 9);
        for item in &model.information.items {
            assert_ne!(item.uid, [0; 16]);
            assert!(!item.localized_name.is_empty());
            assert!(!item.template_id.is_empty());
        }
    }

    #[test]
    fn layout_preserves_a_clear_world_corridor_at_supported_evidence_sizes() {
        let playfield_width = PLAYFIELD_WIDTH_PERCENT;
        let inspection_width = INSPECTION_WIDTH_PERCENT;
        assert!(playfield_width >= 45.0);
        assert!(inspection_width <= 50.0);
        assert!(playfield_width + inspection_width <= 100.0);
        for (width, height) in [(1280.0_f32, 720.0_f32), (1920.0, 1080.0)] {
            assert!(width * playfield_width / 100.0 >= 576.0);
            assert!(height >= 720.0);
        }
    }

    #[test]
    fn screenshot_publication_rejects_blank_and_sparse_buffers() {
        assert!(!rgba_surface_is_complete(&[]));
        assert!(!rgba_surface_is_complete(&[0; 16]));

        let mut partial = vec![0_u8; 4_000];
        for pixel in partial.chunks_exact_mut(4).take(799) {
            pixel[0] = 12;
        }
        assert!(!rgba_surface_is_complete(&partial));

        partial[799 * 4] = 12;
        assert!(rgba_surface_is_complete(&partial));
        assert!(!rgba_surface_is_complete(&[6, 8, 10, 255]));
        assert!(rgba_surface_is_complete(&[12, 8, 10, 255]));
    }
}
