//! Disposable native adapter for the `GB-M03-03F` transition projection.

use std::{
    env, fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};
use bevy::{
    app::AppExit,
    prelude::*,
    render::view::screenshot::Screenshot,
    window::{PresentMode, PrimaryWindow, WindowResolution},
};
use protocol::{
    CharacterLocation, CharacterLocationSnapshot, HandshakeRejection, ManifestHash, SafeArrival,
    SessionDestination, WireText, WorldFlowContentRevisionV1, WorldFlowResult,
    WorldTransferCommand, WorldTransferMutation, WorldTransferPayload, WorldTransferResultCode,
};
use serde::Serialize;
use sim_content::{
    CoreDevelopmentWorldFlow, CoreWorldTransitionCopyKey, load_core_development_world_flow,
};
use sim_core::{MONOTONIC_GROWTH_FLOOR_BYTES, MemoryAssessment, MemorySample, TargetHardware};
use sysinfo::{Pid, ProcessesToUpdate, System, get_current_pid};

use crate::{
    CoreRetryDirective, CoreSafeOrigin, CoreSceneReadiness, CoreWorldTransitionModel,
    CoreWorldTransitionPhase, CoreWorldTransitionResolution,
};

// Independent release launches can need more than one glyph-atlas/upload cycle before every UI
// layer is present in the swapchain. Ninety frames keeps evidence deterministic and avoids
// accepting semantically incomplete composites.
const EVIDENCE_SETTLE_FRAMES: u8 = 90;
const SOAK_DURATION_ENV: &str = "GRAVEBOUND_CORE_TRANSITION_SOAK_SECONDS";
const SOAK_REPORT_PATH_ENV: &str = "GRAVEBOUND_CORE_TRANSITION_REPORT_PATH";
const SOAK_TARGET_VERIFIED_ENV: &str = "GRAVEBOUND_TARGET_CLASS_VERIFIED";
const SOAK_TARGET_GPU_ENV: &str = "GRAVEBOUND_TARGET_GPU";
const SOAK_WARMUP_SECONDS: u64 = 5;
const SOAK_MEMORY_SAMPLE_SECONDS: u64 = 10;
const SOAK_STATE_SECONDS: u64 = 5;
const TARGET_MEMORY_BYTES: u64 = 1_500_000_000;
const HALL_ID: &str = "hub.lantern_halls_01";
const DUNGEON_ID: &str = "dungeon.bell_sepulcher";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreTransitionShowcaseState {
    HallLoading,
    DungeonLoading,
    RecoverableError,
    FatalError,
    LinkLost,
    Reconnecting,
    SameStateRecovery,
    HallResolution,
}

#[derive(Debug, Clone)]
pub struct CoreTransitionShowcaseConfig {
    pub content_root: PathBuf,
    pub reduced_effects: bool,
    pub state: CoreTransitionShowcaseState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransitionUiAction {
    Retry,
    ReturnCharacterSelect,
    Exit,
}

#[derive(Debug, Clone, Resource)]
struct ShowcaseViewModel {
    state: CoreTransitionShowcaseState,
    phase: CoreWorldTransitionPhase,
    resolution: CoreWorldTransitionResolution,
    title: String,
    detail: String,
    safe_origin: String,
    destination: String,
    action_label: Option<String>,
    action: Option<TransitionUiAction>,
    status_label: String,
    records_revision: String,
    reduced_effects: bool,
}

#[derive(Debug, Resource)]
struct ScreenshotRequest(PathBuf);

#[derive(Debug, Default)]
struct CaptureProgress {
    settled_frames: u8,
    queued: bool,
}

#[derive(Debug, Component)]
struct TransitionActionButton(TransitionUiAction);

#[derive(Debug, Component)]
struct TransitionActionStatus;

#[derive(Debug, Component)]
struct TransitionSurfaceRoot;

#[derive(Debug, Clone, Serialize)]
struct CoreTransitionPerformanceReport {
    report_schema: String,
    build_id: String,
    records_blake3: String,
    assets_blake3: String,
    localization_blake3: String,
    duration_ms: u64,
    state_interval_ms: u64,
    transitions_completed: u64,
    rendered_frame_count: usize,
    measured_fps_milli: u64,
    p95_frame_time_micros: u64,
    p99_frame_time_micros: u64,
    memory_samples: Vec<MemorySample>,
    peak_resident_bytes: u64,
    memory_assessment: MemoryAssessment,
    target_hardware: TargetHardware,
    target_class_verified: bool,
    accepted: bool,
    raw_report_hash_blake3: String,
}

#[derive(Resource)]
struct TransitionSoakState {
    models: Vec<ShowcaseViewModel>,
    current_model: usize,
    warmup_elapsed: Duration,
    measurement_elapsed: Duration,
    state_elapsed: Duration,
    measurement_duration: Duration,
    transitions_completed: u64,
    frame_times_micros: Vec<u64>,
    memory_samples: Vec<MemorySample>,
    next_memory_sample: Duration,
    memory: ProcessMemorySampler,
    report_path: PathBuf,
    build_id: String,
    records_blake3: String,
    assets_blake3: String,
    localization_blake3: String,
    target_hardware: TargetHardware,
    target_class_verified: bool,
}

struct ProcessMemorySampler {
    system: System,
    pid: Pid,
}

impl ProcessMemorySampler {
    fn new() -> Result<Self> {
        Ok(Self {
            system: System::new(),
            pid: get_current_pid().map_err(|error| {
                anyhow::anyhow!("failed to identify transition-soak process: {error}")
            })?,
        })
    }

