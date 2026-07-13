//! Native Core identity and character-select surface for `GB-M03-01C`.
//!
//! This mode contains no combat world and cannot route into the First Playable arena. Every roster
//! fact is a projection received from the authoritative identity service.

use std::{
    net::SocketAddr,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use bevy::{
    app::AppExit, prelude::*, render::view::screenshot::Screenshot, window::WindowResolution,
};
use protocol::{
    AccountBootstrapResult, AccountErrorCode, AccountSnapshot, AuthTicket,
    CORE_TEST_IDENTITY_FEATURE_FLAG, CharacterMutationFrame, CharacterMutationPayload,
    CharacterMutationResult, ClientHello, Compression, GRAVE_ARBALIST_CLASS_ID,
    M02_LOCAL_SERVER_NAME, M03_CORE_DEV_BUILD_ID, ManifestHash, Platform, ProgressionQueryFrame,
    ProtocolVersion, ReliableEvent, WireMessage, WireText,
};
use sim_content::{
    CoreDevelopmentIdentityCopy, load_and_validate, load_core_development_identity,
    load_core_development_identity_copy, load_core_development_oaths_bargains,
    load_core_development_progression,
};

use crate::{
    accessibility::AccessibilitySettings,
    network_transport::{
        NetworkStartup, NetworkTransportConfig, NetworkWorkerHandle, TransportEvent,
    },
    oath_ui::{OathUiAction, OathUiCopy, OathUiModel},
    progression_hud::ProgressionHudModel,
    save_screenshot_atomically,
};

const EVIDENCE_SETTLE_FRAMES: u8 = 60;

#[derive(Debug, Clone)]
pub struct CoreIdentityConfig {
    pub server_address: SocketAddr,
    pub certificate_path: PathBuf,
    pub test_token: String,
    pub content_root: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreIdentityPhase {
    Boot,
    PatchCheck,
    Authenticating,
    RosterLoading,
    EmptyRoster,
    RosterReady,
    CharacterCreation,
    Creating,
    Selecting,
    Selected,
    Disconnected,
    Disabled,
    Error,
}

#[derive(Debug, Clone, Resource)]
pub struct CoreIdentityModel {
    phase: CoreIdentityPhase,
    snapshot: Option<AccountSnapshot>,
    error: Option<AccountErrorCode>,
    transport_error: Option<String>,
}

impl Default for CoreIdentityModel {
    fn default() -> Self {
        Self {
            phase: CoreIdentityPhase::Boot,
            snapshot: None,
            error: None,
            transport_error: None,
        }
    }
}

impl CoreIdentityModel {
    #[must_use]
    pub const fn phase(&self) -> CoreIdentityPhase {
        self.phase
    }

    #[must_use]
    pub const fn snapshot(&self) -> Option<&AccountSnapshot> {
        self.snapshot.as_ref()
    }

    fn begin_authentication(&mut self) {
        self.phase = CoreIdentityPhase::Authenticating;
        self.error = None;
        self.transport_error = None;
    }

    fn handshake_accepted(&mut self) {
        self.phase = CoreIdentityPhase::RosterLoading;
    }

    fn apply_bootstrap(&mut self, result: AccountBootstrapResult) {
        match result {
            AccountBootstrapResult::Snapshot(snapshot) => self.set_snapshot(snapshot),
            AccountBootstrapResult::Error(error) => self.reject(error),
        }
    }

    fn apply_mutation(&mut self, result: CharacterMutationResult) {
        if let Some(snapshot) = result.snapshot {
            self.snapshot = Some(snapshot);
        }
        if result.accepted {
            self.error = None;
            self.phase = if self
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.selected_character_id)
                .is_some()
            {
                CoreIdentityPhase::Selected
            } else {
                CoreIdentityPhase::RosterReady
            };
        } else {
            self.reject(result.error.unwrap_or(AccountErrorCode::ServiceUnavailable));
        }
    }

    fn set_snapshot(&mut self, snapshot: AccountSnapshot) {
        self.phase = if snapshot.selected_character_id.is_some() {
            CoreIdentityPhase::Selected
        } else if snapshot.characters.is_empty() {
            CoreIdentityPhase::EmptyRoster
        } else {
            CoreIdentityPhase::RosterReady
        };
        self.snapshot = Some(snapshot);
        self.error = None;
        self.transport_error = None;
    }

    fn reject(&mut self, error: AccountErrorCode) {
        self.error = Some(error);
        self.phase = if matches!(
            error,
            AccountErrorCode::ProductionNamespaceForbidden
                | AccountErrorCode::ContentMismatch
                | AccountErrorCode::AppearanceUnavailable
        ) {
            CoreIdentityPhase::Disabled
        } else {
            CoreIdentityPhase::Error
        };
    }

    fn disconnected(&mut self) {
        self.phase = CoreIdentityPhase::Disconnected;
    }

    fn transport_failed(&mut self) {
        self.transport_error = Some("service_unavailable".to_owned());
        self.phase = CoreIdentityPhase::Error;
    }
}

#[derive(Debug, Clone, Resource)]
struct CoreUiCopy(CoreDevelopmentIdentityCopy);

impl CoreUiCopy {
    fn phase_label(&self, phase: CoreIdentityPhase) -> &str {
        let copy = &self.0.copy().phases;
        match phase {
            CoreIdentityPhase::Boot => &copy.boot,
            CoreIdentityPhase::PatchCheck => &copy.patch_check,
            CoreIdentityPhase::Authenticating => &copy.authenticating,
            CoreIdentityPhase::RosterLoading => &copy.roster_loading,
            CoreIdentityPhase::EmptyRoster => &copy.roster_empty,
            CoreIdentityPhase::RosterReady => &copy.roster_ready,
            CoreIdentityPhase::CharacterCreation => &copy.character_creation,
            CoreIdentityPhase::Creating => &copy.creating,
            CoreIdentityPhase::Selecting => &copy.selecting,
            CoreIdentityPhase::Selected => &copy.selected,
            CoreIdentityPhase::Disconnected => &copy.disconnected,
            CoreIdentityPhase::Disabled => &copy.disabled,
            CoreIdentityPhase::Error => &copy.error,
        }
    }
}

#[derive(Debug, Resource)]
struct CoreNetworkBridge(NetworkWorkerHandle);

#[derive(Debug, Resource)]
struct CoreMutationSequencer {
    next_mutation: u128,
}

impl Default for CoreMutationSequencer {
    fn default() -> Self {
        Self { next_mutation: 1 }
    }
}

impl CoreMutationSequencer {
    fn next_id(&mut self) -> Option<[u8; 16]> {
        let value = self.next_mutation;
        self.next_mutation = value.checked_add(1)?;
        Some(value.to_le_bytes())
    }
}

#[derive(Debug, Clone, Copy, Component, PartialEq, Eq)]
enum CoreAction {
    Create,
    Slot(u8),
    Retry,
}

#[derive(Component)]
struct CoreStatusText;

#[derive(Component)]
struct CoreRosterText;

#[derive(Component)]
struct CoreDetailText;

#[derive(Component)]
struct CoreProgressionHudText;

#[derive(Component)]
struct CoreOathText;

#[derive(Debug, Clone, Resource)]
struct CoreOathUiCopy(OathUiCopy);

#[derive(Debug, Default, Resource)]
struct CoreOathUiState(OathUiModel);

#[derive(Debug, Clone, Copy, Component)]
struct CoreOathActionLabel(OathUiAction);

#[derive(Debug, Resource)]
struct CoreProgressionQueryState {
    content_revision: ManifestHash,
    requested_character_id: Option<[u8; 16]>,
    next_sequence: u32,
}

#[derive(Component)]
struct CoreActionLabel(CoreAction);

#[derive(Debug, Resource)]
struct CoreScreenshotRequest(PathBuf);

#[derive(Debug, Default)]
struct CoreCaptureProgress {
    settled_frames: u8,
    queued: bool,
}

/// Validates the unpromoted Core boundary and opens the authoritative character-select client.
pub fn run_core_identity(config: CoreIdentityConfig) -> Result<()> {
    if config.test_token.trim().is_empty() {
        bail!("--identity must contain a nonempty wipeable test token");
    }
    let certificate_der = std::fs::read(&config.certificate_path).with_context(|| {
        format!(
            "failed to read Core identity server certificate {}",
            config.certificate_path.display()
        )
    })?;
    let identity_content = load_core_development_identity(&config.content_root)
        .context("unpromoted Core identity content failed validation")?;
    let identity_copy = load_core_development_identity_copy(&config.content_root)
        .context("Core identity UI copy failed validation")?;
    let (progression_content_revision, oath_copy) =
        load_core_supporting_content(&config.content_root)?;
    let (_, source_report) = load_and_validate(&config.content_root)
        .context("Core identity source package failed validation")?;
    if identity_content.class().header.id.as_str() != GRAVE_ARBALIST_CLASS_ID {
        bail!("Core identity compiler returned an unauthorized class");
    }
    let manifest_hash = ManifestHash::new(source_report.package_hash_blake3.clone())?;
    let hello = ClientHello {
        protocol_major: ProtocolVersion::current().major,
        protocol_minor: ProtocolVersion::current().minor,
        client_build_id: WireText::new(M03_CORE_DEV_BUILD_ID)?,
        platform: Platform::WindowsNative,
        supported_compression: vec![Compression::None],
        content_manifest_hash: manifest_hash.clone(),
        auth_ticket: AuthTicket::new(config.test_token.into_bytes())?,
        locale: WireText::new("en-US")?,
    };
    let worker = NetworkWorkerHandle::spawn(NetworkTransportConfig {
        server_address: config.server_address,
        server_name: M02_LOCAL_SERVER_NAME.to_owned(),
        certificate_der,
        hello,
        startup: NetworkStartup::CoreIdentity {
            content_manifest_hash: manifest_hash,
        },
    })?;

    let (screenshot_request, window_width, window_height) = core_window_configuration()?;
    let mut model = CoreIdentityModel {
        phase: CoreIdentityPhase::PatchCheck,
        ..default()
    };
    model.begin_authentication();

    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb_u8(6, 8, 12)))
        .insert_resource(AccessibilitySettings::default())
        .insert_resource(model)
        .insert_resource(CoreNetworkBridge(worker))
        .insert_resource(CoreMutationSequencer::default())
        .insert_resource(CoreUiCopy(identity_copy.clone()))
        .insert_resource(ProgressionHudModel::default())
        .insert_resource(CoreOathUiCopy(oath_copy))
        .insert_resource(CoreOathUiState::default())
        .insert_resource(CoreProgressionQueryState {
            content_revision: progression_content_revision,
            requested_character_id: None,
            next_sequence: 1,
        })
        .insert_resource(CoreEvidenceAutomation(screenshot_request.is_some()))
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: identity_copy.copy().window_title.clone(),
                        resolution: WindowResolution::new(window_width, window_height),
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_systems(Startup, spawn_core_identity_ui)
        .add_systems(
            Update,
            (
                poll_core_transport,
                request_selected_progression,
                request_selected_oath,
                handle_core_keyboard,
                handle_oath_keyboard,
                handle_core_buttons,
                handle_oath_buttons,
                automate_core_evidence,
                update_core_identity_ui,
            )
                .chain(),
        )
        .add_systems(Last, shutdown_core_transport);
    if let Some(path) = screenshot_request {
        app.insert_resource(CoreScreenshotRequest(path))
            .add_systems(
                Update,
                capture_core_identity_evidence.after(update_core_identity_ui),
            );
    }
    app.run();
    Ok(())
}

