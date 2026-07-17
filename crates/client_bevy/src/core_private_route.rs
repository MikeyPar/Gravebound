//! Native authority projection for the ordinary M03 private-life route.
//!
//! This module owns no transport, widgets, simulation, or destinations. Control becomes legal
//! only when durable location, server route state, and exact locally compiled scene readiness all
//! agree for the negotiated character, content, actor generation, and route-state version.

use protocol::{
    CHARACTER_ID_BYTES, CORE_PRIVATE_ROUTE_PROTOCOL_MINOR, CORE_WORLD_FLOW_FEATURE_FLAG,
    CharacterLocation, CharacterLocationSnapshot, CorePrivateRouteAvailabilityV1,
    CorePrivateRouteContentRevisionV1, CorePrivateRoutePhaseV1, CorePrivateRouteRoomV1,
    CorePrivateRouteSceneV1, CorePrivateRouteStateV1, PROTOCOL_MAJOR, ReliableEvent,
    ReliableEventFrame, ServerHello, WorldFlowContentRevisionV1,
};
use thiserror::Error;

use crate::{CoreSceneReadiness, CoreWorldTransitionModel, CoreWorldTransitionPhase};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePrivateRouteClientPhase {
    Disabled,
    AwaitingAuthority,
    LoadingScene,
    Controllable,
    TerminalPending,
    FatalAuthorityError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePrivateRouteClientFailure {
    InvalidServerHello,
    FeatureNotNegotiated,
    InvalidProjection,
    ForeignCharacter,
    ContentMismatch,
    StaleReliableSequence,
    StaleProjection,
    ConflictingReplay,
    GenerationAdvanceWithoutAuthority,
    InvalidLocationAuthority,
    InvalidSceneReadiness,
    UnexpectedReliableEvent,
}

/// Exact local readiness for one compiled route scene. The embedded existing readiness preserves
/// location/version/content checks while the appended fields distinguish B0-B6 and actor ABA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePrivateSceneReadiness {
    pub base: CoreSceneReadiness,
    pub scene: CorePrivateRouteSceneV1,
    pub room: Option<CorePrivateRouteRoomV1>,
    pub instance_lineage_id: Option<[u8; protocol::INSTANCE_LINEAGE_ID_BYTES]>,
    pub actor_generation: u64,
    pub route_state_version: u64,
}