    fn resident_bytes(&mut self) -> Result<u64> {
        self.system
            .refresh_processes(ProcessesToUpdate::Some(&[self.pid]), true);
        self.system
            .process(self.pid)
            .map(sysinfo::Process::memory)
            .context("transition-soak process disappeared from resident-memory sampling")
    }
}

pub fn run_core_transition_showcase(config: &CoreTransitionShowcaseConfig) -> Result<()> {
    let content = load_core_development_world_flow(&config.content_root)
        .context("unpromoted Core world-flow content failed validation")?;
    let model = build_showcase_model(&content, config.state, config.reduced_effects)?;
    let (window_width, window_height) = crate::configured_window_size()?;
    let screenshot_request = env::var_os("GRAVEBOUND_SCREENSHOT_PATH").map(PathBuf::from);
    let soak = transition_soak_from_environment(
        &content,
        config.reduced_effects,
        window_width,
        window_height,
    )?;
    let is_soak = soak.is_some();

    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb_u8(6, 8, 11)))
        .insert_resource(model)
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Gravebound - GB-M03-03F Transition Evidence".to_owned(),
                        resolution: WindowResolution::new(window_width, window_height),
                        present_mode: if is_soak {
                            PresentMode::AutoNoVsync
                        } else {
                            PresentMode::AutoVsync
                        },
                        resizable: !is_soak,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_systems(Startup, (spawn_transition_camera, spawn_transition_surface))
        .add_systems(
            Update,
            (
                rebuild_transition_surface_if_missing,
                handle_transition_input,
                style_action_button,
            )
                .chain(),
        );
    if let Some(soak) = soak {
        app.insert_resource(soak).add_systems(
            Update,
            advance_transition_soak.before(rebuild_transition_surface_if_missing),
        );
    }
    if let Some(path) = screenshot_request {
        app.insert_resource(ScreenshotRequest(path))
            .add_systems(Update, capture_evidence);
    }
    app.run();
    Ok(())
}

fn transition_soak_from_environment(
    content: &CoreDevelopmentWorldFlow,
    reduced_effects: bool,
    window_width: u32,
    window_height: u32,
) -> Result<Option<TransitionSoakState>> {
    let Some(duration) = env::var_os(SOAK_DURATION_ENV) else {
        return Ok(None);
    };
    let duration_seconds = duration
        .to_string_lossy()
        .parse::<u64>()
        .with_context(|| format!("{SOAK_DURATION_ENV} must be an integer"))?;
    anyhow::ensure!(
        (1..=7_200).contains(&duration_seconds),
        "{SOAK_DURATION_ENV} must be within 1..=7200"
    );
    let report_path = PathBuf::from(
        env::var_os(SOAK_REPORT_PATH_ENV)
            .context("transition soak requires GRAVEBOUND_CORE_TRANSITION_REPORT_PATH")?,
    );
    let target_class_verified = match env::var(SOAK_TARGET_VERIFIED_ENV).as_deref() {
        Ok("1") => true,
        Ok("0") | Err(_) => false,
        Ok(other) => anyhow::bail!("{SOAK_TARGET_VERIFIED_ENV} must be 0 or 1, got `{other}`"),
    };
    let states = [
        CoreTransitionShowcaseState::HallLoading,
        CoreTransitionShowcaseState::DungeonLoading,
        CoreTransitionShowcaseState::RecoverableError,
        CoreTransitionShowcaseState::FatalError,
        CoreTransitionShowcaseState::LinkLost,
        CoreTransitionShowcaseState::Reconnecting,
        CoreTransitionShowcaseState::SameStateRecovery,
        CoreTransitionShowcaseState::HallResolution,
    ];
    let models = states
        .into_iter()
        .map(|state| build_showcase_model(content, state, reduced_effects))
        .collect::<Result<Vec<_>>>()?;
    let mut system = System::new_all();
    system.refresh_cpu_all();
    let cpu = system
        .cpus()
        .first()
        .map_or("unavailable", |cpu| cpu.brand())
        .to_owned();
    Ok(Some(TransitionSoakState {
        models,
        current_model: 0,
        warmup_elapsed: Duration::ZERO,
        measurement_elapsed: Duration::ZERO,
        state_elapsed: Duration::ZERO,
        measurement_duration: Duration::from_secs(duration_seconds),
        transitions_completed: 0,
        frame_times_micros: Vec::with_capacity(
            usize::try_from(duration_seconds.saturating_mul(120)).unwrap_or(usize::MAX),
        ),
        memory_samples: Vec::new(),
        next_memory_sample: Duration::ZERO,
        memory: ProcessMemorySampler::new()?,
        report_path,
        build_id: crate::executable_build_id()?,
        records_blake3: content.hashes().records_blake3.clone(),
        assets_blake3: content.hashes().assets_blake3.clone(),
        localization_blake3: content.hashes().localization_blake3.clone(),
        target_hardware: TargetHardware {
            operating_system: System::long_os_version().unwrap_or_else(|| "Windows".to_owned()),
            cpu,
            memory_bytes: system.total_memory(),
            gpu: env::var(SOAK_TARGET_GPU_ENV).unwrap_or_else(|_| "unverified".to_owned()),
            width_pixels: window_width,
            height_pixels: window_height,
        },
        target_class_verified,
    }))
}

