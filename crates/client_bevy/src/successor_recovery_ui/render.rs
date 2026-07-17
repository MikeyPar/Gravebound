use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    window::PrimaryWindow,
};

use crate::{DEATH_FONT_BOLD_PATH, DEATH_FONT_REGULAR_PATH};

use super::{
    NativeSuccessorRecoveryView, SuccessorRecoveryUiAction, SuccessorRecoveryUiCommand,
    SuccessorRecoveryUiFocusState, SuccessorRecoveryUiMetrics, SuccessorRecoveryUiReadiness,
    SuccessorRecoveryUiScrollState, SuccessorRecoveryUiSnapshot, SuccessorRecoveryUiTone,
};

#[derive(Debug, Component)]
pub(super) struct SuccessorRecoveryUiRoot;

#[derive(Debug, Component)]
pub(super) struct SuccessorRecoveryUiScrollRoot;

#[derive(Debug, Component)]
pub(super) struct SuccessorRecoveryUiScrollTrack;

#[derive(Debug, Component)]
pub(super) struct SuccessorRecoveryUiScrollThumb;

#[derive(Debug, Clone, Component)]
pub(super) struct SuccessorRecoveryUiButton {
    action: SuccessorRecoveryUiAction,
    enabled: bool,
    primary: bool,
    order: u16,
}

#[derive(Debug, Clone, Copy, Component)]
pub(super) enum SuccessorRecoveryFontWeight {
    Regular,
    Bold,
}

#[derive(Debug, Resource)]
pub(super) struct SuccessorRecoveryUiFonts {
    regular: Handle<Font>,
    bold: Handle<Font>,
    settled: bool,
}

impl FromWorld for SuccessorRecoveryUiFonts {
    fn from_world(world: &mut World) -> Self {
        let assets = world.resource::<AssetServer>();
        Self {
            regular: assets.load(DEATH_FONT_REGULAR_PATH),
            bold: assets.load(DEATH_FONT_BOLD_PATH),
            settled: false,
        }
    }
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn rebuild(
    mut commands: Commands,
    view: Option<Res<NativeSuccessorRecoveryView>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    roots: Query<Entity, With<SuccessorRecoveryUiRoot>>,
    mut focus: ResMut<SuccessorRecoveryUiFocusState>,
    mut readiness: ResMut<SuccessorRecoveryUiReadiness>,
) {
    let Some(view) = view else {
        for entity in &roots {
            commands.entity(entity).despawn();
        }
        focus.focused_order = None;
        focus.ensure_visible = false;
        readiness.ready = false;
        return;
    };
    if !view.is_changed() && !roots.is_empty() {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok(metrics) = SuccessorRecoveryUiMetrics::for_viewport(
        window.resolution.width(),
        window.resolution.height(),
        view.config.ui_scale_percent,
    ) else {
        return;
    };
    for entity in &roots {
        commands.entity(entity).despawn();
    }
    readiness.ready = false;
    focus.focused_order = view
        .snapshot
        .actions
        .iter()
        .position(|action| action.enabled)
        .and_then(|index| u16::try_from(index).ok());
    focus.ensure_visible = focus.focused_order.is_some();
    spawn_view(
        &mut commands,
        &view.snapshot,
        view.config.reduced_effects,
        metrics,
    );
}

fn spawn_view(
    commands: &mut Commands,
    snapshot: &SuccessorRecoveryUiSnapshot,
    reduced_effects: bool,
    metrics: SuccessorRecoveryUiMetrics,
) {
    commands
        .spawn((
            Name::new("Successor recovery root"),
            SuccessorRecoveryUiRoot,
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(100),
                height: percent(100),
                padding: UiRect::all(px(metrics.safe_margin_px)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgb_u8(5, 7, 9)),
            GlobalZIndex(120),
        ))
        .with_children(|root| {
            spawn_backdrop(root, reduced_effects);
            root.spawn((
                Name::new("Successor recovery panel"),
                Node {
                    width: px(metrics.panel_width_px),
                    height: px(metrics.panel_height_px),
                    max_width: percent(100),
                    max_height: percent(100),
                    border: UiRect::all(px(2)),
                    position_type: PositionType::Relative,
                    ..default()
                },
                BackgroundColor(Color::srgba_u8(12, 16, 18, 248)),
                BorderColor::all(tone_accent(snapshot.tone)),
            ))
            .with_children(|panel| {
                panel
                    .spawn((
                        Node {
                            width: percent(100),
                            height: percent(100),
                            padding: UiRect::new(
                                px(metrics.safe_margin_px.clamp(20.0, 38.0)),
                                px(metrics.safe_margin_px.clamp(30.0, 48.0)),
                                px(metrics.safe_margin_px.clamp(20.0, 38.0)),
                                px(metrics.safe_margin_px.clamp(20.0, 38.0)),
                            ),
                            overflow: Overflow::scroll_y(),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(16),
                            ..default()
                        },
                        ScrollPosition::default(),
                        SuccessorRecoveryUiScrollRoot,
                    ))
                    .with_children(|content| {
                        spawn_header(content, snapshot, metrics);
                        spawn_rule(content, tone_accent(snapshot.tone));
                        if let Some(character) = snapshot.character.as_ref() {
                            spawn_character_body(content, snapshot, character, metrics);
                        } else {
                            spawn_pending_body(content, snapshot, metrics);
                        }
                        spawn_actions(content, snapshot, metrics);
                        spawn_footer(content, snapshot, metrics);
                    });
                panel
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            right: px(10),
                            top: px(12),
                            bottom: px(12),
                            width: px(4),
                            ..default()
                        },
                        BackgroundColor(Color::srgba_u8(73, 70, 61, 110)),
                        Visibility::Hidden,
                        SuccessorRecoveryUiScrollTrack,
                    ))
                    .with_child((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            top: percent(0),
                            width: percent(100),
                            height: percent(25),
                            ..default()
                        },
                        BackgroundColor(Color::srgb_u8(190, 146, 77)),
                        SuccessorRecoveryUiScrollThumb,
                    ));
            });
        });
}

