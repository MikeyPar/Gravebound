use std::path::{Path, PathBuf};

use persistence::{
    CaldusExtractionCommit, CaldusExtractionRequest, CaldusVictoryExitCommit, PersistenceConfig,
    PersistenceError, PostgresPersistence, ProductionExtractionExpectedVersionsV1,
    StoredCaldusVictoryOwner, StoredExtractionAuthority, StoredExtractionState,
    StoredWorldFlowRevisionV1, WIPEABLE_CORE_NAMESPACE,
};
use protocol::{
    CorePrivateRouteContentRevisionV1, CorePrivateRoutePhaseV1, CorePrivateRouteRoomV1,
    CorePrivateRouteSceneV1, ExtractionCommitFrameV1, ExtractionCommitPayloadV1,
    ExtractionCommitResultV1, ManifestHash, TERMINAL_INVENTORY_SCHEMA_VERSION,
    TerminalExpectedVersionsV1, TerminalInventoryRejectionCodeV1,
};
use server_app::{
    AccountId, AuthenticatedAccount, AuthenticatedNamespace, CaldusExtractionEvidenceCommand,
    CaldusInstancePresentation, CaldusVictoryCoordinatorError, CaldusVictoryOwnerCommand,
    CorePrivateRouteActorDirectory, CorePrivateRouteActorPosition, CorePrivateRouteActorSeed,
    CorePrivateRouteExtractionBinding, CorePrivateRouteExtractionExitBinding, EntryCaptureContext,
    EntryRestoreProvider, IdentityClock, PostgresCaldusExtractionAuthority,
    PostgresCaldusHallTransferCoordinator, PostgresCaldusVictoryCoordinator,
    PostgresProgressionAwardService, PostgresProgressionRestoreProvider, PostgresRewardService,
    ProductionExtractionBossExitAuthorityV1, ProductionExtractionIntentActor, ProgressionAwardCode,
    ProgressionAwardEvidence, ProgressionAwardPayload, RewardGrantContext, RewardGrantTransaction,
    SecretRewardEpoch, derive_production_extraction_terminal_id_v1,
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

fn restore_id(lineage_id: [u8; 16]) -> [u8; 16] {
    [lineage_id[0].wrapping_add(1); 16]
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
    stage_danger_binding(
        persistence,
        account_id,
        character_id,
        lineage_id,
        restore_id(lineage_id),
    )
    .await;
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

fn private_route_revision() -> CorePrivateRouteContentRevisionV1 {
    let route = sim_content::load_core_private_life_content(&content_root()).unwrap();
    CorePrivateRouteContentRevisionV1 {
        records_blake3: ManifestHash::new(route.revision().records_blake3.clone()).unwrap(),
        assets_blake3: ManifestHash::new(route.revision().assets_blake3.clone()).unwrap(),
        localization_blake3: ManifestHash::new(route.revision().localization_blake3.clone())
            .unwrap(),
    }
}

fn stored_world_flow_revision() -> StoredWorldFlowRevisionV1 {
    let revision = world_flow_revision();
    StoredWorldFlowRevisionV1 {
        records_blake3: revision.records_blake3.as_str().to_owned(),
        assets_blake3: revision.assets_blake3.as_str().to_owned(),
        localization_blake3: revision.localization_blake3.as_str().to_owned(),
    }
}

fn live_extraction_frame(
    sequence: u32,
    mutation_id: [u8; 16],
    character_id: [u8; 16],
    extraction_request_id: [u8; 16],
    versions: ProductionExtractionExpectedVersionsV1,
) -> ExtractionCommitFrameV1 {
    let payload = ExtractionCommitPayloadV1 {
        extraction_request_id,
        expected_versions: TerminalExpectedVersionsV1 {
            account: versions.account,
            character: versions.character,
            world: versions.world,
            inventory: versions.inventory,
            life_clock: versions.life_metrics,
        },
        content_revision: world_flow_revision(),
    };
    ExtractionCommitFrameV1 {
        schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
        sequence,
        mutation_id,
        character_id,
        issued_at_unix_millis: 10_000,
        payload_hash: payload.canonical_hash(),
        payload,
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "the hosted fixture constructs one complete V3 danger binding for terminal-race tests"
)]
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
         'layout.core_private_life_01',1,$5,$6,$7)",
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
         inventory_version,oath_bargain_version,life_metrics_version,ash_wallet_version,
         component_mask,composite_digest,restore_state,
         records_blake3,assets_blake3,localization_blake3)
         VALUES ($1,$2,$3,$4,$5,'hub.lantern_halls_01','hub.lantern_halls_01',3,
         1,1,1,1,1,1,1,31,$6,0,$7,$8,$9)",
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
    let progression_content =
        sim_content::load_core_development_progression(&content_root()).unwrap();
    PostgresProgressionRestoreProvider::new(&progression_content)
        .unwrap()
        .capture(
            &mut transaction,
            EntryCaptureContext {
                account_id,
                character_id,
                restore_point_id: restore_id,
                mutation_id: [88; 16],
                safe_placement_count: 0,
            },
        )
        .await
        .unwrap();
    let inventory = persistence::stage_danger_entry_inventory_restore_v3(
        &mut transaction,
        account_id,
        character_id,
        restore_id,
        [88; 16],
        0,
    )
    .await
    .unwrap();
    persistence::stage_danger_entry_oath_bargain_restore_v3(
        &mut transaction,
        account_id,
        character_id,
        restore_id,
    )
    .await
    .unwrap();
    persistence::stage_danger_entry_life_metrics_restore_v3(
        &mut transaction,
        account_id,
        character_id,
        restore_id,
    )
    .await
    .unwrap();
    persistence::stage_danger_entry_ash_wallet_restore_v3(
        &mut transaction,
        account_id,
        character_id,
        restore_id,
    )
    .await
    .unwrap();
    assert_eq!(inventory.pre_inventory_version, 1);
    assert_eq!(inventory.post_inventory_version, 1);
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