fn build_showcase_model(
    content: &CoreDevelopmentWorldFlow,
    state: CoreTransitionShowcaseState,
    reduced_effects: bool,
) -> Result<ShowcaseViewModel> {
    let revision = content_revision(content)?;
    let projection = projection_for_state(state, revision)?;

    let title = content
        .transition_copy(
            projection
                .phase_copy_key()
                .context("transition showcase cannot render the durable-death handoff")?,
        )
        .to_owned();
    let detail = projection
        .failure()
        .and_then(|failure| content.localized(failure.localization_key()))
        .map_or_else(
            || match projection.resolution() {
                CoreWorldTransitionResolution::Reattached => content
                    .transition_copy(CoreWorldTransitionCopyKey::StatusReattached)
                    .to_owned(),
                CoreWorldTransitionResolution::HallCommitted => content
                    .transition_copy(CoreWorldTransitionCopyKey::StatusHallCommitted)
                    .to_owned(),
                _ => match projection.phase() {
                    CoreWorldTransitionPhase::RequestingTransfer
                    | CoreWorldTransitionPhase::LoadingContent
                    | CoreWorldTransitionPhase::AwaitingAuthoritativeState => content
                        .transition_copy(CoreWorldTransitionCopyKey::StatusNoProgress)
                        .to_owned(),
                    CoreWorldTransitionPhase::LinkLost => content
                        .transition_copy(CoreWorldTransitionCopyKey::StatusVulnerabilityWarning)
                        .to_owned(),
                    _ => content
                        .transition_copy(CoreWorldTransitionCopyKey::StatusPriorSafeState)
                        .replace("{origin}", safe_origin_label(projection.safe_origin())),
                },
            },
            str::to_owned,
        );
    let (action, action_key) = action_contract(&projection);
    let action_label = action_key.map(|key| content.transition_copy(key).to_owned());
    let status_label = status_label(&projection, state, content);
    let destination = destination_label(state).to_owned();
    let safe_origin = safe_origin_label(projection.safe_origin()).to_owned();

    Ok(ShowcaseViewModel {
        state,
        phase: projection.phase(),
        resolution: projection.resolution(),
        title,
        detail,
        safe_origin,
        destination,
        action_label,
        action,
        status_label,
        records_revision: content.hashes().records_blake3[..12].to_owned(),
        reduced_effects,
    })
}

fn projection_for_state(
    state: CoreTransitionShowcaseState,
    revision: WorldFlowContentRevisionV1,
) -> Result<CoreWorldTransitionModel> {
    Ok(match state {
        CoreTransitionShowcaseState::HallLoading => {
            let mut model = CoreWorldTransitionModel::new(revision.clone(), character_select(1))?;
            let mutation = transfer_mutation(
                revision.clone(),
                1,
                1,
                WorldTransferCommand::EnterHallFromCharacterSelect,
            );
            model.begin_transfer(1, mutation.clone())?;
            model.apply_world_flow_result(&accepted(&mutation, hall(2)))?;
            model
        }
        CoreTransitionShowcaseState::DungeonLoading => {
            loading_dungeon_projection(revision.clone())?
        }
        CoreTransitionShowcaseState::RecoverableError => {
            let mut model = CoreWorldTransitionModel::new(revision.clone(), hall(1))?;
            let mutation = dungeon_mutation(revision.clone(), 3, 1);
            model.begin_transfer(1, mutation.clone())?;
            model.apply_world_flow_result(&rejected(
                &mutation,
                WorldTransferResultCode::ServiceUnavailable,
                Some(hall(1)),
            ))?;
            model
        }
        CoreTransitionShowcaseState::FatalError => {
            let mut model = CoreWorldTransitionModel::new(revision.clone(), hall(1))?;
            model.apply_handshake_rejection(HandshakeRejection::ContentMismatch)?;
            model
        }
        CoreTransitionShowcaseState::LinkLost => {
            let mut model = ready_dungeon_projection(revision.clone())?;
            model.transport_lost()?;
            model
        }
        CoreTransitionShowcaseState::Reconnecting => {
            let mut model = ready_dungeon_projection(revision.clone())?;
            model.transport_lost()?;
            model.reconnecting(2)?;
            model
        }
        CoreTransitionShowcaseState::SameStateRecovery => {
            let mut model = ready_dungeon_projection(revision.clone())?;
            model.transport_lost()?;
            model.reconnecting(1)?;
            model.reconnect_resolved(SessionDestination::CombatInstance, Some(dungeon(2)))?;
            model.mark_content_ready(&readiness(revision.clone(), DUNGEON_ID, 2))?;
            model
        }
        CoreTransitionShowcaseState::HallResolution => {
            let mut model = ready_dungeon_projection(revision.clone())?;
            model.transport_lost()?;
            model.reconnecting(1)?;
            model.reconnect_resolved(SessionDestination::LanternHalls, Some(hall(3)))?;
            model.mark_content_ready(&readiness(revision, HALL_ID, 3))?;
            model
        }
    })
}

fn loading_dungeon_projection(
    revision: WorldFlowContentRevisionV1,
) -> Result<CoreWorldTransitionModel> {
    let mut model = CoreWorldTransitionModel::new(revision.clone(), hall(1))?;
    let mutation = dungeon_mutation(revision, 2, 1);
    model.begin_transfer(1, mutation.clone())?;
    model.apply_world_flow_result(&accepted(&mutation, dungeon(2)))?;
    Ok(model)
}

fn ready_dungeon_projection(
    revision: WorldFlowContentRevisionV1,
) -> Result<CoreWorldTransitionModel> {
    let mut model = loading_dungeon_projection(revision.clone())?;
    model.mark_content_ready(&readiness(revision, DUNGEON_ID, 2))?;
    Ok(model)
}