fn spawn_backdrop(parent: &mut ChildSpawnerCommands, reduced_effects: bool) {
    let rings: u8 = if reduced_effects { 1 } else { 3 };
    for index in 0..rings {
        let inset = 7.0 + f32::from(index) * 7.0;
        parent.spawn((
            Name::new("Successor recovery bell ring"),
            Node {
                position_type: PositionType::Absolute,
                left: percent(inset),
                right: percent(inset),
                top: percent(inset),
                bottom: percent(inset),
                border: UiRect::all(px(1)),
                ..default()
            },
            BorderColor::all(Color::srgba_u8(150, 112, 59, 42)),
        ));
    }
}

fn spawn_header(
    parent: &mut ChildSpawnerCommands,
    snapshot: &SuccessorRecoveryUiSnapshot,
    metrics: SuccessorRecoveryUiMetrics,
) {
    parent
        .spawn((Node {
            width: percent(100),
            align_items: AlignItems::FlexEnd,
            justify_content: JustifyContent::SpaceBetween,
            column_gap: px(24),
            ..default()
        },))
        .with_children(|row| {
            row.spawn((Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            },))
                .with_children(|copy| {
                    spawn_text(
                        copy,
                        &snapshot.eyebrow,
                        metrics.label_text_px,
                        Color::srgb_u8(194, 154, 91),
                        SuccessorRecoveryFontWeight::Bold,
                    );
                    spawn_text(
                        copy,
                        &snapshot.title,
                        metrics.title_text_px,
                        Color::srgb_u8(241, 233, 211),
                        SuccessorRecoveryFontWeight::Bold,
                    );
                    spawn_text(
                        copy,
                        &snapshot.subtitle,
                        metrics.body_text_px,
                        Color::srgb_u8(175, 183, 178),
                        SuccessorRecoveryFontWeight::Regular,
                    );
                });
            row.spawn((
                Node {
                    padding: UiRect::axes(px(14), px(8)),
                    border: UiRect::all(px(1)),
                    ..default()
                },
                BackgroundColor(tone_background(snapshot.tone)),
                BorderColor::all(tone_accent(snapshot.tone)),
            ))
            .with_child((
                Text::new(snapshot.status.clone()),
                TextFont::from_font_size(metrics.label_text_px),
                TextColor(Color::srgb_u8(242, 229, 194)),
                SuccessorRecoveryFontWeight::Bold,
            ));
        });
}

