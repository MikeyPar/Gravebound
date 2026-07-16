//! Native rendered-frame probe for the integrated `GB-M03-06E` durable-death boundary.
//!
//! Authority remains split exactly as required by
//! `Gravebound_Production_GDD_v1_Canonical.md`,
//! `Gravebound_Content_Production_Spec_v1.md`, and
//! `Gravebound_Development_Roadmap_v1.md`. The fixture must come from the authenticated
//! server/PostgreSQL journey; this module only verifies the fixture, performs the real
//! `CoreWorldTransitionModel` `DeathFinal` handoff, replays the exact read responses through
//! `DeathViewClientModel`, renders the production native widgets, drives their real input path,
//! and records the native model-to-captured-frame interval. It never claims to measure the
//! durable commit that occurred before the fixture was produced.

use std::{
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use bevy::{
    app::AppExit,
    asset::io::{AssetSourceBuilder, AssetSourceId, file::FileAssetReader},
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    window::{PresentMode, PrimaryWindow, WindowResolution},
};
use protocol::{
    CharacterLocation, CharacterLocationSnapshot, DEATH_VIEW_SCHEMA_VERSION,
    DeathViewContentRevisionV1, DeathViewRequestV1, DeathViewResultV1, M03_CORE_DEV_BUILD_ID,
    SafeArrival, SessionDestination, WireText, WorldFlowContentRevisionV1, WorldTransferCommand,
    WorldTransferMutation, WorldTransferPayload,
};
use serde::{Deserialize, Serialize};
use sim_content::{load_core_development_death_view, load_core_development_world_flow};
use thiserror::Error;

use crate::{
    CoreWorldTransitionModel, CoreWorldTransitionPhase, CoreWorldTransitionResolution,
    DeathSummaryAction, DeathUiAction, DeathUiCommand, DeathUiConfig, DeathUiFocusState,
    DeathUiRenderReadiness, DeathUiSnapshot, DeathViewApplyDisposition, DeathViewClientModel,
    NativeDeathView, NativeDeathViewPlugin, TERMINAL_SUMMARY_LOSS_PAGE_LIMIT,
    validate_death_ui_assets,
};

pub const NATIVE_DEATH_FRAME_PROBE_FIXTURE_SCHEMA_VERSION: u16 = 1;
pub const NATIVE_DEATH_FRAME_PROBE_REPORT_SCHEMA_VERSION: u16 = 1;

const FIXTURE_KIND: &str = "gravebound.gb-m03-06e.native-death-frame-fixture.v1";
const REPORT_KIND: &str = "gravebound.gb-m03-06e.native-death-frame-report.v1";
const TIMING_SCOPE: &str = "native-model-ready-to-rendered-frame;durable-commit-excluded";
const LANTERN_HALLS_ID: &str = "hub.lantern_halls_01";
const PROBE_PORTAL_ID: &str = "portal.dungeon.bell_sepulcher";
const MIN_VIEWPORT_WIDTH: u32 = 1_280;
const MIN_VIEWPORT_HEIGHT: u32 = 720;
const MAX_VIEWPORT_WIDTH: u32 = 7_680;
const MAX_VIEWPORT_HEIGHT: u32 = 4_320;
const MIN_TIMEOUT_MS: u64 = 2_000;
const MAX_TIMEOUT_MS: u64 = 120_000;
const TRACE_SETTLE_FRAMES: u8 = 2;

static TEMP_FILE_ORDINAL: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct NativeDeathFrameProbeConfig {
    pub fixture_path: PathBuf,
    pub screenshot_path: PathBuf,
    pub report_path: PathBuf,
    pub content_root: PathBuf,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub reduced_effects: bool,
    pub ui_scale_percent: u16,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeDeathFrameProbeFixtureV1 {
    pub schema_version: u16,
    pub fixture_kind: String,
    pub build_id: String,
    pub character_id: [u8; 16],
    pub world_flow_revision: WorldFlowContentRevisionV1,
    pub destination: SessionDestination,
    pub latest_result: DeathViewResultV1,
    pub summary_result: DeathViewResultV1,
    pub fixture_hash_blake3: String,
}

impl NativeDeathFrameProbeFixtureV1 {
    pub fn new(
        build_id: impl Into<String>,
        character_id: [u8; 16],
        world_flow_revision: WorldFlowContentRevisionV1,
        destination: SessionDestination,
        latest_result: DeathViewResultV1,
        summary_result: DeathViewResultV1,
    ) -> Result<Self, NativeDeathFrameProbeError> {
        let mut fixture = Self {
            schema_version: NATIVE_DEATH_FRAME_PROBE_FIXTURE_SCHEMA_VERSION,
            fixture_kind: FIXTURE_KIND.to_owned(),
            build_id: build_id.into(),
            character_id,
            world_flow_revision,
            destination,
            latest_result,
            summary_result,
            fixture_hash_blake3: String::new(),
        };
        fixture.validate_payload()?;
        fixture.fixture_hash_blake3 = fixture.canonical_hash()?;
        fixture.validate()?;
        Ok(fixture)
    }

    pub fn from_json_slice(bytes: &[u8]) -> Result<Self, NativeDeathFrameProbeError> {
        let fixture = serde_json::from_slice::<Self>(bytes)?;
        fixture.validate()?;
        Ok(fixture)
    }

    pub fn read_json(path: &Path) -> Result<Self, NativeDeathFrameProbeError> {
        let bytes = fs::read(path)?;
        Self::from_json_slice(&bytes)
    }

    pub fn validate(&self) -> Result<(), NativeDeathFrameProbeError> {
        self.validate_payload()?;
        validate_blake3_hex(&self.fixture_hash_blake3)
            .map_err(|()| NativeDeathFrameProbeError::InvalidFixtureHash)?;
        if self.fixture_hash_blake3 != self.canonical_hash()? {
            return Err(NativeDeathFrameProbeError::FixtureHashMismatch);
        }
        Ok(())
    }

    #[must_use]
    pub fn fixture_hash_blake3(&self) -> &str {
        &self.fixture_hash_blake3
    }

    pub fn write_json_atomically(&self, path: &Path) -> Result<(), NativeDeathFrameProbeError> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self)?;
        atomic_publish_bytes(path, &bytes)?;
        Ok(())
    }

    fn validate_payload(&self) -> Result<(), NativeDeathFrameProbeError> {
        if self.schema_version != NATIVE_DEATH_FRAME_PROBE_FIXTURE_SCHEMA_VERSION {
            return Err(NativeDeathFrameProbeError::UnsupportedFixtureSchema);
        }
        if self.fixture_kind != FIXTURE_KIND {
            return Err(NativeDeathFrameProbeError::InvalidFixtureKind);
        }
        if self.build_id != M03_CORE_DEV_BUILD_ID {
            return Err(NativeDeathFrameProbeError::BuildMismatch);
        }
        if self.character_id == [0; 16] {
            return Err(NativeDeathFrameProbeError::InvalidCharacter);
        }
        if self.destination != SessionDestination::DeathFinal {
            return Err(NativeDeathFrameProbeError::InvalidDestination);
        }
        self.latest_result
            .validate()
            .map_err(|_| NativeDeathFrameProbeError::InvalidLatestResult)?;
        self.summary_result
            .validate()
            .map_err(|_| NativeDeathFrameProbeError::InvalidSummaryResult)?;

        let (latest_sequence, latest) = match &self.latest_result {
            DeathViewResultV1::Latest {
                schema_version,
                request_sequence,
                death: Some(death),
            } if *schema_version == DEATH_VIEW_SCHEMA_VERSION => (*request_sequence, death),
            _ => return Err(NativeDeathFrameProbeError::InvalidLatestResult),
        };
        let (summary_sequence, requested_lost_limit, summary) = match &self.summary_result {
            DeathViewResultV1::Summary {
                schema_version,
                request_sequence,
                requested_lost_limit,
                summary,
            } if *schema_version == DEATH_VIEW_SCHEMA_VERSION => {
                (*request_sequence, *requested_lost_limit, summary)
            }
            _ => return Err(NativeDeathFrameProbeError::InvalidSummaryResult),
        };
        if latest_sequence != 1
            || summary_sequence != 2
            || requested_lost_limit != TERMINAL_SUMMARY_LOSS_PAGE_LIMIT
        {
            return Err(NativeDeathFrameProbeError::UnexpectedResponseSequence);
        }
        if latest.character_id != self.character_id
            || summary.death_id != latest.death_id
            || summary.death_tick != latest.death_tick
            || summary.snapshot_digest != latest.summary_snapshot_digest
            || summary.content_revision != latest.content_revision
            || summary.presentation_revision != latest.presentation_revision
            || summary.lost_total_count != latest.destruction_entry_count
        {
            return Err(NativeDeathFrameProbeError::ResponseAuthorityMismatch);
        }
        Ok(())
    }

    fn canonical_hash(&self) -> Result<String, NativeDeathFrameProbeError> {
        let mut hashable = self.clone();
        hashable.fixture_hash_blake3.clear();
        let bytes = serde_json::to_vec(&hashable)?;
        Ok(blake3::hash(&bytes).to_hex().to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeDeathFrameProbeActionV1 {
    InspectTrace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeDeathFrameProbeReportV1 {
    pub schema_version: u16,
    pub report_kind: String,
    pub timing_scope: String,
    pub accepted: bool,
    pub fixture_build_id: String,
    pub client_executable_blake3: String,
    pub character_id: [u8; 16],
    pub destination: SessionDestination,
    pub world_flow_revision: WorldFlowContentRevisionV1,
    pub death_view_revision: DeathViewContentRevisionV1,
    pub item_content_revision: String,
    pub content_authority_blake3: String,
    pub fixture_hash_blake3: String,
    pub screenshot_blake3: String,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub reduced_effects: bool,
    pub ui_scale_percent: u16,
    pub focused_order: u16,
    pub activated_action: NativeDeathFrameProbeActionV1,
    pub model_ready_to_render_ready_micros: u64,
    pub render_ready_to_focus_action_micros: u64,
    pub focus_action_to_activation_micros: u64,
    pub activation_to_screenshot_captured_micros: u64,
    pub model_ready_to_screenshot_captured_micros: u64,
    pub report_hash_blake3: String,
}

impl NativeDeathFrameProbeReportV1 {
    pub fn from_json_slice(bytes: &[u8]) -> Result<Self, NativeDeathFrameProbeError> {
        let report = serde_json::from_slice::<Self>(bytes)?;
        report.validate()?;
        Ok(report)
    }

    pub fn read_json(path: &Path) -> Result<Self, NativeDeathFrameProbeError> {
        let bytes = fs::read(path)?;
        Self::from_json_slice(&bytes)
    }

    pub fn validate(&self) -> Result<(), NativeDeathFrameProbeError> {
        self.validate_payload()?;
        validate_blake3_hex(&self.report_hash_blake3)
            .map_err(|()| NativeDeathFrameProbeError::InvalidReportHash)?;
        if self.report_hash_blake3 != self.canonical_hash()? {
            return Err(NativeDeathFrameProbeError::ReportHashMismatch);
        }
        Ok(())
    }

    #[must_use]
    pub fn report_hash_blake3(&self) -> &str {
        &self.report_hash_blake3
    }

    fn new(input: NativeDeathFrameProbeReportInput) -> Result<Self, NativeDeathFrameProbeError> {
        let mut report = Self {
            schema_version: NATIVE_DEATH_FRAME_PROBE_REPORT_SCHEMA_VERSION,
            report_kind: REPORT_KIND.to_owned(),
            timing_scope: TIMING_SCOPE.to_owned(),
            accepted: true,
            fixture_build_id: input.fixture_build_id,
            client_executable_blake3: input.client_executable_blake3,
            character_id: input.character_id,
            destination: SessionDestination::DeathFinal,
            world_flow_revision: input.world_flow_revision,
            death_view_revision: input.death_view_revision,
            item_content_revision: input.item_content_revision,
            content_authority_blake3: input.content_authority_blake3,
            fixture_hash_blake3: input.fixture_hash_blake3,
            screenshot_blake3: input.screenshot_blake3,
            viewport_width: input.viewport_width,
            viewport_height: input.viewport_height,
            reduced_effects: input.reduced_effects,
            ui_scale_percent: input.ui_scale_percent,
            focused_order: input.focused_order,
            activated_action: NativeDeathFrameProbeActionV1::InspectTrace,
            model_ready_to_render_ready_micros: elapsed_micros(
                input.model_ready_at,
                input.render_ready_at,
            ),
            render_ready_to_focus_action_micros: elapsed_micros(
                input.render_ready_at,
                input.focus_action_at,
            ),
            focus_action_to_activation_micros: elapsed_micros(
                input.focus_action_at,
                input.activation_at,
            ),
            activation_to_screenshot_captured_micros: elapsed_micros(
                input.activation_at,
                input.screenshot_captured_at,
            ),
            model_ready_to_screenshot_captured_micros: elapsed_micros(
                input.model_ready_at,
                input.screenshot_captured_at,
            ),
            report_hash_blake3: String::new(),
        };
        report.validate_payload()?;
        report.report_hash_blake3 = report.canonical_hash()?;
        report.validate()?;
        Ok(report)
    }

    fn validate_payload(&self) -> Result<(), NativeDeathFrameProbeError> {
        if self.schema_version != NATIVE_DEATH_FRAME_PROBE_REPORT_SCHEMA_VERSION {
            return Err(NativeDeathFrameProbeError::UnsupportedReportSchema);
        }
        if self.report_kind != REPORT_KIND || self.timing_scope != TIMING_SCOPE {
            return Err(NativeDeathFrameProbeError::InvalidReportShape);
        }
        if !self.accepted
            || self.fixture_build_id != M03_CORE_DEV_BUILD_ID
            || self.character_id == [0; 16]
            || self.destination != SessionDestination::DeathFinal
            || self.item_content_revision.is_empty()
            || self.item_content_revision.len() > 96
            || self.item_content_revision.chars().any(char::is_control)
            || !(MIN_VIEWPORT_WIDTH..=MAX_VIEWPORT_WIDTH).contains(&self.viewport_width)
            || !(MIN_VIEWPORT_HEIGHT..=MAX_VIEWPORT_HEIGHT).contains(&self.viewport_height)
            || !(80..=150).contains(&self.ui_scale_percent)
            || self.focused_order != 1
            || self.activated_action != NativeDeathFrameProbeActionV1::InspectTrace
            || self.model_ready_to_render_ready_micros == 0
            || self.render_ready_to_focus_action_micros == 0
            || self.focus_action_to_activation_micros == 0
            || self.activation_to_screenshot_captured_micros == 0
            || self.model_ready_to_screenshot_captured_micros
                < self.model_ready_to_render_ready_micros
            || self.model_ready_to_screenshot_captured_micros
                < self.activation_to_screenshot_captured_micros
            || self.model_ready_to_screenshot_captured_micros
                < self
                    .model_ready_to_render_ready_micros
                    .saturating_add(self.render_ready_to_focus_action_micros)
                    .saturating_add(self.focus_action_to_activation_micros)
                    .saturating_add(self.activation_to_screenshot_captured_micros)
        {
            return Err(NativeDeathFrameProbeError::InvalidReportShape);
        }
        for hash in [
            &self.client_executable_blake3,
            &self.content_authority_blake3,
            &self.fixture_hash_blake3,
            &self.screenshot_blake3,
        ] {
            validate_blake3_hex(hash)
                .map_err(|()| NativeDeathFrameProbeError::InvalidReportShape)?;
        }
        if self.content_authority_blake3
            != canonical_content_authority_hash(
                &self.world_flow_revision,
                &self.death_view_revision,
                &self.item_content_revision,
            )?
        {
            return Err(NativeDeathFrameProbeError::InvalidReportShape);
        }
        Ok(())
    }

    fn canonical_hash(&self) -> Result<String, NativeDeathFrameProbeError> {
        let mut hashable = self.clone();
        hashable.report_hash_blake3.clear();
        let bytes = serde_json::to_vec(&hashable)?;
        Ok(blake3::hash(&bytes).to_hex().to_string())
    }
}

#[derive(Debug, Error)]
pub enum NativeDeathFrameProbeError {
    #[error("native death-frame fixture schema is unsupported")]
    UnsupportedFixtureSchema,
    #[error("native death-frame fixture kind is invalid")]
    InvalidFixtureKind,
    #[error("native death-frame fixture build does not match the Core endpoint")]
    BuildMismatch,
    #[error("native death-frame fixture character identity is invalid")]
    InvalidCharacter,
    #[error("native death-frame fixture destination is not DeathFinal")]
    InvalidDestination,
    #[error("native death-frame fixture Latest response is invalid")]
    InvalidLatestResult,
    #[error("native death-frame fixture Summary response is invalid")]
    InvalidSummaryResult,
    #[error("native death-frame responses do not use the exact initial sequence")]
    UnexpectedResponseSequence,
    #[error("native death-frame responses disagree on durable authority")]
    ResponseAuthorityMismatch,
    #[error("native death-frame fixture hash is malformed")]
    InvalidFixtureHash,
    #[error("native death-frame fixture hash does not match its payload")]
    FixtureHashMismatch,
    #[error("native death-frame report schema is unsupported")]
    UnsupportedReportSchema,
    #[error("native death-frame report shape is invalid")]
    InvalidReportShape,
    #[error("native death-frame report hash is malformed")]
    InvalidReportHash,
    #[error("native death-frame report hash does not match its payload")]
    ReportHashMismatch,
    #[error("native death-frame JSON could not be encoded or decoded: {0}")]
    Json(#[from] serde_json::Error),
    #[error("native death-frame artifact I/O failed: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeDeathFrameProbeStage {
    AwaitingInitialRender,
    AwaitingFocus,
    AwaitingActivation,
    AwaitingTraceRender,
    AwaitingCapture,
    Complete,
    Failed,
}

#[derive(Resource)]
struct NativeDeathFrameProbeRuntime {
    stage: NativeDeathFrameProbeStage,
    model_ready_at: Instant,
    render_ready_at: Option<Instant>,
    focus_action_at: Option<Instant>,
    activation_at: Option<Instant>,
    timeout: Duration,
    expected_focus_order: u16,
    observed_focus_order: Option<u16>,
    trace_not_ready_observed: bool,
    trace_settled_frames: u8,
    fixture_build_id: String,
    client_executable_blake3: String,
    character_id: [u8; 16],
    world_flow_revision: WorldFlowContentRevisionV1,
    death_view_revision: DeathViewContentRevisionV1,
    item_content_revision: String,
    content_authority_blake3: String,
    fixture_hash_blake3: String,
    screenshot_path: PathBuf,
    report_path: PathBuf,
    viewport_width: u32,
    viewport_height: u32,
    reduced_effects: bool,
    ui_scale_percent: u16,
}

struct NativeDeathFrameProbeReportInput {
    fixture_build_id: String,
    client_executable_blake3: String,
    character_id: [u8; 16],
    world_flow_revision: WorldFlowContentRevisionV1,
    death_view_revision: DeathViewContentRevisionV1,
    item_content_revision: String,
    content_authority_blake3: String,
    fixture_hash_blake3: String,
    screenshot_blake3: String,
    viewport_width: u32,
    viewport_height: u32,
    reduced_effects: bool,
    ui_scale_percent: u16,
    focused_order: u16,
    model_ready_at: Instant,
    render_ready_at: Instant,
    focus_action_at: Instant,
    activation_at: Instant,
    screenshot_captured_at: Instant,
}

struct PreparedNativeDeathFrameProbe {
    native_view: NativeDeathView,
    runtime: NativeDeathFrameProbeRuntime,
    asset_root: PathBuf,
}

#[derive(Serialize)]
struct NativeDeathFrameContentAuthority<'a> {
    world_flow: &'a WorldFlowContentRevisionV1,
    death_view: &'a DeathViewContentRevisionV1,
    item_content: &'a str,
}

pub fn run_native_death_frame_probe(config: &NativeDeathFrameProbeConfig) -> Result<()> {
    validate_probe_config(config)?;
    let prepared = prepare_probe(config)?;
    let asset_reader_root = prepared.asset_root.clone();
    let mut app = App::new();
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::new(move || Box::new(FileAssetReader::new(asset_reader_root.clone()))),
    )
    .insert_resource(ClearColor(Color::srgb_u8(5, 7, 8)))
    .insert_resource(prepared.native_view)
    .insert_resource(prepared.runtime)
    .add_plugins(
        DefaultPlugins
            .set(ImagePlugin::default_nearest())
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Gravebound - GB-M03-06E Native Death Frame Probe".to_owned(),
                    resolution: WindowResolution::new(
                        config.viewport_width,
                        config.viewport_height,
                    )
                    .with_scale_factor_override(1.0),
                    present_mode: PresentMode::AutoVsync,
                    resizable: false,
                    ..default()
                }),
                ..default()
            }),
    )
    .add_plugins(NativeDeathViewPlugin)
    .add_systems(Startup, spawn_probe_camera)
    .add_systems(Last, drive_native_death_frame_probe);

    match app.run() {
        AppExit::Success => Ok(()),
        AppExit::Error(code) => bail!(
            "native death-frame probe failed closed with exit code {}",
            code.get()
        ),
    }
}

