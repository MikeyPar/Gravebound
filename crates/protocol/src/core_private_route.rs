//! Append-only protocol 1.18 projection for the ordinary M03 private-life route.
//!
//! The projection is server-authored and read-only. It deliberately uses closed enums for the
//! one Core scene graph instead of accepting client-authored content IDs or destinations.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{CHARACTER_ID_BYTES, INSTANCE_LINEAGE_ID_BYTES, ManifestHash};

pub const CORE_PRIVATE_ROUTE_SCHEMA_VERSION: u16 = 1;

/// Domain-separated aggregate identity for every compiled input owned by the normal Core route.
///
/// The producer hashes the ordered world-flow, Hall/micro-realm, fixed Bell layout/encounters,
/// and Sir Caldus records into these three manifests. Reusing the narrower world-flow revision
/// would allow room or boss content to drift without invalidating the projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorePrivateRouteContentRevisionV1 {
    pub records_blake3: ManifestHash,
    pub assets_blake3: ManifestHash,
    pub localization_blake3: ManifestHash,
}

impl CorePrivateRouteContentRevisionV1 {
    pub fn validate(&self) -> Result<(), CorePrivateRouteValidationError> {
        if [
            &self.records_blake3,
            &self.assets_blake3,
            &self.localization_blake3,
        ]
        .into_iter()
        .any(|hash| hash.as_str().bytes().all(|byte| byte == b'0'))
        {
            return Err(CorePrivateRouteValidationError::ZeroContentRevision);
        }
        Ok(())
    }
}

/// Exact player-facing scenes admitted by the M03 private-life route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorePrivateRouteSceneV1 {
    LanternHalls,
    CoreMicrorealm,
    BellSepulcher,
}

impl CorePrivateRouteSceneV1 {
    #[must_use]
    pub const fn location_id(self) -> &'static str {
        match self {
            Self::LanternHalls => "hub.lantern_halls_01",
            Self::CoreMicrorealm => "world.core_microrealm_01",
            Self::BellSepulcher => "dungeon.bell_sepulcher",
        }
    }
}

/// Fixed `CONT-ROOM-007` node identities. BB1, BS1, and seeded nodes cannot be represented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorePrivateRouteRoomV1 {
    BellVestibuleB0,
    BellCrossB1,
    BellNaveB2,
    BellKnightB3,
    BellRestB4,
    BellBridgeB5,
    CaldusArenaB6,
}

impl CorePrivateRouteRoomV1 {
    #[must_use]
    pub const fn node_id(self) -> &'static str {
        match self {
            Self::BellVestibuleB0 => "B0",
            Self::BellCrossB1 => "B1",
            Self::BellNaveB2 => "B2",
            Self::BellKnightB3 => "B3",
            Self::BellRestB4 => "B4",
            Self::BellBridgeB5 => "B5",
            Self::CaldusArenaB6 => "B6",
        }
    }

    const fn is_combat_room(self) -> bool {
        matches!(
            self,
            Self::BellCrossB1 | Self::BellNaveB2 | Self::BellKnightB3 | Self::BellBridgeB5
        )
    }
}

/// Bounded authoritative phases spanning the exact Hall, micro-realm, fixed rooms, and Caldus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorePrivateRoutePhaseV1 {
    Hall,
    MicrorealmDormant,
    MicrorealmWaiting,
    MicrorealmActive,
    MicrorealmCleared,
    DungeonVestibule,
    RoomDormant,
    RoomAwaitingDoorSafety,
    RoomSpawnWarning,
    RoomActive,
    RoomQuiet,
    RoomCleared,
    Rest,
    BossStaging,
    BossReadyCountdown,
    BossIntroduction,
    BossPhaseOne,
    BossBreakToTwo,
    BossPhaseTwo,
    BossBreakToThree,
    BossPhaseThree,
    BossDefeated,
    BossExitReady,
    TerminalPending,
}

impl CorePrivateRoutePhaseV1 {
    const fn is_microrealm(self) -> bool {
        matches!(
            self,
            Self::MicrorealmDormant
                | Self::MicrorealmWaiting
                | Self::MicrorealmActive
                | Self::MicrorealmCleared
        )
    }

    const fn is_room(self) -> bool {
        matches!(
            self,
            Self::RoomDormant
                | Self::RoomAwaitingDoorSafety
                | Self::RoomSpawnWarning
                | Self::RoomActive
                | Self::RoomQuiet
                | Self::RoomCleared
        )
    }

