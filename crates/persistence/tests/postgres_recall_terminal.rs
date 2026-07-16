//! Hosted `PostgreSQL` acceptance for the GB-M03 Emergency Recall terminal.
//!
//! Authorities:
//! - `Gravebound_Production_GDD_v1_Canonical.md` DTH-010, LOOT-002/033/060,
//!   and TECH-015/021-023;
//! - `Gravebound_Content_Production_Spec_v1.md` CONT-HUB-001/002 and the Core
//!   microrealm/dungeon/boss Recall contract;
//! - `Gravebound_Development_Roadmap_v1.md` GB-M03-03/08 and the restart,
//!   idempotency, clock, and no-duplication exit gates.

use persistence::{
    CORE_ITEM_CONTENT_REVISION, CORE_WORLD_ASSETS_BLAKE3, CORE_WORLD_LOCALIZATION_BLAKE3,
    CORE_WORLD_RECORDS_BLAKE3, PRODUCTION_RECALL_CONTRACT_VERSION_V1, PersistenceConfig,
    PersistenceError, PostgresPersistence, ProductionRecallCommitRequestV1,
    ProductionRecallExpectedVersionsV1, ProductionRecallTransactionV1, ProductionRecallTriggerV1,
    StoredRecallLocationV1, StoredWorldFlowRevisionV1, WIPEABLE_CORE_NAMESPACE,
    stage_danger_entry_ash_wallet_restore_v3, stage_danger_entry_inventory_restore_v3,
    stage_danger_entry_life_metrics_restore_v3, stage_danger_entry_oath_bargain_restore_v3,
};
use sqlx::Row;

const ACCOUNT_ID: [u8; 16] = [31; 16];
const CHARACTER_ID: [u8; 16] = [32; 16];
const LINEAGE_ID: [u8; 16] = [33; 16];
const RESTORE_POINT_ID: [u8; 16] = [34; 16];
const MUTATION_ID: [u8; 16] = [35; 16];
const TERMINAL_ID: [u8; 16] = [36; 16];
const ENTRY_MUTATION_ID: [u8; 16] = [37; 16];
const EQUIPPED_ITEM_UID: [u8; 16] = [38; 16];
const BELT_ITEM_UID: [u8; 16] = [39; 16];
const BACKPACK_ITEM_UID: [u8; 16] = [40; 16];
const GROUND_ITEM_UID: [u8; 16] = [41; 16];
const GROUND_INSTANCE_ID: [u8; 16] = [42; 16];
const GROUND_PICKUP_ID: [u8; 16] = [43; 16];
const MATERIAL_ID: &str = "material.bell_brass";
const START_TICK: u64 = 30_000;

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