fn load_core_supporting_content(
    content_root: &std::path::Path,
) -> Result<(ManifestHash, OathUiCopy)> {
    let progression = load_core_development_progression(content_root)
        .context("Core progression content failed validation")?;
    let progression_revision = ManifestHash::new(progression.hashes().records_blake3.clone())?;
    let oaths = load_core_development_oaths_bargains(content_root)
        .context("Core Oath content failed validation")?;
    let oath_copy =
        OathUiCopy::from_catalog(&oaths).context("Core Oath UI copy failed validation")?;
    Ok((progression_revision, oath_copy))
}

fn core_window_configuration() -> Result<(Option<PathBuf>, u32, u32)> {
    let screenshot_request = std::env::var_os("GRAVEBOUND_SCREENSHOT_PATH").map(PathBuf::from);
    let (width, height) = crate::configured_window_size()?;
    Ok((screenshot_request, width, height))
}

#[derive(Debug, Resource)]
struct CoreEvidenceAutomation(bool);

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn poll_core_transport(
    bridge: Res<CoreNetworkBridge>,
    mut model: ResMut<CoreIdentityModel>,
    mut progression: ResMut<ProgressionHudModel>,
    mut oath: ResMut<CoreOathUiState>,
) {
    for event in bridge.0.drain_events() {
        match event {
            TransportEvent::Connecting => model.begin_authentication(),
            TransportEvent::HandshakeAccepted => model.handshake_accepted(),
            TransportEvent::Reliable(frame) => match frame.event {
                ReliableEvent::AccountBootstrapResult(result) => model.apply_bootstrap(result),
                ReliableEvent::CharacterMutationResult(result) => model.apply_mutation(result),
                ReliableEvent::ProgressionResult(result) => progression.apply(result),
                ReliableEvent::OathViewResult(result) => oath.0.apply_view(result),
                ReliableEvent::InitialOathSelectionResult(result) => oath.0.apply_selection(result),
                _ => model.transport_failed(),
            },
            TransportEvent::LinkLost
            | TransportEvent::Reconnecting { .. }
            | TransportEvent::TransportClosed => model.disconnected(),
            TransportEvent::Fatal(_) => model.transport_failed(),
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn request_selected_oath(
    bridge: Res<CoreNetworkBridge>,
    model: Res<CoreIdentityModel>,
    copy: Res<CoreOathUiCopy>,
    mut oath: ResMut<CoreOathUiState>,
) {
    let selected = model
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.selected_character_id);
    let Some(frame) = oath
        .0
        .request_for_selected(selected, copy.0.revision.clone())
    else {
        return;
    };
    if bridge
        .0
        .queue_reliable(WireMessage::OathViewFrame(frame))
        .is_err()
    {
        oath.0.request_failed();
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn request_selected_progression(
    bridge: Res<CoreNetworkBridge>,
    model: Res<CoreIdentityModel>,
    mut state: ResMut<CoreProgressionQueryState>,
    mut progression: ResMut<ProgressionHudModel>,
) {
    let selected = model
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.selected_character_id);
    if selected == state.requested_character_id {
        return;
    }
    state.requested_character_id = selected;
    progression.clear();
    let Some(character_id) = selected else {
        return;
    };
    let sequence = state.next_sequence;
    let Some(next_sequence) = sequence.checked_add(1) else {
        state.requested_character_id = None;
        return;
    };
    let frame = ProgressionQueryFrame {
        sequence,
        character_id,
        progression_content_revision: state.content_revision.clone(),
    };
    if bridge
        .0
        .queue_reliable(WireMessage::ProgressionQueryFrame(frame))
        .is_ok()
    {
        state.next_sequence = next_sequence;
    } else {
        state.requested_character_id = None;
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn handle_core_keyboard(
    keyboard: Res<ButtonInput<KeyCode>>,
    bridge: Res<CoreNetworkBridge>,
    mut model: ResMut<CoreIdentityModel>,
    mut sequencer: ResMut<CoreMutationSequencer>,
    oath: Res<CoreOathUiState>,
) {
    let action = if keyboard.just_pressed(KeyCode::Enter)
        && !oath.0.action_available(OathUiAction::Confirm)
    {
        Some(CoreAction::Create)
    } else if keyboard.just_pressed(KeyCode::Escape)
        && model.phase == CoreIdentityPhase::CharacterCreation
    {
        if let Some(snapshot) = model.snapshot.clone() {
            model.set_snapshot(snapshot);
        }
        None
    } else if keyboard.just_pressed(KeyCode::Digit1) {
        Some(CoreAction::Slot(1))
    } else if keyboard.just_pressed(KeyCode::Digit2) {
        Some(CoreAction::Slot(2))
    } else if keyboard.just_pressed(KeyCode::KeyR) {
        Some(CoreAction::Retry)
    } else {
        None
    };
    if let Some(action) = action {
        submit_core_action(action, &bridge, &mut model, &mut sequencer);
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn handle_oath_keyboard(
    keyboard: Res<ButtonInput<KeyCode>>,
    bridge: Res<CoreNetworkBridge>,
    copy: Res<CoreOathUiCopy>,
    mut oath: ResMut<CoreOathUiState>,
    mut sequencer: ResMut<CoreMutationSequencer>,
) {
    let action = if keyboard.just_pressed(KeyCode::KeyL) {
        Some(OathUiAction::LongVigil)
    } else if keyboard.just_pressed(KeyCode::KeyN) {
        Some(OathUiAction::Nailkeeper)
    } else if keyboard.just_pressed(KeyCode::Enter) {
        Some(OathUiAction::Confirm)
    } else if keyboard.just_pressed(KeyCode::Escape) {
        Some(OathUiAction::Cancel)
    } else {
        None
    };
    if let Some(action) = action {
        submit_oath_action(action, &bridge, &copy, &mut oath, &mut sequencer);
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn handle_core_buttons(
    interactions: Query<(&Interaction, &CoreActionLabel), Changed<Interaction>>,
    bridge: Res<CoreNetworkBridge>,
    mut model: ResMut<CoreIdentityModel>,
    mut sequencer: ResMut<CoreMutationSequencer>,
) {
    for (interaction, action) in &interactions {
        if *interaction == Interaction::Pressed {
            submit_core_action(action.0, &bridge, &mut model, &mut sequencer);
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn handle_oath_buttons(
    interactions: Query<(&Interaction, &CoreOathActionLabel), Changed<Interaction>>,
    bridge: Res<CoreNetworkBridge>,
    copy: Res<CoreOathUiCopy>,
    mut oath: ResMut<CoreOathUiState>,
    mut sequencer: ResMut<CoreMutationSequencer>,
) {
    for (interaction, action) in &interactions {
        if *interaction == Interaction::Pressed {
            submit_oath_action(action.0, &bridge, &copy, &mut oath, &mut sequencer);
        }
    }
}

fn submit_oath_action(
    action: OathUiAction,
    bridge: &CoreNetworkBridge,
    copy: &CoreOathUiCopy,
    oath: &mut CoreOathUiState,
    sequencer: &mut CoreMutationSequencer,
) {
    match action {
        OathUiAction::LongVigil | OathUiAction::Nailkeeper => oath.0.choose(action),
        OathUiAction::Cancel => oath.0.cancel(),
        OathUiAction::Confirm => {
            let Some(mutation_id) = sequencer.next_id() else {
                oath.0.mutation_failed();
                return;
            };
            let issued_at_unix_millis = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .and_then(|duration| u64::try_from(duration.as_millis()).ok());
            let Some(issued_at_unix_millis) = issued_at_unix_millis else {
                oath.0.mutation_failed();
                return;
            };
            let Some(frame) =
                oath.0
                    .confirm(mutation_id, issued_at_unix_millis, copy.0.revision.clone())
            else {
                return;
            };
            if bridge
                .0
                .queue_reliable(WireMessage::InitialOathSelectionFrame(frame))
                .is_err()
            {
                oath.0.mutation_failed();
            }
        }
    }
}

fn submit_core_action(
    action: CoreAction,
    bridge: &CoreNetworkBridge,
    model: &mut CoreIdentityModel,
    sequencer: &mut CoreMutationSequencer,
) {
    match action {
        CoreAction::Create
            if matches!(
                model.phase,
                CoreIdentityPhase::EmptyRoster
                    | CoreIdentityPhase::RosterReady
                    | CoreIdentityPhase::Selected
            ) && model.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.characters.len() < usize::from(snapshot.slot_capacity)
            }) =>
        {
            model.phase = CoreIdentityPhase::CharacterCreation;
        }
        CoreAction::Create if model.phase == CoreIdentityPhase::CharacterCreation => {
            let payload = CharacterMutationPayload::Create {
                class_id: WireText::new(GRAVE_ARBALIST_CLASS_ID)
                    .expect("approved class ID fits protocol bound"),
            };
            queue_mutation(
                bridge,
                model,
                sequencer,
                payload,
                CoreIdentityPhase::Creating,
            );
        }
        CoreAction::Slot(ordinal)
            if matches!(
                model.phase,
                CoreIdentityPhase::RosterReady | CoreIdentityPhase::Selected
            ) =>
        {
            let character_id = model.snapshot.as_ref().and_then(|snapshot| {
                snapshot
                    .characters
                    .iter()
                    .find(|character| character.roster_ordinal == ordinal)
                    .map(|character| character.character_id)
            });
            if let Some(character_id) = character_id {
                queue_mutation(
                    bridge,
                    model,
                    sequencer,
                    CharacterMutationPayload::Select { character_id },
                    CoreIdentityPhase::Selecting,
                );
            }
        }
        CoreAction::Retry if model.phase == CoreIdentityPhase::Error => {
            if let Some(snapshot) = model.snapshot.clone() {
                model.set_snapshot(snapshot);
            }
        }
        CoreAction::Retry if model.phase == CoreIdentityPhase::Disconnected => {
            model.begin_authentication();
        }
        _ => {}
    }
}

fn queue_mutation(
    bridge: &CoreNetworkBridge,
    model: &mut CoreIdentityModel,
    sequencer: &mut CoreMutationSequencer,
    payload: CharacterMutationPayload,
    pending: CoreIdentityPhase,
) {
    let Some(snapshot) = model.snapshot.as_ref() else {
        model.transport_failed();
        return;
    };
    let Some(mutation_id) = sequencer.next_id() else {
        model.transport_failed();
        return;
    };
    let issued_at_unix_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok());
    let Some(issued_at_unix_millis) = issued_at_unix_millis else {
        model.transport_failed();
        return;
    };
    let frame = CharacterMutationFrame {
        mutation_id,
        expected_account_version: snapshot.account_version,
        payload_hash: payload.canonical_hash(),
        issued_at_unix_millis,
        payload,
    };
    match bridge
        .0
        .queue_reliable(WireMessage::CharacterMutationFrame(frame))
    {
        Ok(()) => model.phase = pending,
        Err(_) => model.transport_failed(),
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn automate_core_evidence(
    automation: Res<CoreEvidenceAutomation>,
    bridge: Res<CoreNetworkBridge>,
    mut model: ResMut<CoreIdentityModel>,
    mut sequencer: ResMut<CoreMutationSequencer>,
) {
    if !automation.0 {
        return;
    }
    let action = match model.phase {
        CoreIdentityPhase::EmptyRoster | CoreIdentityPhase::CharacterCreation => {
            Some(CoreAction::Create)
        }
        CoreIdentityPhase::RosterReady
            if model.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.selected_character_id.is_none() && !snapshot.characters.is_empty()
            }) =>
        {
            Some(CoreAction::Slot(1))
        }
        _ => None,
    };
    if let Some(action) = action {
        submit_core_action(action, &bridge, &mut model, &mut sequencer);
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn shutdown_core_transport(bridge: Res<CoreNetworkBridge>, exits: MessageReader<AppExit>) {
    if !exits.is_empty() {
        bridge.0.shutdown();
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn spawn_core_identity_ui(mut commands: Commands, copy: Res<CoreUiCopy>) {
    let authored = copy.0.copy();
    commands.spawn(Camera2d);
    commands
        .spawn((
            Node {
                width: percent(100),
                max_width: px(1440),
                height: percent(100),
                margin: UiRect::horizontal(auto()),
                padding: UiRect::all(px(24)),
                flex_direction: FlexDirection::Column,
                row_gap: px(14),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(6, 8, 12)),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new(format!(
                    "{}\n{}",
                    authored.brand_header, authored.wipe_warning
                )),
                TextFont::from_font_size(22.0),
                TextColor(Color::srgb_u8(236, 220, 173)),
            ));
            root.spawn((
                Text::new(&authored.phases.authenticating),
                TextFont::from_font_size(16.0),
                TextColor(Color::srgb_u8(151, 208, 201)),
                CoreStatusText,
            ));
            spawn_progression_hud(root);
            spawn_oath_panel(root);
            root.spawn((
                Text::new(&authored.loading_roster),
                TextFont::from_font_size(18.0),
                TextColor(Color::srgb_u8(231, 226, 210)),
                Node {
                    min_height: px(210),
                    padding: UiRect::all(px(18)),
                    border: UiRect::all(px(2)),
                    ..default()
                },
                BackgroundColor(Color::srgba_u8(18, 23, 30, 245)),
                BorderColor::all(Color::srgb_u8(97, 122, 126)),
                CoreRosterText,
            ));
            root.spawn((Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(18),
                align_items: AlignItems::Center,
                ..default()
            },))
                .with_children(|detail_row| {
                    spawn_arbalist_silhouette(detail_row);
                    detail_row.spawn((
                        Text::new(render_class_detail(&copy.0)),
                        TextFont::from_font_size(15.0),
                        TextColor(Color::srgb_u8(201, 207, 196)),
                        CoreDetailText,
                    ));
                });
            root.spawn((Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(12),
                ..default()
            },))
                .with_children(|row| {
                    spawn_action_button(row, CoreAction::Create, &authored.create_action);
                    for ordinal in 1..=2 {
                        spawn_action_button(
                            row,
                            CoreAction::Slot(ordinal),
                            &render_template(
                                &authored.select_slot_action_template,
                                &[("ordinal", ordinal.to_string())],
                            ),
                        );
                    }
                    spawn_action_button(row, CoreAction::Retry, &authored.retry_action);
                });
            root.spawn((
                Text::new(render_template(
                    &authored.footer_template,
                    &[("unavailable", authored.closed_feature_literal.clone())],
                )),
                TextFont::from_font_size(14.0),
                TextColor(Color::srgb_u8(164, 169, 164)),
            ));
        });
}

fn spawn_oath_panel(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(190),
                padding: UiRect::all(px(14)),
                border: UiRect::all(px(2)),
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                ..default()
            },
            BackgroundColor(Color::srgba_u8(22, 17, 16, 245)),
            BorderColor::all(Color::srgb_u8(175, 139, 76)),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("OATH SHRINE\nSelect a living character to inspect Oath eligibility."),
                TextFont::from_font_size(15.0),
                TextColor(Color::srgb_u8(239, 224, 190)),
                CoreOathText,
            ));
            panel
                .spawn((Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(10),
                    row_gap: px(8),
                    ..default()
                },))
                .with_children(|actions| {
                    spawn_oath_button(actions, OathUiAction::LongVigil, "LONG VIGIL [L]");
                    spawn_oath_button(actions, OathUiAction::Nailkeeper, "NAILKEEPER [N]");
                    spawn_oath_button(
                        actions,
                        OathUiAction::Confirm,
                        "CONFIRM PERMANENT OATH [ENTER]",
                    );
                    spawn_oath_button(actions, OathUiAction::Cancel, "CANCEL [ESC]");
                });
        });
}

fn spawn_oath_button(parent: &mut ChildSpawnerCommands, action: OathUiAction, label: &str) {
    parent
        .spawn((
            Button,
            Node {
                min_width: px(184),
                min_height: px(44),
                padding: UiRect::axes(px(12), px(8)),
                border: UiRect::all(px(2)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(31, 27, 25)),
            BorderColor::all(Color::srgb_u8(88, 75, 58)),
            CoreOathActionLabel(action),
        ))
        .with_child((
            Text::new(label),
            TextFont::from_font_size(13.0),
            TextColor(Color::srgb_u8(239, 224, 190)),
        ));
}

fn spawn_progression_hud(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        Text::new("VITALS\nSelect a living character to load progression."),
        TextFont::from_font_size(16.0),
        TextColor(Color::srgb_u8(235, 232, 216)),
        Node {
            width: percent(48),
            min_width: px(440),
            padding: UiRect::axes(px(14), px(10)),
            border: UiRect::all(px(2)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(12, 18, 23, 245)),
        BorderColor::all(Color::srgb_u8(135, 186, 178)),
        CoreProgressionHudText,
    ));
}

fn spawn_action_button(parent: &mut ChildSpawnerCommands, action: CoreAction, label: &str) {
    parent
        .spawn((
            Button,
            Node {
                min_width: px(190),
                min_height: px(48),
                padding: UiRect::axes(px(12), px(9)),
                border: UiRect::all(px(2)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(25, 34, 40)),
            BorderColor::all(Color::srgb_u8(116, 150, 148)),
            CoreActionLabel(action),
        ))
        .with_child((
            Text::new(label),
            TextFont::from_font_size(14.0),
            TextColor(Color::srgb_u8(235, 232, 216)),
        ));
}

fn spawn_arbalist_silhouette(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Relative,
                width: px(92),
                height: px(116),
                border: UiRect::all(px(2)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(Color::srgb_u8(12, 18, 23)),
            BorderColor::all(Color::srgb_u8(116, 150, 148)),
        ))
        .with_children(|silhouette| {
            silhouette.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(34),
                    top: px(14),
                    width: px(24),
                    height: px(24),
                    border_radius: BorderRadius::all(px(12)),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(226, 218, 185)),
            ));
            silhouette.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(29),
                    top: px(37),
                    width: px(34),
                    height: px(58),
                    border: UiRect::all(px(2)),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(48, 83, 86)),
                BorderColor::all(Color::srgb_u8(176, 193, 175)),
            ));
            silhouette.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(9),
                    top: px(55),
                    width: px(74),
                    height: px(6),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(190, 148, 82)),
            ));
            silhouette.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(45),
                    top: px(47),
                    width: px(3),
                    height: px(28),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(232, 228, 207)),
            ));
        });
}