fn action_contract(
    projection: &CoreWorldTransitionModel,
) -> (
    Option<TransitionUiAction>,
    Option<CoreWorldTransitionCopyKey>,
) {
    match projection.retry_directive() {
        CoreRetryDirective::SameMutation
        | CoreRetryDirective::RefreshAuthoritativeState
        | CoreRetryDirective::ReconnectTransport => (
            Some(TransitionUiAction::Retry),
            Some(CoreWorldTransitionCopyKey::ActionRetry),
        ),
        CoreRetryDirective::Unavailable
            if projection.phase() == CoreWorldTransitionPhase::ResolvedToCharacterSelect =>
        {
            (
                Some(TransitionUiAction::ReturnCharacterSelect),
                Some(CoreWorldTransitionCopyKey::ActionReturnCharacterSelect),
            )
        }
        CoreRetryDirective::Unavailable
            if projection.phase() == CoreWorldTransitionPhase::FatalError =>
        {
            (
                Some(TransitionUiAction::Exit),
                Some(CoreWorldTransitionCopyKey::ActionExit),
            )
        }
        CoreRetryDirective::Unavailable => (None, None),
    }
}

fn status_label(
    projection: &CoreWorldTransitionModel,
    state: CoreTransitionShowcaseState,
    content: &CoreDevelopmentWorldFlow,
) -> String {
    match state {
        CoreTransitionShowcaseState::Reconnecting => content
            .transition_copy(CoreWorldTransitionCopyKey::StatusReconnectAttempt)
            .replace(
                "{attempt}",
                &projection.reconnect_attempt().unwrap_or(1).to_string(),
            ),
        CoreTransitionShowcaseState::LinkLost => "SERVER AUTHORITY: 90 TICKS".to_owned(),
        CoreTransitionShowcaseState::SameStateRecovery => "REATTACHED / SAME LINEAGE".to_owned(),
        CoreTransitionShowcaseState::HallResolution => "HALLDEFAULT / VERSION 3".to_owned(),
        CoreTransitionShowcaseState::RecoverableError => "SAME MUTATION / RETRY SAFE".to_owned(),
        CoreTransitionShowcaseState::FatalError => "NO MUTATION RETRY".to_owned(),
        CoreTransitionShowcaseState::HallLoading => "CHARACTER SELECT -> LANTERN HALLS".to_owned(),
        CoreTransitionShowcaseState::DungeonLoading => "LANTERN HALLS -> BELL SEPULCHER".to_owned(),
    }
}

fn safe_origin_label(origin: CoreSafeOrigin) -> &'static str {
    match origin {
        CoreSafeOrigin::CharacterSelect => "CHARACTER SELECT",
        CoreSafeOrigin::LanternHalls => "LANTERN HALLS",
    }
}

const fn destination_label(state: CoreTransitionShowcaseState) -> &'static str {
    match state {
        CoreTransitionShowcaseState::HallLoading | CoreTransitionShowcaseState::HallResolution => {
            "LANTERN HALLS"
        }
        CoreTransitionShowcaseState::FatalError => "CONNECTION CLOSED",
        CoreTransitionShowcaseState::RecoverableError => "BELL SEPULCHER (PRESERVED)",
        CoreTransitionShowcaseState::DungeonLoading
        | CoreTransitionShowcaseState::LinkLost
        | CoreTransitionShowcaseState::Reconnecting
        | CoreTransitionShowcaseState::SameStateRecovery => "BELL SEPULCHER",
    }
}

fn content_revision(content: &CoreDevelopmentWorldFlow) -> Result<WorldFlowContentRevisionV1> {
    Ok(WorldFlowContentRevisionV1 {
        records_blake3: ManifestHash::new(content.hashes().records_blake3.clone())?,
        assets_blake3: ManifestHash::new(content.hashes().assets_blake3.clone())?,
        localization_blake3: ManifestHash::new(content.hashes().localization_blake3.clone())?,
    })
}

fn character_select(version: u64) -> CharacterLocationSnapshot {
    CharacterLocationSnapshot {
        character_id: [41; 16],
        character_version: version,
        location: CharacterLocation::CharacterSelect {
            next_hall_arrival: SafeArrival::SpawnAnchor {
                spawn_id: WireText::new("spawn.hub.character_select_return")
                    .expect("character-select return spawn"),
            },
        },
    }
}

fn hall(version: u64) -> CharacterLocationSnapshot {
    CharacterLocationSnapshot {
        character_id: [41; 16],
        character_version: version,
        location: CharacterLocation::Safe {
            location_id: WireText::new(HALL_ID).expect("Hall ID"),
            arrival: SafeArrival::HallDefault,
        },
    }
}

fn dungeon(version: u64) -> CharacterLocationSnapshot {
    CharacterLocationSnapshot {
        character_id: [41; 16],
        character_version: version,
        location: CharacterLocation::Danger {
            location_id: WireText::new(DUNGEON_ID).expect("dungeon ID"),
            instance_lineage_id: [42; 16],
            entry_restore_point_id: [43; 16],
        },
    }
}

fn dungeon_mutation(
    revision: WorldFlowContentRevisionV1,
    id: u8,
    expected_version: u64,
) -> WorldTransferMutation {
    transfer_mutation(
        revision,
        id,
        expected_version,
        WorldTransferCommand::UsePortal {
            portal_id: WireText::new("portal.dungeon.bell_sepulcher").expect("portal ID"),
        },
    )
}

fn transfer_mutation(
    revision: WorldFlowContentRevisionV1,
    id: u8,
    expected_version: u64,
    command: WorldTransferCommand,
) -> WorldTransferMutation {
    let payload = WorldTransferPayload {
        content_revision: revision,
        command,
    };
    WorldTransferMutation {
        mutation_id: [id; 16],
        character_id: [41; 16],
        expected_character_version: expected_version,
        issued_at_unix_millis: 1,
        payload_hash: payload.canonical_hash(),
        payload,
    }
}

fn accepted(
    mutation: &WorldTransferMutation,
    snapshot: CharacterLocationSnapshot,
) -> WorldFlowResult {
    WorldFlowResult::Transfer {
        request_sequence: 1,
        mutation_id: mutation.mutation_id,
        accepted: true,
        code: WorldTransferResultCode::Accepted,
        snapshot: Some(snapshot),
        transfer_id: Some([51; 16]),
    }
}

