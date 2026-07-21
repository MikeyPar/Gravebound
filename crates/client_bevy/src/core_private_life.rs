//! Ordinary native client route for the wipeable M03 private character life.
//!
//! The three design authorities are `Gravebound_Production_GDD_v1_Canonical.md`,
//! `Gravebound_Content_Production_Spec_v1.md`, and
//! `Gravebound_Development_Roadmap_v1.md`. This client never infers admission, destinations,
//! collision, combat results, or terminal outcomes. Character Select, Hall, and dangerous-scene
//! control are exposed only after the negotiated server capability and matching durable route
//! projections agree with the exact locally compiled Core content.

use std::{
    collections::BTreeMap,
    net::SocketAddr,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use bevy::{app::AppExit, camera::ScalingMode, prelude::*, window::WindowResolution};
use protocol::{
    AccountBootstrapResult, AccountSnapshot, AuthTicket, CORE_WORLD_FLOW_FEATURE_FLAG,
    CharacterLocation, CharacterLocationSnapshot, CharacterMutationFrame, CharacterMutationPayload,
    CharacterMutationResult, ClientHello, Compression, CorePrivateRouteContentRevisionV1,
    CorePrivateRouteSceneV1, M02_LOCAL_SERVER_NAME, M03_CORE_DEV_BUILD_ID, ManifestHash, Platform,
    ProtocolVersion, ReliableEvent, ReliableEventFrame, ServerHello, WireMessage, WireText,
    WorldFlowContentRevisionV1, WorldFlowFrame, WorldFlowRequest, WorldFlowResult,
    WorldTransferCommand, WorldTransferMutation, WorldTransferPayload, WorldTransferResultCode,
};
use sim_content::{load_and_validate, load_core_private_life_content};
use thiserror::Error;

use crate::{
    CorePrivateRouteClientError, CorePrivateRouteClientModel, CorePrivateRouteClientPhase,
    CorePrivateSceneReadiness, CoreSceneReadiness,
    accessibility::AccessibilitySettings,
    network_prediction::{CompleteSnapshot, SnapshotAssembler},
    network_transport::{
        NetworkStartup, NetworkTransportConfig, NetworkWorkerHandle, TransportEvent,
    },
};

const WINDOW_TITLE: &str = "Gravebound - Core Private Life";
const REALM_GATE_ID: &str = "station.realm_gate";
const RUN_ENTITY_ID_STRIDE: u64 = 100_000;
const PLAYER_ENTITY_ID_OFFSET: u64 = 10_000;
const MAX_BUFFERED_PRIVATE_SNAPSHOT_CHUNKS: usize = 128;

#[derive(Debug, Clone)]
pub struct CorePrivateLifeConfig {
    pub server_address: SocketAddr,
    pub certificate_path: PathBuf,
    pub test_token: String,
    pub content_root: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePrivateLifePhase {
    Connecting,
    CharacterSelect,
    Selecting,
    EnteringHall,
    LoadingAuthority,
    Hall,
    PrivateRoute,
    TerminalPending,
    Disconnected,
    Disabled,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateLifeAction {
    Create,
    Select(u8),
    Play,
    RealmGate,
    Retry,
}

#[derive(Debug, Resource)]
struct CorePrivateLifeBridge(NetworkWorkerHandle);

#[derive(Debug, Resource)]
struct CorePrivatePresentationContent(sim_content::CorePrivateLifeContent);

#[derive(Debug, Resource, Default)]
struct CorePrivateSnapshotClient {
    actor_generation: Option<u64>,
    route_state_version: Option<u64>,
    local_entity_id: Option<u64>,
    assembler: SnapshotAssembler,
    buffered: BTreeMap<(u64, u32, u16), protocol::SnapshotChunk>,
    latest: Option<CompleteSnapshot>,
}

impl CorePrivateSnapshotClient {
    fn reset_transport(&mut self) {
        *self = Self::default();
    }

    fn bind_route(
        &mut self,
        route: Option<&protocol::CorePrivateRouteStateV1>,
    ) -> Result<(), CorePrivateLifeClientError> {
        let Some(route) = route.filter(|state| {
            matches!(
                state.scene,
                CorePrivateRouteSceneV1::CoreMicrorealm | CorePrivateRouteSceneV1::BellSepulcher
            )
        }) else {
            self.actor_generation = None;
            self.route_state_version = None;
            self.local_entity_id = None;
            self.assembler = SnapshotAssembler::default();
            self.buffered.clear();
            self.latest = None;
            return Ok(());
        };
        let generation_changed = self.actor_generation != Some(route.actor_generation);
        if generation_changed {
            self.actor_generation = Some(route.actor_generation);
            self.local_entity_id = Some(private_player_entity_id(route.actor_generation)?);
            self.assembler = SnapshotAssembler::default();
            self.latest = None;
        }
        self.route_state_version = Some(route.state_version);
        self.buffered
            .retain(|(version, _, _), _| *version >= route.state_version);
        let ready = self
            .buffered
            .extract_if(.., |(version, _, _), _| *version == route.state_version)
            .map(|(_, chunk)| chunk)
            .collect::<Vec<_>>();
        for chunk in ready {
            self.apply_bound_chunk(chunk)?;
        }
        Ok(())
    }

    fn ingest(&mut self, chunk: protocol::SnapshotChunk) -> Result<(), CorePrivateLifeClientError> {
        chunk
            .validate()
            .map_err(|_| CorePrivateLifeClientError::InvalidSnapshotAuthority)?;
        match self.route_state_version {
            Some(version) if chunk.state_version < version => Ok(()),
            Some(version) if chunk.state_version == version => self.apply_bound_chunk(chunk),
            _ => {
                if self.buffered.len() == MAX_BUFFERED_PRIVATE_SNAPSHOT_CHUNKS {
                    self.buffered.pop_first();
                }
                self.buffered.insert(
                    (chunk.state_version, chunk.sequence, chunk.chunk_index),
                    chunk,
                );
                Ok(())
            }
        }
    }

    fn apply_bound_chunk(
        &mut self,
        chunk: protocol::SnapshotChunk,
    ) -> Result<(), CorePrivateLifeClientError> {
        if Some(chunk.state_version) != self.route_state_version {
            return Err(CorePrivateLifeClientError::InvalidSnapshotAuthority);
        }
        let Some(snapshot) = self
            .assembler
            .push(chunk)
            .map_err(|_| CorePrivateLifeClientError::InvalidSnapshotAuthority)?
        else {
            return Ok(());
        };
        let expected = self
            .local_entity_id
            .ok_or(CorePrivateLifeClientError::InvalidSnapshotAuthority)?;
        let mut players = snapshot
            .entities
            .iter()
            .filter(|entity| entity.kind == protocol::EntityKind::Player);
        if players.next().map(|player| player.entity_id) != Some(expected)
            || players.next().is_some()
        {
            return Err(CorePrivateLifeClientError::InvalidSnapshotAuthority);
        }
        self.latest = Some(snapshot);
        Ok(())
    }
}

fn private_player_entity_id(actor_generation: u64) -> Result<u64, CorePrivateLifeClientError> {
    actor_generation
        .checked_sub(1)
        .and_then(|zero_based| zero_based.checked_mul(RUN_ENTITY_ID_STRIDE))
        .and_then(|base| base.checked_add(PLAYER_ENTITY_ID_OFFSET))
        .filter(|entity_id| *entity_id != 0)
        .ok_or(CorePrivateLifeClientError::InvalidSnapshotAuthority)
}

#[derive(Debug, Resource)]
pub struct CorePrivateLifeClient {
    phase: CorePrivateLifePhase,
    account: Option<AccountSnapshot>,
    server_hello: Option<ServerHello>,
    world_revision: WorldFlowContentRevisionV1,
    route_revision: CorePrivateRouteContentRevisionV1,
    route: Option<CorePrivateRouteClientModel>,
    location: Option<CharacterLocationSnapshot>,
    pending_location_character: Option<[u8; protocol::CHARACTER_ID_BYTES]>,
    pending_transfer: Option<[u8; protocol::MUTATION_ID_BYTES]>,
    last_transfer_code: Option<WorldTransferResultCode>,
    recall_result: Option<protocol::RecallResultV1>,
    error: Option<CorePrivateLifeClientFailure>,
    next_request_sequence: u32,
    next_mutation: u128,
}

impl CorePrivateLifeClient {
    fn new(
        world_revision: WorldFlowContentRevisionV1,
        route_revision: CorePrivateRouteContentRevisionV1,
    ) -> Self {
        Self {
            phase: CorePrivateLifePhase::Connecting,
            account: None,
            server_hello: None,
            world_revision,
            route_revision,
            route: None,
            location: None,
            pending_location_character: None,
            pending_transfer: None,
            last_transfer_code: None,
            recall_result: None,
            error: None,
            next_request_sequence: 1,
            next_mutation: 1,
        }
    }

    #[must_use]
    pub const fn phase(&self) -> CorePrivateLifePhase {
        self.phase
    }

    #[must_use]
    pub const fn account(&self) -> Option<&AccountSnapshot> {
        self.account.as_ref()
    }

    #[must_use]
    pub const fn route(&self) -> Option<&CorePrivateRouteClientModel> {
        self.route.as_ref()
    }

    #[must_use]
    pub fn selected_character_id(&self) -> Option<[u8; protocol::CHARACTER_ID_BYTES]> {
        self.account
            .as_ref()
            .and_then(|account| account.selected_character_id)
    }

    fn accept_server_hello(
        &mut self,
        hello: &ServerHello,
    ) -> Result<(), CorePrivateLifeClientError> {
        if hello.validate().is_err() {
            return self.fail(CorePrivateLifeClientFailure::InvalidServerAuthority);
        }
        self.server_hello = Some(hello.clone());
        self.error = None;
        if !hello
            .feature_flags
            .iter()
            .any(|flag| flag.as_str() == CORE_WORLD_FLOW_FEATURE_FLAG)
        {
            self.route = None;
            self.phase = CorePrivateLifePhase::Disabled;
            return Ok(());
        }
        self.phase = CorePrivateLifePhase::LoadingAuthority;
        self.rebind_selected_route()?;
        Ok(())
    }

    fn apply_bootstrap(
        &mut self,
        result: AccountBootstrapResult,
    ) -> Result<(), CorePrivateLifeClientError> {
        match result {
            AccountBootstrapResult::Snapshot(snapshot) => self.set_account(snapshot),
            AccountBootstrapResult::Error(_) => self.fail(CorePrivateLifeClientFailure::Identity),
        }
    }

    fn apply_character_mutation(
        &mut self,
        result: CharacterMutationResult,
    ) -> Result<(), CorePrivateLifeClientError> {
        if !result.accepted {
            if let Some(snapshot) = result.snapshot {
                self.set_account(snapshot)?;
            }
            return self.fail(CorePrivateLifeClientFailure::Identity);
        }
        let snapshot = result
            .snapshot
            .ok_or(CorePrivateLifeClientError::InvalidAccountAuthority)?;
        self.set_account(snapshot)
    }

    fn set_account(&mut self, snapshot: AccountSnapshot) -> Result<(), CorePrivateLifeClientError> {
        if snapshot.validate().is_err() {
            return self.fail(CorePrivateLifeClientFailure::Identity);
        }
        let previous = self.selected_character_id();
        self.account = Some(snapshot);
        if previous != self.selected_character_id() {
            self.location = None;
            self.pending_location_character = None;
            self.pending_transfer = None;
            self.rebind_selected_route()?;
        }
        if self.phase != CorePrivateLifePhase::Disabled {
            self.phase = CorePrivateLifePhase::CharacterSelect;
        }
        Ok(())
    }

    fn rebind_selected_route(&mut self) -> Result<(), CorePrivateLifeClientError> {
        let Some(hello) = self.server_hello.as_ref() else {
            return Ok(());
        };
        if !hello
            .feature_flags
            .iter()
            .any(|flag| flag.as_str() == CORE_WORLD_FLOW_FEATURE_FLAG)
        {
            self.route = None;
            return Ok(());
        }
        let Some(character_id) = self.selected_character_id() else {
            self.route = None;
            return Ok(());
        };
        if let Some(route) = self.route.as_mut()
            && route.character_id() == character_id
        {
            route.accept_server_hello(hello)?;
            return Ok(());
        }
        let mut route = CorePrivateRouteClientModel::new(
            character_id,
            self.world_revision.clone(),
            self.route_revision.clone(),
        )?;
        route.accept_server_hello(hello)?;
        self.route = Some(route);
        Ok(())
    }

    fn begin_location_query(
        &mut self,
    ) -> Result<Option<WorldFlowFrame>, CorePrivateLifeClientError> {
        if self.phase == CorePrivateLifePhase::Disabled || self.pending_location_character.is_some()
        {
            return Ok(None);
        }
        let Some(character_id) = self.selected_character_id() else {
            return Ok(None);
        };
        if self
            .location
            .as_ref()
            .is_some_and(|snapshot| snapshot.character_id == character_id)
        {
            return Ok(None);
        }
        let sequence = self.take_request_sequence()?;
        self.pending_location_character = Some(character_id);
        Ok(Some(WorldFlowFrame {
            sequence,
            request: WorldFlowRequest::Location {
                character_id,
                content_revision: self.world_revision.clone(),
            },
        }))
    }

    fn begin_transfer(
        &mut self,
        command: WorldTransferCommand,
        issued_at_unix_millis: u64,
    ) -> Result<WorldFlowFrame, CorePrivateLifeClientError> {
        if self.phase == CorePrivateLifePhase::Disabled || self.pending_transfer.is_some() {
            return Err(CorePrivateLifeClientError::ActionUnavailable);
        }
        let snapshot = self
            .location
            .clone()
            .ok_or(CorePrivateLifeClientError::ActionUnavailable)?;
        let allowed = match (&snapshot.location, &command) {
            (
                CharacterLocation::CharacterSelect { .. },
                WorldTransferCommand::EnterHallFromCharacterSelect,
            ) => true,
            (
                CharacterLocation::Safe { location_id, .. },
                WorldTransferCommand::UsePortal { portal_id },
            ) => {
                location_id.as_str() == CorePrivateRouteSceneV1::LanternHalls.location_id()
                    && portal_id.as_str() == REALM_GATE_ID
                    && self
                        .route
                        .as_ref()
                        .is_some_and(CorePrivateRouteClientModel::can_accept_gameplay_input)
            }
            _ => false,
        };
        if !allowed || issued_at_unix_millis == 0 {
            return Err(CorePrivateLifeClientError::ActionUnavailable);
        }
        let mutation_id = self.take_mutation_id()?;
        let payload = WorldTransferPayload {
            content_revision: self.world_revision.clone(),
            command,
        };
        let mutation = WorldTransferMutation {
            mutation_id,
            character_id: snapshot.character_id,
            expected_character_version: snapshot.character_version,
            issued_at_unix_millis,
            payload_hash: payload.canonical_hash(),
            payload,
        };
        let sequence = self.take_request_sequence()?;
        self.pending_transfer = Some(mutation_id);
        self.last_transfer_code = None;
        if let Some(route) = self.route.as_mut() {
            route.begin_committed_transfer_refresh()?;
        }
        self.phase = match mutation.payload.command {
            WorldTransferCommand::EnterHallFromCharacterSelect => {
                CorePrivateLifePhase::EnteringHall
            }
            _ => CorePrivateLifePhase::LoadingAuthority,
        };
        Ok(WorldFlowFrame {
            sequence,
            request: WorldFlowRequest::Transfer(mutation),
        })
    }

    fn apply_world_flow(
        &mut self,
        result: WorldFlowResult,
    ) -> Result<(), CorePrivateLifeClientError> {
        result
            .validate()
            .map_err(|_| CorePrivateLifeClientError::InvalidWorldAuthority)?;
        match result {
            WorldFlowResult::Location { snapshot, .. } => {
                if self.pending_location_character != Some(snapshot.character_id) {
                    return self.fail(CorePrivateLifeClientFailure::InvalidServerAuthority);
                }
                self.pending_location_character = None;
                self.accept_location(snapshot)
            }
            WorldFlowResult::Transfer {
                mutation_id,
                accepted,
                code,
                snapshot,
                ..
            } => {
                if self.pending_transfer != Some(mutation_id) {
                    return self.fail(CorePrivateLifeClientFailure::InvalidServerAuthority);
                }
                self.pending_transfer = None;
                self.last_transfer_code = Some(code);
                let Some(snapshot) = snapshot else {
                    if accepted {
                        return self.fail(CorePrivateLifeClientFailure::InvalidServerAuthority);
                    }
                    self.phase = CorePrivateLifePhase::Error;
                    return Ok(());
                };
                self.accept_location(snapshot)?;
                if !accepted {
                    self.phase = CorePrivateLifePhase::Error;
                }
                Ok(())
            }
            WorldFlowResult::Error { code, snapshot, .. } => {
                self.pending_transfer = None;
                self.last_transfer_code = Some(code);
                if let Some(snapshot) = snapshot {
                    self.accept_location(snapshot)?;
                }
                self.phase = CorePrivateLifePhase::Error;
                Ok(())
            }
        }
    }

    fn accept_location(
        &mut self,
        snapshot: CharacterLocationSnapshot,
    ) -> Result<(), CorePrivateLifeClientError> {
        if snapshot.validate().is_err()
            || Some(snapshot.character_id) != self.selected_character_id()
        {
            return self.fail(CorePrivateLifeClientFailure::InvalidServerAuthority);
        }
        self.location = Some(snapshot.clone());
        match snapshot.location {
            CharacterLocation::CharacterSelect { .. } => {
                self.phase = CorePrivateLifePhase::CharacterSelect;
                Ok(())
            }
            CharacterLocation::Safe { .. } | CharacterLocation::Danger { .. } => {
                let route = self
                    .route
                    .as_mut()
                    .ok_or(CorePrivateLifeClientError::FeatureNotNegotiated)?;
                route.apply_location(snapshot)?;
                self.apply_compiled_readiness()?;
                self.sync_phase();
                Ok(())
            }
        }
    }

    fn apply_route(
        &mut self,
        frame: &ReliableEventFrame,
    ) -> Result<(), CorePrivateLifeClientError> {
        let route = self
            .route
            .as_mut()
            .ok_or(CorePrivateLifeClientError::FeatureNotNegotiated)?;
        route.apply_reliable(frame)?;
        self.apply_compiled_readiness()?;
        self.sync_phase();
        Ok(())
    }

    fn apply_recall(
        &mut self,
        result: protocol::RecallResultV1,
    ) -> Result<(), CorePrivateLifeClientError> {
        result
            .validate()
            .map_err(|_| CorePrivateLifeClientError::InvalidSnapshotAuthority)?;
        match &result {
            protocol::RecallResultV1::Pending { character_id, .. }
            | protocol::RecallResultV1::Cancelled { character_id, .. }
            | protocol::RecallResultV1::Rejected { character_id, .. }
                if Some(*character_id) != self.selected_character_id() =>
            {
                return Err(CorePrivateLifeClientError::InvalidSnapshotAuthority);
            }
            protocol::RecallResultV1::Stored { result, .. }
                if Some(result.character_id) != self.selected_character_id() =>
            {
                return Err(CorePrivateLifeClientError::InvalidSnapshotAuthority);
            }
            _ => {}
        }
        if matches!(result, protocol::RecallResultV1::Stored { .. }) {
            self.location = None;
            self.pending_location_character = None;
            if let Some(route) = self.route.as_mut() {
                route.begin_committed_transfer_refresh()?;
            }
            self.phase = CorePrivateLifePhase::LoadingAuthority;
        }
        self.recall_result = Some(result);
        Ok(())
    }

    fn apply_compiled_readiness(&mut self) -> Result<(), CorePrivateLifeClientError> {
        let Some(route) = self.route.as_mut() else {
            return Ok(());
        };
        let Some(state) = route.route_state().cloned() else {
            return Ok(());
        };
        let Some(location) = self.location.as_ref() else {
            return Ok(());
        };
        if location.character_version != state.character_version {
            return Ok(());
        }
        Ok(route.apply_scene_readiness(CorePrivateSceneReadiness {
            base: CoreSceneReadiness {
                location_id: WireText::new(state.scene.location_id())
                    .map_err(|_| CorePrivateLifeClientError::InvalidWorldAuthority)?,
                character_version: state.character_version,
                content_revision: self.world_revision.clone(),
            },
            scene: state.scene,
            room: state.room,
            instance_lineage_id: state.instance_lineage_id,
            actor_generation: state.actor_generation,
            route_state_version: state.state_version,
        })?)
    }

    fn transport_lost(&mut self) {
        if let Some(route) = self.route.as_mut() {
            route.transport_lost();
        }
        self.location = None;
        self.pending_location_character = None;
        self.pending_transfer = None;
        self.recall_result = None;
        self.phase = CorePrivateLifePhase::Disconnected;
    }

    fn sync_phase(&mut self) {
        let Some(route) = self.route.as_ref() else {
            return;
        };
        self.phase = match route.phase() {
            CorePrivateRouteClientPhase::Disabled => CorePrivateLifePhase::Disabled,
            CorePrivateRouteClientPhase::AwaitingAuthority
            | CorePrivateRouteClientPhase::LoadingScene => CorePrivateLifePhase::LoadingAuthority,
            CorePrivateRouteClientPhase::TerminalPending => CorePrivateLifePhase::TerminalPending,
            CorePrivateRouteClientPhase::FatalAuthorityError => CorePrivateLifePhase::Error,
            CorePrivateRouteClientPhase::Controllable => {
                match route.route_state().map(|state| state.scene) {
                    Some(CorePrivateRouteSceneV1::LanternHalls) => CorePrivateLifePhase::Hall,
                    Some(
                        CorePrivateRouteSceneV1::CoreMicrorealm
                        | CorePrivateRouteSceneV1::BellSepulcher,
                    ) => CorePrivateLifePhase::PrivateRoute,
                    None => CorePrivateLifePhase::LoadingAuthority,
                }
            }
        };
    }

    fn take_request_sequence(&mut self) -> Result<u32, CorePrivateLifeClientError> {
        let sequence = self.next_request_sequence;
        self.next_request_sequence = sequence
            .checked_add(1)
            .ok_or(CorePrivateLifeClientError::SequenceExhausted)?;
        Ok(sequence)
    }

    fn take_mutation_id(&mut self) -> Result<[u8; 16], CorePrivateLifeClientError> {
        let mutation = self.next_mutation;
        self.next_mutation = mutation
            .checked_add(1)
            .ok_or(CorePrivateLifeClientError::SequenceExhausted)?;
        Ok(mutation.to_le_bytes())
    }

    fn fail<T>(
        &mut self,
        failure: CorePrivateLifeClientFailure,
    ) -> Result<T, CorePrivateLifeClientError> {
        self.error = Some(failure);
        self.phase = CorePrivateLifePhase::Error;
        Err(CorePrivateLifeClientError::InvalidAccountAuthority)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePrivateLifeClientFailure {
    Identity,
    InvalidServerAuthority,
    Transport,
}

#[derive(Debug, Error)]
pub enum CorePrivateLifeClientError {
    #[error("normal Core route was not negotiated")]
    FeatureNotNegotiated,
    #[error("normal Core route action is not currently authoritative")]
    ActionUnavailable,
    #[error("normal Core account authority is malformed or contradictory")]
    InvalidAccountAuthority,
    #[error("normal Core world authority is malformed or contradictory")]
    InvalidWorldAuthority,
    #[error("normal Core client sequence exhausted")]
    SequenceExhausted,
    #[error("normal Core gameplay snapshot authority is malformed or contradictory")]
    InvalidSnapshotAuthority,
    #[error(transparent)]
    Route(#[from] CorePrivateRouteClientError),
}

#[derive(Debug, Resource)]
struct InputSequencer {
    input_sequence: u32,
    primary_sequence: u32,
    primary_held: bool,
}

impl Default for InputSequencer {
    fn default() -> Self {
        Self {
            input_sequence: 1,
            primary_sequence: 0,
            primary_held: false,
        }
    }
}

#[derive(Component)]
struct StatusText;
#[derive(Component)]
struct RosterText;
#[derive(Component)]
struct RouteText;
#[derive(Component)]
struct ActionButton(PrivateLifeAction);
#[derive(Component)]
struct PrivateGameplayCamera;
#[derive(Component)]
struct PrivateGameplayEntity {
    entity_id: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
struct PrivateGameplayFloor {
    actor_generation: u64,
    scene: CorePrivateRouteSceneV1,
    room: Option<protocol::CorePrivateRouteRoomV1>,
}

/// Opens the real negotiated private-life route without enabling any local gameplay authority.
pub fn run_core_private_life(config: CorePrivateLifeConfig) -> Result<()> {
    if config.test_token.trim().is_empty() {
        bail!("--identity must contain a nonempty wipeable test token");
    }
    let certificate_der = std::fs::read(&config.certificate_path).with_context(|| {
        format!(
            "failed to read Core private-life server certificate {}",
            config.certificate_path.display()
        )
    })?;
    let content = load_core_private_life_content(&config.content_root)
        .context("normal Core private-life content failed validation")?;
    let (_, source_report) =
        load_and_validate(&config.content_root).context("Core source package failed validation")?;
    let manifest_hash = ManifestHash::new(source_report.package_hash_blake3)?;
    let world_revision = WorldFlowContentRevisionV1 {
        records_blake3: ManifestHash::new(content.world_flow().hashes().records_blake3.clone())?,
        assets_blake3: ManifestHash::new(content.world_flow().hashes().assets_blake3.clone())?,
        localization_blake3: ManifestHash::new(
            content.world_flow().hashes().localization_blake3.clone(),
        )?,
    };
    let route_revision = CorePrivateRouteContentRevisionV1 {
        records_blake3: ManifestHash::new(content.revision().records_blake3.clone())?,
        assets_blake3: ManifestHash::new(content.revision().assets_blake3.clone())?,
        localization_blake3: ManifestHash::new(content.revision().localization_blake3.clone())?,
    };
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
    let (width, height) = crate::configured_window_size()?;
    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb_u8(5, 8, 11)))
        .insert_resource(AccessibilitySettings::default())
        .insert_resource(CorePrivateLifeBridge(worker))
        .insert_resource(CorePrivatePresentationContent(content))
        .insert_resource(CorePrivateLifeClient::new(world_revision, route_revision))
        .insert_resource(CorePrivateSnapshotClient::default())
        .insert_resource(InputSequencer::default())
        .insert_resource(Time::<Fixed>::from_hz(f64::from(
            sim_core::TICKS_PER_SECOND,
        )))
        .add_plugins(
            crate::gravebound_default_plugins()
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: WINDOW_TITLE.to_owned(),
                        resolution: WindowResolution::new(width, height),
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_systems(Startup, spawn_ui)
        .add_systems(
            Update,
            (
                poll_transport,
                request_location,
                handle_keyboard,
                handle_recall_keyboard,
                handle_interact_keyboard,
                handle_buttons,
                present_private_gameplay,
                update_ui,
            )
                .chain(),
        )
        .add_systems(FixedUpdate, send_gameplay_input)
        .add_systems(Last, shutdown_transport);
    app.run();
    Ok(())
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn poll_transport(
    bridge: Res<CorePrivateLifeBridge>,
    mut client: ResMut<CorePrivateLifeClient>,
    mut snapshots: ResMut<CorePrivateSnapshotClient>,
) {
    let mut discard_snapshot_queue = false;
    for event in bridge.0.drain_events() {
        let result = match event {
            TransportEvent::Connecting => {
                client.phase = CorePrivateLifePhase::Connecting;
                Ok(())
            }
            TransportEvent::HandshakeAccepted(hello) => {
                snapshots.reset_transport();
                client.accept_server_hello(&hello)
            }
            TransportEvent::Reliable(frame) => match &frame.event {
                ReliableEvent::AccountBootstrapResult(result) => {
                    client.apply_bootstrap(result.clone())
                }
                ReliableEvent::CharacterMutationResult(result) => {
                    client.apply_character_mutation(result.clone())
                }
                ReliableEvent::WorldFlowResult(result) => client.apply_world_flow(result.clone()),
                ReliableEvent::CorePrivateRouteState(_) => client.apply_route(&frame),
                ReliableEvent::RecallResult(result) => client.apply_recall((**result).clone()),
                _ => Ok(()),
            },
            TransportEvent::LinkLost
            | TransportEvent::Reconnecting { .. }
            | TransportEvent::TransportClosed => {
                snapshots.reset_transport();
                discard_snapshot_queue = true;
                client.transport_lost();
                Ok(())
            }
            TransportEvent::Fatal(_) => {
                snapshots.reset_transport();
                discard_snapshot_queue = true;
                client.error = Some(CorePrivateLifeClientFailure::Transport);
                client.phase = CorePrivateLifePhase::Error;
                Ok(())
            }
        };
        if result.is_err() {
            client.phase = CorePrivateLifePhase::Error;
        }
    }
    let queued = bridge.0.drain_snapshots();
    if discard_snapshot_queue {
        return;
    }
    let route = client
        .route
        .as_ref()
        .and_then(CorePrivateRouteClientModel::route_state);
    let result = snapshots.bind_route(route).and_then(|()| {
        queued
            .into_iter()
            .try_for_each(|chunk| snapshots.ingest(chunk))
    });
    if result.is_err() {
        client.error = Some(CorePrivateLifeClientFailure::InvalidServerAuthority);
        client.phase = CorePrivateLifePhase::Error;
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn request_location(bridge: Res<CorePrivateLifeBridge>, mut client: ResMut<CorePrivateLifeClient>) {
    let Ok(Some(frame)) = client.begin_location_query() else {
        return;
    };
    if bridge
        .0
        .queue_reliable(WireMessage::WorldFlowFrame(frame))
        .is_err()
    {
        client.pending_location_character = None;
        client.phase = CorePrivateLifePhase::Error;
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn handle_keyboard(
    keyboard: Res<ButtonInput<KeyCode>>,
    bridge: Res<CorePrivateLifeBridge>,
    mut client: ResMut<CorePrivateLifeClient>,
) {
    let action = if keyboard.just_pressed(KeyCode::Digit1) {
        Some(PrivateLifeAction::Select(1))
    } else if keyboard.just_pressed(KeyCode::Digit2) {
        Some(PrivateLifeAction::Select(2))
    } else if keyboard.just_pressed(KeyCode::KeyN) {
        Some(PrivateLifeAction::Create)
    } else if keyboard.just_pressed(KeyCode::Enter) {
        Some(PrivateLifeAction::Play)
    } else if keyboard.just_pressed(KeyCode::KeyG) {
        Some(PrivateLifeAction::RealmGate)
    } else if keyboard.just_pressed(KeyCode::KeyR) {
        Some(PrivateLifeAction::Retry)
    } else {
        None
    };
    if let Some(action) = action {
        submit_action(action, &bridge, &mut client);
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn handle_recall_keyboard(
    keyboard: Res<ButtonInput<KeyCode>>,
    bridge: Res<CorePrivateLifeBridge>,
    snapshots: Res<CorePrivateSnapshotClient>,
    mut client: ResMut<CorePrivateLifeClient>,
) {
    let pending = matches!(
        client.recall_result,
        Some(protocol::RecallResultV1::Pending { .. })
    );
    let intent = if keyboard.just_pressed(KeyCode::KeyR) && !pending {
        Some(protocol::RecallIntentV1::Start)
    } else if keyboard.just_released(KeyCode::KeyR) && pending {
        Some(protocol::RecallIntentV1::Cancel)
    } else {
        None
    };
    let Some(intent) = intent else {
        return;
    };
    if client.phase != CorePrivateLifePhase::PrivateRoute
        || !client.server_hello.as_ref().is_some_and(|hello| {
            protocol::TerminalInventoryCapabilityV1::EmergencyRecall.is_advertised_by(hello)
        })
    {
        return;
    }
    let Some(character_id) = client.selected_character_id() else {
        return;
    };
    let Some(client_tick) = snapshots
        .latest
        .as_ref()
        .map(|snapshot| snapshot.server_tick)
    else {
        return;
    };
    let Ok(sequence) = client.take_request_sequence() else {
        client.phase = CorePrivateLifePhase::Error;
        return;
    };
    let frame = protocol::RecallFrameV1 {
        schema_version: protocol::TERMINAL_INVENTORY_SCHEMA_VERSION,
        sequence,
        character_id,
        client_tick,
        intent,
    };
    if frame.validate().is_err()
        || bridge
            .0
            .queue_reliable(WireMessage::RecallFrame(frame))
            .is_err()
    {
        client.phase = CorePrivateLifePhase::Error;
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn handle_interact_keyboard(
    keyboard: Res<ButtonInput<KeyCode>>,
    bridge: Res<CorePrivateLifeBridge>,
    snapshots: Res<CorePrivateSnapshotClient>,
    mut client: ResMut<CorePrivateLifeClient>,
) {
    if !keyboard.just_pressed(KeyCode::KeyE)
        || client.phase != CorePrivateLifePhase::PrivateRoute
        || !client
            .route
            .as_ref()
            .and_then(CorePrivateRouteClientModel::route_state)
            .is_some_and(|route| {
                route.scene == CorePrivateRouteSceneV1::BellSepulcher
                    && route.readiness.room_exit_available.is_available()
                    && !route.readiness.extraction_available.is_available()
            })
    {
        return;
    }
    let Some(client_tick) = snapshots
        .latest
        .as_ref()
        .map(|snapshot| snapshot.server_tick)
    else {
        return;
    };
    let Ok(sequence) = client.take_request_sequence() else {
        client.phase = CorePrivateLifePhase::Error;
        return;
    };
    let frame = protocol::ActionFrame {
        sequence,
        client_tick,
        action: protocol::ActionKind::Interact,
    };
    if bridge
        .0
        .queue_reliable(WireMessage::ActionFrame(frame))
        .is_err()
    {
        client.phase = CorePrivateLifePhase::Error;
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn handle_buttons(
    interactions: Query<(&Interaction, &ActionButton), Changed<Interaction>>,
    bridge: Res<CorePrivateLifeBridge>,
    mut client: ResMut<CorePrivateLifeClient>,
) {
    for (interaction, action) in &interactions {
        if *interaction == Interaction::Pressed {
            submit_action(action.0, &bridge, &mut client);
        }
    }
}

fn submit_action(
    action: PrivateLifeAction,
    bridge: &CorePrivateLifeBridge,
    client: &mut CorePrivateLifeClient,
) {
    if !action_available(action, client) {
        return;
    }
    match action {
        PrivateLifeAction::Create => {
            let Some(account) = client.account.as_ref() else {
                return;
            };
            if account.characters.len() >= usize::from(account.slot_capacity) {
                return;
            }
            let payload = CharacterMutationPayload::Create {
                class_id: WireText::new(protocol::GRAVE_ARBALIST_CLASS_ID)
                    .expect("canonical class ID fits"),
            };
            queue_identity_mutation(payload, bridge, client);
        }
        PrivateLifeAction::Select(ordinal) => {
            let character_id = client.account.as_ref().and_then(|account| {
                account
                    .characters
                    .iter()
                    .find(|character| character.roster_ordinal == ordinal)
                    .map(|character| character.character_id)
            });
            if let Some(character_id) = character_id {
                queue_identity_mutation(
                    CharacterMutationPayload::Select { character_id },
                    bridge,
                    client,
                );
            }
        }
        PrivateLifeAction::Play => queue_transfer(
            WorldTransferCommand::EnterHallFromCharacterSelect,
            bridge,
            client,
        ),
        PrivateLifeAction::RealmGate => queue_transfer(
            WorldTransferCommand::UsePortal {
                portal_id: WireText::new(REALM_GATE_ID).expect("canonical gate ID fits"),
            },
            bridge,
            client,
        ),
        PrivateLifeAction::Retry => {
            client.location = None;
            client.pending_location_character = None;
            client.pending_transfer = None;
            client.error = None;
            client.phase = CorePrivateLifePhase::LoadingAuthority;
        }
    }
}

fn queue_identity_mutation(
    payload: CharacterMutationPayload,
    bridge: &CorePrivateLifeBridge,
    client: &mut CorePrivateLifeClient,
) {
    let Some(expected_account_version) = client
        .account
        .as_ref()
        .map(|account| account.account_version)
    else {
        return;
    };
    let Ok(mutation_id) = client.take_mutation_id() else {
        client.phase = CorePrivateLifePhase::Error;
        return;
    };
    let Some(issued_at_unix_millis) = unix_millis() else {
        client.phase = CorePrivateLifePhase::Error;
        return;
    };
    let frame = CharacterMutationFrame {
        mutation_id,
        expected_account_version,
        payload_hash: payload.canonical_hash(),
        issued_at_unix_millis,
        payload,
    };
    if bridge
        .0
        .queue_reliable(WireMessage::CharacterMutationFrame(frame))
        .is_ok()
    {
        client.phase = CorePrivateLifePhase::Selecting;
    } else {
        client.phase = CorePrivateLifePhase::Error;
    }
}

fn queue_transfer(
    command: WorldTransferCommand,
    bridge: &CorePrivateLifeBridge,
    client: &mut CorePrivateLifeClient,
) {
    let Some(issued_at_unix_millis) = unix_millis() else {
        client.phase = CorePrivateLifePhase::Error;
        return;
    };
    let Ok(frame) = client.begin_transfer(command, issued_at_unix_millis) else {
        return;
    };
    if bridge
        .0
        .queue_reliable(WireMessage::WorldFlowFrame(frame))
        .is_err()
    {
        client.pending_transfer = None;
        client.phase = CorePrivateLifePhase::Error;
    }
}

fn unix_millis() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn send_gameplay_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    bridge: Res<CorePrivateLifeBridge>,
    client: Res<CorePrivateLifeClient>,
    mut sequencer: ResMut<InputSequencer>,
) {
    if !client
        .route
        .as_ref()
        .is_some_and(CorePrivateRouteClientModel::can_accept_gameplay_input)
    {
        return;
    }
    let horizontal =
        i8::from(keyboard.pressed(KeyCode::KeyD)) - i8::from(keyboard.pressed(KeyCode::KeyA));
    let vertical =
        i8::from(keyboard.pressed(KeyCode::KeyS)) - i8::from(keyboard.pressed(KeyCode::KeyW));
    let (horizontal_milli, vertical_milli) = normalized_input(horizontal, vertical);
    let held_primary = mouse.pressed(MouseButton::Left);
    if held_primary && !sequencer.primary_held {
        let Some(next) = sequencer.primary_sequence.checked_add(1) else {
            return;
        };
        sequencer.primary_sequence = next;
    }
    sequencer.primary_held = held_primary;
    let sequence = sequencer.input_sequence;
    bridge.0.replace_input(protocol::InputFrame {
        sequence,
        client_tick: u64::from(sequence),
        movement_x_milli: horizontal_milli,
        movement_y_milli: vertical_milli,
        aim_x_milli: 0,
        aim_y_milli: -1_000,
        held_primary,
        primary_sequence: sequencer.primary_sequence,
        ability_1_sequence: 0,
        ability_2_sequence: 0,
    });
    if let Some(next) = sequence.checked_add(1) {
        sequencer.input_sequence = next;
    }
}

fn normalized_input(x: i8, y: i8) -> (i16, i16) {
    match (x, y) {
        (0, 0) => (0, 0),
        (0, y) => (0, i16::from(y) * 1_000),
        (x, 0) => (i16::from(x) * 1_000, 0),
        (x, y) => (i16::from(x) * 707, i16::from(y) * 707),
    }
}

#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    reason = "Bevy presentation owns disjoint entity, floor, and camera queries"
)]
fn present_private_gameplay(
    mut commands: Commands,
    client: Res<CorePrivateLifeClient>,
    snapshots: Res<CorePrivateSnapshotClient>,
    content: Res<CorePrivatePresentationContent>,
    mut camera: Single<&mut Transform, With<PrivateGameplayCamera>>,
    mut entities: Query<(Entity, &PrivateGameplayEntity, &mut Transform, &mut Sprite)>,
    floors: Query<(Entity, &PrivateGameplayFloor)>,
) {
    let Some(route) = client
        .route
        .as_ref()
        .and_then(CorePrivateRouteClientModel::route_state)
        .filter(|state| {
            matches!(
                state.scene,
                CorePrivateRouteSceneV1::CoreMicrorealm | CorePrivateRouteSceneV1::BellSepulcher
            )
        })
    else {
        despawn_private_gameplay(&mut commands, &entities, &floors);
        return;
    };
    let Some(snapshot) = snapshots.latest.as_ref() else {
        despawn_private_gameplay(&mut commands, &entities, &floors);
        return;
    };
    let Some((width, height)) = private_scene_dimensions(&content.0, route) else {
        despawn_private_gameplay(&mut commands, &entities, &floors);
        return;
    };
    let floor_binding = PrivateGameplayFloor {
        actor_generation: route.actor_generation,
        scene: route.scene,
        room: route.room,
    };
    let floor_matches = floors.iter().any(|(_, floor)| *floor == floor_binding);
    if !floor_matches {
        for (entity, _) in &floors {
            commands.entity(entity).despawn();
        }
        commands.spawn((
            Name::new("Authoritative private arena"),
            floor_binding,
            Sprite::from_color(Color::srgb_u8(12, 20, 24), Vec2::new(width, height)),
            Transform::from_xyz(0.0, 0.0, -1.0),
        ));
    }

    let desired = snapshot
        .entities
        .iter()
        .map(|entity| (entity.entity_id, entity))
        .collect::<BTreeMap<_, _>>();
    for (entity, visual, mut transform, mut sprite) in &mut entities {
        let Some(snapshot) = desired.get(&visual.entity_id) else {
            commands.entity(entity).despawn();
            continue;
        };
        let render = private_snapshot_position(snapshot, width, height);
        transform.translation.x = render.x;
        transform.translation.y = render.y;
        let (color, size, z) = private_entity_style(snapshot.kind);
        sprite.color = color;
        sprite.custom_size = Some(Vec2::splat(size));
        transform.translation.z = z;
    }
    let existing = entities
        .iter()
        .map(|(_, visual, _, _)| visual.entity_id)
        .collect::<std::collections::BTreeSet<_>>();
    for snapshot in snapshot
        .entities
        .iter()
        .filter(|snapshot| !existing.contains(&snapshot.entity_id))
    {
        let (color, size, z) = private_entity_style(snapshot.kind);
        let render = private_snapshot_position(snapshot, width, height);
        commands.spawn((
            Name::new(format!(
                "Private {:?} {}",
                snapshot.kind, snapshot.entity_id
            )),
            PrivateGameplayEntity {
                entity_id: snapshot.entity_id,
            },
            Sprite::from_color(color, Vec2::splat(size)),
            Transform::from_xyz(render.x, render.y, z),
        ));
    }
    if let Some(player) = snapshot
        .entities
        .iter()
        .find(|entity| entity.kind == protocol::EntityKind::Player)
    {
        let render = private_snapshot_position(player, width, height);
        camera.translation.x = render.x;
        camera.translation.y = render.y;
    }
}

fn despawn_private_gameplay(
    commands: &mut Commands,
    entities: &Query<(Entity, &PrivateGameplayEntity, &mut Transform, &mut Sprite)>,
    floors: &Query<(Entity, &PrivateGameplayFloor)>,
) {
    for (entity, _, _, _) in entities {
        commands.entity(entity).despawn();
    }
    for (entity, _) in floors {
        commands.entity(entity).despawn();
    }
}

#[allow(clippy::cast_precision_loss)]
fn private_scene_dimensions(
    content: &sim_content::CorePrivateLifeContent,
    route: &protocol::CorePrivateRouteStateV1,
) -> Option<(f32, f32)> {
    match route.scene {
        CorePrivateRouteSceneV1::CoreMicrorealm => Some((
            content.microrealm_scene().width_milli_tiles as f32 / 1_000.0,
            content.microrealm_scene().height_milli_tiles as f32 / 1_000.0,
        )),
        CorePrivateRouteSceneV1::BellSepulcher => {
            let node_id = route.room?.node_id();
            content
                .fixed_layout()
                .rooms
                .iter()
                .find(|room| room.node_id == node_id)
                .map(|room| {
                    (
                        room.room.width_milli_tiles as f32 / 1_000.0,
                        room.room.height_milli_tiles as f32 / 1_000.0,
                    )
                })
        }
        CorePrivateRouteSceneV1::LanternHalls => None,
    }
}

#[allow(clippy::cast_precision_loss)]
fn private_snapshot_position(snapshot: &protocol::EntitySnapshot, width: f32, height: f32) -> Vec2 {
    Vec2::new(
        snapshot.x_milli_tiles as f32 / 1_000.0 - width * 0.5,
        height * 0.5 - snapshot.y_milli_tiles as f32 / 1_000.0,
    )
}

fn private_entity_style(kind: protocol::EntityKind) -> (Color, f32, f32) {
    match kind {
        protocol::EntityKind::Player => (Color::srgb_u8(99, 225, 197), 0.64, 8.0),
        protocol::EntityKind::Enemy => (Color::srgb_u8(191, 69, 80), 0.72, 5.5),
        protocol::EntityKind::Boss => (Color::srgb_u8(226, 78, 70), 1.15, 5.5),
        protocol::EntityKind::FriendlyProjectile => (Color::srgb_u8(109, 234, 203), 0.20, 7.0),
        protocol::EntityKind::HostileProjectile => (Color::srgb_u8(246, 126, 77), 0.24, 7.0),
        protocol::EntityKind::PersonalPickup | protocol::EntityKind::Loot => {
            (Color::srgb_u8(240, 213, 139), 0.34, 4.5)
        }
        protocol::EntityKind::Objective => (Color::srgb_u8(191, 139, 241), 0.46, 4.0),
    }
}

fn spawn_ui(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: 13.5,
            },
            ..OrthographicProjection::default_2d()
        }),
        PrivateGameplayCamera,
    ));
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(18),
                top: px(18),
                width: px(420),
                max_height: percent(94),
                padding: UiRect::all(px(20)),
                flex_direction: FlexDirection::Column,
                row_gap: px(12),
                ..default()
            },
            BackgroundColor(Color::srgba_u8(5, 8, 11, 238)),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("GRAVEBOUND\nCORE PRIVATE LIFE"),
                TextFont::from_font_size(28.0),
                TextColor(Color::srgb_u8(235, 216, 166)),
            ));
            root.spawn((
                Text::new("Connecting"),
                TextFont::from_font_size(16.0),
                TextColor(Color::srgb_u8(140, 203, 195)),
                StatusText,
            ));
            root.spawn((
                Text::new("Character Select"),
                TextFont::from_font_size(18.0),
                TextColor(Color::srgb_u8(225, 225, 216)),
                Node {
                    min_height: px(150),
                    padding: UiRect::all(px(16)),
                    border: UiRect::all(px(2)),
                    ..default()
                },
                BorderColor::all(Color::srgb_u8(83, 113, 116)),
                BackgroundColor(Color::srgb_u8(15, 21, 27)),
                RosterText,
            ));
            root.spawn((
                Text::new("Awaiting authoritative route."),
                TextFont::from_font_size(17.0),
                TextColor(Color::srgb_u8(192, 200, 190)),
                Node {
                    min_height: px(160),
                    padding: UiRect::all(px(16)),
                    border: UiRect::all(px(2)),
                    ..default()
                },
                BorderColor::all(Color::srgb_u8(80, 87, 91)),
                BackgroundColor(Color::srgb_u8(12, 17, 22)),
                RouteText,
            ));
            root.spawn((Node {
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(10),
                row_gap: px(10),
                ..default()
            },))
                .with_children(|row| {
                    spawn_button(row, PrivateLifeAction::Create, "New Grave Arbalist [N]");
                    spawn_button(row, PrivateLifeAction::Select(1), "Select 1 [1]");
                    spawn_button(row, PrivateLifeAction::Select(2), "Select 2 [2]");
                    spawn_button(row, PrivateLifeAction::Play, "Play [Enter]");
                    spawn_button(row, PrivateLifeAction::RealmGate, "Realm Gate — Enter [G]");
                    spawn_button(row, PrivateLifeAction::Retry, "Retry [R]");
                });
            root.spawn((
                Text::new("MOVE  WASD    FIRE  LEFT MOUSE    RECALL  HOLD R    HALL GATE  G"),
                TextFont::from_font_size(13.0),
                TextColor(Color::srgb_u8(130, 144, 145)),
            ));
        });
}

fn spawn_button(parent: &mut ChildSpawnerCommands, action: PrivateLifeAction, label: &str) {
    parent
        .spawn((
            Button,
            Node {
                padding: UiRect::axes(px(15), px(10)),
                border: UiRect::all(px(2)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(23, 35, 38)),
            BorderColor::all(Color::srgb_u8(105, 151, 145)),
            ActionButton(action),
        ))
        .with_child((
            Text::new(label),
            TextFont::from_font_size(14.0),
            TextColor(Color::srgb_u8(222, 224, 211)),
        ));
}

#[allow(
    clippy::needless_pass_by_value,
    clippy::type_complexity,
    reason = "Bevy system parameters own disjoint query filters"
)]
fn update_ui(
    client: Res<CorePrivateLifeClient>,
    snapshots: Res<CorePrivateSnapshotClient>,
    mut status: Single<&mut Text, With<StatusText>>,
    mut roster: Single<&mut Text, (With<RosterText>, Without<StatusText>)>,
    mut route: Single<&mut Text, (With<RouteText>, Without<StatusText>, Without<RosterText>)>,
    mut actions: Query<(&ActionButton, &mut BackgroundColor, &mut BorderColor)>,
) {
    **status = Text::new(phase_label(client.phase));
    **roster = Text::new(render_roster(&client));
    **route = Text::new(render_route(&client, &snapshots));
    for (action, mut background, mut border) in &mut actions {
        let available = action_available(action.0, &client);
        *background = BackgroundColor(if available {
            Color::srgb_u8(23, 45, 43)
        } else {
            Color::srgb_u8(23, 27, 30)
        });
        *border = BorderColor::all(if available {
            Color::srgb_u8(119, 177, 163)
        } else {
            Color::srgb_u8(66, 74, 76)
        });
    }
}

fn action_available(action: PrivateLifeAction, client: &CorePrivateLifeClient) -> bool {
    match action {
        PrivateLifeAction::Create => client.account.as_ref().is_some_and(|account| {
            client.phase == CorePrivateLifePhase::CharacterSelect
                && account.characters.len() < usize::from(account.slot_capacity)
        }),
        PrivateLifeAction::Select(ordinal) => client.account.as_ref().is_some_and(|account| {
            client.phase == CorePrivateLifePhase::CharacterSelect
                && account
                    .characters
                    .iter()
                    .any(|character| character.roster_ordinal == ordinal)
        }),
        PrivateLifeAction::Play => {
            matches!(
                client.location.as_ref().map(|snapshot| &snapshot.location),
                Some(CharacterLocation::CharacterSelect { .. })
            ) && client.phase == CorePrivateLifePhase::CharacterSelect
        }
        PrivateLifeAction::RealmGate => {
            client.phase == CorePrivateLifePhase::Hall
                && client
                    .route
                    .as_ref()
                    .is_some_and(CorePrivateRouteClientModel::can_accept_gameplay_input)
        }
        PrivateLifeAction::Retry => matches!(
            client.phase,
            CorePrivateLifePhase::Disconnected | CorePrivateLifePhase::Error
        ),
    }
}

fn render_roster(client: &CorePrivateLifeClient) -> String {
    let Some(account) = client.account.as_ref() else {
        return "Character Select\nLoading roster.".to_owned();
    };
    let mut rows = vec!["Character Select".to_owned()];
    for ordinal in 1..=account.slot_capacity {
        let row = account
            .characters
            .iter()
            .find(|character| character.roster_ordinal == ordinal)
            .map_or_else(
                || format!("Slot {ordinal} — Empty"),
                |character| {
                    let selected = if account.selected_character_id == Some(character.character_id)
                    {
                        " — SELECTED"
                    } else {
                        ""
                    };
                    format!(
                        "Hero {ordinal} — Grave Arbalist — Level {}{selected}",
                        character.level
                    )
                },
            );
        rows.push(row);
    }
    rows.join("\n")
}

fn render_route(client: &CorePrivateLifeClient, snapshots: &CorePrivateSnapshotClient) -> String {
    if client.phase == CorePrivateLifePhase::Disabled {
        return "Available in a later test.\nNormal route capability was not negotiated."
            .to_owned();
    }
    let Some(state) = client
        .route
        .as_ref()
        .and_then(CorePrivateRouteClientModel::route_state)
    else {
        let location = match client.location.as_ref().map(|snapshot| &snapshot.location) {
            Some(CharacterLocation::CharacterSelect { .. }) => "Character Select",
            Some(
                CharacterLocation::Safe { location_id, .. }
                | CharacterLocation::Danger { location_id, .. },
            ) => location_id.as_str(),
            None => "Awaiting authoritative location",
        };
        return format!("{location}\nAwaiting authoritative route.");
    };
    let scene = match state.scene {
        CorePrivateRouteSceneV1::LanternHalls => "Lantern Halls",
        CorePrivateRouteSceneV1::CoreMicrorealm => "Core Micro-realm",
        CorePrivateRouteSceneV1::BellSepulcher => "Bell Sepulcher",
    };
    let room = state
        .room
        .map(|room| format!(" — {}", room.node_id()))
        .unwrap_or_default();
    let transfer = client
        .last_transfer_code
        .filter(|code| *code != WorldTransferResultCode::Accepted)
        .map(|code| format!("\nLast transfer: {code:?}"))
        .unwrap_or_default();
    let gameplay = snapshots.latest.as_ref().map_or_else(
        || "\nSnapshot: awaiting authoritative frame".to_owned(),
        |snapshot| {
            let enemies = snapshot
                .entities
                .iter()
                .filter(|entity| {
                    matches!(
                        entity.kind,
                        protocol::EntityKind::Enemy | protocol::EntityKind::Boss
                    ) && entity.state_flags & protocol::ENTITY_STATE_ALIVE != 0
                })
                .count();
            let health = snapshot
                .entities
                .iter()
                .find(|entity| entity.kind == protocol::EntityKind::Player)
                .map_or_else(
                    || "unavailable".to_owned(),
                    |player| format!("{} / {}", player.current_health, player.maximum_health),
                );
            format!(
                "\nSnapshot: tick {}    HP {health}    Hostiles {enemies}",
                snapshot.server_tick
            )
        },
    );
    let recall = match client.recall_result.as_ref() {
        Some(protocol::RecallResultV1::Pending {
            completion_tick, ..
        }) => format!("\nEmergency Recall: channeling to tick {completion_tick}"),
        Some(protocol::RecallResultV1::Cancelled { .. }) => {
            "\nEmergency Recall: cancelled".to_owned()
        }
        Some(protocol::RecallResultV1::Stored { .. }) => {
            "\nEmergency Recall: committed — returning to Hall".to_owned()
        }
        Some(protocol::RecallResultV1::Rejected { code, .. }) => {
            format!("\nEmergency Recall: {code:?}")
        }
        None => String::new(),
    };
    format!(
        "{scene}{room}\nPhase: {:?}\nActor generation: {}    State version: {}\nControl: {}{gameplay}{recall}{transfer}",
        state.phase,
        state.actor_generation,
        state.state_version,
        if client
            .route
            .as_ref()
            .is_some_and(CorePrivateRouteClientModel::can_accept_gameplay_input)
        {
            "READY"
        } else {
            "WITHHELD"
        }
    )
}

const fn phase_label(phase: CorePrivateLifePhase) -> &'static str {
    match phase {
        CorePrivateLifePhase::Connecting => "Connecting",
        CorePrivateLifePhase::CharacterSelect => "Character Select",
        CorePrivateLifePhase::Selecting => "Selecting character",
        CorePrivateLifePhase::EnteringHall => "Entering Lantern Halls",
        CorePrivateLifePhase::LoadingAuthority => "Loading authoritative route",
        CorePrivateLifePhase::Hall => "Lantern Halls — Ready",
        CorePrivateLifePhase::PrivateRoute => "Private route — Ready",
        CorePrivateLifePhase::TerminalPending => "Terminal result pending",
        CorePrivateLifePhase::Disconnected => "Disconnected",
        CorePrivateLifePhase::Disabled => "Available in a later test.",
        CorePrivateLifePhase::Error => "Service unavailable. Try again.",
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn shutdown_transport(bridge: Res<CorePrivateLifeBridge>, exits: MessageReader<AppExit>) {
    if !exits.is_empty() {
        bridge.0.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use protocol::{
        AccountNamespace, CORE_CHARACTER_SLOT_CAPACITY, CharacterLifeState, CharacterSecurityState,
        CharacterSnapshot, CorePrivateRoutePhaseV1, CorePrivateRouteReadinessV1,
        CorePrivateRouteStateV1, M03_CORE_DEV_BUILD_ID, SIMULATION_HZ, SNAPSHOT_HZ, SafeArrival,
    };

    use super::*;

    fn revision(value: char) -> ManifestHash {
        ManifestHash::new(value.to_string().repeat(64)).unwrap()
    }

    fn world_revision() -> WorldFlowContentRevisionV1 {
        WorldFlowContentRevisionV1 {
            records_blake3: revision('1'),
            assets_blake3: revision('2'),
            localization_blake3: revision('3'),
        }
    }

    fn route_revision() -> CorePrivateRouteContentRevisionV1 {
        CorePrivateRouteContentRevisionV1 {
            records_blake3: revision('4'),
            assets_blake3: revision('5'),
            localization_blake3: revision('6'),
        }
    }

    fn hello(enabled: bool) -> ServerHello {
        ServerHello {
            session_id: WireText::new("core-private-life").unwrap(),
            protocol_major: ProtocolVersion::current().major,
            protocol_minor: ProtocolVersion::current().minor,
            required_client_build: WireText::new(M03_CORE_DEV_BUILD_ID).unwrap(),
            content_bundle_version: WireText::new("core-test").unwrap(),
            server_tick_rate: SIMULATION_HZ,
            snapshot_rate: SNAPSHOT_HZ,
            region_id: WireText::new("local").unwrap(),
            feature_flags: enabled
                .then(|| WireText::new(CORE_WORLD_FLOW_FEATURE_FLAG).unwrap())
                .into_iter()
                .collect(),
        }
    }

    fn account() -> AccountSnapshot {
        AccountSnapshot {
            namespace: AccountNamespace::WipeableTest,
            account_version: 1,
            slot_capacity: CORE_CHARACTER_SLOT_CAPACITY,
            characters: vec![CharacterSnapshot {
                character_id: [7; 16],
                roster_ordinal: 1,
                class_id: WireText::new(protocol::GRAVE_ARBALIST_CLASS_ID).unwrap(),
                level: 1,
                oath_id: None,
                life_state: CharacterLifeState::Living,
                security_state: CharacterSecurityState::SafeCharacterSelect,
            }],
            selected_character_id: Some([7; 16]),
        }
    }

    fn character_select() -> CharacterLocationSnapshot {
        CharacterLocationSnapshot {
            character_id: [7; 16],
            character_version: 1,
            location: CharacterLocation::CharacterSelect {
                next_hall_arrival: SafeArrival::HallDefault,
            },
        }
    }

    #[test]
    fn missing_capability_keeps_play_fail_closed() {
        let mut client = CorePrivateLifeClient::new(world_revision(), route_revision());
        client.accept_server_hello(&hello(false)).unwrap();
        client.set_account(account()).unwrap();
        assert_eq!(client.phase(), CorePrivateLifePhase::Disabled);
        assert!(client.begin_location_query().unwrap().is_none());
        assert!(matches!(
            client.begin_transfer(WorldTransferCommand::EnterHallFromCharacterSelect, 1),
            Err(CorePrivateLifeClientError::ActionUnavailable)
        ));
    }

    #[test]
    fn selected_character_requires_location_before_play_and_route_before_hall_control() {
        let mut client = CorePrivateLifeClient::new(world_revision(), route_revision());
        client.accept_server_hello(&hello(true)).unwrap();
        client.set_account(account()).unwrap();
        assert!(
            client
                .begin_transfer(WorldTransferCommand::EnterHallFromCharacterSelect, 1)
                .is_err()
        );
        let query = client.begin_location_query().unwrap().unwrap();
        let WorldFlowRequest::Location { .. } = query.request else {
            panic!("expected location query");
        };
        client
            .apply_world_flow(WorldFlowResult::Location {
                request_sequence: query.sequence,
                snapshot: character_select(),
            })
            .unwrap();
        let transfer = client
            .begin_transfer(WorldTransferCommand::EnterHallFromCharacterSelect, 1)
            .unwrap();
        let WorldFlowRequest::Transfer(mutation) = transfer.request else {
            panic!("expected transfer");
        };
        let hall = CharacterLocationSnapshot {
            character_id: [7; 16],
            character_version: 2,
            location: CharacterLocation::Safe {
                location_id: WireText::new("hub.lantern_halls_01").unwrap(),
                arrival: SafeArrival::HallDefault,
            },
        };
        client
            .apply_world_flow(WorldFlowResult::Transfer {
                request_sequence: transfer.sequence,
                mutation_id: mutation.mutation_id,
                accepted: true,
                code: WorldTransferResultCode::Accepted,
                snapshot: Some(hall),
                transfer_id: Some([8; 16]),
            })
            .unwrap();
        assert_eq!(client.phase(), CorePrivateLifePhase::LoadingAuthority);

        let state = CorePrivateRouteStateV1 {
            schema_version: protocol::CORE_PRIVATE_ROUTE_SCHEMA_VERSION,
            character_id: [7; 16],
            character_version: 2,
            content_revision: route_revision(),
            actor_generation: 1,
            state_version: 1,
            instance_lineage_id: None,
            scene: CorePrivateRouteSceneV1::LanternHalls,
            room: None,
            phase: CorePrivateRoutePhaseV1::Hall,
            readiness: CorePrivateRouteReadinessV1::canonical(CorePrivateRoutePhaseV1::Hall),
        };
        client
            .apply_route(&ReliableEventFrame {
                sequence: 3,
                server_tick: 3,
                event: ReliableEvent::CorePrivateRouteState(Box::new(state)),
            })
            .unwrap();
        assert_eq!(client.phase(), CorePrivateLifePhase::Hall);
        assert!(client.route().unwrap().can_accept_gameplay_input());
    }

    #[test]
    fn normalized_diagonal_never_exceeds_protocol_vector_bound() {
        assert_eq!(normalized_input(1, 1), (707, 707));
        assert_eq!(normalized_input(-1, 0), (-1_000, 0));
        assert_eq!(normalized_input(0, -1), (0, -1_000));
    }

    fn danger_route(actor_generation: u64, state_version: u64) -> CorePrivateRouteStateV1 {
        CorePrivateRouteStateV1 {
            schema_version: protocol::CORE_PRIVATE_ROUTE_SCHEMA_VERSION,
            character_id: [7; 16],
            character_version: 2,
            content_revision: route_revision(),
            actor_generation,
            state_version,
            instance_lineage_id: Some([9; 16]),
            scene: CorePrivateRouteSceneV1::CoreMicrorealm,
            room: None,
            phase: CorePrivateRoutePhaseV1::MicrorealmActive,
            readiness: CorePrivateRouteReadinessV1::canonical(
                CorePrivateRoutePhaseV1::MicrorealmActive,
            ),
        }
    }

    fn snapshot(
        sequence: u32,
        state_version: u64,
        player_entity_id: u64,
    ) -> protocol::SnapshotChunk {
        protocol::SnapshotChunk {
            sequence,
            server_tick: u64::from(sequence),
            state_version,
            acknowledged_input_sequence: sequence,
            chunk_index: 0,
            chunk_count: 1,
            entities: vec![protocol::EntitySnapshot {
                entity_id: player_entity_id,
                kind: protocol::EntityKind::Player,
                x_milli_tiles: 24_000,
                y_milli_tiles: 24_000,
                velocity_x_milli_tiles_per_second: 0,
                velocity_y_milli_tiles_per_second: 0,
                source_entity_id: 0,
                source_input_sequence: 0,
                source_projectile_ordinal: 0,
                current_health: 100,
                maximum_health: 100,
                state_flags: protocol::ENTITY_STATE_ALIVE,
            }],
        }
    }

    #[test]
    fn snapshots_wait_for_matching_route_and_validate_generation_player_identity() {
        let mut client = CorePrivateSnapshotClient::default();
        client.bind_route(Some(&danger_route(1, 5))).unwrap();
        client.ingest(snapshot(1, 6, 10_000)).unwrap();
        assert!(client.latest.is_none());

        client.bind_route(Some(&danger_route(1, 6))).unwrap();
        assert_eq!(client.latest.as_ref().unwrap().state_version, 6);

        client.reset_transport();
        client.bind_route(Some(&danger_route(2, 7))).unwrap();
        assert!(matches!(
            client.ingest(snapshot(1, 7, 10_000)),
            Err(CorePrivateLifeClientError::InvalidSnapshotAuthority)
        ));
    }

    #[test]
    fn recall_result_is_selected_character_bound_before_presentation() {
        let mut client = CorePrivateLifeClient::new(world_revision(), route_revision());
        client.accept_server_hello(&hello(true)).unwrap();
        client.set_account(account()).unwrap();
        let pending = protocol::RecallResultV1::Pending {
            schema_version: protocol::TERMINAL_INVENTORY_SCHEMA_VERSION,
            request_sequence: 1,
            character_id: [7; 16],
            started_tick: 10,
            completion_tick: 10 + protocol::RECALL_CHANNEL_TICKS,
            pending_item_count: 0,
            pending_material_stack_count: 0,
        };
        client.apply_recall(pending.clone()).unwrap();
        assert_eq!(client.recall_result, Some(pending));

        assert!(
            client
                .apply_recall(protocol::RecallResultV1::Pending {
                    schema_version: protocol::TERMINAL_INVENTORY_SCHEMA_VERSION,
                    request_sequence: 2,
                    character_id: [8; 16],
                    started_tick: 11,
                    completion_tick: 11 + protocol::RECALL_CHANNEL_TICKS,
                    pending_item_count: 0,
                    pending_material_stack_count: 0,
                })
                .is_err()
        );
    }
}
