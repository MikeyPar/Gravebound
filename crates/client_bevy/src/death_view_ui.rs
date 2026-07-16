//! Reusable native Bevy surface for durable death summaries and the Hall Memorial Wall.
//!
//! This module consumes renderer-independent `death_view` projections. It never reconstructs a
//! stored death, authors a mutation, or enables successor creation. Authority:
//! `Gravebound_Production_GDD_v1_Canonical.md` (`DTH-020`, `DTH-021`, `UI-001`, `UI-002`,
//! `UI-009`-`UI-011`, `UI-030`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-HUB-001`, `CONT-HUB-002`, `CONT-LOC-001`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-06`, `GB-M03-07`).

use std::{cmp::Ordering, fs, path::Path};

use anyhow::{Context, Result, bail};
use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    window::{PrimaryWindow, WindowResized},
};
use protocol::{DeathMemorialCursorV1, DeathSummaryProjectionKindV1};
use thiserror::Error;

use crate::{
    DeathDamageEventPresentation, DeathFixedProjectionPresentation, DeathLossPresentation,
    DeathSourcePortraitPresentation, DeathSummaryAction, DeathSummaryActionPresentation,
    DeathSummaryActionState, DeathSummaryContext, DeathSummaryPresentation, DeathViewClientModel,
    DeathViewFailure, DeathViewUiCopy, MemorialDetailPhase, MemorialEntryPresentation,
    MemorialListPhase, TerminalDeathPhase,
};

pub const DEATH_PORTRAIT_RUNTIME_PATH: &str = "core/death/core_death_portraits.runtime.png";
pub const DEATH_PORTRAIT_RUNTIME_BLAKE3: &str =
    "e750553346829f5d4c0b7944da9b27ca79cfba5612f9e36e36ef707618678dd3";
pub const DEATH_FONT_REGULAR_PATH: &str = "fonts/alegreya_sans/AlegreyaSans-Regular.ttf";
pub const DEATH_FONT_REGULAR_BLAKE3: &str =
    "6c435d633146e3d45a22a0543b590cddb6d161db81055a2e93f0f43cf2d5df2a";
pub const DEATH_FONT_BOLD_PATH: &str = "fonts/alegreya_sans/AlegreyaSans-Bold.ttf";
pub const DEATH_FONT_BOLD_BLAKE3: &str =
    "c1e4ccd1cf57b5fb428f04f50d3c2532a185dcfa9ddaa2475eac146a313238d5";
pub const DEATH_PORTRAIT_CELL_PIXELS: u32 = 418;
pub const DEATH_PORTRAIT_COLUMNS: u32 = 3;
pub const DEATH_PORTRAIT_ROWS: u32 = 3;

