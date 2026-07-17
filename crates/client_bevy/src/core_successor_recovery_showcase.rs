//! Disposable real-widget coordinator for `GB-M03-07` native recovery evidence.
//!
//! This executable-only route composes the durable death projection, successor client authority,
//! Character Select recovery UI, and shared Hall transition model. It uses checked fixtures in
//! place of transport only after the real PostgreSQL/QUIC path has passed independently. It never
//! enables normal route admission or changes the production server/client startup path.

use std::{env, fs, path::PathBuf};

use anyhow::{Context, Result};
use bevy::{
    asset::io::{AssetSourceBuilder, AssetSourceId, file::FileAssetReader},
    prelude::*,
    render::view::screenshot::Screenshot,
    window::{PresentMode, PrimaryWindow, WindowResolution},
};
use protocol::{
    CORE_SUCCESSOR_FEATURE_FLAG, CharacterLocation, CharacterLocationSnapshot,
    GRAVE_ARBALIST_CLASS_ID, M03_CORE_DEV_BUILD_ID, ManifestHash, PROTOCOL_MAJOR, PROTOCOL_MINOR,
    SIMULATION_HZ, SNAPSHOT_HZ, SUCCESSOR_RESULT_HASH_BYTES, SUCCESSOR_SCHEMA_VERSION, SafeArrival,
    ServerHello, StoredSuccessorResultV1, SuccessorAppearanceSnapshotV1, SuccessorCreateResultV1,
    SuccessorStarterItemsV1, SuccessorVersionVectorV1, WireText, WorldFlowContentRevisionV1,
    WorldFlowFrame, WorldFlowRequest, WorldFlowResult, WorldTransferResultCode,
};
use sim_content::{
    CoreDevelopmentDeathView, CoreDevelopmentWorldFlow, CoreSuccessorRecoveryContent,
    load_core_development_death_view, load_core_development_world_flow,
    load_core_successor_recovery,
};

use crate::{
    DeathSummaryAction, DeathUiAction, DeathUiCommand, DeathUiConfig, DeathUiFocusRequest,
    DeathUiRenderReadiness, DeathUiSnapshot, NativeDeathView, NativeDeathViewPlugin,
    NativeSuccessorRecoveryPlugin, NativeSuccessorRecoveryView, SuccessorRecoveryClientModel,
    SuccessorRecoveryPhase, SuccessorRecoveryUiAction, SuccessorRecoveryUiCommand,
    SuccessorRecoveryUiConfig, SuccessorRecoveryUiReadiness, SuccessorRecoveryUiSnapshot,
    validate_death_ui_assets,
};

const EVIDENCE_SETTLE_FRAMES: u8 = 90;
const SHOWCASE_RESPONSE_FRAMES: u8 = 12;
const CREATE_MUTATION_ID: [u8; 16] = [0x12; 16];
const SUCCESSOR_ID: [u8; 16] = [0x13; 16];
const HALL_MUTATION_ID: [u8; 16] = [0x14; 16];
const CREATE_REQUEST_SEQUENCE: u32 = 1;
const HALL_REQUEST_SEQUENCE: u32 = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreSuccessorRecoveryShowcaseState {
    DeathSummary,
    Creating,
    RecoverableCreate,
    CharacterSelect,
    EnteringHall,
    LoadingHall,
    HallReady,
}

#[derive(Debug, Clone)]
pub struct CoreSuccessorRecoveryShowcaseConfig {
    pub content_root: PathBuf,
    pub reduced_effects: bool,
    pub ui_scale_percent: u16,
    pub state: CoreSuccessorRecoveryShowcaseState,
}

#[derive(Debug, Resource)]
struct ShowcaseRuntime {
    death: crate::DeathViewClientModel,
    death_id: [u8; 16],
    recovery: SuccessorRecoveryClientModel,
    content: CoreSuccessorRecoveryContent,
    world_revision: WorldFlowContentRevisionV1,
    ui_config: SuccessorRecoveryUiConfig,
    pending: Option<PendingAuthority>,
}

