//! Route-bound owner for the exact M03 Bell Sepulcher B0-B6 combat lifecycle.
//!
//! The three authorities are `Gravebound_Production_GDD_v1_Canonical.md` (`DNG-003`-`006`,
//! `COM-001`-`006`, `BRG-001`-`002`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-ROOM-007`, `CONT-BOSS-001`-`002`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`). This component does not enable normal
//! admission, commit rewards, resolve Bargains, or create the B6 exit.

use protocol::{
    CorePrivateRouteContentRevisionV1, CorePrivateRoutePhaseV1, CorePrivateRouteRoomV1,
    CorePrivateRouteSceneV1, CorePrivateRouteStateV1,
};
use sim_core::{FixedRoomPhase, Tick};
use thiserror::Error;

use crate::{
    CoreBellPortalTransition, CoreCharacterCombatEnvelope, CorePrivateMicrorealmRuntime,
    CorePrivateMicrorealmRuntimeError, CorePrivateRouteActorDirectory, CorePrivateRouteActorLease,
    CorePrivateRouteRuntimeError,
    core_private_microrealm_runtime::CorePrivateMicrorealmDungeonHandoff,
};

#[derive(Debug, Clone, PartialEq)]
pub struct CorePrivateFixedDungeonRoomFrame {
    pub tick: Tick,
    pub route: CorePrivateRouteStateV1,
    pub step: sim_content::CoreFixedDungeonRoomStep,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePrivateFixedDungeonAdvance {
    pub route: CorePrivateRouteStateV1,
    pub transition: sim_content::CoreFixedDungeonAdvance,
}

#[derive(Debug)]
pub struct CorePrivateFixedDungeonRuntime {
    route_directory: CorePrivateRouteActorDirectory,
    route_lease: CorePrivateRouteActorLease,
    content_revision: CorePrivateRouteContentRevisionV1,
    combat_envelope: CoreCharacterCombatEnvelope,
    combat: sim_content::CoreFixedDungeonCombat,
    tick: Tick,
}

impl CorePrivateFixedDungeonRuntime {
    pub fn from_committed_bell(
        microrealm: CorePrivateMicrorealmRuntime,
        transition: &CoreBellPortalTransition,
        expected_content_revision: &CorePrivateRouteContentRevisionV1,
        encounters: sim_content::CoreDevelopmentEncounterRooms,
    ) -> Result<Self, CorePrivateFixedDungeonRuntimeError> {
        let handoff = microrealm.into_fixed_dungeon_handoff(transition)?;
        Self::from_handoff(handoff, expected_content_revision, encounters)
    }

    fn from_handoff(
        handoff: CorePrivateMicrorealmDungeonHandoff,
        expected_content_revision: &CorePrivateRouteContentRevisionV1,
        encounters: sim_content::CoreDevelopmentEncounterRooms,
    ) -> Result<Self, CorePrivateFixedDungeonRuntimeError> {
        let route = handoff.route_directory.snapshot(handoff.route_lease)?;
        if route.content_revision != *expected_content_revision
            || route.character_id != handoff.combat_envelope.character_id()
            || route.character_version != handoff.combat_envelope.character_state_version()
            || route.actor_generation != handoff.route_lease.actor_generation()
            || route.scene != CorePrivateRouteSceneV1::BellSepulcher
            || route.room != Some(CorePrivateRouteRoomV1::BellVestibuleB0)
            || route.phase != CorePrivateRoutePhaseV1::DungeonVestibule
            || route.instance_lineage_id.is_none()
        {
            return Err(CorePrivateFixedDungeonRuntimeError::InvalidComposition);
        }
        let run_ordinal = u32::try_from(route.actor_generation)
            .map_err(|_| CorePrivateFixedDungeonRuntimeError::InvalidComposition)?;
        let combat = sim_content::CoreFixedDungeonCombat::from_handoff_at(
            encounters,
            run_ordinal,
            handoff.next_hostile_spawn_ordinal,
            handoff.participant,
        )?;
        Ok(Self {
            route_directory: handoff.route_directory,
            route_lease: handoff.route_lease,
            content_revision: expected_content_revision.clone(),
            combat_envelope: handoff.combat_envelope,
            combat,
            tick: handoff.final_tick,
        })
    }