const MIN_UI_SCALE_PERCENT: u16 = 80;
const MAX_UI_SCALE_PERCENT: u16 = 150;
const MIN_EFFECTIVE_TEXT_PX: f32 = 14.0;
const REFERENCE_HEIGHT_PX: f32 = 1_080.0;
const REFERENCE_SAFE_MARGIN_PX: f32 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathUiSurface {
    TerminalSummary,
    MemorialList,
    MemorialDetail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathUiLayoutMode {
    Minimum,
    Reference,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeathUiMetrics {
    pub layout_mode: DeathUiLayoutMode,
    pub safe_margin_px: f32,
    pub body_text_px: f32,
    pub label_text_px: f32,
    pub title_text_px: f32,
    pub section_gap_px: f32,
    pub portrait_px: f32,
}

impl DeathUiMetrics {
    pub fn for_viewport(
        width_px: f32,
        height_px: f32,
        ui_scale_percent: u16,
    ) -> Result<Self, DeathUiSnapshotError> {
        if !width_px.is_finite()
            || !height_px.is_finite()
            || width_px <= 0.0
            || height_px <= 0.0
            || !(MIN_UI_SCALE_PERCENT..=MAX_UI_SCALE_PERCENT).contains(&ui_scale_percent)
        {
            return Err(DeathUiSnapshotError::InvalidLayout);
        }
        let user_scale = f32::from(ui_scale_percent) / 100.0;
        let viewport_scale = (height_px / REFERENCE_HEIGHT_PX).clamp(2.0 / 3.0, 1.5);
        let combined = user_scale * viewport_scale;
        let minimum_layout = width_px < 1_600.0 || height_px < 900.0;
        Ok(Self {
            layout_mode: if minimum_layout {
                DeathUiLayoutMode::Minimum
            } else {
                DeathUiLayoutMode::Reference
            },
            safe_margin_px: (REFERENCE_SAFE_MARGIN_PX * combined).max(12.0),
            body_text_px: (17.0 * combined).max(MIN_EFFECTIVE_TEXT_PX),
            label_text_px: (14.0 * combined).max(MIN_EFFECTIVE_TEXT_PX),
            title_text_px: (34.0 * combined).max(26.0),
            section_gap_px: (14.0 * combined).max(8.0),
            portrait_px: if minimum_layout {
                (104.0 * combined).clamp(84.0, 116.0)
            } else {
                (128.0 * combined).clamp(112.0, 154.0)
            },
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeathUiConfig {
    pub reduced_effects: bool,
    pub ui_scale_percent: u16,
}

impl DeathUiConfig {
    pub fn validate(self) -> Result<Self, DeathUiSnapshotError> {
        if !(MIN_UI_SCALE_PERCENT..=MAX_UI_SCALE_PERCENT).contains(&self.ui_scale_percent) {
            return Err(DeathUiSnapshotError::InvalidLayout);
        }
        Ok(self)
    }
}

impl Default for DeathUiConfig {
    fn default() -> Self {
        Self {
            reduced_effects: false,
            ui_scale_percent: 100,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeathUiAction {
    Summary(DeathSummaryAction),
    MemorialEntry(DeathMemorialCursorV1),
    LoadMoreLosses,
    LoadOlderMemorials,
    Retry,
    Back,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathUiActionEmphasis {
    Primary,
    Secondary,
    MemorialRow,
    Utility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathUiActionSpec {
    pub action: DeathUiAction,
    pub label: String,
    pub enabled: bool,
    pub emphasis: DeathUiActionEmphasis,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathUiStatus {
    pub title: String,
    pub detail: Option<String>,
    pub recoverable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathUiActivity {
    Idle,
    Busy,
}

impl DeathUiActivity {
    const fn is_busy(self) -> bool {
        matches!(self, Self::Busy)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathUiTraceMode {
    Summary,
    Emphasized,
}

impl DeathUiTraceMode {
    const fn is_emphasized(self) -> bool {
        matches!(self, Self::Emphasized)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathUiAvailability {
    Unavailable,
    Available,
}

impl DeathUiAvailability {
    const fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathUiSnapshot {
    pub surface: DeathUiSurface,
    pub copy: DeathViewUiCopy,
    pub summary: Option<DeathSummaryPresentation>,
    pub memorial_entries: Vec<MemorialEntryPresentation>,
    pub status: Option<DeathUiStatus>,
    pub activity: DeathUiActivity,
    pub trace_mode: DeathUiTraceMode,
    pub load_older_memorials: DeathUiAvailability,
    pub close: DeathUiAvailability,
}

impl DeathUiSnapshot {
    pub fn terminal(model: &DeathViewClientModel) -> Result<Self, DeathUiSnapshotError> {
        let phase = model.terminal().phase();
        if phase == TerminalDeathPhase::Inactive {
            return Err(DeathUiSnapshotError::SurfaceNotOpen);
        }
        let copy = model.ui_copy().clone();
        let summary = model.terminal().summary().cloned();
        if summary
            .as_ref()
            .is_some_and(|value| value.context != DeathSummaryContext::Terminal)
        {
            return Err(DeathUiSnapshotError::InvalidSummaryContext);
        }
        let status = terminal_status(model, &copy);
        Ok(Self {
            surface: DeathUiSurface::TerminalSummary,
            copy,
            summary,
            memorial_entries: Vec::new(),
            status,
            activity: if model.pending().is_some() {
                DeathUiActivity::Busy
            } else {
                DeathUiActivity::Idle
            },
            trace_mode: DeathUiTraceMode::Summary,
            load_older_memorials: DeathUiAvailability::Unavailable,
            close: DeathUiAvailability::Unavailable,
        })
    }

    pub fn memorial_list(model: &DeathViewClientModel) -> Result<Self, DeathUiSnapshotError> {
        let phase = model.memorial().list_phase();
        if phase == MemorialListPhase::Closed {
            return Err(DeathUiSnapshotError::SurfaceNotOpen);
        }
        Ok(Self {
            surface: DeathUiSurface::MemorialList,
            copy: model.ui_copy().clone(),
            summary: None,
            memorial_entries: model.memorial().presentations().cloned().collect(),
            status: memorial_list_status(model),
            activity: if model.pending().is_some() {
                DeathUiActivity::Busy
            } else {
                DeathUiActivity::Idle
            },
            trace_mode: DeathUiTraceMode::Summary,
            load_older_memorials: if model.memorial().can_load_older() {
                DeathUiAvailability::Available
            } else {
                DeathUiAvailability::Unavailable
            },
            close: if matches!(
                phase,
                MemorialListPhase::LoadingInitial
                    | MemorialListPhase::LoadingContinuation
                    | MemorialListPhase::Refreshing
            ) {
                DeathUiAvailability::Unavailable
            } else {
                DeathUiAvailability::Available
            },
        })
    }

    pub fn memorial_detail(model: &DeathViewClientModel) -> Result<Self, DeathUiSnapshotError> {
        let phase = model.memorial().detail_phase();
        if phase == MemorialDetailPhase::Closed {
            return Err(DeathUiSnapshotError::SurfaceNotOpen);
        }
        let copy = model.ui_copy().clone();
        let summary = model.memorial().detail().cloned();
        if summary
            .as_ref()
            .is_some_and(|value| value.context != DeathSummaryContext::Memorial)
        {
            return Err(DeathUiSnapshotError::InvalidSummaryContext);
        }
        Ok(Self {
            surface: DeathUiSurface::MemorialDetail,
            copy,
            summary,
            memorial_entries: Vec::new(),
            status: memorial_detail_status(model),
            activity: if model.pending().is_some() {
                DeathUiActivity::Busy
            } else {
                DeathUiActivity::Idle
            },
            trace_mode: DeathUiTraceMode::Summary,
            load_older_memorials: DeathUiAvailability::Unavailable,
            close: if matches!(
                phase,
                MemorialDetailPhase::Loading
                    | MemorialDetailPhase::LoadingContinuation
                    | MemorialDetailPhase::Refreshing
            ) {
                DeathUiAvailability::Unavailable
            } else {
                DeathUiAvailability::Available
            },
        })
    }

    #[must_use]
    pub fn with_trace_emphasis(mut self, enabled: bool) -> Self {
        self.trace_mode = if enabled {
            DeathUiTraceMode::Emphasized
        } else {
            DeathUiTraceMode::Summary
        };
        self
    }

    #[must_use]
    pub fn actions(&self) -> Vec<DeathUiActionSpec> {
        match self.surface {
            DeathUiSurface::TerminalSummary | DeathUiSurface::MemorialDetail => {
                self.summary_actions()
            }
            DeathUiSurface::MemorialList => self.memorial_actions(),
        }
    }

    fn summary_actions(&self) -> Vec<DeathUiActionSpec> {
        let mut actions = Vec::new();
        if let Some(summary) = self.summary.as_ref() {
            actions.push(summary_action_spec(
                &summary.actions.primary,
                DeathUiActionEmphasis::Primary,
                self.activity.is_busy(),
            ));
            actions.extend(summary.actions.secondary.iter().map(|action| {
                summary_action_spec(
                    action,
                    DeathUiActionEmphasis::Secondary,
                    self.activity.is_busy(),
                )
            }));
            if summary.next_lost_ordinal.is_some() {
                actions.push(DeathUiActionSpec {
                    action: DeathUiAction::LoadMoreLosses,
                    label: self.copy.load_more_action.clone(),
                    enabled: !self.activity.is_busy(),
                    emphasis: DeathUiActionEmphasis::Utility,
                });
            }
        } else if self
            .status
            .as_ref()
            .is_some_and(|status| status.recoverable)
        {
            actions.push(DeathUiActionSpec {
                action: DeathUiAction::Retry,
                label: self.copy.retry_action.clone(),
                enabled: !self.activity.is_busy(),
                emphasis: DeathUiActionEmphasis::Primary,
            });
        }
        if self.surface == DeathUiSurface::MemorialDetail {
            actions.push(DeathUiActionSpec {
                action: DeathUiAction::Back,
                label: self.copy.back_action.clone(),
                enabled: self.close.is_available(),
                emphasis: DeathUiActionEmphasis::Utility,
            });
        }
        actions
    }

    fn memorial_actions(&self) -> Vec<DeathUiActionSpec> {
        let mut actions = self
            .memorial_entries
            .iter()
            .map(|entry| DeathUiActionSpec {
                action: DeathUiAction::MemorialEntry(entry.authority.cursor),
                label: entry.authority.character_name_snapshot.as_str().to_owned(),
                enabled: !self.activity.is_busy(),
                emphasis: DeathUiActionEmphasis::MemorialRow,
            })
            .collect::<Vec<_>>();
        if self.load_older_memorials.is_available() {
            actions.push(DeathUiActionSpec {
                action: DeathUiAction::LoadOlderMemorials,
                label: self.copy.load_more_action.clone(),
                enabled: !self.activity.is_busy(),
                emphasis: DeathUiActionEmphasis::Utility,
            });
        }
        if self
            .status
            .as_ref()
            .is_some_and(|status| status.recoverable)
        {
            actions.push(DeathUiActionSpec {
                action: DeathUiAction::Retry,
                label: self.copy.retry_action.clone(),
                enabled: !self.activity.is_busy(),
                emphasis: DeathUiActionEmphasis::Utility,
            });
        }
        actions.push(DeathUiActionSpec {
            action: DeathUiAction::Back,
            label: self.copy.back_action.clone(),
            enabled: self.close.is_available(),
            emphasis: DeathUiActionEmphasis::Utility,
        });
        actions
    }

    fn validate_portrait_assets(&self) -> Result<(), DeathUiSnapshotError> {
        let Some(summary) = self.summary.as_ref() else {
            return Ok(());
        };
        validate_portrait_mapping(&summary.lethal_cause.killer.portrait)?;
        for event in &summary.timeline.events {
            validate_portrait_mapping(&event.source.portrait)?;
        }
        Ok(())
    }

    #[must_use]
    pub fn semantic_signature(&self) -> String {
        let mut parts = vec![format!("{:?}", self.surface)];
        if let Some(summary) = self.summary.as_ref() {
            parts.extend([
                summary.eyebrow.clone(),
                summary.title.clone(),
                summary.hero.character_name.clone(),
                summary.lethal_cause.killer.value.label.clone(),
                summary.timeline.section_title.clone(),
                summary.network.network.label.clone(),
                summary.lost_section_title.clone(),
                summary.preserved_section_title.clone(),
                summary.created_section_title.clone(),
                summary.echo_outcome.label.clone(),
            ]);
        }
        parts.extend(self.memorial_entries.iter().flat_map(|entry| {
            [
                entry.authority.character_name_snapshot.as_str().to_owned(),
                entry.formatted_death_at.clone(),
                entry.presentation.label.clone(),
            ]
        }));
        if let Some(status) = self.status.as_ref() {
            parts.push(status.title.clone());
            parts.extend(status.detail.clone());
        }
        parts.extend(self.actions().into_iter().map(|action| action.label));
        parts.join("\u{001f}")
    }
}

fn summary_action_spec(
    action: &DeathSummaryActionPresentation,
    emphasis: DeathUiActionEmphasis,
    busy: bool,
) -> DeathUiActionSpec {
    DeathUiActionSpec {
        action: DeathUiAction::Summary(action.action),
        label: action.label.clone(),
        enabled: action.state == DeathSummaryActionState::Enabled && !busy,
        emphasis,
    }
}

fn terminal_status(model: &DeathViewClientModel, copy: &DeathViewUiCopy) -> Option<DeathUiStatus> {
    if let Some(failure) = model.terminal().failure() {
        return Some(status_from_failure(failure));
    }
    match model.terminal().phase() {
        TerminalDeathPhase::PossibleDeathObserved
        | TerminalDeathPhase::AwaitingDurableAcknowledgement => Some(DeathUiStatus {
            title: copy.awaiting_commit.clone(),
            detail: Some(copy.awaiting_commit_detail.clone()),
            recoverable: false,
        }),
        TerminalDeathPhase::LoadingLatest | TerminalDeathPhase::LoadingSummary => {
            Some(DeathUiStatus {
                title: copy.loading_summary.clone(),
                detail: None,
                recoverable: false,
            })
        }
        TerminalDeathPhase::SummaryReady if model.pending().is_some() => Some(DeathUiStatus {
            title: copy.loading_summary.clone(),
            detail: None,
            recoverable: false,
        }),
        _ => None,
    }
}

fn memorial_list_status(model: &DeathViewClientModel) -> Option<DeathUiStatus> {
    if let Some(failure) = model.memorial().failure() {
        return Some(status_from_failure(failure));
    }
    let copy = model.ui_copy();
    match model.memorial().list_phase() {
        MemorialListPhase::LoadingInitial
        | MemorialListPhase::LoadingContinuation
        | MemorialListPhase::Refreshing => Some(DeathUiStatus {
            title: copy.loading_memorial.clone(),
            detail: None,
            recoverable: false,
        }),
        MemorialListPhase::Empty => Some(DeathUiStatus {
            title: copy.memorial_empty.clone(),
            detail: None,
            recoverable: false,
        }),
        _ => None,
    }
}

fn memorial_detail_status(model: &DeathViewClientModel) -> Option<DeathUiStatus> {
    if let Some(failure) = model.memorial().failure() {
        return Some(status_from_failure(failure));
    }
    match model.memorial().detail_phase() {
        MemorialDetailPhase::Loading
        | MemorialDetailPhase::LoadingContinuation
        | MemorialDetailPhase::Refreshing => Some(DeathUiStatus {
            title: model.ui_copy().loading_summary.clone(),
            detail: None,
            recoverable: false,
        }),
        _ => None,
    }
}

fn status_from_failure(failure: &DeathViewFailure) -> DeathUiStatus {
    DeathUiStatus {
        title: failure.title.clone(),
        detail: Some(failure.detail.clone()),
        recoverable: !matches!(
            failure.retry,
            crate::DeathViewRetryDirective::Unavailable
                | crate::DeathViewRetryDirective::RestartAfterUpdate
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DeathUiSnapshotError {
    #[error("death or Memorial surface is not open")]
    SurfaceNotOpen,
    #[error("summary context does not match its native surface")]
    InvalidSummaryContext,
    #[error("native death layout settings are invalid")]
    InvalidLayout,
    #[error("native death portrait asset has no validated atlas mapping: {0}")]
    UnknownPortraitAsset(String),
}

fn validate_portrait_mapping(
    portrait: &DeathSourcePortraitPresentation,
) -> Result<(), DeathUiSnapshotError> {
    if let DeathSourcePortraitPresentation::Asset { asset_id } = portrait
        && portrait_atlas_index(asset_id).is_none()
    {
        return Err(DeathUiSnapshotError::UnknownPortraitAsset(asset_id.clone()));
    }
    Ok(())
}

#[must_use]
pub fn portrait_atlas_index(asset_id: &str) -> Option<usize> {
    match asset_id {
        "portrait.enemy.drowned_pilgrim" => Some(0),
        "portrait.enemy.mire_leech" => Some(1),
        "portrait.enemy.bell_reed" => Some(2),
        "portrait.enemy.bell_acolyte" => Some(3),
        "portrait.enemy.chain_sentry" => Some(4),
        "portrait.enemy.choir_skull" => Some(5),
        "portrait.miniboss.sepulcher_knight" => Some(6),
        "portrait.miniboss.choir_abbot" => Some(7),
        "portrait.boss.sir_caldus" => Some(8),
        _ => None,
    }
}

pub fn validate_death_portrait_atlas(asset_root: &Path) -> Result<()> {
    validate_asset_hash(
        asset_root,
        DEATH_PORTRAIT_RUNTIME_PATH,
        DEATH_PORTRAIT_RUNTIME_BLAKE3,
        "Core death portrait atlas",
    )
}

pub fn validate_death_ui_assets(asset_root: &Path) -> Result<()> {
    validate_death_portrait_atlas(asset_root)?;
    validate_asset_hash(
        asset_root,
        DEATH_FONT_REGULAR_PATH,
        DEATH_FONT_REGULAR_BLAKE3,
        "Core death regular font",
    )?;
    validate_asset_hash(
        asset_root,
        DEATH_FONT_BOLD_PATH,
        DEATH_FONT_BOLD_BLAKE3,
        "Core death bold font",
    )
}

fn validate_asset_hash(
    asset_root: &Path,
    relative_path: &str,
    expected_blake3: &str,
    label: &str,
) -> Result<()> {
    let path = asset_root.join(relative_path);
    let bytes = fs::read(&path).with_context(|| format!("missing {}", path.display()))?;
    let actual = blake3::hash(&bytes).to_hex().to_string();
    if actual != expected_blake3 {
        bail!("{label} hash mismatch: {actual}");
    }
    Ok(())
}

#[derive(Debug, Clone, Resource)]
pub struct NativeDeathView {
    snapshot: DeathUiSnapshot,
    config: DeathUiConfig,
    layout_epoch: u64,
}

impl NativeDeathView {
    pub fn new(
        snapshot: DeathUiSnapshot,
        config: DeathUiConfig,
    ) -> Result<Self, DeathUiSnapshotError> {
        snapshot.validate_portrait_assets()?;
        Ok(Self {
            snapshot,
            config: config.validate()?,
            layout_epoch: 0,
        })
    }

    #[must_use]
    pub const fn snapshot(&self) -> &DeathUiSnapshot {
        &self.snapshot
    }

    pub fn replace_snapshot(
        &mut self,
        snapshot: DeathUiSnapshot,
    ) -> Result<(), DeathUiSnapshotError> {
        snapshot.validate_portrait_assets()?;
        self.snapshot = snapshot;
        self.layout_epoch = self.layout_epoch.saturating_add(1);
        Ok(())
    }

    pub fn set_trace_emphasis(&mut self, enabled: bool) {
        self.snapshot.trace_mode = if enabled {
            DeathUiTraceMode::Emphasized
        } else {
            DeathUiTraceMode::Summary
        };
        self.layout_epoch = self.layout_epoch.saturating_add(1);
    }
}

#[derive(Debug, Clone, Message)]
pub struct DeathUiCommand(pub DeathUiAction);

#[derive(Debug, Clone, Copy, Message, PartialEq, Eq)]
pub enum DeathUiFocusRequest {
    Previous,
    Next,
}

#[derive(Debug, Clone, Copy, Message, PartialEq, Eq)]
pub enum DeathUiScrollRequest {
    Start,
    End,
}

#[derive(Debug, Default, Resource)]
pub struct DeathUiFocusState {
    focused_order: Option<u16>,
    ensure_visible: bool,
}

impl DeathUiFocusState {
    /// Returns the semantic action order currently owned by keyboard/controller focus.
    ///
    /// Callers use this read-only projection instead of attempting to infer focus from render
    /// colors, hover state, or widget-tree implementation details.
    #[must_use]
    pub const fn focused_order(&self) -> Option<u16> {
        self.focused_order
    }
}

#[derive(Debug, Default, Resource)]
pub struct DeathUiRenderReadiness {
    ready: bool,
}

#[derive(Debug, Default, Clone, Copy, Resource, PartialEq)]
pub struct DeathUiScrollState {
    offset: f32,
    max_offset: f32,
}

impl DeathUiScrollState {
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

impl DeathUiRenderReadiness {
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        self.ready
    }
}

#[derive(Debug, Component)]
struct DeathUiRoot;

#[derive(Debug, Component)]
struct DeathUiScrollRoot;

#[derive(Debug, Component)]
struct DeathUiScrollTrack;

#[derive(Debug, Component)]
struct DeathUiScrollThumb;

#[derive(Debug, Clone, Component)]
struct DeathUiButton {
    action: DeathUiAction,
    enabled: bool,
    emphasis: DeathUiActionEmphasis,
    order: u16,
}

/// Stable keyboard/controller focus metadata retained even before certification.
#[derive(Debug, Clone, Copy, Component, PartialEq, Eq)]
pub struct DeathUiFocusOrder(pub u16);

#[derive(Debug, Clone, Copy, Component, PartialEq, Eq)]
enum DeathUiFontWeight {
    Regular,
    Bold,
}

#[derive(Debug, Resource)]
struct DeathUiFonts {
    regular: Handle<Font>,
    bold: Handle<Font>,
    settled: bool,
}

impl FromWorld for DeathUiFonts {
    fn from_world(world: &mut World) -> Self {
        let assets = world.resource::<AssetServer>();
        Self {
            regular: assets.load(DEATH_FONT_REGULAR_PATH),
            bold: assets.load(DEATH_FONT_BOLD_PATH),
            settled: false,
        }
    }
}

pub struct NativeDeathViewPlugin;

impl Plugin for NativeDeathViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DeathUiFocusState>()
            .init_resource::<DeathUiFonts>()
            .init_resource::<DeathUiRenderReadiness>()
            .init_resource::<DeathUiScrollState>()
            .add_message::<DeathUiCommand>()
            .add_message::<DeathUiFocusRequest>()
            .add_message::<DeathUiScrollRequest>()
            .add_systems(
                Update,
                (
                    track_death_ui_window_resize,
                    rebuild_native_death_view,
                    apply_death_ui_fonts,
                    stabilize_death_ui_font_layout,
                    update_death_ui_render_readiness,
                    handle_death_ui_focus_and_activation,
                    scroll_death_ui,
                    keep_focused_death_action_visible,
                    update_death_ui_scrollbar,
                    style_death_ui_buttons,
                )
                    .chain(),
            );
    }
}

#[allow(clippy::needless_pass_by_value)]
fn stabilize_death_ui_font_layout(
    assets: Res<AssetServer>,
    mut fonts: ResMut<DeathUiFonts>,
    mut scroll_roots: Query<&mut ScrollPosition, With<DeathUiScrollRoot>>,
) {
    if fonts.settled {
        return;
    }
    for mut scroll in &mut scroll_roots {
        scroll.x = 0.0;
        scroll.y = 0.0;
    }
    fonts.settled = assets.is_loaded_with_dependencies(fonts.regular.id())
        && assets.is_loaded_with_dependencies(fonts.bold.id());
}

#[allow(clippy::needless_pass_by_value)]
fn update_death_ui_render_readiness(
    fonts: Res<DeathUiFonts>,
    roots: Query<&ComputedNode, With<DeathUiRoot>>,
    texts: Query<&ComputedNode, With<DeathUiFontWeight>>,
    mut readiness: ResMut<DeathUiRenderReadiness>,
) {
    let root_has_layout = roots
        .iter()
        .any(|node| node.size().x > 0.0 && node.size().y > 0.0);
    let text_has_layout = texts
        .iter()
        .any(|node| node.size().x > 0.0 && node.size().y > 0.0);
    let ready = render_layout_is_ready(fonts.settled, root_has_layout, text_has_layout);
    if readiness.ready != ready {
        readiness.ready = ready;
    }
}

const fn render_layout_is_ready(
    fonts_settled: bool,
    root_has_layout: bool,
    text_has_layout: bool,
) -> bool {
    fonts_settled && root_has_layout && text_has_layout
}

#[allow(clippy::needless_pass_by_value)]
fn apply_death_ui_fonts(
    fonts: Res<DeathUiFonts>,
    mut texts: Query<(&DeathUiFontWeight, &mut TextFont), Added<DeathUiFontWeight>>,
) {
    for (weight, mut text_font) in &mut texts {
        text_font.font = FontSource::Handle(match weight {
            DeathUiFontWeight::Regular => fonts.regular.clone(),
            DeathUiFontWeight::Bold => fonts.bold.clone(),
        });
    }
}

#[allow(clippy::needless_pass_by_value)]
fn track_death_ui_window_resize(
    mut resized: MessageReader<WindowResized>,
    mut view: ResMut<NativeDeathView>,
) {
    if resized.read().next().is_some() {
        view.layout_epoch = view.layout_epoch.saturating_add(1);
    }
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn rebuild_native_death_view(
    mut commands: Commands,
    view: Res<NativeDeathView>,
    windows: Query<&Window, With<PrimaryWindow>>,
    roots: Query<Entity, With<DeathUiRoot>>,
    assets: Res<AssetServer>,
    mut atlases: ResMut<Assets<TextureAtlasLayout>>,
    mut focus: ResMut<DeathUiFocusState>,
    mut readiness: ResMut<DeathUiRenderReadiness>,
) {
    if !view.is_changed() && !roots.is_empty() {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok(metrics) = DeathUiMetrics::for_viewport(
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
    let texture = assets.load(DEATH_PORTRAIT_RUNTIME_PATH);
    let atlas = atlases.add(TextureAtlasLayout::from_grid(
        UVec2::splat(DEATH_PORTRAIT_CELL_PIXELS),
        DEATH_PORTRAIT_COLUMNS,
        DEATH_PORTRAIT_ROWS,
        None,
        None,
    ));
    focus.focused_order = None;
    focus.ensure_visible = false;
    spawn_native_death_view(
        &mut commands,
        &view.snapshot,
        view.config,
        metrics,
        &texture,
        &atlas,
    );
}

fn spawn_native_death_view(
    commands: &mut Commands,
    snapshot: &DeathUiSnapshot,
    config: DeathUiConfig,
    metrics: DeathUiMetrics,
    texture: &Handle<Image>,
    atlas: &Handle<TextureAtlasLayout>,
) {
    commands
        .spawn((
            Name::new("Durable death presentation surface"),
            DeathUiRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                padding: UiRect::all(px(metrics.safe_margin_px)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgb_u8(5, 7, 8)),
            GlobalZIndex(100),
        ))
        .with_children(|root| {
            spawn_ambient_layers(root, config.reduced_effects);
            root.spawn((
                Name::new("Death presentation panel"),
                Node {
                    width: percent(100),
                    height: percent(100),
                    max_width: px(1_760),
                    max_height: px(1_040),
                    flex_direction: FlexDirection::Column,
                    border: UiRect::all(px(1)),
                    ..default()
                },
                BackgroundColor(Color::srgba_u8(10, 12, 13, 250)),
                BorderColor::all(Color::srgb_u8(105, 84, 49)),
                BoxShadow::new(Color::srgba_u8(0, 0, 0, 210), px(0), px(12), px(28), px(3)),
            ))
            .with_children(|panel| match snapshot.surface {
                DeathUiSurface::TerminalSummary => {
                    spawn_summary_surface(panel, snapshot, metrics, texture, atlas, false);
                }
                DeathUiSurface::MemorialList => {
                    spawn_memorial_list_surface(panel, snapshot, metrics);
                }
                DeathUiSurface::MemorialDetail => {
                    spawn_summary_surface(panel, snapshot, metrics, texture, atlas, true);
                }
            });
        });
}

fn spawn_ambient_layers(parent: &mut ChildSpawnerCommands, reduced_effects: bool) {
    for (side, color) in [
        (true, Color::srgba_u8(115, 75, 32, 34)),
        (false, Color::srgba_u8(42, 86, 78, 26)),
    ] {
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: if side { percent(4) } else { percent(69) },
                top: if side { percent(6) } else { percent(56) },
                width: percent(27),
                height: percent(38),
                border_radius: BorderRadius::all(percent(50)),
                ..default()
            },
            BackgroundColor(if reduced_effects {
                color.with_alpha(0.035)
            } else {
                color
            }),
        ));
    }
    for offset in [16.0, 50.0, 84.0] {
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(offset),
                top: percent(2),
                width: px(1),
                height: percent(96),
                ..default()
            },
            BackgroundColor(Color::srgba_u8(123, 105, 74, 16)),
        ));
    }
}

fn spawn_scrollbar(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            DeathUiScrollTrack,
            Visibility::Hidden,
            Node {
                position_type: PositionType::Absolute,
                right: px(4),
                top: px(8),
                bottom: px(8),
                width: px(7),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(Color::srgba_u8(54, 49, 39, 210)),
            ZIndex(10),
        ))
        .with_children(|track| {
            track.spawn((
                DeathUiScrollThumb,
                Node {
                    position_type: PositionType::Absolute,
                    top: percent(0),
                    width: percent(100),
                    height: percent(28),
                    border_radius: BorderRadius::all(px(3)),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(225, 189, 117)),
            ));
        });
}

fn spawn_summary_surface(
    parent: &mut ChildSpawnerCommands,
    snapshot: &DeathUiSnapshot,
    metrics: DeathUiMetrics,
    texture: &Handle<Image>,
    atlas: &Handle<TextureAtlasLayout>,
    historical: bool,
) {
    if let Some(summary) = snapshot.summary.as_ref() {
        spawn_summary_header(parent, summary, metrics, historical);
    } else {
        spawn_state_header(parent, snapshot, metrics);
    }
    parent
        .spawn(Node {
            width: percent(100),
            height: px(0),
            min_height: px(0),
            flex_grow: 1.0,
            flex_basis: px(0),
            position_type: PositionType::Relative,
            overflow: Overflow::clip(),
            ..default()
        })
        .with_children(|viewport| {
            viewport
                .spawn((
                    DeathUiScrollRoot,
                    ScrollPosition::default(),
                    Node {
                        width: percent(100),
                        height: percent(100),
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::scroll_y(),
                        padding: UiRect {
                            left: px(metrics.section_gap_px),
                            right: px(metrics.section_gap_px + 8.0),
                            top: px(metrics.section_gap_px),
                            bottom: px(metrics.section_gap_px),
                        },
                        row_gap: px(metrics.section_gap_px),
                        ..default()
                    },
                ))
                .with_children(|content| {
                    if let Some(summary) = snapshot.summary.as_ref() {
                        spawn_hero_and_cause(content, snapshot, summary, metrics, texture, atlas);
                        spawn_timeline(content, snapshot, summary, metrics, texture, atlas);
                        spawn_network(content, snapshot, summary, metrics);
                        spawn_fate_columns(content, snapshot, summary, metrics);
                        spawn_summary_actions(content, snapshot, metrics);
                    } else if let Some(status) = snapshot.status.as_ref() {
                        spawn_status_card(content, status, metrics);
                        spawn_summary_actions(content, snapshot, metrics);
                    }
                });
            spawn_scrollbar(viewport);
        });
}

fn spawn_summary_header(
    parent: &mut ChildSpawnerCommands,
    summary: &DeathSummaryPresentation,
    metrics: DeathUiMetrics,
    historical: bool,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(if metrics.layout_mode == DeathUiLayoutMode::Minimum {
                    78
                } else {
                    104
                }),
                padding: UiRect::axes(px(metrics.section_gap_px * 1.4), px(12)),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(13, 15, 16)),
            BorderColor::all(Color::srgb_u8(84, 68, 42)),
            ZIndex(20),
        ))
        .with_children(|header| {
            header
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(3),
                    ..default()
                })
                .with_children(|titles| {
                    spawn_text(
                        titles,
                        &summary.eyebrow,
                        metrics.label_text_px,
                        Color::srgb_u8(185, 138, 77),
                    );
                    spawn_strong_text(
                        titles,
                        &summary.title,
                        metrics.title_text_px,
                        Color::srgb_u8(239, 229, 201),
                    );
                });
            header
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::End,
                    row_gap: px(4),
                    ..default()
                })
                .with_children(|date| {
                    if historical {
                        spawn_text(
                            date,
                            &summary.hero.hero_label.label,
                            metrics.label_text_px,
                            Color::srgb_u8(139, 166, 154),
                        );
                    }
                    spawn_text(
                        date,
                        &summary.formatted_death_at,
                        metrics.label_text_px,
                        Color::srgb_u8(170, 166, 151),
                    );
                });
        });
}

fn spawn_state_header(
    parent: &mut ChildSpawnerCommands,
    snapshot: &DeathUiSnapshot,
    metrics: DeathUiMetrics,
) {
    let title = snapshot
        .status
        .as_ref()
        .map_or(snapshot.copy.memorial_title.as_str(), |status| {
            status.title.as_str()
        });
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(96),
                padding: UiRect::all(px(metrics.section_gap_px * 1.4)),
                align_items: AlignItems::Center,
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(13, 15, 16)),
            BorderColor::all(Color::srgb_u8(84, 68, 42)),
            ZIndex(20),
        ))
        .with_children(|header| {
            spawn_strong_text(
                header,
                title,
                metrics.title_text_px,
                Color::srgb_u8(239, 229, 201),
            );
        });
}

fn spawn_hero_and_cause(
    parent: &mut ChildSpawnerCommands,
    snapshot: &DeathUiSnapshot,
    summary: &DeathSummaryPresentation,
    metrics: DeathUiMetrics,
    texture: &Handle<Image>,
    atlas: &Handle<TextureAtlasLayout>,
) {
    parent
        .spawn(Node {
            width: percent(100),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Row,
            column_gap: px(metrics.section_gap_px),
            ..default()
        })
        .with_children(|row| {
            spawn_hero_card(row, snapshot, summary, metrics);
            spawn_cause_card(row, snapshot, summary, metrics, texture, atlas);
        });
}

fn spawn_hero_card(
    parent: &mut ChildSpawnerCommands,
    snapshot: &DeathUiSnapshot,
    summary: &DeathSummaryPresentation,
    metrics: DeathUiMetrics,
) {
    parent
        .spawn((
            Node {
                width: percent(43),
                min_height: px(metrics.portrait_px + 38.0),
                padding: UiRect::all(px(12)),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(15, 18, 18)),
            BorderColor::all(Color::srgb_u8(62, 59, 48)),
        ))
        .with_children(|hero| {
            spawn_section_title(hero, &summary.hero.section_title, metrics, false);
            spawn_strong_text(
                hero,
                &summary.hero.character_name,
                metrics.body_text_px * 1.35,
                Color::srgb_u8(243, 232, 202),
            );
            spawn_text(
                hero,
                &summary.hero.hero_label.label,
                metrics.label_text_px,
                Color::srgb_u8(179, 151, 102),
            );
            spawn_field(
                hero,
                &snapshot.copy.fields.class,
                &summary.hero.class.label,
                metrics,
            );
            spawn_field(
                hero,
                &snapshot.copy.fields.level,
                &summary.hero.level.to_string(),
                metrics,
            );
            spawn_field(
                hero,
                &snapshot.copy.fields.lifetime,
                &summary.hero.formatted_lifetime,
                metrics,
            );
            spawn_field(
                hero,
                &snapshot.copy.fields.final_deed,
                &summary.hero.final_deed.label,
                metrics,
            );
        });
}

fn spawn_cause_card(
    parent: &mut ChildSpawnerCommands,
    snapshot: &DeathUiSnapshot,
    summary: &DeathSummaryPresentation,
    metrics: DeathUiMetrics,
    texture: &Handle<Image>,
    atlas: &Handle<TextureAtlasLayout>,
) {
    parent
        .spawn((
            Node {
                flex_grow: 1.0,
                min_height: px(metrics.portrait_px + 38.0),
                padding: UiRect::all(px(12)),
                flex_direction: FlexDirection::Column,
                row_gap: px(7),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(16, 18, 18)),
            BorderColor::all(Color::srgb_u8(83, 63, 41)),
        ))
        .with_children(|cause| {
            spawn_section_title(cause, &summary.lethal_cause.section_title, metrics, true);
            cause
                .spawn(Node {
                    width: percent(100),
                    flex_direction: FlexDirection::Row,
                    column_gap: px(14),
                    ..default()
                })
                .with_children(|body| {
                    spawn_source_portrait(
                        body,
                        &summary.lethal_cause.killer.portrait,
                        texture,
                        atlas,
                        metrics.portrait_px,
                    );
                    body.spawn(Node {
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(6),
                        ..default()
                    })
                    .with_children(|fields| {
                        spawn_cause_fields(fields, snapshot, summary, metrics);
                    });
                });
        });
}

fn spawn_cause_fields(
    parent: &mut ChildSpawnerCommands,
    snapshot: &DeathUiSnapshot,
    summary: &DeathSummaryPresentation,
    metrics: DeathUiMetrics,
) {
    if let Some(cause_value) = summary.lethal_cause.cause.as_ref() {
        spawn_field(
            parent,
            &snapshot.copy.fields.cause,
            &cause_value.label,
            metrics,
        );
    }
    spawn_field(
        parent,
        &snapshot.copy.fields.killer,
        &summary.lethal_cause.killer.value.label,
        metrics,
    );
    spawn_field(
        parent,
        &snapshot.copy.fields.attack,
        &summary.lethal_cause.attack.label,
        metrics,
    );
    spawn_field(
        parent,
        &snapshot.copy.fields.damage,
        &summary.lethal_cause.formatted_final_damage,
        metrics,
    );
    spawn_field(
        parent,
        &snapshot.copy.fields.damage_type,
        &summary.lethal_cause.damage_type.label,
        metrics,
    );
    spawn_field(
        parent,
        &snapshot.copy.fields.source_position,
        &summary.lethal_cause.formatted_source_position,
        metrics,
    );
}

fn spawn_timeline(
    parent: &mut ChildSpawnerCommands,
    snapshot: &DeathUiSnapshot,
    summary: &DeathSummaryPresentation,
    metrics: DeathUiMetrics,
    texture: &Handle<Image>,
    atlas: &Handle<TextureAtlasLayout>,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_shrink: 0.0,
                padding: UiRect::all(px(12)),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                border: UiRect::all(px(if snapshot.trace_mode.is_emphasized() {
                    2
                } else {
                    1
                })),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(14, 17, 17)),
            BorderColor::all(if snapshot.trace_mode.is_emphasized() {
                Color::srgb_u8(194, 142, 70)
            } else {
                Color::srgb_u8(60, 60, 51)
            }),
        ))
        .with_children(|timeline| {
            spawn_section_title(
                timeline,
                &summary.timeline.section_title,
                metrics,
                snapshot.trace_mode.is_emphasized(),
            );
            for event in &summary.timeline.events {
                spawn_timeline_event(timeline, snapshot, event, metrics, texture, atlas);
            }
        });
}

fn spawn_timeline_event(
    parent: &mut ChildSpawnerCommands,
    snapshot: &DeathUiSnapshot,
    event: &DeathDamageEventPresentation,
    metrics: DeathUiMetrics,
    texture: &Handle<Image>,
    atlas: &Handle<TextureAtlasLayout>,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(if snapshot.trace_mode.is_emphasized() {
                    48
                } else {
                    35
                }),
                padding: UiRect::axes(px(8), px(5)),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(10),
                border: UiRect::left(px(if event.lethal { 3 } else { 1 })),
                ..default()
            },
            BackgroundColor(if event.lethal {
                Color::srgb_u8(32, 22, 19)
            } else {
                Color::srgb_u8(18, 21, 21)
            }),
            BorderColor::all(if event.lethal {
                Color::srgb_u8(187, 99, 71)
            } else {
                Color::srgb_u8(50, 54, 51)
            }),
        ))
        .with_children(|row| {
            if snapshot.trace_mode.is_emphasized() {
                spawn_source_portrait(row, &event.source.portrait, texture, atlas, 36.0);
            }
            spawn_timeline_cell(
                row,
                &event.source.value.label,
                percent(20),
                metrics,
                Color::srgb_u8(211, 200, 172),
            );
            spawn_timeline_cell(
                row,
                &event.attack.label,
                percent(26),
                metrics,
                Color::srgb_u8(199, 195, 178),
            );
            spawn_timeline_cell(
                row,
                &event.formatted_final_damage,
                percent(14),
                metrics,
                if event.lethal {
                    Color::srgb_u8(238, 139, 105)
                } else {
                    Color::srgb_u8(213, 174, 114)
                },
            );
            spawn_timeline_cell(
                row,
                &event.damage_type.label,
                percent(15),
                metrics,
                Color::srgb_u8(155, 183, 171),
            );
            spawn_timeline_cell(
                row,
                &event.formatted_source_position,
                percent(23),
                metrics,
                Color::srgb_u8(142, 148, 139),
            );
        });
}

