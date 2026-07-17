use protocol::{
    CORE_PRIVATE_ROUTE_SCHEMA_VERSION, CorePrivateRouteContentRevisionV1, CorePrivateRoutePhaseV1,
    CorePrivateRouteReadinessV1, CorePrivateRouteRoomV1, CorePrivateRouteSceneV1,
    CorePrivateRouteStateV1, WorldFlowContentRevisionV1,
};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateRouteActorPosition {
    pub instance_lineage_id: Option<[u8; 16]>,
    pub scene: CorePrivateRouteSceneV1,
    pub room: Option<CorePrivateRouteRoomV1>,
    pub phase: CorePrivateRoutePhaseV1,
}

impl CorePrivateRouteActorPosition {
    #[must_use]
    pub const fn hall() -> Self {
        Self {
            instance_lineage_id: None,
            scene: CorePrivateRouteSceneV1::LanternHalls,
            room: None,
            phase: CorePrivateRoutePhaseV1::Hall,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePrivateRouteActorSeed {
    pub character_id: [u8; 16],
    pub character_version: u64,
    pub content_revision: CorePrivateRouteContentRevisionV1,
    pub world_flow_revision: WorldFlowContentRevisionV1,
    pub position: CorePrivateRouteActorPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePrivateRouteActorAdvance {
    EnterMicrorealm {
        instance_lineage_id: [u8; 16],
        destination_character_version: u64,
    },
    MicrorealmWaiting,
    MicrorealmActive,
    MicrorealmCleared,
    EnterCombatRoom(CorePrivateRouteRoomV1),
    RoomAwaitingDoorSafety,
    RoomSpawnWarning,
    RoomActive,
    RoomQuiet,
    RoomCleared,
    EnterRest,
    EnterBoss,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePrivateRouteActor {
    state: CorePrivateRouteStateV1,
    world_flow_revision: WorldFlowContentRevisionV1,
    bell_portal_in_range: bool,
}

impl CorePrivateRouteActor {
    pub fn new(
        seed: CorePrivateRouteActorSeed,
        actor_generation: u64,
    ) -> Result<Self, CorePrivateRouteActorError> {
        if actor_generation == 0 {
            return Err(CorePrivateRouteActorError::InvalidGeneration);
        }
        if zero_world_flow_revision(&seed.world_flow_revision) {
            return Err(CorePrivateRouteActorError::InvalidWorldFlowRevision);
        }
        let state = CorePrivateRouteStateV1 {
            schema_version: CORE_PRIVATE_ROUTE_SCHEMA_VERSION,
            character_id: seed.character_id,
            character_version: seed.character_version,
            content_revision: seed.content_revision,
            actor_generation,
            state_version: 1,
            instance_lineage_id: seed.position.instance_lineage_id,
            scene: seed.position.scene,
            room: seed.position.room,
            phase: seed.position.phase,
            readiness: CorePrivateRouteReadinessV1::canonical(seed.position.phase),
        };
        state
            .validate()
            .map_err(|_| CorePrivateRouteActorError::InvalidSeed)?;
        Ok(Self {
            state,
            world_flow_revision: seed.world_flow_revision,
            bell_portal_in_range: false,
        })
    }

    #[must_use]
    pub const fn projection(&self) -> &CorePrivateRouteStateV1 {
        &self.state
    }

    #[must_use]
    pub const fn world_flow_revision(&self) -> &WorldFlowContentRevisionV1 {
        &self.world_flow_revision
    }

    #[must_use]
    pub const fn bell_portal_in_range(&self) -> bool {
        self.bell_portal_in_range
    }

    pub fn set_bell_portal_in_range(
        &mut self,
        in_range: bool,
    ) -> Result<(), CorePrivateRouteActorError> {
        if in_range
            && (self.state.scene != CorePrivateRouteSceneV1::CoreMicrorealm
                || self.state.phase != CorePrivateRoutePhaseV1::MicrorealmCleared)
        {
            return Err(CorePrivateRouteActorError::InvalidTransition);
        }
        self.bell_portal_in_range = in_range;
        Ok(())
    }

    pub fn advance(
        &mut self,
        advance: CorePrivateRouteActorAdvance,
    ) -> Result<&CorePrivateRouteStateV1, CorePrivateRouteActorError> {
        match advance {
            CorePrivateRouteActorAdvance::EnterMicrorealm {
                instance_lineage_id,
                destination_character_version,
            } => self.enter_microrealm(instance_lineage_id, destination_character_version)?,
            CorePrivateRouteActorAdvance::MicrorealmWaiting => self.same_position_phase(
                CorePrivateRouteSceneV1::CoreMicrorealm,
                None,
                CorePrivateRoutePhaseV1::MicrorealmDormant,
                CorePrivateRoutePhaseV1::MicrorealmWaiting,
            )?,
            CorePrivateRouteActorAdvance::MicrorealmActive => self.same_position_phase(
                CorePrivateRouteSceneV1::CoreMicrorealm,
                None,
                CorePrivateRoutePhaseV1::MicrorealmWaiting,
                CorePrivateRoutePhaseV1::MicrorealmActive,
            )?,
            CorePrivateRouteActorAdvance::MicrorealmCleared => self.same_position_phase(
                CorePrivateRouteSceneV1::CoreMicrorealm,
                None,
                CorePrivateRoutePhaseV1::MicrorealmActive,
                CorePrivateRoutePhaseV1::MicrorealmCleared,
            )?,
            CorePrivateRouteActorAdvance::EnterCombatRoom(room) => {
                self.enter_combat_room(room)?;
            }
            CorePrivateRouteActorAdvance::RoomAwaitingDoorSafety => self.room_phase(
                CorePrivateRoutePhaseV1::RoomDormant,
                CorePrivateRoutePhaseV1::RoomAwaitingDoorSafety,
            )?,
            CorePrivateRouteActorAdvance::RoomSpawnWarning => {
                if !matches!(
                    self.state.phase,
                    CorePrivateRoutePhaseV1::RoomAwaitingDoorSafety
                        | CorePrivateRoutePhaseV1::RoomQuiet
                ) {
                    return Err(CorePrivateRouteActorError::InvalidTransition);
                }
                self.require_combat_room()?;
                self.replace_position(
                    self.state.character_version,
                    self.position_with_phase(CorePrivateRoutePhaseV1::RoomSpawnWarning),
                )?;
            }
            CorePrivateRouteActorAdvance::RoomActive => self.room_phase(
                CorePrivateRoutePhaseV1::RoomSpawnWarning,
                CorePrivateRoutePhaseV1::RoomActive,
            )?,
            CorePrivateRouteActorAdvance::RoomQuiet => self.room_phase(
                CorePrivateRoutePhaseV1::RoomActive,
                CorePrivateRoutePhaseV1::RoomQuiet,
            )?,
            CorePrivateRouteActorAdvance::RoomCleared => self.room_phase(
                CorePrivateRoutePhaseV1::RoomQuiet,
                CorePrivateRoutePhaseV1::RoomCleared,
            )?,
            CorePrivateRouteActorAdvance::EnterRest => self.enter_rest()?,
            CorePrivateRouteActorAdvance::EnterBoss => self.enter_boss()?,
            CorePrivateRouteActorAdvance::BossReadyCountdown => self.boss_phase(
                CorePrivateRoutePhaseV1::BossStaging,
                CorePrivateRoutePhaseV1::BossReadyCountdown,
            )?,
            CorePrivateRouteActorAdvance::BossIntroduction => self.boss_phase(
                CorePrivateRoutePhaseV1::BossReadyCountdown,
                CorePrivateRoutePhaseV1::BossIntroduction,
            )?,
            CorePrivateRouteActorAdvance::BossPhaseOne => self.boss_phase(
                CorePrivateRoutePhaseV1::BossIntroduction,
                CorePrivateRoutePhaseV1::BossPhaseOne,
            )?,
            CorePrivateRouteActorAdvance::BossBreakToTwo => self.boss_phase(
                CorePrivateRoutePhaseV1::BossPhaseOne,
                CorePrivateRoutePhaseV1::BossBreakToTwo,
            )?,
            CorePrivateRouteActorAdvance::BossPhaseTwo => self.boss_phase(
                CorePrivateRoutePhaseV1::BossBreakToTwo,
                CorePrivateRoutePhaseV1::BossPhaseTwo,
            )?,
            CorePrivateRouteActorAdvance::BossBreakToThree => self.boss_phase(
                CorePrivateRoutePhaseV1::BossPhaseTwo,
                CorePrivateRoutePhaseV1::BossBreakToThree,
            )?,
            CorePrivateRouteActorAdvance::BossPhaseThree => self.boss_phase(
                CorePrivateRoutePhaseV1::BossBreakToThree,
                CorePrivateRoutePhaseV1::BossPhaseThree,
            )?,
            CorePrivateRouteActorAdvance::BossDefeated => self.boss_phase(
                CorePrivateRoutePhaseV1::BossPhaseThree,
                CorePrivateRoutePhaseV1::BossDefeated,
            )?,
            CorePrivateRouteActorAdvance::BossExitReady => self.boss_phase(
                CorePrivateRoutePhaseV1::BossDefeated,
                CorePrivateRoutePhaseV1::BossExitReady,
            )?,
            CorePrivateRouteActorAdvance::TerminalPending => self.terminal_pending()?,
        }
        Ok(&self.state)
    }

    pub(super) fn commit_bell_portal(
        &mut self,
        destination_character_version: u64,
    ) -> Result<&CorePrivateRouteStateV1, CorePrivateRouteActorError> {
        self.move_to_bell(destination_character_version, true)
    }

    pub(super) fn reconcile_bell_portal(
        &mut self,
        destination_character_version: u64,
    ) -> Result<&CorePrivateRouteStateV1, CorePrivateRouteActorError> {
        self.move_to_bell(destination_character_version, false)
    }

    fn move_to_bell(
        &mut self,
        destination_character_version: u64,
        require_interaction_range: bool,
    ) -> Result<&CorePrivateRouteStateV1, CorePrivateRouteActorError> {
        if self.state.scene != CorePrivateRouteSceneV1::CoreMicrorealm
            || self.state.phase != CorePrivateRoutePhaseV1::MicrorealmCleared
            || self.state.room.is_some()
            || (require_interaction_range && !self.bell_portal_in_range)
        {
            return Err(CorePrivateRouteActorError::InvalidTransition);
        }
        require_next_character_version(
            self.state.character_version,
            destination_character_version,
        )?;
        self.replace_position(
            destination_character_version,
            CorePrivateRouteActorPosition {
                instance_lineage_id: self.state.instance_lineage_id,
                scene: CorePrivateRouteSceneV1::BellSepulcher,
                room: Some(CorePrivateRouteRoomV1::BellVestibuleB0),
                phase: CorePrivateRoutePhaseV1::DungeonVestibule,
            },
        )?;
        self.bell_portal_in_range = false;
        Ok(&self.state)
    }

    fn enter_microrealm(
        &mut self,
        instance_lineage_id: [u8; 16],
        destination_character_version: u64,
    ) -> Result<(), CorePrivateRouteActorError> {
        if self.state.scene != CorePrivateRouteSceneV1::LanternHalls
            || self.state.phase != CorePrivateRoutePhaseV1::Hall
            || self.state.room.is_some()
            || self.state.instance_lineage_id.is_some()
            || instance_lineage_id.iter().all(|byte| *byte == 0)
        {
            return Err(CorePrivateRouteActorError::InvalidTransition);
        }
        require_next_character_version(
            self.state.character_version,
            destination_character_version,
        )?;
        self.replace_position(
            destination_character_version,
            CorePrivateRouteActorPosition {
                instance_lineage_id: Some(instance_lineage_id),
                scene: CorePrivateRouteSceneV1::CoreMicrorealm,
                room: None,
                phase: CorePrivateRoutePhaseV1::MicrorealmDormant,
            },
        )
    }

    fn enter_combat_room(
        &mut self,
        destination: CorePrivateRouteRoomV1,
    ) -> Result<(), CorePrivateRouteActorError> {
        let valid = matches!(
            (self.state.room, self.state.phase, destination),
            (
                Some(CorePrivateRouteRoomV1::BellVestibuleB0),
                CorePrivateRoutePhaseV1::DungeonVestibule,
                CorePrivateRouteRoomV1::BellCrossB1
            ) | (
                Some(CorePrivateRouteRoomV1::BellCrossB1),
                CorePrivateRoutePhaseV1::RoomCleared,
                CorePrivateRouteRoomV1::BellNaveB2
            ) | (
                Some(CorePrivateRouteRoomV1::BellNaveB2),
                CorePrivateRoutePhaseV1::RoomCleared,
                CorePrivateRouteRoomV1::BellKnightB3
            ) | (
                Some(CorePrivateRouteRoomV1::BellRestB4),
                CorePrivateRoutePhaseV1::Rest,
                CorePrivateRouteRoomV1::BellBridgeB5
            )
        );
        if self.state.scene != CorePrivateRouteSceneV1::BellSepulcher || !valid {
            return Err(CorePrivateRouteActorError::InvalidTransition);
        }
        self.replace_position(
            self.state.character_version,
            CorePrivateRouteActorPosition {
                instance_lineage_id: self.state.instance_lineage_id,
                scene: CorePrivateRouteSceneV1::BellSepulcher,
                room: Some(destination),
                phase: CorePrivateRoutePhaseV1::RoomDormant,
            },
        )
    }

    fn enter_rest(&mut self) -> Result<(), CorePrivateRouteActorError> {
        if self.state.scene != CorePrivateRouteSceneV1::BellSepulcher
            || self.state.room != Some(CorePrivateRouteRoomV1::BellKnightB3)
            || self.state.phase != CorePrivateRoutePhaseV1::RoomCleared
        {
            return Err(CorePrivateRouteActorError::InvalidTransition);
        }
        self.replace_position(
            self.state.character_version,
            CorePrivateRouteActorPosition {
                instance_lineage_id: self.state.instance_lineage_id,
                scene: CorePrivateRouteSceneV1::BellSepulcher,
                room: Some(CorePrivateRouteRoomV1::BellRestB4),
                phase: CorePrivateRoutePhaseV1::Rest,
            },
        )
    }

    fn enter_boss(&mut self) -> Result<(), CorePrivateRouteActorError> {
        if self.state.scene != CorePrivateRouteSceneV1::BellSepulcher
            || self.state.room != Some(CorePrivateRouteRoomV1::BellBridgeB5)
            || self.state.phase != CorePrivateRoutePhaseV1::RoomCleared
        {
            return Err(CorePrivateRouteActorError::InvalidTransition);
        }
        self.replace_position(
            self.state.character_version,
            CorePrivateRouteActorPosition {
                instance_lineage_id: self.state.instance_lineage_id,
                scene: CorePrivateRouteSceneV1::BellSepulcher,
                room: Some(CorePrivateRouteRoomV1::CaldusArenaB6),
                phase: CorePrivateRoutePhaseV1::BossStaging,
            },
        )
    }

    fn room_phase(
        &mut self,
        expected: CorePrivateRoutePhaseV1,
        destination: CorePrivateRoutePhaseV1,
    ) -> Result<(), CorePrivateRouteActorError> {
        self.require_combat_room()?;
        if self.state.phase != expected {
            return Err(CorePrivateRouteActorError::InvalidTransition);
        }
        self.replace_position(
            self.state.character_version,
            self.position_with_phase(destination),
        )
    }

    fn boss_phase(
        &mut self,
        expected: CorePrivateRoutePhaseV1,
        destination: CorePrivateRoutePhaseV1,
    ) -> Result<(), CorePrivateRouteActorError> {
        if self.state.scene != CorePrivateRouteSceneV1::BellSepulcher
            || self.state.room != Some(CorePrivateRouteRoomV1::CaldusArenaB6)
            || self.state.phase != expected
        {
            return Err(CorePrivateRouteActorError::InvalidTransition);
        }
        self.replace_position(
            self.state.character_version,
            self.position_with_phase(destination),
        )
    }

    fn terminal_pending(&mut self) -> Result<(), CorePrivateRouteActorError> {
        if self.state.scene == CorePrivateRouteSceneV1::LanternHalls
            || self.state.phase == CorePrivateRoutePhaseV1::TerminalPending
            || self.state.instance_lineage_id.is_none()
        {
            return Err(CorePrivateRouteActorError::InvalidTransition);
        }
        self.replace_position(
            self.state.character_version,
            self.position_with_phase(CorePrivateRoutePhaseV1::TerminalPending),
        )
    }

    fn same_position_phase(
        &mut self,
        scene: CorePrivateRouteSceneV1,
        room: Option<CorePrivateRouteRoomV1>,
        expected: CorePrivateRoutePhaseV1,
        destination: CorePrivateRoutePhaseV1,
    ) -> Result<(), CorePrivateRouteActorError> {
        if self.state.scene != scene || self.state.room != room || self.state.phase != expected {
            return Err(CorePrivateRouteActorError::InvalidTransition);
        }
        self.replace_position(
            self.state.character_version,
            self.position_with_phase(destination),
        )
    }

    fn require_combat_room(&self) -> Result<(), CorePrivateRouteActorError> {
        if self.state.scene != CorePrivateRouteSceneV1::BellSepulcher
            || !matches!(
                self.state.room,
                Some(
                    CorePrivateRouteRoomV1::BellCrossB1
                        | CorePrivateRouteRoomV1::BellNaveB2
                        | CorePrivateRouteRoomV1::BellKnightB3
                        | CorePrivateRouteRoomV1::BellBridgeB5
                )
            )
        {
            return Err(CorePrivateRouteActorError::InvalidTransition);
        }
        Ok(())
    }

    fn position_with_phase(&self, phase: CorePrivateRoutePhaseV1) -> CorePrivateRouteActorPosition {
        CorePrivateRouteActorPosition {
            instance_lineage_id: self.state.instance_lineage_id,
            scene: self.state.scene,
            room: self.state.room,
            phase,
        }
    }

    fn replace_position(
        &mut self,
        character_version: u64,
        position: CorePrivateRouteActorPosition,
    ) -> Result<(), CorePrivateRouteActorError> {
        let state_version = self
            .state
            .state_version
            .checked_add(1)
            .ok_or(CorePrivateRouteActorError::StateVersionOverflow)?;
        let next = CorePrivateRouteStateV1 {
            schema_version: CORE_PRIVATE_ROUTE_SCHEMA_VERSION,
            character_id: self.state.character_id,
            character_version,
            content_revision: self.state.content_revision.clone(),
            actor_generation: self.state.actor_generation,
            state_version,
            instance_lineage_id: position.instance_lineage_id,
            scene: position.scene,
            room: position.room,
            phase: position.phase,
            readiness: CorePrivateRouteReadinessV1::canonical(position.phase),
        };
        next.validate()
            .map_err(|_| CorePrivateRouteActorError::InvalidTransition)?;
        self.state = next;
        Ok(())
    }
}

fn require_next_character_version(
    current: u64,
    destination: u64,
) -> Result<(), CorePrivateRouteActorError> {
    if current.checked_add(1) != Some(destination) {
        return Err(CorePrivateRouteActorError::CharacterVersionMismatch);
    }
    Ok(())
}

fn zero_world_flow_revision(revision: &WorldFlowContentRevisionV1) -> bool {
    [
        &revision.records_blake3,
        &revision.assets_blake3,
        &revision.localization_blake3,
    ]
    .into_iter()
    .any(|hash| hash.as_str().bytes().all(|byte| byte == b'0'))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CorePrivateRouteActorError {
    #[error("private-route actor generation must be nonzero")]
    InvalidGeneration,
    #[error("private-route actor seed is invalid")]
    InvalidSeed,
    #[error("private-route actor world-flow revision must be nonzero")]
    InvalidWorldFlowRevision,
    #[error("private-route actor transition is not legal from the current state")]
    InvalidTransition,
    #[error("private-route actor character version did not advance exactly once")]
    CharacterVersionMismatch,
    #[error("private-route actor state version overflowed")]
    StateVersionOverflow,
}
