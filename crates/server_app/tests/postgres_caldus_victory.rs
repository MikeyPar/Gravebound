use std::path::{Path, PathBuf};

use persistence::{
    CaldusVictoryExitCommit, PersistenceConfig, PersistenceError, PostgresPersistence,
    StoredCaldusVictoryOwner, WIPEABLE_CORE_NAMESPACE,
};
use protocol::ManifestHash;
use server_app::{
    AccountId, AuthenticatedAccount, AuthenticatedNamespace, CaldusVictoryCoordinatorError,
    CaldusVictoryOwnerCommand, PostgresCaldusVictoryCoordinator, PostgresProgressionAwardService,
    PostgresRewardService, ProgressionAwardCode, ProgressionAwardEvidence, ProgressionAwardPayload,
    RewardGrantContext, RewardGrantTransaction, SecretRewardEpoch,
};
use sim_core::{
    CoreBossParticipant, CoreBossParticipantLock, CoreCaldusAntiCheatState,
    CoreCaldusDefeatPresence, CoreCaldusEligibilityEvidence, CoreCaldusRecallState,
    CoreCaldusSessionState, CoreCaldusVictoryIdentities, EncounterXpEvidence, EntityId,
    RewardLifeState, RewardRecallState, RewardTrustState,
};

const ACTIVE_TICKS: u32 = 5_400;
const CURRENT_TICK: u64 = 9_000;

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

async fn reset_fixture(
    persistence: &PostgresPersistence,
    account_id: [u8; 16],
    character_id: [u8; 16],
    lineage_id: [u8; 16],
    lock: &CoreBossParticipantLock,
) {
    let identities = CoreCaldusVictoryIdentities::derive(lineage_id, lock).unwrap();
    let mut transaction = persistence.begin_transaction().await.unwrap();
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