fn spawn_timeline_cell(
    parent: &mut ChildSpawnerCommands,
    value: &str,
    width: Val,
    metrics: DeathUiMetrics,
    color: Color,
) {
    parent.spawn((
        Text::new(value),
        TextFont::from_font_size(metrics.label_text_px),
        DeathUiFontWeight::Regular,
        TextColor(color),
        Node {
            width,
            max_width: width,
            overflow: Overflow::clip(),
            ..default()
        },
    ));
}

fn spawn_network(
    parent: &mut ChildSpawnerCommands,
    snapshot: &DeathUiSnapshot,
    summary: &DeathSummaryPresentation,
    metrics: DeathUiMetrics,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_shrink: 0.0,
                padding: UiRect::all(px(10)),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(14, 18, 18)),
            BorderColor::all(Color::srgb_u8(52, 66, 61)),
        ))
        .with_children(|network| {
            spawn_section_title(network, &summary.network.section_title, metrics, false);
            network
                .spawn(Node {
                    width: percent(100),
                    flex_direction: FlexDirection::Row,
                    column_gap: px(24),
                    ..default()
                })
                .with_children(|row| {
                    spawn_field(
                        row,
                        &snapshot.copy.fields.network,
                        &summary.network.network.label,
                        metrics,
                    );
                    spawn_field(
                        row,
                        &snapshot.copy.fields.recall,
                        &summary.network.recall.label,
                        metrics,
                    );
                });
        });
}