fn validate_probe_config(config: &NativeDeathFrameProbeConfig) -> Result<()> {
    ensure!(
        (MIN_VIEWPORT_WIDTH..=MAX_VIEWPORT_WIDTH).contains(&config.viewport_width)
            && (MIN_VIEWPORT_HEIGHT..=MAX_VIEWPORT_HEIGHT).contains(&config.viewport_height),
        "native death-frame viewport must remain within {MIN_VIEWPORT_WIDTH}x{MIN_VIEWPORT_HEIGHT}..{MAX_VIEWPORT_WIDTH}x{MAX_VIEWPORT_HEIGHT}"
    );
    ensure!(
        (80..=150).contains(&config.ui_scale_percent),
        "native death-frame UI scale must remain within 80..=150"
    );
    ensure!(
        (MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&config.timeout_ms),
        "native death-frame timeout must remain within {MIN_TIMEOUT_MS}..={MAX_TIMEOUT_MS} ms"
    );
    ensure!(
        config
            .screenshot_path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("png")),
        "native death-frame screenshot path must use .png"
    );
    ensure!(
        config
            .report_path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json")),
        "native death-frame report path must use .json"
    );
    ensure!(
        absolute_path(&config.screenshot_path)? != absolute_path(&config.report_path)?,
        "native death-frame screenshot and report paths must be distinct"
    );
    ensure!(
        !config.screenshot_path.exists(),
        "native death-frame screenshot destination already exists: {}",
        config.screenshot_path.display()
    );
    ensure!(
        !config.report_path.exists(),
        "native death-frame report destination already exists: {}",
        config.report_path.display()
    );
    Ok(())
}