fn spawn_rule(parent: &mut ChildSpawnerCommands, color: Color) {
    parent.spawn((
        Node {
            width: percent(100),
            height: px(1),
            ..default()
        },
        BackgroundColor(color.with_alpha(0.55)),
    ));
}

fn spawn_character_body(
    parent: &mut ChildSpawnerCommands,
    snapshot: &SuccessorRecoveryUiSnapshot,
    character: &super::SuccessorRecoveryUiCharacter,
    metrics: SuccessorRecoveryUiMetrics,
) {
    parent
        .spawn((Node {
            width: percent(100),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Row,
            column_gap: px(24),
            ..default()
        },))
        .with_children(|body| {
            body.spawn((
                Node {
                    width: percent(38),
                    min_width: px(290),
                    padding: UiRect::all(px(18)),
                    border: UiRect::all(px(1)),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    row_gap: px(12),
                    ..default()
                },
                BackgroundColor(Color::srgba_u8(17, 23, 25, 242)),
                BorderColor::all(Color::srgb_u8(67, 85, 81)),
            ))
            .with_children(|card| {
                spawn_badge(card, &snapshot.selected_badge, metrics.label_text_px);
                spawn_silhouette(card);
                spawn_text(
                    card,
                    &character.slot_text,
                    metrics.label_text_px,
                    Color::srgb_u8(161, 169, 164),
                    SuccessorRecoveryFontWeight::Bold,
                );
            });
            body.spawn((
                Node {
                    flex_grow: 1.0,
                    padding: UiRect::all(px(24)),
                    border: UiRect::all(px(1)),
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::SpaceBetween,
                    row_gap: px(14),
                    ..default()
                },
                BackgroundColor(Color::srgba_u8(20, 18, 16, 238)),
                BorderColor::all(Color::srgb_u8(95, 74, 45)),
            ))
            .with_children(|details| {
                details
                    .spawn((Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(8),
                        ..default()
                    },))
                    .with_children(|copy| {
                        spawn_text(
                            copy,
                            &character.class_name,
                            metrics.heading_text_px,
                            Color::srgb_u8(241, 229, 201),
                            SuccessorRecoveryFontWeight::Bold,
                        );
                        for value in [
                            character.level_text.as_str(),
                            character.oath_text.as_str(),
                            character.starter_text.as_str(),
                            character.security_text.as_str(),
                        ] {
                            spawn_text(
                                copy,
                                value,
                                metrics.body_text_px,
                                Color::srgb_u8(191, 194, 182),
                                SuccessorRecoveryFontWeight::Regular,
                            );
                        }
                    });
                spawn_progress(details, snapshot, metrics);
            });
        });
}

fn spawn_pending_body(
    parent: &mut ChildSpawnerCommands,
    snapshot: &SuccessorRecoveryUiSnapshot,
    metrics: SuccessorRecoveryUiMetrics,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_grow: 1.0,
                padding: UiRect::all(px(32)),
                border: UiRect::all(px(1)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(16),
                ..default()
            },
            BackgroundColor(Color::srgba_u8(17, 21, 22, 242)),
            BorderColor::all(tone_accent(snapshot.tone)),
        ))
        .with_children(|body| {
            spawn_bell_mark(body);
            spawn_text(
                body,
                &snapshot.status,
                metrics.heading_text_px,
                Color::srgb_u8(231, 216, 181),
                SuccessorRecoveryFontWeight::Bold,
            );
            spawn_progress(body, snapshot, metrics);
        });
}