fn spawn_fate_columns(
    parent: &mut ChildSpawnerCommands,
    snapshot: &DeathUiSnapshot,
    summary: &DeathSummaryPresentation,
    metrics: DeathUiMetrics,
) {
    parent
        .spawn(Node {
            width: percent(100),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Row,
            column_gap: px(metrics.section_gap_px),
            ..default()
        })
        .with_children(|row| {
            spawn_loss_card(row, snapshot, summary, metrics);
            spawn_fixed_card(
                row,
                &summary.preserved_section_title,
                &summary.preserved,
                metrics,
                Color::srgb_u8(86, 139, 115),
            );
            spawn_created_card(row, summary, metrics);
        });
}

fn spawn_loss_card(
    parent: &mut ChildSpawnerCommands,
    _snapshot: &DeathUiSnapshot,
    summary: &DeathSummaryPresentation,
    metrics: DeathUiMetrics,
) {
    parent
        .spawn((
            Node {
                width: percent(33),
                min_height: px(126),
                padding: UiRect::all(px(10)),
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(20, 15, 14)),
            BorderColor::all(Color::srgb_u8(126, 68, 55)),
        ))
        .with_children(|card| {
            spawn_section_title(card, &summary.lost_section_title, metrics, true);
            for loss in &summary.lost {
                let (name, quantity) = match loss {
                    DeathLossPresentation::Item {
                        item,
                        formatted_quantity,
                        ..
                    } => (&item.label, formatted_quantity),
                    DeathLossPresentation::RunMaterial {
                        material,
                        formatted_quantity,
                        ..
                    } => (&material.label, formatted_quantity),
                };
                spawn_value_and_quantity(card, name, quantity, metrics);
            }
        });
}

