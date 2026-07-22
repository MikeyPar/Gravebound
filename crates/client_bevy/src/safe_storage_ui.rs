//! Native Hall Vault and Overflow presentation.

use std::fmt::Write as _;

use bevy::prelude::*;
use sim_content::CompiledProductionItemCatalog;
use thiserror::Error;

use crate::safe_storage::{
    SafeStorageClientModel, SafeStorageClientPhase, SafeStorageSelectionPane,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SafeStorageUiSnapshot {
    pub(crate) title: String,
    pub(crate) eyebrow: String,
    pub(crate) character_safe_rows: String,
    pub(crate) surface_rows: String,
    pub(crate) detail: String,
    pub(crate) status: String,
    pub(crate) footer: String,
}

impl SafeStorageUiSnapshot {
    pub(crate) fn from_model(
        model: &SafeStorageClientModel,
        catalog: &CompiledProductionItemCatalog,
    ) -> Result<Self, SafeStorageUiError> {
        let surface = model.surface().ok_or(SafeStorageUiError::SurfaceClosed)?;
        let (title, eyebrow) = match surface {
            protocol::SafeStorageSurfaceV1::Vault => (
                "LANTERN VAULT",
                "ACCOUNT-BOUND CUSTODY  ·  160 DURABLE SLOTS",
            ),
            protocol::SafeStorageSurfaceV1::Overflow => (
                "OVERFLOW CACHE",
                "TEMPORARY SAFE CUSTODY  ·  WITHDRAW BEFORE EXPIRY",
            ),
        };
        let character_safe_rows = render_rows(
            model.character_safe(),
            model.selected_pane() == SafeStorageSelectionPane::CharacterSafe,
            model.selected_index(),
            catalog,
        )?;
        let surface_rows = render_rows(
            model.stacks(),
            model.selected_pane() == SafeStorageSelectionPane::Surface,
            model.selected_index(),
            catalog,
        )?;
        let selected = match model.selected_pane() {
            SafeStorageSelectionPane::CharacterSafe => model.character_safe(),
            SafeStorageSelectionPane::Surface => model.stacks(),
        }
        .get(model.selected_index());
        let detail = match selected {
            Some(stack) => render_detail(stack, catalog)?,
            None => "No item selected. Empty slots remain available for future custody.".to_owned(),
        };
        let status = render_status(model);
        let footer = match surface {
            protocol::SafeStorageSurfaceV1::Vault => {
                "←/→ or Tab switch custody  ·  ↑/↓ select  ·  Enter deposit/withdraw  ·  R retry  ·  Esc close"
            }
            protocol::SafeStorageSurfaceV1::Overflow => {
                "←/→ or Tab switch view  ·  ↑/↓ select  ·  Enter withdraw  ·  R retry  ·  Esc close"
            }
        };
        Ok(Self {
            title: title.to_owned(),
            eyebrow: eyebrow.to_owned(),
            character_safe_rows,
            surface_rows,
            detail,
            status,
            footer: footer.to_owned(),
        })
    }
}

fn render_rows(
    stacks: &[protocol::SafeStorageStackV1],
    pane_selected: bool,
    selected_index: usize,
    catalog: &CompiledProductionItemCatalog,
) -> Result<String, SafeStorageUiError> {
    const WINDOW_ROWS: usize = 12;
    if stacks.is_empty() {
        return Ok("   — Empty —".to_owned());
    }
    let start = selected_index
        .saturating_sub(WINDOW_ROWS / 2)
        .min(stacks.len().saturating_sub(WINDOW_ROWS));
    let mut rows = String::new();
    for (index, stack) in stacks.iter().enumerate().skip(start).take(WINDOW_ROWS) {
        let name = catalog
            .localized_item_name(stack.template_id.as_str())
            .ok_or(SafeStorageUiError::MissingItemContent)?;
        let marker = if pane_selected && index == selected_index {
            "▶"
        } else {
            " "
        };
        writeln!(
            &mut rows,
            "{marker} {:>3}  {:<24}  x{}",
            stack.slot_index + 1,
            truncate_label(name, 24),
            stack.items.len()
        )
        .expect("writing to String cannot fail");
    }
    if stacks.len() > WINDOW_ROWS {
        write!(
            &mut rows,
            "\n   Showing {}–{} of {}",
            start + 1,
            (start + WINDOW_ROWS).min(stacks.len()),
            stacks.len()
        )
        .expect("writing to String cannot fail");
    }
    Ok(rows)
}

fn render_detail(
    stack: &protocol::SafeStorageStackV1,
    catalog: &CompiledProductionItemCatalog,
) -> Result<String, SafeStorageUiError> {
    let name = catalog
        .localized_item_name(stack.template_id.as_str())
        .ok_or(SafeStorageUiError::MissingItemContent)?;
    let mut detail = format!(
        "{name}\n{:?}  ·  Slot {}  ·  {:?}  ·  {:?}\nTemplate: {}",
        stack.location,
        stack.slot_index + 1,
        stack.item_kind,
        stack.provenance,
        stack.template_id.as_str()
    );
    if let (Some(level), Some(rarity)) = (stack.item_level, stack.rarity) {
        write!(
            &mut detail,
            "\nLevel {level}  ·  {rarity:?}  ·  Salvage {} (band {})",
            stack.salvage_value, stack.salvage_band
        )
        .expect("writing to String cannot fail");
    }
    if let Some(deadline) = stack.overflow_expires_at_unix_millis {
        write!(
            &mut detail,
            "\nEarliest overflow expiry: {}",
            format_unix_millis_utc(deadline)?
        )
        .expect("writing to String cannot fail");
    }
    detail.push_str("\nDurable identities:");
    for item in &stack.items {
        write!(
            &mut detail,
            "\n  {}  ·  v{}  ·  {:?}",
            format_uid(item.item_uid),
            item.item_version,
            item.provenance,
        )
        .expect("writing to String cannot fail");
        if item.salvage_value > 0 {
            write!(
                &mut detail,
                "  ·  salvage {} (band {})",
                item.salvage_value, item.salvage_band
            )
            .expect("writing to String cannot fail");
        }
        if let Some(deadline) = item.overflow_expires_at_unix_millis {
            write!(&mut detail, "  ·  {}", format_unix_millis_utc(deadline)?)
                .expect("writing to String cannot fail");
        }
    }
    Ok(detail)
}

fn render_status(model: &SafeStorageClientModel) -> String {
    let versions = model.versions().map_or_else(
        || "Versions awaiting authority".to_owned(),
        |(account, inventory)| format!("Account v{account}  ·  Inventory v{inventory}"),
    );
    let activity = match model.phase() {
        SafeStorageClientPhase::Dormant => "CLOSED",
        SafeStorageClientPhase::Loading => "SYNCING AUTHORITY",
        SafeStorageClientPhase::Ready => "READY",
        SafeStorageClientPhase::Mutating => "COMMITTING TRANSFER",
        SafeStorageClientPhase::Reconnecting => "RETRY REQUIRED · EXACT REQUEST HELD",
        SafeStorageClientPhase::Failed => "AUTHORITY UNAVAILABLE",
    };
    if let Some(code) = model.last_query_code() {
        format!("{activity}  ·  Query: {code:?}  ·  {versions}")
    } else if let Some(code) = model.last_mutation_code() {
        format!("{activity}  ·  Last transfer: {code:?}  ·  {versions}")
    } else {
        format!("{activity}  ·  {versions}")
    }
}

#[derive(Debug, Clone, Resource)]
pub(crate) struct NativeSafeStorageView {
    pub(crate) revision: u64,
    pub(crate) snapshot: SafeStorageUiSnapshot,
}

#[derive(Debug, Component)]
struct SafeStorageUiRoot {
    revision: u64,
}

#[derive(Debug, Default)]
pub(crate) struct NativeSafeStoragePlugin;

impl Plugin for NativeSafeStoragePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, render_native_safe_storage);
    }
}