#[allow(clippy::too_many_lines)] // Preparation keeps the complete cross-model authority handoff auditable in one linear path.
fn prepare_probe(config: &NativeDeathFrameProbeConfig) -> Result<PreparedNativeDeathFrameProbe> {
    let fixture = NativeDeathFrameProbeFixtureV1::read_json(&config.fixture_path)
        .with_context(|| format!("rejected fixture {}", config.fixture_path.display()))?;
    let content_root = fs::canonicalize(&config.content_root).with_context(|| {
        format!(
            "could not resolve content root {}",
            config.content_root.display()
        )
    })?;
    let repository_root = content_root
        .parent()
        .context("content root has no repository parent")?;
    let asset_root = repository_root.join("assets");
    validate_death_ui_assets(&asset_root)?;

    let world = load_core_development_world_flow(&content_root)
        .context("Core world-flow content failed validation")?;
    let compiled_world_revision = WorldFlowContentRevisionV1 {
        records_blake3: protocol::ManifestHash::new(world.hashes().records_blake3.clone())?,
        assets_blake3: protocol::ManifestHash::new(world.hashes().assets_blake3.clone())?,
        localization_blake3: protocol::ManifestHash::new(
            world.hashes().localization_blake3.clone(),
        )?,
    };
    ensure!(
        fixture.world_flow_revision == compiled_world_revision,
        "native death-frame world-flow revision does not match compiled content"
    );

    let catalog = load_core_development_death_view(&content_root)
        .context("Core death presentation failed validation")?;
    let death_view_revision = DeathViewContentRevisionV1 {
        records_blake3: protocol::ManifestHash::new(catalog.hashes().records_blake3.clone())?,
        assets_blake3: protocol::ManifestHash::new(catalog.hashes().assets_blake3.clone())?,
        localization_blake3: protocol::ManifestHash::new(
            catalog.hashes().localization_blake3.clone(),
        )?,
    };
    let item_content_revision = catalog.item_content_revision().to_owned();
    let content_authority_blake3 = canonical_content_authority_hash(
        &compiled_world_revision,
        &death_view_revision,
        &item_content_revision,
    )?;

    let mut transition = transition_model(&fixture)?;
    transition.transport_lost()?;
    transition.await_authoritative_resolution()?;
    transition.reconnecting(1)?;
    transition.reconnect_resolved(fixture.destination, None)?;
    ensure!(
        transition.phase() == CoreWorldTransitionPhase::ResolvedToDeathSummary
            && transition.resolution() == CoreWorldTransitionResolution::DeathCommitted,
        "Core world transition did not resolve the fixture to committed death"
    );

    let mut death_view = DeathViewClientModel::new(catalog)?;
    let latest_request = death_view.begin_world_transition_death_handoff(&transition)?;
    ensure!(
        latest_request.sequence == 1
            && latest_request.request == DeathViewRequestV1::LatestCommitted,
        "death handoff did not issue the exact Latest request"
    );
    let latest_outcome = death_view.handle_result(&fixture.latest_result)?;
    ensure!(
        latest_outcome.disposition == DeathViewApplyDisposition::Applied,
        "fixture Latest response was not applied"
    );
    let summary_request = latest_outcome
        .follow_up
        .context("fixture Latest response did not request Summary")?;
    let expected_death_id = match &fixture.latest_result {
        DeathViewResultV1::Latest {
            death: Some(death), ..
        } => death.death_id,
        _ => unreachable!("validated fixture requires a committed Latest response"),
    };
    ensure!(
        summary_request.sequence == 2
            && summary_request.request
                == DeathViewRequestV1::Summary {
                    death_id: expected_death_id,
                    lost_start_ordinal: 0,
                    lost_limit: TERMINAL_SUMMARY_LOSS_PAGE_LIMIT,
                },
        "fixture Latest response did not issue the exact Summary request"
    );
    let summary_outcome = death_view.handle_result(&fixture.summary_result)?;
    ensure!(
        summary_outcome.disposition == DeathViewApplyDisposition::Applied
            && summary_outcome.follow_up.is_none()
            && death_view.pending().is_none(),
        "fixture Summary response did not settle the terminal client model"
    );

    let snapshot = DeathUiSnapshot::terminal(&death_view)?;
    let inspect_actions = snapshot
        .actions()
        .into_iter()
        .enumerate()
        .filter(|(_, action)| {
            action.action == DeathUiAction::Summary(DeathSummaryAction::InspectTrace)
                && action.enabled
        })
        .collect::<Vec<_>>();
    ensure!(
        inspect_actions.len() == 1,
        "native death-frame snapshot must expose exactly one enabled InspectTrace action"
    );
    let expected_focus_order =
        u16::try_from(inspect_actions[0].0).context("InspectTrace focus order overflow")?;
    let native_view = NativeDeathView::new(
        snapshot,
        DeathUiConfig {
            reduced_effects: config.reduced_effects,
            ui_scale_percent: config.ui_scale_percent,
        },
    )?;
    let executable_id = crate::executable_build_id()?;
    let client_executable_blake3 = executable_id
        .strip_prefix("release-")
        .context("native executable build hash has an unexpected prefix")?
        .to_owned();
    validate_blake3_hex(&client_executable_blake3)
        .map_err(|()| anyhow::anyhow!("native executable build hash is malformed"))?;

    prepare_output_parent(&config.screenshot_path)?;
    prepare_output_parent(&config.report_path)?;
    let model_ready_at = Instant::now();
    Ok(PreparedNativeDeathFrameProbe {
        native_view,
        runtime: NativeDeathFrameProbeRuntime {
            stage: NativeDeathFrameProbeStage::AwaitingInitialRender,
            model_ready_at,
            render_ready_at: None,
            focus_action_at: None,
            activation_at: None,
            timeout: Duration::from_millis(config.timeout_ms),
            expected_focus_order,
            observed_focus_order: None,
            trace_not_ready_observed: false,
            trace_settled_frames: 0,
            fixture_build_id: fixture.build_id,
            client_executable_blake3,
            character_id: fixture.character_id,
            world_flow_revision: fixture.world_flow_revision,
            death_view_revision,
            item_content_revision,
            content_authority_blake3,
            fixture_hash_blake3: fixture.fixture_hash_blake3,
            screenshot_path: config.screenshot_path.clone(),
            report_path: config.report_path.clone(),
            viewport_width: config.viewport_width,
            viewport_height: config.viewport_height,
            reduced_effects: config.reduced_effects,
            ui_scale_percent: config.ui_scale_percent,
        },
        asset_root,
    })
}

