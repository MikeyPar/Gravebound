//! Hosted `PostgreSQL` acceptance for the GB-M03 successful-extraction terminal.
//!
//! Authorities:
//! - `Gravebound_Production_GDD_v1_Canonical.md` DTH-011, LOOT-002/033/050/060,
//!   and TECH-015/021-023;
//! - `Gravebound_Content_Production_Spec_v1.md` CONT-HUB-001/002 and the exact
//!   Bell Sepulcher/Sir Caldus exit;
//! - `Gravebound_Development_Roadmap_v1.md` GB-M03-03/08 and the restart,
//!   idempotency, and no-duplication exit gates.

use persistence::{
    CORE_ITEM_CONTENT_REVISION, CORE_PROGRESSION_RECORDS_BLAKE3, CORE_WORLD_ASSETS_BLAKE3,
    CORE_WORLD_LOCALIZATION_BLAKE3, CORE_WORLD_RECORDS_BLAKE3,
    PRODUCTION_EXTRACTION_CONTRACT_VERSION_V1, PersistenceConfig, PostgresPersistence,
    ProductionExtractionCommitRequestV1, ProductionExtractionExpectedVersionsV1,
    ProductionExtractionTransactionV1, StoredExtractionLocationV1, StoredWorldFlowRevisionV1,
    WIPEABLE_CORE_NAMESPACE, stage_danger_entry_ash_wallet_restore_v3,
    stage_danger_entry_inventory_restore_v3, stage_danger_entry_life_metrics_restore_v3,
    stage_danger_entry_oath_bargain_restore_v3,
};
use sqlx::Row;

const ACCOUNT_ID: [u8; 16] = [201; 16];
const CHARACTER_ID: [u8; 16] = [202; 16];
const LINEAGE_ID: [u8; 16] = [203; 16];
const RESTORE_POINT_ID: [u8; 16] = [204; 16];
const ENCOUNTER_ID: [u8; 16] = [205; 16];
const EXIT_INSTANCE_ID: [u8; 16] = [206; 16];
const EXTRACTION_REQUEST_ID: [u8; 16] = [207; 16];
const EXTRACTION_RECEIPT_ID: [u8; 16] = [208; 16];
const MUTATION_ID: [u8; 16] = [209; 16];
const TERMINAL_ID: [u8; 16] = [210; 16];
const ENTRY_MUTATION_ID: [u8; 16] = [211; 16];
const EQUIPPED_ITEM_UID: [u8; 16] = [212; 16];
const BELT_ITEM_UID: [u8; 16] = [213; 16];
const BACKPACK_ITEM_UID: [u8; 16] = [214; 16];
const REWARD_REQUEST_ID: [u8; 16] = [220; 16];
const REWARD_RESULT_HASH: [u8; 32] = [221; 32];
const PROGRESSION_PAYLOAD_HASH: [u8; 32] = [222; 32];
const MATERIAL_ID: &str = "material.bell_brass";

fn persistence_config() -> PersistenceConfig {
    PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL")
}

async fn disposable_database() -> PostgresPersistence {
    let persistence = PostgresPersistence::connect(&persistence_config())
        .await
        .unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence.migrate().await.unwrap();
    persistence
}

async fn reconnect_database() -> PostgresPersistence {
    let persistence = PostgresPersistence::connect(&persistence_config())
        .await
        .unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence
}

fn content_revision() -> StoredWorldFlowRevisionV1 {
    StoredWorldFlowRevisionV1 {
        records_blake3: CORE_WORLD_RECORDS_BLAKE3.into(),
        assets_blake3: CORE_WORLD_ASSETS_BLAKE3.into(),
        localization_blake3: CORE_WORLD_LOCALIZATION_BLAKE3.into(),
    }
}