async fn close_danger_authority(
    persistence: &PostgresPersistence,
    account_id: [u8; 16],
    character_id: [u8; 16],
    lineage_id: [u8; 16],
    restore_id: [u8; 16],
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query("SELECT 1 FROM accounts WHERE namespace_id=$1 AND account_id=$2 FOR UPDATE")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
    sqlx::query(
        "UPDATE character_entry_restore_points SET restore_state=1,
         consumed_at=transaction_timestamp() WHERE namespace_id=$1 AND account_id=$2
         AND character_id=$3 AND restore_point_id=$4 AND restore_state=0",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(restore_id.as_slice())
    .execute(transaction.connection())
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
        content_revision: stored_world_flow_revision(),
    }
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
#[expect(
    clippy::too_many_lines,
    reason = "one hosted custody journey proves active-lineage, content, ground, and material fail-closed boundaries"
)]
async fn current_danger_snapshot_reads_exact_core_pending_ground_custody() {
    let persistence = disposable_database().await;
    let account_id = [221; 16];
    let character_id = [222; 16];
    let lineage_id = [223; 16];
    let restore_id = [224; 16];
    let lock = lock(225, 1);
    reset_fixture(&persistence, account_id, character_id, lineage_id, &lock).await;

    let mut fixture = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO item_instances
         (namespace_id,item_uid,account_id,character_id,template_id,content_revision,
          item_kind,item_level,rarity,creation_kind,creation_request_id,roll_index,
          unit_ordinal,item_version,security_state,location_kind,instance_id,pickup_id,
          expires_at_tick,provenance_kind,salvage_band,salvage_value)
         VALUES ($1,$2,$3,$4,'item.charm.ember_tooth.t1',$5,
          0,1,0,1,$2,0,0,1,2,3,$6,$7,31000,1,1,4)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind([226_u8; 16].as_slice())
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(persistence::CORE_ITEM_CONTENT_REVISION)
    .bind([227_u8; 16].as_slice())
    .bind([228_u8; 16].as_slice())
    .execute(fixture.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_inventories SET inventory_version=2
          WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
            AND inventory_version=1",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(fixture.connection())
    .await
    .unwrap();
    fixture.commit().await.unwrap();

    let authority = persistence::StoredActiveDangerAuthorityV1 {
        account_id,
        character_id,
        instance_lineage_id: lineage_id,
        entry_restore_point_id: restore_id,
    };
    let snapshot = persistence
        .load_current_danger_extraction_snapshot_v1(authority, &stored_world_flow_revision())
        .await
        .unwrap();
    assert_eq!(snapshot.expected_versions.inventory, 2);
    assert!(snapshot.pending_items.iter().any(|item| {
        matches!(
            item.location,
            persistence::StoredCurrentDangerPendingItemLocationV1::PersonalGround {
                instance_id,
                pickup_id,
                expires_at_tick: 31_000,
            } if instance_id == [227; 16] && pickup_id == [228; 16]
        )
    }));
    assert!(snapshot.pending_materials.is_empty());

    let mut wrong_revision = stored_world_flow_revision();
    wrong_revision.records_blake3 = "d".repeat(64);
    assert!(matches!(
        persistence
            .load_current_danger_extraction_snapshot_v1(authority, &wrong_revision)
            .await,
        Err(PersistenceError::CurrentDangerExtractionSnapshotContentMismatch)
    ));

    let mut staged = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "UPDATE character_instance_lineages SET lineage_state=0
          WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND lineage_id=$4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(lineage_id.as_slice())
    .execute(staged.connection())
    .await
    .unwrap();
    staged.commit().await.unwrap();
    assert!(matches!(
        persistence
            .load_current_danger_extraction_snapshot_v1(authority, &stored_world_flow_revision())
            .await,
        Err(PersistenceError::CurrentDangerExtractionSnapshotBindingMismatch)
    ));
    let mut active = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "UPDATE character_instance_lineages SET lineage_state=1
          WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND lineage_id=$4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(lineage_id.as_slice())
    .execute(active.connection())
    .await
    .unwrap();
    active.commit().await.unwrap();

    let mut non_core = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO character_run_material_stacks
         (namespace_id,account_id,character_id,material_id,quantity,
          material_version,security_state)
         VALUES ($1,$2,$3,'material.bell_brass',3,1,2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(non_core.connection())
    .await
    .unwrap();
    non_core.commit().await.unwrap();
    assert!(matches!(
        persistence
            .load_current_danger_extraction_snapshot_v1(authority, &stored_world_flow_revision())
            .await,
        Err(PersistenceError::CurrentDangerExtractionSnapshotContentMismatch)
    ));
    persistence.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn caldus_victory_fresh_replay_and_payload_conflict_are_durable() {
    let persistence = disposable_database().await;
    let account_id = [231; 16];
    let character_id = [232; 16];
    let lineage_id = [233; 16];
    let lock = lock(235, 1);
    reset_fixture(&persistence, account_id, character_id, lineage_id, &lock).await;
    let (_, _, coordinator) = services(&persistence);
    let owner = owner(account_id, character_id, 235);

    let fresh = coordinator
        .commit(
            lineage_id,
            restore_id(lineage_id),
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
            restore_id(lineage_id),
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

    let mut conflicting_owner = owner;
    conflicting_owner.eligibility.direct_damage += 1;
    let conflict = coordinator
        .commit(
            lineage_id,
            restore_id(lineage_id),
            &lock,
            ACTIVE_TICKS,
            CURRENT_TICK,
            &[conflicting_owner],
        )
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
        danger_authorities: vec![persistence::StoredActiveDangerAuthorityV1 {
            account_id,
            character_id,
            instance_lineage_id: lineage_id,
            entry_restore_point_id: restore_id(lineage_id),
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
        .commit(
            lineage_id,
            restore_id(lineage_id),
            &lock,
            ACTIVE_TICKS,
            CURRENT_TICK,
            &[owner],
        )
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
async fn terminal_winner_blocks_fresh_caldus_progression_and_exit_but_not_reward_replay() {
    let persistence = disposable_database().await;
    let account_id = [201; 16];
    let character_id = [202; 16];
    let lineage_id = [203; 16];
    let lock = lock(204, 1);
    reset_fixture(&persistence, account_id, character_id, lineage_id, &lock).await;
    let (rewards, _, coordinator) = services(&persistence);
    let owner = owner(account_id, character_id, 204);
    let identities = CoreCaldusVictoryIdentities::derive(lineage_id, &lock).unwrap();
    let reward_request_id = identities.reward_for(owner.participant).unwrap().bytes();
    let context = RewardGrantContext {
        reward_request_id,
        account_id,
        character_id,
        source_instance_id: identities.encounter_id.bytes(),
        reward_table_id: "reward.boss_caldus",
        current_tick: CURRENT_TICK,
    };
    let authority = persistence::StoredActiveDangerAuthorityV1 {
        account_id,
        character_id,
        instance_lineage_id: lineage_id,
        entry_restore_point_id: restore_id(lineage_id),
    };
    assert!(matches!(
        rewards
            .grant_in_active_danger(context, authority)
            .await
            .unwrap(),
        RewardGrantTransaction::Fresh { .. }
    ));
    close_danger_authority(
        &persistence,
        account_id,
        character_id,
        lineage_id,
        restore_id(lineage_id),
    )
    .await;
    assert!(matches!(
        rewards
            .grant_in_active_danger(context, authority)
            .await
            .unwrap(),
        RewardGrantTransaction::Replay { .. }
    ));
    assert!(matches!(
        coordinator
            .commit(
                lineage_id,
                restore_id(lineage_id),
                &lock,
                ACTIVE_TICKS,
                CURRENT_TICK,
                &[owner],
            )
            .await,
        Err(CaldusVictoryCoordinatorError::Persistence(
            PersistenceError::ActiveDangerAuthoritySuperseded
        ))
    ));
    assert_eq!(
        exit_count(&persistence, identities.encounter_id.bytes()).await,
        0
    );
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn complete_caldus_result_replays_exactly_after_terminal_winner() {
    let persistence = disposable_database().await;
    let account_id = [211; 16];
    let character_id = [212; 16];
    let lineage_id = [213; 16];
    let lock = lock(214, 1);
    reset_fixture(&persistence, account_id, character_id, lineage_id, &lock).await;
    let (_, _, coordinator) = services(&persistence);
    let owner = owner(account_id, character_id, 214);
    let fresh = coordinator
        .commit(
            lineage_id,
            restore_id(lineage_id),
            &lock,
            ACTIVE_TICKS,
            CURRENT_TICK,
            std::slice::from_ref(&owner),
        )
        .await
        .unwrap();
    close_danger_authority(
        &persistence,
        account_id,
        character_id,
        lineage_id,
        restore_id(lineage_id),
    )
    .await;
    let replay = coordinator
        .commit(
            lineage_id,
            restore_id(lineage_id),
            &lock,
            ACTIVE_TICKS,
            CURRENT_TICK,
            &[owner],
        )
        .await
        .unwrap();
    assert!(replay.exit.replayed);
    assert!(matches!(
        replay.owners[0].reward,
        RewardGrantTransaction::Replay { .. }
    ));
    assert_eq!(
        fresh.exit.canonical_request_hash,
        replay.exit.canonical_request_hash
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
    let (_, _, victory_coordinator) = services(&persistence);
    let owner = owner(account_id, character_id, 165);
    let victory = victory_coordinator
        .commit(
            lineage_id,
            restore_id,
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
    let mismatched_lineage_id = [199_u8; 16];
    let hashes = sim_content::load_core_development_world_flow(&content_root())
        .unwrap()
        .hashes()
        .clone();
    let mut mismatch = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO character_instance_lineages (namespace_id,account_id,character_id,
         lineage_id,content_id,layout_id,lineage_state,closed_at,records_blake3,assets_blake3,
         localization_blake3) VALUES ($1,$2,$3,$4,'world.core_microrealm_01',
         'layout.core_private_life_01',2,transaction_timestamp(),$5,$6,$7)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(mismatched_lineage_id.as_slice())
    .bind(&hashes.records_blake3)
    .bind(&hashes.assets_blake3)
    .bind(&hashes.localization_blake3)
    .execute(mismatch.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_danger_checkpoints SET lineage_id=$1 WHERE namespace_id=$2
         AND account_id=$3 AND character_id=$4",
    )
    .bind(mismatched_lineage_id.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(mismatch.connection())
    .await
    .unwrap();
    mismatch.commit().await.unwrap();

    let mutation = extraction_transfer([202; 16], character_id, request_id, receipt_id);
    let rolled_back = hall.transfer(owner.authenticated, 2, &mutation).await;
    assert!(matches!(
        rolled_back,
        protocol::WorldFlowResult::Transfer {
            code: protocol::WorldTransferResultCode::ServiceUnavailable,
            ..
        }
    ));
    let mut rollback_check = persistence.begin_transaction().await.unwrap();
    let rollback_state: (Option<Vec<u8>>, i16) = sqlx::query_as(
        "SELECT x.transfer_mutation_id,w.location_kind FROM character_extraction_results x
         JOIN character_world_locations w USING (namespace_id,account_id,character_id)
         WHERE x.namespace_id=$1 AND x.extraction_request_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request_id.as_slice())
    .fetch_one(rollback_check.connection())
    .await
    .unwrap();
    rollback_check.rollback().await.unwrap();
    assert_eq!(rollback_state, (None, 2));

    let mut repair = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "UPDATE character_danger_checkpoints SET lineage_id=$1 WHERE namespace_id=$2
         AND account_id=$3 AND character_id=$4",
    )
    .bind(lineage_id.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(repair.connection())
    .await
    .unwrap();
    sqlx::query(
        "DELETE FROM character_instance_lineages WHERE namespace_id=$1 AND account_id=$2
         AND character_id=$3 AND lineage_id=$4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(mismatched_lineage_id.as_slice())
    .execute(repair.connection())
    .await
    .unwrap();
    repair.commit().await.unwrap();

    let accepted = hall.transfer(owner.authenticated, 3, &mutation).await;
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
    let replay = hall.transfer(owner.authenticated, 4, &mutation).await;
    assert!(matches!(
        replay,
        protocol::WorldFlowResult::Transfer {
            request_sequence: 4,
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
async fn another_terminal_between_request_and_receipt_supersedes_caldus_extraction() {
    let persistence = disposable_database().await;
    let account_id = [171; 16];
    let character_id = [172; 16];
    let lineage_id = [173; 16];
    let restore_id = [174; 16];
    let lock = lock(175, 1);
    reset_fixture(&persistence, account_id, character_id, lineage_id, &lock).await;
    let (_, _, coordinator) = services(&persistence);
    coordinator
        .commit(
            lineage_id,
            restore_id,
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
        "UPDATE character_entry_restore_points SET restore_state=1,
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

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
#[allow(
    clippy::too_many_lines,
    reason = "the hosted journey keeps the actor permit, durable acceptance, reconnect, and conflict evidence in one ordered trace"
)]
async fn live_caldus_intent_uses_the_route_permit_and_postgres_planner_across_reconnect() {
    let persistence = disposable_database().await;
    let account_id = [181; 16];
    let character_id = [182; 16];
    let lineage_id = [183; 16];
    let restore_id = [184; 16];
    let mutation_id = [185; 16];
    let lock = lock(186, 1);
    let participant = lock.participants[0];
    reset_fixture(&persistence, account_id, character_id, lineage_id, &lock).await;

    let authenticated = AuthenticatedAccount {
        account_id: AccountId::new(account_id).unwrap(),
        namespace: AuthenticatedNamespace::WipeableTest,
    };
    let (_, _, victory_coordinator) = services(&persistence);
    let victory = victory_coordinator
        .commit(
            lineage_id,
            restore_id,
            &lock,
            ACTIVE_TICKS,
            CURRENT_TICK,
            &[owner(account_id, character_id, 186)],
        )
        .await
        .unwrap();
    let caldus_content = sim_content::load_core_development_caldus(&content_root()).unwrap();
    let mut presentation = CaldusInstancePresentation::new(lineage_id, 1).unwrap();
    victory
        .present_exit(&caldus_content, &mut presentation)
        .unwrap();

    let snapshot = persistence
        .load_current_danger_extraction_snapshot_v1(
            persistence::StoredActiveDangerAuthorityV1 {
                account_id,
                character_id,
                instance_lineage_id: lineage_id,
                entry_restore_point_id: restore_id,
            },
            &stored_world_flow_revision(),
        )
        .await
        .unwrap();
    assert_eq!(snapshot.authority.character_id, character_id);
    assert_eq!(snapshot.content_revision, stored_world_flow_revision());
    assert!(!snapshot.pending_items.is_empty());
    let versions = snapshot.expected_versions;
    let generation = persistence
        .allocate_private_route_generation_v1(account_id, character_id)
        .await
        .unwrap();
    let directory = CorePrivateRouteActorDirectory::new();
    let lease = directory
        .register_actor(
            authenticated,
            CorePrivateRouteActorSeed {
                character_id,
                character_version: versions.character,
                content_revision: private_route_revision(),
                world_flow_revision: world_flow_revision(),
                position: CorePrivateRouteActorPosition {
                    instance_lineage_id: Some(lineage_id),
                    scene: CorePrivateRouteSceneV1::BellSepulcher,
                    room: Some(CorePrivateRouteRoomV1::CaldusArenaB6),
                    phase: CorePrivateRoutePhaseV1::BossExitReady,
                },
            },
            generation.actor_generation,
        )
        .unwrap();
    let accepted_route = directory.snapshot(lease).unwrap();
    let identities = CoreCaldusVictoryIdentities::derive(lineage_id, &lock).unwrap();
    let extraction = identities.extraction_for(participant).unwrap();
    let terminal_id = derive_production_extraction_terminal_id_v1(
        account_id,
        character_id,
        identities.encounter_id.bytes(),
        extraction.request_id.bytes(),
        extraction.receipt_id.bytes(),
    )
    .unwrap();
    let exit = CorePrivateRouteExtractionExitBinding::new(
        identities.encounter_id.bytes(),
        identities.exit_instance_id.bytes(),
        extraction.request_id.bytes(),
        extraction.receipt_id.bytes(),
        terminal_id,
    )
    .unwrap();
    let binding = CorePrivateRouteExtractionBinding::new(
        account_id,
        accepted_route,
        world_flow_revision(),
        restore_id,
        exit,
    )
    .unwrap();
    let permit = directory
        .prepare_extraction_terminal(lease, binding)
        .await
        .unwrap();
    let authority = ProductionExtractionBossExitAuthorityV1::seal(
        authenticated,
        character_id,
        permit,
        &presentation,
        &lock,
        participant,
        versions,
    )
    .unwrap();
    let frame = live_extraction_frame(
        1,
        mutation_id,
        character_id,
        extraction.request_id.bytes(),
        versions,
    );

    let actor = ProductionExtractionIntentActor::new(
        authority.clone(),
        directory.clone(),
        lease,
        persistence.clone(),
        ExtractionClock,
    )
    .unwrap();
    let fresh = actor.handle(authenticated, &frame, 9_100).await;
    assert_eq!(fresh.server_tick, 9_100);
    assert!(matches!(
        fresh.result,
        ExtractionCommitResultV1::Pending {
            request_sequence: 1,
            mutation_id: id,
            character_id: owner,
            extraction_request_id: request,
            ..
        } if id == mutation_id
            && owner == character_id
            && request == extraction.request_id.bytes()
    ));
    let fresh_intent = actor.prepared_intent().await.unwrap();
    assert!(fresh_intent.prepared().is_some());
    assert_eq!(
        fresh_intent.acceptance().attempt.actor_generation,
        generation.actor_generation
    );
    assert_eq!(
        fresh_intent
            .acceptance()
            .attempt
            .accepted_pre_route_state_version
            + 1,
        fresh_intent
            .acceptance()
            .attempt
            .accepted_post_route_state_version
    );
    let accepted_hash = fresh_intent.acceptance().canonical_attempt_hash;
    drop(actor);

    let restarted_persistence = reconnect_database().await;
    let replay_actor = ProductionExtractionIntentActor::new(
        authority,
        directory.clone(),
        lease,
        restarted_persistence.clone(),
        ExtractionClock,
    )
    .unwrap();
    let mut replay_frame = frame.clone();
    replay_frame.sequence = 2;
    let replay = replay_actor
        .handle(authenticated, &replay_frame, 9_999)
        .await;
    assert_eq!(replay.server_tick, 9_100);
    assert!(matches!(
        replay.result,
        ExtractionCommitResultV1::Pending {
            request_sequence: 2,
            ..
        }
    ));
    let replayed_intent = replay_actor.prepared_intent().await.unwrap();
    assert_eq!(
        replayed_intent.acceptance().canonical_attempt_hash,
        accepted_hash
    );
    assert!(replayed_intent.prepared().is_some());

    let mut changed = replay_frame;
    changed.sequence = 3;
    changed.mutation_id = [187; 16];
    let conflict = replay_actor.handle(authenticated, &changed, 10_000).await;
    assert!(matches!(
        conflict.result,
        ExtractionCommitResultV1::Rejected {
            request_sequence: 3,
            code: TerminalInventoryRejectionCodeV1::IdempotencyConflict,
            ..
        }
    ));

    let mut verification = restarted_persistence.begin_transaction().await.unwrap();
    let state: (i16, i64, i64) = sqlx::query_as(
        "SELECT extraction.extraction_state,
                (SELECT count(*) FROM production_extraction_intent_acceptances_v1
                  WHERE namespace_id=$1 AND extraction_request_id=$2),
                (SELECT count(*) FROM production_extraction_intent_conflict_audits_v1
                  WHERE namespace_id=$1 AND extraction_request_id=$2)
           FROM character_extraction_results AS extraction
          WHERE extraction.namespace_id=$1 AND extraction.extraction_request_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(extraction.request_id.bytes().as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    verification.rollback().await.unwrap();
    assert_eq!(state, (0, 1, 1));

    directory.begin_shutdown();
    assert!(directory.finish_shutdown().await.unwrap().zero_residue);
}