    const fn is_boss(self) -> bool {
        matches!(
            self,
            Self::BossStaging
                | Self::BossReadyCountdown
                | Self::BossIntroduction
                | Self::BossPhaseOne
                | Self::BossBreakToTwo
                | Self::BossPhaseTwo
                | Self::BossBreakToThree
                | Self::BossPhaseThree
                | Self::BossDefeated
                | Self::BossExitReady
        )
    }
}

/// Server-owned interaction readiness. Callers construct this through [`Self::canonical`] so
/// phase and readiness cannot drift silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorePrivateRouteAvailabilityV1 {
    Unavailable,
    Available,
}

impl CorePrivateRouteAvailabilityV1 {
    #[must_use]
    pub const fn from_bool(available: bool) -> Self {
        if available {
            Self::Available
        } else {
            Self::Unavailable
        }
    }

    #[must_use]
    pub const fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorePrivateRouteReadinessV1 {
    pub accepts_gameplay_input: CorePrivateRouteAvailabilityV1,
    pub microrealm_cleared: CorePrivateRouteAvailabilityV1,
    pub bell_portal_available: CorePrivateRouteAvailabilityV1,
    pub room_exit_available: CorePrivateRouteAvailabilityV1,
    pub boss_encounter_ready: CorePrivateRouteAvailabilityV1,
    pub extraction_available: CorePrivateRouteAvailabilityV1,
}

impl CorePrivateRouteReadinessV1 {
    #[must_use]
    pub const fn canonical(phase: CorePrivateRoutePhaseV1) -> Self {
        let actions_available = !matches!(phase, CorePrivateRoutePhaseV1::TerminalPending);
        Self {
            accepts_gameplay_input: CorePrivateRouteAvailabilityV1::from_bool(actions_available),
            microrealm_cleared: CorePrivateRouteAvailabilityV1::from_bool(matches!(
                phase,
                CorePrivateRoutePhaseV1::MicrorealmCleared
            )),
            bell_portal_available: CorePrivateRouteAvailabilityV1::from_bool(
                actions_available && matches!(phase, CorePrivateRoutePhaseV1::MicrorealmCleared),
            ),
            room_exit_available: CorePrivateRouteAvailabilityV1::from_bool(
                actions_available
                    && matches!(
                        phase,
                        CorePrivateRoutePhaseV1::DungeonVestibule
                            | CorePrivateRoutePhaseV1::RoomCleared
                            | CorePrivateRoutePhaseV1::Rest
                    ),
            ),
            boss_encounter_ready: CorePrivateRouteAvailabilityV1::from_bool(
                actions_available
                    && matches!(
                        phase,
                        CorePrivateRoutePhaseV1::BossIntroduction
                            | CorePrivateRoutePhaseV1::BossPhaseOne
                            | CorePrivateRoutePhaseV1::BossBreakToTwo
                            | CorePrivateRoutePhaseV1::BossPhaseTwo
                            | CorePrivateRoutePhaseV1::BossBreakToThree
                            | CorePrivateRoutePhaseV1::BossPhaseThree
                            | CorePrivateRoutePhaseV1::BossDefeated
                            | CorePrivateRoutePhaseV1::BossExitReady
                    ),
            ),
            extraction_available: CorePrivateRouteAvailabilityV1::from_bool(
                actions_available && matches!(phase, CorePrivateRoutePhaseV1::BossExitReady),
            ),
        }
    }
}

/// Latest authoritative private-route projection for one selected character.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorePrivateRouteStateV1 {
    pub schema_version: u16,
    pub character_id: [u8; CHARACTER_ID_BYTES],
    pub character_version: u64,
    pub content_revision: CorePrivateRouteContentRevisionV1,
    /// Monotonic owner generation. Actor replacement must advance this before publication.
    pub actor_generation: u64,
    /// Monotonic within one actor generation. Equal versions must be byte-identical replays.
    pub state_version: u64,
    /// Present only in dangerous scenes and stable from micro-realm entry through extraction.
    pub instance_lineage_id: Option<[u8; INSTANCE_LINEAGE_ID_BYTES]>,
    pub scene: CorePrivateRouteSceneV1,
    pub room: Option<CorePrivateRouteRoomV1>,
    pub phase: CorePrivateRoutePhaseV1,
    pub readiness: CorePrivateRouteReadinessV1,
}

