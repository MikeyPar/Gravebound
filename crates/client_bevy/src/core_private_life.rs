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
use bevy::{
    app::AppExit,
    camera::ScalingMode,
    prelude::*,
    window::{PrimaryWindow, WindowResolution},
};
use protocol::{
    AccountBootstrapFrame, AccountBootstrapRequest, AccountBootstrapResult, AccountSnapshot,
    AuthTicket, CORE_WORLD_FLOW_FEATURE_FLAG, CharacterLocation, CharacterLocationSnapshot,
    CharacterMutationFrame, CharacterMutationPayload, CharacterMutationResult, ClientHello,
    Compression, CorePrivateRouteContentRevisionV1, CorePrivateRouteSceneV1, M02_LOCAL_SERVER_NAME,
    M03_CORE_DEV_BUILD_ID, ManifestHash, Platform, ProtocolVersion, ReliableEvent,
    ReliableEventFrame, ServerHello, WireMessage, WireText, WorldFlowContentRevisionV1,
    WorldFlowFrame, WorldFlowRequest, WorldFlowResult, WorldTransferCommand, WorldTransferMutation,
    WorldTransferPayload, WorldTransferResultCode,
};
use sim_content::{
    CoreSuccessorRecoveryContent, load_and_validate, load_core_development_death_view,
    load_core_private_life_content, load_core_successor_recovery,
};
use thiserror::Error;

use crate::{
    CorePrivateRouteClientError, CorePrivateRouteClientModel, CorePrivateRouteClientPhase,
    CorePrivateSceneReadiness, CoreSceneReadiness, DeathSummaryAction, DeathUiAction,
    DeathUiCommand, DeathUiConfig, DeathUiSnapshot, DeathViewClientModel, MemorialDetailPhase,
    MemorialListPhase, NativeDeathView, NativeDeathViewPlugin, NativeSuccessorRecoveryPlugin,
    NativeSuccessorRecoveryView, SuccessorRecoveryClientModel, SuccessorRecoveryPhase,
    SuccessorRecoveryUiAction, SuccessorRecoveryUiCommand, SuccessorRecoveryUiConfig,
    SuccessorRecoveryUiSnapshot, TerminalDeathPhase,
    accessibility::AccessibilitySettings,
    bargain_ui::BargainUiAction,
    network_prediction::{CompleteSnapshot, SnapshotAssembler},
    network_transport::{
        NetworkStartup, NetworkTransportConfig, NetworkWorkerHandle, TransportEvent,
    },
};

const WINDOW_TITLE: &str = "Gravebound - Core Private Life";
const REALM_GATE_ID: &str = "station.realm_gate";
const BELL_DUNGEON_PORTAL_ID: &str = "portal.dungeon.bell_sepulcher";
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
    Retry,
}

#[derive(Debug, Resource)]
struct CorePrivateLifeBridge(NetworkWorkerHandle);

#[derive(Debug, Resource)]
struct CorePrivatePresentationContent(sim_content::CorePrivateLifeContent);

#[derive(Debug, Resource)]
struct CorePrivateBargainCopy(crate::bargain_ui::BargainUiCopy);

#[derive(Debug, Resource, Default)]
struct CorePrivateBargainState {
    model: crate::bargain_ui::BargainUiModel,
    open: bool,
    loaded: bool,
    may_advance_rest: bool,
}

#[derive(Debug, Resource, Default)]
struct CorePrivateHallInteractionState {
    latest: Option<protocol::HallInteractionResultV1>,
    open_station: Option<protocol::HallStationV1>,
}

impl CorePrivateHallInteractionState {
    fn reset(&mut self) {
        self.latest = None;
        self.open_station = None;
    }

    fn apply(&mut self, result: protocol::HallInteractionResultV1) {
        match result.code {
            protocol::HallInteractionResultCodeV1::Opened => {
                self.open_station = result.station;
            }
            protocol::HallInteractionResultCodeV1::Closed
            | protocol::HallInteractionResultCodeV1::InvalidState
            | protocol::HallInteractionResultCodeV1::CancelledOutOfRange => {
                self.open_station = None;
            }
            _ => {}
        }
        self.latest = Some(result);
    }

    fn is_holding(&self) -> bool {
        self.latest
            .as_ref()
            .is_some_and(|result| result.code == protocol::HallInteractionResultCodeV1::Holding)
    }
}

#[derive(Debug, Resource)]
struct CorePrivateTerminalUi {
    death: DeathViewClientModel,
    successor: Option<SuccessorRecoveryClientModel>,
    successor_content: CoreSuccessorRecoveryContent,
    content_manifest_hash: ManifestHash,
    death_config: DeathUiConfig,
    successor_config: SuccessorRecoveryUiConfig,
    retry_timer: Timer,
    view_signature: Option<String>,
    terminal_complete: bool,
}

impl CorePrivateTerminalUi {
    fn new(
        death: DeathViewClientModel,
        successor_content: CoreSuccessorRecoveryContent,
        content_manifest_hash: ManifestHash,
    ) -> Self {
        Self {
            death,
            successor: None,
            successor_content,
            content_manifest_hash,
            death_config: DeathUiConfig::default(),
            successor_config: SuccessorRecoveryUiConfig::default(),
            retry_timer: Timer::from_seconds(0.1, TimerMode::Once),
            view_signature: None,
            terminal_complete: false,
        }
    }

    fn accept_server_hello(
        &mut self,
        hello: &ServerHello,
    ) -> Result<(), CorePrivateLifeClientError> {
        let revision = WireText::new(self.successor_content.item_content_revision().to_owned())
            .map_err(|_| CorePrivateLifeClientError::InvalidSnapshotAuthority)?;
        self.successor = Some(SuccessorRecoveryClientModel::new(hello, revision));
        Ok(())
    }

    fn observe_summary_authority(&mut self) -> Result<(), CorePrivateLifeClientError> {
        let Some(authority) = self.death.terminal_successor_authority() else {
            return Ok(());
        };
        let successor = self
            .successor
            .as_mut()
            .ok_or(CorePrivateLifeClientError::InvalidSnapshotAuthority)?;
        if successor.phase() == SuccessorRecoveryPhase::AwaitingTerminalSummary {
            successor
                .observe_terminal_summary(authority)
                .map_err(|_| CorePrivateLifeClientError::InvalidSnapshotAuthority)?;
        }
        Ok(())
    }

    fn death_surface_open(&self) -> bool {
        !self.terminal_complete && self.death.terminal().phase() != TerminalDeathPhase::Inactive
    }

    fn memorial_surface_open(&self) -> bool {
        self.death.memorial().list_phase() != MemorialListPhase::Closed
            || self.death.memorial().detail_phase() != MemorialDetailPhase::Closed
    }

    fn surface_open(&self) -> bool {
        self.death_surface_open() || self.memorial_surface_open()
    }
}

