use protocol::{
    CorePrivateRouteAvailabilityV1, CorePrivateRouteContentRevisionV1, CorePrivateRoutePhaseV1,
    CorePrivateRouteRoomV1, CorePrivateRouteSceneV1, ManifestHash, WorldFlowContentRevisionV1,
};

use super::{
    CorePrivateRouteActorAdvance, CorePrivateRouteActorDirectory, CorePrivateRouteActorPosition,
    CorePrivateRouteActorSeed, CorePrivateRouteRuntimeError,
};
use crate::{
    AccountId, AuthenticatedAccount, AuthenticatedNamespace, CoreBellPortalAuthority,
    CoreBellPortalBinding, CoreBellPortalPermitLease, CoreBellPortalRejection,
    CoreBellPortalTransition,
};

const ACCOUNT_ID: [u8; 16] = [1; 16];
const CHARACTER_ID: [u8; 16] = [2; 16];
const LINEAGE_ID: [u8; 16] = [3; 16];
const RESTORE_ID: [u8; 16] = [4; 16];

fn hash(byte: char) -> ManifestHash {
    ManifestHash::new(byte.to_string().repeat(64)).expect("valid fixture hash")
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
        account_id: AccountId::new(ACCOUNT_ID).expect("nonzero account"),
        namespace: AuthenticatedNamespace::WipeableTest,
    }
}

fn hall_seed(character_id: [u8; 16], character_version: u64) -> CorePrivateRouteActorSeed {
    CorePrivateRouteActorSeed {
        character_id,
        character_version,
        content_revision: route_revision(),
        world_flow_revision: world_revision(),
        position: CorePrivateRouteActorPosition::hall(),
    }
}

fn binding(character_version: u64) -> CoreBellPortalBinding {
    CoreBellPortalBinding {
        account_id: ACCOUNT_ID,
        character_id: CHARACTER_ID,
        mutation_id: [5; 16],
        instance_lineage_id: LINEAGE_ID,
        entry_restore_point_id: RESTORE_ID,
        character_version,
        content_revision: world_revision(),
    }
}

async fn clear_microrealm(
    directory: &CorePrivateRouteActorDirectory,
    lease: super::CorePrivateRouteActorLease,
) {
    directory
        .advance(
            lease,
            CorePrivateRouteActorAdvance::EnterMicrorealm {
                instance_lineage_id: LINEAGE_ID,
                destination_character_version: 2,
            },
        )
        .await
        .expect("Hall enters the exact Core micro-realm");
    for advance in [
        CorePrivateRouteActorAdvance::MicrorealmWaiting,
        CorePrivateRouteActorAdvance::MicrorealmActive,
        CorePrivateRouteActorAdvance::MicrorealmCleared,
    ] {
        directory
            .advance(lease, advance)
            .await
            .expect("micro-realm phase advances in order");
    }
}

async fn commit_bell_entry(
    directory: &CorePrivateRouteActorDirectory,
    lease: super::CorePrivateRouteActorLease,
) {
    clear_microrealm(directory, lease).await;
    directory
        .set_bell_portal_in_range(lease, true)
        .await
        .expect("authoritative range becomes eligible");
    let permit = directory
        .prepare_bell_portal(binding(2))
        .await
        .expect("cleared in-range portal prepares");
    directory
        .commit_bell_portal(
            permit,
            CoreBellPortalTransition {
                binding: binding(2),
                transfer_id: [6; 16],
                destination_character_version: 3,
            },
        )
        .await
        .expect("durable Bell transfer commits");
}

async fn clear_current_room(
    directory: &CorePrivateRouteActorDirectory,
    lease: super::CorePrivateRouteActorLease,
) {
    for advance in [
        CorePrivateRouteActorAdvance::RoomAwaitingDoorSafety,
        CorePrivateRouteActorAdvance::RoomSpawnWarning,
        CorePrivateRouteActorAdvance::RoomActive,
        CorePrivateRouteActorAdvance::RoomQuiet,
        CorePrivateRouteActorAdvance::RoomCleared,
    ] {
        directory
            .advance(lease, advance)
            .await
            .expect("fixed combat-room phase advances in order");
    }
}

