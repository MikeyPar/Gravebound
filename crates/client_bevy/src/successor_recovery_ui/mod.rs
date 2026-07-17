//! Reusable native presentation for the `GB-M03-07` successor-to-Hall handoff.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`DTH-020`, `DTH-021`, `UI-007`-
//! `009`), `Gravebound_Content_Production_Spec_v1.md` (`CONT-CATALOG-003`, `CONT-HUB-001`),
//! and `Gravebound_Development_Roadmap_v1.md` (`GB-M03-07`). This module projects the existing
//! recovery state machine and the independently validated successor copy target. It authors no
//! mutation, result, item identity, character identity, destination, or authoritative version.

mod render;

use bevy::{prelude::*, window::WindowResized};
use sim_content::{CoreSuccessorRecoveryContent, CoreSuccessorRecoveryCopyKey};
use thiserror::Error;

use crate::{
    SuccessorCharacterSelectProjection, SuccessorRecoveryClientModel, SuccessorRecoveryPhase,
    SuccessorRecoveryRetryDirective,
};

pub const SUCCESSOR_MIN_VIEW_WIDTH: f32 = 1_280.0;
pub const SUCCESSOR_MIN_VIEW_HEIGHT: f32 = 720.0;
pub const SUCCESSOR_MIN_UI_SCALE_PERCENT: u16 = 80;
pub const SUCCESSOR_MAX_UI_SCALE_PERCENT: u16 = 150;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuccessorRecoveryUiSurface {
    Creating,
    RecoverableCreate,
    CharacterSelect,
    EnteringHall,
    LoadingHall,
    RecoverableHall,
    HallReady,
    Fatal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuccessorRecoveryUiTone {
    Neutral,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuccessorRecoveryUiActivity {
    Idle,
    Busy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuccessorRecoveryUiAction {
    Play,
    RetryCreate,
    RetryHall,
    RefreshDeathSummary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuccessorRecoveryUiActionSpec {
    pub action: SuccessorRecoveryUiAction,
    pub label: String,
    pub input_hint: Option<String>,
    pub enabled: bool,
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuccessorRecoveryUiCharacter {
    pub selected_character_id: [u8; 16],
    pub roster_ordinal: u8,
    pub class_id: String,
    pub class_name: String,
    pub appearance_id: String,
    pub slot_text: String,
    pub level_text: String,
    pub oath_text: String,
    pub starter_text: String,
    pub security_text: String,
    pub account_version: u64,
    pub character_version: u64,
    pub world_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuccessorRecoveryUiSnapshot {
    pub surface: SuccessorRecoveryUiSurface,
    pub tone: SuccessorRecoveryUiTone,
    pub activity: SuccessorRecoveryUiActivity,
    pub eyebrow: String,
    pub title: String,
    pub subtitle: String,
    pub status: String,
    pub selected_badge: String,
    pub confirmation: Option<String>,
    pub progress_completed: u8,
    pub character: Option<SuccessorRecoveryUiCharacter>,
    pub hall_name: String,
    pub actions: Vec<SuccessorRecoveryUiActionSpec>,
    pub authority_footer: String,
    pub clean_recovery_footer: String,
}

impl SuccessorRecoveryUiSnapshot {
    pub fn project(
        model: &SuccessorRecoveryClientModel,
        content: &CoreSuccessorRecoveryContent,
    ) -> Result<Self, SuccessorRecoveryUiError> {
        let phase = model.phase();
        let surface = surface_for_phase(phase)?;
        let character = model
            .character_select_projection()
            .map(|projection| project_character(&projection, content))
            .transpose()?;
        if phase_requires_character(phase) && character.is_none() {
            return Err(SuccessorRecoveryUiError::MissingCharacterProjection);
        }
        let retry = model.retry_directive();
        let phase_presentation = phase_presentation(phase, retry, character.is_some())?;
        let tone = phase_presentation.tone;
        let activity = phase_presentation.activity;
        let status_key = phase_presentation.status_key;
        let progress_completed = phase_presentation.progress_completed;
        let confirmation = match progress_completed {
            1 => Some(
                content
                    .copy(CoreSuccessorRecoveryCopyKey::FieldConfirmationOne)
                    .to_owned(),
            ),
            2 => Some(
                content
                    .copy(CoreSuccessorRecoveryCopyKey::FieldConfirmationTwo)
                    .to_owned(),
            ),
            _ => None,
        };
        Ok(Self {
            surface,
            tone,
            activity,
            eyebrow: content
                .copy(CoreSuccessorRecoveryCopyKey::SurfaceEyebrow)
                .to_owned(),
            title: content
                .copy(CoreSuccessorRecoveryCopyKey::SurfaceTitle)
                .to_owned(),
            subtitle: content
                .copy(CoreSuccessorRecoveryCopyKey::SurfaceSubtitle)
                .to_owned(),
            status: content.copy(status_key).to_owned(),
            selected_badge: content
                .copy(CoreSuccessorRecoveryCopyKey::BadgeSelected)
                .to_owned(),
            confirmation,
            progress_completed,
            character,
            hall_name: content.hall_name().to_owned(),
            actions: project_actions(phase, retry, content),
            authority_footer: content
                .copy(CoreSuccessorRecoveryCopyKey::FooterAuthority)
                .to_owned(),
            clean_recovery_footer: content
                .copy(CoreSuccessorRecoveryCopyKey::FooterCleanRecovery)
                .to_owned(),
        })
    }

    /// Information-only signature used to prove reduced effects cannot alter recovery semantics.
    #[must_use]
    pub fn semantic_signature(&self) -> String {
        format!(
            "{:?}|{:?}|{}|{}|{}|{}|{}|{:?}|{:?}",
            self.surface,
            self.tone,
            self.status,
            self.progress_completed,
            self.character
                .as_ref()
                .map_or("none", |character| character.class_id.as_str()),
            self.character
                .as_ref()
                .map_or(0, |character| character.character_version),
            self.hall_name,
            self.actions,
            self.activity,
        )
    }
}

fn surface_for_phase(
    phase: SuccessorRecoveryPhase,
) -> Result<SuccessorRecoveryUiSurface, SuccessorRecoveryUiError> {
    match phase {
        SuccessorRecoveryPhase::Submitting => Ok(SuccessorRecoveryUiSurface::Creating),
        SuccessorRecoveryPhase::RecoverableError => {
            Ok(SuccessorRecoveryUiSurface::RecoverableCreate)
        }
        SuccessorRecoveryPhase::CharacterSelect => Ok(SuccessorRecoveryUiSurface::CharacterSelect),
        SuccessorRecoveryPhase::EnteringHall => Ok(SuccessorRecoveryUiSurface::EnteringHall),
        SuccessorRecoveryPhase::LoadingHall => Ok(SuccessorRecoveryUiSurface::LoadingHall),
        SuccessorRecoveryPhase::HallRecoverableError => {
            Ok(SuccessorRecoveryUiSurface::RecoverableHall)
        }
        SuccessorRecoveryPhase::ControllableHall => Ok(SuccessorRecoveryUiSurface::HallReady),
        SuccessorRecoveryPhase::FatalError => Ok(SuccessorRecoveryUiSurface::Fatal),
        SuccessorRecoveryPhase::Disabled
        | SuccessorRecoveryPhase::AwaitingTerminalSummary
        | SuccessorRecoveryPhase::Ready => Err(SuccessorRecoveryUiError::PhaseNotRenderable),
    }
}

#[derive(Debug, Clone, Copy)]
struct PhasePresentation {
    tone: SuccessorRecoveryUiTone,
    activity: SuccessorRecoveryUiActivity,
    status_key: CoreSuccessorRecoveryCopyKey,
    progress_completed: u8,
}

fn phase_presentation(
    phase: SuccessorRecoveryPhase,
    retry: SuccessorRecoveryRetryDirective,
    has_character: bool,
) -> Result<PhasePresentation, SuccessorRecoveryUiError> {
    let (tone, activity, status_key, progress_completed) = match phase {
        SuccessorRecoveryPhase::Submitting => (
            SuccessorRecoveryUiTone::Neutral,
            SuccessorRecoveryUiActivity::Busy,
            CoreSuccessorRecoveryCopyKey::StatusCreating,
            0,
        ),
        SuccessorRecoveryPhase::RecoverableError => (
            SuccessorRecoveryUiTone::Warning,
            SuccessorRecoveryUiActivity::Idle,
            CoreSuccessorRecoveryCopyKey::StatusRecoverable,
            0,
        ),
        SuccessorRecoveryPhase::CharacterSelect => (
            SuccessorRecoveryUiTone::Success,
            SuccessorRecoveryUiActivity::Idle,
            CoreSuccessorRecoveryCopyKey::StatusReady,
            1,
        ),
        SuccessorRecoveryPhase::EnteringHall => (
            SuccessorRecoveryUiTone::Neutral,
            SuccessorRecoveryUiActivity::Busy,
            CoreSuccessorRecoveryCopyKey::StatusEnteringHall,
            2,
        ),
        SuccessorRecoveryPhase::LoadingHall => (
            SuccessorRecoveryUiTone::Neutral,
            SuccessorRecoveryUiActivity::Busy,
            CoreSuccessorRecoveryCopyKey::StatusLoadingHall,
            2,
        ),
        SuccessorRecoveryPhase::HallRecoverableError => (
            SuccessorRecoveryUiTone::Warning,
            SuccessorRecoveryUiActivity::Idle,
            CoreSuccessorRecoveryCopyKey::StatusRecoverable,
            2,
        ),
        SuccessorRecoveryPhase::ControllableHall => (
            SuccessorRecoveryUiTone::Success,
            SuccessorRecoveryUiActivity::Idle,
            CoreSuccessorRecoveryCopyKey::StatusHallReady,
            2,
        ),
        SuccessorRecoveryPhase::FatalError => (
            SuccessorRecoveryUiTone::Error,
            SuccessorRecoveryUiActivity::Idle,
            if retry == SuccessorRecoveryRetryDirective::RestartAfterUpdate {
                CoreSuccessorRecoveryCopyKey::StatusUpdate
            } else {
                CoreSuccessorRecoveryCopyKey::StatusFatal
            },
            u8::from(has_character),
        ),
        SuccessorRecoveryPhase::Disabled
        | SuccessorRecoveryPhase::AwaitingTerminalSummary
        | SuccessorRecoveryPhase::Ready => {
            return Err(SuccessorRecoveryUiError::PhaseNotRenderable);
        }
    };
    Ok(PhasePresentation {
        tone,
        activity,
        status_key,
        progress_completed,
    })
}

const fn phase_requires_character(phase: SuccessorRecoveryPhase) -> bool {
    matches!(
        phase,
        SuccessorRecoveryPhase::CharacterSelect
            | SuccessorRecoveryPhase::EnteringHall
            | SuccessorRecoveryPhase::LoadingHall
            | SuccessorRecoveryPhase::HallRecoverableError
            | SuccessorRecoveryPhase::ControllableHall
    )
}

fn project_character(
    projection: &SuccessorCharacterSelectProjection,
    content: &CoreSuccessorRecoveryContent,
) -> Result<SuccessorRecoveryUiCharacter, SuccessorRecoveryUiError> {
    if projection.class_id.as_str() != content.class_id()
        || projection.appearance.content_id() != content.appearance_id()
        || projection.level != 1
        || projection.has_oath
        || projection.roster_ordinal == 0
    {
        return Err(SuccessorRecoveryUiError::ContentAuthorityMismatch);
    }
    Ok(SuccessorRecoveryUiCharacter {
        selected_character_id: projection.selected_character_id,
        roster_ordinal: projection.roster_ordinal,
        class_id: projection.class_id.as_str().to_owned(),
        class_name: content.class_name().to_owned(),
        appearance_id: projection.appearance.content_id().to_owned(),
        slot_text: content
            .copy(CoreSuccessorRecoveryCopyKey::FieldSlot)
            .replace("{ordinal}", &format!("{:02}", projection.roster_ordinal)),
        level_text: content
            .copy(CoreSuccessorRecoveryCopyKey::FieldLevel)
            .replace("{level}", &projection.level.to_string()),
        oath_text: content
            .copy(CoreSuccessorRecoveryCopyKey::FieldOathNone)
            .to_owned(),
        starter_text: content
            .copy(CoreSuccessorRecoveryCopyKey::FieldNewStarterKit)
            .to_owned(),
        security_text: content
            .copy(CoreSuccessorRecoveryCopyKey::FieldSafeCharacterSelect)
            .to_owned(),
        account_version: projection.account_version,
        character_version: projection.character_version,
        world_version: projection.world_version,
    })
}

fn project_actions(
    phase: SuccessorRecoveryPhase,
    retry: SuccessorRecoveryRetryDirective,
    content: &CoreSuccessorRecoveryContent,
) -> Vec<SuccessorRecoveryUiActionSpec> {
    let play = || SuccessorRecoveryUiActionSpec {
        action: SuccessorRecoveryUiAction::Play,
        label: content
            .copy(CoreSuccessorRecoveryCopyKey::ActionPlay)
            .to_owned(),
        input_hint: Some(
            content
                .copy(CoreSuccessorRecoveryCopyKey::InputPlay)
                .to_owned(),
        ),
        enabled: true,
        primary: true,
    };
    let retry_action = |action| SuccessorRecoveryUiActionSpec {
        action,
        label: content
            .copy(CoreSuccessorRecoveryCopyKey::ActionRetry)
            .to_owned(),
        input_hint: Some(
            content
                .copy(CoreSuccessorRecoveryCopyKey::InputPlay)
                .to_owned(),
        ),
        enabled: true,
        primary: true,
    };
    match (phase, retry) {
        (SuccessorRecoveryPhase::CharacterSelect, _) => vec![play()],
        (
            SuccessorRecoveryPhase::RecoverableError,
            SuccessorRecoveryRetryDirective::ExactCreateFrame,
        ) => vec![retry_action(SuccessorRecoveryUiAction::RetryCreate)],
        (
            SuccessorRecoveryPhase::HallRecoverableError,
            SuccessorRecoveryRetryDirective::SameHallMutation,
        ) => vec![retry_action(SuccessorRecoveryUiAction::RetryHall)],
        (
            SuccessorRecoveryPhase::FatalError,
            SuccessorRecoveryRetryDirective::RefreshDeathSummary,
        ) => vec![retry_action(SuccessorRecoveryUiAction::RefreshDeathSummary)],
        _ => Vec::new(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuccessorRecoveryUiConfig {
    pub reduced_effects: bool,
    pub ui_scale_percent: u16,
}

impl Default for SuccessorRecoveryUiConfig {
    fn default() -> Self {
        Self {
            reduced_effects: false,
            ui_scale_percent: 100,
        }
    }
}

impl SuccessorRecoveryUiConfig {
    pub fn validate(self) -> Result<Self, SuccessorRecoveryUiError> {
        if !(SUCCESSOR_MIN_UI_SCALE_PERCENT..=SUCCESSOR_MAX_UI_SCALE_PERCENT)
            .contains(&self.ui_scale_percent)
        {
            return Err(SuccessorRecoveryUiError::InvalidLayout);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuccessorRecoveryUiLayoutMode {
    Minimum,
    Reference,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SuccessorRecoveryUiMetrics {
    pub layout_mode: SuccessorRecoveryUiLayoutMode,
    pub safe_margin_px: f32,
    pub panel_width_px: f32,
    pub panel_height_px: f32,
    pub title_text_px: f32,
    pub heading_text_px: f32,
    pub body_text_px: f32,
    pub label_text_px: f32,
    pub action_height_px: f32,
}

impl SuccessorRecoveryUiMetrics {
    pub fn for_viewport(
        width: f32,
        height: f32,
        scale_percent: u16,
    ) -> Result<Self, SuccessorRecoveryUiError> {
        if !width.is_finite()
            || !height.is_finite()
            || width < SUCCESSOR_MIN_VIEW_WIDTH
            || height < SUCCESSOR_MIN_VIEW_HEIGHT
            || !(SUCCESSOR_MIN_UI_SCALE_PERCENT..=SUCCESSOR_MAX_UI_SCALE_PERCENT)
                .contains(&scale_percent)
        {
            return Err(SuccessorRecoveryUiError::InvalidLayout);
        }
        let layout_mode = if width < 1_600.0 || height < 900.0 {
            SuccessorRecoveryUiLayoutMode::Minimum
        } else {
            SuccessorRecoveryUiLayoutMode::Reference
        };
        let scale = f32::from(scale_percent) / 100.0;
        let density = (height / 1_080.0).clamp(0.82, 1.0) * scale;
        let safe_margin_px = (width * 0.024).clamp(18.0, 46.0);
        let panel_width_px = (1_180.0 * density).min(width - safe_margin_px * 2.0);
        let panel_height_px = (760.0 * density).min(height - safe_margin_px * 2.0);
        Ok(Self {
            layout_mode,
            safe_margin_px,
            panel_width_px,
            panel_height_px,
            title_text_px: 48.0 * density,
            heading_text_px: 28.0 * density,
            body_text_px: (18.0 * density).max(14.0),
            label_text_px: (15.0 * density).max(14.0),
            action_height_px: (64.0 * density).max(52.0),
        })
    }
}

#[derive(Debug, Clone, Resource)]
pub struct NativeSuccessorRecoveryView {
    snapshot: SuccessorRecoveryUiSnapshot,
    config: SuccessorRecoveryUiConfig,
    layout_epoch: u64,
}

impl NativeSuccessorRecoveryView {
    pub fn new(
        snapshot: SuccessorRecoveryUiSnapshot,
        config: SuccessorRecoveryUiConfig,
    ) -> Result<Self, SuccessorRecoveryUiError> {
        Ok(Self {
            snapshot,
            config: config.validate()?,
            layout_epoch: 0,
        })
    }

    #[must_use]
    pub const fn snapshot(&self) -> &SuccessorRecoveryUiSnapshot {
        &self.snapshot
    }

    pub fn replace_snapshot(&mut self, snapshot: SuccessorRecoveryUiSnapshot) {
        self.snapshot = snapshot;
        self.layout_epoch = self.layout_epoch.saturating_add(1);
    }
}

#[derive(Debug, Clone, Message)]
pub struct SuccessorRecoveryUiCommand(pub SuccessorRecoveryUiAction);

#[derive(Debug, Default, Resource)]
pub struct SuccessorRecoveryUiFocusState {
    focused_order: Option<u16>,
    ensure_visible: bool,
}

#[derive(Debug, Default, Clone, Copy, Resource, PartialEq)]
pub struct SuccessorRecoveryUiScrollState {
    offset: f32,
    max_offset: f32,
}

impl SuccessorRecoveryUiScrollState {
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

impl SuccessorRecoveryUiFocusState {
    #[must_use]
    pub const fn focused_order(&self) -> Option<u16> {
        self.focused_order
    }
}

#[derive(Debug, Default, Resource)]
pub struct SuccessorRecoveryUiReadiness {
    ready: bool,
}

impl SuccessorRecoveryUiReadiness {
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        self.ready
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SuccessorRecoveryUiError {
    #[error("successor recovery phase has no native handoff surface")]
    PhaseNotRenderable,
    #[error("successor recovery phase is missing its stored character projection")]
    MissingCharacterProjection,
    #[error("successor recovery projection does not match compiled content authority")]
    ContentAuthorityMismatch,
    #[error("native successor recovery layout is invalid")]
    InvalidLayout,
}

pub struct NativeSuccessorRecoveryPlugin;

impl Plugin for NativeSuccessorRecoveryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SuccessorRecoveryUiFocusState>()
            .init_resource::<SuccessorRecoveryUiReadiness>()
            .init_resource::<SuccessorRecoveryUiScrollState>()
            .init_resource::<render::SuccessorRecoveryUiFonts>()
            .add_message::<SuccessorRecoveryUiCommand>()
            .add_systems(
                Update,
                (
                    track_window_resize,
                    render::rebuild,
                    render::apply_fonts,
                    render::update_readiness,
                    render::handle_input,
                    render::scroll,
                    render::keep_focused_visible,
                    render::update_scrollbar,
                    render::style_buttons,
                )
                    .chain(),
            );
    }
}

#[allow(clippy::needless_pass_by_value)]
fn track_window_resize(
    mut resized: MessageReader<WindowResized>,
    view: Option<ResMut<NativeSuccessorRecoveryView>>,
) {
    if resized.read().next().is_none() {
        return;
    }
    if let Some(mut view) = view {
        view.layout_epoch = view.layout_epoch.saturating_add(1);
    }
}

#[cfg(test)]
mod tests;