#[derive(Debug, Clone, Copy)]
enum PendingAuthority {
    Create {
        frames: u8,
    },
    HallResult {
        frames: u8,
        request_sequence: u32,
        mutation_id: [u8; 16],
    },
    HallReadiness {
        frames: u8,
    },
}

impl PendingAuthority {
    fn tick(&mut self) -> bool {
        let frames = match self {
            Self::Create { frames }
            | Self::HallResult { frames, .. }
            | Self::HallReadiness { frames } => frames,
        };
        *frames = frames.saturating_sub(1);
        *frames == 0
    }
}

enum InitialView {
    Death(Box<NativeDeathView>),
    Recovery(Box<NativeSuccessorRecoveryView>),
}

struct BuiltShowcase {
    runtime: ShowcaseRuntime,
    initial_view: InitialView,
}

#[derive(Debug, Resource)]
struct ScreenshotRequest(PathBuf);

#[derive(Debug, Resource)]
struct ShowcaseInitialDeathFocus {
    enabled: bool,
    issued: bool,
}

#[derive(Debug, Default)]
struct CaptureProgress {
    settled_frames: u8,
    queued: bool,
}

pub fn run_core_successor_recovery_showcase(
    config: &CoreSuccessorRecoveryShowcaseConfig,
) -> Result<()> {
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
    let death = load_core_development_death_view(&content_root)
        .context("successor showcase death presentation failed validation")?;
    let content = load_core_successor_recovery(&content_root)
        .context("successor showcase presentation target failed validation")?;
    let world = load_core_development_world_flow(&content_root)
        .context("successor showcase world-flow target failed validation")?;
    let built = build_showcase(config, death, content, &world)?;
    let (width, height) = crate::configured_window_size()?;
    let screenshot = env::var_os("GRAVEBOUND_SCREENSHOT_PATH").map(PathBuf::from);
    let asset_reader_root = asset_root.clone();
    let mut app = App::new();
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::new(move || Box::new(FileAssetReader::new(asset_reader_root.clone()))),
    )
    .insert_resource(ClearColor(Color::srgb_u8(5, 7, 9)))
    .insert_resource(built.runtime)
    .insert_resource(ShowcaseInitialDeathFocus {
        enabled: config.state == CoreSuccessorRecoveryShowcaseState::DeathSummary,
        issued: false,
    })
    .add_plugins(
        crate::gravebound_default_plugins()
            .set(ImagePlugin::default_nearest())
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Gravebound".to_owned(),
                    resolution: WindowResolution::new(width, height),
                    present_mode: PresentMode::AutoVsync,
                    resizable: true,
                    ..default()
                }),
                ..default()
            }),
    )
    .add_plugins((NativeDeathViewPlugin, NativeSuccessorRecoveryPlugin))
    .add_systems(Startup, spawn_camera)
    .add_systems(
        Update,
        (
            apply_initial_death_focus,
            handle_death_commands,
            handle_successor_commands,
            advance_fixture_authority,
        )
            .chain(),
    );
    match built.initial_view {
        InitialView::Death(view) => {
            app.insert_resource(*view);
        }
        InitialView::Recovery(view) => {
            app.insert_resource(*view);
        }
    }
    if let Some(path) = screenshot {
        app.insert_resource(ScreenshotRequest(path))
            .add_systems(Update, capture_evidence);
    }
    app.run();
    Ok(())
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((Camera2d, IsDefaultUiCamera, BoxShadowSamples(6)));
}

#[allow(clippy::needless_pass_by_value)]
fn apply_initial_death_focus(
    readiness: Res<DeathUiRenderReadiness>,
    mut initial: ResMut<ShowcaseInitialDeathFocus>,
    mut requests: MessageWriter<DeathUiFocusRequest>,
) {
    if initial.enabled && !initial.issued && readiness.is_ready() {
        requests.write(DeathUiFocusRequest::Next);
        initial.issued = true;
    }
}

