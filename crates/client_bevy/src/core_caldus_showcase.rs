//! Disposable native evidence surface for the unpromoted `GB-M03-03E` Caldus package.

use std::{env, path::PathBuf};

use anyhow::{Context, Result, ensure};
use bevy::{
    prelude::*,
    render::view::screenshot::Screenshot,
    window::{PresentMode, PrimaryWindow, WindowResolution},
};
use sim_content::load_core_development_caldus;

const EVIDENCE_SETTLE_FRAMES: u8 = 30;
const HUD_Z: i32 = 100;
const CALDUS_EXIT_ID: &str = "portal.exit.dungeon.bell_sepulcher";
const HALL_ID: &str = "hub.lantern_halls_01";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreCaldusShowcaseState {
    Staging,
    Introduction,
    PhaseOne,
    ChargePressure,
    FinalRings,
    VictoryExit,
    ExtractionCommitted,
    HallArrival,
}

#[derive(Debug, Clone)]
pub struct CoreCaldusShowcaseConfig {
    pub content_root: PathBuf,
    pub reduced_effects: bool,
    pub state: CoreCaldusShowcaseState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TelegraphKind {
    ShieldArc,
    BellRing,
    ChargeLane,
    StopRing,
    Exit,
    Hall,
}

#[derive(Debug, Resource)]
struct ShowcaseModel {
    boss_name: String,
    boss_description: String,
    exit_name: String,
    state: CoreCaldusShowcaseState,
    state_title: &'static str,
    state_detail: &'static str,
    phase_label: &'static str,
    health_percent: u8,
    status_chip: &'static str,
    timeline: &'static str,
    telegraph: TelegraphKind,
    reduced_effects: bool,
    base_health: u32,
    armor: u16,
    pattern_summary: String,
    records_revision: String,
}

#[derive(Debug, Resource)]
struct ScreenshotRequest(PathBuf);

#[derive(Debug, Default)]
struct CaptureProgress {
    settled_frames: u8,
    queued: bool,
}

#[allow(clippy::needless_pass_by_value)]
pub fn run_core_caldus_showcase(config: CoreCaldusShowcaseConfig) -> Result<()> {
    let content = load_core_development_caldus(&config.content_root)
        .context("unpromoted Core Caldus content failed validation")?;
    let model = build_model(&content, config.state, config.reduced_effects)?;
    let (window_width, window_height) = crate::configured_window_size()?;
    let screenshot_request = env::var_os("GRAVEBOUND_SCREENSHOT_PATH").map(PathBuf::from);

    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb_u8(5, 7, 10)))
        .insert_resource(model)
        .add_plugins(
            crate::gravebound_default_plugins()
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Gravebound - GB-M03-03E Sir Caldus Evidence".to_owned(),
                        resolution: WindowResolution::new(window_width, window_height),
                        present_mode: PresentMode::AutoVsync,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_systems(Startup, spawn_showcase);
    if let Some(path) = screenshot_request {
        app.insert_resource(ScreenshotRequest(path))
            .add_systems(Update, capture_evidence);
    }
    app.run();
    Ok(())
}

fn build_model(
    content: &sim_content::CoreDevelopmentCaldus,
    state: CoreCaldusShowcaseState,
    reduced_effects: bool,
) -> Result<ShowcaseModel> {
    let boss = content.boss();
    let exit = content.exit();
    ensure!(boss.header.id.as_str() == "boss.sir_caldus");
    ensure!(boss.pattern_ids.len() == 4);
    ensure!(exit.header.id.as_str() == CALDUS_EXIT_ID);
    ensure!(exit.destination_content_id.as_str() == HALL_ID);
    ensure!(exit.requires_committed_extraction_receipt);
    let (state_title, state_detail, phase_label, health_percent, status_chip, timeline, telegraph) =
        state_contract(state);
    let pattern_summary = content
        .patterns()
        .iter()
        .map(|pattern| {
            format!(
                "{} / {} raw",
                short_id(pattern.id.as_str()),
                pattern.raw_damage
            )
        })
        .collect::<Vec<_>>()
        .join("   ");
    Ok(ShowcaseModel {
        boss_name: content
            .localized("boss.sir_caldus.name")
            .context("missing Caldus name")?
            .to_owned(),
        boss_description: content
            .localized("boss.sir_caldus.description")
            .context("missing Caldus description")?
            .to_owned(),
        exit_name: content
            .localized("portal.exit.dungeon.bell_sepulcher.name")
            .context("missing Caldus exit name")?
            .to_owned(),
        state,
        state_title,
        state_detail,
        phase_label,
        health_percent,
        status_chip,
        timeline,
        telegraph,
        reduced_effects,
        base_health: boss.base_health,
        armor: boss.armor,
        pattern_summary,
        records_revision: content.hashes().records_blake3[..12].to_owned(),
    })
}