fn rejected(
    mutation: &WorldTransferMutation,
    code: WorldTransferResultCode,
    snapshot: Option<CharacterLocationSnapshot>,
) -> WorldFlowResult {
    WorldFlowResult::Transfer {
        request_sequence: 1,
        mutation_id: mutation.mutation_id,
        accepted: false,
        code,
        snapshot,
        transfer_id: None,
    }
}

fn readiness(
    revision: WorldFlowContentRevisionV1,
    location_id: &str,
    character_version: u64,
) -> CoreSceneReadiness {
    CoreSceneReadiness {
        location_id: WireText::new(location_id).expect("scene ID"),
        character_version,
        content_revision: revision,
    }
}

#[allow(clippy::needless_pass_by_value)]
fn spawn_transition_surface(mut commands: Commands, model: Res<ShowcaseViewModel>) {
    let accent = phase_accent(model.phase);
    commands
        .spawn((
            TransitionSurfaceRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(Color::srgb_u8(6, 8, 11)),
        ))
        .with_children(|root| {
            spawn_header(root, &model, accent);
            root.spawn(Node {
                width: percent(100),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Row,
                ..default()
            })
            .with_children(|body| {
                spawn_information_rail(body, &model, accent);
                spawn_protected_playfield(body, &model, accent);
            });
            spawn_footer(root, &model);
        });
}

fn spawn_transition_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn rebuild_transition_surface_if_missing(
    commands: Commands,
    model: Res<ShowcaseViewModel>,
    surfaces: Query<Entity, With<TransitionSurfaceRoot>>,
) {
    if surfaces.is_empty() {
        spawn_transition_surface(commands, model);
    }
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn advance_transition_soak(
    time: Res<Time<Real>>,
    mut soak: ResMut<TransitionSoakState>,
    mut model: ResMut<ShowcaseViewModel>,
    surfaces: Query<Entity, With<TransitionSurfaceRoot>>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let delta = time.delta();
    if soak.warmup_elapsed < Duration::from_secs(SOAK_WARMUP_SECONDS) {
        soak.warmup_elapsed = soak.warmup_elapsed.saturating_add(delta);
        return;
    }
    if soak.memory_samples.is_empty() {
        let resident_bytes = soak
            .memory
            .resident_bytes()
            .expect("transition-soak resident memory must remain available");
        soak.memory_samples.push(MemorySample {
            elapsed_ms: 0,
            resident_bytes,
        });
        soak.next_memory_sample = Duration::from_secs(SOAK_MEMORY_SAMPLE_SECONDS);
    }
    soak.measurement_elapsed = soak.measurement_elapsed.saturating_add(delta);
    soak.state_elapsed = soak.state_elapsed.saturating_add(delta);
    soak.frame_times_micros
        .push(u64::try_from(delta.as_micros()).unwrap_or(u64::MAX).max(1));
    if soak.state_elapsed >= Duration::from_secs(SOAK_STATE_SECONDS) {
        soak.state_elapsed = Duration::ZERO;
        soak.current_model = (soak.current_model + 1) % soak.models.len();
        *model = soak.models[soak.current_model].clone();
        soak.transitions_completed = soak.transitions_completed.saturating_add(1);
        for entity in &surfaces {
            commands.entity(entity).despawn();
        }
    }
    if soak.measurement_elapsed >= soak.next_memory_sample {
        sample_transition_soak_memory(&mut soak);
        soak.next_memory_sample = soak
            .next_memory_sample
            .saturating_add(Duration::from_secs(SOAK_MEMORY_SAMPLE_SECONDS));
    }
    if soak.measurement_elapsed < soak.measurement_duration {
        return;
    }
    let final_elapsed_ms = u64::try_from(soak.measurement_elapsed.as_millis()).unwrap_or(u64::MAX);
    if soak
        .memory_samples
        .last()
        .is_none_or(|sample| sample.elapsed_ms < final_elapsed_ms)
    {
        sample_transition_soak_memory(&mut soak);
    }
    let report = compile_transition_soak_report(&mut soak);
    publish_transition_soak_report(&soak.report_path, &report)
        .expect("transition-soak report must publish atomically");
    exit.write(AppExit::Success);
}

fn sample_transition_soak_memory(soak: &mut TransitionSoakState) {
    let resident_bytes = soak
        .memory
        .resident_bytes()
        .expect("transition-soak resident memory must remain available");
    soak.memory_samples.push(MemorySample {
        elapsed_ms: u64::try_from(soak.measurement_elapsed.as_millis()).unwrap_or(u64::MAX),
        resident_bytes,
    });
}

fn compile_transition_soak_report(
    soak: &mut TransitionSoakState,
) -> CoreTransitionPerformanceReport {
    let mut frame_times = std::mem::take(&mut soak.frame_times_micros);
    frame_times.sort_unstable();
    let frame_count = frame_times.len();
    let total_micros: u128 = frame_times.iter().map(|value| u128::from(*value)).sum();
    let measured_fps_milli = u64::try_from(
        u128::try_from(frame_count)
            .unwrap_or(u128::MAX)
            .saturating_mul(1_000_000_000)
            .checked_div(total_micros.max(1))
            .unwrap_or(0),
    )
    .unwrap_or(u64::MAX);
    let p95 = nearest_rank(&frame_times, 95);
    let p99 = nearest_rank(&frame_times, 99);
    let peak_resident_bytes = soak
        .memory_samples
        .iter()
        .map(|sample| sample.resident_bytes)
        .max()
        .unwrap_or(0);
    let memory_assessment = assess_transition_soak_memory(&soak.memory_samples);
    let duration_ms = u64::try_from(soak.measurement_elapsed.as_millis()).unwrap_or(u64::MAX);
    let accepted = soak.target_class_verified
        && soak.target_hardware.width_pixels == 1_920
        && soak.target_hardware.height_pixels == 1_080
        && duration_ms >= 30 * 60 * 1_000
        && soak.transitions_completed >= 8
        && measured_fps_milli >= 60_000
        && p95 <= 16_700
        && p99 <= 33_300
        && memory_assessment == MemoryAssessment::Pass;
    let mut report = CoreTransitionPerformanceReport {
        report_schema: "gravebound.performance.gb-m03-03f-transition.v1".to_owned(),
        build_id: soak.build_id.clone(),
        records_blake3: soak.records_blake3.clone(),
        assets_blake3: soak.assets_blake3.clone(),
        localization_blake3: soak.localization_blake3.clone(),
        duration_ms,
        state_interval_ms: SOAK_STATE_SECONDS * 1_000,
        transitions_completed: soak.transitions_completed,
        rendered_frame_count: frame_count,
        measured_fps_milli,
        p95_frame_time_micros: p95,
        p99_frame_time_micros: p99,
        memory_samples: soak.memory_samples.clone(),
        peak_resident_bytes,
        memory_assessment,
        target_hardware: soak.target_hardware.clone(),
        target_class_verified: soak.target_class_verified,
        accepted,
        raw_report_hash_blake3: String::new(),
    };
    report.raw_report_hash_blake3 = hash_transition_soak_report(&report);
    report
}

fn nearest_rank(sorted_values: &[u64], percentile: usize) -> u64 {
    let rank = sorted_values.len().saturating_mul(percentile).div_ceil(100);
    sorted_values
        .get(rank.saturating_sub(1))
        .copied()
        .unwrap_or(u64::MAX)
}

fn assess_transition_soak_memory(samples: &[MemorySample]) -> MemoryAssessment {
    let duration = samples.last().map_or(0, |sample| sample.elapsed_ms)
        - samples.first().map_or(0, |sample| sample.elapsed_ms);
    let peak = samples
        .iter()
        .map(|sample| sample.resident_bytes)
        .max()
        .unwrap_or(0);
    if duration < 30 * 60 * 1_000 {
        return MemoryAssessment::InsufficientDuration;
    }
    if peak > TARGET_MEMORY_BYTES {
        return MemoryAssessment::OverBudget;
    }
    let growth = samples
        .last()
        .map_or(0, |sample| sample.resident_bytes)
        .saturating_sub(samples.first().map_or(0, |sample| sample.resident_bytes));
    if growth >= MONOTONIC_GROWTH_FLOOR_BYTES
        && samples
            .windows(2)
            .all(|pair| pair[0].resident_bytes < pair[1].resident_bytes)
    {
        MemoryAssessment::MonotonicGrowth
    } else {
        MemoryAssessment::Pass
    }
}

fn hash_transition_soak_report(report: &CoreTransitionPerformanceReport) -> String {
    let mut hashable = report.clone();
    hashable.raw_report_hash_blake3.clear();
    let bytes = serde_json::to_vec(&hashable).expect("transition report must serialize");
    blake3::hash(&bytes).to_hex().to_string()
}

fn publish_transition_soak_report(
    path: &Path,
    report: &CoreTransitionPerformanceReport,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create report directory {}", parent.display()))?;
    }
    let temporary = path.with_extension("partial.json");
    let bytes =
        serde_json::to_vec_pretty(report).context("failed to serialize transition report")?;
    fs::write(&temporary, bytes)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    fs::rename(&temporary, path)
        .with_context(|| format!("failed to publish {}", path.display()))?;
    Ok(())
}