#[tokio::test]
async fn actor_trace_is_exactly_hall_microrealm_b0_through_b6_terminal() {
    let directory = CorePrivateRouteActorDirectory::new();
    let lease = directory
        .register_actor(authenticated(), hall_seed(CHARACTER_ID, 1), 11)
        .expect("actor registers");

    assert!(matches!(
        directory
            .advance(lease, CorePrivateRouteActorAdvance::MicrorealmActive)
            .await,
        Err(CorePrivateRouteRuntimeError::Actor(
            super::CorePrivateRouteActorError::InvalidTransition
        ))
    ));
    commit_bell_entry(&directory, lease).await;
    let vestibule = directory.snapshot(lease).expect("snapshot");
    assert_eq!(
        vestibule.room,
        Some(CorePrivateRouteRoomV1::BellVestibuleB0)
    );
    assert_eq!(vestibule.phase, CorePrivateRoutePhaseV1::DungeonVestibule);
    assert_eq!(vestibule.character_version, 3);

    for room in [
        CorePrivateRouteRoomV1::BellCrossB1,
        CorePrivateRouteRoomV1::BellNaveB2,
        CorePrivateRouteRoomV1::BellKnightB3,
    ] {
        directory
            .advance(lease, CorePrivateRouteActorAdvance::EnterCombatRoom(room))
            .await
            .expect("next exact combat room opens");
        clear_current_room(&directory, lease).await;
    }
    directory
        .advance(lease, CorePrivateRouteActorAdvance::EnterRest)
        .await
        .expect("B4 rest follows B3");
    directory
        .advance(
            lease,
            CorePrivateRouteActorAdvance::EnterCombatRoom(CorePrivateRouteRoomV1::BellBridgeB5),
        )
        .await
        .expect("B5 follows rest");
    clear_current_room(&directory, lease).await;
    directory
        .advance(lease, CorePrivateRouteActorAdvance::EnterBoss)
        .await
        .expect("B6 follows B5");
    for advance in [
        CorePrivateRouteActorAdvance::BossReadyCountdown,
        CorePrivateRouteActorAdvance::BossIntroduction,
        CorePrivateRouteActorAdvance::BossPhaseOne,
        CorePrivateRouteActorAdvance::BossBreakToTwo,
        CorePrivateRouteActorAdvance::BossPhaseTwo,
        CorePrivateRouteActorAdvance::BossBreakToThree,
        CorePrivateRouteActorAdvance::BossPhaseThree,
        CorePrivateRouteActorAdvance::BossDefeated,
    ] {
        directory
            .advance(lease, advance)
            .await
            .expect("Caldus phases advance in order");
    }
    let defeated = directory.snapshot(lease).expect("defeated projection");
    assert_eq!(
        defeated.readiness.extraction_available,
        CorePrivateRouteAvailabilityV1::Unavailable
    );
    directory
        .advance(lease, CorePrivateRouteActorAdvance::BossExitReady)
        .await
        .expect("durable reward and exit owner unlock extraction");
    let exit_ready = directory.snapshot(lease).expect("exit-ready projection");
    assert_eq!(
        exit_ready.readiness.extraction_available,
        CorePrivateRouteAvailabilityV1::Available
    );
    directory
        .advance(lease, CorePrivateRouteActorAdvance::TerminalPending)
        .await
        .expect("terminal barrier revokes control");
    assert_eq!(
        directory
            .snapshot(lease)
            .expect("terminal projection")
            .phase,
        CorePrivateRoutePhaseV1::TerminalPending
    );

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
async fn bell_reservation_is_exclusive_pins_state_and_drop_releases_synchronously() {
    let directory = CorePrivateRouteActorDirectory::new();
    let lease = directory
        .register_actor(authenticated(), hall_seed(CHARACTER_ID, 1), 20)
        .expect("actor registers");
    clear_microrealm(&directory, lease).await;
    assert!(matches!(
        directory.prepare_bell_portal(binding(2)).await,
        Err(CoreBellPortalRejection::OutOfRange)
    ));
    directory
        .set_bell_portal_in_range(lease, true)
        .await
        .expect("range accepted");
    let permit = directory
        .prepare_bell_portal(binding(2))
        .await
        .expect("permit prepared");
    assert!(permit.permit().is_well_formed_for(&binding(2)));
    assert!(matches!(
        directory.prepare_bell_portal(binding(2)).await,
        Err(CoreBellPortalRejection::TransferInProgress)
    ));
    assert!(matches!(
        directory
            .advance(lease, CorePrivateRouteActorAdvance::TerminalPending)
            .await,
        Err(CorePrivateRouteRuntimeError::TransferInProgress)
    ));
    drop(permit);
    directory
        .advance(lease, CorePrivateRouteActorAdvance::TerminalPending)
        .await
        .expect("lease Drop releases before any asynchronous cleanup");

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
async fn concurrent_bell_prepare_has_one_winner_and_no_stranded_reservation() {
    let directory = CorePrivateRouteActorDirectory::new();
    let lease = directory
        .register_actor(authenticated(), hall_seed(CHARACTER_ID, 1), 30)
        .expect("actor registers");
    clear_microrealm(&directory, lease).await;
    directory
        .set_bell_portal_in_range(lease, true)
        .await
        .expect("range accepted");

    let mut tasks = Vec::new();
    for _ in 0..8 {
        let contender = directory.clone();
        tasks.push(tokio::spawn(async move {
            contender.prepare_bell_portal(binding(2)).await
        }));
    }
    let mut winner = None;
    let mut rejected = 0;
    for task in tasks {
        match task.await.expect("contender task") {
            Ok(permit) => {
                assert!(winner.replace(permit).is_none(), "only one permit may win");
            }
            Err(CoreBellPortalRejection::TransferInProgress) => rejected += 1,
            Err(other) => panic!("unexpected Bell rejection: {other:?}"),
        }
    }
    assert_eq!(rejected, 7);
    drop(winner);
    assert!(directory.prepare_bell_portal(binding(2)).await.is_ok());

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
async fn reconcile_applies_committed_bell_once_and_never_rewinds_later_room_state() {
    let directory = CorePrivateRouteActorDirectory::new();
    let lease = directory
        .register_actor(authenticated(), hall_seed(CHARACTER_ID, 1), 40)
        .expect("actor registers");
    clear_microrealm(&directory, lease).await;
    let transition = CoreBellPortalTransition {
        binding: binding(2),
        transfer_id: [6; 16],
        destination_character_version: 3,
    };
    directory
        .reconcile_bell_portal(transition.clone())
        .await
        .expect("durable receipt does not depend on current interaction range");
    directory
        .advance(
            lease,
            CorePrivateRouteActorAdvance::EnterCombatRoom(CorePrivateRouteRoomV1::BellCrossB1),
        )
        .await
        .expect("actor advances beyond B0");
    directory
        .reconcile_bell_portal(transition)
        .await
        .expect("replay is idempotent");
    let snapshot = directory.snapshot(lease).expect("snapshot");
    assert_eq!(snapshot.room, Some(CorePrivateRouteRoomV1::BellCrossB1));
    assert_eq!(snapshot.phase, CorePrivateRoutePhaseV1::RoomDormant);

    directory.begin_shutdown();
    let report = directory.finish_shutdown().await.expect("shutdown");
    assert!(report.zero_residue);
    assert_eq!(
        report.served_actor_commands, 7,
        "four micro-realm advances, two reconciliations, and one room advance share the mailbox"
    );
}

#[tokio::test]
async fn explicit_persistent_generation_floor_blocks_aba_and_one_account_has_one_actor() {
    let directory = CorePrivateRouteActorDirectory::new();
    let first = directory
        .register_actor(authenticated(), hall_seed(CHARACTER_ID, 7), 50)
        .expect("first generation registers");
    assert!(matches!(
        directory.register_actor(authenticated(), hall_seed([9; 16], 1), 1),
        Err(CorePrivateRouteRuntimeError::AccountAlreadyActive)
    ));
    directory
        .retire_actor(first)
        .await
        .expect("terminal owner retires danger generation");
    assert!(matches!(
        directory.register_actor(authenticated(), hall_seed(CHARACTER_ID, 8), 50),
        Err(CorePrivateRouteRuntimeError::StaleGeneration)
    ));
    let replacement = directory
        .register_actor(authenticated(), hall_seed(CHARACTER_ID, 8), 51)
        .expect("persistently higher Hall generation registers");
    assert_eq!(replacement.actor_generation(), 51);
    assert_eq!(
        directory.snapshot(replacement).expect("replacement").scene,
        CorePrivateRouteSceneV1::LanternHalls
    );

    directory.begin_shutdown();
    let report = directory.finish_shutdown().await.expect("shutdown");
    assert!(report.zero_residue);
    assert_eq!(report.remaining_actor_tasks, 0);
    assert_eq!(report.remaining_registered_actors, 0);
    assert_eq!(report.remaining_portal_reservations, 0);
}

#[tokio::test]
async fn shutdown_invalidates_outstanding_permit_and_drains_actor_task() {
    let directory = CorePrivateRouteActorDirectory::new();
    let lease = directory
        .register_actor(authenticated(), hall_seed(CHARACTER_ID, 1), 60)
        .expect("actor registers");
    clear_microrealm(&directory, lease).await;
    directory
        .set_bell_portal_in_range(lease, true)
        .await
        .expect("range accepted");
    let permit = directory
        .prepare_bell_portal(binding(2))
        .await
        .expect("permit prepared");
    directory.begin_shutdown();
    assert!(matches!(
        directory
            .commit_bell_portal(
                permit,
                CoreBellPortalTransition {
                    binding: binding(2),
                    transfer_id: [6; 16],
                    destination_character_version: 3,
                }
            )
            .await,
        Err(CoreBellPortalRejection::InstanceUnavailable)
    ));
    assert!(
        directory
            .finish_shutdown()
            .await
            .expect("shutdown")
            .zero_residue
    );
}