#[allow(clippy::type_complexity)]
const fn state_contract(
    state: CoreCaldusShowcaseState,
) -> (
    &'static str,
    &'static str,
    &'static str,
    u8,
    &'static str,
    &'static str,
    TelegraphKind,
) {
    match state {
        CoreCaldusShowcaseState::Staging => (
            "PARTICIPANT LOCK",
            "Safe entrance clear. Door closure waits for the visible ready countdown.",
            "READY  03.2",
            100,
            "1 / 1 LOADED",
            "N LOCKED: 1   |   LATE ENTRY: CLOSED   |   SCALE: 7,200 HP",
            TelegraphKind::ShieldArc,
        ),
        CoreCaldusShowcaseState::Introduction => (
            "PARTICIPANTS LOCKED 1",
            "The door is closed. Caldus is visible and invulnerable during the 2.5-second introduction.",
            "INTRODUCTION  02.5",
            100,
            "NO HOSTILE OUTPUT",
            "N LOCKED: 1   |   7,200 HP / ARMOR 10   |   RECALL AVAILABLE",
            TelegraphKind::ShieldArc,
        ),
        CoreCaldusShowcaseState::PhaseOne => (
            "LEARN THE BELL",
            "Shield Arc locks its target. Follow the three-shot gap when Bell Ring sounds.",
            "PHASE I",
            82,
            "SHIELD ARC",
            "0.00 ARC   |   1.80 ARC   |   3.60 ARC   |   6.00 BELL RING",
            TelegraphKind::BellRing,
        ),
        CoreCaldusShowcaseState::ChargePressure => (
            "CARDINAL PRESSURE",
            "The lane locks before movement. Leave the telegraph, then follow the Stop Ring gap.",
            "PHASE II",
            52,
            "CHARGE LOCKED",
            "PREVIEW 700 MS   |   CHARGE 500 MS   |   CENTER RETURN 2.0 TILES/S",
            TelegraphKind::ChargeLane,
        ),
        CoreCaldusShowcaseState::FinalRings => (
            "FINAL CADENCE",
            "Alternating Bell Rings preserve a three-shot gap. Below 20%, only cadence accelerates.",
            "PHASE III",
            18,
            "FEVERED LOOP",
            "RING 0.80   |   RING 2.20   |   SHIELD 5.20   |   LOOP 7.20 -> 6.48 S",
            TelegraphKind::StopRing,
        ),
        CoreCaldusShowcaseState::VictoryExit => (
            "REWARDS TERMINAL",
            "Personal reward and XP receipts are durable. The stable exit may now be presented.",
            "CALDUS DEFEATED",
            0,
            "EXIT AUTHORIZED",
            "REWARD COMMITTED   |   XP 450 + FIRST CLEAR 225   |   EXIT ID STABLE",
            TelegraphKind::Exit,
        ),
        CoreCaldusShowcaseState::ExtractionCommitted => (
            "EXTRACTION COMMITTED",
            "The exact exit was accepted and its durable receipt now wins over crash restore.",
            "HALL TRANSFER PENDING",
            0,
            "RETRY SAFE",
            "RECEIPT COMMITTED   |   DANGER LOCATION RETAINED   |   INVENTORY UNCHANGED",
            TelegraphKind::Exit,
        ),
        CoreCaldusShowcaseState::HallArrival => (
            "AUTHORITATIVE RETURN",
            "The exact committed extraction receipt was consumed with the HallDefault transfer.",
            "LANTERN HALLS",
            0,
            "SAFE / COMMITTED",
            "RECEIPT CONSUMED   |   CHECKPOINT CLEARED   |   INVENTORY UNCHANGED (03E)",
            TelegraphKind::Hall,
        ),
    }
}