impl CorePrivateRouteStateV1 {
    #[must_use]
    pub const fn required_feature_flag() -> &'static str {
        crate::CORE_WORLD_FLOW_FEATURE_FLAG
    }

    pub fn validate(&self) -> Result<(), CorePrivateRouteValidationError> {
        if self.schema_version != CORE_PRIVATE_ROUTE_SCHEMA_VERSION {
            return Err(CorePrivateRouteValidationError::SchemaVersion);
        }
        if all_zero(&self.character_id) {
            return Err(CorePrivateRouteValidationError::ZeroCharacterId);
        }
        if self.character_version == 0 {
            return Err(CorePrivateRouteValidationError::ZeroCharacterVersion);
        }
        if self.actor_generation == 0 {
            return Err(CorePrivateRouteValidationError::ZeroActorGeneration);
        }
        if self.state_version == 0 {
            return Err(CorePrivateRouteValidationError::ZeroStateVersion);
        }
        if self
            .instance_lineage_id
            .is_some_and(|lineage| all_zero(&lineage))
        {
            return Err(CorePrivateRouteValidationError::InvalidLineage);
        }
        self.content_revision.validate()?;
        validate_scene_shape(self)?;
        if self.readiness != CorePrivateRouteReadinessV1::canonical(self.phase) {
            return Err(CorePrivateRouteValidationError::ReadinessMismatch);
        }
        Ok(())
    }
}

