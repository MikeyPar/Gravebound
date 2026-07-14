use std::path::{Path, PathBuf};

use persistence::{
    CaldusExtractionCommit, CaldusExtractionRequest, CaldusVictoryExitCommit, PersistenceConfig,
    PersistenceError, PostgresPersistence, StoredCaldusVictoryOwner, StoredExtractionAuthority,
    StoredExtractionState, StoredWorldFlowRevisionV1, WIPEABLE_CORE_NAMESPACE,
};
use protocol::ManifestHash;
use server_app::{
    AccountId, AuthenticatedAccount, AuthenticatedNamespace, CaldusExtractionEvidenceCommand,
    CaldusInstancePresentation, CaldusVictoryCoordinatorError, CaldusVictoryOwnerCommand,
    IdentityClock, PostgresCaldusExtractionAuthority, PostgresCaldusHallTransferCoordinator,
    PostgresCaldusVictoryCoordinator, PostgresProgressionAwardService, PostgresRewardService,
    ProgressionAwardCode, ProgressionAwardEvidence, ProgressionAwardPayload, RewardGrantContext,
    RewardGrantTransaction, SecretRewardEpoch,
};
use sim_core::{
    CoreBossParticipant, CoreBossParticipantLock, CoreCaldusAntiCheatState,
    CoreCaldusDefeatPresence, CoreCaldusEligibilityEvidence, CoreCaldusRecallState,
    CoreCaldusSessionState, CoreCaldusVictoryIdentities, EncounterXpEvidence, EntityId,
    RewardLifeState, RewardRecallState, RewardTrustState,
};

const ACTIVE_TICKS: u32 = 5_400;
const CURRENT_TICK: u64 = 9_000;

#[derive(Debug, Clone, Copy)]
struct ExtractionClock;

impl IdentityClock for ExtractionClock {
    fn unix_millis(&self) -> u64 {
        20_000
    }
}

fn content_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
}

async fn disposable_database() -> PostgresPersistence {
    let config = PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL");
    let persistence = PostgresPersistence::connect(&config).await.unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence.migrate().await.unwrap();
    persistence
}

async fn reconnect_database() -> PostgresPersistence {
    let config = PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL");
    let persistence = PostgresPersistence::connect(&config).await.unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence
}

fn participant(entity: u64) -> CoreBossParticipant {
    CoreBossParticipant {
        entity_id: EntityId::new(entity).unwrap(),
        party_slot: 0,
    }
}

fn lock(entity: u64, attempt_ordinal: u32) -> CoreBossParticipantLock {
    CoreBossParticipantLock {
        attempt_ordinal,
        participants: vec![participant(entity)],
        maximum_health: 7_200,
    }
}

fn progression_revision() -> ManifestHash {
    let progression = sim_content::load_core_development_progression(&content_root()).unwrap();
    ManifestHash::new(progression.hashes().records_blake3.clone()).unwrap()
}

fn owner(account_id: [u8; 16], character_id: [u8; 16], entity: u64) -> CaldusVictoryOwnerCommand {
    let participant = participant(entity);
    CaldusVictoryOwnerCommand {
        participant,
        authenticated: AuthenticatedAccount {
            account_id: AccountId::new(account_id).unwrap(),
            namespace: AuthenticatedNamespace::WipeableTest,
        },
        character_id,
        expected_progression_version: 1,
        progression_content_revision: progression_revision(),
        eligibility: CoreCaldusEligibilityEvidence {
            participant,
            presence_ticks: ACTIVE_TICKS,
            direct_damage: 100,
            effective_healing_to_others: 0,
            damage_prevented_on_others: 0,
            objective_credits: 0,
            longest_inactivity_ticks: 0,
            defeat_presence: CoreCaldusDefeatPresence::AliveAndPresent,
            recall_state: CoreCaldusRecallState::Stayed,
            session_state: CoreCaldusSessionState::Valid,
            anti_cheat_state: CoreCaldusAntiCheatState::Valid,
        },
    }
}