fn spawn_fixed_card(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    entries: &[DeathFixedProjectionPresentation],
    metrics: DeathUiMetrics,
    accent: Color,
) {
    parent
        .spawn((
            Node {
                width: percent(33),
                min_height: px(126),
                padding: UiRect::all(px(10)),
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(15, 19, 18)),
            BorderColor::all(accent.with_alpha(0.72)),
        ))
        .with_children(|card| {
            spawn_section_title(card, title, metrics, false);
            for entry in entries {
                spawn_value_and_quantity(
                    card,
                    &entry.value.label,
                    &entry.formatted_quantity,
                    metrics,
                );
            }
        });
}

fn spawn_created_card(
    parent: &mut ChildSpawnerCommands,
    summary: &DeathSummaryPresentation,
    metrics: DeathUiMetrics,
) {
    parent
        .spawn((
            Node {
                width: percent(33),
                min_height: px(126),
                padding: UiRect::all(px(10)),
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(15, 19, 18)),
            BorderColor::all(Color::srgb_u8(135, 105, 164).with_alpha(0.72)),
        ))
        .with_children(|card| {
            spawn_section_title(card, &summary.created_section_title, metrics, false);
            for entry in &summary.created {
                let detail = created_projection_detail(entry, &summary.echo_outcome.label);
                spawn_value_and_quantity(card, &entry.value.label, detail, metrics);
            }
        });
}