#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::type_complexity
)] // Bevy system parameters encode disjoint UI queries and resources.
fn update_core_identity_ui(
    model: Res<CoreIdentityModel>,
    copy: Res<CoreUiCopy>,
    progression: Res<ProgressionHudModel>,
    oath: Res<CoreOathUiState>,
    oath_copy: Res<CoreOathUiCopy>,
    mut status: Single<&mut Text, With<CoreStatusText>>,
    mut roster: Single<&mut Text, (With<CoreRosterText>, Without<CoreStatusText>)>,
    mut details: Single<
        &mut Text,
        (
            With<CoreDetailText>,
            Without<CoreStatusText>,
            Without<CoreRosterText>,
        ),
    >,
    mut progression_text: Single<
        &mut Text,
        (
            With<CoreProgressionHudText>,
            Without<CoreStatusText>,
            Without<CoreRosterText>,
            Without<CoreDetailText>,
            Without<CoreOathText>,
        ),
    >,
    mut oath_text: Single<
        &mut Text,
        (
            With<CoreOathText>,
            Without<CoreStatusText>,
            Without<CoreRosterText>,
            Without<CoreDetailText>,
            Without<CoreProgressionHudText>,
        ),
    >,
    mut actions: Query<
        (&CoreActionLabel, &mut BackgroundColor, &mut BorderColor),
        Without<CoreOathActionLabel>,
    >,
    mut oath_actions: Query<
        (&CoreOathActionLabel, &mut BackgroundColor, &mut BorderColor),
        Without<CoreActionLabel>,
    >,
) {
    let authored = copy.0.copy();
    let error = model
        .error
        .map(|error| format!(" - {}", account_error_code(error)))
        .or_else(|| {
            model
                .transport_error
                .as_ref()
                .map(|error| format!(" - {error}"))
        })
        .unwrap_or_default();
    **status = Text::new(render_template(
        &authored.status_template,
        &[
            ("phase", copy.phase_label(model.phase).to_owned()),
            ("error", error),
            ("major", ProtocolVersion::current().major.to_string()),
            ("minor", ProtocolVersion::current().minor.to_string()),
            ("feature_flag", CORE_TEST_IDENTITY_FEATURE_FLAG.to_owned()),
        ],
    ));
    **progression_text = Text::new(progression.render());
    **oath_text = Text::new(oath.0.render(&oath_copy.0));

    let mut rows = Vec::with_capacity(2);
    for ordinal in 1..=2 {
        let row = model.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .characters
                .iter()
                .find(|character| character.roster_ordinal == ordinal)
                .map(|character| {
                    let selected = if snapshot.selected_character_id == Some(character.character_id)
                    {
                        authored.selected_badge.as_str()
                    } else {
                        ""
                    };
                    render_template(
                        &authored.populated_slot_template,
                        &[
                            ("ordinal", ordinal.to_string()),
                            ("selected", selected.to_owned()),
                            ("class_name", copy.0.class_name().to_owned()),
                            ("level", character.level.to_string()),
                            ("not_equipped", authored.not_equipped_literal.clone()),
                        ],
                    )
                })
        });
        rows.push(row.unwrap_or_else(|| {
            render_template(
                &authored.empty_slot_template,
                &[
                    ("ordinal", ordinal.to_string()),
                    ("class_name", copy.0.class_name().to_owned()),
                ],
            )
        }));
    }
    **roster = Text::new(rows.join("\n\n"));
    **details = Text::new(render_class_detail(&copy.0));
    for (action, mut background, mut border) in &mut actions {
        if core_action_available(action.0, &model) {
            *background = BackgroundColor(Color::srgb_u8(25, 43, 47));
            *border = BorderColor::all(Color::srgb_u8(135, 186, 178));
        } else {
            *background = BackgroundColor(Color::srgb_u8(25, 29, 33));
            *border = BorderColor::all(Color::srgb_u8(74, 82, 84));
        }
    }
    for (action, mut background, mut border) in &mut oath_actions {
        if oath.0.action_available(action.0) {
            *background = BackgroundColor(Color::srgb_u8(58, 42, 29));
            *border = BorderColor::all(Color::srgb_u8(210, 168, 88));
        } else {
            *background = BackgroundColor(Color::srgb_u8(31, 27, 25));
            *border = BorderColor::all(Color::srgb_u8(88, 75, 58));
        }
    }
}