fn request(trigger: ProductionRecallTriggerV1) -> ProductionRecallCommitRequestV1 {
    ProductionRecallCommitRequestV1 {
        contract_version: PRODUCTION_RECALL_CONTRACT_VERSION_V1,
        namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
        account_id: ACCOUNT_ID,
        character_id: CHARACTER_ID,
        mutation_id: MUTATION_ID,
        terminal_id: TERMINAL_ID,
        trigger,
        request_sequence: match trigger {
            ProductionRecallTriggerV1::Explicit => Some(77),
            ProductionRecallTriggerV1::LinkLost => None,
        },
        explicit_client_tick: match trigger {
            ProductionRecallTriggerV1::Explicit => Some(78),
            ProductionRecallTriggerV1::LinkLost => None,
        },
        instance_lineage_id: LINEAGE_ID,
        entry_restore_point_id: RESTORE_POINT_ID,
        expected_versions: ProductionRecallExpectedVersionsV1 {
            account: 1,
            character: 2,
            world: 2,
            inventory: 3,
            life_metrics: 2,
            progression: 1,
            oath_bargain: 1,
            ash_wallet: 1,
        },
        content_revision: content_revision(),
        issued_at_unix_ms: 1,
        trigger_started_tick: START_TICK,
        completion_tick: START_TICK + trigger.channel_ticks(),
        final_lifetime_ticks: 12_000 + trigger.channel_ticks(),
        final_permadeath_combat_ticks: 10_000 + trigger.channel_ticks(),
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
         SET lifetime_ticks=12000,permadeath_combat_ticks=10000,life_metrics_version=2
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
        "INSERT INTO item_instances
         (namespace_id,item_uid,account_id,character_id,template_id,content_revision,
          item_kind,item_level,rarity,creation_kind,creation_request_id,roll_index,
          unit_ordinal,item_version,security_state,location_kind,instance_id,pickup_id,
          expires_at_tick,provenance_kind,salvage_band,salvage_value)
         VALUES ($1,$2,$3,$4,'item.charm.ember_tooth.t1',$5,
          0,8,1,1,$2,3,0,1,2,3,$6,$7,31000,1,1,12)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(GROUND_ITEM_UID.as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(CORE_ITEM_CONTENT_REVISION)
    .bind(GROUND_INSTANCE_ID.as_slice())
    .bind(GROUND_PICKUP_ID.as_slice())
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
    let checkpoint_payload = [1_u8];
    let checkpoint_digest = blake3::hash(&checkpoint_payload);
    sqlx::query(
        "INSERT INTO character_danger_checkpoints
         (namespace_id,account_id,character_id,lineage_id,checkpoint_tick,
          component_mask,composite_digest,character_version,progression_version,
          inventory_version,oath_bargain_version,records_blake3,assets_blake3,
          localization_blake3,checkpoint_schema_version,checkpoint_payload,
          checkpoint_payload_digest)
         VALUES ($1,$2,$3,$4,29990,15,$5,2,1,3,1,$6,$7,$8,1,$9,$10)",
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
    transaction.commit().await.unwrap();
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
#[allow(
    clippy::too_many_lines,
    reason = "the restart proof keeps the complete terminal graph assertions together"
)]
async fn explicit_recall_is_atomic_replay_safe_and_restart_durable() {
    let persistence = disposable_database().await;
    reset_fixture(&persistence).await;
    let request = request(ProductionRecallTriggerV1::Explicit);
    let prepared = persistence
        .prepare_production_recall_v1(&request)
        .await
        .unwrap();
    assert!(!prepared.replayed());
    let committed = persistence
        .commit_production_recall_v1(&request, prepared.canonical_plan_hash())
        .await
        .unwrap();
    let ProductionRecallTransactionV1::Fresh(result) = committed else {
        panic!("first Recall commit must be fresh");
    };
    assert_eq!(result.stabilized_items.len(), 2);
    assert_eq!(result.destroyed_items.len(), 2);
    assert_eq!(result.destroyed_materials.len(), 1);
    assert_eq!(result.versions.account.pre, result.versions.account.post);
    assert_eq!(result.post_lifetime_ticks, 12_012);
    assert_eq!(result.post_permadeath_combat_ticks, 10_012);
    assert_eq!(result.explicit_client_tick, Some(78));
    assert!(matches!(
        result.destroyed_items[0].source,
        StoredRecallLocationV1::RunBackpack(0)
    ));
    assert!(matches!(
        result.destroyed_items[1].source,
        StoredRecallLocationV1::PersonalGround {
            instance_id: GROUND_INSTANCE_ID,
            pickup_id: GROUND_PICKUP_ID,
            expires_at_tick: 31_000,
        }
    ));

    persistence.close().await;
    let reconnected = reconnect_database().await;
    let replay_prepared = reconnected
        .prepare_production_recall_v1(&request)
        .await
        .unwrap();
    assert!(replay_prepared.replayed());
    let replayed = reconnected
        .commit_production_recall_v1(&request, replay_prepared.canonical_plan_hash())
        .await
        .unwrap();
    assert_eq!(
        replayed,
        ProductionRecallTransactionV1::Replayed(result.clone())
    );
    let mut altered_client_tick = request.clone();
    altered_client_tick.explicit_client_tick = Some(79);
    assert!(matches!(
        reconnected
            .prepare_production_recall_v1(&altered_client_tick)
            .await,
        Err(PersistenceError::RecallIdempotencyConflict)
    ));
    let recovered = reconnected
        .load_committed_recall_terminal_v1(ACCOUNT_ID, CHARACTER_ID)
        .await
        .unwrap()
        .expect("latest committed Recall recovery");
    assert_eq!(recovered.result, result);
    assert_eq!(recovered.lineage_id, LINEAGE_ID);
    assert_eq!(recovered.restore_point_id, RESTORE_POINT_ID);
    assert_eq!(recovered.content_revision, content_revision());
    assert!(recovered.owns_current_hall);
    assert_eq!(
        reconnected
            .load_committed_recall_terminal_by_identity_v1(
                ACCOUNT_ID,
                CHARACTER_ID,
                MUTATION_ID,
                TERMINAL_ID,
            )
            .await
            .unwrap(),
        Some(recovered)
    );

    let mut verification = reconnected.begin_transaction().await.unwrap();
    let aggregate = sqlx::query(
        "SELECT a.state_version,c.character_state_version,w.character_version,
                w.location_kind,w.location_content_id,i.inventory_version,
                l.life_metrics_version,l.lifetime_ticks,l.permadeath_combat_ticks,
                r.restore_state,r.recall_terminal_id,g.lineage_state,
                (SELECT count(*) FROM character_danger_checkpoints d
                 WHERE d.namespace_id=a.namespace_id AND d.account_id=a.account_id
                   AND d.character_id=c.character_id) AS checkpoint_count
         FROM accounts a
         JOIN characters c USING (namespace_id,account_id)
         JOIN character_world_locations w USING (namespace_id,account_id,character_id)
         JOIN character_inventories i USING (namespace_id,account_id,character_id)
         JOIN character_life_metrics l USING (namespace_id,account_id,character_id)
         JOIN character_entry_restore_points r
           ON r.namespace_id=c.namespace_id AND r.account_id=c.account_id
          AND r.character_id=c.character_id
         JOIN character_instance_lineages g
           ON g.namespace_id=r.namespace_id AND g.lineage_id=r.lineage_id
         WHERE a.namespace_id=$1 AND a.account_id=$2 AND c.character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    assert_eq!(aggregate.try_get::<i64, _>("state_version").unwrap(), 1);
    assert_eq!(
        aggregate
            .try_get::<i64, _>("character_state_version")
            .unwrap(),
        3
    );
    assert_eq!(aggregate.try_get::<i64, _>("character_version").unwrap(), 3);
    assert_eq!(aggregate.try_get::<i16, _>("location_kind").unwrap(), 1);
    assert_eq!(
        aggregate
            .try_get::<String, _>("location_content_id")
            .unwrap(),
        "hub.lantern_halls_01"
    );
    assert_eq!(aggregate.try_get::<i64, _>("inventory_version").unwrap(), 4);
    assert_eq!(
        aggregate.try_get::<i64, _>("life_metrics_version").unwrap(),
        3
    );
    assert_eq!(
        aggregate.try_get::<i64, _>("lifetime_ticks").unwrap(),
        12_012
    );
    assert_eq!(
        aggregate
            .try_get::<i64, _>("permadeath_combat_ticks")
            .unwrap(),
        10_012
    );
    assert_eq!(aggregate.try_get::<i16, _>("restore_state").unwrap(), 3);
    assert_eq!(
        aggregate
            .try_get::<Vec<u8>, _>("recall_terminal_id")
            .unwrap(),
        TERMINAL_ID
    );
    assert_eq!(aggregate.try_get::<i16, _>("lineage_state").unwrap(), 2);
    assert_eq!(aggregate.try_get::<i64, _>("checkpoint_count").unwrap(), 0);

    let custody = sqlx::query(
        "SELECT
            count(*) FILTER (WHERE location_kind IN (0,1) AND security_state=0) AS stabilized,
            count(*) FILTER (WHERE location_kind=4 AND security_state=3
                AND destruction_reason='recall') AS destroyed,
            count(*) FILTER (WHERE terminal_recall_id=$1) AS terminal_items
         FROM item_instances
         WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4",
    )
    .bind(TERMINAL_ID.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    assert_eq!(custody.try_get::<i64, _>("stabilized").unwrap(), 2);
    assert_eq!(custody.try_get::<i64, _>("destroyed").unwrap(), 2);
    assert_eq!(custody.try_get::<i64, _>("terminal_items").unwrap(), 4);
    let material: (i32, i64, i16, String, Vec<u8>) = sqlx::query_as(
        "SELECT quantity,material_version,security_state,terminal_reason,terminal_recall_id
         FROM character_run_material_stacks
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND material_id=$4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(MATERIAL_ID)
    .fetch_one(verification.connection())
    .await
    .unwrap();
    assert_eq!(material, (0, 2, 3, "recall".into(), TERMINAL_ID.to_vec()));
    verification.rollback().await.unwrap();

    let mut later_hall_mutation = reconnected.begin_transaction().await.unwrap();
    sqlx::query(
        "UPDATE character_inventories
         SET inventory_version=inventory_version+1,updated_at=transaction_timestamp()
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
           AND inventory_version=$4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(i64::try_from(result.versions.inventory.post).unwrap())
    .execute(later_hall_mutation.connection())
    .await
    .unwrap();
    later_hall_mutation.commit().await.unwrap();

    let historical = reconnected
        .load_committed_recall_terminal_by_identity_v1(
            ACCOUNT_ID,
            CHARACTER_ID,
            MUTATION_ID,
            TERMINAL_ID,
        )
        .await
        .unwrap()
        .expect("immutable historical Recall recovery");
    assert_eq!(historical.result, result);
    assert!(
        !historical.owns_current_hall,
        "later Hall activity must not invalidate history or masquerade as the current terminal"
    );
    assert!(matches!(
        reconnected
            .load_committed_recall_terminal_v1([0; 16], CHARACTER_ID)
            .await,
        Err(PersistenceError::CorruptStoredRecall)
    ));
    reconnected.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn link_lost_uses_ninety_ticks_and_altered_replay_is_audited_once() {
    let persistence = disposable_database().await;
    reset_fixture(&persistence).await;
    let request = request(ProductionRecallTriggerV1::LinkLost);
    let prepared = persistence
        .prepare_production_recall_v1(&request)
        .await
        .unwrap();
    let committed = persistence
        .commit_production_recall_v1(&request, prepared.canonical_plan_hash())
        .await
        .unwrap();
    let result = committed.result().unwrap();
    assert_eq!(result.completion_tick, START_TICK + 90);
    assert_eq!(result.post_lifetime_ticks, 12_090);
    assert_eq!(result.post_permadeath_combat_ticks, 10_090);
    assert!(result.request_sequence.is_none());
    assert!(result.explicit_client_tick.is_none());

    let mut altered = request.clone();
    altered.final_permadeath_combat_ticks += 1;
    assert!(matches!(
        persistence.prepare_production_recall_v1(&altered).await,
        Err(PersistenceError::RecallIdempotencyConflict)
    ));
    assert!(matches!(
        persistence.prepare_production_recall_v1(&altered).await,
        Err(PersistenceError::RecallIdempotencyConflict)
    ));
    let mut verification = persistence.begin_transaction().await.unwrap();
    let conflicts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM recall_terminal_conflict_audits_v1
         WHERE namespace_id=$1 AND stored_terminal_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(TERMINAL_ID.as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    assert_eq!(conflicts, 1);
    verification.rollback().await.unwrap();
    persistence.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn sealed_plan_drift_fails_before_any_recall_write() {
    let persistence = disposable_database().await;
    reset_fixture(&persistence).await;
    let request = request(ProductionRecallTriggerV1::Explicit);
    let prepared = persistence
        .prepare_production_recall_v1(&request)
        .await
        .unwrap();

    let mut mutation = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO item_instances
         (namespace_id,item_uid,account_id,character_id,template_id,content_revision,
          item_kind,item_level,rarity,creation_kind,creation_request_id,roll_index,
          unit_ordinal,item_version,security_state,location_kind,slot_index,
          provenance_kind,salvage_band,salvage_value)
         VALUES ($1,$2,$3,$4,'item.armor.pilgrim.t1',$5,
          0,8,1,1,$2,4,0,1,2,2,1,1,1,12)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind([44_u8; 16].as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(CORE_ITEM_CONTENT_REVISION)
    .execute(mutation.connection())
    .await
    .unwrap();
    mutation.commit().await.unwrap();

    assert!(matches!(
        persistence
            .commit_production_recall_v1(&request, prepared.canonical_plan_hash())
            .await,
        Err(PersistenceError::ProductionRecallPlanChanged)
    ));
    let mut verification = persistence.begin_transaction().await.unwrap();
    let terminal_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM character_recall_terminal_results_v1
         WHERE namespace_id=$1 AND terminal_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(TERMINAL_ID.as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    let root_state: i16 = sqlx::query_scalar(
        "SELECT restore_state FROM character_entry_restore_points
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
           AND restore_point_id=$4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(RESTORE_POINT_ID.as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    assert_eq!(terminal_count, 0);
    assert_eq!(root_state, 0);
    verification.rollback().await.unwrap();
    persistence.close().await;
}