fn spawn_badge(parent: &mut ChildSpawnerCommands, value: &str, text_px: f32) {
    parent
        .spawn((
            Node {
                padding: UiRect::axes(px(12), px(6)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(49, 38, 23)),
            BorderColor::all(Color::srgb_u8(193, 147, 76)),
        ))
        .with_child((
            Text::new(value.to_owned()),
            TextFont::from_font_size(text_px),
            TextColor(Color::srgb_u8(240, 219, 174)),
            SuccessorRecoveryFontWeight::Bold,
        ));
}

fn spawn_silhouette(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Relative,
                width: px(160),
                height: px(220),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(9, 12, 13)),
        ))
        .with_children(|shape| {
            spawn_shape(shape, 59.0, 20.0, 42.0, 42.0, Color::srgb_u8(176, 184, 174));
            spawn_shape(
                shape,
                43.0,
                67.0,
                74.0,
                104.0,
                Color::srgb_u8(105, 120, 115),
            );
            spawn_shape(shape, 29.0, 170.0, 44.0, 42.0, Color::srgb_u8(73, 88, 86));
            spawn_shape(shape, 87.0, 170.0, 44.0, 42.0, Color::srgb_u8(73, 88, 86));
            spawn_shape(shape, 18.0, 108.0, 124.0, 8.0, Color::srgb_u8(203, 156, 82));
            spawn_shape(shape, 112.0, 83.0, 7.0, 61.0, Color::srgb_u8(150, 113, 65));
        });
}

fn spawn_bell_mark(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Relative,
                width: px(92),
                height: px(92),
                border: UiRect::all(px(2)),
                ..default()
            },
            BorderColor::all(Color::srgb_u8(177, 132, 69)),
        ))
        .with_children(|mark| {
            spawn_shape(mark, 24.0, 20.0, 44.0, 38.0, Color::srgb_u8(117, 91, 51));
            spawn_shape(mark, 18.0, 58.0, 56.0, 7.0, Color::srgb_u8(188, 143, 72));
            spawn_shape(mark, 41.0, 65.0, 10.0, 12.0, Color::srgb_u8(188, 143, 72));
        });
}

fn spawn_shape(
    parent: &mut ChildSpawnerCommands,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    color: Color,
) {
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(left),
            top: px(top),
            width: px(width),
            height: px(height),
            ..default()
        },
        BackgroundColor(color),
    ));
}

fn spawn_progress(
    parent: &mut ChildSpawnerCommands,
    snapshot: &SuccessorRecoveryUiSnapshot,
    metrics: SuccessorRecoveryUiMetrics,
) {
    parent
        .spawn((Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            row_gap: px(8),
            ..default()
        },))
        .with_children(|progress| {
            if let Some(confirmation) = snapshot.confirmation.as_ref() {
                spawn_text(
                    progress,
                    confirmation,
                    metrics.label_text_px,
                    Color::srgb_u8(204, 178, 125),
                    SuccessorRecoveryFontWeight::Bold,
                );
            }
            progress
                .spawn((Node {
                    width: percent(100),
                    height: px(8),
                    column_gap: px(6),
                    ..default()
                },))
                .with_children(|bar| {
                    for step in 1..=2 {
                        bar.spawn((
                            Node {
                                flex_grow: 1.0,
                                height: px(8),
                                ..default()
                            },
                            BackgroundColor(if snapshot.progress_completed >= step {
                                Color::srgb_u8(190, 142, 71)
                            } else {
                                Color::srgb_u8(48, 51, 48)
                            }),
                        ));
                    }
                });
        });
}

fn spawn_actions(
    parent: &mut ChildSpawnerCommands,
    snapshot: &SuccessorRecoveryUiSnapshot,
    metrics: SuccessorRecoveryUiMetrics,
) {
    if snapshot.actions.is_empty() {
        return;
    }
    parent
        .spawn((Node {
            width: percent(100),
            justify_content: JustifyContent::FlexEnd,
            column_gap: px(12),
            ..default()
        },))
        .with_children(|actions| {
            for (index, action) in snapshot.actions.iter().enumerate() {
                let order = u16::try_from(index).unwrap_or(u16::MAX);
                actions
                    .spawn((
                        Button,
                        Node {
                            min_width: px(240),
                            height: px(metrics.action_height_px),
                            padding: UiRect::axes(px(20), px(10)),
                            border: UiRect::all(px(2)),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::SpaceBetween,
                            column_gap: px(24),
                            ..default()
                        },
                        BackgroundColor(Color::srgb_u8(47, 34, 22)),
                        BorderColor::all(Color::srgb_u8(181, 137, 72)),
                        SuccessorRecoveryUiButton {
                            action: action.action,
                            enabled: action.enabled,
                            primary: action.primary,
                            order,
                        },
                    ))
                    .with_children(|button| {
                        spawn_text(
                            button,
                            &action.label,
                            metrics.body_text_px,
                            Color::srgb_u8(242, 229, 200),
                            SuccessorRecoveryFontWeight::Bold,
                        );
                        if let Some(hint) = action.input_hint.as_ref() {
                            spawn_text(
                                button,
                                hint,
                                metrics.label_text_px,
                                Color::srgb_u8(181, 168, 139),
                                SuccessorRecoveryFontWeight::Regular,
                            );
                        }
                    });
            }
        });
}

