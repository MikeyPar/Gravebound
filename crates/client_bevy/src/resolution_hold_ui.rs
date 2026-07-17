//! Reusable native presentation projection for blocking Resolution Hold recovery.
//!
//! The projection contains no transport or mutation authority. It renders only validated server
//! stacks and emits semantic commands for the owning Hall controller.

use std::fmt::Write as _;

use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    window::{PrimaryWindow, WindowResized},
};
use content_schema::ProductionItemTemplatePayload;
use protocol::{
    ResolutionHoldDestinationV1, ResolutionHoldItemKindV1, ResolutionHoldRejectionCodeV1,
};
use sim_content::CompiledProductionItemCatalog;
use thiserror::Error;

use crate::death_view_ui::{DEATH_FONT_BOLD_PATH, DEATH_FONT_REGULAR_PATH};
use crate::resolution_hold::{
    ResolutionHoldClientFailure, ResolutionHoldClientModel, ResolutionHoldClientPhase,
    ResolutionHoldRetryDirective,
};

const RESOLUTION_HOLD_ICON_RUNTIME_PATH: &str = "core/items/core_item_icons.runtime.png";

pub const RESOLUTION_HOLD_MIN_UI_SCALE_PERCENT: u16 = 80;
pub const RESOLUTION_HOLD_MAX_UI_SCALE_PERCENT: u16 = 150;
pub const RESOLUTION_HOLD_MIN_VIEW_WIDTH: f32 = 1_280.0;
pub const RESOLUTION_HOLD_MIN_VIEW_HEIGHT: f32 = 720.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionHoldUiLayoutMode {
    Compact,
    Reference,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolutionHoldUiMetrics {
    pub layout_mode: ResolutionHoldUiLayoutMode,
    pub safe_margin_px: f32,
    pub title_text_px: f32,
    pub body_text_px: f32,
    pub label_text_px: f32,
    pub icon_size_px: f32,
}

impl ResolutionHoldUiMetrics {
    pub fn for_viewport(
        width: f32,
        height: f32,
        ui_scale_percent: u16,
    ) -> Result<Self, ResolutionHoldUiError> {
        if !width.is_finite()
            || !height.is_finite()
            || width < RESOLUTION_HOLD_MIN_VIEW_WIDTH
            || height < RESOLUTION_HOLD_MIN_VIEW_HEIGHT
            || !(RESOLUTION_HOLD_MIN_UI_SCALE_PERCENT..=RESOLUTION_HOLD_MAX_UI_SCALE_PERCENT)
                .contains(&ui_scale_percent)
        {
            return Err(ResolutionHoldUiError::InvalidLayout);
        }
        let scale = f32::from(ui_scale_percent) / 100.0;
        let compact = height < 900.0 || ui_scale_percent > 120;
        Ok(Self {
            layout_mode: if compact {
                ResolutionHoldUiLayoutMode::Compact
            } else {
                ResolutionHoldUiLayoutMode::Reference
            },
            safe_margin_px: (24.0 * scale).clamp(16.0, 36.0),
            title_text_px: (28.0 * scale).clamp(22.0, 38.0),
            body_text_px: (16.0 * scale).clamp(14.0, 24.0),
            label_text_px: (14.0 * scale).clamp(14.0, 21.0),
            icon_size_px: (56.0 * scale).clamp(48.0, 78.0),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolutionHoldUiConfig {
    pub reduced_effects: bool,
    pub ui_scale_percent: u16,
}

impl Default for ResolutionHoldUiConfig {
    fn default() -> Self {
        Self {
            reduced_effects: false,
            ui_scale_percent: 100,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionHoldUiCopy {
    pub eyebrow: String,
    pub title: String,
    pub explanation: String,
    pub safety_notice: String,
    pub no_destination: String,
    pub move_action: String,
    pub destroy_review_action: String,
    pub cancel_action: String,
    pub confirm_destroy_action: String,
    pub retry_action: String,
}

impl Default for ResolutionHoldUiCopy {
    fn default() -> Self {
        Self {
            eyebrow: "LANTERN HALLS  /  SECURE CUSTODY".to_owned(),
            title: "STORAGE RESOLUTION REQUIRED".to_owned(),
            explanation: "Your extraction succeeded. These accepted items are safe, but each held stack must be moved to legal storage or explicitly destroyed before you can enter danger or change inventory.".to_owned(),
            safety_notice: "Nothing here expires. Moving uses the server-planned destination. Permanent destruction grants no Ash, salvage, materials, replacement, or other benefit.".to_owned(),
            no_destination: "No legal storage is currently available.".to_owned(),
            move_action: "MOVE WHOLE STACK".to_owned(),
            destroy_review_action: "DESTROY PERMANENTLY".to_owned(),
            cancel_action: "CANCEL — KEEP ITEM".to_owned(),
            confirm_destroy_action: "DESTROY COMPLETE STACK".to_owned(),
            retry_action: "RETRY".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionHoldUiTone {
    Neutral,
    Progress,
    Warning,
    Failure,
    Success,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionHoldUiStatus {
    pub title: String,
    pub detail: String,
    pub tone: ResolutionHoldUiTone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionHoldUiAction {
    Select {
        extraction_id: [u8; 16],
        stack_index: u8,
    },
    Move,
    RequestDestroy,
    CancelDestroy,
    ConfirmDestroy,
    Retry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionHoldUiActionEmphasis {
    Primary,
    Secondary,
    Destructive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionHoldUiActionSpec {
    pub action: ResolutionHoldUiAction,
    pub label: String,
    pub enabled: bool,
    pub emphasis: ResolutionHoldUiActionEmphasis,
    pub default_focus: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionHoldUiEntry {
    pub extraction_id: [u8; 16],
    pub stack_index: u8,
    pub icon_index: usize,
    pub localized_name: String,
    pub kind_label: String,
    pub quantity: u8,
    pub durable_uids: Vec<String>,
    pub destination_label: String,
    pub overflow_deadline_utc: String,
    pub can_move: bool,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionHoldDestroyReview {
    pub localized_name: String,
    pub quantity: u8,
    pub warning: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionHoldUiSnapshot {
    pub phase: ResolutionHoldClientPhase,
    pub copy: ResolutionHoldUiCopy,
    pub entries: Vec<ResolutionHoldUiEntry>,
    pub status: Option<ResolutionHoldUiStatus>,
    pub actions: Vec<ResolutionHoldUiActionSpec>,
    pub destroy_review: Option<ResolutionHoldDestroyReview>,
}

impl ResolutionHoldUiSnapshot {
    pub fn from_model(
        model: &ResolutionHoldClientModel,
        catalog: &CompiledProductionItemCatalog,
        copy: ResolutionHoldUiCopy,
    ) -> Result<Self, ResolutionHoldUiError> {
        if matches!(
            model.phase(),
            ResolutionHoldClientPhase::Dormant | ResolutionHoldClientPhase::Resolved
        ) {
            return Err(ResolutionHoldUiError::SurfaceNotOpen);
        }
        let selected_key = model
            .selected_stack()
            .map(|stack| (stack.extraction_id, stack.stack_index));
        let mut entries = Vec::with_capacity(model.stacks().len());
        for stack in model.stacks() {
            if stack.content_revision.as_str() != catalog.revision_label() {
                return Err(ResolutionHoldUiError::ContentRevisionMismatch);
            }
            let template = catalog
                .items()
                .get(stack.template_id.as_str())
                .ok_or(ResolutionHoldUiError::MissingItemContent)?;
            let content_kind = match template.payload {
                ProductionItemTemplatePayload::Equipment { .. } => {
                    ResolutionHoldItemKindV1::Equipment
                }
                ProductionItemTemplatePayload::Consumable { .. } => {
                    ResolutionHoldItemKindV1::Consumable
                }
                ProductionItemTemplatePayload::Material { .. } => {
                    return Err(ResolutionHoldUiError::ItemKindMismatch);
                }
            };
            if content_kind != stack.item_kind {
                return Err(ResolutionHoldUiError::ItemKindMismatch);
            }
            let localized_name = catalog
                .localized_item_name(stack.template_id.as_str())
                .ok_or(ResolutionHoldUiError::MissingItemContent)?
                .to_owned();
            let icon_index = catalog
                .items()
                .keys()
                .position(|item_id| item_id == stack.template_id.as_str())
                .ok_or(ResolutionHoldUiError::MissingItemContent)?;
            let (destination_label, can_move) = destination_copy(stack.planned_destination, &copy);
            entries.push(ResolutionHoldUiEntry {
                extraction_id: stack.extraction_id,
                stack_index: stack.stack_index,
                icon_index,
                localized_name,
                kind_label: match stack.item_kind {
                    ResolutionHoldItemKindV1::Equipment => "EQUIPMENT".to_owned(),
                    ResolutionHoldItemKindV1::Consumable => "CONSUMABLE".to_owned(),
                },
                quantity: u8::try_from(stack.items.len())
                    .expect("validated Hold item count fits u8"),
                durable_uids: stack
                    .items
                    .iter()
                    .map(|item| format_uid(item.item_uid))
                    .collect(),
                destination_label,
                overflow_deadline_utc: format_unix_millis_utc(stack.overflow_deadline_unix_millis)?,
                can_move,
                selected: selected_key == Some((stack.extraction_id, stack.stack_index)),
            });
        }
        let status = status_for_model(model);
        let destroy_review = if model.phase() == ResolutionHoldClientPhase::ConfirmDestroy {
            let selected = entries
                .iter()
                .find(|entry| entry.selected)
                .ok_or(ResolutionHoldUiError::MissingSelectedStack)?;
            Some(ResolutionHoldDestroyReview {
                localized_name: selected.localized_name.clone(),
                quantity: selected.quantity,
                warning: format!(
                    "Permanently destroy all {} × {}? This cannot be undone and grants no benefit.",
                    selected.quantity, selected.localized_name
                ),
            })
        } else {
            None
        };
        let actions = action_specs(model, &entries, &copy);
        Ok(Self {
            phase: model.phase(),
            copy,
            entries,
            status,
            actions,
            destroy_review,
        })
    }

    #[must_use]
    pub fn selected_entry(&self) -> Option<&ResolutionHoldUiEntry> {
        self.entries.iter().find(|entry| entry.selected)
    }

    #[must_use]
    pub fn escape_action(&self) -> Option<ResolutionHoldUiAction> {
        (self.phase == ResolutionHoldClientPhase::ConfirmDestroy)
            .then_some(ResolutionHoldUiAction::CancelDestroy)
    }
}

#[derive(Debug, Clone, Resource)]
pub struct NativeResolutionHoldView {
    snapshot: ResolutionHoldUiSnapshot,
    config: ResolutionHoldUiConfig,
    layout_epoch: u64,
}

impl NativeResolutionHoldView {
    pub fn new(
        snapshot: ResolutionHoldUiSnapshot,
        config: ResolutionHoldUiConfig,
    ) -> Result<Self, ResolutionHoldUiError> {
        if !(RESOLUTION_HOLD_MIN_UI_SCALE_PERCENT..=RESOLUTION_HOLD_MAX_UI_SCALE_PERCENT)
            .contains(&config.ui_scale_percent)
        {
            return Err(ResolutionHoldUiError::InvalidLayout);
        }
        Ok(Self {
            snapshot,
            config,
            layout_epoch: 0,
        })
    }

    #[must_use]
    pub const fn snapshot(&self) -> &ResolutionHoldUiSnapshot {
        &self.snapshot
    }

    pub fn replace_snapshot(&mut self, snapshot: ResolutionHoldUiSnapshot) {
        self.snapshot = snapshot;
        self.layout_epoch = self.layout_epoch.saturating_add(1);
    }
}

#[derive(Debug, Clone, Copy, Message, PartialEq, Eq)]
pub struct ResolutionHoldUiCommand(pub ResolutionHoldUiAction);

#[derive(Debug, Default, Resource)]
pub struct ResolutionHoldUiFocusState {
    focused_order: Option<u16>,
    ensure_visible: bool,
}

impl ResolutionHoldUiFocusState {
    #[must_use]
    pub const fn focused_order(&self) -> Option<u16> {
        self.focused_order
    }
}

#[derive(Debug, Default, Clone, Copy, Resource, PartialEq)]
pub struct ResolutionHoldUiScrollState {
    offset: f32,
    max_offset: f32,
}

impl ResolutionHoldUiScrollState {
    #[must_use]
    pub fn has_overflow(self) -> bool {
        self.max_offset > 0.5
    }

    #[must_use]
    pub const fn offset(self) -> f32 {
        self.offset
    }

    #[must_use]
    pub const fn max_offset(self) -> f32 {
        self.max_offset
    }
}

#[derive(Debug, Resource)]
struct ResolutionHoldUiFonts {
    regular: Handle<Font>,
    bold: Handle<Font>,
}

impl FromWorld for ResolutionHoldUiFonts {
    fn from_world(world: &mut World) -> Self {
        let assets = world.resource::<AssetServer>();
        Self {
            regular: assets.load(DEATH_FONT_REGULAR_PATH),
            bold: assets.load(DEATH_FONT_BOLD_PATH),
        }
    }
}

#[derive(Debug, Component)]
struct ResolutionHoldUiRoot;

#[derive(Debug, Component)]
struct ResolutionHoldUiScrollRoot;

#[derive(Debug, Component)]
struct ResolutionHoldUiFocusMarker {
    order: u16,
}

#[derive(Debug, Clone, Component)]
struct ResolutionHoldUiButton {
    action: ResolutionHoldUiAction,
    enabled: bool,
    emphasis: ResolutionHoldUiActionEmphasis,
    order: u16,
    initial_focus: ResolutionHoldUiInitialFocus,
    role: ResolutionHoldUiButtonRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolutionHoldUiInitialFocus {
    Ordinary,
    Default,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolutionHoldUiButtonRole {
    StackRow { selected: bool },
    Action,
}

#[derive(Debug, Clone, Copy, Component, PartialEq, Eq)]
pub struct ResolutionHoldUiFocusOrder(pub u16);

pub struct NativeResolutionHoldPlugin;

impl Plugin for NativeResolutionHoldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ResolutionHoldUiFocusState>()
            .init_resource::<ResolutionHoldUiScrollState>()
            .init_resource::<ResolutionHoldUiFonts>()
            .add_message::<ResolutionHoldUiCommand>()
            .add_systems(
                Update,
                (
                    track_resolution_hold_window_resize,
                    rebuild_native_resolution_hold,
                    handle_resolution_hold_focus_and_activation,
                    scroll_resolution_hold_list,
                    keep_focused_resolution_hold_row_visible,
                    update_resolution_hold_scroll_state,
                    update_resolution_hold_focus_markers,
                    style_resolution_hold_buttons,
                )
                    .chain(),
            );
    }
}

#[allow(clippy::needless_pass_by_value)]
fn track_resolution_hold_window_resize(
    mut resized: MessageReader<WindowResized>,
    view: Option<ResMut<NativeResolutionHoldView>>,
) {
    if resized.read().next().is_some()
        && let Some(mut view) = view
    {
        view.layout_epoch = view.layout_epoch.saturating_add(1);
    }
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn rebuild_native_resolution_hold(
    mut commands: Commands,
    view: Option<Res<NativeResolutionHoldView>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    roots: Query<Entity, With<ResolutionHoldUiRoot>>,
    assets: Res<AssetServer>,
    mut atlases: ResMut<Assets<TextureAtlasLayout>>,
    fonts: Res<ResolutionHoldUiFonts>,
    mut focus: ResMut<ResolutionHoldUiFocusState>,
) {
    let Some(view) = view else {
        for entity in &roots {
            commands.entity(entity).despawn();
        }
        return;
    };
    if !view.is_changed() && !roots.is_empty() {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok(metrics) = ResolutionHoldUiMetrics::for_viewport(
        window.resolution.width(),
        window.resolution.height(),
        view.config.ui_scale_percent,
    ) else {
        return;
    };
    for entity in &roots {
        commands.entity(entity).despawn();
    }
    let texture = assets.load(RESOLUTION_HOLD_ICON_RUNTIME_PATH);
    let atlas = atlases.add(TextureAtlasLayout::from_grid(
        UVec2::splat(64),
        6,
        3,
        None,
        None,
    ));
    focus.focused_order = None;
    focus.ensure_visible = false;
    spawn_native_resolution_hold(
        &mut commands,
        &view.snapshot,
        view.config,
        metrics,
        &texture,
        &atlas,
        &fonts,
    );
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn spawn_native_resolution_hold(
    commands: &mut Commands,
    snapshot: &ResolutionHoldUiSnapshot,
    config: ResolutionHoldUiConfig,
    metrics: ResolutionHoldUiMetrics,
    texture: &Handle<Image>,
    atlas: &Handle<TextureAtlasLayout>,
    fonts: &ResolutionHoldUiFonts,
) {
    commands
        .spawn((
            Name::new("Blocking Resolution Hold recovery surface"),
            ResolutionHoldUiRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                padding: UiRect::all(px(metrics.safe_margin_px)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba_u8(2, 4, 5, 238)),
            GlobalZIndex(90),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: percent(100),
                    max_width: px(1_180),
                    height: percent(100),
                    max_height: px(1_000),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(if metrics.layout_mode
                        == ResolutionHoldUiLayoutMode::Compact
                    {
                        16
                    } else {
                        22
                    })),
                    row_gap: px(if metrics.layout_mode == ResolutionHoldUiLayoutMode::Compact {
                        8
                    } else {
                        12
                    }),
                    border: UiRect::all(px(2)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(Color::srgba_u8(10, 14, 16, 252)),
                BorderColor::all(Color::srgb_u8(178, 139, 73)),
                BoxShadow::new(Color::srgba_u8(0, 0, 0, 180), px(0), px(10), px(0), px(28)),
            ))
            .with_children(|panel| {
                spawn_hold_text(
                    panel,
                    &snapshot.copy.eyebrow,
                    metrics.label_text_px,
                    Color::srgb_u8(135, 173, 161),
                    false,
                    fonts,
                );
                panel
                    .spawn(Node {
                        width: percent(100),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::SpaceBetween,
                        column_gap: px(12),
                        ..default()
                    })
                    .with_children(|header| {
                        spawn_hold_text(
                            header,
                            &snapshot.copy.title,
                            metrics.title_text_px,
                            Color::srgb_u8(242, 222, 171),
                            true,
                            fonts,
                        );
                        spawn_hold_badge(
                            header,
                            &format!("{} HELD", snapshot.entries.len()),
                            metrics,
                            fonts,
                        );
                    });
                spawn_hold_text(
                    panel,
                    &snapshot.copy.explanation,
                    metrics.body_text_px,
                    Color::srgb_u8(211, 210, 195),
                    false,
                    fonts,
                );
                if let Some(status) = snapshot.status.as_ref() {
                    spawn_hold_status(panel, status, metrics, fonts);
                }
                panel
                    .spawn(Node {
                        width: percent(100),
                        min_height: px(if metrics.layout_mode
                            == ResolutionHoldUiLayoutMode::Compact
                        {
                            180
                        } else {
                            300
                        }),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Row,
                        column_gap: px(12),
                        overflow: Overflow::clip(),
                        ..default()
                    })
                    .with_children(|content| {
                        spawn_hold_stack_list(
                            content, snapshot, metrics, texture, atlas, fonts,
                        );
                        spawn_hold_detail(content, snapshot, metrics, fonts);
                    });
                spawn_hold_actions(panel, snapshot, metrics, fonts);
                spawn_hold_text(
                    panel,
                    &snapshot.copy.safety_notice,
                    metrics.label_text_px,
                    Color::srgb_u8(158, 157, 145),
                    false,
                    fonts,
                );
                spawn_hold_text(
                    panel,
                    if config.reduced_effects {
                        "REDUCED EFFECTS  ·  TAB / D-PAD NAVIGATE  ·  ENTER / A ACTIVATE  ·  ESC / B CANCELS REVIEW ONLY"
                    } else {
                        "STANDARD EFFECTS  ·  TAB / D-PAD NAVIGATE  ·  ENTER / A ACTIVATE  ·  ESC / B CANCELS REVIEW ONLY"
                    },
                    metrics.label_text_px,
                    Color::srgb_u8(106, 133, 128),
                    false,
                    fonts,
                );
            });
        });
}

fn spawn_hold_badge(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    metrics: ResolutionHoldUiMetrics,
    fonts: &ResolutionHoldUiFonts,
) {
    parent
        .spawn((
            Node {
                padding: UiRect::axes(px(12), px(6)),
                border: UiRect::all(px(1)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(Color::srgb_u8(28, 34, 32)),
            BorderColor::all(Color::srgb_u8(101, 127, 116)),
        ))
        .with_children(|badge| {
            spawn_hold_text(
                badge,
                label,
                metrics.label_text_px,
                Color::srgb_u8(191, 211, 200),
                true,
                fonts,
            );
        });
}

fn spawn_hold_status(
    parent: &mut ChildSpawnerCommands,
    status: &ResolutionHoldUiStatus,
    metrics: ResolutionHoldUiMetrics,
    fonts: &ResolutionHoldUiFonts,
) {
    let (background, border, title_color) = match status.tone {
        ResolutionHoldUiTone::Neutral => (
            Color::srgb_u8(20, 25, 25),
            Color::srgb_u8(67, 78, 75),
            Color::srgb_u8(207, 210, 198),
        ),
        ResolutionHoldUiTone::Progress => (
            Color::srgb_u8(17, 29, 29),
            Color::srgb_u8(66, 116, 107),
            Color::srgb_u8(160, 213, 199),
        ),
        ResolutionHoldUiTone::Warning => (
            Color::srgb_u8(39, 27, 18),
            Color::srgb_u8(164, 113, 55),
            Color::srgb_u8(240, 190, 118),
        ),
        ResolutionHoldUiTone::Failure => (
            Color::srgb_u8(41, 20, 19),
            Color::srgb_u8(151, 70, 61),
            Color::srgb_u8(237, 157, 142),
        ),
        ResolutionHoldUiTone::Success => (
            Color::srgb_u8(18, 31, 25),
            Color::srgb_u8(72, 123, 91),
            Color::srgb_u8(162, 214, 177),
        ),
    };
    parent
        .spawn((
            Node {
                width: percent(100),
                padding: UiRect::axes(px(14), px(9)),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(3),
                border: UiRect::left(px(3)),
                ..default()
            },
            BackgroundColor(background),
            BorderColor::all(border),
        ))
        .with_children(|card| {
            spawn_hold_text(
                card,
                &status.title,
                metrics.body_text_px,
                title_color,
                true,
                fonts,
            );
            spawn_hold_text(
                card,
                &status.detail,
                metrics.label_text_px,
                Color::srgb_u8(190, 190, 177),
                false,
                fonts,
            );
        });
}

fn spawn_hold_stack_list(
    parent: &mut ChildSpawnerCommands,
    snapshot: &ResolutionHoldUiSnapshot,
    metrics: ResolutionHoldUiMetrics,
    texture: &Handle<Image>,
    atlas: &Handle<TextureAtlasLayout>,
    fonts: &ResolutionHoldUiFonts,
) {
    parent
        .spawn((
            Node {
                width: percent(44),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(10)),
                row_gap: px(8),
                border: UiRect::all(px(1)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(13, 17, 18)),
            BorderColor::all(Color::srgb_u8(55, 68, 65)),
        ))
        .with_children(|list| {
            spawn_hold_text(
                list,
                "HELD STACKS",
                metrics.label_text_px,
                Color::srgb_u8(154, 174, 166),
                true,
                fonts,
            );
            list.spawn((
                ResolutionHoldUiScrollRoot,
                ScrollPosition::default(),
                Node {
                    width: percent(100),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(7),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
            ))
            .with_children(|rows| {
                for (index, entry) in snapshot.entries.iter().enumerate() {
                    let order =
                        u16::try_from(index + 1).expect("Hold stack count fits focus order");
                    spawn_hold_stack_row(
                        rows, snapshot, entry, order, metrics, texture, atlas, fonts,
                    );
                }
            });
        });
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn spawn_hold_stack_row(
    parent: &mut ChildSpawnerCommands,
    snapshot: &ResolutionHoldUiSnapshot,
    entry: &ResolutionHoldUiEntry,
    order: u16,
    metrics: ResolutionHoldUiMetrics,
    texture: &Handle<Image>,
    atlas: &Handle<TextureAtlasLayout>,
    fonts: &ResolutionHoldUiFonts,
) {
    let enabled = snapshot.phase == ResolutionHoldClientPhase::Ready;
    parent
        .spawn((
            Button,
            ResolutionHoldUiButton {
                action: ResolutionHoldUiAction::Select {
                    extraction_id: entry.extraction_id,
                    stack_index: entry.stack_index,
                },
                enabled,
                emphasis: ResolutionHoldUiActionEmphasis::Secondary,
                order,
                initial_focus: if enabled && entry.selected {
                    ResolutionHoldUiInitialFocus::Default
                } else {
                    ResolutionHoldUiInitialFocus::Ordinary
                },
                role: ResolutionHoldUiButtonRole::StackRow {
                    selected: entry.selected,
                },
            },
            ResolutionHoldUiFocusOrder(order),
            AccessibleLabel::new(format!(
                "{} quantity {}, destination {}",
                entry.localized_name, entry.quantity, entry.destination_label
            )),
            Node {
                width: percent(100),
                min_height: px(metrics.icon_size_px + 16.0),
                padding: UiRect::all(px(8)),
                align_items: AlignItems::Center,
                column_gap: px(10),
                border: UiRect::all(px(if entry.selected { 2 } else { 1 })),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(if entry.selected {
                Color::srgb_u8(29, 39, 36)
            } else {
                Color::srgb_u8(19, 24, 24)
            }),
            BorderColor::all(if entry.selected {
                Color::srgb_u8(190, 151, 78)
            } else {
                Color::srgb_u8(62, 75, 71)
            }),
        ))
        .with_children(|row| {
            spawn_hold_focus_marker(row, order, metrics, fonts);
            row.spawn((
                ImageNode::from_atlas_image(
                    texture.clone(),
                    TextureAtlas {
                        layout: atlas.clone(),
                        index: entry.icon_index,
                    },
                ),
                Node {
                    width: px(metrics.icon_size_px),
                    height: px(metrics.icon_size_px),
                    border: UiRect::all(px(1)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BorderColor::all(Color::srgb_u8(150, 121, 70)),
            ));
            row.spawn(Node {
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(2),
                ..default()
            })
            .with_children(|copy| {
                spawn_hold_text(
                    copy,
                    &format!("{}  ×{}", entry.localized_name, entry.quantity),
                    metrics.body_text_px,
                    Color::srgb_u8(232, 222, 195),
                    true,
                    fonts,
                );
                spawn_hold_text(
                    copy,
                    &entry.kind_label,
                    metrics.label_text_px,
                    Color::srgb_u8(132, 165, 155),
                    false,
                    fonts,
                );
                spawn_hold_text(
                    copy,
                    &entry.destination_label,
                    metrics.label_text_px,
                    if entry.can_move {
                        Color::srgb_u8(179, 194, 173)
                    } else {
                        Color::srgb_u8(224, 153, 117)
                    },
                    false,
                    fonts,
                );
            });
        });
}

fn spawn_hold_detail(
    parent: &mut ChildSpawnerCommands,
    snapshot: &ResolutionHoldUiSnapshot,
    metrics: ResolutionHoldUiMetrics,
    fonts: &ResolutionHoldUiFonts,
) {
    parent
        .spawn((
            Node {
                width: percent(56),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(16)),
                row_gap: px(9),
                border: UiRect::all(px(1)),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ScrollPosition::default(),
            BackgroundColor(Color::srgb_u8(15, 19, 20)),
            BorderColor::all(Color::srgb_u8(58, 67, 65)),
        ))
        .with_children(|detail| {
            if let Some(review) = snapshot.destroy_review.as_ref() {
                spawn_destroy_review_detail(detail, review, metrics, fonts);
                return;
            }
            let Some(entry) = snapshot.selected_entry() else {
                spawn_hold_text(
                    detail,
                    "WAITING FOR AUTHORITY",
                    metrics.body_text_px,
                    Color::srgb_u8(164, 179, 172),
                    true,
                    fonts,
                );
                return;
            };
            spawn_selected_stack_detail(detail, entry, metrics, fonts);
        });
}

fn spawn_destroy_review_detail(
    parent: &mut ChildSpawnerCommands,
    review: &ResolutionHoldDestroyReview,
    metrics: ResolutionHoldUiMetrics,
    fonts: &ResolutionHoldUiFonts,
) {
    spawn_hold_text(
        parent,
        "PERMANENT DESTRUCTION REVIEW",
        metrics.label_text_px,
        Color::srgb_u8(224, 154, 116),
        true,
        fonts,
    );
    spawn_hold_text(
        parent,
        &format!("{}  ×{}", review.localized_name, review.quantity),
        metrics.title_text_px * 0.8,
        Color::srgb_u8(242, 218, 180),
        true,
        fonts,
    );
    spawn_hold_text(
        parent,
        &review.warning,
        metrics.body_text_px,
        Color::srgb_u8(233, 177, 143),
        false,
        fonts,
    );
    spawn_hold_text(
        parent,
        "Default: CANCEL — KEEP ITEM",
        metrics.body_text_px,
        Color::srgb_u8(165, 211, 190),
        true,
        fonts,
    );
}

fn spawn_selected_stack_detail(
    parent: &mut ChildSpawnerCommands,
    entry: &ResolutionHoldUiEntry,
    metrics: ResolutionHoldUiMetrics,
    fonts: &ResolutionHoldUiFonts,
) {
    spawn_hold_text(
        parent,
        "SELECTED STACK",
        metrics.label_text_px,
        Color::srgb_u8(137, 167, 157),
        true,
        fonts,
    );
    spawn_hold_text(
        parent,
        &format!("{}  ×{}", entry.localized_name, entry.quantity),
        metrics.title_text_px * 0.8,
        Color::srgb_u8(240, 224, 184),
        true,
        fonts,
    );
    spawn_detail_field(
        parent,
        "SERVER-PLANNED DESTINATION",
        &entry.destination_label,
        metrics,
        fonts,
    );
    spawn_detail_field(
        parent,
        "ORIGINAL OVERFLOW DEADLINE",
        &entry.overflow_deadline_utc,
        metrics,
        fonts,
    );
    spawn_hold_text(
        parent,
        "DURABLE ITEM IDENTITY",
        metrics.label_text_px,
        Color::srgb_u8(126, 152, 144),
        true,
        fonts,
    );
    for uid in &entry.durable_uids {
        spawn_hold_text(
            parent,
            uid,
            metrics.label_text_px,
            Color::srgb_u8(186, 188, 176),
            false,
            fonts,
        );
    }
}

fn spawn_detail_field(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    value: &str,
    metrics: ResolutionHoldUiMetrics,
    fonts: &ResolutionHoldUiFonts,
) {
    spawn_hold_text(
        parent,
        label,
        metrics.label_text_px,
        Color::srgb_u8(126, 152, 144),
        true,
        fonts,
    );
    spawn_hold_text(
        parent,
        value,
        metrics.body_text_px,
        Color::srgb_u8(216, 213, 194),
        false,
        fonts,
    );
}

fn spawn_hold_actions(
    parent: &mut ChildSpawnerCommands,
    snapshot: &ResolutionHoldUiSnapshot,
    metrics: ResolutionHoldUiMetrics,
    fonts: &ResolutionHoldUiFonts,
) {
    if snapshot.actions.is_empty() {
        return;
    }
    parent
        .spawn(Node {
            width: percent(100),
            min_height: px(52),
            flex_shrink: 0.0,
            flex_wrap: FlexWrap::Wrap,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::End,
            column_gap: px(10),
            row_gap: px(8),
            ..default()
        })
        .with_children(|actions| {
            for (index, action) in snapshot.actions.iter().enumerate() {
                let order = 100_u16
                    .checked_add(u16::try_from(index).expect("Hold action count fits focus order"))
                    .expect("Hold focus order does not overflow");
                spawn_hold_action_button(actions, action, order, metrics, fonts);
            }
        });
}

fn spawn_hold_action_button(
    parent: &mut ChildSpawnerCommands,
    action: &ResolutionHoldUiActionSpec,
    order: u16,
    metrics: ResolutionHoldUiMetrics,
    fonts: &ResolutionHoldUiFonts,
) {
    let destructive = action.emphasis == ResolutionHoldUiActionEmphasis::Destructive;
    parent
        .spawn((
            Button,
            ResolutionHoldUiButton {
                action: action.action,
                enabled: action.enabled,
                emphasis: action.emphasis,
                order,
                initial_focus: if action.default_focus {
                    ResolutionHoldUiInitialFocus::Default
                } else {
                    ResolutionHoldUiInitialFocus::Ordinary
                },
                role: ResolutionHoldUiButtonRole::Action,
            },
            ResolutionHoldUiFocusOrder(order),
            AccessibleLabel::new(action.label.clone()),
            Node {
                min_width: px(if destructive { 230 } else { 210 }),
                min_height: px(48),
                column_gap: px(7),
                padding: UiRect::axes(px(18), px(10)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(
                    if action.emphasis == ResolutionHoldUiActionEmphasis::Primary {
                        2
                    } else {
                        1
                    },
                )),
                ..default()
            },
            BackgroundColor(action_background(action.emphasis, action.enabled)),
            BorderColor::all(action_border(action.emphasis, action.enabled)),
        ))
        .with_children(|button| {
            spawn_hold_focus_marker(button, order, metrics, fonts);
            spawn_hold_text(
                button,
                &action.label,
                metrics.body_text_px,
                if action.enabled {
                    Color::srgb_u8(238, 225, 194)
                } else {
                    Color::srgb_u8(112, 112, 105)
                },
                true,
                fonts,
            );
        });
}

fn spawn_hold_focus_marker(
    parent: &mut ChildSpawnerCommands,
    order: u16,
    metrics: ResolutionHoldUiMetrics,
    fonts: &ResolutionHoldUiFonts,
) {
    parent.spawn((
        ResolutionHoldUiFocusMarker { order },
        Text::new("▶"),
        TextFont {
            font: FontSource::Handle(fonts.bold.clone()),
            font_size: FontSize::Px(metrics.label_text_px),
            ..default()
        },
        TextColor(Color::srgb_u8(244, 224, 164)),
        Visibility::Hidden,
    ));
}

fn spawn_hold_text(
    parent: &mut ChildSpawnerCommands,
    value: &str,
    size: f32,
    color: Color,
    bold: bool,
    fonts: &ResolutionHoldUiFonts,
) {
    parent.spawn((
        Text::new(value),
        TextFont {
            font: FontSource::Handle(if bold {
                fonts.bold.clone()
            } else {
                fonts.regular.clone()
            }),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
        Node {
            flex_shrink: 0.0,
            ..default()
        },
    ));
}

type HoldButtonInteractions<'w, 's> = Query<
    'w,
    's,
    (&'static Interaction, &'static ResolutionHoldUiButton),
    (Changed<Interaction>, With<Button>),
>;

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn handle_resolution_hold_focus_and_activation(
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    view: Option<Res<NativeResolutionHoldView>>,
    mut changed_buttons: HoldButtonInteractions,
    all_buttons: Query<&ResolutionHoldUiButton, With<Button>>,
    mut focus: ResMut<ResolutionHoldUiFocusState>,
    mut commands: MessageWriter<ResolutionHoldUiCommand>,
) {
    let Some(view) = view else {
        return;
    };
    let mut ordered = all_buttons.iter().cloned().collect::<Vec<_>>();
    ordered.sort_by_key(|button| button.order);
    if focus.focused_order.is_none() {
        focus.focused_order = ordered
            .iter()
            .find(|button| {
                button.enabled && button.initial_focus == ResolutionHoldUiInitialFocus::Default
            })
            .or_else(|| ordered.iter().find(|button| button.enabled))
            .map(|button| button.order);
    }
    for (interaction, button) in &mut changed_buttons {
        if button.enabled && matches!(interaction, Interaction::Hovered | Interaction::Pressed) {
            focus.focused_order = Some(button.order);
        }
        if button.enabled && *interaction == Interaction::Pressed {
            commands.write(ResolutionHoldUiCommand(button.action));
        }
    }
    let gamepad_pressed = |button| gamepads.iter().any(|pad| pad.just_pressed(button));
    let previous = (keyboard.just_pressed(KeyCode::Tab)
        && (keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight)))
        || keyboard.just_pressed(KeyCode::ArrowLeft)
        || keyboard.just_pressed(KeyCode::ArrowUp)
        || gamepad_pressed(GamepadButton::DPadLeft)
        || gamepad_pressed(GamepadButton::DPadUp);
    let next = (keyboard.just_pressed(KeyCode::Tab)
        && !keyboard.pressed(KeyCode::ShiftLeft)
        && !keyboard.pressed(KeyCode::ShiftRight))
        || keyboard.just_pressed(KeyCode::ArrowRight)
        || keyboard.just_pressed(KeyCode::ArrowDown)
        || gamepad_pressed(GamepadButton::DPadRight)
        || gamepad_pressed(GamepadButton::DPadDown);
    if previous || next {
        let next_order =
            next_hold_focus_order(&ordered, focus.focused_order, if previous { -1 } else { 1 });
        if focus.focused_order != next_order {
            focus.focused_order = next_order;
            focus.ensure_visible = next_order.is_some();
        }
    }
    let activate = keyboard.just_pressed(KeyCode::Enter)
        || keyboard.just_pressed(KeyCode::Space)
        || gamepad_pressed(GamepadButton::South);
    if activate
        && let Some(button) = ordered
            .iter()
            .find(|button| button.enabled && Some(button.order) == focus.focused_order)
    {
        commands.write(ResolutionHoldUiCommand(button.action));
    }
    let back = keyboard.just_pressed(KeyCode::Escape) || gamepad_pressed(GamepadButton::East);
    if back && let Some(action) = view.snapshot.escape_action() {
        commands.write(ResolutionHoldUiCommand(action));
    }
}

fn next_hold_focus_order(
    ordered: &[ResolutionHoldUiButton],
    current: Option<u16>,
    direction: i8,
) -> Option<u16> {
    let enabled = ordered
        .iter()
        .filter(|button| button.enabled)
        .map(|button| button.order)
        .collect::<Vec<_>>();
    if enabled.is_empty() {
        return None;
    }
    let index = current
        .and_then(|order| enabled.iter().position(|candidate| *candidate == order))
        .unwrap_or(if direction < 0 { 0 } else { enabled.len() - 1 });
    let next = if direction < 0 {
        index.checked_sub(1).unwrap_or(enabled.len() - 1)
    } else {
        (index + 1) % enabled.len()
    };
    Some(enabled[next])
}

#[allow(clippy::needless_pass_by_value)]
fn scroll_resolution_hold_list(
    mut wheel: MessageReader<MouseWheel>,
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut roots: Query<(&mut ScrollPosition, &ComputedNode), With<ResolutionHoldUiScrollRoot>>,
) {
    let Ok((mut scroll, computed)) = roots.single_mut() else {
        return;
    };
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
        delta += 260.0;
    }
    if keyboard.just_pressed(KeyCode::PageUp)
        || gamepads
            .iter()
            .any(|gamepad| gamepad.just_pressed(GamepadButton::LeftTrigger))
    {
        delta -= 260.0;
    }
    let max_offset = ((computed.content_size() - computed.size())
        * computed.inverse_scale_factor())
    .y
    .max(0.0);
    scroll.y = (scroll.y + delta).clamp(0.0, max_offset);
}

#[allow(clippy::needless_pass_by_value, clippy::type_complexity)]
fn keep_focused_resolution_hold_row_visible(
    mut focus: ResMut<ResolutionHoldUiFocusState>,
    mut roots: Query<
        (&mut ScrollPosition, &ComputedNode, &UiGlobalTransform),
        With<ResolutionHoldUiScrollRoot>,
    >,
    buttons: Query<(&ResolutionHoldUiButton, &ComputedNode, &UiGlobalTransform), With<Button>>,
) {
    if !focus.ensure_visible {
        return;
    }
    focus.ensure_visible = false;
    let Some(order) = focus.focused_order else {
        return;
    };
    let Some((button, button_node, button_transform)) =
        buttons.iter().find(|(button, _, _)| button.order == order)
    else {
        return;
    };
    if !matches!(button.role, ResolutionHoldUiButtonRole::StackRow { .. }) {
        return;
    }
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
    scroll.y = scroll_offset_to_reveal_hold(
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
fn scroll_offset_to_reveal_hold(
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

#[allow(clippy::needless_pass_by_value)]
fn update_resolution_hold_scroll_state(
    roots: Query<(&ScrollPosition, &ComputedNode), With<ResolutionHoldUiScrollRoot>>,
    mut state: ResMut<ResolutionHoldUiScrollState>,
) {
    let Ok((scroll, computed)) = roots.single() else {
        *state = ResolutionHoldUiScrollState::default();
        return;
    };
    let max_offset = ((computed.content_size() - computed.size())
        * computed.inverse_scale_factor())
    .y
    .max(0.0);
    *state = ResolutionHoldUiScrollState {
        offset: scroll.y,
        max_offset,
    };
}

#[allow(clippy::needless_pass_by_value)]
fn update_resolution_hold_focus_markers(
    focus: Res<ResolutionHoldUiFocusState>,
    mut markers: Query<(&ResolutionHoldUiFocusMarker, &mut Visibility)>,
) {
    if !focus.is_changed() {
        return;
    }
    for (marker, mut visibility) in &mut markers {
        *visibility = if focus.focused_order == Some(marker.order) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

#[allow(clippy::needless_pass_by_value)]
fn style_resolution_hold_buttons(
    focus: Res<ResolutionHoldUiFocusState>,
    mut buttons: Query<
        (
            &Interaction,
            &ResolutionHoldUiButton,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<Button>,
    >,
) {
    for (interaction, button, mut background, mut border) in &mut buttons {
        let focused = focus.focused_order == Some(button.order) && button.enabled;
        if !button.enabled {
            background.0 = Color::srgb_u8(18, 20, 20);
            *border = BorderColor::all(Color::srgb_u8(51, 54, 52));
            continue;
        }
        let selected_row = matches!(
            button.role,
            ResolutionHoldUiButtonRole::StackRow { selected: true }
        );
        let base_background = if selected_row {
            Color::srgb_u8(29, 39, 36)
        } else {
            action_background(button.emphasis, true)
        };
        let base_border = if selected_row {
            Color::srgb_u8(190, 151, 78)
        } else {
            action_border(button.emphasis, true)
        };
        match interaction {
            Interaction::Pressed => {
                background.0 = Color::srgb_u8(55, 47, 31);
                *border = BorderColor::all(Color::srgb_u8(241, 218, 151));
            }
            Interaction::Hovered => {
                background.0 = Color::srgb_u8(36, 47, 43);
                *border = BorderColor::all(Color::srgb_u8(203, 176, 108));
            }
            Interaction::None => {
                background.0 = base_background;
                *border = BorderColor::all(if focused {
                    Color::srgb_u8(244, 224, 164)
                } else {
                    base_border
                });
            }
        }
    }
}

fn action_background(emphasis: ResolutionHoldUiActionEmphasis, enabled: bool) -> Color {
    if !enabled {
        return Color::srgb_u8(18, 20, 20);
    }
    match emphasis {
        ResolutionHoldUiActionEmphasis::Primary => Color::srgb_u8(46, 37, 24),
        ResolutionHoldUiActionEmphasis::Secondary => Color::srgb_u8(20, 27, 26),
        ResolutionHoldUiActionEmphasis::Destructive => Color::srgb_u8(43, 24, 21),
    }
}

fn action_border(emphasis: ResolutionHoldUiActionEmphasis, enabled: bool) -> Color {
    if !enabled {
        return Color::srgb_u8(51, 54, 52);
    }
    match emphasis {
        ResolutionHoldUiActionEmphasis::Primary => Color::srgb_u8(180, 142, 72),
        ResolutionHoldUiActionEmphasis::Secondary => Color::srgb_u8(65, 82, 77),
        ResolutionHoldUiActionEmphasis::Destructive => Color::srgb_u8(151, 69, 57),
    }
}

fn destination_copy(
    destination: Option<ResolutionHoldDestinationV1>,
    copy: &ResolutionHoldUiCopy,
) -> (String, bool) {
    let Some(destination) = destination else {
        return (copy.no_destination.clone(), false);
    };
    let label = match destination {
        ResolutionHoldDestinationV1::CharacterSafe { slot_index } => {
            format!("Character Safe · Slot {}", u16::from(slot_index) + 1)
        }
        ResolutionHoldDestinationV1::Vault { slot_index } => {
            format!("Vault · Slot {}", u32::from(slot_index) + 1)
        }
        ResolutionHoldDestinationV1::Overflow { slot_index } => {
            format!("Overflow Cache · Slot {}", u16::from(slot_index) + 1)
        }
    };
    (label, true)
}

fn action_specs(
    model: &ResolutionHoldClientModel,
    entries: &[ResolutionHoldUiEntry],
    copy: &ResolutionHoldUiCopy,
) -> Vec<ResolutionHoldUiActionSpec> {
    match model.phase() {
        ResolutionHoldClientPhase::Ready => vec![
            ResolutionHoldUiActionSpec {
                action: ResolutionHoldUiAction::Move,
                label: copy.move_action.clone(),
                enabled: entries
                    .iter()
                    .find(|entry| entry.selected)
                    .is_some_and(|entry| entry.can_move),
                emphasis: ResolutionHoldUiActionEmphasis::Primary,
                default_focus: false,
            },
            ResolutionHoldUiActionSpec {
                action: ResolutionHoldUiAction::RequestDestroy,
                label: copy.destroy_review_action.clone(),
                enabled: entries.iter().any(|entry| entry.selected),
                emphasis: ResolutionHoldUiActionEmphasis::Destructive,
                default_focus: false,
            },
        ],
        ResolutionHoldClientPhase::ConfirmDestroy => vec![
            ResolutionHoldUiActionSpec {
                action: ResolutionHoldUiAction::CancelDestroy,
                label: copy.cancel_action.clone(),
                enabled: true,
                emphasis: ResolutionHoldUiActionEmphasis::Primary,
                default_focus: true,
            },
            ResolutionHoldUiActionSpec {
                action: ResolutionHoldUiAction::ConfirmDestroy,
                label: copy.confirm_destroy_action.clone(),
                enabled: true,
                emphasis: ResolutionHoldUiActionEmphasis::Destructive,
                default_focus: false,
            },
        ],
        ResolutionHoldClientPhase::RecoverableError
            if model.retry_directive() != ResolutionHoldRetryDirective::WaitForHall =>
        {
            vec![ResolutionHoldUiActionSpec {
                action: ResolutionHoldUiAction::Retry,
                label: retry_label(model.retry_directive(), copy),
                enabled: model.retry_directive() != ResolutionHoldRetryDirective::Unavailable,
                emphasis: ResolutionHoldUiActionEmphasis::Primary,
                default_focus: true,
            }]
        }
        _ => Vec::new(),
    }
}

fn retry_label(directive: ResolutionHoldRetryDirective, copy: &ResolutionHoldUiCopy) -> String {
    match directive {
        ResolutionHoldRetryDirective::RetryExactMutation => "RETRY SAME REQUEST".to_owned(),
        ResolutionHoldRetryDirective::RefreshAuthority => "REFRESH STORAGE".to_owned(),
        ResolutionHoldRetryDirective::CorrectClock => "CHECK CLOCK & REFRESH".to_owned(),
        _ => copy.retry_action.clone(),
    }
}

fn status_for_model(model: &ResolutionHoldClientModel) -> Option<ResolutionHoldUiStatus> {
    match model.phase() {
        ResolutionHoldClientPhase::Querying => Some(status(
            "Checking secure storage",
            "Waiting for the authoritative Hall inventory snapshot.",
            ResolutionHoldUiTone::Progress,
        )),
        ResolutionHoldClientPhase::Submitting => Some(status(
            "Request locked",
            "Waiting for durable storage acknowledgement. Controls remain locked.",
            ResolutionHoldUiTone::Progress,
        )),
        ResolutionHoldClientPhase::Refreshing => {
            let replayed = model.last_stored_result().is_some();
            Some(status(
                if replayed {
                    "Storage update acknowledged"
                } else {
                    "Refreshing storage"
                },
                "Verifying the remaining held stacks before returning control.",
                ResolutionHoldUiTone::Success,
            ))
        }
        ResolutionHoldClientPhase::ConfirmDestroy => Some(status(
            "Permanent action",
            "Cancel is selected by default. Confirm only if you accept permanent, reward-free destruction.",
            ResolutionHoldUiTone::Warning,
        )),
        ResolutionHoldClientPhase::RecoverableError | ResolutionHoldClientPhase::FatalError => {
            model.failure().map(status_for_failure)
        }
        _ => None,
    }
}

fn status_for_failure(failure: ResolutionHoldClientFailure) -> ResolutionHoldUiStatus {
    match failure {
        ResolutionHoldClientFailure::ResponseLost => status(
            "Connection interrupted",
            "No new request will be created. Reconnect and retry the retained request or refresh storage authority.",
            ResolutionHoldUiTone::Failure,
        ),
        ResolutionHoldClientFailure::FeatureNotNegotiated => status(
            "Storage recovery unavailable",
            "This server did not advertise the required recovery capability. Player control remains locked.",
            ResolutionHoldUiTone::Failure,
        ),
        ResolutionHoldClientFailure::InvalidResponse => status(
            "Storage response rejected",
            "The response was malformed or inconsistent. No local state was applied.",
            ResolutionHoldUiTone::Failure,
        ),
        ResolutionHoldClientFailure::ContentProjectionMismatch => status(
            "Content update required",
            "The held item projection does not match this client build. No item action is available.",
            ResolutionHoldUiTone::Failure,
        ),
        ResolutionHoldClientFailure::Rejected(code) => status_for_rejection(code),
    }
}

fn status_for_rejection(code: ResolutionHoldRejectionCodeV1) -> ResolutionHoldUiStatus {
    let (title, detail) = match code {
        ResolutionHoldRejectionCodeV1::FeatureDisabled => (
            "Storage recovery disabled",
            "The server has disabled this capability. Player control remains locked.",
        ),
        ResolutionHoldRejectionCodeV1::InvalidRequest => (
            "Request rejected",
            "The server rejected the request shape. No item state changed.",
        ),
        ResolutionHoldRejectionCodeV1::IssuedAtInvalid => (
            "System clock needs attention",
            "Correct the device clock, then refresh storage before creating a new request.",
        ),
        ResolutionHoldRejectionCodeV1::ContentMismatch => (
            "Content update required",
            "This client cannot safely present the server's current item authority.",
        ),
        ResolutionHoldRejectionCodeV1::StaleAuthority => (
            "Storage changed",
            "Refresh the current storage snapshot before choosing a new action.",
        ),
        ResolutionHoldRejectionCodeV1::ForeignAuthority => (
            "Character authority changed",
            "The authenticated account no longer owns this selected character request.",
        ),
        ResolutionHoldRejectionCodeV1::HallBindingRequired => (
            "Returning to Lantern Halls",
            "Storage recovery resumes after the authoritative Hall arrival is confirmed.",
        ),
        ResolutionHoldRejectionCodeV1::StorageFull => (
            "No legal storage available",
            "The Move action could not place the complete stack. Free safe storage or choose permanent destruction.",
        ),
        ResolutionHoldRejectionCodeV1::NoHeldStack => (
            "Stack already resolved",
            "Refresh storage to load the current held stack list.",
        ),
        ResolutionHoldRejectionCodeV1::ConfirmationRequired => (
            "Confirmation required",
            "Review the permanent-destruction warning and confirm again explicitly.",
        ),
        ResolutionHoldRejectionCodeV1::IdempotencyConflict => (
            "Request identity conflict",
            "The same request identity was reused with different data. Recovery is locked for support review.",
        ),
        ResolutionHoldRejectionCodeV1::DatabaseUnavailable => (
            "Storage service unavailable",
            "The exact unresolved request is retained and may be retried without changing its identity.",
        ),
        ResolutionHoldRejectionCodeV1::CorruptStoredAuthority => (
            "Stored authority requires support",
            "The server rejected inconsistent durable storage data. No local workaround is available.",
        ),
        ResolutionHoldRejectionCodeV1::UnresolvedMutation => (
            "Another storage update is pending",
            "Wait for the prior durable mutation, then refresh the authoritative stack list.",
        ),
    };
    status(title, detail, ResolutionHoldUiTone::Failure)
}

fn status(title: &str, detail: &str, tone: ResolutionHoldUiTone) -> ResolutionHoldUiStatus {
    ResolutionHoldUiStatus {
        title: title.to_owned(),
        detail: detail.to_owned(),
        tone,
    }
}

fn format_uid(uid: [u8; 16]) -> String {
    let mut output = String::with_capacity(36);
    for (index, byte) in uid.into_iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            output.push('-');
        }
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn format_unix_millis_utc(unix_millis: u64) -> Result<String, ResolutionHoldUiError> {
    let total_seconds = unix_millis / 1_000;
    let days = i64::try_from(total_seconds / 86_400)
        .map_err(|_| ResolutionHoldUiError::InvalidTimestamp)?;
    let seconds_of_day = total_seconds % 86_400;
    let (year, month, day) = civil_from_days(days)?;
    let hour = seconds_of_day / 3_600;
    let minute = seconds_of_day % 3_600 / 60;
    Ok(format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} UTC"
    ))
}

// Gregorian civil-date conversion for nonnegative days since the Unix epoch.
fn civil_from_days(days_since_epoch: i64) -> Result<(i64, u64, u64), ResolutionHoldUiError> {
    let z = days_since_epoch
        .checked_add(719_468)
        .ok_or(ResolutionHoldUiError::InvalidTimestamp)?;
    let era = z / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    Ok((
        year,
        u64::try_from(month).map_err(|_| ResolutionHoldUiError::InvalidTimestamp)?,
        u64::try_from(day).map_err(|_| ResolutionHoldUiError::InvalidTimestamp)?,
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ResolutionHoldUiError {
    #[error("Resolution Hold UI is not open")]
    SurfaceNotOpen,
    #[error("Resolution Hold item content or localization is missing")]
    MissingItemContent,
    #[error("Resolution Hold item kind conflicts with compiled content")]
    ItemKindMismatch,
    #[error("Resolution Hold content revision conflicts with the compiled catalog")]
    ContentRevisionMismatch,
    #[error("Resolution Hold destructive review has no selected stack")]
    MissingSelectedStack,
    #[error("Resolution Hold viewport or UI scale is unsupported")]
    InvalidLayout,
    #[error("Resolution Hold deadline is outside the supported UTC range")]
    InvalidTimestamp,
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use protocol::{
        CORE_RESOLUTION_HOLD_FEATURE_FLAG, M03_CORE_DEV_BUILD_ID, ProtocolVersion,
        RESOLUTION_HOLD_SCHEMA_VERSION, ResolutionHoldItemV1, ResolutionHoldQueryResultV1,
        ResolutionHoldStackV1, ResolutionHoldVersionsV1, SIMULATION_HZ, SNAPSHOT_HZ, ServerHello,
        WireText,
    };
    use sim_content::load_core_development_items;

    use super::*;

    const CHARACTER_ID: [u8; 16] = [1; 16];

    fn catalog() -> CompiledProductionItemCatalog {
        load_core_development_items(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content"))
            .unwrap()
    }

    fn hello() -> ServerHello {
        let version = ProtocolVersion::current();
        ServerHello {
            session_id: WireText::new("hold-ui-test").unwrap(),
            protocol_major: version.major,
            protocol_minor: version.minor,
            required_client_build: WireText::new(M03_CORE_DEV_BUILD_ID).unwrap(),
            content_bundle_version: WireText::new("core-test").unwrap(),
            server_tick_rate: SIMULATION_HZ,
            snapshot_rate: SNAPSHOT_HZ,
            region_id: WireText::new("local").unwrap(),
            feature_flags: vec![WireText::new(CORE_RESOLUTION_HOLD_FEATURE_FLAG).unwrap()],
        }
    }

    fn stack(
        extraction_byte: u8,
        template_id: &str,
        content_revision: &str,
        kind: ResolutionHoldItemKindV1,
        item_bytes: &[u8],
        destination: Option<ResolutionHoldDestinationV1>,
    ) -> ResolutionHoldStackV1 {
        ResolutionHoldStackV1 {
            extraction_id: [extraction_byte; 16],
            stack_index: 0,
            template_id: WireText::new(template_id).unwrap(),
            content_revision: WireText::new(content_revision).unwrap(),
            item_kind: kind,
            items: item_bytes
                .iter()
                .copied()
                .map(|byte| ResolutionHoldItemV1 {
                    item_uid: [byte; 16],
                    item_version: 7,
                })
                .collect(),
            stack_digest: [extraction_byte.saturating_add(20); 32],
            extracted_at_unix_millis: 1_699_740_800_000,
            overflow_deadline_unix_millis: 1_700_000_000_000,
            planned_destination: destination,
        }
    }

    fn ready_model(
        catalog: &CompiledProductionItemCatalog,
        stacks: Vec<ResolutionHoldStackV1>,
    ) -> ResolutionHoldClientModel {
        let mut model = ResolutionHoldClientModel::new(
            WireText::new(catalog.revision_label().to_owned()).unwrap(),
        );
        model.begin_hall_query(&hello(), CHARACTER_ID, 1).unwrap();
        model
            .apply_query_result(&ResolutionHoldQueryResultV1::Stored {
                schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
                request_sequence: 1,
                character_id: CHARACTER_ID,
                versions: ResolutionHoldVersionsV1 {
                    account: 10,
                    character: 20,
                    world: 30,
                    inventory: 40,
                },
                storage_resolution_required: true,
                stacks,
            })
            .unwrap();
        model
    }

    #[test]
    fn projection_uses_compiled_names_icons_quantities_and_one_based_destinations() {
        let catalog = catalog();
        let revision = catalog.revision_label();
        let stacks = vec![
            stack(
                2,
                "item.weapon.crossbow.pine_crossbow",
                revision,
                ResolutionHoldItemKindV1::Equipment,
                &[2],
                Some(ResolutionHoldDestinationV1::CharacterSafe { slot_index: 0 }),
            ),
            stack(
                3,
                "item.weapon.crossbow.pine_crossbow",
                revision,
                ResolutionHoldItemKindV1::Equipment,
                &[3],
                Some(ResolutionHoldDestinationV1::Vault { slot_index: 7 }),
            ),
            stack(
                4,
                "item.weapon.crossbow.pine_crossbow",
                revision,
                ResolutionHoldItemKindV1::Equipment,
                &[4],
                Some(ResolutionHoldDestinationV1::Overflow { slot_index: 19 }),
            ),
            stack(
                5,
                "consumable.red_tonic",
                revision,
                ResolutionHoldItemKindV1::Consumable,
                &[5, 6],
                None,
            ),
        ];
        let mut model = ready_model(&catalog, stacks);
        model.select_stack([4; 16], 0).unwrap();
        let snapshot =
            ResolutionHoldUiSnapshot::from_model(&model, &catalog, ResolutionHoldUiCopy::default())
                .unwrap();
        assert_eq!(snapshot.entries.len(), 4);
        assert_eq!(snapshot.entries[0].localized_name, "Pine Crossbow");
        assert_eq!(
            snapshot.entries[0].destination_label,
            "Character Safe · Slot 1"
        );
        assert_eq!(snapshot.entries[1].destination_label, "Vault · Slot 8");
        assert_eq!(
            snapshot.entries[2].destination_label,
            "Overflow Cache · Slot 20"
        );
        assert_eq!(
            snapshot.entries[2].overflow_deadline_utc,
            "2023-11-14 22:13 UTC"
        );
        assert!(snapshot.entries[2].selected);
        assert_eq!(snapshot.entries[3].localized_name, "Red Tonic");
        assert_eq!(snapshot.entries[3].quantity, 2);
        assert!(!snapshot.entries[3].can_move);
        assert_eq!(snapshot.entries[3].durable_uids.len(), 2);
        assert!(snapshot.actions[0].enabled);
        assert!(snapshot.escape_action().is_none());
    }

    #[test]
    fn destruction_review_defaults_to_cancel_and_never_exposes_close_to_play() {
        let catalog = catalog();
        let mut model = ready_model(
            &catalog,
            vec![stack(
                2,
                "item.weapon.crossbow.pine_crossbow",
                catalog.revision_label(),
                ResolutionHoldItemKindV1::Equipment,
                &[2],
                None,
            )],
        );
        model.request_destroy_confirmation().unwrap();
        let snapshot =
            ResolutionHoldUiSnapshot::from_model(&model, &catalog, ResolutionHoldUiCopy::default())
                .unwrap();
        assert_eq!(
            snapshot.escape_action(),
            Some(ResolutionHoldUiAction::CancelDestroy)
        );
        assert_eq!(
            snapshot.actions[0].action,
            ResolutionHoldUiAction::CancelDestroy
        );
        assert!(snapshot.actions[0].default_focus);
        assert_eq!(
            snapshot.actions[1].action,
            ResolutionHoldUiAction::ConfirmDestroy
        );
        assert!(
            snapshot
                .destroy_review
                .as_ref()
                .unwrap()
                .warning
                .contains("grants no benefit")
        );
    }

    #[test]
    fn projection_rejects_missing_or_mismatched_compiled_item_authority() {
        let catalog = catalog();
        let unknown = ready_model(
            &catalog,
            vec![stack(
                2,
                "item.unknown",
                catalog.revision_label(),
                ResolutionHoldItemKindV1::Equipment,
                &[2],
                None,
            )],
        );
        assert_eq!(
            ResolutionHoldUiSnapshot::from_model(
                &unknown,
                &catalog,
                ResolutionHoldUiCopy::default(),
            ),
            Err(ResolutionHoldUiError::MissingItemContent)
        );

        let wrong_kind = ready_model(
            &catalog,
            vec![stack(
                2,
                "consumable.red_tonic",
                catalog.revision_label(),
                ResolutionHoldItemKindV1::Equipment,
                &[2],
                None,
            )],
        );
        assert_eq!(
            ResolutionHoldUiSnapshot::from_model(
                &wrong_kind,
                &catalog,
                ResolutionHoldUiCopy::default(),
            ),
            Err(ResolutionHoldUiError::ItemKindMismatch)
        );
    }

    #[test]
    fn certified_viewports_keep_safe_margins_and_legible_text() {
        for (width, height, scale) in [
            (1_280.0, 720.0, 80),
            (1_280.0, 720.0, 150),
            (1_920.0, 1_080.0, 100),
            (1_920.0, 1_080.0, 150),
        ] {
            let metrics = ResolutionHoldUiMetrics::for_viewport(width, height, scale).unwrap();
            assert!(metrics.safe_margin_px >= 16.0);
            assert!(metrics.body_text_px >= 14.0);
            assert!(metrics.label_text_px >= 14.0);
        }
        assert_eq!(
            ResolutionHoldUiMetrics::for_viewport(1_000.0, 700.0, 100),
            Err(ResolutionHoldUiError::InvalidLayout)
        );
    }

    #[test]
    fn focus_wraps_and_skips_disabled_controls() {
        let button = |order, enabled| ResolutionHoldUiButton {
            action: ResolutionHoldUiAction::Retry,
            enabled,
            emphasis: ResolutionHoldUiActionEmphasis::Primary,
            order,
            initial_focus: ResolutionHoldUiInitialFocus::Ordinary,
            role: ResolutionHoldUiButtonRole::Action,
        };
        let buttons = vec![button(1, true), button(2, false), button(3, true)];
        assert_eq!(next_hold_focus_order(&buttons, Some(1), 1), Some(3));
        assert_eq!(next_hold_focus_order(&buttons, Some(3), 1), Some(1));
        assert_eq!(next_hold_focus_order(&buttons, Some(1), -1), Some(3));
        assert_eq!(next_hold_focus_order(&buttons, None, 1), Some(1));
        assert_eq!(next_hold_focus_order(&buttons, None, -1), Some(3));
    }

    #[test]
    fn focused_row_scroll_is_clamped_and_minimally_revealed() {
        let close = |actual: f32, expected: f32| (actual - expected).abs() < f32::EPSILON;
        assert!(close(
            scroll_offset_to_reveal_hold(100.0, 500.0, 0.0, 200.0, 210.0, 250.0, 12.0),
            162.0
        ));
        assert!(close(
            scroll_offset_to_reveal_hold(100.0, 500.0, 0.0, 200.0, 5.0, 65.0, 12.0),
            93.0
        ));
        assert!(close(
            scroll_offset_to_reveal_hold(490.0, 500.0, 0.0, 200.0, 300.0, 400.0, 12.0),
            500.0
        ));
    }

    #[test]
    fn native_view_rejects_uncertified_scale() {
        let catalog = catalog();
        let model = ready_model(
            &catalog,
            vec![stack(
                2,
                "item.weapon.crossbow.pine_crossbow",
                catalog.revision_label(),
                ResolutionHoldItemKindV1::Equipment,
                &[2],
                None,
            )],
        );
        let snapshot =
            ResolutionHoldUiSnapshot::from_model(&model, &catalog, ResolutionHoldUiCopy::default())
                .unwrap();
        assert!(matches!(
            NativeResolutionHoldView::new(
                snapshot,
                ResolutionHoldUiConfig {
                    reduced_effects: false,
                    ui_scale_percent: 151,
                },
            ),
            Err(ResolutionHoldUiError::InvalidLayout)
        ));
    }
}