fn created_projection_detail<'a>(
    entry: &'a DeathFixedProjectionPresentation,
    echo_outcome_label: &'a str,
) -> &'a str {
    if entry.kind == DeathSummaryProjectionKindV1::CreatedEcho {
        echo_outcome_label
    } else {
        &entry.formatted_quantity
    }
}

fn spawn_value_and_quantity(
    parent: &mut ChildSpawnerCommands,
    value: &str,
    quantity: &str,
    metrics: DeathUiMetrics,
) {
    parent
        .spawn(Node {
            width: percent(100),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        })
        .with_children(|row| {
            spawn_text(
                row,
                value,
                metrics.label_text_px,
                Color::srgb_u8(204, 201, 184),
            );
            spawn_text(
                row,
                quantity,
                metrics.label_text_px,
                Color::srgb_u8(143, 145, 136),
            );
        });
}

fn spawn_summary_actions(
    parent: &mut ChildSpawnerCommands,
    snapshot: &DeathUiSnapshot,
    metrics: DeathUiMetrics,
) {
    let actions = snapshot.actions();
    if actions.is_empty() {
        return;
    }
    parent
        .spawn(Node {
            width: percent(100),
            min_height: px(88),
            flex_shrink: 0.0,
            padding: UiRect::axes(px(2), px(8)),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            column_gap: px(10),
            flex_wrap: FlexWrap::Wrap,
            ..default()
        })
        .with_children(|row| {
            for (order, action) in actions.iter().enumerate() {
                spawn_action_button(
                    row,
                    action,
                    u16::try_from(order).unwrap_or(u16::MAX),
                    metrics,
                );
            }
        });
    if let Some(unavailable) = snapshot
        .summary
        .as_ref()
        .and_then(|summary| summary.actions.primary.unavailable_detail.as_ref())
    {
        spawn_text(
            parent,
            unavailable,
            metrics.label_text_px,
            Color::srgb_u8(128, 128, 119),
        );
    }
}

fn spawn_memorial_list_surface(
    parent: &mut ChildSpawnerCommands,
    snapshot: &DeathUiSnapshot,
    metrics: DeathUiMetrics,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(if metrics.layout_mode == DeathUiLayoutMode::Minimum {
                    78
                } else {
                    104
                }),
                padding: UiRect::axes(px(metrics.section_gap_px * 1.4), px(12)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(13, 15, 16)),
            BorderColor::all(Color::srgb_u8(84, 68, 42)),
            ZIndex(20),
        ))
        .with_children(|header| {
            spawn_strong_text(
                header,
                &snapshot.copy.memorial_title,
                metrics.title_text_px,
                Color::srgb_u8(239, 229, 201),
            );
            if let Some(status) = snapshot.status.as_ref() {
                spawn_text(
                    header,
                    &status.title,
                    metrics.label_text_px,
                    if status.recoverable {
                        Color::srgb_u8(213, 148, 87)
                    } else {
                        Color::srgb_u8(141, 170, 157)
                    },
                );
            }
        });
    parent
        .spawn(Node {
            width: percent(100),
            height: px(0),
            min_height: px(0),
            flex_grow: 1.0,
            flex_basis: px(0),
            position_type: PositionType::Relative,
            overflow: Overflow::clip(),
            ..default()
        })
        .with_children(|viewport| {
            viewport
                .spawn((
                    DeathUiScrollRoot,
                    ScrollPosition::default(),
                    Node {
                        width: percent(100),
                        height: percent(100),
                        padding: UiRect {
                            left: px(metrics.section_gap_px),
                            right: px(metrics.section_gap_px + 8.0),
                            top: px(metrics.section_gap_px),
                            bottom: px(metrics.section_gap_px),
                        },
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::scroll_y(),
                        row_gap: px(8),
                        ..default()
                    },
                ))
                .with_children(|list| {
                    spawn_memorial_list_content(list, snapshot, metrics);
                });
            spawn_scrollbar(viewport);
        });
}

fn spawn_memorial_list_content(
    parent: &mut ChildSpawnerCommands,
    snapshot: &DeathUiSnapshot,
    metrics: DeathUiMetrics,
) {
    if snapshot.memorial_entries.is_empty() {
        if let Some(status) = snapshot.status.as_ref() {
            spawn_status_card(parent, status, metrics);
        }
    } else {
        let actions = snapshot.actions();
        for (index, entry) in snapshot.memorial_entries.iter().enumerate() {
            let Some(action) = actions.get(index) else {
                break;
            };
            spawn_memorial_row(
                parent,
                entry,
                action,
                u16::try_from(index).unwrap_or(u16::MAX),
                metrics,
            );
        }
        if let Some(status) = snapshot.status.as_ref() {
            spawn_status_card(parent, status, metrics);
        }
    }
    let row_count = snapshot.memorial_entries.len();
    for (offset, action) in snapshot.actions().iter().skip(row_count).enumerate() {
        spawn_action_button(
            parent,
            action,
            u16::try_from(row_count.saturating_add(offset)).unwrap_or(u16::MAX),
            metrics,
        );
    }
}

fn spawn_memorial_row(
    parent: &mut ChildSpawnerCommands,
    entry: &MemorialEntryPresentation,
    action: &DeathUiActionSpec,
    order: u16,
    metrics: DeathUiMetrics,
) {
    parent
        .spawn((
            Button,
            DeathUiButton {
                action: action.action.clone(),
                enabled: action.enabled,
                emphasis: action.emphasis,
                order,
            },
            DeathUiFocusOrder(order),
            AccessibleLabel::new(action.label.clone()),
            Node {
                width: percent(100),
                min_height: px(if metrics.layout_mode == DeathUiLayoutMode::Minimum {
                    72
                } else {
                    88
                }),
                flex_shrink: 0.0,
                padding: UiRect::axes(px(16), px(10)),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                column_gap: px(18),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(17, 20, 20)),
            BorderColor::all(Color::srgb_u8(61, 61, 53)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                width: percent(33),
                flex_direction: FlexDirection::Column,
                row_gap: px(3),
                ..default()
            })
            .with_children(|hero| {
                spawn_strong_text(
                    hero,
                    entry.authority.character_name_snapshot.as_str(),
                    metrics.body_text_px * 1.12,
                    Color::srgb_u8(236, 224, 194),
                );
                spawn_text(
                    hero,
                    &entry.class.label,
                    metrics.label_text_px,
                    Color::srgb_u8(154, 164, 151),
                );
            });
            row.spawn(Node {
                width: percent(33),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(3),
                ..default()
            })
            .with_children(|record| {
                spawn_strong_text(
                    record,
                    &entry.presentation.label,
                    metrics.body_text_px,
                    Color::srgb_u8(193, 164, 111),
                );
                spawn_text(
                    record,
                    &entry.echo_outcome.label,
                    metrics.label_text_px,
                    Color::srgb_u8(145, 174, 161),
                );
            });
            row.spawn(Node {
                width: percent(33),
                justify_content: JustifyContent::End,
                ..default()
            })
            .with_children(|date| {
                spawn_text(
                    date,
                    &entry.formatted_death_at,
                    metrics.label_text_px,
                    Color::srgb_u8(153, 151, 139),
                );
            });
        });
}

fn spawn_status_card(
    parent: &mut ChildSpawnerCommands,
    status: &DeathUiStatus,
    metrics: DeathUiMetrics,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(112),
                flex_shrink: 0.0,
                padding: UiRect::all(px(18)),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                row_gap: px(8),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(17, 19, 19)),
            BorderColor::all(if status.recoverable {
                Color::srgb_u8(151, 97, 54)
            } else {
                Color::srgb_u8(65, 83, 76)
            }),
        ))
        .with_children(|card| {
            spawn_strong_text(
                card,
                &status.title,
                metrics.body_text_px * 1.2,
                Color::srgb_u8(232, 220, 191),
            );
            if let Some(detail) = status.detail.as_ref() {
                spawn_text(
                    card,
                    detail,
                    metrics.body_text_px,
                    Color::srgb_u8(173, 174, 160),
                );
            }
        });
}

fn spawn_action_button(
    parent: &mut ChildSpawnerCommands,
    action: &DeathUiActionSpec,
    order: u16,
    metrics: DeathUiMetrics,
) {
    let primary = action.emphasis == DeathUiActionEmphasis::Primary;
    parent
        .spawn((
            Button,
            DeathUiButton {
                action: action.action.clone(),
                enabled: action.enabled,
                emphasis: action.emphasis,
                order,
            },
            DeathUiFocusOrder(order),
            AccessibleLabel::new(action.label.clone()),
            Node {
                min_width: px(if primary { 270 } else { 176 }),
                min_height: px(if primary { 58 } else { 44 }),
                flex_shrink: 0.0,
                padding: UiRect::axes(px(if primary { 24 } else { 16 }), px(10)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(if primary { 2 } else { 1 })),
                ..default()
            },
            BackgroundColor(if action.enabled {
                if primary {
                    Color::srgb_u8(48, 36, 24)
                } else {
                    Color::srgb_u8(24, 29, 28)
                }
            } else {
                Color::srgb_u8(20, 21, 21)
            }),
            BorderColor::all(if action.enabled {
                if primary {
                    Color::srgb_u8(181, 137, 72)
                } else {
                    Color::srgb_u8(74, 91, 83)
                }
            } else {
                Color::srgb_u8(55, 55, 52)
            }),
        ))
        .with_children(|button| {
            let size = if primary {
                metrics.body_text_px * 1.05
            } else {
                metrics.label_text_px
            };
            let color = if action.enabled {
                Color::srgb_u8(238, 228, 200)
            } else {
                Color::srgb_u8(112, 112, 105)
            };
            if primary {
                spawn_strong_text(button, &action.label, size, color);
            } else {
                spawn_text(button, &action.label, size, color);
            }
        });
}