#[allow(clippy::needless_pass_by_value)]
fn spawn_showcase(mut commands: Commands, model: Res<ShowcaseModel>) {
    commands.spawn((Name::new("Caldus evidence camera"), Camera2d));
    commands
        .spawn((
            Name::new("Caldus evidence root"),
            GlobalZIndex(HUD_Z),
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(14)),
                row_gap: px(10),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(5, 7, 10)),
        ))
        .with_children(|root| {
            spawn_header(root, &model);
            root.spawn(Node {
                width: percent(100),
                flex_grow: 1.0,
                min_height: px(0),
                column_gap: px(10),
                ..default()
            })
            .with_children(|body| {
                spawn_playfield(body, &model);
                spawn_intel(body, &model);
            });
            spawn_footer(root, &model);
        });
}

fn spawn_header(parent: &mut ChildSpawnerCommands, model: &ShowcaseModel) {
    parent
        .spawn(panel(percent(100), px(86), Color::srgb_u8(159, 121, 55)))
        .with_children(|header| {
            header
                .spawn(Node {
                    width: percent(100),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|line| {
                    spawn_text(
                        line,
                        "GRAVEBOUND  /  BELL TOWER SEPULCHER  /  SIR CALDUS",
                        18.0,
                        Color::srgb_u8(238, 220, 174),
                    );
                    spawn_chip(line, model.status_chip, Color::srgb_u8(215, 162, 62));
                });
            spawn_text(
                header,
                format!(
                    "{}   |   {}   |   RECORDS {}   |   {}",
                    model.phase_label,
                    model.state_title,
                    model.records_revision,
                    if model.reduced_effects {
                        "REDUCED EFFECTS"
                    } else {
                        "STANDARD EFFECTS"
                    }
                ),
                13.0,
                Color::srgb_u8(176, 188, 184),
            );
        });
}

fn spawn_playfield(parent: &mut ChildSpawnerCommands, model: &ShowcaseModel) {
    parent
        .spawn(panel(percent(72), percent(100), Color::srgb_u8(74, 68, 55)))
        .with_children(|field| {
            spawn_boss_bar(field, model);
            field
                .spawn((
                    Node {
                        width: percent(100),
                        flex_grow: 1.0,
                        min_height: px(0),
                        position_type: PositionType::Relative,
                        overflow: Overflow::clip(),
                        border: UiRect::all(px(2)),
                        ..default()
                    },
                    BackgroundColor(if model.telegraph == TelegraphKind::Hall {
                        Color::srgb_u8(18, 27, 24)
                    } else {
                        Color::srgb_u8(12, 14, 17)
                    }),
                    BorderColor::all(Color::srgb_u8(118, 101, 68)),
                ))
                .with_children(|arena| spawn_arena(arena, model));
            spawn_text(field, model.timeline, 12.0, Color::srgb_u8(218, 187, 115));
        });
}

fn spawn_boss_bar(parent: &mut ChildSpawnerCommands, model: &ShowcaseModel) {
    parent
        .spawn(Node {
            width: percent(100),
            height: px(56),
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            ..default()
        })
        .with_children(|bar| {
            bar.spawn(Node {
                width: percent(100),
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            })
            .with_children(|line| {
                spawn_text(line, &model.boss_name, 16.0, Color::srgb_u8(237, 224, 195));
                spawn_text(
                    line,
                    format!("{}%   ARMOR {}", model.health_percent, model.armor),
                    12.0,
                    Color::srgb_u8(188, 176, 148),
                );
            });
            bar.spawn((
                Node {
                    width: percent(100),
                    height: px(12),
                    border: UiRect::all(px(1)),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(35, 29, 29)),
                BorderColor::all(Color::srgb_u8(92, 75, 65)),
            ))
            .with_children(|track| {
                track.spawn((
                    Node {
                        width: percent(f32::from(model.health_percent)),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(if model.health_percent <= 20 {
                        Color::srgb_u8(195, 68, 55)
                    } else {
                        Color::srgb_u8(151, 49, 43)
                    }),
                ));
            });
        });
}

fn spawn_arena(parent: &mut ChildSpawnerCommands, model: &ShowcaseModel) {
    if model.telegraph == TelegraphKind::Hall {
        spawn_hall(parent, model);
        return;
    }
    spawn_circle(
        parent,
        50.0,
        50.0,
        68.0,
        Color::srgba_u8(94, 82, 60, 24),
        Color::srgb_u8(94, 82, 60),
        2.0,
    );
    match model.telegraph {
        TelegraphKind::ShieldArc => {
            spawn_lane(
                parent,
                50.0,
                48.0,
                46.0,
                13.0,
                -24.0,
                Color::srgba_u8(222, 177, 70, 105),
            );
        }
        TelegraphKind::BellRing | TelegraphKind::StopRing => {
            spawn_circle(
                parent,
                50.0,
                50.0,
                52.0,
                Color::srgba_u8(219, 163, 57, 24),
                Color::srgb_u8(219, 163, 57),
                if model.reduced_effects { 5.0 } else { 3.0 },
            );
            spawn_gap(
                parent,
                if model.telegraph == TelegraphKind::StopRing {
                    68.0
                } else {
                    31.0
                },
                16.0,
            );
        }
        TelegraphKind::ChargeLane => {
            spawn_lane(
                parent,
                50.0,
                50.0,
                74.0,
                14.0,
                0.0,
                Color::srgba_u8(197, 70, 58, 112),
            );
            spawn_circle(
                parent,
                82.0,
                50.0,
                23.0,
                Color::srgba_u8(219, 163, 57, 22),
                Color::srgb_u8(219, 163, 57),
                3.0,
            );
        }
        TelegraphKind::Exit => spawn_exit(parent, model),
        TelegraphKind::Hall => {}
    }
    spawn_actor(
        parent,
        50.0,
        49.0,
        54.0,
        Color::srgb_u8(147, 113, 59),
        "CALDUS",
    );
    spawn_actor(
        parent,
        36.0,
        75.0,
        34.0,
        Color::srgb_u8(86, 150, 151),
        "YOU",
    );
    spawn_arena_label(parent, model);
}

fn spawn_hall(parent: &mut ChildSpawnerCommands, model: &ShowcaseModel) {
    for x in [18.0, 34.0, 66.0, 82.0] {
        spawn_lane(
            parent,
            x,
            52.0,
            7.0,
            72.0,
            0.0,
            Color::srgba_u8(184, 137, 55, 54),
        );
    }
    spawn_circle(
        parent,
        50.0,
        54.0,
        30.0,
        Color::srgba_u8(70, 147, 126, 34),
        Color::srgb_u8(70, 147, 126),
        3.0,
    );
    spawn_actor(
        parent,
        50.0,
        54.0,
        38.0,
        Color::srgb_u8(86, 150, 151),
        "YOU",
    );
    spawn_text_at(
        parent,
        50.0,
        18.0,
        "LANTERN HALLS  /  HALLDEFAULT",
        18.0,
        Color::srgb_u8(225, 191, 112),
    );
    spawn_text_at(
        parent,
        50.0,
        84.0,
        "SAFE ARRIVAL COMMITTED",
        14.0,
        Color::srgb_u8(98, 190, 153),
    );
    spawn_text_at(
        parent,
        50.0,
        91.0,
        &model.exit_name,
        11.0,
        Color::srgb_u8(157, 168, 161),
    );
}

fn spawn_exit(parent: &mut ChildSpawnerCommands, model: &ShowcaseModel) {
    spawn_circle(
        parent,
        72.0,
        42.0,
        24.0,
        Color::srgba_u8(70, 147, 126, 52),
        Color::srgb_u8(94, 208, 167),
        4.0,
    );
    spawn_text_at(
        parent,
        72.0,
        42.0,
        "EXIT",
        14.0,
        Color::srgb_u8(209, 245, 226),
    );
    spawn_text_at(
        parent,
        72.0,
        54.0,
        &model.exit_name,
        10.0,
        Color::srgb_u8(137, 206, 178),
    );
}

fn spawn_arena_label(parent: &mut ChildSpawnerCommands, model: &ShowcaseModel) {
    spawn_text_at(
        parent,
        50.0,
        8.0,
        model.state_title,
        15.0,
        Color::srgb_u8(236, 210, 149),
    );
}

fn spawn_intel(parent: &mut ChildSpawnerCommands, model: &ShowcaseModel) {
    parent
        .spawn(panel(percent(28), percent(100), Color::srgb_u8(74, 68, 55)))
        .with_children(|intel| {
            spawn_text(intel, "ENCOUNTER READ", 14.0, Color::srgb_u8(222, 178, 85));
            spawn_text(
                intel,
                model.state_detail,
                13.0,
                Color::srgb_u8(224, 221, 208),
            );
            spawn_rule(intel);
            spawn_stat(
                intel,
                "LOCKED HEALTH",
                format!("{} SOLO", model.base_health),
            );
            spawn_stat(intel, "PHASE BREAK", "4.0 S  /  +25% INCOMING");
            spawn_stat(intel, "SOFT ENRAGE", "360 S  /  CADENCE ONLY");
            spawn_rule(intel);
            spawn_text(
                intel,
                "HOSTILE LANGUAGE",
                12.0,
                Color::srgb_u8(222, 178, 85),
            );
            spawn_text(
                intel,
                &model.pattern_summary,
                10.0,
                Color::srgb_u8(162, 174, 169),
            );
            intel.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            spawn_text(
                intel,
                &model.boss_description,
                10.0,
                Color::srgb_u8(135, 145, 142),
            );
            spawn_chip(
                intel,
                "NORMAL INGRESS DISABLED",
                Color::srgb_u8(166, 77, 65),
            );
        });
}

fn spawn_footer(parent: &mut ChildSpawnerCommands, model: &ShowcaseModel) {
    parent
        .spawn(panel(percent(100), px(50), Color::srgb_u8(74, 68, 55)))
        .with_children(|footer| {
            spawn_text(
                footer,
                match model.state {
                    CoreCaldusShowcaseState::VictoryExit => "REWARD TERMINAL -> STABLE EXIT -> EXTRACTION REQUEST",
                    CoreCaldusShowcaseState::ExtractionCommitted => "EXTRACTION REQUEST -> COMMITTED RECEIPT -> RETRY-SAFE HALL TRANSFER",
                    CoreCaldusShowcaseState::HallArrival => "COMMITTED RECEIPT -> ATOMIC HALLDEFAULT -> CHECKPOINT CLEANUP -> SAFE",
                    _ => "SERVER-OWNED 30 HZ STATE  |  READABLE SHAPE + AUDIO LANGUAGE  |  NO CLIENT FINALITY",
                },
                12.0,
                Color::srgb_u8(197, 189, 165),
            );
            spawn_text(
                footer,
                "DISPOSABLE NATIVE EVIDENCE  |  GB-M03-08 INVENTORY CONVERSION ABSENT  |  CORE PROMOTION OFF",
                10.0,
                Color::srgb_u8(149, 158, 154),
            );
        });
}

fn panel(width: Val, height: Val, border: Color) -> impl Bundle {
    (
        Node {
            width,
            height,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(10)),
            row_gap: px(6),
            border: UiRect::all(px(1)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(9, 12, 15, 246)),
        BorderColor::all(border),
    )
}

fn spawn_text(
    parent: &mut ChildSpawnerCommands,
    value: impl Into<String>,
    size: f32,
    color: Color,
) {
    parent.spawn((
        Text::new(value),
        TextFont::from_font_size(size),
        TextColor(color),
        Node {
            max_width: percent(100),
            ..default()
        },
    ));
}

fn spawn_chip(parent: &mut ChildSpawnerCommands, value: &str, color: Color) {
    parent
        .spawn((
            Node {
                padding: UiRect::axes(px(9), px(4)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(color.with_alpha(0.12)),
            BorderColor::all(color),
        ))
        .with_children(|chip| spawn_text(chip, value, 11.0, color));
}

fn spawn_rule(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        Node {
            width: percent(100),
            height: px(1),
            ..default()
        },
        BackgroundColor(Color::srgb_u8(61, 63, 59)),
    ));
}

fn spawn_stat(parent: &mut ChildSpawnerCommands, label: &str, value: impl Into<String>) {
    parent
        .spawn(Node {
            width: percent(100),
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        })
        .with_children(|line| {
            spawn_text(line, label, 10.0, Color::srgb_u8(131, 143, 139));
            spawn_text(line, value, 10.0, Color::srgb_u8(220, 207, 176));
        });
}

fn spawn_circle(
    parent: &mut ChildSpawnerCommands,
    left: f32,
    top: f32,
    size: f32,
    fill: Color,
    border: Color,
    border_width: f32,
) {
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: percent(left - size / 2.0),
            top: percent(top - size / 2.0),
            width: percent(size),
            aspect_ratio: Some(1.0),
            border: UiRect::all(px(border_width)),
            border_radius: BorderRadius::all(percent(50)),
            ..default()
        },
        BackgroundColor(fill),
        BorderColor::all(border),
    ));
}