fn progression_payload(command: &CaldusVictoryOwnerCommand) -> ProgressionAwardPayload {
    ProgressionAwardPayload {
        character_id: command.character_id,
        expected_progression_version: command.expected_progression_version,
        source_content_id: "boss.sir_caldus".to_owned(),
        progression_content_revision: command.progression_content_revision.clone(),
        evidence: ProgressionAwardEvidence::Encounter(EncounterXpEvidence {
            active_ticks: u64::from(ACTIVE_TICKS),
            present_ticks: u64::from(command.eligibility.presence_ticks),
            longest_inactivity_ticks: u64::from(command.eligibility.longest_inactivity_ticks),
            encounter_contribution_reference_health: 7_200,
            direct_damage: command.eligibility.direct_damage,
            effective_healing_to_others: command.eligibility.effective_healing_to_others,
            damage_prevented_on_others: command.eligibility.damage_prevented_on_others,
            qualifying_objective_credits: command.eligibility.objective_credits,
            life_state: RewardLifeState::Living,
            recall_state: RewardRecallState::Eligible,
            trust_state: RewardTrustState::Valid,
        }),
    }
}

fn services(
    persistence: &PostgresPersistence,
) -> (
    PostgresRewardService,
    PostgresProgressionAwardService,
    PostgresCaldusVictoryCoordinator,
) {
    let rewards = PostgresRewardService::load(
        persistence.clone(),
        &content_root(),
        SecretRewardEpoch::new("caldus-integration-v1", [0x5a; 32]).unwrap(),
    )
    .unwrap();
    let progression_content =
        sim_content::load_core_development_progression(&content_root()).unwrap();
    let oath_bargain = sim_content::load_core_development_oaths_bargains(&content_root()).unwrap();
    let progression = PostgresProgressionAwardService::new(
        persistence.clone(),
        &progression_content,
        &oath_bargain,
    )
    .unwrap();
    let coordinator = PostgresCaldusVictoryCoordinator::new(
        persistence.clone(),
        rewards.clone(),
        progression.clone(),
    );
    (rewards, progression, coordinator)
}