fn spawn_source_portrait(
    parent: &mut ChildSpawnerCommands,
    portrait: &DeathSourcePortraitPresentation,
    texture: &Handle<Image>,
    atlas: &Handle<TextureAtlasLayout>,
    size: f32,
) {
    let DeathSourcePortraitPresentation::Asset { asset_id } = portrait else {
        parent.spawn((
            Node {
                width: px(size),
                height: px(size),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(13, 15, 15)),
            BorderColor::all(Color::srgb_u8(58, 58, 53)),
        ));
        return;
    };
    let Some(index) = portrait_atlas_index(asset_id) else {
        return;
    };
    parent.spawn((
        ImageNode::from_atlas_image(
            texture.clone(),
            TextureAtlas {
                layout: atlas.clone(),
                index,
            },
        ),
        Node {
            width: px(size),
            height: px(size),
            border: UiRect::all(px(2)),
            ..default()
        },
        BorderColor::all(Color::srgb_u8(139, 106, 58)),
    ));
}

fn spawn_section_title(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    metrics: DeathUiMetrics,
    danger: bool,
) {
    spawn_strong_text(
        parent,
        title,
        metrics.label_text_px,
        if danger {
            Color::srgb_u8(209, 129, 91)
        } else {
            Color::srgb_u8(177, 146, 91)
        },
    );
}

fn spawn_field(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    value: &str,
    metrics: DeathUiMetrics,
) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(8),
            ..default()
        })
        .with_children(|row| {
            spawn_text(
                row,
                label,
                metrics.label_text_px,
                Color::srgb_u8(130, 137, 128),
            );
            spawn_text(
                row,
                value,
                metrics.label_text_px,
                Color::srgb_u8(211, 207, 190),
            );
        });
}

fn spawn_text(
    parent: &mut ChildSpawnerCommands,
    value: impl Into<String>,
    size: f32,
    color: Color,
) {
    spawn_weighted_text(parent, value, size, color, DeathUiFontWeight::Regular);
}

fn spawn_strong_text(
    parent: &mut ChildSpawnerCommands,
    value: impl Into<String>,
    size: f32,
    color: Color,
) {
    spawn_weighted_text(parent, value, size, color, DeathUiFontWeight::Bold);
}

fn spawn_weighted_text(
    parent: &mut ChildSpawnerCommands,
    value: impl Into<String>,
    size: f32,
    color: Color,
    weight: DeathUiFontWeight,
) {
    parent.spawn((
        Text::new(value),
        TextFont::from_font_size(size.max(MIN_EFFECTIVE_TEXT_PX)),
        weight,
        TextColor(color),
        Node {
            max_width: percent(100),
            ..default()
        },
    ));
}

type DeathUiButtonInteractions<'w, 's> = Query<
    'w,
    's,
    (&'static Interaction, &'static DeathUiButton),
    (Changed<Interaction>, With<Button>),
>;

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn handle_death_ui_focus_and_activation(
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut requested_focus: MessageReader<DeathUiFocusRequest>,
    mut changed_buttons: DeathUiButtonInteractions,
    all_buttons: Query<&DeathUiButton, With<Button>>,
    mut focus: ResMut<DeathUiFocusState>,
    mut commands: MessageWriter<DeathUiCommand>,
) {
    let mut ordered = all_buttons.iter().cloned().collect::<Vec<_>>();
    ordered.sort_by_key(|button| button.order);

    for (interaction, button) in &mut changed_buttons {
        if button.enabled && matches!(interaction, Interaction::Hovered | Interaction::Pressed) {
            focus.focused_order = Some(button.order);
        }
        if button.enabled && *interaction == Interaction::Pressed {
            commands.write(DeathUiCommand(button.action.clone()));
        }
    }

    let requested_focus = requested_focus.read().last().copied();
    let gamepad_pressed = |button| gamepads.iter().any(|pad| pad.just_pressed(button));
    let previous = keyboard.just_pressed(KeyCode::Tab)
        && (keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight))
        || keyboard.just_pressed(KeyCode::ArrowLeft)
        || keyboard.just_pressed(KeyCode::ArrowUp)
        || gamepad_pressed(GamepadButton::DPadLeft)
        || gamepad_pressed(GamepadButton::DPadUp)
        || requested_focus == Some(DeathUiFocusRequest::Previous);
    let next = (keyboard.just_pressed(KeyCode::Tab)
        && !keyboard.pressed(KeyCode::ShiftLeft)
        && !keyboard.pressed(KeyCode::ShiftRight))
        || keyboard.just_pressed(KeyCode::ArrowRight)
        || keyboard.just_pressed(KeyCode::ArrowDown)
        || gamepad_pressed(GamepadButton::DPadRight)
        || gamepad_pressed(GamepadButton::DPadDown)
        || requested_focus == Some(DeathUiFocusRequest::Next);
    if previous || next {
        let next_order =
            next_focus_order(&ordered, focus.focused_order, if previous { -1 } else { 1 });
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
        commands.write(DeathUiCommand(button.action.clone()));
    }

    let back = keyboard_requests_back(&keyboard) || gamepad_pressed(GamepadButton::East);
    if back && let Some(action) = enabled_back_action(&ordered) {
        commands.write(DeathUiCommand(action));
    }
}

fn keyboard_requests_back(keyboard: &ButtonInput<KeyCode>) -> bool {
    keyboard.just_pressed(KeyCode::Escape)
}

fn enabled_back_action(buttons: &[DeathUiButton]) -> Option<DeathUiAction> {
    buttons
        .iter()
        .find(|button| button.enabled && button.action == DeathUiAction::Back)
        .map(|button| button.action.clone())
}

#[allow(clippy::needless_pass_by_value, clippy::type_complexity)]
fn keep_focused_death_action_visible(
    mut focus: ResMut<DeathUiFocusState>,
    mut roots: Query<
        (&mut ScrollPosition, &ComputedNode, &UiGlobalTransform),
        With<DeathUiScrollRoot>,
    >,
    buttons: Query<(&DeathUiButton, &ComputedNode, &UiGlobalTransform), With<Button>>,
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

fn next_focus_order(ordered: &[DeathUiButton], current: Option<u16>, direction: i8) -> Option<u16> {
    let enabled = ordered
        .iter()
        .filter(|button| button.enabled)
        .map(|button| button.order)
        .collect::<Vec<_>>();
    if enabled.is_empty() {
        return None;
    }
    let current_index = current.and_then(|order| enabled.iter().position(|value| *value == order));
    let next_index = match (current_index, direction.cmp(&0)) {
        (Some(index), Ordering::Less) => index.checked_sub(1).unwrap_or(enabled.len() - 1),
        (Some(index), _) => (index + 1) % enabled.len(),
        (None, Ordering::Less) => enabled.len() - 1,
        (None, _) => 0,
    };
    enabled.get(next_index).copied()
}

#[allow(clippy::needless_pass_by_value)]
fn scroll_death_ui(
    mut wheel: MessageReader<MouseWheel>,
    mut requested: MessageReader<DeathUiScrollRequest>,
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut roots: Query<(&mut ScrollPosition, &ComputedNode), With<DeathUiScrollRoot>>,
) {
    let requested = requested.read().last().copied();
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
        delta += 360.0;
    }
    if keyboard.just_pressed(KeyCode::PageUp)
        || gamepads
            .iter()
            .any(|gamepad| gamepad.just_pressed(GamepadButton::LeftTrigger))
    {
        delta -= 360.0;
    }
    if delta == 0.0 && requested.is_none() {
        return;
    }
    for (mut scroll, computed) in &mut roots {
        let max_offset = ((computed.content_size() - computed.size())
            * computed.inverse_scale_factor())
        .y
        .max(0.0);
        scroll.y = match requested {
            Some(DeathUiScrollRequest::Start) => 0.0,
            Some(DeathUiScrollRequest::End) => max_offset,
            None => (scroll.y + delta).clamp(0.0, max_offset),
        };
    }
}