fn spawn_header(parent: &mut ChildSpawnerCommands, model: &ShowcaseViewModel, accent: Color) {
    parent
        .spawn((
            Node {
                width: percent(100),
                height: px(76),
                padding: UiRect::axes(px(24), px(14)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                border: UiRect::bottom(px(2)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(10, 13, 17)),
            BorderColor::all(accent),
        ))
        .with_children(|header| {
            spawn_text(
                header,
                format!("GRAVEBOUND  /  GB-M03-03F\n{}", model.title),
                19.0,
                Color::srgb_u8(239, 232, 208),
            );
            spawn_text(header, &model.status_label, 13.0, accent);
        });
}

fn spawn_information_rail(
    parent: &mut ChildSpawnerCommands,
    model: &ShowcaseViewModel,
    accent: Color,
) {
    parent
        .spawn((
            Node {
                width: percent(32),
                min_width: px(320),
                max_width: px(520),
                height: percent(100),
                padding: UiRect::all(px(24)),
                flex_direction: FlexDirection::Column,
                row_gap: px(18),
                border: UiRect::right(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgba_u8(12, 16, 20, 248)),
            BorderColor::all(Color::srgb_u8(54, 64, 66)),
        ))
        .with_children(|rail| {
            spawn_label(rail, "AUTHORITATIVE STATE", accent);
            spawn_text(rail, &model.detail, 18.0, Color::srgb_u8(240, 233, 211));
            spawn_divider(rail, accent);
            spawn_label(rail, "SAFE ORIGIN", Color::srgb_u8(126, 194, 171));
            spawn_text(
                rail,
                &model.safe_origin,
                20.0,
                Color::srgb_u8(224, 234, 219),
            );
            spawn_label(rail, "DESTINATION", Color::srgb_u8(198, 174, 116));
            spawn_text(
                rail,
                &model.destination,
                20.0,
                Color::srgb_u8(239, 226, 188),
            );
            rail.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            if let (Some(action), Some(label)) = (model.action, &model.action_label) {
                rail.spawn((
                    Button,
                    TransitionActionButton(action),
                    Node {
                        width: percent(100),
                        min_height: px(54),
                        padding: UiRect::axes(px(18), px(12)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(px(2)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb_u8(22, 30, 31)),
                    BorderColor::all(accent),
                ))
                .with_children(|button| {
                    spawn_text(button, label, 16.0, Color::srgb_u8(248, 242, 219));
                });
            }
            rail.spawn((
                Text::new("KEYBOARD + POINTER READY"),
                TextFont::from_font_size(11.0),
                TextColor(Color::srgb_u8(121, 136, 135)),
                TransitionActionStatus,
            ));
        });
}

fn spawn_protected_playfield(
    parent: &mut ChildSpawnerCommands,
    model: &ShowcaseViewModel,
    accent: Color,
) {
    parent
        .spawn((
            Node {
                flex_grow: 1.0,
                height: percent(100),
                position_type: PositionType::Relative,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(if model.reduced_effects {
                Color::srgb_u8(12, 17, 19)
            } else {
                Color::srgb_u8(9, 14, 17)
            }),
        ))
        .with_children(|field| {
            for (size, alpha) in [(66.0, 0.08), (48.0, 0.12), (30.0, 0.18)] {
                field.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent((100.0 - size) / 2.0),
                        top: percent((100.0 - size) / 2.0),
                        width: percent(size),
                        height: percent(size),
                        border: UiRect::all(px(if model.reduced_effects { 2 } else { 3 })),
                        border_radius: BorderRadius::all(percent(50)),
                        ..default()
                    },
                    BorderColor::all(accent.with_alpha(alpha)),
                ));
            }
            field
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(30),
                        top: percent(32),
                        width: percent(40),
                        min_height: px(160),
                        padding: UiRect::all(px(22)),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        row_gap: px(12),
                        border: UiRect::all(px(1)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba_u8(8, 12, 15, 220)),
                    BorderColor::all(Color::srgb_u8(44, 55, 57)),
                ))
                .with_children(|card| {
                    spawn_label(card, "PLAYFIELD CORRIDOR PRESERVED", accent);
                    spawn_text(
                        card,
                        &model.status_label,
                        17.0,
                        Color::srgb_u8(228, 219, 190),
                    );
                    spawn_text(
                        card,
                        if model.reduced_effects {
                            "REDUCED EFFECTS / IDENTICAL INFORMATION"
                        } else {
                            "STANDARD EFFECTS / PRESENTATION ONLY"
                        },
                        11.0,
                        Color::srgb_u8(118, 137, 135),
                    );
                });
        });
}

fn spawn_footer(parent: &mut ChildSpawnerCommands, model: &ShowcaseViewModel) {
    parent
        .spawn((
            Node {
                width: percent(100),
                height: px(42),
                padding: UiRect::axes(px(24), px(9)),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                border: UiRect::top(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(8, 11, 14)),
            BorderColor::all(Color::srgb_u8(40, 48, 50)),
        ))
        .with_children(|footer| {
            spawn_text(
                footer,
                format!(
                    "STATE {:?}  /  RESOLUTION {:?}",
                    model.state, model.resolution
                ),
                10.0,
                Color::srgb_u8(119, 132, 130),
            );
            spawn_text(
                footer,
                format!(
                    "CORE WORLD {}  /  NORMAL ROUTE DISABLED",
                    model.records_revision
                ),
                10.0,
                Color::srgb_u8(151, 127, 82),
            );
        });
}

fn spawn_label(parent: &mut ChildSpawnerCommands, value: &str, color: Color) {
    spawn_text(parent, value, 11.0, color);
}

fn spawn_divider(parent: &mut ChildSpawnerCommands, color: Color) {
    parent.spawn((
        Node {
            width: percent(100),
            height: px(1),
            ..default()
        },
        BackgroundColor(color.with_alpha(0.5)),
    ));
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

fn phase_accent(phase: CoreWorldTransitionPhase) -> Color {
    match phase {
        CoreWorldTransitionPhase::FatalError => Color::srgb_u8(211, 91, 84),
        CoreWorldTransitionPhase::RecoverableError => Color::srgb_u8(220, 174, 83),
        CoreWorldTransitionPhase::LinkLost | CoreWorldTransitionPhase::Reconnecting => {
            Color::srgb_u8(223, 144, 73)
        }
        CoreWorldTransitionPhase::ResolvedToHall | CoreWorldTransitionPhase::Ready => {
            Color::srgb_u8(104, 194, 157)
        }
        _ => Color::srgb_u8(112, 169, 192),
    }
}

#[allow(clippy::needless_pass_by_value)]
fn handle_transition_input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Query<(&Interaction, &TransitionActionButton), Changed<Interaction>>,
    model: Res<ShowcaseViewModel>,
    mut status: Query<&mut Text, With<TransitionActionStatus>>,
    mut exit: MessageWriter<AppExit>,
) {
    let keyboard_action = model.action.and_then(|action| {
        let pressed = match action {
            TransitionUiAction::Retry => keys.just_pressed(KeyCode::KeyR),
            TransitionUiAction::ReturnCharacterSelect => keys.just_pressed(KeyCode::Enter),
            TransitionUiAction::Exit => keys.just_pressed(KeyCode::Escape),
        };
        pressed.then_some(action)
    });
    let pointer_action = buttons.iter().find_map(|(interaction, button)| {
        (*interaction == Interaction::Pressed).then_some(button.0)
    });
    let Some(action) = keyboard_action.or(pointer_action) else {
        return;
    };
    if let Ok(mut value) = status.single_mut() {
        match action {
            TransitionUiAction::Retry => "RETRY REQUESTED / AUTHORITY UNCHANGED",
            TransitionUiAction::ReturnCharacterSelect => {
                "CHARACTER SELECT ROUTE REQUESTED / AUTHORITY UNCHANGED"
            }
            TransitionUiAction::Exit => "EXIT REQUESTED",
        }
        .clone_into(&mut value.0);
    }
    if action == TransitionUiAction::Exit {
        exit.write(AppExit::Success);
    }
}

type ChangedActionButtons<'w, 's> = Query<
    'w,
    's,
    (
        &'static Interaction,
        &'static mut BackgroundColor,
        &'static mut BorderColor,
    ),
    (Changed<Interaction>, With<TransitionActionButton>),
>;

fn style_action_button(mut buttons: ChangedActionButtons) {
    for (interaction, mut background, mut border) in &mut buttons {
        match interaction {
            Interaction::Pressed => {
                background.0 = Color::srgb_u8(42, 58, 56);
                *border = BorderColor::all(Color::srgb_u8(230, 218, 171));
            }
            Interaction::Hovered => {
                background.0 = Color::srgb_u8(30, 43, 42);
                *border = BorderColor::all(Color::srgb_u8(170, 207, 184));
            }
            Interaction::None => {
                background.0 = Color::srgb_u8(22, 30, 31);
            }
        }
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
    fn all_showcase_states_are_projection_driven_and_content_bound() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let content = load_core_development_world_flow(&root).unwrap();
        for state in [
            CoreTransitionShowcaseState::HallLoading,
            CoreTransitionShowcaseState::DungeonLoading,
            CoreTransitionShowcaseState::RecoverableError,
            CoreTransitionShowcaseState::FatalError,
            CoreTransitionShowcaseState::LinkLost,
            CoreTransitionShowcaseState::Reconnecting,
            CoreTransitionShowcaseState::SameStateRecovery,
            CoreTransitionShowcaseState::HallResolution,
        ] {
            for reduced_effects in [false, true] {
                let model = build_showcase_model(&content, state, reduced_effects).unwrap();
                assert_eq!(model.state, state);
                assert_eq!(model.reduced_effects, reduced_effects);
                assert!(!model.title.is_empty());
                assert!(!model.detail.is_empty());
                assert!(!model.safe_origin.is_empty());
                assert!(!model.destination.is_empty());
            }
        }
    }

    #[test]
    fn retry_and_fatal_actions_match_the_pure_projection() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let content = load_core_development_world_flow(&root).unwrap();
        let retry = build_showcase_model(
            &content,
            CoreTransitionShowcaseState::RecoverableError,
            false,
        )
        .unwrap();
        assert_eq!(retry.action, Some(TransitionUiAction::Retry));
        assert!(retry.action_label.unwrap().contains("RETRY"));
        let fatal =
            build_showcase_model(&content, CoreTransitionShowcaseState::FatalError, false).unwrap();
        assert_eq!(fatal.action, Some(TransitionUiAction::Exit));
        assert!(fatal.action_label.unwrap().contains("EXIT"));
    }

    #[test]
    fn reduced_effects_never_changes_information_or_actions() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let content = load_core_development_world_flow(&root).unwrap();
        for state in [
            CoreTransitionShowcaseState::HallLoading,
            CoreTransitionShowcaseState::DungeonLoading,
            CoreTransitionShowcaseState::RecoverableError,
            CoreTransitionShowcaseState::FatalError,
            CoreTransitionShowcaseState::LinkLost,
            CoreTransitionShowcaseState::Reconnecting,
            CoreTransitionShowcaseState::SameStateRecovery,
            CoreTransitionShowcaseState::HallResolution,
        ] {
            let standard = build_showcase_model(&content, state, false).unwrap();
            let reduced = build_showcase_model(&content, state, true).unwrap();
            assert_eq!(standard.phase, reduced.phase);
            assert_eq!(standard.resolution, reduced.resolution);
            assert_eq!(standard.title, reduced.title);
            assert_eq!(standard.detail, reduced.detail);
            assert_eq!(standard.safe_origin, reduced.safe_origin);
            assert_eq!(standard.destination, reduced.destination);
            assert_eq!(standard.action, reduced.action);
            assert_eq!(standard.action_label, reduced.action_label);
            assert_eq!(standard.status_label, reduced.status_label);
        }
    }

    #[test]
    fn transition_soak_memory_gate_requires_duration_budget_and_no_monotonic_leak() {
        assert_eq!(
            assess_transition_soak_memory(&[
                MemorySample {
                    elapsed_ms: 0,
                    resident_bytes: 300_000_000,
                },
                MemorySample {
                    elapsed_ms: 60_000,
                    resident_bytes: 301_000_000,
                },
            ]),
            MemoryAssessment::InsufficientDuration
        );
        assert_eq!(
            assess_transition_soak_memory(&[
                MemorySample {
                    elapsed_ms: 0,
                    resident_bytes: 300_000_000,
                },
                MemorySample {
                    elapsed_ms: 900_000,
                    resident_bytes: 305_000_000,
                },
                MemorySample {
                    elapsed_ms: 1_800_000,
                    resident_bytes: 304_000_000,
                },
            ]),
            MemoryAssessment::Pass
        );
        assert_eq!(
            assess_transition_soak_memory(&[
                MemorySample {
                    elapsed_ms: 0,
                    resident_bytes: 300_000_000,
                },
                MemorySample {
                    elapsed_ms: 900_000,
                    resident_bytes: 305_000_000,
                },
                MemorySample {
                    elapsed_ms: 1_800_000,
                    resident_bytes: 310_000_000,
                },
            ]),
            MemoryAssessment::MonotonicGrowth
        );
    }
}