#[allow(
    clippy::too_many_lines,
    reason = "the normalized PostgreSQL fixture lists every required durable row explicitly"
)]
async fn reset_fixture(
    persistence: &PostgresPersistence,
    account_id: [u8; 16],
    character_id: [u8; 16],
    lineage_id: [u8; 16],
    lock: &CoreBossParticipantLock,
) {
    let identities = CoreCaldusVictoryIdentities::derive(lineage_id, lock).unwrap();
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query("DELETE FROM character_extraction_results WHERE namespace_id=$1 AND account_id=$2")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
    sqlx::query("DELETE FROM caldus_victory_exits WHERE namespace_id=$1 AND encounter_id=$2")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(identities.encounter_id.bytes().as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
    sqlx::query("DELETE FROM accounts WHERE namespace_id=$1 AND account_id=$2")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO accounts (namespace_id, account_id, state_version, slot_capacity) \
         VALUES ($1,$2,1,2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO characters (namespace_id, account_id, character_id, roster_ordinal, class_id, \
         level, oath_id, life_state, security_state, character_state_version) \
         VALUES ($1,$2,$3,1,'class.grave_arbalist',1,NULL,0,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE accounts SET selected_character_id=$1 WHERE namespace_id=$2 AND account_id=$3",
    )
    .bind(character_id.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_world_locations (namespace_id, account_id, character_id, \
         character_version, location_kind, location_content_id, safe_arrival_kind) \
         VALUES ($1,$2,$3,1,1,'hub.lantern_halls_01',0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_progression (namespace_id, account_id, character_id, total_xp, \
         level, current_health, progression_version) VALUES ($1,$2,$3,0,1,120,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_inventories (namespace_id, account_id, character_id, inventory_version) \
         VALUES ($1,$2,$3,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_oath_bargain_state (namespace_id, account_id, character_id, \
         earned_bargain_slots, oath_bargain_version) VALUES ($1,$2,$3,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ash_wallets (namespace_id, account_id, balance, wallet_version) \
         VALUES ($1,$2,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn exit_count(persistence: &PostgresPersistence, encounter_id: [u8; 16]) -> i64 {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let count = sqlx::query_scalar(
        "SELECT count(*) FROM caldus_victory_exits WHERE namespace_id=$1 AND encounter_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(encounter_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    count
}

fn world_flow_revision() -> protocol::WorldFlowContentRevisionV1 {
    let world = sim_content::load_core_development_world_flow(&content_root()).unwrap();
    protocol::WorldFlowContentRevisionV1 {
        records_blake3: ManifestHash::new(world.hashes().records_blake3.clone()).unwrap(),
        assets_blake3: ManifestHash::new(world.hashes().assets_blake3.clone()).unwrap(),
        localization_blake3: ManifestHash::new(world.hashes().localization_blake3.clone()).unwrap(),
    }
}

async fn stage_danger_binding(
    persistence: &PostgresPersistence,
    account_id: [u8; 16],
    character_id: [u8; 16],
    lineage_id: [u8; 16],
    restore_id: [u8; 16],
) {
    let world = sim_content::load_core_development_world_flow(&content_root()).unwrap();
    let hashes = world.hashes();
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO character_instance_lineages (namespace_id,account_id,character_id,
         lineage_id,content_id,layout_id,lineage_state,records_blake3,assets_blake3,
         localization_blake3) VALUES ($1,$2,$3,$4,'world.core_microrealm_01',
         'layout.core_private_life_01',0,$5,$6,$7)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(lineage_id.as_slice())
    .bind(&hashes.records_blake3)
    .bind(&hashes.assets_blake3)
    .bind(&hashes.localization_blake3)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_entry_restore_points (namespace_id,account_id,character_id,
         restore_point_id,lineage_id,source_location_id,restore_location_id,
         snapshot_contract_version,account_version,character_version,progression_version,
         inventory_version,oath_bargain_version,component_mask,composite_digest,restore_state,
         records_blake3,assets_blake3,localization_blake3)
         VALUES ($1,$2,$3,$4,$5,'hub.lantern_halls_01','hub.lantern_halls_01',1,
         1,1,1,1,1,7,$6,0,$7,$8,$9)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(restore_id.as_slice())
    .bind(lineage_id.as_slice())
    .bind([91_u8; 32].as_slice())
    .bind(&hashes.records_blake3)
    .bind(&hashes.assets_blake3)
    .bind(&hashes.localization_blake3)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE characters SET character_state_version=2 WHERE namespace_id=$1
         AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_world_locations SET character_version=2,location_kind=2,
         location_content_id='world.core_microrealm_01',safe_arrival_kind=NULL,
         instance_lineage_id=$1,entry_restore_point_id=$2 WHERE namespace_id=$3
         AND account_id=$4 AND character_id=$5",
    )
    .bind(lineage_id.as_slice())
    .bind(restore_id.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_danger_checkpoints (namespace_id,account_id,character_id,
         lineage_id,checkpoint_tick,component_mask,composite_digest,character_version,
         progression_version,inventory_version,oath_bargain_version,records_blake3,
         assets_blake3,localization_blake3,checkpoint_schema_version,checkpoint_payload,
         checkpoint_payload_digest) VALUES ($1,$2,$3,$4,30,15,$5,2,1,1,1,$6,$7,$8,1,$9,$10)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(lineage_id.as_slice())
    .bind([92_u8; 32].as_slice())
    .bind(&hashes.records_blake3)
    .bind(&hashes.assets_blake3)
    .bind(&hashes.localization_blake3)
    .bind([1_u8].as_slice())
    .bind([93_u8; 32].as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

fn extraction_transfer(
    mutation_id: [u8; 16],
    character_id: [u8; 16],
    request_id: [u8; 16],
    receipt_id: [u8; 16],
) -> protocol::WorldTransferMutation {
    let payload = protocol::WorldTransferPayload {
        content_revision: world_flow_revision(),
        command: protocol::WorldTransferCommand::UseCommittedExtraction {
            portal_id: protocol::WireText::new("portal.exit.dungeon.bell_sepulcher").unwrap(),
            extraction_request_id: request_id,
            extraction_receipt_id: receipt_id,
        },
    };
    protocol::WorldTransferMutation {
        mutation_id,
        character_id,
        expected_character_version: 2,
        issued_at_unix_millis: 10_000,
        payload_hash: payload.canonical_hash(),
        payload,
    }
}

fn extraction_request(
    account_id: [u8; 16],
    character_id: [u8; 16],
    lineage_id: [u8; 16],
    restore_id: [u8; 16],
    lock: &CoreBossParticipantLock,
) -> CaldusExtractionRequest {
    let identities = CoreCaldusVictoryIdentities::derive(lineage_id, lock).unwrap();
    let extraction = identities.extraction_for(lock.participants[0]).unwrap();
    let revision = world_flow_revision();
    CaldusExtractionRequest {
        account_id,
        character_id,
        extraction_request_id: extraction.request_id.bytes(),
        encounter_id: identities.encounter_id.bytes(),
        instance_lineage_id: lineage_id,
        entry_restore_point_id: restore_id,
        exit_instance_id: identities.exit_instance_id.bytes(),
        attempt_ordinal: lock.attempt_ordinal,
        party_slot: lock.participants[0].party_slot,
        participant_entity_id: lock.participants[0].entity_id.get(),
        expected_character_version: 2,
        content_revision: StoredWorldFlowRevisionV1 {
            records_blake3: revision.records_blake3.as_str().to_owned(),
            assets_blake3: revision.assets_blake3.as_str().to_owned(),
            localization_blake3: revision.localization_blake3.as_str().to_owned(),
        },
    }
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn caldus_victory_fresh_replay_and_payload_conflict_are_durable() {
    let persistence = disposable_database().await;
    let account_id = [141; 16];
    let character_id = [142; 16];
    let lineage_id = [143; 16];
    let lock = lock(144, 1);
    reset_fixture(&persistence, account_id, character_id, lineage_id, &lock).await;
    let (_, _, coordinator) = services(&persistence);
    let owner = owner(account_id, character_id, 144);

    let fresh = coordinator
        .commit(
            lineage_id,
            &lock,
            ACTIVE_TICKS,
            CURRENT_TICK,
            std::slice::from_ref(&owner),
        )
        .await
        .unwrap();
    assert!(!fresh.exit.replayed);
    assert_eq!(fresh.owners.len(), 1);
    let RewardGrantTransaction::Fresh { result, .. } = &fresh.owners[0].reward else {
        panic!("first Caldus reward must be fresh")
    };
    assert_eq!(result.items.len(), 4);
    assert_eq!(
        result
            .items
            .iter()
            .filter(|item| item.template_id == "consumable.red_tonic")
            .count(),
        2
    );
    let equipment = result
        .items
        .iter()
        .filter(|item| item.template_id != "consumable.red_tonic")
        .collect::<Vec<_>>();
    assert_eq!(equipment.len(), 2);
    assert!(
        equipment
            .iter()
            .all(|item| matches!(item.item_level, Some(8..=10)))
    );
    assert_eq!(fresh.owners[0].progression.base_xp, 450);
    assert_eq!(fresh.owners[0].progression.first_clear_bonus_xp, 225);

    let replay = coordinator
        .commit(
            lineage_id,
            &lock,
            ACTIVE_TICKS,
            CURRENT_TICK,
            std::slice::from_ref(&owner),
        )
        .await
        .unwrap();
    assert!(replay.exit.replayed);
    assert!(matches!(
        replay.owners[0].reward,
        RewardGrantTransaction::Replay { .. }
    ));
    assert_eq!(fresh.exit.exit_instance_id, replay.exit.exit_instance_id);
    assert_eq!(
        fresh.exit.canonical_request_hash,
        replay.exit.canonical_request_hash
    );

    let conflict = coordinator
        .commit(lineage_id, &lock, ACTIVE_TICKS - 1, CURRENT_TICK, &[owner])
        .await
        .unwrap_err();
    assert!(matches!(
        conflict,
        CaldusVictoryCoordinatorError::ProgressionNotCommitted(
            ProgressionAwardCode::IdempotencyConflict
        )
    ));
    assert_eq!(
        exit_count(&persistence, fresh.identities.encounter_id.bytes()).await,
        1
    );
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn caldus_victory_partial_item_terminal_blocks_exit_then_converges() {
    let persistence = disposable_database().await;
    let account_id = [151; 16];
    let character_id = [152; 16];
    let lineage_id = [153; 16];
    let lock = lock(154, 1);
    reset_fixture(&persistence, account_id, character_id, lineage_id, &lock).await;
    let (rewards, _, coordinator) = services(&persistence);
    let owner = owner(account_id, character_id, 154);
    let identities = CoreCaldusVictoryIdentities::derive(lineage_id, &lock).unwrap();
    let reward_request_id = identities.reward_for(owner.participant).unwrap().bytes();
    let RewardGrantTransaction::Fresh { durable, .. } = rewards
        .grant(RewardGrantContext {
            reward_request_id,
            account_id,
            character_id,
            source_instance_id: identities.encounter_id.bytes(),
            reward_table_id: "reward.boss_caldus",
            current_tick: CURRENT_TICK,
        })
        .await
        .unwrap()
    else {
        panic!("partial fixture must commit only the item terminal")
    };
    let partial_exit = CaldusVictoryExitCommit {
        encounter_id: identities.encounter_id.bytes(),
        instance_lineage_id: lineage_id,
        attempt_ordinal: lock.attempt_ordinal,
        exit_instance_id: identities.exit_instance_id.bytes(),
        owners: vec![StoredCaldusVictoryOwner {
            party_slot: owner.participant.party_slot,
            participant_entity_id: owner.participant.entity_id.get(),
            account_id,
            character_id,
            reward_request_id,
            reward_result_hash: durable.result_hash,
            progression_payload_hash: progression_payload(&owner).canonical_hash(),
        }],
    };
    assert!(matches!(
        persistence.commit_caldus_victory_exit(&partial_exit).await,
        Err(PersistenceError::CaldusRewardNotTerminal)
    ));
    assert_eq!(
        exit_count(&persistence, identities.encounter_id.bytes()).await,
        0
    );

    let recovered = coordinator
        .commit(lineage_id, &lock, ACTIVE_TICKS, CURRENT_TICK, &[owner])
        .await
        .unwrap();
    assert!(!recovered.exit.replayed);
    assert!(matches!(
        recovered.owners[0].reward,
        RewardGrantTransaction::Replay { .. }
    ));
    assert_eq!(recovered.owners[0].progression.base_xp, 450);
    assert_eq!(
        exit_count(&persistence, identities.encounter_id.bytes()).await,
        1
    );
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
#[allow(
    clippy::too_many_lines,
    reason = "the end-to-end transaction trace keeps ordered durable assertions together"
)]
async fn caldus_committed_receipt_supersedes_restore_and_transfers_once_to_hall_default() {
    let persistence = disposable_database().await;
    let account_id = [161; 16];
    let character_id = [162; 16];
    let lineage_id = [163; 16];
    let restore_id = [164; 16];
    let lock = lock(165, 1);
    reset_fixture(&persistence, account_id, character_id, lineage_id, &lock).await;
    stage_danger_binding(
        &persistence,
        account_id,
        character_id,
        lineage_id,
        restore_id,
    )
    .await;
    let (_, _, victory_coordinator) = services(&persistence);
    let owner = owner(account_id, character_id, 165);
    let victory = victory_coordinator
        .commit(
            lineage_id,
            &lock,
            ACTIVE_TICKS,
            CURRENT_TICK,
            std::slice::from_ref(&owner),
        )
        .await
        .unwrap();
    let caldus_content = sim_content::load_core_development_caldus(&content_root()).unwrap();
    let mut presentation = CaldusInstancePresentation::new(lineage_id, 1).unwrap();
    victory
        .present_exit(&caldus_content, &mut presentation)
        .unwrap();
    let extraction = PostgresCaldusExtractionAuthority::new(persistence.clone());
    let committed = extraction
        .request_and_commit_wipeable_evidence(
            &presentation,
            &lock,
            &CaldusExtractionEvidenceCommand {
                authenticated: owner.authenticated,
                character_id,
                participant: owner.participant,
                instance_lineage_id: lineage_id,
                entry_restore_point_id: restore_id,
                expected_character_version: 2,
                content_revision: world_flow_revision(),
            },
        )
        .await
        .unwrap();
    let request_id = committed.request.request.extraction_request_id;
    let receipt_id = committed.receipt.extraction_receipt_id.unwrap();
    let before_transfer_inventory: i64 = {
        let mut transaction = persistence.begin_transaction().await.unwrap();
        let version = sqlx::query_scalar(
            "SELECT inventory_version FROM character_inventories WHERE namespace_id=$1
             AND account_id=$2 AND character_id=$3",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .fetch_one(transaction.connection())
        .await
        .unwrap();
        transaction.rollback().await.unwrap();
        version
    };
    drop(extraction);
    let restarted_persistence = reconnect_database().await;
    let hall = PostgresCaldusHallTransferCoordinator::new(
        restarted_persistence,
        ExtractionClock,
        world_flow_revision(),
    );
    let mut wrong_receipt = receipt_id;
    wrong_receipt[0] ^= 1;
    let rejected = hall
        .transfer(
            owner.authenticated,
            1,
            &extraction_transfer([201; 16], character_id, request_id, wrong_receipt),
        )
        .await;
    assert!(matches!(
        rejected,
        protocol::WorldFlowResult::Transfer {
            code: protocol::WorldTransferResultCode::InvalidSource,
            ..
        }
    ));
    let mutation = extraction_transfer([202; 16], character_id, request_id, receipt_id);
    let accepted = hall.transfer(owner.authenticated, 2, &mutation).await;
    let protocol::WorldFlowResult::Transfer {
        accepted: true,
        code: protocol::WorldTransferResultCode::Accepted,
        snapshot: Some(snapshot),
        transfer_id: Some(transfer_id),
        ..
    } = &accepted
    else {
        panic!("committed extraction must transfer to Hall")
    };
    assert_eq!(*transfer_id, receipt_id);
    assert_eq!(snapshot.character_version, 3);
    assert!(matches!(
        snapshot.location,
        protocol::CharacterLocation::Safe {
            ref location_id,
            arrival: protocol::SafeArrival::HallDefault,
        } if location_id.as_str() == "hub.lantern_halls_01"
    ));
    let replay = hall.transfer(owner.authenticated, 3, &mutation).await;
    assert!(matches!(
        replay,
        protocol::WorldFlowResult::Transfer {
            request_sequence: 3,
            accepted: true,
            code: protocol::WorldTransferResultCode::Accepted,
            transfer_id: Some(id),
            ..
        } if id == receipt_id
    ));

    let mut verification = persistence.begin_transaction().await.unwrap();
    let terminal: (i16, i16, i16, i64, Option<Vec<u8>>) = sqlx::query_as(
        "SELECT r.restore_state,l.lineage_state,w.location_kind,i.inventory_version,
                x.transfer_mutation_id
         FROM character_entry_restore_points r
         JOIN character_instance_lineages l USING (namespace_id,account_id,character_id,lineage_id)
         JOIN character_world_locations w USING (namespace_id,account_id,character_id)
         JOIN character_inventories i USING (namespace_id,account_id,character_id)
         JOIN character_extraction_results x ON x.namespace_id=r.namespace_id
              AND x.account_id=r.account_id AND x.character_id=r.character_id
              AND x.entry_restore_point_id=r.restore_point_id
         WHERE r.namespace_id=$1 AND r.account_id=$2 AND r.character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    assert_eq!((terminal.0, terminal.1, terminal.2), (1, 2, 1));
    assert_eq!(terminal.3, before_transfer_inventory);
    assert_eq!(terminal.4, Some([202_u8; 16].to_vec()));
    let checkpoint_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM character_danger_checkpoints WHERE namespace_id=$1
         AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    assert_eq!(checkpoint_count, 0);
    verification.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn crash_restore_between_request_and_receipt_supersedes_caldus_extraction() {
    let persistence = disposable_database().await;
    let account_id = [171; 16];
    let character_id = [172; 16];
    let lineage_id = [173; 16];
    let restore_id = [174; 16];
    let lock = lock(175, 1);
    reset_fixture(&persistence, account_id, character_id, lineage_id, &lock).await;
    stage_danger_binding(
        &persistence,
        account_id,
        character_id,
        lineage_id,
        restore_id,
    )
    .await;
    let (_, _, coordinator) = services(&persistence);
    coordinator
        .commit(
            lineage_id,
            &lock,
            ACTIVE_TICKS,
            CURRENT_TICK,
            std::slice::from_ref(&owner(account_id, character_id, 175)),
        )
        .await
        .unwrap();

    let request = extraction_request(account_id, character_id, lineage_id, restore_id, &lock);
    let requested = persistence
        .request_caldus_extraction(&request)
        .await
        .unwrap();
    assert!(matches!(
        requested,
        persistence::CaldusExtractionTransaction::Fresh(ref result)
            if result.state == StoredExtractionState::Requested
    ));

    let mut restore = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "UPDATE character_entry_restore_points SET restore_state=2,
         consumed_at=transaction_timestamp() WHERE namespace_id=$1 AND account_id=$2
         AND character_id=$3 AND restore_point_id=$4 AND restore_state=0",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(restore_id.as_slice())
    .execute(restore.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_instance_lineages SET lineage_state=2,closed_at=transaction_timestamp()
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND lineage_id=$4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(lineage_id.as_slice())
    .execute(restore.connection())
    .await
    .unwrap();
    restore.commit().await.unwrap();

    let identities = CoreCaldusVictoryIdentities::derive(lineage_id, &lock).unwrap();
    let extraction = identities.extraction_for(lock.participants[0]).unwrap();
    assert!(matches!(
        persistence
            .commit_caldus_extraction(CaldusExtractionCommit {
                extraction_request_id: extraction.request_id.bytes(),
                extraction_receipt_id: extraction.receipt_id.bytes(),
                authority: StoredExtractionAuthority::WipeableTestEvidence,
            })
            .await,
        Err(PersistenceError::ExtractionSuperseded)
    ));
    let replay = persistence
        .request_caldus_extraction(&request)
        .await
        .unwrap();
    assert!(matches!(
        replay,
        persistence::CaldusExtractionTransaction::Replay(ref result)
            if result.state == StoredExtractionState::Requested
                && result.extraction_receipt_id.is_none()
    ));
}
