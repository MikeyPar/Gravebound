//! Disposable native evidence surface for the unpromoted `GB-M03-03D` encounter package.

use std::{env, path::PathBuf};

use anyhow::{Context, Result, ensure};
use bevy::{
    prelude::*,
    render::view::screenshot::Screenshot,
    window::{PresentMode, PrimaryWindow, WindowResolution},
};
use sim_content::{
    CoreDevelopmentEncounterRooms, CoreFixedRoomEncounterPlan, compile_core_fixed_room_encounters,
    load_core_development_encounter_rooms,
};
use sim_core::{FixedRoomEvent, FixedRoomInput, FixedRoomPhase, Tick};

const EVIDENCE_SETTLE_FRAMES: u8 = 30;
const HUD_Z: i32 = 100;
const NORMAL_COUNT: usize = 6;
const MINIBOSS_COUNT: usize = 2;

#[derive(Debug, Clone)]
pub struct CoreEncounterShowcaseConfig {
    pub content_root: PathBuf,
    pub reduced_effects: bool,
}

#[derive(Debug, Clone)]
struct ActorCard {
    id: String,
    name: String,
    description: String,
    pattern_count: usize,
    miniboss: bool,
}

#[derive(Debug, Clone)]
struct RoomCard {
    node_id: String,
    room_id: String,
    status: String,
    detail: String,
    warning: bool,
    disabled: bool,
}

#[derive(Debug, Resource)]
struct ShowcaseModel {
    actors: Vec<ActorCard>,
    rooms: Vec<RoomCard>,
    disabled_branches: String,
    records_revision: String,
    layout_revision: String,
    reduced_effects: bool,
}

#[derive(Debug, Resource)]
struct ScreenshotRequest(PathBuf);

#[derive(Debug, Default)]
struct CaptureProgress {
    settled_frames: u8,
    queued: bool,
}