fn render_native_safe_storage(
    mut commands: Commands,
    view: Option<Res<NativeSafeStorageView>>,
    roots: Query<(Entity, &SafeStorageUiRoot)>,
) {
    let existing = roots.iter().next();
    let Some(view) = view else {
        if let Some((entity, _)) = existing {
            commands.entity(entity).despawn();
        }
        return;
    };
    if existing.is_some_and(|(_, root)| root.revision == view.revision) {
        return;
    }
    if let Some((entity, _)) = existing {
        commands.entity(entity).despawn();
    }
    spawn_surface(&mut commands, &view);
}

fn spawn_surface(commands: &mut Commands, view: &NativeSafeStorageView) {
    commands
        .spawn((
            Name::new("Native safe storage surface"),
            SafeStorageUiRoot {
                revision: view.revision,
            },
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                padding: UiRect::all(px(28)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba_u8(2, 5, 7, 238)),
            GlobalZIndex(80),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: percent(100),
                    max_width: px(1_180),
                    height: percent(100),
                    max_height: px(920),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(12),
                    padding: UiRect::all(px(22)),
                    border: UiRect::all(px(2)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(Color::srgba_u8(9, 15, 18, 252)),
                BorderColor::all(Color::srgb_u8(124, 162, 147)),
                BoxShadow::new(Color::srgba_u8(0, 0, 0, 190), px(0), px(10), px(0), px(28)),
            ))
            .with_children(|panel| {
                spawn_text(
                    panel,
                    &view.snapshot.eyebrow,
                    13.0,
                    Color::srgb_u8(118, 157, 145),
                );
                spawn_text(
                    panel,
                    &view.snapshot.title,
                    30.0,
                    Color::srgb_u8(238, 218, 166),
                );
                spawn_text(
                    panel,
                    &view.snapshot.status,
                    14.0,
                    Color::srgb_u8(190, 203, 193),
                );
                panel
                    .spawn((Node {
                        width: percent(100),
                        flex_grow: 1.0,
                        min_height: px(280),
                        flex_direction: FlexDirection::Row,
                        column_gap: px(12),
                        overflow: Overflow::clip(),
                        ..default()
                    },))
                    .with_children(|columns| {
                        spawn_column(
                            columns,
                            "CHARACTER SAFE  ·  8 SLOTS",
                            &view.snapshot.character_safe_rows,
                        );
                        spawn_column(columns, &view.snapshot.title, &view.snapshot.surface_rows);
                    });
                panel
                    .spawn((
                        Node {
                            width: percent(100),
                            min_height: px(126),
                            padding: UiRect::all(px(14)),
                            border: UiRect::all(px(1)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb_u8(13, 21, 24)),
                        BorderColor::all(Color::srgb_u8(70, 94, 87)),
                    ))
                    .with_child((
                        Text::new(view.snapshot.detail.clone()),
                        TextFont::from_font_size(14.0),
                        TextColor(Color::srgb_u8(213, 214, 199)),
                    ));
                spawn_text(
                    panel,
                    &view.snapshot.footer,
                    13.0,
                    Color::srgb_u8(131, 158, 149),
                );
            });
        });
}