fn build_showcase(
    config: &CoreSuccessorRecoveryShowcaseConfig,
    death_catalog: CoreDevelopmentDeathView,
    content: CoreSuccessorRecoveryContent,
    world: &CoreDevelopmentWorldFlow,
) -> Result<BuiltShowcase> {
    let death = crate::core_death_view_showcase::ready_terminal_model(death_catalog)?;
    let authority = death
        .terminal_successor_authority()
        .context("showcase terminal summary did not expose successor authority")?;
    let death_id = authority.death_id();
    let mut recovery =
        SuccessorRecoveryClientModel::new(&successor_hello(), item_revision(&content)?);
    recovery.observe_terminal_summary(authority)?;
    advance_to_state(&mut recovery, &content, world, death_id, config.state)?;
    let ui_config = SuccessorRecoveryUiConfig {
        reduced_effects: config.reduced_effects,
        ui_scale_percent: config.ui_scale_percent,
    };
    let initial_view = if config.state == CoreSuccessorRecoveryShowcaseState::DeathSummary {
        InitialView::Death(Box::new(NativeDeathView::new(
            DeathUiSnapshot::terminal_with_successor(&death, &recovery)?,
            DeathUiConfig {
                reduced_effects: config.reduced_effects,
                ui_scale_percent: config.ui_scale_percent,
            },
        )?))
    } else {
        InitialView::Recovery(Box::new(NativeSuccessorRecoveryView::new(
            SuccessorRecoveryUiSnapshot::project(&recovery, &content)?,
            ui_config,
        )?))
    };
    Ok(BuiltShowcase {
        runtime: ShowcaseRuntime {
            death,
            death_id,
            recovery,
            content,
            world_revision: world_revision(world)?,
            ui_config,
            pending: None,
        },
        initial_view,
    })
}