fn validate_scene_shape(
    state: &CorePrivateRouteStateV1,
) -> Result<(), CorePrivateRouteValidationError> {
    match state.scene {
        CorePrivateRouteSceneV1::LanternHalls => {
            if state.instance_lineage_id.is_some()
                || state.room.is_some()
                || state.phase != CorePrivateRoutePhaseV1::Hall
            {
                return Err(CorePrivateRouteValidationError::ScenePhaseMismatch);
            }
        }
        CorePrivateRouteSceneV1::CoreMicrorealm => {
            if state.instance_lineage_id.is_none()
                || state.room.is_some()
                || (!state.phase.is_microrealm()
                    && state.phase != CorePrivateRoutePhaseV1::TerminalPending)
            {
                return Err(CorePrivateRouteValidationError::ScenePhaseMismatch);
            }
        }
        CorePrivateRouteSceneV1::BellSepulcher => {
            if state.instance_lineage_id.is_none() {
                return Err(CorePrivateRouteValidationError::ScenePhaseMismatch);
            }
            let room = state
                .room
                .ok_or(CorePrivateRouteValidationError::RoomPhaseMismatch)?;
            let matches = if state.phase == CorePrivateRoutePhaseV1::TerminalPending {
                true
            } else {
                match room {
                    CorePrivateRouteRoomV1::BellVestibuleB0 => {
                        state.phase == CorePrivateRoutePhaseV1::DungeonVestibule
                    }
                    CorePrivateRouteRoomV1::BellRestB4 => {
                        state.phase == CorePrivateRoutePhaseV1::Rest
                    }
                    CorePrivateRouteRoomV1::CaldusArenaB6 => state.phase.is_boss(),
                    room if room.is_combat_room() => state.phase.is_room(),
                    _ => false,
                }
            };
            if !matches {
                return Err(CorePrivateRouteValidationError::RoomPhaseMismatch);
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CorePrivateRouteValidationError {
    #[error("private-route schema version is unsupported")]
    SchemaVersion,
    #[error("private-route character ID must be nonzero")]
    ZeroCharacterId,
    #[error("private-route character version must be nonzero")]
    ZeroCharacterVersion,
    #[error("private-route composed content revision must be nonzero")]
    ZeroContentRevision,
    #[error("private-route actor generation must be nonzero")]
    ZeroActorGeneration,
    #[error("private-route state version must be nonzero")]
    ZeroStateVersion,
    #[error("private-route danger lineage must be nonzero")]
    InvalidLineage,
    #[error("private-route scene, lineage, room, and phase disagree")]
    ScenePhaseMismatch,
    #[error("private-route fixed room and phase disagree")]
    RoomPhaseMismatch,
    #[error("private-route phase and interaction readiness disagree")]
    ReadinessMismatch,
}

fn all_zero<const N: usize>(bytes: &[u8; N]) -> bool {
    bytes.iter().all(|byte| *byte == 0)
}

#[cfg(test)]
mod tests {
    use crate::ManifestHash;

    use super::*;

    fn revision() -> CorePrivateRouteContentRevisionV1 {
        CorePrivateRouteContentRevisionV1 {
            records_blake3: ManifestHash::new("1".repeat(64)).unwrap(),
            assets_blake3: ManifestHash::new("2".repeat(64)).unwrap(),
            localization_blake3: ManifestHash::new("3".repeat(64)).unwrap(),
        }
    }

    fn state(
        scene: CorePrivateRouteSceneV1,
        room: Option<CorePrivateRouteRoomV1>,
        phase: CorePrivateRoutePhaseV1,
    ) -> CorePrivateRouteStateV1 {
        CorePrivateRouteStateV1 {
            schema_version: CORE_PRIVATE_ROUTE_SCHEMA_VERSION,
            character_id: [1; CHARACTER_ID_BYTES],
            character_version: 1,
            content_revision: revision(),
            actor_generation: 1,
            state_version: 1,
            instance_lineage_id: (scene != CorePrivateRouteSceneV1::LanternHalls)
                .then_some([2; INSTANCE_LINEAGE_ID_BYTES]),
            scene,
            room,
            phase,
            readiness: CorePrivateRouteReadinessV1::canonical(phase),
        }
    }

    #[test]
    fn exact_core_route_shapes_validate() {
        let fixtures = [
            state(
                CorePrivateRouteSceneV1::LanternHalls,
                None,
                CorePrivateRoutePhaseV1::Hall,
            ),
            state(
                CorePrivateRouteSceneV1::CoreMicrorealm,
                None,
                CorePrivateRoutePhaseV1::MicrorealmCleared,
            ),
            state(
                CorePrivateRouteSceneV1::BellSepulcher,
                Some(CorePrivateRouteRoomV1::BellVestibuleB0),
                CorePrivateRoutePhaseV1::DungeonVestibule,
            ),
            state(
                CorePrivateRouteSceneV1::BellSepulcher,
                Some(CorePrivateRouteRoomV1::BellCrossB1),
                CorePrivateRoutePhaseV1::RoomActive,
            ),
            state(
                CorePrivateRouteSceneV1::BellSepulcher,
                Some(CorePrivateRouteRoomV1::BellRestB4),
                CorePrivateRoutePhaseV1::Rest,
            ),
            state(
                CorePrivateRouteSceneV1::BellSepulcher,
                Some(CorePrivateRouteRoomV1::CaldusArenaB6),
                CorePrivateRoutePhaseV1::BossPhaseThree,
            ),
        ];
        for fixture in fixtures {
            assert_eq!(fixture.validate(), Ok(()));
        }
    }

    #[test]
    fn readiness_is_derived_from_phase_and_terminal_authority() {
        let cleared =
            CorePrivateRouteReadinessV1::canonical(CorePrivateRoutePhaseV1::MicrorealmCleared);
        assert!(cleared.accepts_gameplay_input.is_available());
        assert!(cleared.microrealm_cleared.is_available());
        assert!(cleared.bell_portal_available.is_available());
        assert!(!cleared.extraction_available.is_available());

        let defeated =
            CorePrivateRouteReadinessV1::canonical(CorePrivateRoutePhaseV1::BossDefeated);
        assert!(defeated.boss_encounter_ready.is_available());
        assert!(!defeated.extraction_available.is_available());
        let boss = CorePrivateRouteReadinessV1::canonical(CorePrivateRoutePhaseV1::BossExitReady);
        assert!(boss.boss_encounter_ready.is_available());
        assert!(boss.extraction_available.is_available());

        let terminal =
            CorePrivateRouteReadinessV1::canonical(CorePrivateRoutePhaseV1::TerminalPending);
        assert!(!terminal.accepts_gameplay_input.is_available());
        assert!(!terminal.microrealm_cleared.is_available());
        assert!(!terminal.boss_encounter_ready.is_available());
        assert!(!terminal.extraction_available.is_available());
    }

    #[test]
    fn impossible_scene_room_and_readiness_combinations_fail_closed() {
        let mut hall = state(
            CorePrivateRouteSceneV1::LanternHalls,
            None,
            CorePrivateRoutePhaseV1::Hall,
        );
        hall.instance_lineage_id = Some([2; INSTANCE_LINEAGE_ID_BYTES]);
        assert_eq!(
            hall.validate(),
            Err(CorePrivateRouteValidationError::ScenePhaseMismatch)
        );

        let mut branch = state(
            CorePrivateRouteSceneV1::BellSepulcher,
            Some(CorePrivateRouteRoomV1::BellCrossB1),
            CorePrivateRoutePhaseV1::RoomActive,
        );
        branch.room = Some(CorePrivateRouteRoomV1::CaldusArenaB6);
        assert_eq!(
            branch.validate(),
            Err(CorePrivateRouteValidationError::RoomPhaseMismatch)
        );

        let mut forged = state(
            CorePrivateRouteSceneV1::CoreMicrorealm,
            None,
            CorePrivateRoutePhaseV1::MicrorealmActive,
        );
        forged.readiness.bell_portal_available = CorePrivateRouteAvailabilityV1::Available;
        assert_eq!(
            forged.validate(),
            Err(CorePrivateRouteValidationError::ReadinessMismatch)
        );
    }

    #[test]
    fn zero_authority_and_unknown_schema_fail_closed() {
        let mut projection = state(
            CorePrivateRouteSceneV1::CoreMicrorealm,
            None,
            CorePrivateRoutePhaseV1::MicrorealmDormant,
        );
        projection.actor_generation = 0;
        assert_eq!(
            projection.validate(),
            Err(CorePrivateRouteValidationError::ZeroActorGeneration)
        );
        projection.actor_generation = 1;
        projection.character_version = 0;
        assert_eq!(
            projection.validate(),
            Err(CorePrivateRouteValidationError::ZeroCharacterVersion)
        );
        projection.character_version = 1;
        projection.schema_version = 2;
        assert_eq!(
            projection.validate(),
            Err(CorePrivateRouteValidationError::SchemaVersion)
        );
    }

    #[test]
    fn closed_route_enum_discriminants_are_pinned_append_only() {
        let scenes = [
            CorePrivateRouteSceneV1::LanternHalls,
            CorePrivateRouteSceneV1::CoreMicrorealm,
            CorePrivateRouteSceneV1::BellSepulcher,
        ];
        for (index, scene) in scenes.into_iter().enumerate() {
            assert_eq!(
                postcard::to_stdvec(&scene).unwrap(),
                vec![u8::try_from(index).unwrap()]
            );
        }

        let rooms = [
            CorePrivateRouteRoomV1::BellVestibuleB0,
            CorePrivateRouteRoomV1::BellCrossB1,
            CorePrivateRouteRoomV1::BellNaveB2,
            CorePrivateRouteRoomV1::BellKnightB3,
            CorePrivateRouteRoomV1::BellRestB4,
            CorePrivateRouteRoomV1::BellBridgeB5,
            CorePrivateRouteRoomV1::CaldusArenaB6,
        ];
        for (index, room) in rooms.into_iter().enumerate() {
            assert_eq!(
                postcard::to_stdvec(&room).unwrap(),
                vec![u8::try_from(index).unwrap()]
            );
        }

        let phases = [
            CorePrivateRoutePhaseV1::Hall,
            CorePrivateRoutePhaseV1::MicrorealmDormant,
            CorePrivateRoutePhaseV1::MicrorealmWaiting,
            CorePrivateRoutePhaseV1::MicrorealmActive,
            CorePrivateRoutePhaseV1::MicrorealmCleared,
            CorePrivateRoutePhaseV1::DungeonVestibule,
            CorePrivateRoutePhaseV1::RoomDormant,
            CorePrivateRoutePhaseV1::RoomAwaitingDoorSafety,
            CorePrivateRoutePhaseV1::RoomSpawnWarning,
            CorePrivateRoutePhaseV1::RoomActive,
            CorePrivateRoutePhaseV1::RoomQuiet,
            CorePrivateRoutePhaseV1::RoomCleared,
            CorePrivateRoutePhaseV1::Rest,
            CorePrivateRoutePhaseV1::BossStaging,
            CorePrivateRoutePhaseV1::BossReadyCountdown,
            CorePrivateRoutePhaseV1::BossIntroduction,
            CorePrivateRoutePhaseV1::BossPhaseOne,
            CorePrivateRoutePhaseV1::BossBreakToTwo,
            CorePrivateRoutePhaseV1::BossPhaseTwo,
            CorePrivateRoutePhaseV1::BossBreakToThree,
            CorePrivateRoutePhaseV1::BossPhaseThree,
            CorePrivateRoutePhaseV1::BossDefeated,
            CorePrivateRoutePhaseV1::BossExitReady,
            CorePrivateRoutePhaseV1::TerminalPending,
        ];
        for (index, phase) in phases.into_iter().enumerate() {
            assert_eq!(
                postcard::to_stdvec(&phase).unwrap(),
                vec![u8::try_from(index).unwrap()]
            );
        }

        assert_eq!(
            postcard::to_stdvec(&CorePrivateRouteAvailabilityV1::Unavailable).unwrap(),
            vec![0]
        );
        assert_eq!(
            postcard::to_stdvec(&CorePrivateRouteAvailabilityV1::Available).unwrap(),
            vec![1]
        );
    }
}