fn request() -> ProductionExtractionCommitRequestV1 {
    ProductionExtractionCommitRequestV1 {
        contract_version: PRODUCTION_EXTRACTION_CONTRACT_VERSION_V1,
        namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
        account_id: ACCOUNT_ID,
        character_id: CHARACTER_ID,
        mutation_id: MUTATION_ID,
        terminal_id: TERMINAL_ID,
        extraction_request_id: EXTRACTION_REQUEST_ID,
        extraction_receipt_id: EXTRACTION_RECEIPT_ID,
        encounter_id: ENCOUNTER_ID,
        instance_lineage_id: LINEAGE_ID,
        entry_restore_point_id: RESTORE_POINT_ID,
        exit_instance_id: EXIT_INSTANCE_ID,
        expected_versions: ProductionExtractionExpectedVersionsV1 {
            account: 1,
            character: 2,
            world: 2,
            inventory: 3,
            life_metrics: 2,
        },
        content_revision: content_revision(),
        issued_at_unix_ms: 1,
        observed_tick: 30_000,
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "the complete V3 danger fixture remains explicit for hosted transaction review"
)]
async fn reset_fixture(persistence: &PostgresPersistence) {
    persistence.reset_disposable_identity_data().await.unwrap();
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO accounts (namespace_id,account_id,state_version,slot_capacity)
         VALUES ($1,$2,1,2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ash_wallets (namespace_id,account_id,balance,wallet_version)
         VALUES ($1,$2,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO characters
         (namespace_id,account_id,character_id,roster_ordinal,class_id,level,
          oath_id,life_state,security_state,character_state_version)
         VALUES ($1,$2,$3,1,'class.grave_arbalist',10,NULL,0,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE accounts SET selected_character_id=$1
         WHERE namespace_id=$2 AND account_id=$3",
    )
    .bind(CHARACTER_ID.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_progression
         (namespace_id,account_id,character_id,total_xp,level,current_health,
          progression_version)
         VALUES ($1,$2,$3,2700,10,120,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_inventories
         (namespace_id,account_id,character_id,inventory_version)
         VALUES ($1,$2,$3,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_oath_bargain_state
         (namespace_id,account_id,character_id,earned_bargain_slots,oath_bargain_version)
         VALUES ($1,$2,$3,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_life_metrics
         SET lifetime_ticks=10000,permadeath_combat_ticks=8000
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
           AND life_metrics_version=1",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO item_instances
         (namespace_id,item_uid,account_id,character_id,template_id,content_revision,
          item_kind,item_level,rarity,creation_kind,creation_request_id,roll_index,
          unit_ordinal,item_version,security_state,location_kind,slot_index,
          provenance_kind,salvage_band,salvage_value)
         VALUES
         ($1,$2,$3,$4,'item.weapon.crossbow.pine_crossbow',$5,
          0,10,0,0,$2,0,0,1,0,0,0,0,0,0),
         ($1,$6,$3,$4,'consumable.red_tonic',$5,
          1,NULL,NULL,0,$6,1,0,1,0,1,0,4,0,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(EQUIPPED_ITEM_UID.as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(CORE_ITEM_CONTENT_REVISION)
    .bind(BELT_ITEM_UID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_instance_lineages
         (namespace_id,account_id,character_id,lineage_id,content_id,layout_id,
          lineage_state,records_blake3,assets_blake3,localization_blake3)
         VALUES ($1,$2,$3,$4,'world.core_microrealm_01',
          'layout.core_private_life_01',0,$5,$6,$7)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(LINEAGE_ID.as_slice())
    .bind(CORE_WORLD_RECORDS_BLAKE3)
    .bind(CORE_WORLD_ASSETS_BLAKE3)
    .bind(CORE_WORLD_LOCALIZATION_BLAKE3)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_entry_restore_points
         (namespace_id,account_id,character_id,restore_point_id,lineage_id,
          source_location_id,restore_location_id,snapshot_contract_version,
          account_version,character_version,progression_version,inventory_version,
          oath_bargain_version,life_metrics_version,ash_wallet_version,component_mask,
          composite_digest,restore_state,records_blake3,assets_blake3,localization_blake3)
         VALUES ($1,$2,$3,$4,$5,'hub.lantern_halls_01','hub.lantern_halls_01',
          3,1,1,1,2,1,1,1,31,$6,0,$7,$8,$9)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(RESTORE_POINT_ID.as_slice())
    .bind(LINEAGE_ID.as_slice())
    .bind([91_u8; 32].as_slice())
    .bind(CORE_WORLD_RECORDS_BLAKE3)
    .bind(CORE_WORLD_ASSETS_BLAKE3)
    .bind(CORE_WORLD_LOCALIZATION_BLAKE3)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO entry_restore_progression_v3
         (namespace_id,account_id,character_id,restore_point_id,level,total_xp,
          current_health,progression_version,component_digest)
         VALUES ($1,$2,$3,$4,10,2700,120,1,$5)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(RESTORE_POINT_ID.as_slice())
    .bind([92_u8; 32].as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO entry_restore_progression_v1
         (namespace_id,account_id,character_id,restore_point_id,level,total_xp,
          current_health,progression_version)
         VALUES ($1,$2,$3,$4,10,2700,120,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(RESTORE_POINT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    let inventory = stage_danger_entry_inventory_restore_v3(
        &mut transaction,
        ACCOUNT_ID,
        CHARACTER_ID,
        RESTORE_POINT_ID,
        ENTRY_MUTATION_ID,
        0,
    )
    .await
    .unwrap();
    assert_eq!(inventory.pre_inventory_version, 1);
    assert_eq!(inventory.post_inventory_version, 2);
    assert_eq!(inventory.items.len(), 2);
    stage_danger_entry_oath_bargain_restore_v3(
        &mut transaction,
        ACCOUNT_ID,
        CHARACTER_ID,
        RESTORE_POINT_ID,
    )
    .await
    .unwrap();
    stage_danger_entry_life_metrics_restore_v3(
        &mut transaction,
        ACCOUNT_ID,
        CHARACTER_ID,
        RESTORE_POINT_ID,
    )
    .await
    .unwrap();
    stage_danger_entry_ash_wallet_restore_v3(
        &mut transaction,
        ACCOUNT_ID,
        CHARACTER_ID,
        RESTORE_POINT_ID,
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_world_locations
         (namespace_id,account_id,character_id,character_version,location_kind,
          location_content_id,instance_lineage_id,entry_restore_point_id)
         VALUES ($1,$2,$3,2,2,'world.core_microrealm_01',$4,$5)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(LINEAGE_ID.as_slice())
    .bind(RESTORE_POINT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE characters SET character_state_version=2
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_inventories SET inventory_version=3
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
           AND inventory_version=2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_life_metrics
         SET lifetime_ticks=12_000,permadeath_combat_ticks=10_000,life_metrics_version=2
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO item_instances
         (namespace_id,item_uid,account_id,character_id,template_id,content_revision,
          item_kind,item_level,rarity,creation_kind,creation_request_id,roll_index,
          unit_ordinal,item_version,security_state,location_kind,slot_index,
          provenance_kind,salvage_band,salvage_value)
         VALUES ($1,$2,$3,$4,'item.armor.parish_leather',$5,
          0,8,1,1,$2,2,0,1,2,2,0,1,1,12)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(BACKPACK_ITEM_UID.as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(CORE_ITEM_CONTENT_REVISION)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_run_material_stacks
         (namespace_id,account_id,character_id,material_id,quantity,
          material_version,security_state)
         VALUES ($1,$2,$3,$4,3,1,2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(MATERIAL_ID)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE account_material_wallet_balances_v1
         SET quantity=10
         WHERE namespace_id=$1 AND account_id=$2 AND material_id=$3
           AND quantity=0 AND wallet_cap=999 AND material_version=1",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(MATERIAL_ID)
    .execute(transaction.connection())
    .await
    .unwrap();
    let checkpoint_payload = [1_u8];
    let checkpoint_digest = blake3::hash(&checkpoint_payload);
    sqlx::query(
        "INSERT INTO character_danger_checkpoints
         (namespace_id,account_id,character_id,lineage_id,checkpoint_tick,
          component_mask,composite_digest,character_version,progression_version,
          inventory_version,oath_bargain_version,records_blake3,assets_blake3,
          localization_blake3,checkpoint_schema_version,checkpoint_payload,
          checkpoint_payload_digest)
         VALUES ($1,$2,$3,$4,29_990,15,$5,2,1,3,1,$6,$7,$8,1,$9,$10)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(LINEAGE_ID.as_slice())
    .bind([93_u8; 32].as_slice())
    .bind(CORE_WORLD_RECORDS_BLAKE3)
    .bind(CORE_WORLD_ASSETS_BLAKE3)
    .bind(CORE_WORLD_LOCALIZATION_BLAKE3)
    .bind(checkpoint_payload.as_slice())
    .bind(checkpoint_digest.as_bytes().as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO reward_requests
         (namespace_id,reward_request_id,account_id,character_id,source_instance_id,
          reward_table_id,content_revision,epoch_id,canonical_request_hash,plan_hash,
          result_hash,audit_digest,pre_inventory_version,post_inventory_version,
          request_state,reward_item_count)
         VALUES ($1,$2,$3,$4,$5,'reward.boss_caldus',$6,'extraction-terminal-v1',
          $7,$8,$9,$10,3,3,1,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(REWARD_REQUEST_ID.as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(ENCOUNTER_ID.as_slice())
    .bind(CORE_ITEM_CONTENT_REVISION)
    .bind([96_u8; 32].as_slice())
    .bind([97_u8; 32].as_slice())
    .bind(REWARD_RESULT_HASH.as_slice())
    .bind([98_u8; 32].as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_xp_award_results
         (namespace_id,account_id,character_id,reward_event_id,payload_hash,
          source_content_id,xp_profile_id,progression_content_revision,
          eligibility_kind,eligible,encounter_active_ticks,encounter_present_ticks,
          encounter_longest_inactivity_ticks,encounter_reference_health,
          encounter_direct_damage,encounter_effective_healing,encounter_damage_prevented,
          encounter_objective_credits,encounter_life_state,encounter_recall_state,
          encounter_trust_state,first_clear_awarded,base_xp,bonus_xp,requested_xp,
          applied_xp,discarded_xp,pre_total_xp,post_total_xp,pre_level,post_level,
          pre_progression_version,post_progression_version,result_code,result_payload,
          entry_restore_point_id)
         VALUES ($1,$2,$3,$4,$5,'boss.sir_caldus','xp.boss_caldus',$6,
          1,TRUE,300,300,0,7200,1,0,0,0,0,0,0,FALSE,450,0,450,0,450,
          2700,2700,10,10,1,1,0,$7,$8)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(REWARD_REQUEST_ID.as_slice())
    .bind(PROGRESSION_PAYLOAD_HASH.as_slice())
    .bind(CORE_PROGRESSION_RECORDS_BLAKE3)
    .bind([1_u8].as_slice())
    .bind(RESTORE_POINT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO caldus_victory_exits
         (namespace_id,encounter_id,instance_lineage_id,attempt_ordinal,
          exit_instance_id,canonical_request_hash,eligible_owner_count)
         VALUES ($1,$2,$3,1,$4,$5,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ENCOUNTER_ID.as_slice())
    .bind(LINEAGE_ID.as_slice())
    .bind(EXIT_INSTANCE_ID.as_slice())
    .bind([94_u8; 32].as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO caldus_victory_exit_owners
         (namespace_id,encounter_id,party_slot,participant_entity_id,account_id,
          character_id,reward_request_id,reward_result_hash,progression_payload_hash)
         VALUES ($1,$2,0,$3,$4,$5,$6,$7,$8)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ENCOUNTER_ID.as_slice())
    .bind([1_u8; 8].as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(REWARD_REQUEST_ID.as_slice())
    .bind(REWARD_RESULT_HASH.as_slice())
    .bind(PROGRESSION_PAYLOAD_HASH.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_extraction_results
         (namespace_id,account_id,character_id,extraction_request_id,
          request_payload_hash,encounter_id,instance_lineage_id,entry_restore_point_id,
          exit_instance_id,exit_content_id,attempt_ordinal,party_slot,
          participant_entity_id,expected_character_version,records_blake3,
          assets_blake3,localization_blake3,extraction_state)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,
          'portal.exit.dungeon.bell_sepulcher',1,0,$10,2,$11,$12,$13,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(EXTRACTION_REQUEST_ID.as_slice())
    .bind([95_u8; 32].as_slice())
    .bind(ENCOUNTER_ID.as_slice())
    .bind(LINEAGE_ID.as_slice())
    .bind(RESTORE_POINT_ID.as_slice())
    .bind(EXIT_INSTANCE_ID.as_slice())
    .bind([1_u8; 8].as_slice())
    .bind(CORE_WORLD_RECORDS_BLAKE3)
    .bind(CORE_WORLD_ASSETS_BLAKE3)
    .bind(CORE_WORLD_LOCALIZATION_BLAKE3)
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
#[allow(
    clippy::too_many_lines,
    reason = "one hosted journey proves the complete normalized terminal graph and retry contract"
)]
async fn extraction_commit_restart_replay_and_conflict_are_atomic() {
    let persistence = disposable_database().await;
    reset_fixture(&persistence).await;
    let commit_request = request();
    let prepared = persistence
        .prepare_production_extraction_v1(&commit_request)
        .await
        .unwrap();
    assert!(!prepared.replayed());
    assert_eq!(prepared.request(), &commit_request);
    assert_eq!(
        prepared.canonical_request_hash(),
        commit_request.canonical_hash().unwrap()
    );

    let ProductionExtractionTransactionV1::Fresh(fresh) = persistence
        .commit_production_extraction_v1(&commit_request, prepared.canonical_plan_hash())
        .await
        .unwrap()
    else {
        panic!("first production extraction must commit fresh");
    };
    assert_eq!(fresh.placements.len(), 3);
    assert_eq!(fresh.material_credits.len(), 1);
    assert!(!fresh.storage_resolution_required);
    assert_eq!(fresh.versions.account.pre, 1);
    assert_eq!(fresh.versions.account.post, 2);
    assert_eq!(fresh.versions.character.pre, 2);
    assert_eq!(fresh.versions.character.post, 3);
    assert_eq!(fresh.versions.world.pre, 2);
    assert_eq!(fresh.versions.world.post, 3);
    assert_eq!(fresh.versions.inventory.pre, 3);
    assert_eq!(fresh.versions.inventory.post, 4);
    assert_eq!(fresh.versions.life_metrics.pre, 2);
    assert_eq!(fresh.versions.life_metrics.post, 3);
    assert!(fresh.placements.iter().any(|placement| {
        placement.item_uid == BACKPACK_ITEM_UID
            && placement.source == StoredExtractionLocationV1::RunBackpack(0)
            && placement.destination == StoredExtractionLocationV1::CharacterSafe(0)
    }));
    assert_eq!(fresh.material_credits[0].material_id, MATERIAL_ID);
    assert_eq!(fresh.material_credits[0].pre_wallet_quantity, 10);
    assert_eq!(fresh.material_credits[0].post_wallet_quantity, 13);

    persistence.close().await;
    let persistence = reconnect_database().await;
    let replay_prepared = persistence
        .prepare_production_extraction_v1(&commit_request)
        .await
        .unwrap();
    assert!(replay_prepared.replayed());
    assert_eq!(
        replay_prepared.canonical_plan_hash(),
        fresh.canonical_plan_hash
    );
    let ProductionExtractionTransactionV1::Replayed(replayed) = persistence
        .commit_production_extraction_v1(&commit_request, replay_prepared.canonical_plan_hash())
        .await
        .unwrap()
    else {
        panic!("response-loss retry after reconnect must return the stored result");
    };
    assert_eq!(replayed, fresh);

    let mut changed = commit_request.clone();
    changed.issued_at_unix_ms += 1;
    let ProductionExtractionTransactionV1::Conflict {
        extraction_request_id,
        terminal_id,
    } = persistence
        .commit_production_extraction_v1(&changed, fresh.canonical_plan_hash)
        .await
        .unwrap()
    else {
        panic!("same extraction identity with a changed payload must conflict");
    };
    assert_eq!(extraction_request_id, EXTRACTION_REQUEST_ID);
    assert_eq!(terminal_id, TERMINAL_ID);

    let mut reused_terminal = commit_request.clone();
    reused_terminal.mutation_id = [215; 16];
    reused_terminal.extraction_request_id = [216; 16];
    reused_terminal.extraction_receipt_id = [217; 16];
    reused_terminal.issued_at_unix_ms += 2;
    let ProductionExtractionTransactionV1::Conflict {
        extraction_request_id,
        terminal_id,
    } = persistence
        .commit_production_extraction_v1(&reused_terminal, fresh.canonical_plan_hash)
        .await
        .unwrap()
    else {
        panic!("reusing a stored terminal identity must return an audited conflict");
    };
    assert_eq!(extraction_request_id, EXTRACTION_REQUEST_ID);
    assert_eq!(terminal_id, TERMINAL_ID);

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let aggregate = sqlx::query(
        "SELECT account.state_version,character.character_state_version,
                character.security_state,world.character_version,world.location_kind,
                world.location_content_id,world.safe_arrival_kind,
                inventory.inventory_version,life.life_metrics_version
         FROM accounts AS account
         JOIN characters AS character
           ON character.namespace_id=account.namespace_id
          AND character.account_id=account.account_id
         JOIN character_world_locations AS world
           ON world.namespace_id=character.namespace_id
          AND world.account_id=character.account_id
          AND world.character_id=character.character_id
         JOIN character_inventories AS inventory
           ON inventory.namespace_id=character.namespace_id
          AND inventory.account_id=character.account_id
          AND inventory.character_id=character.character_id
         JOIN character_life_metrics AS life
           ON life.namespace_id=character.namespace_id
          AND life.account_id=character.account_id
          AND life.character_id=character.character_id
         WHERE account.namespace_id=$1 AND account.account_id=$2
           AND account.selected_character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(aggregate.get::<i64, _>("state_version"), 2);
    assert_eq!(aggregate.get::<i64, _>("character_state_version"), 3);
    assert_eq!(aggregate.get::<i16, _>("security_state"), 0);
    assert_eq!(aggregate.get::<i64, _>("character_version"), 3);
    assert_eq!(aggregate.get::<i16, _>("location_kind"), 1);
    assert_eq!(
        aggregate.get::<String, _>("location_content_id"),
        "hub.lantern_halls_01"
    );
    assert_eq!(aggregate.get::<i16, _>("safe_arrival_kind"), 0);
    assert_eq!(aggregate.get::<i64, _>("inventory_version"), 4);
    assert_eq!(aggregate.get::<i64, _>("life_metrics_version"), 3);

    let item_locations: Vec<(i16, i16, i16)> = sqlx::query_as(
        "SELECT location_kind,slot_index,security_state
         FROM item_instances
         WHERE namespace_id=$1 AND account_id=$2 AND terminal_extraction_id=$3
         ORDER BY location_kind,slot_index",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(TERMINAL_ID.as_slice())
    .fetch_all(transaction.connection())
    .await
    .unwrap();
    assert_eq!(item_locations, vec![(0, 0, 0), (1, 0, 0), (5, 0, 0)]);

    let wallet: (i32, i64) = sqlx::query_as(
        "SELECT quantity,material_version
         FROM account_material_wallet_balances_v1
         WHERE namespace_id=$1 AND account_id=$2 AND material_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(MATERIAL_ID)
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(wallet, (13, 2));
    let pouch: (i32, i64, i16, Vec<u8>) = sqlx::query_as(
        "SELECT quantity,material_version,security_state,terminal_extraction_id
         FROM character_run_material_stacks
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND material_id=$4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(MATERIAL_ID)
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(pouch, (0, 2, 4, TERMINAL_ID.to_vec()));

    let counts = sqlx::query(
        "SELECT
          (SELECT count(*) FROM character_extraction_terminal_results_v1
           WHERE namespace_id=$1 AND terminal_id=$2) AS terminals,
          (SELECT count(*) FROM extraction_terminal_item_placements_v1
           WHERE namespace_id=$1 AND terminal_id=$2) AS placements,
          (SELECT count(*) FROM item_ledger_events
           WHERE namespace_id=$1 AND terminal_extraction_id=$2) AS item_ledgers,
          (SELECT count(*) FROM extraction_terminal_material_credits_v1
           WHERE namespace_id=$1 AND terminal_id=$2) AS credits,
          (SELECT count(*) FROM account_material_ledger_events_v1
           WHERE namespace_id=$1 AND terminal_id=$2) AS material_ledgers,
          (SELECT count(*) FROM extraction_terminal_audit_events_v1
           WHERE namespace_id=$1 AND terminal_id=$2) AS audits,
          (SELECT count(*) FROM extraction_terminal_outbox_events_v1
           WHERE namespace_id=$1 AND terminal_id=$2) AS outbox,
          (SELECT count(*) FROM extraction_terminal_conflict_audits_v1
           WHERE namespace_id=$1 AND extraction_request_id=$3) AS conflicts,
          (SELECT count(*) FROM character_danger_checkpoints
           WHERE namespace_id=$1 AND account_id=$4 AND character_id=$5) AS checkpoints",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(TERMINAL_ID.as_slice())
    .bind(EXTRACTION_REQUEST_ID.as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(counts.get::<i64, _>("terminals"), 1);
    assert_eq!(counts.get::<i64, _>("placements"), 3);
    assert_eq!(counts.get::<i64, _>("item_ledgers"), 3);
    assert_eq!(counts.get::<i64, _>("credits"), 1);
    assert_eq!(counts.get::<i64, _>("material_ledgers"), 1);
    assert_eq!(counts.get::<i64, _>("audits"), 1);
    assert_eq!(counts.get::<i64, _>("outbox"), 1);
    assert_eq!(counts.get::<i64, _>("conflicts"), 2);
    assert_eq!(counts.get::<i64, _>("checkpoints"), 0);
    transaction.rollback().await.unwrap();
    persistence.close().await;
}

async fn terminal_count(persistence: &PostgresPersistence) -> i64 {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let count = sqlx::query_scalar(
        "SELECT count(*) FROM character_extraction_terminal_results_v1
         WHERE namespace_id=$1 AND account_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    count
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn extraction_rejects_unresolved_reward_stale_item_and_attempt_drift() {
    let persistence = disposable_database().await;

    reset_fixture(&persistence).await;
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO reward_requests
         (namespace_id,reward_request_id,account_id,character_id,source_instance_id,
          reward_table_id,content_revision,epoch_id,canonical_request_hash,
          pre_inventory_version,request_state)
         VALUES ($1,$2,$3,$4,$5,'reward.boss_caldus',$6,'terminal-race-v1',$7,3,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind([218_u8; 16].as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(ENCOUNTER_ID.as_slice())
    .bind(CORE_ITEM_CONTENT_REVISION)
    .bind([96_u8; 32].as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    assert!(matches!(
        persistence
            .prepare_production_extraction_v1(&request())
            .await,
        Err(persistence::PersistenceError::ProductionExtractionUnresolvedMutation)
    ));
    assert_eq!(terminal_count(&persistence).await, 0);

    reset_fixture(&persistence).await;
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let changed = sqlx::query(
        "UPDATE item_instances
         SET content_revision=$1
         WHERE namespace_id=$2 AND item_uid=$3",
    )
    .bind(format!("core-dev.blake3.{}", "b".repeat(64)))
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(BACKPACK_ITEM_UID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(changed, 1);
    transaction.commit().await.unwrap();
    assert!(matches!(
        persistence
            .prepare_production_extraction_v1(&request())
            .await,
        Err(persistence::PersistenceError::ProductionExtractionContentMismatch)
    ));
    assert_eq!(terminal_count(&persistence).await, 0);

    reset_fixture(&persistence).await;
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let changed = sqlx::query(
        "UPDATE character_extraction_results
         SET attempt_ordinal=2
         WHERE namespace_id=$1 AND extraction_request_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(EXTRACTION_REQUEST_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(changed, 1);
    transaction.commit().await.unwrap();
    assert!(matches!(
        persistence
            .prepare_production_extraction_v1(&request())
            .await,
        Err(persistence::PersistenceError::ProductionExtractionBindingMismatch)
    ));
    assert_eq!(terminal_count(&persistence).await, 0);
    persistence.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn extraction_rejects_plan_drift_and_missing_caldus_owner() {
    let persistence = disposable_database().await;
    reset_fixture(&persistence).await;
    let commit_request = request();
    let prepared = persistence
        .prepare_production_extraction_v1(&commit_request)
        .await
        .unwrap();
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let changed = sqlx::query(
        "UPDATE item_instances SET slot_index=1
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND item_uid=$4
           AND location_kind=2 AND slot_index=0",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(BACKPACK_ITEM_UID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(changed, 1);
    transaction.commit().await.unwrap();
    assert!(matches!(
        persistence
            .commit_production_extraction_v1(&commit_request, prepared.canonical_plan_hash())
            .await,
        Err(persistence::PersistenceError::ProductionExtractionPlanChanged)
    ));
    assert_eq!(terminal_count(&persistence).await, 0);

    reset_fixture(&persistence).await;
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let deleted = sqlx::query(
        "DELETE FROM caldus_victory_exit_owners
         WHERE namespace_id=$1 AND encounter_id=$2 AND party_slot=0",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ENCOUNTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(deleted, 1);
    transaction.commit().await.unwrap();
    assert!(matches!(
        persistence
            .prepare_production_extraction_v1(&request())
            .await,
        Err(persistence::PersistenceError::ProductionExtractionBindingMismatch)
    ));
    assert_eq!(terminal_count(&persistence).await, 0);
    persistence.close().await;
}