#[allow(clippy::needless_pass_by_value)]
pub fn run_core_encounter_showcase(config: CoreEncounterShowcaseConfig) -> Result<()> {
    let content = load_core_development_encounter_rooms(&config.content_root)
        .context("unpromoted Core encounter content failed validation")?;
    let model = build_model(&content, config.reduced_effects)?;
    let (window_width, window_height) = crate::configured_window_size()?;
    let screenshot_request = env::var_os("GRAVEBOUND_SCREENSHOT_PATH").map(PathBuf::from);

    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb_u8(7, 9, 12)))
        .insert_resource(model)
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Gravebound - GB-M03-03D Encounter Evidence".to_owned(),
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
    content: &CoreDevelopmentEncounterRooms,
    reduced_effects: bool,
) -> Result<ShowcaseModel> {
    ensure!(content.roster().len() == NORMAL_COUNT + MINIBOSS_COUNT);
    ensure!(content.rooms().len() == 9);
    let layout = content.compile_fixed_layout_definition()?;
    ensure!(layout.rooms.len() == 7);
    ensure!(layout.disabled_branch_node_ids == ["BB1", "BS1"]);
    let plans = compile_core_fixed_room_encounters(content, 1)?;
    ensure!(plans.len() == 4);

    let actors = content
        .roster()
        .iter()
        .enumerate()
        .map(|(index, actor)| {
            let id = actor.header.id.as_str();
            let name = content
                .localized(&format!("{id}.name"))
                .with_context(|| format!("missing showcase name for {id}"))?;
            let description = content
                .localized(&format!("{id}.description"))
                .with_context(|| format!("missing showcase description for {id}"))?;
            Ok(ActorCard {
                id: id.to_owned(),
                name: name.to_owned(),
                description: description.to_owned(),
                pattern_count: actor.required_pattern_ids.len(),
                miniboss: index >= NORMAL_COUNT,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let rooms = content
        .fixed_layout()
        .main_chain_node_ids
        .iter()
        .map(|node_id| build_room_card(content, &plans, node_id))
        .collect::<Result<Vec<_>>>()?;

    Ok(ShowcaseModel {
        actors,
        rooms,
        disabled_branches: layout.disabled_branch_node_ids.join(" + "),
        records_revision: content.hashes().records_blake3[..12].to_owned(),
        layout_revision: layout.deterministic_digest()[..12].to_owned(),
        reduced_effects,
    })
}

fn build_room_card(
    content: &CoreDevelopmentEncounterRooms,
    plans: &[CoreFixedRoomEncounterPlan],
    node_id: &str,
) -> Result<RoomCard> {
    let node = content
        .fixed_layout()
        .nodes
        .iter()
        .find(|node| node.node_id == node_id)
        .with_context(|| format!("missing fixed node {node_id}"))?;
    if let Some(plan) = plans.iter().find(|plan| plan.node_id == node_id) {
        let mut authority = plan.new_authority()?;
        let hostile_count = u16::try_from(plan.assignments().len())
            .context("showcase hostile count exceeds room authority capacity")?;
        let events = authority.step(
            Tick(0),
            FixedRoomInput {
                crossed_activation_boundary: true,
                living_inside: 1,
                living_party_outside: 0,
                doorway_hurtbox_blocked: false,
                required_hostiles_remaining: hostile_count,
                required_objectives_remaining: 0,
            },
        )?;
        ensure!(authority.phase() == FixedRoomPhase::SpawnWarning);
        ensure!(events.iter().any(|event| matches!(
            event,
            FixedRoomEvent::BeginGroupWarning { warning_ticks } if *warning_ticks == 27
        )));
        return Ok(RoomCard {
            node_id: node_id.to_owned(),
            room_id: plan.room_template_id.as_str().to_owned(),
            status: "WARNING".to_owned(),
            detail: format!(
                "{} HOSTILES  /  BUDGET {}  /  900 MS → T27",
                plan.assignments().len(),
                plan.base_budget
            ),
            warning: true,
            disabled: false,
        });
    }
    let (status, detail, disabled) = match node_id {
        "B0" => ("SAFE", "VESTIBULE / ENTRY", false),
        "B4" => ("REST", "BARGAIN SHRINE", false),
        "B6" => ("DISABLED", "SIR CALDUS RESERVED FOR 03E", true),
        _ => anyhow::bail!("unexpected encounterless main-chain node {node_id}"),
    };
    Ok(RoomCard {
        node_id: node_id.to_owned(),
        room_id: node.room_template_id.as_str().to_owned(),
        status: status.to_owned(),
        detail: detail.to_owned(),
        warning: false,
        disabled,
    })
}

#[allow(clippy::needless_pass_by_value)]
fn spawn_showcase(mut commands: Commands, model: Res<ShowcaseModel>) {
    commands.spawn((Name::new("Encounter evidence camera"), Camera2d));
    commands
        .spawn((
            Name::new("Encounter evidence root"),
            GlobalZIndex(HUD_Z),
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(14)),
                row_gap: px(10),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(7, 9, 12)),
        ))
        .with_children(|root| {
            spawn_header(root, &model);
            root.spawn(Node {
                width: percent(100),
                flex_grow: 1.0,
                column_gap: px(10),
                min_height: px(0),
                ..default()
            })
            .with_children(|body| {
                spawn_encounter_field(body, &model);
                spawn_roster(body, &model);
            });
            spawn_footer(root, &model);
        });
}

fn spawn_header(parent: &mut ChildSpawnerCommands, model: &ShowcaseModel) {
    parent
        .spawn(panel_node(percent(100), px(92), Color::srgb_u8(169, 142, 82)))
        .with_children(|header| {
            spawn_text(
                header,
                "GRAVEBOUND  /  GB-M03-03D  /  BELL SEPULCHER ENCOUNTER READABILITY",
                18.0,
                Color::srgb_u8(235, 224, 193),
            );
            spawn_text(
                header,
                format!(
                    "CORE 6 NORMAL + 2 MINIBOSSES  •  9 TEMPLATES  •  FIXED B0→B6  •  RECORDS {}  •  LAYOUT {}\n{}  •  HOSTILE WARNING PRIORITY PRESERVED",
                    model.records_revision,
                    model.layout_revision,
                    if model.reduced_effects { "REDUCED EFFECTS" } else { "STANDARD EFFECTS" }
                ),
                13.0,
                Color::srgb_u8(188, 196, 190),
            );
        });
}

fn spawn_encounter_field(parent: &mut ChildSpawnerCommands, model: &ShowcaseModel) {
    parent
        .spawn(panel_node(percent(72), percent(100), Color::srgb_u8(101, 83, 54)))
        .with_children(|field| {
            spawn_text(
                field,
                "FIXED PRIVATE-LIFE ROUTE  /  AUTHORITATIVE ACTIVATION STATE",
                14.0,
                Color::srgb_u8(226, 186, 91),
            );
            field
                .spawn(Node {
                    width: percent(100),
                    height: px(126),
                    column_gap: px(5),
                    align_items: AlignItems::Stretch,
                    ..default()
                })
                .with_children(|route| {
                    for room in &model.rooms {
                        spawn_room_card(route, room);
                    }
                });
            field
                .spawn((
                    Node {
                        width: percent(100),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(10)),
                        row_gap: px(8),
                        border: UiRect::all(px(if model.reduced_effects { 3 } else { 2 })),
                        ..default()
                    },
                    BackgroundColor(Color::srgb_u8(20, 23, 25)),
                    BorderColor::all(Color::srgb_u8(219, 164, 58)),
                ))
                .with_children(|arena| {
                    spawn_text(
                        arena,
                        "900 MS GROUP WARNING  •  DOORS SEALED HURTBOX-SAFE  •  ACTIVATES AT TICK 27",
                        14.0,
                        Color::srgb_u8(246, 194, 72),
                    );
                    arena
                        .spawn(Node {
                            width: percent(100),
                            flex_grow: 1.0,
                            display: Display::Grid,
                            grid_template_columns: RepeatedGridTrack::fr(4, 1.0),
                            grid_template_rows: RepeatedGridTrack::fr(2, 1.0),
                            column_gap: px(7),
                            row_gap: px(7),
                            ..default()
                        })
                        .with_children(|grid| {
                            for actor in &model.actors {
                                spawn_actor_tile(grid, actor, model.reduced_effects);
                            }
                        });
                });
        });
}

fn spawn_room_card(parent: &mut ChildSpawnerCommands, room: &RoomCard) {
    let border = if room.warning {
        Color::srgb_u8(219, 164, 58)
    } else if room.disabled {
        Color::srgb_u8(143, 69, 65)
    } else {
        Color::srgb_u8(83, 112, 99)
    };
    parent
        .spawn((
            Node {
                flex_basis: percent(14.28),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::all(px(7)),
                border: UiRect::all(px(1)),
                min_width: px(0),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(15, 18, 21)),
            BorderColor::all(border),
        ))
        .with_children(|card| {
            spawn_text(card, &room.node_id, 18.0, border);
            spawn_text(card, &room.status, 11.0, Color::srgb_u8(230, 222, 201));
            spawn_text(card, &room.detail, 9.0, Color::srgb_u8(166, 174, 170));
            spawn_text(card, &room.room_id, 8.0, Color::srgb_u8(112, 121, 119));
        });
}

fn spawn_actor_tile(parent: &mut ChildSpawnerCommands, actor: &ActorCard, reduced: bool) {
    let accent = actor_color(&actor.id);
    parent
        .spawn((
            Node {
                min_width: px(0),
                min_height: px(0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                padding: UiRect::all(px(5)),
                border: UiRect::all(px(if actor.miniboss { 2 } else { 1 })),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(12, 15, 18)),
            BorderColor::all(accent),
        ))
        .with_children(|tile| {
            tile.spawn((
                Node {
                    width: px(if actor.miniboss { 38 } else { 28 }),
                    height: px(if actor.miniboss { 38 } else { 28 }),
                    border: UiRect::all(px(if reduced { 4 } else { 2 })),
                    margin: UiRect::bottom(px(4)),
                    ..default()
                },
                BackgroundColor(accent.with_alpha(if reduced { 0.74 } else { 0.92 })),
                BorderColor::all(Color::srgb_u8(238, 220, 169)),
            ));
            spawn_text(tile, &actor.name, 12.0, Color::srgb_u8(235, 226, 207));
            spawn_text(
                tile,
                format!(
                    "{}  •  {} PATTERN{}",
                    if actor.miniboss { "MINIBOSS" } else { "NORMAL" },
                    actor.pattern_count,
                    if actor.pattern_count == 1 { "" } else { "S" }
                ),
                9.0,
                accent,
            );
        });
}

fn spawn_roster(parent: &mut ChildSpawnerCommands, model: &ShowcaseModel) {
    parent
        .spawn(panel_node(
            percent(28),
            percent(100),
            Color::srgb_u8(101, 83, 54),
        ))
        .with_children(|roster| {
            spawn_text(
                roster,
                "CONTENT-DRIVEN ROLE INDEX",
                14.0,
                Color::srgb_u8(226, 186, 91),
            );
            for actor in &model.actors {
                roster
                    .spawn((
                        Node {
                            width: percent(100),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            justify_content: JustifyContent::Center,
                            padding: UiRect::axes(px(7), px(3)),
                            border: UiRect::left(px(3)),
                            min_height: px(0),
                            ..default()
                        },
                        BackgroundColor(Color::srgb_u8(13, 16, 19)),
                        BorderColor::all(actor_color(&actor.id)),
                    ))
                    .with_children(|card| {
                        spawn_text(card, &actor.name, 12.0, Color::srgb_u8(232, 225, 203));
                        spawn_text(card, &actor.description, 9.0, Color::srgb_u8(153, 163, 160));
                    });
            }
        });
}

fn spawn_footer(parent: &mut ChildSpawnerCommands, model: &ShowcaseModel) {
    parent
        .spawn(panel_node(percent(100), px(62), Color::srgb_u8(101, 83, 54)))
        .with_children(|footer| {
            spawn_text(
                footer,
                format!(
                    "FAIL-CLOSED ROUTES  •  B6 SIR CALDUS: DISABLED (03E)  •  {}: DISABLED  •  SEEDED BRANCHES: OFF  •  NORMAL INGRESS: OFF",
                    model.disabled_branches
                ),
                13.0,
                Color::srgb_u8(224, 151, 134),
            );
            spawn_text(
                footer,
                "DISPOSABLE NATIVE EVIDENCE ONLY  •  SIMULATION OWNS STATE  •  PRESENTATION CANNOT ADMIT OR MUTATE A RUN",
                11.0,
                Color::srgb_u8(169, 177, 173),
            );
        });
}

fn panel_node(width: Val, height: Val, border: Color) -> impl Bundle {
    (
        Node {
            width,
            height,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(10)),
            row_gap: px(5),
            border: UiRect::all(px(1)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(10, 13, 16, 244)),
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

fn actor_color(id: &str) -> Color {
    if id.contains("sepulcher_knight") {
        Color::srgb_u8(202, 151, 71)
    } else if id.contains("choir_abbot") {
        Color::srgb_u8(159, 106, 178)
    } else if id.contains("mire_leech") {
        Color::srgb_u8(103, 155, 115)
    } else if id.contains("bell_reed") {
        Color::srgb_u8(192, 137, 77)
    } else if id.contains("bell_acolyte") {
        Color::srgb_u8(179, 91, 83)
    } else if id.contains("chain_sentry") {
        Color::srgb_u8(118, 145, 165)
    } else if id.contains("choir_skull") {
        Color::srgb_u8(142, 111, 164)
    } else {
        Color::srgb_u8(137, 153, 146)
    }
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
    fn showcase_model_is_compiled_and_fail_closed() {
        let content = load_core_development_encounter_rooms(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content"),
        )
        .expect("encounter content");
        let model = build_model(&content, true).expect("showcase model");
        assert_eq!(model.actors.len(), 8);
        assert_eq!(
            model.actors.iter().filter(|actor| actor.miniboss).count(),
            2
        );
        assert_eq!(model.rooms.len(), 7);
        assert_eq!(model.rooms.iter().filter(|room| room.warning).count(), 4);
        assert_eq!(model.rooms.iter().filter(|room| room.disabled).count(), 1);
        assert_eq!(model.disabled_branches, "BB1 + BS1");
        assert!(model.reduced_effects);
    }
}