impl CorePrivateBargainState {
    fn captures_input(&self) -> bool {
        self.open && self.model.captures_input()
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

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
    pending_inventory: Option<protocol::CorePendingInventoryStateV1>,
    extraction_ready: Option<protocol::CoreExtractionReadyStateV1>,
    extraction_frame: Option<protocol::ExtractionCommitFrameV1>,
    extraction_result: Option<protocol::ExtractionCommitResultV1>,
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
            pending_inventory: None,
            extraction_ready: None,
            extraction_frame: None,
            extraction_result: None,
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
        if !private_life_features_advertised(hello) {
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
        if !private_life_features_advertised(hello) {
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
            (
                CharacterLocation::Danger { location_id, .. },
                WorldTransferCommand::UsePortal { portal_id },
            ) => {
                location_id.as_str() == CorePrivateRouteSceneV1::CoreMicrorealm.location_id()
                    && portal_id.as_str() == BELL_DUNGEON_PORTAL_ID
                    && self.route.as_ref().is_some_and(|route| {
                        route.route_state().is_some_and(|state| {
                            state.readiness.bell_portal_available.is_available()
                        })
                    })
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
        let reset_extraction = route.route_state().is_some_and(|state| {
            matches!(
                state.scene,
                CorePrivateRouteSceneV1::LanternHalls | CorePrivateRouteSceneV1::CoreMicrorealm
            )
        });
        if reset_extraction {
            self.pending_inventory = None;
            self.extraction_ready = None;
            self.extraction_frame = None;
            self.extraction_result = None;
        }
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

    fn apply_pending_inventory(
        &mut self,
        state: protocol::CorePendingInventoryStateV1,
    ) -> Result<(), CorePrivateLifeClientError> {
        state
            .validate()
            .map_err(|_| CorePrivateLifeClientError::InvalidSnapshotAuthority)?;
        let route = self
            .route
            .as_ref()
            .and_then(CorePrivateRouteClientModel::route_state)
            .ok_or(CorePrivateLifeClientError::InvalidSnapshotAuthority)?;
        if Some(state.character_id) != self.selected_character_id()
            || route.character_id != state.character_id
            || route.instance_lineage_id != Some(state.instance_lineage_id)
            || route.character_version != state.expected_extraction_versions.character
        {
            return Err(CorePrivateLifeClientError::InvalidSnapshotAuthority);
        }
        self.pending_inventory = Some(state);
        Ok(())
    }

    fn apply_extraction_ready(
        &mut self,
        state: protocol::CoreExtractionReadyStateV1,
    ) -> Result<(), CorePrivateLifeClientError> {
        state
            .validate()
            .map_err(|_| CorePrivateLifeClientError::InvalidSnapshotAuthority)?;
        if Some(state.character_id) != self.selected_character_id() {
            return Err(CorePrivateLifeClientError::InvalidSnapshotAuthority);
        }
        self.extraction_ready = Some(state);
        Ok(())
    }

    fn begin_extraction(
        &mut self,
        issued_at_unix_millis: u64,
    ) -> Result<protocol::ExtractionCommitFrameV1, CorePrivateLifeClientError> {
        if let Some(frame) = self.extraction_frame.clone() {
            return Ok(frame);
        }
        let pending = self
            .pending_inventory
            .clone()
            .ok_or(CorePrivateLifeClientError::ActionUnavailable)?;
        let ready = self
            .extraction_ready
            .clone()
            .ok_or(CorePrivateLifeClientError::ActionUnavailable)?;
        let route = self
            .route
            .as_ref()
            .and_then(CorePrivateRouteClientModel::route_state)
            .ok_or(CorePrivateLifeClientError::ActionUnavailable)?;
        if !route.readiness.extraction_available.is_available()
            || pending.character_id != ready.character_id
            || pending.instance_lineage_id != ready.instance_lineage_id
            || pending.entry_restore_point_id != ready.entry_restore_point_id
            || pending.content_revision != ready.content_revision
            || pending.expected_extraction_versions != ready.expected_versions
            || !self.server_hello.as_ref().is_some_and(|hello| {
                protocol::TerminalInventoryCapabilityV1::ExtractionCommit.is_advertised_by(hello)
            })
        {
            return Err(CorePrivateLifeClientError::ActionUnavailable);
        }
        let sequence = self.take_request_sequence()?;
        let mutation_id = self.take_mutation_id()?;
        let payload = protocol::ExtractionCommitPayloadV1 {
            extraction_request_id: ready.extraction_request_id,
            expected_versions: ready.expected_versions,
            content_revision: ready.content_revision.clone(),
        };
        let frame = protocol::ExtractionCommitFrameV1 {
            schema_version: protocol::TERMINAL_INVENTORY_SCHEMA_VERSION,
            sequence,
            mutation_id,
            character_id: ready.character_id,
            issued_at_unix_millis,
            payload_hash: payload.canonical_hash(),
            payload,
        };
        frame
            .validate()
            .map_err(|_| CorePrivateLifeClientError::InvalidSnapshotAuthority)?;
        self.extraction_frame = Some(frame.clone());
        Ok(frame)
    }

    fn apply_extraction(
        &mut self,
        result: protocol::ExtractionCommitResultV1,
    ) -> Result<(), CorePrivateLifeClientError> {
        result
            .validate()
            .map_err(|_| CorePrivateLifeClientError::InvalidSnapshotAuthority)?;
        let frame = self
            .extraction_frame
            .as_ref()
            .ok_or(CorePrivateLifeClientError::InvalidSnapshotAuthority)?;
        let exact = match &result {
            protocol::ExtractionCommitResultV1::Pending {
                request_sequence,
                mutation_id,
                character_id,
                extraction_request_id,
                ..
            }
            | protocol::ExtractionCommitResultV1::Rejected {
                request_sequence,
                mutation_id,
                character_id,
                extraction_request_id,
                ..
            } => {
                *request_sequence == frame.sequence
                    && *mutation_id == frame.mutation_id
                    && *character_id == frame.character_id
                    && *extraction_request_id == frame.payload.extraction_request_id
            }
            protocol::ExtractionCommitResultV1::Stored {
                request_sequence,
                result,
                ..
            } => {
                *request_sequence == frame.sequence
                    && result.mutation_id == frame.mutation_id
                    && result.character_id == frame.character_id
                    && result.extraction_request_id == frame.payload.extraction_request_id
            }
        };
        if !exact {
            return Err(CorePrivateLifeClientError::InvalidSnapshotAuthority);
        }
        if matches!(result, protocol::ExtractionCommitResultV1::Stored { .. }) {
            self.location = None;
            self.pending_location_character = None;
            if let Some(route) = self.route.as_mut() {
                route.begin_committed_transfer_refresh()?;
            }
            self.phase = CorePrivateLifePhase::LoadingAuthority;
        }
        self.extraction_result = Some(result);
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

fn private_life_features_advertised(hello: &ServerHello) -> bool {
    [
        CORE_WORLD_FLOW_FEATURE_FLAG,
        protocol::HALL_INTERACTION_FEATURE_FLAG,
    ]
    .into_iter()
    .all(|required| {
        hello
            .feature_flags
            .iter()
            .any(|flag| flag.as_str() == required)
    })
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
    last_aim: (i16, i16),
}

impl Default for InputSequencer {
    fn default() -> Self {
        Self {
            input_sequence: 1,
            primary_sequence: 0,
            primary_held: false,
            last_aim: (1_000, 0),
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
#[derive(Component)]
struct PrivateGameplayGeometry;

type NormalPrivateUiVisibility<'w, 's> = Query<
    'w,
    's,
    &'static mut Visibility,
    Or<(
        With<StatusText>,
        With<RosterText>,
        With<RouteText>,
        With<ActionButton>,
    )>,
>;
type PrivateGameplayVisibility<'w, 's> = Query<
    'w,
    's,
    &'static mut Visibility,
    Or<(
        With<PrivateGameplayFloor>,
        With<PrivateGameplayEntity>,
        With<PrivateGameplayGeometry>,
    )>,
>;

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
    let bargain_catalog = sim_content::load_core_development_oaths_bargains(&config.content_root)
        .context("normal Core Bargain content failed validation")?;
    let bargain_copy = crate::bargain_ui::BargainUiCopy::from_catalog(&bargain_catalog)
        .context("normal Core Bargain presentation failed validation")?;
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
    let hello = private_life_hello(manifest_hash.clone(), config.test_token)?;
    let worker = NetworkWorkerHandle::spawn(NetworkTransportConfig {
        server_address: config.server_address,
        server_name: M02_LOCAL_SERVER_NAME.to_owned(),
        certificate_der,
        hello,
        startup: NetworkStartup::CoreIdentity {
            content_manifest_hash: manifest_hash.clone(),
        },
    })?;
    let (width, height) = crate::configured_window_size()?;
    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb_u8(5, 8, 11)))
        .insert_resource(AccessibilitySettings::default())
        .insert_resource(CorePrivateLifeBridge(worker))
        .insert_resource(CorePrivatePresentationContent(content))
        .insert_resource(CorePrivateBargainCopy(bargain_copy))
        .insert_resource(CorePrivateBargainState::default())
        .insert_resource(CorePrivateHallInteractionState::default())
        .insert_resource(load_terminal_ui(
            &config.content_root,
            manifest_hash.clone(),
        )?)
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
        .add_plugins((NativeDeathViewPlugin, NativeSuccessorRecoveryPlugin))
        .add_systems(Startup, spawn_ui)
        .add_systems(
            Update,
            (
                poll_transport,
                drive_terminal_death_lookup,
                handle_terminal_death_commands,
                handle_successor_recovery_commands,
                sync_terminal_views,
                request_location,
                handle_keyboard,
                handle_hall_interaction_keyboard,
                handle_recall_keyboard,
                handle_bargain_keyboard,
                handle_interact_keyboard,
                send_reliable_combat_edges,
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

fn private_life_hello(manifest_hash: ManifestHash, test_token: String) -> Result<ClientHello> {
    Ok(ClientHello {
        protocol_major: ProtocolVersion::current().major,
        protocol_minor: ProtocolVersion::current().minor,
        client_build_id: WireText::new(M03_CORE_DEV_BUILD_ID)?,
        platform: Platform::WindowsNative,
        supported_compression: vec![Compression::None],
        content_manifest_hash: manifest_hash,
        auth_ticket: AuthTicket::new(test_token.into_bytes())?,
        locale: WireText::new("en-US")?,
    })
}

fn load_terminal_ui(
    content_root: &std::path::Path,
    content_manifest_hash: ManifestHash,
) -> Result<CorePrivateTerminalUi> {
    let death = DeathViewClientModel::new(
        load_core_development_death_view(content_root)
            .context("normal Core death presentation failed validation")?,
    )
    .context("normal Core death projection failed validation")?;
    let successor = load_core_successor_recovery(content_root)
        .context("normal Core successor presentation failed validation")?;
    Ok(CorePrivateTerminalUi::new(
        death,
        successor,
        content_manifest_hash,
    ))
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn poll_transport(
    bridge: Res<CorePrivateLifeBridge>,
    mut client: ResMut<CorePrivateLifeClient>,
    mut snapshots: ResMut<CorePrivateSnapshotClient>,
    mut bargain: ResMut<CorePrivateBargainState>,
    mut hall: ResMut<CorePrivateHallInteractionState>,
    mut terminal: ResMut<CorePrivateTerminalUi>,
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
                client
                    .accept_server_hello(&hello)
                    .and_then(|()| terminal.accept_server_hello(&hello))
            }
            TransportEvent::Reliable(frame) => apply_private_reliable(
                &frame,
                &bridge,
                &mut client,
                &mut bargain,
                &mut hall,
                &mut terminal,
            ),
            TransportEvent::LinkLost
            | TransportEvent::Reconnecting { .. }
            | TransportEvent::TransportClosed => {
                snapshots.reset_transport();
                bargain.reset();
                hall.reset();
                discard_snapshot_queue = true;
                client.transport_lost();
                Ok(())
            }
            TransportEvent::Fatal(_) => {
                snapshots.reset_transport();
                bargain.reset();
                hall.reset();
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
    if client.phase != CorePrivateLifePhase::Hall {
        hall.reset();
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
    let at_b4 = client
        .route
        .as_ref()
        .and_then(CorePrivateRouteClientModel::route_state)
        .is_some_and(|route| {
            route.room == Some(protocol::CorePrivateRouteRoomV1::BellRestB4)
                && route.phase == protocol::CorePrivateRoutePhaseV1::Rest
        });
    if !at_b4 {
        bargain.reset();
    }
}

fn apply_private_reliable(
    frame: &ReliableEventFrame,
    bridge: &CorePrivateLifeBridge,
    client: &mut CorePrivateLifeClient,
    bargain: &mut CorePrivateBargainState,
    hall: &mut CorePrivateHallInteractionState,
    terminal: &mut CorePrivateTerminalUi,
) -> Result<(), CorePrivateLifeClientError> {
    match &frame.event {
        ReliableEvent::AccountBootstrapResult(result) => client.apply_bootstrap(result.clone()),
        ReliableEvent::CharacterMutationResult(result) => {
            client.apply_character_mutation(result.clone())
        }
        ReliableEvent::WorldFlowResult(result) => {
            if terminal
                .successor
                .as_ref()
                .is_some_and(|successor| successor.phase() == SuccessorRecoveryPhase::EnteringHall)
            {
                terminal
                    .successor
                    .as_mut()
                    .expect("checked successor recovery")
                    .apply_hall_result(result)
                    .map_err(|_| CorePrivateLifeClientError::InvalidSnapshotAuthority)?;
            }
            client.apply_world_flow(result.clone())
        }
        ReliableEvent::CorePrivateRouteState(_) => client.apply_route(frame),
        ReliableEvent::RecallResult(result) => client.apply_recall((**result).clone()),
        ReliableEvent::BargainViewResult(result) => {
            let result = result.clone();
            bargain.loaded = matches!(
                result.code,
                protocol::BargainResultCode::Available | protocol::BargainResultCode::NoOffer
            );
            bargain.may_advance_rest = result.code == protocol::BargainResultCode::NoOffer;
            bargain.open = result.code == protocol::BargainResultCode::Available;
            bargain.model.apply_view(result);
            Ok(())
        }
        ReliableEvent::BargainDecisionResult(result) => {
            let result = result.clone();
            bargain.may_advance_rest = matches!(
                result.code,
                protocol::BargainResultCode::Accepted | protocol::BargainResultCode::Refused
            );
            bargain.open = !bargain.may_advance_rest;
            bargain.model.apply_decision(result);
            Ok(())
        }
        ReliableEvent::CorePendingInventoryState(state) => {
            client.apply_pending_inventory((**state).clone())
        }
        ReliableEvent::CoreExtractionReadyState(state) => {
            client.apply_extraction_ready((**state).clone())
        }
        ReliableEvent::HallInteractionResult(result) => {
            apply_hall_interaction_result(*result, bridge, client, hall, terminal)
        }
        ReliableEvent::ExtractionCommitResult(result) => {
            client.apply_extraction((**result).clone())
        }
        ReliableEvent::DeathViewResult(result) => {
            let outcome = terminal
                .death
                .handle_result(result)
                .map_err(|_| CorePrivateLifeClientError::InvalidSnapshotAuthority)?;
            if let Some(follow_up) = outcome.follow_up {
                bridge
                    .0
                    .queue_reliable(WireMessage::DeathViewFrame(follow_up))
                    .map_err(|_| CorePrivateLifeClientError::ActionUnavailable)?;
            }
            terminal.observe_summary_authority()?;
            terminal.retry_timer.reset();
            terminal.view_signature = None;
            Ok(())
        }
        ReliableEvent::SuccessorCreateResult(result) => {
            terminal
                .successor
                .as_mut()
                .ok_or(CorePrivateLifeClientError::InvalidSnapshotAuthority)?
                .apply_create_result(result)
                .map_err(|_| CorePrivateLifeClientError::InvalidSnapshotAuthority)?;
            if matches!(
                result.as_ref(),
                protocol::SuccessorCreateResultV1::Stored { .. }
            ) {
                let sequence = client.take_request_sequence()?;
                bridge
                    .0
                    .queue_reliable(WireMessage::AccountBootstrapFrame(AccountBootstrapFrame {
                        sequence,
                        request: AccountBootstrapRequest::Refresh,
                        content_manifest_hash: terminal.content_manifest_hash.clone(),
                    }))
                    .map_err(|_| CorePrivateLifeClientError::ActionUnavailable)?;
            }
            terminal.view_signature = None;
            Ok(())
        }
        _ => Ok(()),
    }
}

fn apply_hall_interaction_result(
    result: protocol::HallInteractionResultV1,
    bridge: &CorePrivateLifeBridge,
    client: &mut CorePrivateLifeClient,
    hall: &mut CorePrivateHallInteractionState,
    terminal: &mut CorePrivateTerminalUi,
) -> Result<(), CorePrivateLifeClientError> {
    hall.apply(result);
    if result.code != protocol::HallInteractionResultCodeV1::Opened {
        return Ok(());
    }
    match result.station {
        Some(protocol::HallStationV1::RealmGate) => {
            queue_transfer(
                WorldTransferCommand::UsePortal {
                    portal_id: WireText::new(REALM_GATE_ID).expect("canonical gate ID fits"),
                },
                bridge,
                client,
            );
        }
        Some(protocol::HallStationV1::MemorialWall) => {
            let memorial = terminal
                .death
                .open_memorial_wall()
                .map_err(|_| CorePrivateLifeClientError::ActionUnavailable)?;
            bridge
                .0
                .queue_reliable(WireMessage::DeathViewFrame(memorial))
                .map_err(|_| CorePrivateLifeClientError::ActionUnavailable)?;
            terminal.view_signature = None;
        }
        Some(
            protocol::HallStationV1::Vault
            | protocol::HallStationV1::Overflow
            | protocol::HallStationV1::OathShrine,
        )
        | None => {}
    }
    Ok(())
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn drive_terminal_death_lookup(
    time: Res<Time>,
    bridge: Res<CorePrivateLifeBridge>,
    client: Res<CorePrivateLifeClient>,
    snapshots: Res<CorePrivateSnapshotClient>,
    mut terminal: ResMut<CorePrivateTerminalUi>,
) {
    if terminal.terminal_complete {
        return;
    }
    let lethal_observed = client.phase == CorePrivateLifePhase::TerminalPending
        && snapshots.latest.as_ref().is_some_and(|snapshot| {
            snapshot.entities.iter().any(|entity| {
                entity.kind == protocol::EntityKind::Player && entity.current_health == 0
            })
        });
    let request = match terminal.death.terminal().phase() {
        TerminalDeathPhase::Inactive if lethal_observed => {
            let Some(character_id) = client.selected_character_id() else {
                return;
            };
            terminal
                .death
                .observe_local_health_zero(character_id)
                .and_then(|()| terminal.death.begin_committed_death_lookup(character_id))
                .ok()
        }
        TerminalDeathPhase::AwaitingDurableAcknowledgement => {
            terminal.retry_timer.tick(time.delta());
            terminal
                .retry_timer
                .is_finished()
                .then(|| terminal.death.retry().ok())
                .flatten()
        }
        _ => None,
    };
    if let Some(frame) = request {
        if bridge
            .0
            .queue_reliable(WireMessage::DeathViewFrame(frame))
            .is_err()
        {
            return;
        }
        terminal.retry_timer.reset();
        terminal.view_signature = None;
    }
    if terminal.observe_summary_authority().is_err() {
        terminal.terminal_complete = true;
        return;
    }
    if terminal
        .successor
        .as_ref()
        .is_some_and(|successor| successor.phase() == SuccessorRecoveryPhase::LoadingHall)
        && client.phase == CorePrivateLifePhase::Hall
    {
        let readiness = CoreSceneReadiness {
            location_id: WireText::new(CorePrivateRouteSceneV1::LanternHalls.location_id())
                .expect("compiled Hall ID is bounded"),
            character_version: client
                .location
                .as_ref()
                .map_or(1, |location| location.character_version),
            content_revision: client.world_revision.clone(),
        };
        if terminal
            .successor
            .as_mut()
            .expect("checked successor recovery")
            .mark_hall_content_ready(&readiness)
            .is_ok()
        {
            terminal.terminal_complete = true;
            terminal.view_signature = None;
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn handle_terminal_death_commands(
    bridge: Res<CorePrivateLifeBridge>,
    mut client: ResMut<CorePrivateLifeClient>,
    mut terminal: ResMut<CorePrivateTerminalUi>,
    mut messages: MessageReader<DeathUiCommand>,
    view: Option<ResMut<NativeDeathView>>,
) {
    let mut view = view;
    for message in messages.read() {
        let frame = match message.0 {
            DeathUiAction::Summary(DeathSummaryAction::CreateSuccessor) => {
                let Ok(mutation_id) = client.take_mutation_id() else {
                    continue;
                };
                terminal.successor.as_mut().and_then(|successor| {
                    successor
                        .begin_create(mutation_id)
                        .ok()
                        .map(WireMessage::SuccessorCreateFrame)
                })
            }
            DeathUiAction::Summary(DeathSummaryAction::InspectTrace) => {
                if let Some(view) = view.as_mut() {
                    view.set_trace_emphasis(true);
                }
                None
            }
            DeathUiAction::Summary(DeathSummaryAction::Memorial) => terminal
                .death
                .open_memorial_wall()
                .ok()
                .map(WireMessage::DeathViewFrame),
            DeathUiAction::LoadMoreLosses => {
                if terminal.death.memorial().detail_phase() == MemorialDetailPhase::Closed {
                    terminal.death.load_more_losses().ok()
                } else {
                    terminal.death.load_more_memorial_losses().ok()
                }
                .map(WireMessage::DeathViewFrame)
            }
            DeathUiAction::Retry | DeathUiAction::Summary(DeathSummaryAction::Retry) => {
                if terminal.memorial_surface_open() {
                    terminal.death.retry_memorial().ok()
                } else {
                    terminal.death.retry().ok()
                }
                .map(WireMessage::DeathViewFrame)
            }
            DeathUiAction::MemorialEntry(cursor) => terminal
                .death
                .select_memorial(cursor)
                .ok()
                .map(WireMessage::DeathViewFrame),
            DeathUiAction::LoadOlderMemorials => terminal
                .death
                .load_older_memorials()
                .ok()
                .map(WireMessage::DeathViewFrame),
            DeathUiAction::Back => {
                if terminal.death.memorial().detail_phase() == MemorialDetailPhase::Closed {
                    let _ = terminal.death.close_memorial_wall();
                } else {
                    let _ = terminal.death.close_memorial_detail();
                }
                terminal.view_signature = None;
                None
            }
            DeathUiAction::Summary(DeathSummaryAction::CharacterSelect) => None,
        };
        if let Some(frame) = frame {
            if bridge.0.queue_reliable(frame).is_err() {
                client.phase = CorePrivateLifePhase::Error;
            }
            terminal.view_signature = None;
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn handle_successor_recovery_commands(
    bridge: Res<CorePrivateLifeBridge>,
    mut client: ResMut<CorePrivateLifeClient>,
    mut terminal: ResMut<CorePrivateTerminalUi>,
    mut messages: MessageReader<SuccessorRecoveryUiCommand>,
) {
    for message in messages.read() {
        let frame = match message.0 {
            SuccessorRecoveryUiAction::Play => {
                let Some(issued_at) = unix_millis() else {
                    continue;
                };
                let Ok(normal_frame) = client.begin_transfer(
                    WorldTransferCommand::EnterHallFromCharacterSelect,
                    issued_at,
                ) else {
                    continue;
                };
                let WorldFlowRequest::Transfer(mutation) = &normal_frame.request else {
                    continue;
                };
                let Some(successor) = terminal.successor.as_mut() else {
                    continue;
                };
                match successor.begin_play(
                    normal_frame.sequence,
                    mutation.mutation_id,
                    issued_at,
                    client.world_revision.clone(),
                ) {
                    Ok(successor_frame) if successor_frame == normal_frame => {
                        Some(WireMessage::WorldFlowFrame(normal_frame))
                    }
                    _ => {
                        client.phase = CorePrivateLifePhase::Error;
                        None
                    }
                }
            }
            SuccessorRecoveryUiAction::RetryCreate => terminal
                .successor
                .as_mut()
                .and_then(|successor| successor.retry_create().ok())
                .map(WireMessage::SuccessorCreateFrame),
            SuccessorRecoveryUiAction::RetryHall
            | SuccessorRecoveryUiAction::RefreshDeathSummary => None,
        };
        if let Some(frame) = frame {
            if bridge.0.queue_reliable(frame).is_err() {
                client.phase = CorePrivateLifePhase::Error;
            }
            terminal.view_signature = None;
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn sync_terminal_views(
    mut commands: Commands,
    mut terminal: ResMut<CorePrivateTerminalUi>,
    mut normal_ui: NormalPrivateUiVisibility,
    mut gameplay: PrivateGameplayVisibility,
    death_view: Option<ResMut<NativeDeathView>>,
    successor_view: Option<ResMut<NativeSuccessorRecoveryView>>,
) {
    let open = terminal.surface_open();
    let visibility = if open {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    for mut current in &mut normal_ui {
        *current = visibility;
    }
    for mut current in &mut gameplay {
        *current = visibility;
    }
    if !open {
        commands.remove_resource::<NativeDeathView>();
        commands.remove_resource::<NativeSuccessorRecoveryView>();
        terminal.view_signature = None;
        return;
    }

    if terminal.memorial_surface_open() {
        let snapshot = if terminal.death.memorial().detail_phase() == MemorialDetailPhase::Closed {
            DeathUiSnapshot::memorial_list(&terminal.death)
        } else {
            DeathUiSnapshot::memorial_detail(&terminal.death)
        };
        let Ok(snapshot) = snapshot else {
            return;
        };
        let signature = format!("memorial:{}", snapshot.semantic_signature());
        if terminal.view_signature.as_ref() == Some(&signature) {
            return;
        }
        if let Some(mut view) = death_view {
            if view.replace_snapshot(snapshot).is_err() {
                return;
            }
        } else if let Ok(view) = NativeDeathView::new(snapshot, terminal.death_config) {
            commands.remove_resource::<NativeSuccessorRecoveryView>();
            commands.insert_resource(view);
        }
        terminal.view_signature = Some(signature);
        return;
    }

    let successor_renderable = terminal.successor.as_ref().is_some_and(|successor| {
        !matches!(
            successor.phase(),
            SuccessorRecoveryPhase::Disabled
                | SuccessorRecoveryPhase::AwaitingTerminalSummary
                | SuccessorRecoveryPhase::Ready
        )
    });
    if successor_renderable {
        let Some(successor) = terminal.successor.as_ref() else {
            return;
        };
        let Ok(snapshot) =
            SuccessorRecoveryUiSnapshot::project(successor, &terminal.successor_content)
        else {
            return;
        };
        let signature = format!("successor:{}", snapshot.semantic_signature());
        if terminal.view_signature.as_ref() == Some(&signature) {
            return;
        }
        if let Some(mut view) = successor_view {
            view.replace_snapshot(snapshot);
        } else if let Ok(view) =
            NativeSuccessorRecoveryView::new(snapshot, terminal.successor_config)
        {
            commands.remove_resource::<NativeDeathView>();
            commands.insert_resource(view);
        }
        terminal.view_signature = Some(signature);
        return;
    }

    let snapshot = match terminal.successor.as_ref() {
        Some(successor) if successor.phase() == SuccessorRecoveryPhase::Ready => {
            DeathUiSnapshot::terminal_with_successor(&terminal.death, successor)
        }
        _ => DeathUiSnapshot::terminal(&terminal.death),
    };
    let Ok(snapshot) = snapshot else {
        return;
    };
    let signature = format!("death:{}", snapshot.semantic_signature());
    if terminal.view_signature.as_ref() == Some(&signature) {
        return;
    }
    if let Some(mut view) = death_view {
        if view.replace_snapshot(snapshot).is_err() {
            return;
        }
    } else if let Ok(view) = NativeDeathView::new(snapshot, terminal.death_config) {
        commands.remove_resource::<NativeSuccessorRecoveryView>();
        commands.insert_resource(view);
    }
    terminal.view_signature = Some(signature);
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
    bargain: Res<CorePrivateBargainState>,
    mut client: ResMut<CorePrivateLifeClient>,
) {
    if bargain.captures_input() {
        return;
    }
    let action = if keyboard.just_pressed(KeyCode::Digit1) {
        Some(PrivateLifeAction::Select(1))
    } else if keyboard.just_pressed(KeyCode::Digit2) {
        Some(PrivateLifeAction::Select(2))
    } else if keyboard.just_pressed(KeyCode::KeyN) {
        Some(PrivateLifeAction::Create)
    } else if keyboard.just_pressed(KeyCode::Enter) {
        Some(PrivateLifeAction::Play)
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
fn handle_hall_interaction_keyboard(
    keyboard: Res<ButtonInput<KeyCode>>,
    bridge: Res<CorePrivateLifeBridge>,
    hall: Res<CorePrivateHallInteractionState>,
    mut client: ResMut<CorePrivateLifeClient>,
) {
    if client.phase != CorePrivateLifePhase::Hall {
        return;
    }
    let intent = if keyboard.just_pressed(KeyCode::Escape) && hall.open_station.is_some() {
        Some(protocol::HallInteractionIntentV1::ClosePanel)
    } else if keyboard.just_pressed(KeyCode::KeyF) && hall.open_station.is_none() {
        Some(protocol::HallInteractionIntentV1::BeginHold)
    } else if keyboard.just_released(KeyCode::KeyF) && hall.is_holding() {
        Some(protocol::HallInteractionIntentV1::Release)
    } else {
        None
    };
    let Some(intent) = intent else {
        return;
    };
    let Ok(sequence) = client.take_request_sequence() else {
        client.phase = CorePrivateLifePhase::Error;
        return;
    };
    let frame = protocol::HallInteractionFrameV1 {
        schema_version: protocol::HALL_INTERACTION_SCHEMA_VERSION,
        sequence,
        intent,
    };
    if bridge
        .0
        .queue_reliable(WireMessage::HallInteractionFrame(frame))
        .is_err()
    {
        client.phase = CorePrivateLifePhase::Error;
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn handle_recall_keyboard(
    keyboard: Res<ButtonInput<KeyCode>>,
    bridge: Res<CorePrivateLifeBridge>,
    snapshots: Res<CorePrivateSnapshotClient>,
    bargain: Res<CorePrivateBargainState>,
    mut client: ResMut<CorePrivateLifeClient>,
) {
    if bargain.captures_input() {
        return;
    }
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
fn handle_bargain_keyboard(
    keyboard: Res<ButtonInput<KeyCode>>,
    bridge: Res<CorePrivateLifeBridge>,
    copy: Res<CorePrivateBargainCopy>,
    mut bargain: ResMut<CorePrivateBargainState>,
    mut client: ResMut<CorePrivateLifeClient>,
) {
    if !bargain.open {
        return;
    }
    let action = if keyboard.just_pressed(KeyCode::Digit1) {
        Some(BargainUiAction::Cell(0))
    } else if keyboard.just_pressed(KeyCode::Digit2) {
        Some(BargainUiAction::Cell(1))
    } else if keyboard.just_pressed(KeyCode::Digit3) {
        Some(BargainUiAction::Cell(2))
    } else if keyboard.just_pressed(KeyCode::KeyF) {
        Some(BargainUiAction::Refuse)
    } else if keyboard.just_pressed(KeyCode::Enter) {
        Some(BargainUiAction::Confirm)
    } else if keyboard.just_pressed(KeyCode::Escape) {
        Some(BargainUiAction::Cancel)
    } else {
        None
    };
    let Some(action) = action else {
        return;
    };
    match action {
        BargainUiAction::Cell(_) | BargainUiAction::Refuse => bargain.model.choose(action),
        BargainUiAction::Cancel => {
            if bargain.model.action_available(BargainUiAction::Cancel) {
                bargain.model.cancel();
            } else {
                bargain.open = false;
            }
        }
        BargainUiAction::Confirm => {
            let Ok(mutation_id) = client.take_mutation_id() else {
                bargain.model.mutation_failed();
                return;
            };
            let Some(issued_at) = unix_millis() else {
                bargain.model.mutation_failed();
                return;
            };
            let Some(frame) =
                bargain
                    .model
                    .confirm(mutation_id, issued_at, copy.0.revision.clone())
            else {
                return;
            };
            if bridge
                .0
                .queue_reliable(WireMessage::BargainDecisionFrame(frame))
                .is_err()
            {
                bargain.model.mutation_failed();
            }
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn handle_interact_keyboard(
    keyboard: Res<ButtonInput<KeyCode>>,
    bridge: Res<CorePrivateLifeBridge>,
    snapshots: Res<CorePrivateSnapshotClient>,
    copy: Res<CorePrivateBargainCopy>,
    mut bargain: ResMut<CorePrivateBargainState>,
    mut client: ResMut<CorePrivateLifeClient>,
) {
    if !keyboard.just_pressed(KeyCode::KeyF) || client.phase != CorePrivateLifePhase::PrivateRoute {
        return;
    }
    let Some(route) = client
        .route
        .as_ref()
        .and_then(CorePrivateRouteClientModel::route_state)
    else {
        return;
    };
    if route.readiness.extraction_available.is_available() {
        let Some(issued_at) = unix_millis() else {
            client.phase = CorePrivateLifePhase::Error;
            return;
        };
        let Ok(frame) = client.begin_extraction(issued_at) else {
            return;
        };
        if bridge
            .0
            .queue_reliable(WireMessage::ExtractionCommitFrame(frame))
            .is_err()
        {
            client.phase = CorePrivateLifePhase::Error;
        }
        return;
    }
    if route.readiness.bell_portal_available.is_available() {
        queue_transfer(
            WorldTransferCommand::UsePortal {
                portal_id: WireText::new(BELL_DUNGEON_PORTAL_ID)
                    .expect("canonical Bell portal ID fits"),
            },
            &bridge,
            &mut client,
        );
        return;
    }
    if route.room == Some(protocol::CorePrivateRouteRoomV1::BellRestB4)
        && route.phase == protocol::CorePrivateRoutePhaseV1::Rest
    {
        if bargain.open {
            return;
        }
        if bargain.may_advance_rest {
            bargain.reset();
        } else if bargain.loaded {
            bargain.open = true;
            return;
        } else {
            let Some(frame) = bargain
                .model
                .request_for_selected(client.selected_character_id(), copy.0.revision.clone())
            else {
                return;
            };
            if bridge
                .0
                .queue_reliable(WireMessage::BargainViewFrame(frame))
                .is_err()
            {
                bargain.model.request_failed();
                return;
            }
            bargain.open = true;
            return;
        }
    }
    if route.scene != CorePrivateRouteSceneV1::BellSepulcher
        || !route.readiness.room_exit_available.is_available()
        || route.readiness.extraction_available.is_available()
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

#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    reason = "the fixed input projector consumes independent Bevy resources without local authority"
)]
fn send_gameplay_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<PrivateGameplayCamera>>,
    bridge: Res<CorePrivateLifeBridge>,
    client: Res<CorePrivateLifeClient>,
    snapshots: Res<CorePrivateSnapshotClient>,
    content: Res<CorePrivatePresentationContent>,
    bargain: Res<CorePrivateBargainState>,
    mut sequencer: ResMut<InputSequencer>,
) {
    if bargain.captures_input()
        || !client
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
    let aim = client
        .route
        .as_ref()
        .and_then(CorePrivateRouteClientModel::route_state)
        .and_then(|route| {
            cursor_private_aim(
                &window,
                *camera,
                &content.0,
                route,
                snapshots.latest.as_ref(),
            )
        })
        .unwrap_or(sequencer.last_aim);
    sequencer.last_aim = aim;
    let sequence = sequencer.input_sequence;
    bridge.0.replace_input(protocol::InputFrame {
        sequence,
        client_tick: u64::from(sequence),
        movement_x_milli: horizontal_milli,
        movement_y_milli: vertical_milli,
        aim_x_milli: aim.0,
        aim_y_milli: aim.1,
        held_primary,
        primary_sequence: sequencer.primary_sequence,
        ability_1_sequence: 0,
        ability_2_sequence: 0,
    });
    if let Some(next) = sequence.checked_add(1) {
        sequencer.input_sequence = next;
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn send_reliable_combat_edges(
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    bridge: Res<CorePrivateLifeBridge>,
    snapshots: Res<CorePrivateSnapshotClient>,
    bargain: Res<CorePrivateBargainState>,
    mut client: ResMut<CorePrivateLifeClient>,
) {
    if bargain.captures_input()
        || client.phase != CorePrivateLifePhase::PrivateRoute
        || !client
            .route
            .as_ref()
            .is_some_and(CorePrivateRouteClientModel::can_accept_gameplay_input)
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
    for action in [
        mouse
            .just_pressed(MouseButton::Right)
            .then_some(protocol::ActionKind::Ability1Press),
        keyboard
            .just_pressed(KeyCode::Space)
            .then_some(protocol::ActionKind::Ability2Press),
    ]
    .into_iter()
    .flatten()
    {
        let Ok(sequence) = client.take_request_sequence() else {
            client.phase = CorePrivateLifePhase::Error;
            return;
        };
        let frame = protocol::ActionFrame {
            sequence,
            client_tick,
            action,
        };
        if bridge
            .0
            .queue_reliable(WireMessage::ActionFrame(frame))
            .is_err()
        {
            client.phase = CorePrivateLifePhase::Error;
            return;
        }
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn cursor_private_aim(
    window: &Window,
    (camera, camera_transform): (&Camera, &GlobalTransform),
    content: &sim_content::CorePrivateLifeContent,
    route: &protocol::CorePrivateRouteStateV1,
    snapshot: Option<&CompleteSnapshot>,
) -> Option<(i16, i16)> {
    let cursor = window.cursor_position()?;
    let cursor_world = camera.viewport_to_world_2d(camera_transform, cursor).ok()?;
    let (width, height) = private_scene_dimensions(content, route)?;
    let player = snapshot?
        .entities
        .iter()
        .find(|entity| entity.kind == protocol::EntityKind::Player)?;
    let player_world = private_snapshot_position(player, width, height);
    let delta_x = cursor_world.x - player_world.x;
    let delta_y = player_world.y - cursor_world.y;
    let length = delta_x.hypot(delta_y);
    if !length.is_finite() || length <= f32::EPSILON {
        return None;
    }
    Some((
        (delta_x / length * 1_000.0).round() as i16,
        (delta_y / length * 1_000.0).round() as i16,
    ))
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
    geometry: Query<Entity, With<PrivateGameplayGeometry>>,
) {
    let Some(route) = client
        .route
        .as_ref()
        .and_then(CorePrivateRouteClientModel::route_state)
        .filter(|state| {
            matches!(
                state.scene,
                CorePrivateRouteSceneV1::LanternHalls
                    | CorePrivateRouteSceneV1::CoreMicrorealm
                    | CorePrivateRouteSceneV1::BellSepulcher
            )
        })
    else {
        despawn_private_gameplay(&mut commands, &entities, &floors, &geometry);
        return;
    };
    let Some(snapshot) = snapshots.latest.as_ref() else {
        despawn_private_gameplay(&mut commands, &entities, &floors, &geometry);
        return;
    };
    let Some((width, height)) = private_scene_dimensions(&content.0, route) else {
        despawn_private_gameplay(&mut commands, &entities, &floors, &geometry);
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
        for entity in &geometry {
            commands.entity(entity).despawn();
        }
        commands.spawn((
            Name::new("Authoritative private arena"),
            floor_binding,
            Sprite::from_color(Color::srgb_u8(12, 20, 24), Vec2::new(width, height)),
            Transform::from_xyz(0.0, 0.0, -1.0),
        ));
        spawn_private_geometry(&mut commands, &content.0, route, width, height);
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
    geometry: &Query<Entity, With<PrivateGameplayGeometry>>,
) {
    for (entity, _, _, _) in entities {
        commands.entity(entity).despawn();
    }
    for (entity, _) in floors {
        commands.entity(entity).despawn();
    }
    for entity in geometry {
        commands.entity(entity).despawn();
    }
}

fn spawn_private_geometry(
    commands: &mut Commands,
    content: &sim_content::CorePrivateLifeContent,
    route: &protocol::CorePrivateRouteStateV1,
    width: f32,
    height: f32,
) {
    match route.scene {
        CorePrivateRouteSceneV1::LanternHalls | CorePrivateRouteSceneV1::CoreMicrorealm => {
            let scene = if route.scene == CorePrivateRouteSceneV1::LanternHalls {
                content.hall_scene()
            } else {
                content.microrealm_scene()
            };
            spawn_world_scene_geometry(commands, scene, width, height);
        }
        CorePrivateRouteSceneV1::BellSepulcher => {
            spawn_bell_geometry(commands, content, route, width, height);
        }
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "compiled Core scene dimensions are bounded well inside exact f32 integer range"
)]
fn spawn_world_scene_geometry(
    commands: &mut Commands,
    scene: &sim_core::WorldSceneDefinition,
    width: f32,
    height: f32,
) {
    let shell = scene.shell_thickness_milli_tiles;
    for rectangle in [
        sim_core::TileRectangle::new(0, 0, scene.width_milli_tiles, shell),
        sim_core::TileRectangle::new(
            0,
            scene.height_milli_tiles - shell,
            scene.width_milli_tiles,
            shell,
        ),
        sim_core::TileRectangle::new(0, shell, shell, scene.height_milli_tiles - shell * 2),
        sim_core::TileRectangle::new(
            scene.width_milli_tiles - shell,
            shell,
            shell,
            scene.height_milli_tiles - shell * 2,
        ),
    ]
    .into_iter()
    .chain(scene.solid_rectangles.iter().copied())
    {
        spawn_private_rectangle(
            commands,
            rectangle,
            width,
            height,
            Color::srgb_u8(42, 48, 49),
            0.0,
        );
    }
    for object in &scene.objects {
        let (center, size) = match object.geometry {
            sim_core::SceneObjectGeometry::Point(point)
            | sim_core::SceneObjectGeometry::PointInteractable { point, .. } => (
                authored_private_point(point, width, height),
                Vec2::splat(if object.id == REALM_GATE_ID {
                    1.25
                } else {
                    0.72
                }),
            ),
            sim_core::SceneObjectGeometry::Circle {
                center,
                radius_milli_tiles,
            } => (
                authored_private_point(center, width, height),
                Vec2::splat(radius_milli_tiles as f32 / 500.0),
            ),
            sim_core::SceneObjectGeometry::Rectangle(rectangle) => {
                authored_private_rectangle(rectangle, width, height)
            }
        };
        let color = private_scene_object_color(&object.id);
        commands.spawn((
            Name::new(format!("Authoritative {}", object.id)),
            PrivateGameplayGeometry,
            Sprite::from_color(color, size.max(Vec2::splat(0.24))),
            Transform::from_xyz(center.x, center.y, 1.0),
        ));
    }
}

fn private_scene_object_color(object_id: &str) -> Color {
    let enabled_core_station = matches!(
        object_id,
        "station.realm_gate"
            | "station.vault"
            | "station.overflow"
            | "station.memorial_wall"
            | "station.oath_shrine"
    );
    if object_id.starts_with("station.") && !enabled_core_station {
        Color::srgb_u8(72, 74, 73)
    } else if object_id == "station.memorial_wall" {
        Color::srgb_u8(172, 151, 111)
    } else if object_id == "station.oath_shrine" {
        Color::srgb_u8(126, 101, 156)
    } else if object_id.starts_with("station.") {
        Color::srgb_u8(114, 151, 143)
    } else {
        Color::srgb_u8(103, 119, 115)
    }
}

fn spawn_bell_geometry(
    commands: &mut Commands,
    content: &sim_content::CorePrivateLifeContent,
    route: &protocol::CorePrivateRouteStateV1,
    width: f32,
    height: f32,
) {
    let Some(room) = route.room.and_then(|room| {
        content
            .fixed_layout()
            .rooms
            .iter()
            .find(|candidate| candidate.node_id == room.node_id())
    }) else {
        return;
    };
    let border = 500;
    let room_width = i32::try_from(room.room.width_milli_tiles).unwrap_or(i32::MAX);
    let room_height = i32::try_from(room.room.height_milli_tiles).unwrap_or(i32::MAX);
    for rectangle in [
        sim_core::TileRectangle::new(0, 0, room_width, border),
        sim_core::TileRectangle::new(0, room_height - border, room_width, border),
        sim_core::TileRectangle::new(0, border, border, room_height - border * 2),
        sim_core::TileRectangle::new(
            room_width - border,
            border,
            border,
            room_height - border * 2,
        ),
    ] {
        spawn_private_rectangle(
            commands,
            rectangle,
            width,
            height,
            Color::srgb_u8(50, 45, 44),
            0.0,
        );
    }
    for volume in room
        .room
        .volumes
        .iter()
        .filter(|volume| volume.kind == sim_core::DungeonRoomVolumeKind::Solid)
    {
        let sim_core::DungeonRoomVolumeGeometry::Rectangle {
            x,
            y,
            width: rectangle_width,
            height: rectangle_height,
        } = volume.geometry
        else {
            continue;
        };
        let (Ok(rectangle_width), Ok(rectangle_height)) = (
            i32::try_from(rectangle_width),
            i32::try_from(rectangle_height),
        ) else {
            continue;
        };
        spawn_private_rectangle(
            commands,
            sim_core::TileRectangle::new(x, y, rectangle_width, rectangle_height),
            width,
            height,
            Color::srgb_u8(55, 49, 47),
            0.1,
        );
    }
}

fn spawn_private_rectangle(
    commands: &mut Commands,
    rectangle: sim_core::TileRectangle,
    width: f32,
    height: f32,
    color: Color,
    z: f32,
) {
    let (center, size) = authored_private_rectangle(rectangle, width, height);
    commands.spawn((
        Name::new("Authoritative collision"),
        PrivateGameplayGeometry,
        Sprite::from_color(color, size),
        Transform::from_xyz(center.x, center.y, z),
    ));
}

#[allow(clippy::cast_precision_loss)]
fn authored_private_point(point: sim_core::TilePoint, width: f32, height: f32) -> Vec2 {
    Vec2::new(
        point.x_milli_tiles as f32 / 1_000.0 - width * 0.5,
        height * 0.5 - point.y_milli_tiles as f32 / 1_000.0,
    )
}

#[allow(clippy::cast_precision_loss)]
fn authored_private_rectangle(
    rectangle: sim_core::TileRectangle,
    width: f32,
    height: f32,
) -> (Vec2, Vec2) {
    let rectangle_width = rectangle.width_milli_tiles as f32 / 1_000.0;
    let rectangle_height = rectangle.height_milli_tiles as f32 / 1_000.0;
    (
        Vec2::new(
            rectangle.x_milli_tiles as f32 / 1_000.0 + rectangle_width * 0.5 - width * 0.5,
            height * 0.5 - (rectangle.y_milli_tiles as f32 / 1_000.0 + rectangle_height * 0.5),
        ),
        Vec2::new(rectangle_width, rectangle_height),
    )
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
        CorePrivateRouteSceneV1::LanternHalls => Some((
            content.hall_scene().width_milli_tiles as f32 / 1_000.0,
            content.hall_scene().height_milli_tiles as f32 / 1_000.0,
        )),
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
                    spawn_button(row, PrivateLifeAction::Retry, "Retry [R]");
                });
            root.spawn((
                Text::new(
                    "MOVE WASD  AIM MOUSE  FIRE LMB  ABILITY RMB / SPACE  USE Q / E  INTERACT F  RECALL HOLD R",
                ),
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
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "Bevy system parameters own disjoint query filters"
)]
fn update_ui(
    client: Res<CorePrivateLifeClient>,
    snapshots: Res<CorePrivateSnapshotClient>,
    content: Res<CorePrivatePresentationContent>,
    bargain: Res<CorePrivateBargainState>,
    bargain_copy: Res<CorePrivateBargainCopy>,
    hall: Res<CorePrivateHallInteractionState>,
    mut status: Single<&mut Text, With<StatusText>>,
    mut roster: Single<&mut Text, (With<RosterText>, Without<StatusText>)>,
    mut route: Single<&mut Text, (With<RouteText>, Without<StatusText>, Without<RosterText>)>,
    mut actions: Query<(&ActionButton, &mut BackgroundColor, &mut BorderColor)>,
) {
    **status = Text::new(phase_label(client.phase));
    **roster = Text::new(render_roster(&client));
    let route_text = render_route(&client, &snapshots, &content.0, &hall);
    **route = Text::new(if bargain.open {
        format!(
            "{}\n\n{}",
            route_text,
            bargain.model.render(&bargain_copy.0)
        )
    } else if bargain.may_advance_rest {
        format!("{route_text}\n\nBargain resolved. Press F to continue.")
    } else {
        route_text
    });
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

fn render_route(
    client: &CorePrivateLifeClient,
    snapshots: &CorePrivateSnapshotClient,
    content: &sim_content::CorePrivateLifeContent,
    hall: &CorePrivateHallInteractionState,
) -> String {
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
    let recall = render_recall_status(client.recall_result.as_ref());
    let extraction = render_extraction_status(client.extraction_result.as_ref());
    let hall_interaction = render_hall_interaction(state, snapshots, content, hall);
    format!(
        "{scene}{room}\nPhase: {:?}\nActor generation: {}    State version: {}\nControl: {}{gameplay}{hall_interaction}{recall}{extraction}{transfer}",
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

fn render_hall_interaction(
    route: &protocol::CorePrivateRouteStateV1,
    snapshots: &CorePrivateSnapshotClient,
    content: &sim_content::CorePrivateLifeContent,
    hall: &CorePrivateHallInteractionState,
) -> String {
    if route.scene != CorePrivateRouteSceneV1::LanternHalls {
        return String::new();
    }
    if let Some(station) = hall.open_station {
        return format!(
            "\n{} — panel open    Esc close",
            hall_station_label(station)
        );
    }
    if let Some(result) = hall.latest.as_ref() {
        match result.code {
            protocol::HallInteractionResultCodeV1::Holding => {
                return format!(
                    "\nHold F — {}    {}/{}",
                    result.station.map_or("Station", hall_station_label),
                    result.held_ticks,
                    result.required_ticks
                );
            }
            protocol::HallInteractionResultCodeV1::CancelledOutOfRange
            | protocol::HallInteractionResultCodeV1::OutOfRange => {
                return "\nMove within 1.5 tiles of an active Hall station.".to_owned();
            }
            protocol::HallInteractionResultCodeV1::CancelledReleased => {
                return "\nInteraction cancelled — hold F until complete.".to_owned();
            }
            _ => {}
        }
    }
    nearest_hall_station(content, snapshots.latest.as_ref()).map_or_else(
        || "\nExplore the Hall — active stations glow in color.".to_owned(),
        |station| {
            let instruction = if matches!(
                station,
                protocol::HallStationV1::RealmGate
                    | protocol::HallStationV1::Vault
                    | protocol::HallStationV1::Overflow
            ) {
                "Press F"
            } else {
                "Hold F"
            };
            format!("\n{instruction} — {}", hall_station_label(station))
        },
    )
}

fn nearest_hall_station(
    content: &sim_content::CorePrivateLifeContent,
    snapshot: Option<&CompleteSnapshot>,
) -> Option<protocol::HallStationV1> {
    let player = snapshot?
        .entities
        .iter()
        .find(|entity| entity.kind == protocol::EntityKind::Player)?;
    content
        .hall_scene()
        .objects
        .iter()
        .filter_map(|object| {
            let station = protocol::HallStationV1::from_content_id(&object.id)?;
            let interaction = object.interaction?;
            let point = match object.geometry {
                sim_core::SceneObjectGeometry::Point(point)
                | sim_core::SceneObjectGeometry::PointInteractable { point, .. } => point,
                sim_core::SceneObjectGeometry::Circle { center, .. } => center,
                sim_core::SceneObjectGeometry::Rectangle(rectangle) => sim_core::TilePoint::new(
                    rectangle.x_milli_tiles + rectangle.width_milli_tiles / 2,
                    rectangle.y_milli_tiles + rectangle.height_milli_tiles / 2,
                ),
            };
            let dx = i64::from(player.x_milli_tiles) - i64::from(point.x_milli_tiles);
            let dy = i64::from(player.y_milli_tiles) - i64::from(point.y_milli_tiles);
            let distance_squared = dx * dx + dy * dy;
            let range = i64::from(interaction.range_milli_tiles);
            (distance_squared <= range * range).then_some((distance_squared, station))
        })
        .min_by_key(|(distance_squared, _)| *distance_squared)
        .map(|(_, station)| station)
}

const fn hall_station_label(station: protocol::HallStationV1) -> &'static str {
    match station {
        protocol::HallStationV1::RealmGate => "Realm Gate",
        protocol::HallStationV1::Vault => "Vault",
        protocol::HallStationV1::Overflow => "Overflow",
        protocol::HallStationV1::MemorialWall => "Memorial Wall",
        protocol::HallStationV1::OathShrine => "Oath Shrine",
    }
}

fn render_recall_status(result: Option<&protocol::RecallResultV1>) -> String {
    match result {
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
    }
}

fn render_extraction_status(result: Option<&protocol::ExtractionCommitResultV1>) -> String {
    match result {
        Some(protocol::ExtractionCommitResultV1::Pending { .. }) => {
            "\nExtraction: committing secured inventory".to_owned()
        }
        Some(protocol::ExtractionCommitResultV1::Stored { result, .. }) => format!(
            "\nExtraction: committed — {} item placements{}",
            result.placements.len(),
            if result.storage_resolution_required {
                " — storage resolution required"
            } else {
                ""
            }
        ),
        Some(protocol::ExtractionCommitResultV1::Rejected { code, .. }) => {
            format!("\nExtraction: {code:?} — press E to retry")
        }
        None => String::new(),
    }
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
            feature_flags: if enabled {
                vec![
                    WireText::new(CORE_WORLD_FLOW_FEATURE_FLAG).unwrap(),
                    WireText::new(protocol::HALL_INTERACTION_FEATURE_FLAG).unwrap(),
                ]
            } else {
                Vec::new()
            },
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

    #[test]
    fn extraction_reuses_exact_server_bound_request() {
        let mut client = CorePrivateLifeClient::new(world_revision(), route_revision());
        let mut server_hello = hello(true);
        server_hello
            .feature_flags
            .push(WireText::new(protocol::CORE_EXTRACTION_TERMINAL_FEATURE_FLAG).unwrap());
        client.accept_server_hello(&server_hello).unwrap();
        client.set_account(account()).unwrap();
        client
            .accept_location(CharacterLocationSnapshot {
                character_id: [7; 16],
                character_version: 2,
                location: CharacterLocation::Danger {
                    location_id: WireText::new("dungeon.bell_sepulcher").unwrap(),
                    instance_lineage_id: [9; 16],
                    entry_restore_point_id: [3; 16],
                },
            })
            .unwrap();
        client
            .apply_route(&ReliableEventFrame {
                sequence: 1,
                server_tick: 30,
                event: ReliableEvent::CorePrivateRouteState(Box::new(CorePrivateRouteStateV1 {
                    schema_version: protocol::CORE_PRIVATE_ROUTE_SCHEMA_VERSION,
                    character_id: [7; 16],
                    character_version: 2,
                    content_revision: route_revision(),
                    actor_generation: 1,
                    state_version: 7,
                    instance_lineage_id: Some([9; 16]),
                    scene: CorePrivateRouteSceneV1::BellSepulcher,
                    room: Some(protocol::CorePrivateRouteRoomV1::CaldusArenaB6),
                    phase: CorePrivateRoutePhaseV1::BossExitReady,
                    readiness: CorePrivateRouteReadinessV1::canonical(
                        CorePrivateRoutePhaseV1::BossExitReady,
                    ),
                })),
            })
            .unwrap();
        let expected_versions = protocol::TerminalExpectedVersionsV1 {
            account: 1,
            character: 2,
            world: 2,
            inventory: 3,
            life_clock: 4,
        };
        client
            .apply_pending_inventory(protocol::CorePendingInventoryStateV1 {
                schema_version: protocol::CORE_PENDING_INVENTORY_SCHEMA_VERSION,
                character_id: [7; 16],
                instance_lineage_id: [9; 16],
                entry_restore_point_id: [3; 16],
                location_content_id: WireText::new("dungeon.bell_sepulcher").unwrap(),
                content_revision: world_revision(),
                expected_extraction_versions: expected_versions,
                items: Vec::new(),
                materials: Vec::new(),
            })
            .unwrap();
        client
            .apply_extraction_ready(protocol::CoreExtractionReadyStateV1 {
                schema_version: protocol::CORE_PENDING_INVENTORY_SCHEMA_VERSION,
                character_id: [7; 16],
                instance_lineage_id: [9; 16],
                entry_restore_point_id: [3; 16],
                extraction_request_id: [4; 16],
                content_revision: world_revision(),
                expected_versions,
            })
            .unwrap();

        let first = client.begin_extraction(1_000).unwrap();
        let retry = client.begin_extraction(9_999).unwrap();
        assert_eq!(retry, first);
        assert_eq!(first.character_id, [7; 16]);
        assert_eq!(first.payload.extraction_request_id, [4; 16]);
        assert_eq!(first.payload.expected_versions, expected_versions);
        assert_eq!(first.payload.content_revision, world_revision());
    }
}