fn core_action_available(action: CoreAction, model: &CoreIdentityModel) -> bool {
    match action {
        CoreAction::Create => {
            model.phase == CoreIdentityPhase::CharacterCreation
                || (matches!(
                    model.phase,
                    CoreIdentityPhase::EmptyRoster
                        | CoreIdentityPhase::RosterReady
                        | CoreIdentityPhase::Selected
                ) && model.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.characters.len() < usize::from(snapshot.slot_capacity)
                }))
        }
        CoreAction::Slot(ordinal) => {
            matches!(
                model.phase,
                CoreIdentityPhase::RosterReady | CoreIdentityPhase::Selected
            ) && model.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot
                    .characters
                    .iter()
                    .any(|character| character.roster_ordinal == ordinal)
            })
        }
        CoreAction::Retry => matches!(
            model.phase,
            CoreIdentityPhase::Error | CoreIdentityPhase::Disconnected
        ),
    }
}

fn render_class_detail(copy: &CoreDevelopmentIdentityCopy) -> String {
    let authored = copy.copy();
    let mut rendered = render_template(
        &authored.class_detail_template,
        &[
            ("class_name", copy.class_name().to_owned()),
            ("not_equipped", authored.not_equipped_literal.clone()),
            ("unavailable", authored.closed_feature_literal.clone()),
        ],
    );
    rendered.push_str("\n\n");
    rendered.push_str(copy.class_description());
    for ability in copy.abilities() {
        rendered.push('\n');
        rendered.push_str(ability.name());
        rendered.push_str(" - ");
        rendered.push_str(ability.description());
    }
    rendered
}