fn transition_model(fixture: &NativeDeathFrameProbeFixtureV1) -> Result<CoreWorldTransitionModel> {
    let snapshot = CharacterLocationSnapshot {
        character_id: fixture.character_id,
        character_version: 1,
        location: CharacterLocation::Safe {
            location_id: WireText::new(LANTERN_HALLS_ID)?,
            arrival: SafeArrival::HallDefault,
        },
    };
    let mut transition =
        CoreWorldTransitionModel::new(fixture.world_flow_revision.clone(), snapshot)?;
    let payload = WorldTransferPayload {
        content_revision: fixture.world_flow_revision.clone(),
        command: WorldTransferCommand::UsePortal {
            portal_id: WireText::new(PROBE_PORTAL_ID)?,
        },
    };
    let mutation = WorldTransferMutation {
        mutation_id: [0x6e; 16],
        character_id: fixture.character_id,
        expected_character_version: 1,
        issued_at_unix_millis: 1,
        payload_hash: payload.canonical_hash(),
        payload,
    };
    transition.begin_transfer(1, mutation)?;
    Ok(transition)
}

fn spawn_probe_camera(mut commands: Commands) {
    commands.spawn((Camera2d, IsDefaultUiCamera, BoxShadowSamples(6)));
}

#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::too_many_lines
)] // One ordered Bevy system owns the fail-closed input/capture state machine.
fn drive_native_death_frame_probe(
    mut commands: Commands,
    readiness: Res<DeathUiRenderReadiness>,
    focus: Res<DeathUiFocusState>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut keyboard: MessageWriter<KeyboardInput>,
    mut ui_commands: MessageReader<DeathUiCommand>,
    mut view: ResMut<NativeDeathView>,
    mut runtime: ResMut<NativeDeathFrameProbeRuntime>,
    mut exit: MessageWriter<AppExit>,
) {
    if matches!(
        runtime.stage,
        NativeDeathFrameProbeStage::Complete | NativeDeathFrameProbeStage::Failed
    ) {
        return;
    }
    if runtime.model_ready_at.elapsed() > runtime.timeout {
        fail_probe(
            &mut runtime,
            &mut exit,
            "native death-frame probe timed out before a validated capture",
        );
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let observed_commands = ui_commands.read().cloned().collect::<Vec<_>>();

    match runtime.stage {
        NativeDeathFrameProbeStage::AwaitingInitialRender => {
            if !observed_commands.is_empty() {
                fail_probe(
                    &mut runtime,
                    &mut exit,
                    "death UI emitted a command before probe input",
                );
            } else if readiness.is_ready() {
                runtime.render_ready_at = Some(Instant::now());
                write_keyboard_input(
                    &mut keyboard,
                    window,
                    KeyCode::Tab,
                    Key::Tab,
                    ButtonState::Pressed,
                );
                runtime.stage = NativeDeathFrameProbeStage::AwaitingFocus;
            }
        }
        NativeDeathFrameProbeStage::AwaitingFocus => {
            if !observed_commands.is_empty() {
                fail_probe(
                    &mut runtime,
                    &mut exit,
                    "death UI emitted a command before activation input",
                );
            } else if let Some(order) = focus.focused_order() {
                if order != runtime.expected_focus_order {
                    fail_probe(
                        &mut runtime,
                        &mut exit,
                        "focus-next selected an action other than InspectTrace",
                    );
                    return;
                }
                runtime.observed_focus_order = Some(order);
                runtime.focus_action_at = Some(Instant::now());
                write_keyboard_input(
                    &mut keyboard,
                    window,
                    KeyCode::Tab,
                    Key::Tab,
                    ButtonState::Released,
                );
                write_keyboard_input(
                    &mut keyboard,
                    window,
                    KeyCode::Enter,
                    Key::Enter,
                    ButtonState::Pressed,
                );
                runtime.stage = NativeDeathFrameProbeStage::AwaitingActivation;
            }
        }
        NativeDeathFrameProbeStage::AwaitingActivation => {
            if observed_commands.is_empty() {
                return;
            }
            if observed_commands.len() != 1
                || observed_commands[0].0
                    != DeathUiAction::Summary(DeathSummaryAction::InspectTrace)
            {
                fail_probe(
                    &mut runtime,
                    &mut exit,
                    "activation input did not emit exactly one InspectTrace command",
                );
                return;
            }
            runtime.activation_at = Some(Instant::now());
            write_keyboard_input(
                &mut keyboard,
                window,
                KeyCode::Enter,
                Key::Enter,
                ButtonState::Released,
            );
            view.set_trace_emphasis(true);
            runtime.stage = NativeDeathFrameProbeStage::AwaitingTraceRender;
        }
        NativeDeathFrameProbeStage::AwaitingTraceRender => {
            if !observed_commands.is_empty() {
                fail_probe(
                    &mut runtime,
                    &mut exit,
                    "death UI emitted a duplicate command after activation",
                );
                return;
            }
            if !readiness.is_ready() {
                runtime.trace_not_ready_observed = true;
                runtime.trace_settled_frames = 0;
                return;
            }
            if !runtime.trace_not_ready_observed {
                return;
            }
            runtime.trace_settled_frames = runtime.trace_settled_frames.saturating_add(1);
            if runtime.trace_settled_frames >= TRACE_SETTLE_FRAMES {
                runtime.stage = NativeDeathFrameProbeStage::AwaitingCapture;
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(publish_native_death_frame_capture);
            }
        }
        NativeDeathFrameProbeStage::AwaitingCapture => {
            if !observed_commands.is_empty() {
                fail_probe(
                    &mut runtime,
                    &mut exit,
                    "death UI emitted a command while capture was pending",
                );
            }
        }
        NativeDeathFrameProbeStage::Complete | NativeDeathFrameProbeStage::Failed => {}
    }
}

fn write_keyboard_input(
    writer: &mut MessageWriter<KeyboardInput>,
    window: Entity,
    key_code: KeyCode,
    logical_key: Key,
    state: ButtonState,
) {
    writer.write(KeyboardInput {
        key_code,
        logical_key,
        state,
        text: None,
        repeat: false,
        window,
    });
}

#[allow(clippy::needless_pass_by_value)]
fn publish_native_death_frame_capture(
    captured: On<ScreenshotCaptured>,
    mut runtime: ResMut<NativeDeathFrameProbeRuntime>,
    mut exit: MessageWriter<AppExit>,
) {
    let captured_at = Instant::now();
    let result = publish_capture_artifacts(&captured, &runtime, captured_at);
    match result {
        Ok(report) => {
            info!(
                report_hash = %report.report_hash_blake3,
                screenshot_hash = %report.screenshot_blake3,
                model_to_frame_micros = report.model_ready_to_screenshot_captured_micros,
                "native death-frame evidence published"
            );
            runtime.stage = NativeDeathFrameProbeStage::Complete;
            exit.write(AppExit::Success);
        }
        Err(error) => {
            error!(%error, "native death-frame evidence publication failed");
            runtime.stage = NativeDeathFrameProbeStage::Failed;
            exit.write(AppExit::from_code(2));
        }
    }
}

fn publish_capture_artifacts(
    captured: &ScreenshotCaptured,
    runtime: &NativeDeathFrameProbeRuntime,
    captured_at: Instant,
) -> Result<NativeDeathFrameProbeReportV1> {
    ensure!(
        runtime.stage == NativeDeathFrameProbeStage::AwaitingCapture,
        "screenshot arrived outside the capture stage"
    );
    ensure!(
        captured.image.width() == runtime.viewport_width
            && captured.image.height() == runtime.viewport_height,
        "captured viewport {}x{} does not match configured {}x{}",
        captured.image.width(),
        captured.image.height(),
        runtime.viewport_width,
        runtime.viewport_height
    );
    let screenshot_temp = atomic_temp_path(&runtime.screenshot_path);
    let report_temp = atomic_temp_path(&runtime.report_path);
    let publish_result = (|| -> Result<NativeDeathFrameProbeReportV1> {
        ensure!(
            !screenshot_temp.exists() && !report_temp.exists(),
            "native death-frame temporary path collision"
        );
        let dynamic = captured
            .image
            .clone()
            .try_into_dynamic()
            .map_err(|error| anyhow::anyhow!("captured frame could not be encoded: {error}"))?;
        dynamic
            .to_rgb8()
            .save(&screenshot_temp)
            .with_context(|| format!("failed to encode {}", screenshot_temp.display()))?;
        sync_file(&screenshot_temp)?;
        let screenshot_bytes = fs::read(&screenshot_temp)
            .with_context(|| format!("failed to read {}", screenshot_temp.display()))?;
        let screenshot_blake3 = blake3::hash(&screenshot_bytes).to_hex().to_string();

        let report = NativeDeathFrameProbeReportV1::new(NativeDeathFrameProbeReportInput {
            fixture_build_id: runtime.fixture_build_id.clone(),
            client_executable_blake3: runtime.client_executable_blake3.clone(),
            character_id: runtime.character_id,
            world_flow_revision: runtime.world_flow_revision.clone(),
            death_view_revision: runtime.death_view_revision.clone(),
            item_content_revision: runtime.item_content_revision.clone(),
            content_authority_blake3: runtime.content_authority_blake3.clone(),
            fixture_hash_blake3: runtime.fixture_hash_blake3.clone(),
            screenshot_blake3,
            viewport_width: runtime.viewport_width,
            viewport_height: runtime.viewport_height,
            reduced_effects: runtime.reduced_effects,
            ui_scale_percent: runtime.ui_scale_percent,
            focused_order: runtime
                .observed_focus_order
                .context("probe did not retain the validated focus order")?,
            model_ready_at: runtime.model_ready_at,
            render_ready_at: runtime
                .render_ready_at
                .context("probe did not retain initial render readiness")?,
            focus_action_at: runtime
                .focus_action_at
                .context("probe did not retain focus-action timing")?,
            activation_at: runtime
                .activation_at
                .context("probe did not retain activation timing")?,
            screenshot_captured_at: captured_at,
        })?;
        let report_bytes =
            serde_json::to_vec_pretty(&report).context("failed to encode death-frame report")?;
        write_synced_new(&report_temp, &report_bytes)?;

        fs::rename(&screenshot_temp, &runtime.screenshot_path).with_context(|| {
            format!(
                "failed to publish screenshot {}",
                runtime.screenshot_path.display()
            )
        })?;
        if let Err(error) = fs::rename(&report_temp, &runtime.report_path) {
            let _ = fs::remove_file(&runtime.screenshot_path);
            return Err(error).with_context(|| {
                format!(
                    "failed to publish death-frame report {}",
                    runtime.report_path.display()
                )
            });
        }
        Ok(report)
    })();
    if publish_result.is_err() {
        let _ = fs::remove_file(&screenshot_temp);
        let _ = fs::remove_file(&report_temp);
    }
    publish_result
}

fn fail_probe(
    runtime: &mut NativeDeathFrameProbeRuntime,
    exit: &mut MessageWriter<AppExit>,
    reason: &str,
) {
    error!(reason, "native death-frame probe failed closed");
    runtime.stage = NativeDeathFrameProbeStage::Failed;
    exit.write(AppExit::from_code(2));
}

fn canonical_content_authority_hash(
    world_flow: &WorldFlowContentRevisionV1,
    death_view: &DeathViewContentRevisionV1,
    item_content: &str,
) -> Result<String, NativeDeathFrameProbeError> {
    let bytes = serde_json::to_vec(&NativeDeathFrameContentAuthority {
        world_flow,
        death_view,
        item_content,
    })?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn elapsed_micros(start: Instant, end: Instant) -> u64 {
    u64::try_from(end.saturating_duration_since(start).as_micros())
        .unwrap_or(u64::MAX)
        .max(1)
}

fn validate_blake3_hex(value: &str) -> Result<(), ()> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(())
    }
}