fn spawn_lane(
    parent: &mut ChildSpawnerCommands,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    rotation_degrees: f32,
    color: Color,
) {
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: percent(left - width / 2.0),
            top: percent(top - height / 2.0),
            width: percent(width),
            height: percent(height),
            border: UiRect::all(px(2)),
            ..default()
        },
        BackgroundColor(color),
        BorderColor::all(color.with_alpha(0.9)),
        UiTransform::from_rotation(Rot2::degrees(rotation_degrees)),
    ));
}

fn spawn_gap(parent: &mut ChildSpawnerCommands, left: f32, top: f32) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(left),
                top: percent(top),
                width: px(56),
                height: px(28),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(13, 23, 22)),
            BorderColor::all(Color::srgb_u8(88, 187, 155)),
        ))
        .with_children(|gap| spawn_text(gap, "GAP", 10.0, Color::srgb_u8(117, 220, 183)));
}

fn spawn_actor(
    parent: &mut ChildSpawnerCommands,
    left: f32,
    top: f32,
    size: f32,
    color: Color,
    label: &str,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(left),
                top: percent(top),
                width: px(size),
                height: px(size),
                margin: UiRect::all(px(-size / 2.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(3)),
                border_radius: BorderRadius::all(percent(50)),
                ..default()
            },
            BackgroundColor(color),
            BorderColor::all(Color::srgb_u8(236, 220, 179)),
        ))
        .with_children(|actor| {
            spawn_text(
                actor,
                label,
                if size > 40.0 { 10.0 } else { 8.0 },
                Color::srgb_u8(247, 242, 225),
            );
        });
}