fn spawn_footer(
    parent: &mut ChildSpawnerCommands,
    snapshot: &SuccessorRecoveryUiSnapshot,
    metrics: SuccessorRecoveryUiMetrics,
) {
    parent
        .spawn((Node {
            width: percent(100),
            justify_content: JustifyContent::SpaceBetween,
            column_gap: px(24),
            ..default()
        },))
        .with_children(|footer| {
            spawn_text(
                footer,
                &snapshot.authority_footer,
                metrics.label_text_px,
                Color::srgb_u8(135, 147, 141),
                SuccessorRecoveryFontWeight::Regular,
            );
            spawn_text(
                footer,
                &snapshot.clean_recovery_footer,
                metrics.label_text_px,
                Color::srgb_u8(135, 147, 141),
                SuccessorRecoveryFontWeight::Regular,
            );
        });
}

fn spawn_text(
    parent: &mut ChildSpawnerCommands,
    value: &str,
    size: f32,
    color: Color,
    weight: SuccessorRecoveryFontWeight,
) {
    parent.spawn((
        Text::new(value.to_owned()),
        TextFont::from_font_size(size),
        TextColor(color),
        weight,
    ));
}

const fn tone_accent(tone: SuccessorRecoveryUiTone) -> Color {
    match tone {
        SuccessorRecoveryUiTone::Neutral => Color::srgb_u8(151, 118, 67),
        SuccessorRecoveryUiTone::Success => Color::srgb_u8(151, 139, 82),
        SuccessorRecoveryUiTone::Warning => Color::srgb_u8(191, 128, 60),
        SuccessorRecoveryUiTone::Error => Color::srgb_u8(177, 77, 64),
    }
}