fn advance_to_state(
    recovery: &mut SuccessorRecoveryClientModel,
    content: &CoreSuccessorRecoveryContent,
    world: &CoreDevelopmentWorldFlow,
    death_id: [u8; 16],
    state: CoreSuccessorRecoveryShowcaseState,
) -> Result<()> {
    if state == CoreSuccessorRecoveryShowcaseState::DeathSummary {
        return Ok(());
    }
    recovery.begin_create(CREATE_MUTATION_ID)?;
    if state == CoreSuccessorRecoveryShowcaseState::Creating {
        return Ok(());
    }
    if state == CoreSuccessorRecoveryShowcaseState::RecoverableCreate {
        recovery.handle_create_response_loss()?;
        return Ok(());
    }
    apply_create_result(recovery, content, death_id)?;
    if state == CoreSuccessorRecoveryShowcaseState::CharacterSelect {
        return Ok(());
    }
    begin_play(recovery, world)?;
    if state == CoreSuccessorRecoveryShowcaseState::EnteringHall {
        return Ok(());
    }
    apply_hall_result(recovery, HALL_REQUEST_SEQUENCE, HALL_MUTATION_ID)?;
    if state == CoreSuccessorRecoveryShowcaseState::LoadingHall {
        return Ok(());
    }
    mark_hall_ready(recovery, world)?;
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn handle_death_commands(
    mut commands: Commands,
    mut messages: MessageReader<DeathUiCommand>,
    mut runtime: ResMut<ShowcaseRuntime>,
) {
    for message in messages.read() {
        if message.0 != DeathUiAction::Summary(DeathSummaryAction::CreateSuccessor)
            || runtime.pending.is_some()
            || runtime.recovery.phase() != SuccessorRecoveryPhase::Ready
        {
            continue;
        }
        if let Err(error) = runtime.recovery.begin_create(CREATE_MUTATION_ID) {
            error!(%error, "native successor showcase rejected Create Successor");
            continue;
        }
        runtime.pending = Some(PendingAuthority::Create {
            frames: SHOWCASE_RESPONSE_FRAMES,
        });
        show_recovery_view(&mut commands, &runtime);
    }
}

#[allow(clippy::needless_pass_by_value)]
fn handle_successor_commands(
    mut commands: Commands,
    mut messages: MessageReader<SuccessorRecoveryUiCommand>,
    mut runtime: ResMut<ShowcaseRuntime>,
) {
    for message in messages.read() {
        if runtime.pending.is_some() {
            continue;
        }
        let pending = match message.0 {
            SuccessorRecoveryUiAction::Play => {
                let revision = runtime.world_revision.clone();
                let result = begin_play_with_revision(
                    &mut runtime.recovery,
                    HALL_REQUEST_SEQUENCE,
                    HALL_MUTATION_ID,
                    revision,
                );
                match result {
                    Ok(frame) => pending_hall_result(&frame),
                    Err(error) => {
                        error!(%error, "native successor showcase rejected Play");
                        None
                    }
                }
            }
            SuccessorRecoveryUiAction::RetryCreate => match runtime.recovery.retry_create() {
                Ok(_) => Some(PendingAuthority::Create {
                    frames: SHOWCASE_RESPONSE_FRAMES,
                }),
                Err(error) => {
                    error!(%error, "native successor showcase rejected create retry");
                    None
                }
            },
            SuccessorRecoveryUiAction::RetryHall => {
                match runtime.recovery.retry_play(HALL_REQUEST_SEQUENCE + 1) {
                    Ok(frame) => pending_hall_result(&frame),
                    Err(error) => {
                        error!(%error, "native successor showcase rejected Hall retry");
                        None
                    }
                }
            }
            SuccessorRecoveryUiAction::RefreshDeathSummary => {
                reset_to_death_view(&mut commands, &mut runtime);
                None
            }
        };
        runtime.pending = pending;
        if runtime.recovery.phase() != SuccessorRecoveryPhase::Ready {
            show_recovery_view(&mut commands, &runtime);
        }
    }
}

fn pending_hall_result(frame: &WorldFlowFrame) -> Option<PendingAuthority> {
    let WorldFlowRequest::Transfer(mutation) = &frame.request else {
        return None;
    };
    Some(PendingAuthority::HallResult {
        frames: SHOWCASE_RESPONSE_FRAMES,
        request_sequence: frame.sequence,
        mutation_id: mutation.mutation_id,
    })
}

#[allow(clippy::needless_pass_by_value)]
fn advance_fixture_authority(mut commands: Commands, mut runtime: ResMut<ShowcaseRuntime>) {
    let Some(mut pending) = runtime.pending.take() else {
        return;
    };
    if !pending.tick() {
        runtime.pending = Some(pending);
        return;
    }
    let result = match pending {
        PendingAuthority::Create { .. } => {
            let runtime = &mut *runtime;
            apply_create_result(&mut runtime.recovery, &runtime.content, runtime.death_id)
        }
        PendingAuthority::HallResult {
            request_sequence,
            mutation_id,
            ..
        } => apply_hall_result(&mut runtime.recovery, request_sequence, mutation_id),
        PendingAuthority::HallReadiness { .. } => {
            let revision = runtime.world_revision.clone();
            mark_hall_ready_with_revision(&mut runtime.recovery, revision)
        }
    };
    if let Err(error) = result {
        error!(%error, "native successor showcase fixture transition failed");
        return;
    }
    runtime.pending = match pending {
        PendingAuthority::HallResult { .. } => Some(PendingAuthority::HallReadiness {
            frames: SHOWCASE_RESPONSE_FRAMES,
        }),
        PendingAuthority::Create { .. } | PendingAuthority::HallReadiness { .. } => None,
    };
    show_recovery_view(&mut commands, &runtime);
}

fn show_recovery_view(commands: &mut Commands, runtime: &ShowcaseRuntime) {
    let result = SuccessorRecoveryUiSnapshot::project(&runtime.recovery, &runtime.content)
        .and_then(|snapshot| NativeSuccessorRecoveryView::new(snapshot, runtime.ui_config));
    match result {
        Ok(view) => {
            commands.remove_resource::<NativeDeathView>();
            commands.insert_resource(view);
        }
        Err(error) => error!(%error, "native successor showcase projection failed"),
    }
}

fn reset_to_death_view(commands: &mut Commands, runtime: &mut ShowcaseRuntime) {
    let Some(authority) = runtime.death.terminal_successor_authority() else {
        error!("native successor showcase lost durable death authority");
        return;
    };
    let Ok(revision) = item_revision(&runtime.content) else {
        error!("native successor showcase item revision became invalid");
        return;
    };
    let mut recovery = SuccessorRecoveryClientModel::new(&successor_hello(), revision);
    if let Err(error) = recovery.observe_terminal_summary(authority) {
        error!(%error, "native successor showcase could not restore terminal authority");
        return;
    }
    let snapshot = DeathUiSnapshot::terminal_with_successor(&runtime.death, &recovery);
    let view = snapshot.and_then(|snapshot| {
        NativeDeathView::new(
            snapshot,
            DeathUiConfig {
                reduced_effects: runtime.ui_config.reduced_effects,
                ui_scale_percent: runtime.ui_config.ui_scale_percent,
            },
        )
    });
    match view {
        Ok(view) => {
            runtime.recovery = recovery;
            runtime.pending = None;
            commands.remove_resource::<NativeSuccessorRecoveryView>();
            commands.insert_resource(view);
        }
        Err(error) => error!(%error, "native successor showcase could not restore death view"),
    }
}

fn apply_create_result(
    recovery: &mut SuccessorRecoveryClientModel,
    content: &CoreSuccessorRecoveryContent,
    death_id: [u8; 16],
) -> Result<()> {
    recovery.apply_create_result(&SuccessorCreateResultV1::Stored {
        schema_version: SUCCESSOR_SCHEMA_VERSION,
        request_sequence: CREATE_REQUEST_SEQUENCE,
        replayed: false,
        result: Box::new(stored_result(content, death_id)?),
    })?;
    Ok(())
}

fn begin_play(
    recovery: &mut SuccessorRecoveryClientModel,
    world: &CoreDevelopmentWorldFlow,
) -> Result<WorldFlowFrame> {
    begin_play_with_revision(
        recovery,
        HALL_REQUEST_SEQUENCE,
        HALL_MUTATION_ID,
        world_revision(world)?,
    )
}

fn begin_play_with_revision(
    recovery: &mut SuccessorRecoveryClientModel,
    request_sequence: u32,
    mutation_id: [u8; 16],
    revision: WorldFlowContentRevisionV1,
) -> Result<WorldFlowFrame> {
    Ok(recovery.begin_play(request_sequence, mutation_id, 50_000, revision)?)
}

fn apply_hall_result(
    recovery: &mut SuccessorRecoveryClientModel,
    request_sequence: u32,
    mutation_id: [u8; 16],
) -> Result<()> {
    recovery.apply_hall_result(&WorldFlowResult::Transfer {
        request_sequence,
        mutation_id,
        accepted: true,
        code: WorldTransferResultCode::Accepted,
        snapshot: Some(CharacterLocationSnapshot {
            character_id: SUCCESSOR_ID,
            character_version: 2,
            location: CharacterLocation::Safe {
                location_id: WireText::new("hub.lantern_halls_01")?,
                arrival: SafeArrival::HallDefault,
            },
        }),
        transfer_id: Some([0x15; 16]),
    })?;
    Ok(())
}

fn mark_hall_ready(
    recovery: &mut SuccessorRecoveryClientModel,
    world: &CoreDevelopmentWorldFlow,
) -> Result<()> {
    mark_hall_ready_with_revision(recovery, world_revision(world)?)
}

fn mark_hall_ready_with_revision(
    recovery: &mut SuccessorRecoveryClientModel,
    revision: WorldFlowContentRevisionV1,
) -> Result<()> {
    recovery.mark_hall_content_ready(&crate::CoreSceneReadiness {
        location_id: WireText::new("hub.lantern_halls_01")?,
        character_version: 2,
        content_revision: revision,
    })?;
    Ok(())
}

fn stored_result(
    content: &CoreSuccessorRecoveryContent,
    death_id: [u8; 16],
) -> Result<StoredSuccessorResultV1> {
    let mut stored = StoredSuccessorResultV1 {
        mutation_id: CREATE_MUTATION_ID,
        death_id,
        successor_id: SUCCESSOR_ID,
        receipt_id: [0x16; 16],
        former_roster_ordinal: 2,
        class_id: WireText::new(GRAVE_ARBALIST_CLASS_ID)?,
        appearance: SuccessorAppearanceSnapshotV1::CoreBaseSilhouette,
        starter_items: SuccessorStarterItemsV1 {
            weapon_uid: [0x17; 16],
            relic_uid: [0x18; 16],
            tonic_unit_uids: [[0x19; 16], [0x1A; 16]],
        },
        versions: SuccessorVersionVectorV1 {
            account: 12,
            character: 1,
            progression: 1,
            world: 1,
            inventory: 1,
            life_metrics: 1,
            oath_bargain: 1,
        },
        content_revision: item_revision(content)?,
        selected_character_id: SUCCESSOR_ID,
        result_hash: [0; SUCCESSOR_RESULT_HASH_BYTES],
    };
    stored.result_hash = stored.canonical_result_hash();
    Ok(stored)
}

fn item_revision(
    content: &CoreSuccessorRecoveryContent,
) -> Result<WireText<{ protocol::SUCCESSOR_CONTENT_ID_MAX_BYTES }>> {
    Ok(WireText::new(content.item_content_revision().to_owned())?)
}

fn successor_hello() -> ServerHello {
    ServerHello {
        session_id: WireText::new("successor-native-showcase").expect("bounded session ID"),
        protocol_major: PROTOCOL_MAJOR,
        protocol_minor: PROTOCOL_MINOR,
        required_client_build: WireText::new(M03_CORE_DEV_BUILD_ID).expect("bounded build ID"),
        content_bundle_version: WireText::new("core-test").expect("bounded content version"),
        server_tick_rate: SIMULATION_HZ,
        snapshot_rate: SNAPSHOT_HZ,
        region_id: WireText::new("local").expect("bounded region"),
        feature_flags: vec![
            WireText::new(CORE_SUCCESSOR_FEATURE_FLAG).expect("bounded successor feature"),
        ],
    }
}

fn world_revision(world: &CoreDevelopmentWorldFlow) -> Result<WorldFlowContentRevisionV1> {
    Ok(WorldFlowContentRevisionV1 {
        records_blake3: ManifestHash::new(world.hashes().records_blake3.clone())?,
        assets_blake3: ManifestHash::new(world.hashes().assets_blake3.clone())?,
        localization_blake3: ManifestHash::new(world.hashes().localization_blake3.clone())?,
    })
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn capture_evidence(
    mut commands: Commands,
    request: Res<ScreenshotRequest>,
    death_view: Option<Res<NativeDeathView>>,
    recovery_view: Option<Res<NativeSuccessorRecoveryView>>,
    death_ready: Res<DeathUiRenderReadiness>,
    recovery_ready: Res<SuccessorRecoveryUiReadiness>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut progress: Local<CaptureProgress>,
) {
    let ready = (death_view.is_some() && death_ready.is_ready())
        || (recovery_view.is_some() && recovery_ready.is_ready());
    if progress.queued || !ready || windows.single().is_err() {
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

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn config(state: CoreSuccessorRecoveryShowcaseState) -> CoreSuccessorRecoveryShowcaseConfig {
        CoreSuccessorRecoveryShowcaseConfig {
            content_root: root(),
            reduced_effects: false,
            ui_scale_percent: 100,
            state,
        }
    }

    fn build(state: CoreSuccessorRecoveryShowcaseState) -> BuiltShowcase {
        let root = root();
        let death = load_core_development_death_view(&root).unwrap();
        let content = load_core_successor_recovery(&root).unwrap();
        let world = load_core_development_world_flow(&root).unwrap();
        build_showcase(&config(state), death, content, &world).unwrap()
    }

    #[test]
    fn every_evidence_state_uses_the_real_models_and_expected_surface() {
        let cases = [
            (
                CoreSuccessorRecoveryShowcaseState::DeathSummary,
                SuccessorRecoveryPhase::Ready,
                None,
            ),
            (
                CoreSuccessorRecoveryShowcaseState::Creating,
                SuccessorRecoveryPhase::Submitting,
                Some(crate::SuccessorRecoveryUiSurface::Creating),
            ),
            (
                CoreSuccessorRecoveryShowcaseState::RecoverableCreate,
                SuccessorRecoveryPhase::RecoverableError,
                Some(crate::SuccessorRecoveryUiSurface::RecoverableCreate),
            ),
            (
                CoreSuccessorRecoveryShowcaseState::CharacterSelect,
                SuccessorRecoveryPhase::CharacterSelect,
                Some(crate::SuccessorRecoveryUiSurface::CharacterSelect),
            ),
            (
                CoreSuccessorRecoveryShowcaseState::EnteringHall,
                SuccessorRecoveryPhase::EnteringHall,
                Some(crate::SuccessorRecoveryUiSurface::EnteringHall),
            ),
            (
                CoreSuccessorRecoveryShowcaseState::LoadingHall,
                SuccessorRecoveryPhase::LoadingHall,
                Some(crate::SuccessorRecoveryUiSurface::LoadingHall),
            ),
            (
                CoreSuccessorRecoveryShowcaseState::HallReady,
                SuccessorRecoveryPhase::ControllableHall,
                Some(crate::SuccessorRecoveryUiSurface::HallReady),
            ),
        ];
        for (state, phase, surface) in cases {
            let built = build(state);
            assert_eq!(built.runtime.recovery.phase(), phase, "{state:?}");
            match (built.initial_view, surface) {
                (InitialView::Death(view), None) => {
                    assert!(view.snapshot().actions()[0].enabled);
                }
                (InitialView::Recovery(view), Some(expected)) => {
                    assert_eq!(view.snapshot().surface, expected);
                }
                _ => panic!("evidence state selected the wrong native surface: {state:?}"),
            }
        }
    }

    #[test]
    fn exact_command_sequence_reaches_control_only_after_both_authorities() {
        let mut built = build(CoreSuccessorRecoveryShowcaseState::DeathSummary);
        built
            .runtime
            .recovery
            .begin_create(CREATE_MUTATION_ID)
            .unwrap();
        assert_eq!(built.runtime.recovery.confirmations(), 1);
        apply_create_result(
            &mut built.runtime.recovery,
            &built.runtime.content,
            built.runtime.death_id,
        )
        .unwrap();
        assert_eq!(
            built.runtime.recovery.phase(),
            SuccessorRecoveryPhase::CharacterSelect
        );
        begin_play_with_revision(
            &mut built.runtime.recovery,
            HALL_REQUEST_SEQUENCE,
            HALL_MUTATION_ID,
            built.runtime.world_revision.clone(),
        )
        .unwrap();
        assert_eq!(built.runtime.recovery.confirmations(), 2);
        apply_hall_result(
            &mut built.runtime.recovery,
            HALL_REQUEST_SEQUENCE,
            HALL_MUTATION_ID,
        )
        .unwrap();
        assert_eq!(
            built.runtime.recovery.phase(),
            SuccessorRecoveryPhase::LoadingHall
        );
        mark_hall_ready_with_revision(&mut built.runtime.recovery, built.runtime.world_revision)
            .unwrap();
        assert_eq!(
            built.runtime.recovery.phase(),
            SuccessorRecoveryPhase::ControllableHall
        );
    }
}