    #[must_use]
    pub const fn route_lease(&self) -> CorePrivateRouteActorLease {
        self.route_lease
    }

    #[must_use]
    pub const fn account_id(&self) -> [u8; 16] {
        self.route_lease.account_id()
    }

    #[must_use]
    pub const fn character_id(&self) -> [u8; 16] {
        self.route_lease.character_id()
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    #[must_use]
    pub const fn node(&self) -> sim_content::CoreFixedDungeonNode {
        self.combat.node()
    }

    #[must_use]
    pub fn room_phase(&self) -> Option<FixedRoomPhase> {
        self.combat.room_phase()
    }

    pub async fn advance(
        &mut self,
    ) -> Result<CorePrivateFixedDungeonAdvance, CorePrivateFixedDungeonRuntimeError> {
        let route_before = self.route_directory.snapshot(self.route_lease)?;
        self.validate_route_authority(&route_before)?;
        let mut staged = self.combat.clone();
        let transition = staged.advance()?;
        let (room, phase) = route_position(transition.to, staged.room_phase())?;
        let route = self
            .route_directory
            .apply_fixed_dungeon_authority(
                self.route_lease,
                route_before.state_version,
                room,
                phase,
            )
            .await?;
        self.combat = staged;
        Ok(CorePrivateFixedDungeonAdvance { route, transition })
    }

    pub async fn step_room(
        &mut self,
        input: &sim_content::CoreImmutableFixedRoomInput,
    ) -> Result<CorePrivateFixedDungeonRoomFrame, CorePrivateFixedDungeonRuntimeError> {
        let tick = self
            .tick
            .checked_next()
            .ok_or(CorePrivateFixedDungeonRuntimeError::TickExhausted)?;
        if input
            .combat_step
            .as_ref()
            .is_some_and(|combat| combat.tick != tick)
        {
            return Err(CorePrivateFixedDungeonRuntimeError::CombatTickMismatch);
        }
        let route_before = self.route_directory.snapshot(self.route_lease)?;
        self.validate_route_authority(&route_before)?;
        let mut staged = self.combat.clone();
        let step = staged.step_room(tick, input)?;
        let (room, phase) = route_position(staged.node(), Some(step.phase_after()))?;
        let route = self
            .route_directory
            .apply_fixed_dungeon_authority(
                self.route_lease,
                route_before.state_version,
                room,
                phase,
            )
            .await?;
        self.combat = staged;
        self.tick = tick;
        Ok(CorePrivateFixedDungeonRoomFrame { tick, route, step })
    }

    fn validate_route_authority(
        &self,
        route: &CorePrivateRouteStateV1,
    ) -> Result<(), CorePrivateFixedDungeonRuntimeError> {
        let (room, phase) = route_position(self.combat.node(), self.combat.room_phase())?;
        if route.character_id != self.combat_envelope.character_id()
            || route.character_version != self.combat_envelope.character_state_version()
            || route.content_revision != self.content_revision
            || route.actor_generation != self.route_lease.actor_generation()
            || route.scene != CorePrivateRouteSceneV1::BellSepulcher
            || route.room != Some(room)
            || route.phase != phase
            || route.instance_lineage_id.is_none()
        {
            return Err(CorePrivateFixedDungeonRuntimeError::RouteAuthorityMismatch);
        }
        Ok(())
    }
}

fn route_position(
    node: sim_content::CoreFixedDungeonNode,
    room_phase: Option<FixedRoomPhase>,
) -> Result<(CorePrivateRouteRoomV1, CorePrivateRoutePhaseV1), CorePrivateFixedDungeonRuntimeError>
{
    use sim_content::CoreFixedDungeonNode as Node;
    match node {
        Node::BellVestibuleB0 => Ok((
            CorePrivateRouteRoomV1::BellVestibuleB0,
            CorePrivateRoutePhaseV1::DungeonVestibule,
        )),
        Node::BellCrossB1 => combat_route_position(CorePrivateRouteRoomV1::BellCrossB1, room_phase),
        Node::BellNaveB2 => combat_route_position(CorePrivateRouteRoomV1::BellNaveB2, room_phase),
        Node::BellKnightB3 => {
            combat_route_position(CorePrivateRouteRoomV1::BellKnightB3, room_phase)
        }
        Node::BellRestB4 => Ok((
            CorePrivateRouteRoomV1::BellRestB4,
            CorePrivateRoutePhaseV1::Rest,
        )),
        Node::BellBridgeB5 => {
            combat_route_position(CorePrivateRouteRoomV1::BellBridgeB5, room_phase)
        }
        Node::CaldusArenaB6 => Ok((
            CorePrivateRouteRoomV1::CaldusArenaB6,
            CorePrivateRoutePhaseV1::BossStaging,
        )),
    }
}

fn combat_route_position(
    room: CorePrivateRouteRoomV1,
    phase: Option<FixedRoomPhase>,
) -> Result<(CorePrivateRouteRoomV1, CorePrivateRoutePhaseV1), CorePrivateFixedDungeonRuntimeError>
{
    let phase = match phase.ok_or(CorePrivateFixedDungeonRuntimeError::InvalidComposition)? {
        FixedRoomPhase::Dormant => CorePrivateRoutePhaseV1::RoomDormant,
        FixedRoomPhase::AwaitingDoorSafety => CorePrivateRoutePhaseV1::RoomAwaitingDoorSafety,
        FixedRoomPhase::SpawnWarning => CorePrivateRoutePhaseV1::RoomSpawnWarning,
        FixedRoomPhase::Active => CorePrivateRoutePhaseV1::RoomActive,
        FixedRoomPhase::Quiet => CorePrivateRoutePhaseV1::RoomQuiet,
        FixedRoomPhase::Cleared => CorePrivateRoutePhaseV1::RoomCleared,
    };
    Ok((room, phase))
}

#[derive(Debug, Error)]
pub enum CorePrivateFixedDungeonRuntimeError {
    #[error("live Core fixed-dungeon composition is invalid")]
    InvalidComposition,
    #[error("live Core fixed-dungeon route authority no longer matches local state")]
    RouteAuthorityMismatch,
    #[error("live Core fixed-dungeon run-local tick exhausted")]
    TickExhausted,
    #[error("live Core fixed-dungeon combat tick does not match the server-owned frame")]
    CombatTickMismatch,
    #[error(transparent)]
    Microrealm(#[from] CorePrivateMicrorealmRuntimeError),
    #[error(transparent)]
    Dungeon(#[from] sim_content::CoreFixedDungeonError),
    #[error(transparent)]
    Route(#[from] CorePrivateRouteRuntimeError),
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroU64, path::Path};

    use protocol::{ManifestHash, WorldFlowContentRevisionV1};
    use sim_core::{CombatStep, EntityId, EntityIdAllocator, SimulationVector};

    use super::*;
    use crate::{
        AccountId, AuthenticatedAccount, AuthenticatedNamespace, CorePrivateRouteActorSeed,
    };

    const ACCOUNT_ID: [u8; 16] = [0x31; 16];
    const CHARACTER_ID: [u8; 16] = [0x32; 16];
    const LINEAGE_ID: [u8; 16] = [0x33; 16];

    fn hash(byte: char) -> ManifestHash {
        ManifestHash::new(byte.to_string().repeat(64)).expect("valid hash")
    }

    fn route_revision() -> CorePrivateRouteContentRevisionV1 {
        CorePrivateRouteContentRevisionV1 {
            records_blake3: hash('a'),
            assets_blake3: hash('b'),
            localization_blake3: hash('c'),
        }
    }

    fn world_revision() -> WorldFlowContentRevisionV1 {
        WorldFlowContentRevisionV1 {
            records_blake3: hash('d'),
            assets_blake3: hash('e'),
            localization_blake3: hash('f'),
        }
    }

    fn authenticated() -> AuthenticatedAccount {
        AuthenticatedAccount {
            account_id: AccountId::new(ACCOUNT_ID).expect("account"),
            namespace: AuthenticatedNamespace::WipeableTest,
        }
    }

    fn fixture() -> (
        CorePrivateRouteActorDirectory,
        CorePrivateRouteActorLease,
        CorePrivateFixedDungeonRuntime,
    ) {
        let directory = CorePrivateRouteActorDirectory::new();
        let lease = directory
            .register_actor(
                authenticated(),
                CorePrivateRouteActorSeed {
                    character_id: CHARACTER_ID,
                    character_version: 3,
                    content_revision: route_revision(),
                    world_flow_revision: world_revision(),
                    position: crate::CorePrivateRouteActorPosition {
                        instance_lineage_id: Some(LINEAGE_ID),
                        scene: CorePrivateRouteSceneV1::BellSepulcher,
                        room: Some(CorePrivateRouteRoomV1::BellVestibuleB0),
                        phase: CorePrivateRoutePhaseV1::DungeonVestibule,
                    },
                },
                7,
            )
            .expect("route actor");
        let combat = crate::combat_factory::core_character_combat_test_fixture(CHARACTER_ID);
        let player_id = EntityId::new(710_000).expect("player ID");
        let (mut envelope, player) = combat
            .into_live_player(player_id, SimulationVector::new(8.5, 40.5))
            .expect("live player");
        envelope
            .rebase_character_state_version(2, 3)
            .expect("Bell version rebase");
        let handoff = CorePrivateMicrorealmDungeonHandoff {
            route_directory: directory.clone(),
            route_lease: lease,
            combat_envelope: envelope,
            participant: sim_core::NormalWaveHandoff {
                player,
                hostile_projectile_ids: EntityIdAllocator::starting_at(
                    NonZeroU64::new(900_000).expect("projectile allocator"),
                ),
            },
            next_hostile_spawn_ordinal: 9,
            final_tick: Tick(32),
        };
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let encounters =
            sim_content::load_core_development_encounter_rooms(&root).expect("Core encounters");
        let runtime =
            CorePrivateFixedDungeonRuntime::from_handoff(handoff, &route_revision(), encounters)
                .expect("fixed dungeon runtime");
        (directory, lease, runtime)
    }

    fn room_input(tick: Tick, crossed: bool) -> sim_content::CoreImmutableFixedRoomInput {
        sim_content::CoreImmutableFixedRoomInput {
            crossed_activation_boundary: crossed,
            living_inside: 1,
            living_party_outside: 0,
            doorway_hurtbox_blocked: false,
            combat_step: Some(CombatStep {
                tick,
                ..CombatStep::default()
            }),
        }
    }

    #[tokio::test]
    async fn carried_tick_and_route_cas_enter_b1_then_commit_one_multiphase_frame() {
        let (directory, _, mut runtime) = fixture();
        assert_eq!(runtime.tick(), Tick(32));
        assert_eq!(
            runtime.node(),
            sim_content::CoreFixedDungeonNode::BellVestibuleB0
        );

        let entered = runtime.advance().await.expect("enter B1");
        assert_eq!(
            entered.route.room,
            Some(CorePrivateRouteRoomV1::BellCrossB1)
        );
        assert_eq!(entered.route.phase, CorePrivateRoutePhaseV1::RoomDormant);
        assert_eq!(runtime.tick(), Tick(32));

        let frame = runtime
            .step_room(&room_input(Tick(33), true))
            .await
            .expect("participant lock and warning");
        assert_eq!(frame.tick, Tick(33));
        assert_eq!(frame.route.phase, CorePrivateRoutePhaseV1::RoomSpawnWarning);
        assert_eq!(runtime.tick(), Tick(33));

        directory.begin_shutdown();
        assert!(
            directory
                .finish_shutdown()
                .await
                .expect("shutdown")
                .zero_residue
        );
    }

    #[tokio::test]
    async fn stale_route_rejects_a_frame_without_advancing_local_tick_or_phase() {
        let (directory, lease, mut runtime) = fixture();
        runtime.advance().await.expect("enter B1");
        directory
            .advance(
                lease,
                crate::CorePrivateRouteActorAdvance::RoomAwaitingDoorSafety,
            )
            .await
            .expect("competing route writer");

        assert!(matches!(
            runtime.step_room(&room_input(Tick(33), false)).await,
            Err(CorePrivateFixedDungeonRuntimeError::RouteAuthorityMismatch)
        ));
        assert_eq!(runtime.tick(), Tick(32));
        assert_eq!(runtime.room_phase(), Some(FixedRoomPhase::Dormant));

        directory.begin_shutdown();
        assert!(
            directory
                .finish_shutdown()
                .await
                .expect("shutdown")
                .zero_residue
        );
    }
}