const fn tone_background(tone: SuccessorRecoveryUiTone) -> Color {
    match tone {
        SuccessorRecoveryUiTone::Neutral => Color::srgb_u8(35, 31, 24),
        SuccessorRecoveryUiTone::Success => Color::srgb_u8(32, 40, 30),
        SuccessorRecoveryUiTone::Warning => Color::srgb_u8(48, 34, 21),
        SuccessorRecoveryUiTone::Error => Color::srgb_u8(48, 24, 23),
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn apply_fonts(
    assets: Res<AssetServer>,
    mut fonts: ResMut<SuccessorRecoveryUiFonts>,
    mut texts: Query<(&SuccessorRecoveryFontWeight, &mut TextFont), Added<Text>>,
) {
    for (weight, mut text_font) in &mut texts {
        text_font.font = FontSource::Handle(match weight {
            SuccessorRecoveryFontWeight::Regular => fonts.regular.clone(),
            SuccessorRecoveryFontWeight::Bold => fonts.bold.clone(),
        });
    }
    fonts.settled = assets.is_loaded_with_dependencies(fonts.regular.id())
        && assets.is_loaded_with_dependencies(fonts.bold.id());
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn update_readiness(
    fonts: Res<SuccessorRecoveryUiFonts>,
    roots: Query<&ComputedNode, With<SuccessorRecoveryUiRoot>>,
    texts: Query<&ComputedNode, With<SuccessorRecoveryFontWeight>>,
    mut readiness: ResMut<SuccessorRecoveryUiReadiness>,
) {
    readiness.ready = fonts.settled
        && roots
            .iter()
            .any(|node| node.size().x > 0.0 && node.size().y > 0.0)
        && texts
            .iter()
            .any(|node| node.size().x > 0.0 && node.size().y > 0.0);
}

type ChangedButtons<'w, 's> = Query<
    'w,
    's,
    (&'static Interaction, &'static SuccessorRecoveryUiButton),
    (Changed<Interaction>, With<Button>),
>;

#[allow(clippy::needless_pass_by_value)]
pub(super) fn handle_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut changed: ChangedButtons,
    buttons: Query<&SuccessorRecoveryUiButton, With<Button>>,
    mut focus: ResMut<SuccessorRecoveryUiFocusState>,
    mut commands: MessageWriter<SuccessorRecoveryUiCommand>,
) {
    let mut ordered = buttons.iter().cloned().collect::<Vec<_>>();
    ordered.sort_by_key(|button| button.order);
    for (interaction, button) in &mut changed {
        if button.enabled && matches!(interaction, Interaction::Hovered | Interaction::Pressed) {
            focus.ensure_visible = focus.focused_order != Some(button.order);
            focus.focused_order = Some(button.order);
        }
        if button.enabled && *interaction == Interaction::Pressed {
            commands.write(SuccessorRecoveryUiCommand(button.action));
        }
    }
    let gamepad_pressed = |button| gamepads.iter().any(|gamepad| gamepad.just_pressed(button));
    let previous = keyboard.just_pressed(KeyCode::ArrowUp)
        || keyboard.just_pressed(KeyCode::ArrowLeft)
        || (keyboard.just_pressed(KeyCode::Tab)
            && (keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight)))
        || gamepad_pressed(GamepadButton::DPadUp)
        || gamepad_pressed(GamepadButton::DPadLeft);
    let next = keyboard.just_pressed(KeyCode::ArrowDown)
        || keyboard.just_pressed(KeyCode::ArrowRight)
        || (keyboard.just_pressed(KeyCode::Tab)
            && !keyboard.pressed(KeyCode::ShiftLeft)
            && !keyboard.pressed(KeyCode::ShiftRight))
        || gamepad_pressed(GamepadButton::DPadDown)
        || gamepad_pressed(GamepadButton::DPadRight);
    if previous || next {
        focus.focused_order = cycle_focus(&ordered, focus.focused_order, !previous);
        focus.ensure_visible = true;
    }
    let activate = keyboard.just_pressed(KeyCode::Enter)
        || keyboard.just_pressed(KeyCode::Space)
        || gamepad_pressed(GamepadButton::South);
    if activate
        && let Some(button) = focus.focused_order.and_then(|order| {
            ordered
                .iter()
                .find(|button| button.order == order && button.enabled)
        })
    {
        commands.write(SuccessorRecoveryUiCommand(button.action));
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn scroll(
    mut wheel: MessageReader<MouseWheel>,
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut roots: Query<(&mut ScrollPosition, &ComputedNode), With<SuccessorRecoveryUiScrollRoot>>,
) {
    let mut delta = wheel
        .read()
        .map(|event| match event.unit {
            MouseScrollUnit::Line => -event.y * 42.0,
            MouseScrollUnit::Pixel => -event.y,
        })
        .sum::<f32>();
    if keyboard.just_pressed(KeyCode::PageDown)
        || gamepads
            .iter()
            .any(|gamepad| gamepad.just_pressed(GamepadButton::RightTrigger))
    {
        delta += 320.0;
    }
    if keyboard.just_pressed(KeyCode::PageUp)
        || gamepads
            .iter()
            .any(|gamepad| gamepad.just_pressed(GamepadButton::LeftTrigger))
    {
        delta -= 320.0;
    }
    if delta == 0.0 {
        return;
    }
    for (mut position, computed) in &mut roots {
        let max_offset = ((computed.content_size() - computed.size())
            * computed.inverse_scale_factor())
        .y
        .max(0.0);
        position.y = (position.y + delta).clamp(0.0, max_offset);
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn keep_focused_visible(
    mut focus: ResMut<SuccessorRecoveryUiFocusState>,
    mut roots: Query<
        (&mut ScrollPosition, &ComputedNode, &UiGlobalTransform),
        With<SuccessorRecoveryUiScrollRoot>,
    >,
    buttons: Query<
        (
            &SuccessorRecoveryUiButton,
            &ComputedNode,
            &UiGlobalTransform,
        ),
        With<Button>,
    >,
) {
    if !focus.ensure_visible {
        return;
    }
    focus.ensure_visible = false;
    let Some(order) = focus.focused_order else {
        return;
    };
    let Some((_, button_node, button_transform)) =
        buttons.iter().find(|(button, _, _)| button.order == order)
    else {
        return;
    };
    let Ok((mut scroll, root_node, root_transform)) = roots.single_mut() else {
        return;
    };
    let (_, _, root_translation) = root_transform.to_scale_angle_translation();
    let (_, _, button_translation) = button_transform.to_scale_angle_translation();
    let root_scale = root_node.inverse_scale_factor();
    let button_scale = button_node.inverse_scale_factor();
    let root_center_y = root_translation.y * root_scale;
    let button_center_y = button_translation.y * button_scale;
    let root_height = root_node.size().y * root_scale;
    let button_height = button_node.size().y * button_scale;
    let max_offset = ((root_node.content_size() - root_node.size()) * root_scale)
        .y
        .max(0.0);
    scroll.y = scroll_offset_to_reveal(
        scroll.y,
        max_offset,
        root_center_y - root_height * 0.5,
        root_center_y + root_height * 0.5,
        button_center_y - button_height * 0.5,
        button_center_y + button_height * 0.5,
        12.0,
    );
}

#[allow(clippy::too_many_arguments)]
fn scroll_offset_to_reveal(
    current_offset: f32,
    max_offset: f32,
    viewport_top: f32,
    viewport_bottom: f32,
    item_top: f32,
    item_bottom: f32,
    padding: f32,
) -> f32 {
    let target = if item_top < viewport_top + padding {
        current_offset - (viewport_top + padding - item_top)
    } else if item_bottom > viewport_bottom - padding {
        current_offset + (item_bottom - (viewport_bottom - padding))
    } else {
        current_offset
    };
    target.clamp(0.0, max_offset.max(0.0))
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ScrollbarGeometry {
    height_percent: f32,
    top_percent: f32,
    max_offset: f32,
}

fn scrollbar_geometry(
    viewport_height: f32,
    content_height: f32,
    scroll_offset: f32,
    inverse_scale_factor: f32,
) -> Option<ScrollbarGeometry> {
    let overflow = (content_height - viewport_height).max(0.0);
    if !viewport_height.is_finite()
        || !content_height.is_finite()
        || !scroll_offset.is_finite()
        || !inverse_scale_factor.is_finite()
        || viewport_height <= 0.0
        || content_height <= 0.0
        || inverse_scale_factor <= 0.0
        || overflow <= 0.5
    {
        return None;
    }
    let thumb_fraction = (viewport_height / content_height).clamp(0.12, 1.0);
    let max_offset = overflow * inverse_scale_factor;
    let progress = (scroll_offset / max_offset).clamp(0.0, 1.0);
    Some(ScrollbarGeometry {
        height_percent: thumb_fraction * 100.0,
        top_percent: progress * (1.0 - thumb_fraction) * 100.0,
        max_offset,
    })
}

pub(super) fn update_scrollbar(
    roots: Query<(&ScrollPosition, &ComputedNode), With<SuccessorRecoveryUiScrollRoot>>,
    mut tracks: Query<&mut Visibility, With<SuccessorRecoveryUiScrollTrack>>,
    mut thumbs: Query<&mut Node, With<SuccessorRecoveryUiScrollThumb>>,
    mut state: ResMut<SuccessorRecoveryUiScrollState>,
) {
    let Ok((position, computed)) = roots.single() else {
        *state = SuccessorRecoveryUiScrollState::default();
        return;
    };
    let Ok(mut track_visibility) = tracks.single_mut() else {
        return;
    };
    let Ok(mut thumb) = thumbs.single_mut() else {
        return;
    };
    let Some(geometry) = scrollbar_geometry(
        computed.size().y,
        computed.content_size().y,
        position.y,
        computed.inverse_scale_factor(),
    ) else {
        *state = SuccessorRecoveryUiScrollState::default();
        *track_visibility = Visibility::Hidden;
        return;
    };
    *state = SuccessorRecoveryUiScrollState {
        offset: position.y,
        max_offset: geometry.max_offset,
    };
    *track_visibility = Visibility::Visible;
    thumb.height = percent(geometry.height_percent);
    thumb.top = percent(geometry.top_percent);
}

fn cycle_focus(
    buttons: &[SuccessorRecoveryUiButton],
    current: Option<u16>,
    forward: bool,
) -> Option<u16> {
    let enabled = buttons
        .iter()
        .filter(|button| button.enabled)
        .map(|button| button.order)
        .collect::<Vec<_>>();
    if enabled.is_empty() {
        return None;
    }
    let current_index = current.and_then(|order| enabled.iter().position(|value| *value == order));
    Some(match (current_index, forward) {
        (Some(index), true) => enabled[(index + 1) % enabled.len()],
        (Some(0) | None, false) => *enabled.last().unwrap_or(&enabled[0]),
        (Some(index), false) => enabled[index - 1],
        (None, true) => enabled[0],
    })
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn style_buttons(
    focus: Res<SuccessorRecoveryUiFocusState>,
    mut buttons: Query<
        (
            &Interaction,
            &SuccessorRecoveryUiButton,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<Button>,
    >,
) {
    for (interaction, button, mut background, mut border) in &mut buttons {
        if !button.enabled {
            background.0 = Color::srgb_u8(24, 25, 24);
            *border = BorderColor::all(Color::srgb_u8(55, 57, 54));
            continue;
        }
        let focused = focus.focused_order == Some(button.order);
        match interaction {
            Interaction::Pressed => {
                background.0 = Color::srgb_u8(78, 54, 30);
                *border = BorderColor::all(Color::srgb_u8(241, 219, 163));
            }
            Interaction::Hovered => {
                background.0 = Color::srgb_u8(66, 47, 29);
                *border = BorderColor::all(Color::srgb_u8(213, 179, 113));
            }
            Interaction::None => {
                background.0 = if button.primary {
                    Color::srgb_u8(49, 36, 23)
                } else {
                    Color::srgb_u8(28, 32, 31)
                };
                *border = BorderColor::all(if focused {
                    Color::srgb_u8(239, 222, 171)
                } else {
                    Color::srgb_u8(181, 137, 72)
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_cycles_only_enabled_actions_and_wraps() {
        let buttons = vec![
            SuccessorRecoveryUiButton {
                action: SuccessorRecoveryUiAction::Play,
                enabled: false,
                primary: true,
                order: 0,
            },
            SuccessorRecoveryUiButton {
                action: SuccessorRecoveryUiAction::RetryCreate,
                enabled: true,
                primary: true,
                order: 1,
            },
            SuccessorRecoveryUiButton {
                action: SuccessorRecoveryUiAction::RefreshDeathSummary,
                enabled: true,
                primary: false,
                order: 2,
            },
        ];
        assert_eq!(cycle_focus(&buttons, None, true), Some(1));
        assert_eq!(cycle_focus(&buttons, Some(1), true), Some(2));
        assert_eq!(cycle_focus(&buttons, Some(2), true), Some(1));
        assert_eq!(cycle_focus(&buttons, Some(1), false), Some(2));
    }

    #[test]
    fn scrollbar_is_hidden_without_overflow_and_tracks_both_extents() {
        assert_eq!(scrollbar_geometry(600.0, 600.0, 0.0, 1.0), None);
        let start = scrollbar_geometry(400.0, 800.0, 0.0, 1.0).unwrap();
        let end = scrollbar_geometry(400.0, 800.0, 400.0, 1.0).unwrap();
        assert!((start.height_percent - 50.0).abs() < f32::EPSILON);
        assert!(start.top_percent.abs() < f32::EPSILON);
        assert!((end.top_percent - 50.0).abs() < f32::EPSILON);
        assert!((end.max_offset - 400.0).abs() < f32::EPSILON);
    }
}