fn prepare_output_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory {}", parent.display()))?;
    }
    Ok(())
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_owned())
    } else {
        Ok(std::env::current_dir()
            .context("failed to resolve current directory")?
            .join(path))
    }
}

fn atomic_publish_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("destination already exists: {}", path.display()),
        ));
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let temporary = atomic_temp_path(path);
    let result = (|| -> io::Result<()> {
        write_synced_new(&temporary, bytes)?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn write_synced_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn sync_file(path: &Path) -> io::Result<()> {
    fs::OpenOptions::new().write(true).open(path)?.sync_all()
}

fn atomic_temp_path(path: &Path) -> PathBuf {
    let ordinal = TEMP_FILE_ORDINAL.fetch_add(1, Ordering::Relaxed);
    let extension = path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("tmp");
    let stem = path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("gravebound-evidence");
    path.with_file_name(format!(
        ".{stem}.{}.{}.partial.{extension}",
        std::process::id(),
        ordinal
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{DEATH_VIEW_SCHEMA_VERSION, DeathViewResultV1};

    fn content_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn valid_fixture() -> NativeDeathFrameProbeFixtureV1 {
        let root = content_root();
        let world = load_core_development_world_flow(&root).unwrap();
        let world_revision = WorldFlowContentRevisionV1 {
            records_blake3: protocol::ManifestHash::new(world.hashes().records_blake3.clone())
                .unwrap(),
            assets_blake3: protocol::ManifestHash::new(world.hashes().assets_blake3.clone())
                .unwrap(),
            localization_blake3: protocol::ManifestHash::new(
                world.hashes().localization_blake3.clone(),
            )
            .unwrap(),
        };
        let catalog = load_core_development_death_view(&root).unwrap();
        let revision = crate::core_death_view_showcase::revision(&catalog).unwrap();
        let latest =
            crate::core_death_view_showcase::latest(&revision, catalog.item_content_revision());
        let summary =
            crate::core_death_view_showcase::summary(&revision, catalog.item_content_revision());
        NativeDeathFrameProbeFixtureV1::new(
            M03_CORE_DEV_BUILD_ID,
            crate::core_death_view_showcase::CHARACTER_ID,
            world_revision,
            SessionDestination::DeathFinal,
            DeathViewResultV1::Latest {
                schema_version: DEATH_VIEW_SCHEMA_VERSION,
                request_sequence: 1,
                death: Some(latest),
            },
            DeathViewResultV1::Summary {
                schema_version: DEATH_VIEW_SCHEMA_VERSION,
                request_sequence: 2,
                requested_lost_limit: TERMINAL_SUMMARY_LOSS_PAGE_LIMIT,
                summary,
            },
        )
        .unwrap()
    }

    fn valid_report() -> NativeDeathFrameProbeReportV1 {
        let fixture = valid_fixture();
        let DeathViewResultV1::Latest {
            death: Some(latest),
            ..
        } = &fixture.latest_result
        else {
            unreachable!()
        };
        let now = Instant::now();
        NativeDeathFrameProbeReportV1::new(NativeDeathFrameProbeReportInput {
            fixture_build_id: fixture.build_id.clone(),
            client_executable_blake3: "1".repeat(64),
            character_id: fixture.character_id,
            world_flow_revision: fixture.world_flow_revision.clone(),
            death_view_revision: latest.presentation_revision.clone(),
            item_content_revision: latest.content_revision.as_str().to_owned(),
            content_authority_blake3: canonical_content_authority_hash(
                &fixture.world_flow_revision,
                &latest.presentation_revision,
                latest.content_revision.as_str(),
            )
            .unwrap(),
            fixture_hash_blake3: fixture.fixture_hash_blake3.clone(),
            screenshot_blake3: "3".repeat(64),
            viewport_width: 1_920,
            viewport_height: 1_080,
            reduced_effects: false,
            ui_scale_percent: 100,
            focused_order: 1,
            model_ready_at: now,
            render_ready_at: now + Duration::from_millis(4),
            focus_action_at: now + Duration::from_millis(6),
            activation_at: now + Duration::from_millis(8),
            screenshot_captured_at: now + Duration::from_millis(14),
        })
        .unwrap()
    }

    #[test]
    fn fixture_hash_is_deterministic_and_round_trips_strict_json() {
        let first = valid_fixture();
        let second = valid_fixture();
        assert_eq!(first.fixture_hash_blake3, second.fixture_hash_blake3);
        let bytes = serde_json::to_vec_pretty(&first).unwrap();
        assert_eq!(
            NativeDeathFrameProbeFixtureV1::from_json_slice(&bytes).unwrap(),
            first
        );

        let mut unknown = serde_json::to_value(&first).unwrap();
        unknown
            .as_object_mut()
            .unwrap()
            .insert("unexpected".to_owned(), serde_json::json!(true));
        assert!(
            NativeDeathFrameProbeFixtureV1::from_json_slice(&serde_json::to_vec(&unknown).unwrap())
                .is_err()
        );
    }

    #[test]
    fn fixture_rejects_tampering_and_invalid_terminal_shapes() {
        let mut tampered = valid_fixture();
        tampered.character_id[0] ^= 1;
        assert!(matches!(
            tampered.validate(),
            Err(NativeDeathFrameProbeError::ResponseAuthorityMismatch
                | NativeDeathFrameProbeError::FixtureHashMismatch)
        ));

        let mut wrong_destination = valid_fixture();
        wrong_destination.destination = SessionDestination::LanternHalls;
        assert!(matches!(
            wrong_destination.validate(),
            Err(NativeDeathFrameProbeError::InvalidDestination)
        ));

        let mut wrong_sequence = valid_fixture();
        if let DeathViewResultV1::Summary {
            request_sequence, ..
        } = &mut wrong_sequence.summary_result
        {
            *request_sequence = 3;
        }
        assert!(matches!(
            wrong_sequence.validate(),
            Err(NativeDeathFrameProbeError::UnexpectedResponseSequence)
        ));
    }

    #[test]
    fn report_hash_round_trips_and_rejects_invalid_claims() {
        let report = valid_report();
        let bytes = serde_json::to_vec_pretty(&report).unwrap();
        assert_eq!(
            NativeDeathFrameProbeReportV1::from_json_slice(&bytes).unwrap(),
            report
        );

        let mut durable_claim = report.clone();
        durable_claim.timing_scope = "durable-commit-to-rendered-frame".to_owned();
        assert!(matches!(
            durable_claim.validate(),
            Err(NativeDeathFrameProbeError::InvalidReportShape)
        ));

        let mut tampered_hash = report;
        tampered_hash.screenshot_blake3 = "4".repeat(64);
        assert!(matches!(
            tampered_hash.validate(),
            Err(NativeDeathFrameProbeError::ReportHashMismatch)
        ));

        let mut unbound_content = valid_report();
        unbound_content.item_content_revision.push('x');
        unbound_content.report_hash_blake3 = unbound_content.canonical_hash().unwrap();
        assert!(matches!(
            unbound_content.validate(),
            Err(NativeDeathFrameProbeError::InvalidReportShape)
        ));
    }
}