fn render_template(template: &str, values: &[(&str, String)]) -> String {
    values
        .iter()
        .fold(template.to_owned(), |rendered, (key, value)| {
            rendered.replace(&format!("{{{key}}}"), value)
        })
}

const fn account_error_code(error: AccountErrorCode) -> &'static str {
    match error {
        AccountErrorCode::Unauthenticated => "unauthenticated",
        AccountErrorCode::ProductionNamespaceForbidden => "production_namespace_forbidden",
        AccountErrorCode::AccountMismatch => "account_mismatch",
        AccountErrorCode::CharacterNotFound => "character_not_found",
        AccountErrorCode::CharacterNotOwned => "character_not_owned",
        AccountErrorCode::CharacterDead => "character_dead",
        AccountErrorCode::ClassDisabled => "class_disabled",
        AccountErrorCode::AppearanceUnavailable => "appearance_unavailable",
        AccountErrorCode::InvalidName => "invalid_name",
        AccountErrorCode::CharacterSlotFull => "character_slot_full",
        AccountErrorCode::StateVersionMismatch => "state_version_mismatch",
        AccountErrorCode::IdempotencyConflict => "idempotency_conflict",
        AccountErrorCode::PayloadHashMismatch => "payload_hash_mismatch",
        AccountErrorCode::IssuedAtInvalid => "issued_at_invalid",
        AccountErrorCode::ContentMismatch => "content_mismatch",
        AccountErrorCode::RateLimited => "rate_limited",
        AccountErrorCode::ServiceUnavailable => "service_unavailable",
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn capture_core_identity_evidence(
    mut commands: Commands,
    request: Res<CoreScreenshotRequest>,
    model: Res<CoreIdentityModel>,
    mut progress: Local<CoreCaptureProgress>,
) {
    if progress.queued || model.phase != CoreIdentityPhase::Selected {
        return;
    }
    progress.settled_frames = progress.settled_frames.saturating_add(1);
    if progress.settled_frames >= EVIDENCE_SETTLE_FRAMES {
        progress.queued = true;
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_screenshot_atomically(request.0.clone()));
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use protocol::{
        AccountNamespace, CORE_CHARACTER_SLOT_CAPACITY, CharacterLifeState, CharacterSecurityState,
        CharacterSnapshot,
    };

    use super::*;

    fn character(ordinal: u8) -> CharacterSnapshot {
        CharacterSnapshot {
            character_id: [ordinal; 16],
            roster_ordinal: ordinal,
            class_id: WireText::new(GRAVE_ARBALIST_CLASS_ID).unwrap(),
            level: 1,
            oath_id: None,
            life_state: CharacterLifeState::Living,
            security_state: CharacterSecurityState::SafeCharacterSelect,
        }
    }

    fn snapshot(characters: Vec<CharacterSnapshot>, selected: Option<[u8; 16]>) -> AccountSnapshot {
        AccountSnapshot {
            namespace: AccountNamespace::WipeableTest,
            account_version: 1 + u64::try_from(characters.len()).unwrap(),
            slot_capacity: CORE_CHARACTER_SLOT_CAPACITY,
            characters,
            selected_character_id: selected,
        }
    }

    #[test]
    fn authoritative_snapshots_drive_empty_ready_and_selected_states() {
        let mut model = CoreIdentityModel::default();
        model.begin_authentication();
        assert_eq!(model.phase(), CoreIdentityPhase::Authenticating);
        model.handshake_accepted();
        assert_eq!(model.phase(), CoreIdentityPhase::RosterLoading);
        model.apply_bootstrap(AccountBootstrapResult::Snapshot(snapshot(Vec::new(), None)));
        assert_eq!(model.phase(), CoreIdentityPhase::EmptyRoster);
        model.apply_mutation(CharacterMutationResult {
            mutation_id: [1; 16],
            accepted: true,
            error: None,
            snapshot: Some(snapshot(vec![character(1)], None)),
        });
        assert_eq!(model.phase(), CoreIdentityPhase::RosterReady);
        model.apply_mutation(CharacterMutationResult {
            mutation_id: [2; 16],
            accepted: true,
            error: None,
            snapshot: Some(snapshot(vec![character(1)], Some([1; 16]))),
        });
        assert_eq!(model.phase(), CoreIdentityPhase::Selected);
    }

    #[test]
    fn errors_keep_the_last_safe_projection_and_classify_disabled_states() {
        let mut model = CoreIdentityModel::default();
        model.set_snapshot(snapshot(vec![character(1)], None));
        model.apply_mutation(CharacterMutationResult {
            mutation_id: [3; 16],
            accepted: false,
            error: Some(AccountErrorCode::StateVersionMismatch),
            snapshot: Some(snapshot(vec![character(1)], None)),
        });
        assert_eq!(model.phase(), CoreIdentityPhase::Error);
        assert_eq!(model.snapshot().unwrap().characters.len(), 1);
        model.apply_bootstrap(AccountBootstrapResult::Error(
            AccountErrorCode::ContentMismatch,
        ));
        assert_eq!(model.phase(), CoreIdentityPhase::Disabled);
        assert_eq!(model.snapshot().unwrap().characters.len(), 1);
    }

    #[test]
    fn mutation_ids_are_nonzero_monotonic_and_deterministic() {
        let mut sequencer = CoreMutationSequencer::default();
        assert_eq!(sequencer.next_id(), Some(1_u128.to_le_bytes()));
        assert_eq!(sequencer.next_id(), Some(2_u128.to_le_bytes()));
    }

    #[test]
    fn action_availability_tracks_authoritative_phase_and_capacity() {
        let mut model = CoreIdentityModel::default();
        model.set_snapshot(snapshot(vec![character(1)], Some([1; 16])));
        assert!(core_action_available(CoreAction::Create, &model));
        assert!(core_action_available(CoreAction::Slot(1), &model));
        assert!(!core_action_available(CoreAction::Slot(2), &model));
        assert!(!core_action_available(CoreAction::Retry, &model));

        model.set_snapshot(snapshot(vec![character(1), character(2)], None));
        assert!(!core_action_available(CoreAction::Create, &model));
        model.reject(AccountErrorCode::StateVersionMismatch);
        assert!(core_action_available(CoreAction::Retry, &model));
    }

    #[test]
    fn validated_copy_renders_without_unresolved_placeholders() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let copy = load_core_development_identity_copy(&root).unwrap();
        let rendered = render_class_detail(&copy);
        assert!(rendered.contains("Grave Arbalist"));
        assert!(rendered.contains("AVAILABLE IN A LATER TEST"));
        assert!(rendered.contains("Crossbow - Fire one narrow bolt"));
        assert!(!rendered.contains('{'));
        assert!(!rendered.contains('}'));
    }
}