fn spawn_text_at(
    parent: &mut ChildSpawnerCommands,
    left: f32,
    top: f32,
    value: impl Into<String>,
    size: f32,
    color: Color,
) {
    parent.spawn((
        Text::new(value),
        TextFont::from_font_size(size),
        TextColor(color),
        Node {
            position_type: PositionType::Absolute,
            left: percent(left),
            top: percent(top),
            max_width: percent(52),
            ..default()
        },
        UiTransform::from_translation(Val2::percent(-50.0, -50.0)),
    ));
}

fn short_id(id: &str) -> &str {
    id.rsplit('.').next().unwrap_or(id)
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
    use std::path::Path;

    use super::*;

    #[test]
    fn showcase_models_are_content_bound_and_cover_terminal_ordering() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let content = load_core_development_caldus(&root).unwrap();
        for state in [
            CoreCaldusShowcaseState::Staging,
            CoreCaldusShowcaseState::Introduction,
            CoreCaldusShowcaseState::PhaseOne,
            CoreCaldusShowcaseState::ChargePressure,
            CoreCaldusShowcaseState::FinalRings,
            CoreCaldusShowcaseState::VictoryExit,
            CoreCaldusShowcaseState::ExtractionCommitted,
            CoreCaldusShowcaseState::HallArrival,
        ] {
            let model = build_model(&content, state, true).unwrap();
            assert!(model.reduced_effects);
            assert_eq!(model.state, state);
            assert_eq!(model.base_health, 7_200);
            assert!(model.pattern_summary.contains("shield_arc"));
        }
        let exit = build_model(&content, CoreCaldusShowcaseState::VictoryExit, false).unwrap();
        let hall = build_model(&content, CoreCaldusShowcaseState::HallArrival, false).unwrap();
        assert_eq!(exit.health_percent, 0);
        assert_eq!(exit.telegraph, TelegraphKind::Exit);
        assert_eq!(hall.telegraph, TelegraphKind::Hall);
        assert!(hall.timeline.contains("CHECKPOINT CLEARED"));
    }
}
