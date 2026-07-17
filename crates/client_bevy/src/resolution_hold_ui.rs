//! Reusable native presentation projection for blocking Resolution Hold recovery.
//!
//! The projection contains no transport or mutation authority. It renders only validated server
//! stacks and emits semantic commands for the owning Hall controller.

use std::fmt::Write as _;

use content_schema::ProductionItemTemplatePayload;
use protocol::{
    ResolutionHoldDestinationV1, ResolutionHoldItemKindV1, ResolutionHoldRejectionCodeV1,
};
use sim_content::CompiledProductionItemCatalog;
use thiserror::Error;

use crate::resolution_hold::{
    ResolutionHoldClientFailure, ResolutionHoldClientModel, ResolutionHoldClientPhase,
    ResolutionHoldRetryDirective,
};

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
}