impl CorePrivateSceneReadiness {
    fn validate(&self) -> Result<(), CorePrivateRouteClientError> {
        if self.actor_generation == 0
            || self.route_state_version == 0
            || self.base.character_version == 0
            || self.base.location_id.as_str() != self.scene.location_id()
            || self
                .instance_lineage_id
                .is_some_and(|lineage| lineage.iter().all(|byte| *byte == 0))
        {
            return Err(CorePrivateRouteClientError::InvalidSceneReadiness);
        }
        let shape_matches = match self.scene {
            CorePrivateRouteSceneV1::LanternHalls => {
                self.room.is_none() && self.instance_lineage_id.is_none()
            }
            CorePrivateRouteSceneV1::CoreMicrorealm => {
                self.room.is_none() && self.instance_lineage_id.is_some()
            }
            CorePrivateRouteSceneV1::BellSepulcher => {
                self.room.is_some() && self.instance_lineage_id.is_some()
            }
        };
        if !shape_matches {
            return Err(CorePrivateRouteClientError::InvalidSceneReadiness);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePrivateRouteClientModel {
    character_id: [u8; CHARACTER_ID_BYTES],
    world_flow_revision: WorldFlowContentRevisionV1,
    route_content_revision: CorePrivateRouteContentRevisionV1,
    phase: CorePrivateRouteClientPhase,
    failure: Option<CorePrivateRouteClientFailure>,
    feature_authorized: bool,
    last_reliable_sequence: u32,
    generation_advance_authorized: bool,
    retired_generation_floor: u64,
    location: Option<CharacterLocationSnapshot>,
    route_state: Option<CorePrivateRouteStateV1>,
    scene_readiness: Option<CorePrivateSceneReadiness>,
}

impl CorePrivateRouteClientModel {
    pub fn new(
        character_id: [u8; CHARACTER_ID_BYTES],
        world_flow_revision: WorldFlowContentRevisionV1,
        route_content_revision: CorePrivateRouteContentRevisionV1,
    ) -> Result<Self, CorePrivateRouteClientError> {
        if character_id.iter().all(|byte| *byte == 0) {
            return Err(CorePrivateRouteClientError::ForeignCharacter);
        }
        route_content_revision
            .validate()
            .map_err(|_| CorePrivateRouteClientError::ContentMismatch)?;
        Ok(Self {
            character_id,
            world_flow_revision,
            route_content_revision,
            phase: CorePrivateRouteClientPhase::Disabled,
            failure: None,
            feature_authorized: false,
            last_reliable_sequence: 0,
            generation_advance_authorized: false,
            retired_generation_floor: 0,
            location: None,
            route_state: None,
            scene_readiness: None,
        })
    }

    #[must_use]
    pub const fn phase(&self) -> CorePrivateRouteClientPhase {
        self.phase
    }

    #[must_use]
    pub const fn failure(&self) -> Option<CorePrivateRouteClientFailure> {
        self.failure
    }

    #[must_use]
    pub const fn feature_authorized(&self) -> bool {
        self.feature_authorized
    }

    #[must_use]
    pub const fn route_state(&self) -> Option<&CorePrivateRouteStateV1> {
        self.route_state.as_ref()
    }

    #[must_use]
    pub const fn can_accept_gameplay_input(&self) -> bool {
        matches!(self.phase, CorePrivateRouteClientPhase::Controllable)
    }

    /// Applies one exact handshake generation. Feature absence is a supported disabled state, not
    /// a client-side invitation to infer route availability.
    pub fn accept_server_hello(
        &mut self,
        hello: &ServerHello,
    ) -> Result<bool, CorePrivateRouteClientError> {
        if hello.validate().is_err()
            || hello.protocol_major != PROTOCOL_MAJOR
            || hello.protocol_minor != CORE_PRIVATE_ROUTE_PROTOCOL_MINOR
        {
            return self.fail(
                CorePrivateRouteClientFailure::InvalidServerHello,
                CorePrivateRouteClientError::InvalidServerHello,
            );
        }
        self.last_reliable_sequence = 0;
        self.location = None;
        self.scene_readiness = None;
        self.failure = None;
        self.feature_authorized = hello
            .feature_flags
            .iter()
            .any(|flag| flag.as_str() == CORE_WORLD_FLOW_FEATURE_FLAG);
        if !self.feature_authorized {
            self.route_state = None;
            self.generation_advance_authorized = false;
            self.phase = CorePrivateRouteClientPhase::Disabled;
            return Ok(false);
        }
        self.generation_advance_authorized = true;
        self.phase = CorePrivateRouteClientPhase::AwaitingAuthority;
        Ok(true)
    }

    /// Immediately revokes control. A reconnect handshake must be accepted before a replacement
    /// generation can publish state.
    pub fn transport_lost(&mut self) {
        self.location = None;
        self.scene_readiness = None;
        self.generation_advance_authorized = false;
        if self.feature_authorized && self.phase != CorePrivateRouteClientPhase::FatalAuthorityError
        {
            self.phase = CorePrivateRouteClientPhase::AwaitingAuthority;
        }
    }

    /// Authorizes one actor-generation advance after a committed transfer. It never creates a
    /// destination locally; callers invoke it only after the owning world-flow result is accepted.
    pub fn begin_committed_transfer_refresh(&mut self) -> Result<(), CorePrivateRouteClientError> {
        self.require_feature()?;
        if self.phase == CorePrivateRouteClientPhase::FatalAuthorityError {
            return Err(CorePrivateRouteClientError::FatalAuthorityError);
        }
        self.location = None;
        self.scene_readiness = None;
        self.generation_advance_authorized = true;
        self.phase = CorePrivateRouteClientPhase::AwaitingAuthority;
        Ok(())
    }

    /// Consumes the existing transition model only after its location and compiled content gate
    /// reached `Ready`. This prevents Hall readiness from being reused as danger/combat readiness.
    pub fn apply_world_transition(
        &mut self,
        transition: &CoreWorldTransitionModel,
    ) -> Result<(), CorePrivateRouteClientError> {
        if transition.phase() != CoreWorldTransitionPhase::Ready {
            return Err(CorePrivateRouteClientError::InvalidLocationAuthority);
        }
        let snapshot = transition
            .current_snapshot()
            .ok_or(CorePrivateRouteClientError::InvalidLocationAuthority)?;
        self.apply_location(snapshot.clone())
    }

    pub fn apply_location(
        &mut self,
        snapshot: CharacterLocationSnapshot,
    ) -> Result<(), CorePrivateRouteClientError> {
        self.require_feature()?;
        if snapshot.validate().is_err() {
            return self.fail(
                CorePrivateRouteClientFailure::InvalidLocationAuthority,
                CorePrivateRouteClientError::InvalidLocationAuthority,
            );
        }
        if snapshot.character_id != self.character_id {
            return self.fail(
                CorePrivateRouteClientFailure::ForeignCharacter,
                CorePrivateRouteClientError::ForeignCharacter,
            );
        }
        self.location = Some(snapshot);
        self.reconcile_authority()
    }

    pub fn apply_reliable(
        &mut self,
        frame: &ReliableEventFrame,
    ) -> Result<(), CorePrivateRouteClientError> {
        self.require_feature()?;
        if frame.validate().is_err() {
            return self.fail(
                CorePrivateRouteClientFailure::InvalidProjection,
                CorePrivateRouteClientError::InvalidProjection,
            );
        }
        let ReliableEvent::CorePrivateRouteState(state) = &frame.event else {
            return Err(CorePrivateRouteClientError::UnexpectedReliableEvent);
        };
        if frame.sequence < self.last_reliable_sequence {
            return Err(CorePrivateRouteClientError::StaleReliableSequence);
        }
        if frame.sequence == self.last_reliable_sequence {
            if self.route_state.as_ref() == Some(state.as_ref()) {
                return self.reconcile_authority();
            }
            return self.fail(
                CorePrivateRouteClientFailure::StaleReliableSequence,
                CorePrivateRouteClientError::StaleReliableSequence,
            );
        }
        self.accept_route_state(state.as_ref())?;
        self.last_reliable_sequence = frame.sequence;
        self.reconcile_authority()
    }

    pub fn apply_scene_readiness(
        &mut self,
        readiness: CorePrivateSceneReadiness,
    ) -> Result<(), CorePrivateRouteClientError> {
        self.require_feature()?;
        if readiness.validate().is_err() {
            return self.reject_scene_readiness(CorePrivateRouteClientError::InvalidSceneReadiness);
        }
        if readiness.base.content_revision != self.world_flow_revision {
            return self.fail(
                CorePrivateRouteClientFailure::ContentMismatch,
                CorePrivateRouteClientError::ContentMismatch,
            );
        }
        if let Some(state) = &self.route_state
            && !readiness_matches_state(&readiness, state)
        {
            return self.reject_scene_readiness(CorePrivateRouteClientError::InvalidSceneReadiness);
        }
        self.scene_readiness = Some(readiness);
        self.reconcile_authority()
    }

    /// Clears a completed dangerous actor generation. A late event from that generation can no
    /// longer recreate control while the terminal owner resolves Hall/CharacterSelect/death.
    pub fn retire_danger_generation(&mut self) -> Result<(), CorePrivateRouteClientError> {
        self.require_feature()?;
        if self.phase == CorePrivateRouteClientPhase::FatalAuthorityError {
            return Err(CorePrivateRouteClientError::FatalAuthorityError);
        }
        if let Some(state) = self.route_state.take() {
            self.retired_generation_floor =
                self.retired_generation_floor.max(state.actor_generation);
        }
        self.location = None;
        self.scene_readiness = None;
        self.generation_advance_authorized = true;
        self.phase = CorePrivateRouteClientPhase::AwaitingAuthority;
        Ok(())
    }

    fn accept_route_state(
        &mut self,
        state: &CorePrivateRouteStateV1,
    ) -> Result<(), CorePrivateRouteClientError> {
        if state.validate().is_err() {
            return self.fail(
                CorePrivateRouteClientFailure::InvalidProjection,
                CorePrivateRouteClientError::InvalidProjection,
            );
        }
        if state.character_id != self.character_id {
            return self.fail(
                CorePrivateRouteClientFailure::ForeignCharacter,
                CorePrivateRouteClientError::ForeignCharacter,
            );
        }
        if state.content_revision != self.route_content_revision {
            return self.fail(
                CorePrivateRouteClientFailure::ContentMismatch,
                CorePrivateRouteClientError::ContentMismatch,
            );
        }
        if state.actor_generation <= self.retired_generation_floor {
            return Err(CorePrivateRouteClientError::StaleProjection);
        }
        if let Some(previous) = &self.route_state {
            if state.actor_generation < previous.actor_generation
                || (state.actor_generation == previous.actor_generation
                    && state.state_version < previous.state_version)
            {
                return Err(CorePrivateRouteClientError::StaleProjection);
            }
            if state.actor_generation == previous.actor_generation
                && state.state_version == previous.state_version
            {
                if state == previous {
                    return Ok(());
                }
                return self.fail(
                    CorePrivateRouteClientFailure::ConflictingReplay,
                    CorePrivateRouteClientError::ConflictingReplay,
                );
            }
            if state.actor_generation > previous.actor_generation {
                if !self.generation_advance_authorized {
                    return self.fail(
                        CorePrivateRouteClientFailure::GenerationAdvanceWithoutAuthority,
                        CorePrivateRouteClientError::GenerationAdvanceWithoutAuthority,
                    );
                }
                self.location = None;
                self.scene_readiness = None;
            }
        }
        self.generation_advance_authorized = false;
        if self
            .scene_readiness
            .as_ref()
            .is_some_and(|readiness| !readiness_matches_state(readiness, state))
        {
            self.scene_readiness = None;
        }
        self.route_state = Some(state.clone());
        Ok(())
    }

    fn reconcile_authority(&mut self) -> Result<(), CorePrivateRouteClientError> {
        if self.phase == CorePrivateRouteClientPhase::FatalAuthorityError {
            return Err(CorePrivateRouteClientError::FatalAuthorityError);
        }
        let (Some(location), Some(state)) = (&self.location, &self.route_state) else {
            self.phase = CorePrivateRouteClientPhase::AwaitingAuthority;
            return Ok(());
        };
        if location.character_version != state.character_version {
            self.phase = CorePrivateRouteClientPhase::AwaitingAuthority;
            return Ok(());
        }
        if !location_matches_state(location, state) {
            return self.fail(
                CorePrivateRouteClientFailure::InvalidLocationAuthority,
                CorePrivateRouteClientError::InvalidLocationAuthority,
            );
        }
        if state.phase == CorePrivateRoutePhaseV1::TerminalPending {
            self.scene_readiness = None;
            self.phase = CorePrivateRouteClientPhase::TerminalPending;
            return Ok(());
        }
        let Some(readiness) = &self.scene_readiness else {
            self.phase = CorePrivateRouteClientPhase::LoadingScene;
            return Ok(());
        };
        if !readiness_matches_state(readiness, state)
            || readiness.base.character_version != location.character_version
        {
            self.scene_readiness = None;
            self.phase = CorePrivateRouteClientPhase::LoadingScene;
            return Ok(());
        }
        self.phase = if state.readiness.accepts_gameplay_input
            == CorePrivateRouteAvailabilityV1::Available
        {
            CorePrivateRouteClientPhase::Controllable
        } else {
            CorePrivateRouteClientPhase::TerminalPending
        };
        Ok(())
    }

    fn require_feature(&self) -> Result<(), CorePrivateRouteClientError> {
        if self.feature_authorized {
            Ok(())
        } else {
            Err(CorePrivateRouteClientError::FeatureNotNegotiated)
        }
    }

    fn reject_scene_readiness<T>(
        &mut self,
        error: CorePrivateRouteClientError,
    ) -> Result<T, CorePrivateRouteClientError> {
        self.scene_readiness = None;
        if self.phase != CorePrivateRouteClientPhase::FatalAuthorityError {
            self.phase = CorePrivateRouteClientPhase::LoadingScene;
        }
        Err(error)
    }

    fn fail<T>(
        &mut self,
        failure: CorePrivateRouteClientFailure,
        error: CorePrivateRouteClientError,
    ) -> Result<T, CorePrivateRouteClientError> {
        self.location = None;
        self.scene_readiness = None;
        self.generation_advance_authorized = false;
        self.failure = Some(failure);
        self.phase = CorePrivateRouteClientPhase::FatalAuthorityError;
        Err(error)
    }
}

fn location_matches_state(
    location: &CharacterLocationSnapshot,
    state: &CorePrivateRouteStateV1,
) -> bool {
    if location.character_id != state.character_id {
        return false;
    }
    match (&location.location, state.scene) {
        (CharacterLocation::Safe { location_id, .. }, CorePrivateRouteSceneV1::LanternHalls) => {
            location_id.as_str() == state.scene.location_id() && state.instance_lineage_id.is_none()
        }
        (
            CharacterLocation::Danger {
                location_id,
                instance_lineage_id,
                ..
            },
            CorePrivateRouteSceneV1::CoreMicrorealm | CorePrivateRouteSceneV1::BellSepulcher,
        ) => {
            location_id.as_str() == state.scene.location_id()
                && state.instance_lineage_id == Some(*instance_lineage_id)
        }
        _ => false,
    }
}

fn readiness_matches_state(
    readiness: &CorePrivateSceneReadiness,
    state: &CorePrivateRouteStateV1,
) -> bool {
    readiness.base.location_id.as_str() == state.scene.location_id()
        && readiness.base.character_version == state.character_version
        && readiness.scene == state.scene
        && readiness.room == state.room
        && readiness.instance_lineage_id == state.instance_lineage_id
        && readiness.actor_generation == state.actor_generation
        && readiness.route_state_version == state.state_version
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CorePrivateRouteClientError {
    #[error("normal Core route was not negotiated")]
    FeatureNotNegotiated,
    #[error("normal Core route server hello is malformed or version-incompatible")]
    InvalidServerHello,
    #[error("normal Core route projection is malformed")]
    InvalidProjection,
    #[error("normal Core route authority belongs to another character")]
    ForeignCharacter,
    #[error("normal Core route content authority does not match compiled content")]
    ContentMismatch,
    #[error("normal Core route reliable sequence is stale or conflicting")]
    StaleReliableSequence,
    #[error("normal Core route actor/state version is stale")]
    StaleProjection,
    #[error("normal Core route changed payload at an existing generation/state version")]
    ConflictingReplay,
    #[error("normal Core route actor generation advanced without transfer/reconnect authority")]
    GenerationAdvanceWithoutAuthority,
    #[error("normal Core route durable location contradicts the route projection")]
    InvalidLocationAuthority,
    #[error("normal Core route compiled scene readiness is stale or mismatched")]
    InvalidSceneReadiness,
    #[error("reliable event is not a Core private-route projection")]
    UnexpectedReliableEvent,
    #[error("normal Core route entered a fatal authority state")]
    FatalAuthorityError,
}

#[cfg(test)]
mod tests {
    use protocol::{
        CORE_PRIVATE_ROUTE_SCHEMA_VERSION, M03_CORE_DEV_BUILD_ID, ManifestHash, ProtocolVersion,
        SIMULATION_HZ, SNAPSHOT_HZ, SafeArrival, WireText,
    };

    use super::*;

    const CHARACTER_ID: [u8; CHARACTER_ID_BYTES] = [1; CHARACTER_ID_BYTES];
    const LINEAGE_ID: [u8; protocol::INSTANCE_LINEAGE_ID_BYTES] =
        [2; protocol::INSTANCE_LINEAGE_ID_BYTES];

    fn world_revision() -> WorldFlowContentRevisionV1 {
        WorldFlowContentRevisionV1 {
            records_blake3: ManifestHash::new("1".repeat(64)).unwrap(),
            assets_blake3: ManifestHash::new("2".repeat(64)).unwrap(),
            localization_blake3: ManifestHash::new("3".repeat(64)).unwrap(),
        }
    }

    fn route_revision() -> CorePrivateRouteContentRevisionV1 {
        CorePrivateRouteContentRevisionV1 {
            records_blake3: ManifestHash::new("4".repeat(64)).unwrap(),
            assets_blake3: ManifestHash::new("5".repeat(64)).unwrap(),
            localization_blake3: ManifestHash::new("6".repeat(64)).unwrap(),
        }
    }

    fn hello(enabled: bool) -> ServerHello {
        let version = ProtocolVersion::current();
        ServerHello {
            session_id: WireText::new("core-private-route-client").unwrap(),
            protocol_major: version.major,
            protocol_minor: version.minor,
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

    fn model() -> CorePrivateRouteClientModel {
        CorePrivateRouteClientModel::new(CHARACTER_ID, world_revision(), route_revision()).unwrap()
    }

    fn state(
        generation: u64,
        version: u64,
        character_version: u64,
        scene: CorePrivateRouteSceneV1,
        room: Option<CorePrivateRouteRoomV1>,
        phase: CorePrivateRoutePhaseV1,
    ) -> CorePrivateRouteStateV1 {
        CorePrivateRouteStateV1 {
            schema_version: CORE_PRIVATE_ROUTE_SCHEMA_VERSION,
            character_id: CHARACTER_ID,
            character_version,
            content_revision: route_revision(),
            actor_generation: generation,
            state_version: version,
            instance_lineage_id: (scene != CorePrivateRouteSceneV1::LanternHalls)
                .then_some(LINEAGE_ID),
            scene,
            room,
            phase,
            readiness: protocol::CorePrivateRouteReadinessV1::canonical(phase),
        }
    }

    fn frame(sequence: u32, state: CorePrivateRouteStateV1) -> ReliableEventFrame {
        ReliableEventFrame {
            sequence,
            server_tick: u64::from(sequence),
            event: ReliableEvent::CorePrivateRouteState(Box::new(state)),
        }
    }

    fn danger_location(character_version: u64, location_id: &str) -> CharacterLocationSnapshot {
        CharacterLocationSnapshot {
            character_id: CHARACTER_ID,
            character_version,
            location: CharacterLocation::Danger {
                location_id: WireText::new(location_id).unwrap(),
                instance_lineage_id: LINEAGE_ID,
                entry_restore_point_id: [3; protocol::TRANSFER_ID_BYTES],
            },
        }
    }

    fn hall_location(character_version: u64) -> CharacterLocationSnapshot {
        CharacterLocationSnapshot {
            character_id: CHARACTER_ID,
            character_version,
            location: CharacterLocation::Safe {
                location_id: WireText::new("hub.lantern_halls_01").unwrap(),
                arrival: SafeArrival::HallDefault,
            },
        }
    }

    fn readiness(state: &CorePrivateRouteStateV1) -> CorePrivateSceneReadiness {
        CorePrivateSceneReadiness {
            base: CoreSceneReadiness {
                location_id: WireText::new(state.scene.location_id()).unwrap(),
                character_version: state.character_version,
                content_revision: world_revision(),
            },
            scene: state.scene,
            room: state.room,
            instance_lineage_id: state.instance_lineage_id,
            actor_generation: state.actor_generation,
            route_state_version: state.state_version,
        }
    }

    #[test]
    fn absent_feature_keeps_route_disabled_and_rejects_projection() {
        let mut model = model();
        assert_eq!(model.accept_server_hello(&hello(false)), Ok(false));
        assert_eq!(model.phase(), CorePrivateRouteClientPhase::Disabled);
        assert_eq!(
            model.apply_location(hall_location(1)),
            Err(CorePrivateRouteClientError::FeatureNotNegotiated)
        );
    }

    #[test]
    fn either_authority_arrival_order_waits_for_exact_room_readiness() {
        let route = state(
            1,
            1,
            2,
            CorePrivateRouteSceneV1::BellSepulcher,
            Some(CorePrivateRouteRoomV1::BellCrossB1),
            CorePrivateRoutePhaseV1::RoomActive,
        );
        let mut event_first = model();
        event_first.accept_server_hello(&hello(true)).unwrap();
        event_first
            .apply_reliable(&frame(1, route.clone()))
            .unwrap();
        assert_eq!(
            event_first.phase(),
            CorePrivateRouteClientPhase::AwaitingAuthority
        );
        event_first
            .apply_location(danger_location(2, "dungeon.bell_sepulcher"))
            .unwrap();
        assert_eq!(
            event_first.phase(),
            CorePrivateRouteClientPhase::LoadingScene
        );
        event_first
            .apply_scene_readiness(readiness(&route))
            .unwrap();
        assert!(event_first.can_accept_gameplay_input());

        let mut location_first = model();
        location_first.accept_server_hello(&hello(true)).unwrap();
        location_first
            .apply_location(danger_location(2, "dungeon.bell_sepulcher"))
            .unwrap();
        location_first
            .apply_reliable(&frame(1, route.clone()))
            .unwrap();
        assert_eq!(
            location_first.phase(),
            CorePrivateRouteClientPhase::LoadingScene
        );
        location_first
            .apply_scene_readiness(readiness(&route))
            .unwrap();
        assert!(location_first.can_accept_gameplay_input());
    }

    #[test]
    fn hall_readiness_cannot_enable_a_dungeon_or_wrong_room() {
        let route = state(
            1,
            1,
            2,
            CorePrivateRouteSceneV1::BellSepulcher,
            Some(CorePrivateRouteRoomV1::BellNaveB2),
            CorePrivateRoutePhaseV1::RoomSpawnWarning,
        );
        let mut model = model();
        model.accept_server_hello(&hello(true)).unwrap();
        model.apply_reliable(&frame(1, route.clone())).unwrap();
        model
            .apply_location(danger_location(2, "dungeon.bell_sepulcher"))
            .unwrap();

        let hall = state(
            1,
            1,
            2,
            CorePrivateRouteSceneV1::LanternHalls,
            None,
            CorePrivateRoutePhaseV1::Hall,
        );
        assert_eq!(
            model.apply_scene_readiness(readiness(&hall)),
            Err(CorePrivateRouteClientError::InvalidSceneReadiness)
        );
        let mut wrong_room = readiness(&route);
        wrong_room.room = Some(CorePrivateRouteRoomV1::BellCrossB1);
        assert_eq!(
            model.apply_scene_readiness(wrong_room),
            Err(CorePrivateRouteClientError::InvalidSceneReadiness)
        );
        assert_eq!(model.phase(), CorePrivateRouteClientPhase::LoadingScene);
    }

    #[test]
    fn exact_replay_is_idempotent_but_changed_same_version_is_fatal() {
        let route = state(
            1,
            1,
            2,
            CorePrivateRouteSceneV1::CoreMicrorealm,
            None,
            CorePrivateRoutePhaseV1::MicrorealmActive,
        );
        let mut model = model();
        model.accept_server_hello(&hello(true)).unwrap();
        model.apply_reliable(&frame(1, route.clone())).unwrap();
        model.apply_reliable(&frame(1, route.clone())).unwrap();

        let mut changed = route;
        changed.phase = CorePrivateRoutePhaseV1::MicrorealmCleared;
        changed.readiness = protocol::CorePrivateRouteReadinessV1::canonical(changed.phase);
        assert_eq!(
            model.apply_reliable(&frame(2, changed)),
            Err(CorePrivateRouteClientError::ConflictingReplay)
        );
        assert_eq!(
            model.phase(),
            CorePrivateRouteClientPhase::FatalAuthorityError
        );
    }

    #[test]
    fn actor_generation_advance_requires_transfer_or_reconnect_authority() {
        let first = state(
            1,
            1,
            2,
            CorePrivateRouteSceneV1::CoreMicrorealm,
            None,
            CorePrivateRoutePhaseV1::MicrorealmDormant,
        );
        let next = state(
            2,
            1,
            3,
            CorePrivateRouteSceneV1::BellSepulcher,
            Some(CorePrivateRouteRoomV1::BellVestibuleB0),
            CorePrivateRoutePhaseV1::DungeonVestibule,
        );
        let mut rejected = model();
        rejected.accept_server_hello(&hello(true)).unwrap();
        rejected.apply_reliable(&frame(1, first.clone())).unwrap();
        assert_eq!(
            rejected.apply_reliable(&frame(2, next.clone())),
            Err(CorePrivateRouteClientError::GenerationAdvanceWithoutAuthority)
        );

        let mut accepted = model();
        accepted.accept_server_hello(&hello(true)).unwrap();
        accepted.apply_reliable(&frame(1, first)).unwrap();
        accepted.begin_committed_transfer_refresh().unwrap();
        accepted.apply_reliable(&frame(2, next)).unwrap();
        assert_eq!(
            accepted.phase(),
            CorePrivateRouteClientPhase::AwaitingAuthority
        );
    }

    #[test]
    fn link_loss_terminal_pending_and_retirement_never_leave_stale_control() {
        let active = state(
            1,
            1,
            2,
            CorePrivateRouteSceneV1::CoreMicrorealm,
            None,
            CorePrivateRoutePhaseV1::MicrorealmActive,
        );
        let mut model = model();
        model.accept_server_hello(&hello(true)).unwrap();
        model.apply_reliable(&frame(1, active.clone())).unwrap();
        model
            .apply_location(danger_location(2, "world.core_microrealm_01"))
            .unwrap();
        model.apply_scene_readiness(readiness(&active)).unwrap();
        assert!(model.can_accept_gameplay_input());
        model.transport_lost();
        assert!(!model.can_accept_gameplay_input());

        model.accept_server_hello(&hello(true)).unwrap();
        let terminal = state(
            1,
            2,
            2,
            CorePrivateRouteSceneV1::CoreMicrorealm,
            None,
            CorePrivateRoutePhaseV1::TerminalPending,
        );
        model.apply_reliable(&frame(1, terminal)).unwrap();
        model
            .apply_location(danger_location(2, "world.core_microrealm_01"))
            .unwrap();
        assert_eq!(model.phase(), CorePrivateRouteClientPhase::TerminalPending);

        model.retire_danger_generation().unwrap();
        assert_eq!(
            model.apply_reliable(&frame(2, active)),
            Err(CorePrivateRouteClientError::StaleProjection)
        );
        assert!(!model.can_accept_gameplay_input());
    }

    #[test]
    fn version_skew_buffers_but_same_version_location_contradiction_is_fatal() {
        let route = state(
            1,
            2,
            3,
            CorePrivateRouteSceneV1::LanternHalls,
            None,
            CorePrivateRoutePhaseV1::Hall,
        );
        let mut model = model();
        model.accept_server_hello(&hello(true)).unwrap();
        model.apply_reliable(&frame(1, route)).unwrap();
        model.apply_location(hall_location(2)).unwrap();
        assert_eq!(
            model.phase(),
            CorePrivateRouteClientPhase::AwaitingAuthority
        );

        let contradiction = danger_location(3, "world.core_microrealm_01");
        assert_eq!(
            model.apply_location(contradiction),
            Err(CorePrivateRouteClientError::InvalidLocationAuthority)
        );
        assert_eq!(
            model.failure(),
            Some(CorePrivateRouteClientFailure::InvalidLocationAuthority)
        );
    }

    #[test]
    fn malformed_authority_and_content_drift_revoke_existing_control() {
        let active = state(
            1,
            1,
            2,
            CorePrivateRouteSceneV1::CoreMicrorealm,
            None,
            CorePrivateRoutePhaseV1::MicrorealmActive,
        );
        let mut malformed = model();
        malformed.accept_server_hello(&hello(true)).unwrap();
        malformed.apply_reliable(&frame(1, active.clone())).unwrap();
        malformed
            .apply_location(danger_location(2, "world.core_microrealm_01"))
            .unwrap();
        malformed.apply_scene_readiness(readiness(&active)).unwrap();
        assert!(malformed.can_accept_gameplay_input());

        let mut invalid = active.clone();
        invalid.state_version = 2;
        invalid.readiness.bell_portal_available = CorePrivateRouteAvailabilityV1::Available;
        assert_eq!(
            malformed.apply_reliable(&frame(2, invalid)),
            Err(CorePrivateRouteClientError::InvalidProjection)
        );
        assert_eq!(
            malformed.phase(),
            CorePrivateRouteClientPhase::FatalAuthorityError
        );
        assert!(!malformed.can_accept_gameplay_input());

        let mut drift = model();
        drift.accept_server_hello(&hello(true)).unwrap();
        drift.apply_reliable(&frame(1, active.clone())).unwrap();
        drift
            .apply_location(danger_location(2, "world.core_microrealm_01"))
            .unwrap();
        let mut wrong_content = readiness(&active);
        wrong_content.base.content_revision.records_blake3 =
            ManifestHash::new("9".repeat(64)).unwrap();
        assert_eq!(
            drift.apply_scene_readiness(wrong_content),
            Err(CorePrivateRouteClientError::ContentMismatch)
        );
        assert_eq!(
            drift.phase(),
            CorePrivateRouteClientPhase::FatalAuthorityError
        );
    }
}