fn spawn_column(parent: &mut ChildSpawnerCommands, title: &str, rows: &str) {
    parent
        .spawn((
            Node {
                width: percent(50),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                padding: UiRect::all(px(14)),
                border: UiRect::all(px(1)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(12, 19, 22)),
            BorderColor::all(Color::srgb_u8(58, 78, 73)),
        ))
        .with_children(|column| {
            spawn_text(column, title, 14.0, Color::srgb_u8(221, 196, 137));
            spawn_text(column, rows, 15.0, Color::srgb_u8(218, 222, 209));
        });
}

fn spawn_text(parent: &mut ChildSpawnerCommands, value: &str, size: f32, color: Color) {
    parent.spawn((
        Text::new(value.to_owned()),
        TextFont::from_font_size(size),
        TextColor(color),
    ));
}

fn truncate_label(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let mut truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        truncated.pop();
        truncated.push('…');
    }
    truncated
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

fn format_unix_millis_utc(unix_millis: u64) -> Result<String, SafeStorageUiError> {
    let total_seconds = unix_millis / 1_000;
    let days =
        i64::try_from(total_seconds / 86_400).map_err(|_| SafeStorageUiError::InvalidTimestamp)?;
    let seconds_of_day = total_seconds % 86_400;
    let (year, month, day) = civil_from_days(days)?;
    let hour = seconds_of_day / 3_600;
    let minute = seconds_of_day % 3_600 / 60;
    Ok(format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} UTC"
    ))
}

fn civil_from_days(days_since_epoch: i64) -> Result<(i64, u64, u64), SafeStorageUiError> {
    let z = days_since_epoch
        .checked_add(719_468)
        .ok_or(SafeStorageUiError::InvalidTimestamp)?;
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
        u64::try_from(month).map_err(|_| SafeStorageUiError::InvalidTimestamp)?,
        u64::try_from(day).map_err(|_| SafeStorageUiError::InvalidTimestamp)?,
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum SafeStorageUiError {
    #[error("safe-storage surface is closed")]
    SurfaceClosed,
    #[error("safe-storage item content is missing")]
    MissingItemContent,
    #[error("safe-storage Overflow deadline is invalid")]
    InvalidTimestamp,
}