fn update_death_ui_scrollbar(
    roots: Query<(&ScrollPosition, &ComputedNode), With<DeathUiScrollRoot>>,
    mut tracks: Query<&mut Visibility, With<DeathUiScrollTrack>>,
    mut thumbs: Query<&mut Node, With<DeathUiScrollThumb>>,
    mut state: ResMut<DeathUiScrollState>,
) {
    let Ok((scroll, computed)) = roots.single() else {
        *state = DeathUiScrollState::default();
        return;
    };
    let Ok(mut track_visibility) = tracks.single_mut() else {
        return;
    };
    let Ok(mut thumb) = thumbs.single_mut() else {
        return;
    };
    let viewport_height = computed.size().y;
    let content_height = computed.content_size().y;
    let Some(geometry) = scrollbar_geometry(
        viewport_height,
        content_height,
        scroll.y,
        computed.inverse_scale_factor(),
    ) else {
        *state = DeathUiScrollState::default();
        *track_visibility = Visibility::Hidden;
        return;
    };
    *state = DeathUiScrollState {
        offset: scroll.y,
        max_offset: geometry.max_offset,
    };
    *track_visibility = Visibility::Visible;
    thumb.height = percent(geometry.height_percent);
    thumb.top = percent(geometry.top_percent);
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct DeathUiScrollbarGeometry {
    height_percent: f32,
    top_percent: f32,
    max_offset: f32,
}

fn scrollbar_geometry(
    viewport_height: f32,
    content_height: f32,
    scroll_offset: f32,
    inverse_scale_factor: f32,
) -> Option<DeathUiScrollbarGeometry> {
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
    Some(DeathUiScrollbarGeometry {
        height_percent: thumb_fraction * 100.0,
        top_percent: progress * (1.0 - thumb_fraction) * 100.0,
        max_offset,
    })
}

#[allow(clippy::needless_pass_by_value)]
fn style_death_ui_buttons(
    focus: Res<DeathUiFocusState>,
    mut buttons: Query<
        (
            &Interaction,
            &DeathUiButton,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<Button>,
    >,
) {
    for (interaction, button, mut background, mut border) in &mut buttons {
        let focused = focus.focused_order == Some(button.order) && button.enabled;
        if !button.enabled {
            background.0 = Color::srgb_u8(20, 21, 21);
            *border = BorderColor::all(Color::srgb_u8(55, 55, 52));
            continue;
        }
        let primary = button.emphasis == DeathUiActionEmphasis::Primary;
        match interaction {
            Interaction::Pressed => {
                background.0 = if primary {
                    Color::srgb_u8(74, 51, 29)
                } else {
                    Color::srgb_u8(38, 52, 48)
                };
                *border = BorderColor::all(Color::srgb_u8(236, 210, 144));
            }
            Interaction::Hovered => {
                background.0 = if primary {
                    Color::srgb_u8(62, 45, 28)
                } else {
                    Color::srgb_u8(31, 42, 39)
                };
                *border = BorderColor::all(Color::srgb_u8(190, 165, 104));
            }
            Interaction::None => {
                background.0 = if primary {
                    Color::srgb_u8(48, 36, 24)
                } else {
                    Color::srgb_u8(24, 29, 28)
                };
                *border = BorderColor::all(if focused {
                    Color::srgb_u8(235, 220, 166)
                } else if primary {
                    Color::srgb_u8(181, 137, 72)
                } else {
                    Color::srgb_u8(74, 91, 83)
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimum_and_reference_layouts_keep_text_and_safe_margins_valid() {
        let minimum = DeathUiMetrics::for_viewport(1_280.0, 720.0, 80).unwrap();
        assert_eq!(minimum.layout_mode, DeathUiLayoutMode::Minimum);
        assert!(minimum.body_text_px >= MIN_EFFECTIVE_TEXT_PX);
        assert!(minimum.label_text_px >= MIN_EFFECTIVE_TEXT_PX);
        assert!(minimum.safe_margin_px >= 12.0);

        let reference = DeathUiMetrics::for_viewport(1_920.0, 1_080.0, 100).unwrap();
        assert_eq!(reference.layout_mode, DeathUiLayoutMode::Reference);
        assert!((reference.safe_margin_px - REFERENCE_SAFE_MARGIN_PX).abs() < f32::EPSILON);
        assert!(reference.title_text_px > minimum.title_text_px);
    }

    #[test]
    fn invalid_scale_or_viewport_fails_closed() {
        for scale in [0, 79, 151, u16::MAX] {
            assert_eq!(
                DeathUiMetrics::for_viewport(1_280.0, 720.0, scale),
                Err(DeathUiSnapshotError::InvalidLayout)
            );
        }
        assert_eq!(
            DeathUiMetrics::for_viewport(f32::NAN, 720.0, 100),
            Err(DeathUiSnapshotError::InvalidLayout)
        );
    }

    #[test]
    fn portrait_atlas_mapping_is_complete_unique_and_unknown_safe() {
        let assets = [
            "portrait.enemy.drowned_pilgrim",
            "portrait.enemy.mire_leech",
            "portrait.enemy.bell_reed",
            "portrait.enemy.bell_acolyte",
            "portrait.enemy.chain_sentry",
            "portrait.enemy.choir_skull",
            "portrait.miniboss.sepulcher_knight",
            "portrait.miniboss.choir_abbot",
            "portrait.boss.sir_caldus",
        ];
        let indexes = assets
            .iter()
            .map(|asset| portrait_atlas_index(asset).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(indexes, (0..9).collect::<Vec<_>>());
        assert_eq!(portrait_atlas_index("portrait.enemy.unknown"), None);
        assert_eq!(
            validate_portrait_mapping(&DeathSourcePortraitPresentation::ExplicitlyAbsent),
            Ok(())
        );
        assert_eq!(
            validate_portrait_mapping(&DeathSourcePortraitPresentation::Asset {
                asset_id: "portrait.miniboss.sepulcher_knight".to_owned(),
            }),
            Ok(())
        );
        assert_eq!(
            validate_portrait_mapping(&DeathSourcePortraitPresentation::Asset {
                asset_id: "portrait.enemy.unknown".to_owned(),
            }),
            Err(DeathUiSnapshotError::UnknownPortraitAsset(
                "portrait.enemy.unknown".to_owned()
            ))
        );
    }

    #[test]
    fn created_echo_projection_uses_the_authoritative_outcome_label() {
        let entry = DeathFixedProjectionPresentation {
            ordinal: 1,
            kind: DeathSummaryProjectionKindV1::CreatedEcho,
            value: crate::DeathLocalizedValue {
                content_id: "projection.created.echo".to_owned(),
                label: "Echo outcome".to_owned(),
            },
            quantity: 1,
            formatted_quantity: "x1".to_owned(),
        };
        assert_eq!(
            created_projection_detail(&entry, "Available Echo"),
            "Available Echo"
        );
    }

    #[test]
    fn focus_navigation_skips_disabled_actions_and_wraps() {
        let buttons = vec![
            DeathUiButton {
                action: DeathUiAction::Summary(DeathSummaryAction::CreateSuccessor),
                enabled: false,
                emphasis: DeathUiActionEmphasis::Primary,
                order: 0,
            },
            DeathUiButton {
                action: DeathUiAction::Summary(DeathSummaryAction::InspectTrace),
                enabled: true,
                emphasis: DeathUiActionEmphasis::Secondary,
                order: 1,
            },
            DeathUiButton {
                action: DeathUiAction::Back,
                enabled: true,
                emphasis: DeathUiActionEmphasis::Utility,
                order: 2,
            },
        ];
        assert_eq!(next_focus_order(&buttons, None, 1), Some(1));
        assert_eq!(next_focus_order(&buttons, Some(1), 1), Some(2));
        assert_eq!(next_focus_order(&buttons, Some(2), 1), Some(1));
        assert_eq!(next_focus_order(&buttons, Some(1), -1), Some(2));
    }

    #[test]
    fn escape_resolves_only_to_the_enabled_read_only_back_action() {
        let mut keyboard = ButtonInput::<KeyCode>::default();
        keyboard.press(KeyCode::Escape);
        assert!(keyboard_requests_back(&keyboard));

        let mut buttons = vec![
            DeathUiButton {
                action: DeathUiAction::Summary(DeathSummaryAction::CreateSuccessor),
                enabled: true,
                emphasis: DeathUiActionEmphasis::Primary,
                order: 0,
            },
            DeathUiButton {
                action: DeathUiAction::Back,
                enabled: true,
                emphasis: DeathUiActionEmphasis::Utility,
                order: 1,
            },
        ];
        assert_eq!(enabled_back_action(&buttons), Some(DeathUiAction::Back));

        buttons[1].enabled = false;
        assert_eq!(enabled_back_action(&buttons), None);
    }

    #[test]
    fn focus_visibility_scrolls_only_as_far_as_required_and_clamps() {
        let visible = scroll_offset_to_reveal(50.0, 300.0, 0.0, 100.0, 25.0, 75.0, 10.0);
        assert!((visible - 50.0).abs() < f32::EPSILON);

        let below = scroll_offset_to_reveal(0.0, 300.0, 0.0, 100.0, 120.0, 160.0, 10.0);
        assert!((below - 70.0).abs() < f32::EPSILON);

        let above = scroll_offset_to_reveal(100.0, 300.0, 0.0, 100.0, -40.0, -10.0, 10.0);
        assert!((above - 50.0).abs() < f32::EPSILON);

        let end = scroll_offset_to_reveal(290.0, 300.0, 0.0, 100.0, 400.0, 500.0, 10.0);
        assert!((end - 300.0).abs() < f32::EPSILON);
    }

    #[test]
    fn checked_in_death_ui_assets_have_the_approved_identity() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        validate_death_ui_assets(&root).unwrap();
    }

    #[test]
    fn evidence_readiness_requires_fonts_root_and_text_layout() {
        assert!(render_layout_is_ready(true, true, true));
        for state in [
            (false, true, true),
            (true, false, true),
            (true, true, false),
        ] {
            assert!(!render_layout_is_ready(state.0, state.1, state.2));
        }
    }

    #[test]
    fn scrollbar_geometry_is_hidden_without_overflow_and_tracks_both_extents() {
        assert_eq!(scrollbar_geometry(600.0, 600.0, 0.0, 1.0), None);
        assert_eq!(scrollbar_geometry(0.0, 900.0, 0.0, 1.0), None);

        let top = scrollbar_geometry(600.0, 1_200.0, 0.0, 1.0).unwrap();
        assert!((top.height_percent - 50.0).abs() < f32::EPSILON);
        assert!(top.top_percent.abs() < f32::EPSILON);

        let bottom = scrollbar_geometry(600.0, 1_200.0, 600.0, 1.0).unwrap();
        assert!((bottom.height_percent - 50.0).abs() < f32::EPSILON);
        assert!((bottom.top_percent - 50.0).abs() < f32::EPSILON);
    }
}
