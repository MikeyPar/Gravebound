use protocol::{
    CorePrivateRouteAvailabilityV1, CorePrivateRouteContentRevisionV1, CorePrivateRoutePhaseV1,
    CorePrivateRouteRoomV1, CorePrivateRouteSceneV1, CorePrivateRouteStateV1, ManifestHash,
    WorldFlowContentRevisionV1,
};

use super::{
    CorePrivateRouteActorAdvance, CorePrivateRouteActorDirectory, CorePrivateRouteActorPosition,
    CorePrivateRouteActorSeed, CorePrivateRouteEnterMicrorealmTransition,
    CorePrivateRouteExtractionBinding, CorePrivateRouteExtractionExitBinding,
    CorePrivateRouteReturnToCharacterSelectTransition, CorePrivateRouteRuntimeError,
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

fn extraction_exit_binding() -> CorePrivateRouteExtractionExitBinding {
    CorePrivateRouteExtractionExitBinding::new([7; 16], [8; 16], [9; 16], [10; 16], [11; 16])
        .expect("server-owned exit identities are well formed")
}

fn changed_extraction_exit_binding() -> CorePrivateRouteExtractionExitBinding {
    CorePrivateRouteExtractionExitBinding::new([7; 16], [8; 16], [12; 16], [10; 16], [11; 16])
        .expect("changed request remains structurally valid")
}

fn extraction_binding(
    accepted_route: protocol::CorePrivateRouteStateV1,
    exit: CorePrivateRouteExtractionExitBinding,
) -> CorePrivateRouteExtractionBinding {
    CorePrivateRouteExtractionBinding::new(
        ACCOUNT_ID,
        accepted_route,
        world_revision(),
        RESTORE_ID,
        exit,
    )
    .expect("BossExitReady authority is well formed")
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

async fn advance_bell_to_boss_exit_ready(
    directory: &CorePrivateRouteActorDirectory,
    lease: super::CorePrivateRouteActorLease,
) -> protocol::CorePrivateRouteStateV1 {
    for room in [
        CorePrivateRouteRoomV1::BellCrossB1,
        CorePrivateRouteRoomV1::BellNaveB2,
        CorePrivateRouteRoomV1::BellKnightB3,
    ] {
        directory
            .advance(lease, CorePrivateRouteActorAdvance::EnterCombatRoom(room))
            .await
            .expect("next exact combat room opens");
        clear_current_room(directory, lease).await;
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
    clear_current_room(directory, lease).await;
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
    exit_ready
}

async fn reach_boss_exit_ready(
    directory: &CorePrivateRouteActorDirectory,
    lease: super::CorePrivateRouteActorLease,
) -> protocol::CorePrivateRouteStateV1 {
    commit_bell_entry(directory, lease).await;
    advance_bell_to_boss_exit_ready(directory, lease).await
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

    let exit_ready = advance_bell_to_boss_exit_ready(&directory, lease).await;
    let permit = directory
        .prepare_extraction_terminal(
            lease,
            extraction_binding(exit_ready, extraction_exit_binding()),
        )
        .await
        .expect("terminal barrier revokes control");
    directory
        .revalidate_extraction_terminal(lease, &permit)
        .await
        .expect("current permit revalidates before publication");
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
async fn boss_authority_cas_is_atomic_replay_safe_and_resettable_before_defeat() {
    let directory = CorePrivateRouteActorDirectory::new();
    let lease = directory
        .register_actor(authenticated(), hall_seed(CHARACTER_ID, 1), 82)
        .expect("actor registers");
    commit_bell_entry(&directory, lease).await;
    for room in [
        CorePrivateRouteRoomV1::BellCrossB1,
        CorePrivateRouteRoomV1::BellNaveB2,
        CorePrivateRouteRoomV1::BellKnightB3,
    ] {
        directory
            .advance(lease, CorePrivateRouteActorAdvance::EnterCombatRoom(room))
            .await
            .expect("enter fixed room");
        clear_current_room(&directory, lease).await;
    }
    directory
        .advance(lease, CorePrivateRouteActorAdvance::EnterRest)
        .await
        .expect("enter B4");
    directory
        .advance(
            lease,
            CorePrivateRouteActorAdvance::EnterCombatRoom(CorePrivateRouteRoomV1::BellBridgeB5),
        )
        .await
        .expect("enter B5");
    clear_current_room(&directory, lease).await;
    let staging = directory
        .advance(lease, CorePrivateRouteActorAdvance::EnterBoss)
        .await
        .expect("enter B6 staging");

    let countdown = commit_boss_phase(
        &directory,
        lease,
        staging.state_version,
        CorePrivateRoutePhaseV1::BossReadyCountdown,
    )
    .await;
    assert_eq!(countdown.phase, CorePrivateRoutePhaseV1::BossReadyCountdown);
    assert!(matches!(
        directory
            .apply_fixed_dungeon_authority(
                lease,
                staging.state_version,
                CorePrivateRouteRoomV1::CaldusArenaB6,
                CorePrivateRoutePhaseV1::BossIntroduction,
            )
            .await,
        Err(CorePrivateRouteRuntimeError::StaleRouteState)
    ));

    let introduction = commit_boss_phase(
        &directory,
        lease,
        countdown.state_version,
        CorePrivateRoutePhaseV1::BossIntroduction,
    )
    .await;
    let reset = commit_boss_phase(
        &directory,
        lease,
        introduction.state_version,
        CorePrivateRoutePhaseV1::BossStaging,
    )
    .await;
    assert_eq!(reset.phase, CorePrivateRoutePhaseV1::BossStaging);
    let replay = commit_boss_phase(
        &directory,
        lease,
        reset.state_version,
        CorePrivateRoutePhaseV1::BossStaging,
    )
    .await;
    assert_eq!(replay, reset);

    let mut phase = replay;
    for target in [
        CorePrivateRoutePhaseV1::BossReadyCountdown,
        CorePrivateRoutePhaseV1::BossIntroduction,
        CorePrivateRoutePhaseV1::BossBreakToTwo,
        CorePrivateRoutePhaseV1::BossDefeated,
    ] {
        phase = commit_boss_phase(&directory, lease, phase.state_version, target).await;
    }
    let defeated = phase;
    assert_eq!(defeated.phase, CorePrivateRoutePhaseV1::BossDefeated);

    directory.begin_shutdown();
    assert!(directory.finish_shutdown().await.unwrap().zero_residue);
}

async fn commit_boss_phase(
    directory: &CorePrivateRouteActorDirectory,
    lease: super::CorePrivateRouteActorLease,
    expected_state_version: u64,
    phase: CorePrivateRoutePhaseV1,
) -> CorePrivateRouteStateV1 {
    directory
        .apply_fixed_dungeon_authority(
            lease,
            expected_state_version,
            CorePrivateRouteRoomV1::CaldusArenaB6,
            phase,
        )
        .await
        .expect("commit boss phase")
}

#[tokio::test]
async fn fixed_dungeon_cas_commits_multiphase_frames_resets_and_rejects_stale_authority() {
    let directory = CorePrivateRouteActorDirectory::new();
    let lease = directory
        .register_actor(authenticated(), hall_seed(CHARACTER_ID, 1), 12)
        .expect("actor registers");
    commit_bell_entry(&directory, lease).await;
    let b0 = directory.snapshot(lease).expect("B0 snapshot");

    let b1 = directory
        .apply_fixed_dungeon_authority(
            lease,
            b0.state_version,
            CorePrivateRouteRoomV1::BellCrossB1,
            CorePrivateRoutePhaseV1::RoomDormant,
        )
        .await
        .expect("B0 enters B1 under one route CAS");
    let warning = directory
        .apply_fixed_dungeon_authority(
            lease,
            b1.state_version,
            CorePrivateRouteRoomV1::BellCrossB1,
            CorePrivateRoutePhaseV1::RoomSpawnWarning,
        )
        .await
        .expect("one frame locks the participant and closes a clear doorway");
    assert_eq!(warning.room, Some(CorePrivateRouteRoomV1::BellCrossB1));
    assert_eq!(warning.phase, CorePrivateRoutePhaseV1::RoomSpawnWarning);
    assert_eq!(warning.state_version, b0.state_version + 3);

    let replay = directory
        .apply_fixed_dungeon_authority(
            lease,
            warning.state_version,
            CorePrivateRouteRoomV1::BellCrossB1,
            CorePrivateRoutePhaseV1::RoomSpawnWarning,
        )
        .await
        .expect("same fixed-dungeon projection is an exact no-op");
    assert_eq!(replay.state_version, warning.state_version);

    assert!(matches!(
        directory
            .apply_fixed_dungeon_authority(
                lease,
                b0.state_version,
                CorePrivateRouteRoomV1::BellCrossB1,
                CorePrivateRoutePhaseV1::RoomActive,
            )
            .await,
        Err(CorePrivateRouteRuntimeError::StaleRouteState)
    ));
    assert_eq!(
        directory
            .snapshot(lease)
            .expect("unchanged after stale CAS"),
        warning
    );

    let active = directory
        .apply_fixed_dungeon_authority(
            lease,
            warning.state_version,
            CorePrivateRouteRoomV1::BellCrossB1,
            CorePrivateRoutePhaseV1::RoomActive,
        )
        .await
        .expect("warning reaches active");
    let reset = directory
        .apply_fixed_dungeon_authority(
            lease,
            active.state_version,
            CorePrivateRouteRoomV1::BellCrossB1,
            CorePrivateRoutePhaseV1::RoomDormant,
        )
        .await
        .expect("empty-room reset returns to dormant atomically");
    assert_eq!(reset.phase, CorePrivateRoutePhaseV1::RoomDormant);

    assert!(matches!(
        directory
            .apply_fixed_dungeon_authority(
                lease,
                reset.state_version,
                CorePrivateRouteRoomV1::BellRestB4,
                CorePrivateRoutePhaseV1::RoomActive,
            )
            .await,
        Err(CorePrivateRouteRuntimeError::StaleRouteState)
    ));
    assert_eq!(
        directory
            .snapshot(lease)
            .expect("invalid target rolls back"),
        reset
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
async fn committed_microrealm_entry_reconciles_once_and_exact_replay_never_rewinds() {
    let directory = CorePrivateRouteActorDirectory::new();
    let lease = directory
        .register_actor(authenticated(), hall_seed(CHARACTER_ID, 4), 70)
        .expect("Hall actor registers");
    let transition = CorePrivateRouteEnterMicrorealmTransition {
        transfer_id: [70; 16],
        source_character_version: 4,
        destination_character_version: 5,
        instance_lineage_id: LINEAGE_ID,
        content_revision: world_revision(),
    };

    let entered = directory
        .reconcile_enter_microrealm(lease, transition.clone())
        .await
        .expect("committed entry converges the retained Hall actor");
    assert_eq!(entered.character_version, 5);
    assert_eq!(entered.instance_lineage_id, Some(LINEAGE_ID));
    assert_eq!(entered.scene, CorePrivateRouteSceneV1::CoreMicrorealm);
    assert_eq!(entered.phase, CorePrivateRoutePhaseV1::MicrorealmDormant);

    directory
        .advance(lease, CorePrivateRouteActorAdvance::MicrorealmWaiting)
        .await
        .expect("runtime may advance after durable entry");
    let replayed = directory
        .reconcile_enter_microrealm(lease, transition.clone())
        .await
        .expect("exact receipt replay is a no-op after later phase progress");
    assert_eq!(replayed.phase, CorePrivateRoutePhaseV1::MicrorealmWaiting);
    assert_eq!(replayed.state_version, entered.state_version + 1);

    let mut changed_transfer = transition.clone();
    changed_transfer.transfer_id = [72; 16];
    assert!(matches!(
        directory
            .reconcile_enter_microrealm(lease, changed_transfer)
            .await,
        Err(CorePrivateRouteRuntimeError::StaleRouteState)
    ));

    let mut changed_lineage = transition.clone();
    changed_lineage.instance_lineage_id = [71; 16];
    assert!(matches!(
        directory
            .reconcile_enter_microrealm(lease, changed_lineage)
            .await,
        Err(CorePrivateRouteRuntimeError::StaleRouteState)
    ));
    let mut changed_content = transition.clone();
    changed_content.content_revision.records_blake3 = hash('9');
    assert!(matches!(
        directory
            .reconcile_enter_microrealm(lease, changed_content)
            .await,
        Err(CorePrivateRouteRuntimeError::ContentAuthorityMismatch)
    ));
    let mut skipped_version = transition;
    skipped_version.destination_character_version = 6;
    assert!(matches!(
        directory
            .reconcile_enter_microrealm(lease, skipped_version)
            .await,
        Err(CorePrivateRouteRuntimeError::InvalidActorBinding)
    ));

    directory.begin_shutdown();
    let report = directory.finish_shutdown().await.expect("shutdown");
    assert!(report.zero_residue);
    assert_eq!(report.remaining_transition_reconciliations, 0);
}

#[tokio::test]
async fn committed_character_select_return_retires_only_its_exact_hall_generation() {
    let directory = CorePrivateRouteActorDirectory::new();
    let lease = directory
        .register_actor(authenticated(), hall_seed(CHARACTER_ID, 8), 80)
        .expect("Hall actor registers");
    let transition = CorePrivateRouteReturnToCharacterSelectTransition {
        transfer_id: [80; 16],
        source_character_version: 8,
        destination_character_version: 9,
        content_revision: world_revision(),
    };

    directory
        .reconcile_return_to_character_select(lease, transition.clone())
        .await
        .expect("committed return retires its Hall actor");
    assert!(matches!(
        directory.snapshot(lease),
        Err(CorePrivateRouteRuntimeError::ActorUnavailable)
    ));
    directory
        .reconcile_return_to_character_select(lease, transition.clone())
        .await
        .expect("exact response-loss replay uses the retirement tombstone");

    let mut changed_replay = transition.clone();
    changed_replay.transfer_id = [81; 16];
    assert!(matches!(
        directory
            .reconcile_return_to_character_select(lease, changed_replay)
            .await,
        Err(CorePrivateRouteRuntimeError::StaleRouteState)
    ));

    let replacement = directory
        .register_actor(authenticated(), hall_seed(CHARACTER_ID, 9), 81)
        .expect("a persistently newer Hall generation registers");
    directory
        .reconcile_return_to_character_select(lease, transition)
        .await
        .expect("old exact replay cannot retire the replacement");
    assert_eq!(
        directory
            .snapshot(replacement)
            .expect("replacement remains live")
            .actor_generation,
        81
    );

    directory.begin_shutdown();
    let report = directory.finish_shutdown().await.expect("shutdown");
    assert!(report.zero_residue);
    assert_eq!(report.remaining_transition_reconciliations, 0);
}

#[tokio::test]
async fn extraction_reservation_is_atomic_idempotent_and_pins_paired_authority() {
    let directory = CorePrivateRouteActorDirectory::new();
    let lease = directory
        .register_actor(authenticated(), hall_seed(CHARACTER_ID, 1), 12)
        .expect("actor registers");
    let accepted = reach_boss_exit_ready(&directory, lease).await;
    let accepted_version = accepted.state_version;
    let binding = extraction_binding(accepted.clone(), extraction_exit_binding());

    let permit = directory
        .prepare_extraction_terminal(lease, binding.clone())
        .await
        .expect("BossExitReady actor reserves the extraction terminal");
    assert_ne!(permit.permit_id(), [0; 16]);
    assert_eq!(permit.actor_generation(), lease.actor_generation());
    assert_eq!(permit.accepted_route_state_version(), accepted_version);
    assert_eq!(
        permit.terminal_pending_route_state_version(),
        accepted_version + 1
    );
    assert_eq!(permit.route_content_revision(), &route_revision());
    assert_eq!(permit.world_flow_revision(), &world_revision());
    assert_eq!(permit.binding().account_id(), ACCOUNT_ID);
    assert_eq!(permit.binding().accepted_route(), &accepted);
    assert_eq!(permit.binding().exit().extraction_request_id(), [9; 16]);

    let replay = directory
        .prepare_extraction_terminal(lease, binding.clone())
        .await
        .expect("exact prepare replay returns the actor-owned reservation");
    assert_eq!(replay, permit);
    directory
        .revalidate_extraction_terminal(lease, &permit)
        .await
        .expect("permit remains current before prepared/result publication");
    assert_eq!(
        directory
            .snapshot(lease)
            .expect("terminal projection")
            .phase,
        CorePrivateRoutePhaseV1::TerminalPending
    );
    assert!(matches!(
        directory
            .advance(lease, CorePrivateRouteActorAdvance::BossExitReady)
            .await,
        Err(CorePrivateRouteRuntimeError::TerminalInProgress)
    ));

    let changed = extraction_binding(accepted, changed_extraction_exit_binding());
    assert!(matches!(
        directory.prepare_extraction_terminal(lease, changed).await,
        Err(CorePrivateRouteRuntimeError::TerminalReservationConflict)
    ));

    directory.begin_shutdown();
    let report = directory.finish_shutdown().await.expect("shutdown");
    assert!(report.zero_residue);
    assert_eq!(report.remaining_terminal_reservations, 0);
    assert_eq!(report.remaining_actor_tasks, 0);
}

#[tokio::test]
async fn extraction_abort_is_exact_monotonic_and_cannot_release_another_permit() {
    let directory = CorePrivateRouteActorDirectory::new();
    let lease = directory
        .register_actor(authenticated(), hall_seed(CHARACTER_ID, 1), 121)
        .expect("actor registers");
    let accepted = reach_boss_exit_ready(&directory, lease).await;
    let first = directory
        .prepare_extraction_terminal(
            lease,
            extraction_binding(accepted, extraction_exit_binding()),
        )
        .await
        .expect("first terminal reserves");

    let reopened = directory
        .abort_extraction_terminal(lease, &first)
        .await
        .expect("the exact uncommitted permit reopens the exit");
    assert_eq!(reopened.phase, CorePrivateRoutePhaseV1::BossExitReady);
    assert_eq!(
        reopened.readiness.extraction_available,
        CorePrivateRouteAvailabilityV1::Available
    );
    assert_eq!(
        reopened.state_version,
        first.terminal_pending_route_state_version() + 1
    );
    assert!(reopened.state_version > first.terminal_pending_route_state_version());
    assert!(matches!(
        directory
            .revalidate_extraction_terminal(lease, &first)
            .await,
        Err(CorePrivateRouteRuntimeError::TerminalReservationConflict)
    ));

    let second = directory
        .prepare_extraction_terminal(
            lease,
            extraction_binding(reopened, changed_extraction_exit_binding()),
        )
        .await
        .expect("the reopened authority can reserve a new exact terminal");
    assert_ne!(second.permit_id(), first.permit_id());
    let second_pending = directory
        .snapshot(lease)
        .expect("second terminal projection");
    assert!(matches!(
        directory.abort_extraction_terminal(lease, &first).await,
        Err(CorePrivateRouteRuntimeError::TerminalReservationConflict)
    ));
    assert_eq!(
        directory.snapshot(lease).expect("stale abort is inert"),
        second_pending
    );
    directory
        .revalidate_extraction_terminal(lease, &second)
        .await
        .expect("the newer reservation remains pinned");

    let foreign_directory = CorePrivateRouteActorDirectory::new();
    let foreign_lease = foreign_directory
        .register_actor(authenticated(), hall_seed([77; 16], 1), 121)
        .expect("foreign character actor registers");
    let foreign_before = foreign_directory
        .snapshot(foreign_lease)
        .expect("foreign actor projection");
    assert!(matches!(
        foreign_directory
            .abort_extraction_terminal(foreign_lease, &second)
            .await,
        Err(CorePrivateRouteRuntimeError::InvalidExtractionBinding)
    ));
    assert_eq!(
        foreign_directory
            .snapshot(foreign_lease)
            .expect("foreign permit is inert"),
        foreign_before
    );

    directory
        .abort_extraction_terminal(lease, &second)
        .await
        .expect("current terminal cleans up");
    directory.begin_shutdown();
    assert!(
        directory
            .finish_shutdown()
            .await
            .expect("shutdown")
            .zero_residue
    );
    foreign_directory.begin_shutdown();
    assert!(
        foreign_directory
            .finish_shutdown()
            .await
            .expect("foreign shutdown")
            .zero_residue
    );
}

#[tokio::test]
async fn extraction_abort_rejects_a_changed_permit_without_disturbing_current_authority() {
    let directory = CorePrivateRouteActorDirectory::new();
    let lease = directory
        .register_actor(authenticated(), hall_seed(CHARACTER_ID, 1), 122)
        .expect("actor registers");
    let accepted = reach_boss_exit_ready(&directory, lease).await;
    let current = directory
        .prepare_extraction_terminal(
            lease,
            extraction_binding(accepted, extraction_exit_binding()),
        )
        .await
        .expect("current terminal reserves");

    let other_directory = CorePrivateRouteActorDirectory::new();
    let other_lease = other_directory
        .register_actor(authenticated(), hall_seed(CHARACTER_ID, 1), 122)
        .expect("independent actor registers");
    let other_accepted = reach_boss_exit_ready(&other_directory, other_lease).await;
    let changed = other_directory
        .prepare_extraction_terminal(
            other_lease,
            extraction_binding(other_accepted, changed_extraction_exit_binding()),
        )
        .await
        .expect("changed server authority reserves only its own actor");

    let before = directory
        .snapshot(lease)
        .expect("current terminal projection");
    assert!(matches!(
        directory.abort_extraction_terminal(lease, &changed).await,
        Err(CorePrivateRouteRuntimeError::TerminalReservationConflict)
    ));
    assert_eq!(
        directory.snapshot(lease).expect("changed abort is inert"),
        before
    );
    directory
        .revalidate_extraction_terminal(lease, &current)
        .await
        .expect("current permit remains authoritative");

    directory.begin_shutdown();
    assert!(
        directory
            .finish_shutdown()
            .await
            .expect("shutdown")
            .zero_residue
    );
    other_directory.begin_shutdown();
    assert!(
        other_directory
            .finish_shutdown()
            .await
            .expect("other shutdown")
            .zero_residue
    );
}

#[tokio::test]
async fn extraction_prepare_rejects_stale_or_mixed_actor_authority_without_residue() {
    let directory = CorePrivateRouteActorDirectory::new();
    let lease = directory
        .register_actor(authenticated(), hall_seed(CHARACTER_ID, 1), 13)
        .expect("actor registers");
    let accepted = reach_boss_exit_ready(&directory, lease).await;

    let mut stale_state = accepted.clone();
    stale_state.state_version += 1;
    assert!(matches!(
        directory
            .prepare_extraction_terminal(
                lease,
                extraction_binding(stale_state, extraction_exit_binding()),
            )
            .await,
        Err(CorePrivateRouteRuntimeError::StaleRouteState)
    ));

    let mut wrong_route_content = accepted.clone();
    wrong_route_content.content_revision.records_blake3 = hash('1');
    assert!(matches!(
        directory
            .prepare_extraction_terminal(
                lease,
                extraction_binding(wrong_route_content, extraction_exit_binding()),
            )
            .await,
        Err(CorePrivateRouteRuntimeError::ContentAuthorityMismatch)
    ));

    let wrong_world_content = WorldFlowContentRevisionV1 {
        records_blake3: hash('2'),
        assets_blake3: hash('3'),
        localization_blake3: hash('4'),
    };
    let mixed_content = CorePrivateRouteExtractionBinding::new(
        ACCOUNT_ID,
        accepted.clone(),
        wrong_world_content,
        RESTORE_ID,
        extraction_exit_binding(),
    )
    .expect("mixed content is structurally valid but not actor authority");
    assert!(matches!(
        directory
            .prepare_extraction_terminal(lease, mixed_content)
            .await,
        Err(CorePrivateRouteRuntimeError::ContentAuthorityMismatch)
    ));

    let mut stale_generation = accepted.clone();
    stale_generation.actor_generation += 1;
    assert!(matches!(
        directory
            .prepare_extraction_terminal(
                lease,
                extraction_binding(stale_generation, extraction_exit_binding()),
            )
            .await,
        Err(CorePrivateRouteRuntimeError::StaleGeneration)
    ));

    let foreign_account = CorePrivateRouteExtractionBinding::new(
        [99; 16],
        accepted.clone(),
        world_revision(),
        RESTORE_ID,
        extraction_exit_binding(),
    )
    .expect("foreign account remains structurally valid");
    assert!(matches!(
        directory
            .prepare_extraction_terminal(lease, foreign_account)
            .await,
        Err(CorePrivateRouteRuntimeError::InvalidExtractionBinding)
    ));
    assert!(matches!(
        CorePrivateRouteExtractionExitBinding::new([7; 16], [7; 16], [9; 16], [10; 16], [11; 16]),
        Err(CorePrivateRouteRuntimeError::InvalidExtractionBinding)
    ));

    directory
        .prepare_extraction_terminal(
            lease,
            extraction_binding(accepted, extraction_exit_binding()),
        )
        .await
        .expect("rejected authority never strands a reservation or advances state");
    directory.begin_shutdown();
    let report = directory.finish_shutdown().await.expect("shutdown");
    assert!(report.zero_residue);
    assert_eq!(report.remaining_terminal_reservations, 0);
}

#[tokio::test]
async fn retirement_and_shutdown_invalidate_terminal_permits_and_drain_workers() {
    let directory = CorePrivateRouteActorDirectory::new();
    let lease = directory
        .register_actor(authenticated(), hall_seed(CHARACTER_ID, 1), 14)
        .expect("actor registers");
    let accepted = reach_boss_exit_ready(&directory, lease).await;
    let permit = directory
        .prepare_extraction_terminal(
            lease,
            extraction_binding(accepted, extraction_exit_binding()),
        )
        .await
        .expect("terminal reserves");
    directory
        .retire_actor(lease)
        .await
        .expect("terminal retirement invalidates the in-memory reservation");
    assert!(matches!(
        directory.abort_extraction_terminal(lease, &permit).await,
        Err(CorePrivateRouteRuntimeError::ActorUnavailable)
    ));
    assert!(matches!(
        directory
            .revalidate_extraction_terminal(lease, &permit)
            .await,
        Err(CorePrivateRouteRuntimeError::ActorUnavailable)
    ));
    let replacement = directory
        .register_actor(authenticated(), hall_seed(CHARACTER_ID, 3), 15)
        .expect("only a higher persistent generation replaces the retired actor");
    assert!(matches!(
        directory
            .revalidate_extraction_terminal(lease, &permit)
            .await,
        Err(CorePrivateRouteRuntimeError::StaleGeneration)
    ));
    let replacement_before = directory
        .snapshot(replacement)
        .expect("replacement projection");
    assert!(matches!(
        directory.abort_extraction_terminal(lease, &permit).await,
        Err(CorePrivateRouteRuntimeError::StaleGeneration)
    ));
    assert_eq!(
        directory
            .snapshot(replacement)
            .expect("stale permit cannot disturb replacement"),
        replacement_before
    );
    assert_eq!(replacement.actor_generation(), 15);
    directory.begin_shutdown();
    let report = directory.finish_shutdown().await.expect("shutdown");
    assert!(report.zero_residue);
    assert_eq!(report.remaining_terminal_reservations, 0);
    assert_eq!(report.remaining_actor_tasks, 0);

    let shutdown_directory = CorePrivateRouteActorDirectory::new();
    let shutdown_lease = shutdown_directory
        .register_actor(authenticated(), hall_seed(CHARACTER_ID, 1), 16)
        .expect("actor registers");
    let accepted = reach_boss_exit_ready(&shutdown_directory, shutdown_lease).await;
    let shutdown_permit = shutdown_directory
        .prepare_extraction_terminal(
            shutdown_lease,
            extraction_binding(accepted, extraction_exit_binding()),
        )
        .await
        .expect("terminal reserves");
    shutdown_directory.begin_shutdown();
    assert!(matches!(
        shutdown_directory
            .revalidate_extraction_terminal(shutdown_lease, &shutdown_permit)
            .await,
        Err(CorePrivateRouteRuntimeError::Retired)
    ));
    let report = shutdown_directory
        .finish_shutdown()
        .await
        .expect("shutdown");
    assert!(report.zero_residue);
    assert_eq!(report.remaining_terminal_reservations, 0);
    assert_eq!(report.remaining_actor_tasks, 0);
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
        directory.set_bell_portal_in_range(lease, false).await,
        Err(CorePrivateRouteRuntimeError::TransferInProgress)
    ));
    drop(permit);
    directory
        .set_bell_portal_in_range(lease, false)
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
    assert_eq!(report.remaining_terminal_reservations, 0);
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
