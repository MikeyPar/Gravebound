use persistence::{
    AuthoritativeDeathPlanV1, CORE_DEATH_VIEW_ASSETS_BLAKE3, CORE_DEATH_VIEW_LOCALIZATION_BLAKE3,
    CORE_DEATH_VIEW_RECORDS_BLAKE3, CORE_ITEM_CONTENT_REVISION, CORE_WORLD_ASSETS_BLAKE3,
    CORE_WORLD_LOCALIZATION_BLAKE3, CORE_WORLD_RECORDS_BLAKE3, DURABLE_DEATH_SCHEMA_VERSION,
    DURABLE_DEATH_SUMMARY_REVISION, DURABLE_DEATH_TELEMETRY_CONTEXT_SCHEMA_VERSION,
    DeathAggregateVersionsV1, DeathVersionAdvanceV1, DeathViewReadError, DurableCombatTraceEntryV1,
    DurableDamageTypeV1, DurableDeathCauseV1, DurableDeathCommitRequestV1,
    DurableDeathContentAuthorityV1, DurableDeathEventV1, DurableDeathItemContentAuthorityV1,
    DurableDeathNetworkHealthV1, DurableDeathPresentationAuthorityV1, DurableDeathProvenanceV1,
    DurableDeathSummaryV1, DurableDeathTelemetryContextV1, DurableDeathTracePromotionV1,
    DurableDeathTransactionV1, DurableDestructionEntryV1, DurableDestructionLocationV1,
    DurableEchoEnvelopeV1, DurableEchoOutcomeV1, DurableEchoRecordV1, DurableEchoStateV1,
    DurableEchoTransitionReasonV1, DurableEchoTransitionV1, DurableEquipmentSlotV1,
    DurableMemorialRecordV1, DurableNetworkStateV1, DurableOrderedContentIdV1,
    DurableRecallStateV1, DurableSuccessorPresetV1, DurableSummaryDamageReferenceV1,
    DurableSummaryProjectionEntryV1, DurableSummaryProjectionKindV1, DurableSummaryProjectionsV1,
    DurableTraceStatusV1, LiveDamageTraceCauseV1, LiveDamageTraceContentAuthorityV1,
    LiveDamageTraceDamageTypeV1, LiveDamageTraceDangerAuthorityV1, LiveDamageTraceEntryV1,
    LiveDamageTraceNetworkStateV1, LiveDamageTraceRecallStateV1, LiveDamageTraceStatusV1,
    LiveDamageTraceTickCommandV1, LiveDamageTraceTickRequestV1, LiveDamageTraceTickTransactionV1,
    PersistenceConfig, PersistenceError, PostgresM03TelemetryOutboxAdapter, PostgresPersistence,
    StoredLiveDamageTraceSnapshotEntryV1, StoredPrivateLifeBootstrapStateV1,
    TelemetryPseudonymizationKeyV1, WIPEABLE_CORE_NAMESPACE,
    canonical_death_terminal_payload_hash_v1, derive_durable_death_item_ledger_event_id,
    stage_danger_entry_ash_wallet_restore_v3, stage_danger_entry_inventory_restore_v3,
    stage_danger_entry_life_metrics_restore_v3, stage_danger_entry_oath_bargain_restore_v3,
};
use serde::Serialize;
use sqlx::Row;
use telemetry::{
    CommittedTelemetrySource, TelemetryConnectivity, TelemetryIngestOutcome, TelemetryPipeline,
    TelemetryPipelineMode,
};

#[path = "support/terminal_telemetry.rs"]
mod terminal_telemetry;

const TELEMETRY_SESSION_ID: [u8; 16] = [0xE3; 16];

const ACCOUNT_ID: [u8; 16] = [230; 16];
const CHARACTER_ID: [u8; 16] = [231; 16];
const SECOND_CHARACTER_ID: [u8; 16] = [201; 16];
const LINEAGE_ID: [u8; 16] = [232; 16];
const SECOND_LINEAGE_ID: [u8; 16] = [202; 16];
const RESTORE_POINT_ID: [u8; 16] = [233; 16];
const SECOND_RESTORE_POINT_ID: [u8; 16] = [203; 16];
const INSTANCE_ID: [u8; 16] = [234; 16];
const SECOND_INSTANCE_ID: [u8; 16] = [204; 16];
const ITEM_UID: [u8; 16] = [235; 16];
const CHARACTER_SAFE_ITEM_UID: [u8; 16] = [241; 16];
const VAULT_ITEM_UID: [u8; 16] = [242; 16];
const ENTRY_MUTATION_ID: [u8; 16] = [237; 16];
const SECOND_ENTRY_MUTATION_ID: [u8; 16] = [205; 16];
const DEED_REWARD_ID: [u8; 16] = [238; 16];
const SECOND_DEED_REWARD_ID: [u8; 16] = [206; 16];
const MATERIAL_ID: &str = "material.bell_brass";
const ITEM_TEMPLATE_ID: &str = "item.weapon.crossbow.pine_crossbow";
const DEED_ID: &str = "deed.core.sir_caldus_defeated";
const RECORDS_BLAKE3: &str = CORE_WORLD_RECORDS_BLAKE3;
const ASSETS_BLAKE3: &str = CORE_WORLD_ASSETS_BLAKE3;
const LOCALIZATION_BLAKE3: &str = CORE_WORLD_LOCALIZATION_BLAKE3;
const ISSUED_AT_UNIX_MS: u64 = 1;
const NONLETHAL_TRACE_TICK_ID: [u8; 16] = [239; 16];
const LETHAL_TRACE_TICK_ID: [u8; 16] = [240; 16];
const SECOND_NONLETHAL_TRACE_TICK_ID: [u8; 16] = [207; 16];
const SECOND_LETHAL_TRACE_TICK_ID: [u8; 16] = [208; 16];
const SOURCE_SIM_ENTITY_ID: u64 = 81;

#[derive(Clone, Copy)]
#[allow(
    clippy::struct_field_names,
    reason = "the fixture keeps each distinct durable identity axis explicit"
)]
struct RequestIds {
    death_id: [u8; 16],
    echo_id: [u8; 16],
    mutation_id: [u8; 16],
}

impl RequestIds {
    fn primary() -> Self {
        Self {
            death_id: uuid_v7(41),
            echo_id: uuid_v7(42),
            mutation_id: [43; 16],
        }
    }

    fn changed_payload() -> Self {
        Self {
            death_id: uuid_v7(44),
            echo_id: uuid_v7(45),
            mutation_id: Self::primary().mutation_id,
        }
    }

    fn changed_final_identity() -> Self {
        Self {
            death_id: uuid_v7(46),
            echo_id: uuid_v7(47),
            mutation_id: [48; 16],
        }
    }

    fn secondary() -> Self {
        Self {
            death_id: uuid_v7(51),
            echo_id: uuid_v7(52),
            mutation_id: [53; 16],
        }
    }

    fn incident() -> Self {
        Self {
            death_id: uuid_v7(54),
            echo_id: uuid_v7(55),
            mutation_id: [56; 16],
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FixtureCustody {
    Full,
    Empty,
}

impl FixtureCustody {
    const fn entry_inventory_version(self) -> i64 {
        match self {
            Self::Full => 2,
            Self::Empty => 1,
        }
    }
}

fn uuid_v7(seed: u8) -> [u8; 16] {
    let mut value = [seed; 16];
    value[6] = 0x70 | (seed & 0x0f);
    value[8] = 0x80 | (seed & 0x3f);
    value
}

fn content_authority() -> DurableDeathContentAuthorityV1 {
    DurableDeathContentAuthorityV1 {
        content_revision: CORE_ITEM_CONTENT_REVISION.into(),
        records_blake3: RECORDS_BLAKE3.into(),
        assets_blake3: ASSETS_BLAKE3.into(),
        localization_blake3: LOCALIZATION_BLAKE3.into(),
        enabled_items: vec![DurableDeathItemContentAuthorityV1 {
            template_id: ITEM_TEMPLATE_ID.into(),
            echo_signature_tag: None,
        }],
    }
}

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

async fn reset_fixture(persistence: &PostgresPersistence) {
    reset_fixture_with_custody(persistence, FixtureCustody::Full).await;
}

async fn reset_zero_custody_fixture(persistence: &PostgresPersistence) {
    reset_fixture_with_custody(persistence, FixtureCustody::Empty).await;
}

#[allow(
    clippy::too_many_lines,
    reason = "the hosted fixture keeps the complete V3 danger root explicit and auditable"
)]
async fn reset_fixture_with_custody(persistence: &PostgresPersistence, custody: FixtureCustody) {
    persistence.reset_disposable_identity_data().await.unwrap();
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO accounts (namespace_id,account_id,state_version,slot_capacity) \
         VALUES ($1,$2,1,2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ash_wallets (namespace_id,account_id,balance,wallet_version) \
         VALUES ($1,$2,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO characters (namespace_id,account_id,character_id,roster_ordinal,class_id, \
         level,oath_id,life_state,security_state,character_state_version) \
         VALUES ($1,$2,$3,1,'class.grave_arbalist',10,NULL,0,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE accounts SET selected_character_id=$1 WHERE namespace_id=$2 AND account_id=$3",
    )
    .bind(CHARACTER_ID.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_progression (namespace_id,account_id,character_id,total_xp,level, \
         current_health,progression_version) VALUES ($1,$2,$3,2700,10,120,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_inventories (namespace_id,account_id,character_id,inventory_version) \
         VALUES ($1,$2,$3,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    if custody == FixtureCustody::Full {
        sqlx::query(
            "INSERT INTO item_instances (namespace_id,item_uid,account_id,character_id,template_id, \
             content_revision,item_kind,item_level,rarity,creation_kind,creation_request_id,roll_index, \
             unit_ordinal,item_version,security_state,location_kind,slot_index,provenance_kind, \
             salvage_band,salvage_value) \
             VALUES ($1,$2,$3,$4,$5,$6,0,10,0,0,$2,1,0,1,0,5,0,0,0,0), \
                    ($1,$7,$3,NULL,$5,$6,0,10,0,0,$7,2,0,1,0,6,0,0,0,0)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(CHARACTER_SAFE_ITEM_UID.as_slice())
        .bind(ACCOUNT_ID.as_slice())
        .bind(CHARACTER_ID.as_slice())
        .bind(ITEM_TEMPLATE_ID)
        .bind(CORE_ITEM_CONTENT_REVISION)
        .bind(VAULT_ITEM_UID.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO character_oath_bargain_state (namespace_id,account_id,character_id, \
         earned_bargain_slots,oath_bargain_version) VALUES ($1,$2,$3,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    if custody == FixtureCustody::Full {
        sqlx::query(
            "INSERT INTO item_instances (namespace_id,item_uid,account_id,character_id,template_id, \
             content_revision,item_kind,item_level,rarity,creation_kind,creation_request_id,roll_index, \
             unit_ordinal,item_version,security_state,location_kind,slot_index,provenance_kind, \
             salvage_band,salvage_value) \
             VALUES ($1,$2,$3,$4,$5,$6,0,10,0,0,$2,0,0,1,0,0,0,0,0,0)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ITEM_UID.as_slice())
        .bind(ACCOUNT_ID.as_slice())
        .bind(CHARACTER_ID.as_slice())
        .bind(ITEM_TEMPLATE_ID)
        .bind(CORE_ITEM_CONTENT_REVISION)
        .execute(transaction.connection())
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO character_instance_lineages (namespace_id,account_id,character_id,lineage_id, \
         content_id,layout_id,lineage_state,records_blake3,assets_blake3,localization_blake3) \
         VALUES ($1,$2,$3,$4,'world.core_microrealm_01','layout.core_private_life_01',0,$5,$6,$7)",
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
        "INSERT INTO character_entry_restore_points (namespace_id,account_id,character_id, \
         restore_point_id,lineage_id,source_location_id,restore_location_id, \
         snapshot_contract_version,account_version,character_version,progression_version, \
         inventory_version,oath_bargain_version,life_metrics_version,ash_wallet_version, \
         component_mask,composite_digest,restore_state,records_blake3,assets_blake3, \
         localization_blake3) VALUES ($1,$2,$3,$4,$5,'hub.lantern_halls_01', \
         'hub.lantern_halls_01',3,1,1,1,$6,1,1,1,31,$7,0,$8,$9,$10)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(RESTORE_POINT_ID.as_slice())
    .bind(LINEAGE_ID.as_slice())
    .bind(custody.entry_inventory_version())
    .bind([91_u8; 32].as_slice())
    .bind(CORE_WORLD_RECORDS_BLAKE3)
    .bind(CORE_WORLD_ASSETS_BLAKE3)
    .bind(CORE_WORLD_LOCALIZATION_BLAKE3)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO entry_restore_progression_v3 (namespace_id,account_id,character_id, \
         restore_point_id,level,total_xp,current_health,progression_version,component_digest) \
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
        "INSERT INTO entry_restore_progression_v1 (namespace_id,account_id,character_id, \
         restore_point_id,level,total_xp,current_health,progression_version) \
         VALUES ($1,$2,$3,$4,10,2700,120,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(RESTORE_POINT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    let staged_inventory = stage_danger_entry_inventory_restore_v3(
        &mut transaction,
        ACCOUNT_ID,
        CHARACTER_ID,
        RESTORE_POINT_ID,
        ENTRY_MUTATION_ID,
        0,
    )
    .await
    .unwrap();
    assert_eq!(
        staged_inventory.post_inventory_version,
        u64::try_from(custody.entry_inventory_version()).unwrap()
    );
    assert_eq!(
        staged_inventory.items.len(),
        usize::from(custody == FixtureCustody::Full)
    );
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
        "INSERT INTO character_world_locations (namespace_id,account_id,character_id, \
         character_version,location_kind,location_content_id,instance_lineage_id, \
         entry_restore_point_id) VALUES ($1,$2,$3,2,2,'world.core_microrealm_01',$4,$5)",
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
        "UPDATE characters SET character_state_version=2 WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_progression SET current_health=50,progression_version=2 \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_life_metrics SET lifetime_ticks=19990,permadeath_combat_ticks=17990, \
         life_metrics_version=2 WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    let checkpoint_payload = [1_u8];
    let checkpoint_payload_digest = blake3::hash(&checkpoint_payload);
    sqlx::query(
        "INSERT INTO character_danger_checkpoints (namespace_id,account_id,character_id, \
         lineage_id,checkpoint_tick,component_mask,composite_digest,character_version, \
         progression_version,inventory_version,oath_bargain_version,records_blake3, \
         assets_blake3,localization_blake3,checkpoint_schema_version,checkpoint_payload, \
         checkpoint_payload_digest) VALUES ($1,$2,$3,$4,19990,15,$5,2,2,$6,1,$7,$8,$9,1,$10,$11)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(LINEAGE_ID.as_slice())
    .bind([94_u8; 32].as_slice())
    .bind(custody.entry_inventory_version())
    .bind(CORE_WORLD_RECORDS_BLAKE3)
    .bind(CORE_WORLD_ASSETS_BLAKE3)
    .bind(CORE_WORLD_LOCALIZATION_BLAKE3)
    .bind(checkpoint_payload.as_slice())
    .bind(checkpoint_payload_digest.as_bytes().as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    if custody == FixtureCustody::Full {
        sqlx::query(
            "INSERT INTO character_run_material_stacks (namespace_id,account_id,character_id, \
             material_id,quantity,material_version,security_state) VALUES ($1,$2,$3,$4,7,1,2)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ACCOUNT_ID.as_slice())
        .bind(CHARACTER_ID.as_slice())
        .bind(MATERIAL_ID)
        .execute(transaction.connection())
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO character_life_deeds (namespace_id,account_id,character_id,deed_id, \
         reward_event_id,source_content_id,deed_kind,achieved_tick,content_revision) \
         VALUES ($1,$2,$3,$4,$5,'boss.sir_caldus',0,19000,$6)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(DEED_ID)
    .bind(DEED_REWARD_ID.as_slice())
    .bind(CORE_ITEM_CONTENT_REVISION)
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

#[allow(
    clippy::too_many_lines,
    reason = "the second-slot fixture keeps supersession and provenance evidence explicit"
)]
async fn stage_second_zero_custody_character(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO characters (namespace_id,account_id,character_id,roster_ordinal,class_id, \
         level,oath_id,life_state,security_state,character_state_version) \
         VALUES ($1,$2,$3,2,'class.grave_arbalist',10,NULL,0,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(SECOND_CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_progression (namespace_id,account_id,character_id,total_xp,level, \
         current_health,progression_version) VALUES ($1,$2,$3,2700,10,120,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(SECOND_CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_inventories (namespace_id,account_id,character_id,inventory_version) \
         VALUES ($1,$2,$3,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(SECOND_CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_oath_bargain_state (namespace_id,account_id,character_id, \
         earned_bargain_slots,oath_bargain_version) VALUES ($1,$2,$3,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(SECOND_CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_instance_lineages (namespace_id,account_id,character_id,lineage_id, \
         content_id,layout_id,lineage_state,records_blake3,assets_blake3,localization_blake3) \
         VALUES ($1,$2,$3,$4,'world.core_microrealm_01','layout.core_private_life_01',0,$5,$6,$7)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(SECOND_CHARACTER_ID.as_slice())
    .bind(SECOND_LINEAGE_ID.as_slice())
    .bind(CORE_WORLD_RECORDS_BLAKE3)
    .bind(CORE_WORLD_ASSETS_BLAKE3)
    .bind(CORE_WORLD_LOCALIZATION_BLAKE3)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_entry_restore_points (namespace_id,account_id,character_id, \
         restore_point_id,lineage_id,source_location_id,restore_location_id, \
         snapshot_contract_version,account_version,character_version,progression_version, \
         inventory_version,oath_bargain_version,life_metrics_version,ash_wallet_version, \
         component_mask,composite_digest,restore_state,records_blake3,assets_blake3, \
         localization_blake3) VALUES ($1,$2,$3,$4,$5,'hub.lantern_halls_01', \
         'hub.lantern_halls_01',3,1,1,1,1,1,1,1,31,$6,0,$7,$8,$9)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(SECOND_CHARACTER_ID.as_slice())
    .bind(SECOND_RESTORE_POINT_ID.as_slice())
    .bind(SECOND_LINEAGE_ID.as_slice())
    .bind([101_u8; 32].as_slice())
    .bind(CORE_WORLD_RECORDS_BLAKE3)
    .bind(CORE_WORLD_ASSETS_BLAKE3)
    .bind(CORE_WORLD_LOCALIZATION_BLAKE3)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO entry_restore_progression_v3 (namespace_id,account_id,character_id, \
         restore_point_id,level,total_xp,current_health,progression_version,component_digest) \
         VALUES ($1,$2,$3,$4,10,2700,120,1,$5)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(SECOND_CHARACTER_ID.as_slice())
    .bind(SECOND_RESTORE_POINT_ID.as_slice())
    .bind([102_u8; 32].as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO entry_restore_progression_v1 (namespace_id,account_id,character_id, \
         restore_point_id,level,total_xp,current_health,progression_version) \
         VALUES ($1,$2,$3,$4,10,2700,120,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(SECOND_CHARACTER_ID.as_slice())
    .bind(SECOND_RESTORE_POINT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    let staged_inventory = stage_danger_entry_inventory_restore_v3(
        &mut transaction,
        ACCOUNT_ID,
        SECOND_CHARACTER_ID,
        SECOND_RESTORE_POINT_ID,
        SECOND_ENTRY_MUTATION_ID,
        0,
    )
    .await
    .unwrap();
    assert_eq!(staged_inventory.post_inventory_version, 1);
    assert!(staged_inventory.items.is_empty());
    stage_danger_entry_oath_bargain_restore_v3(
        &mut transaction,
        ACCOUNT_ID,
        SECOND_CHARACTER_ID,
        SECOND_RESTORE_POINT_ID,
    )
    .await
    .unwrap();
    stage_danger_entry_life_metrics_restore_v3(
        &mut transaction,
        ACCOUNT_ID,
        SECOND_CHARACTER_ID,
        SECOND_RESTORE_POINT_ID,
    )
    .await
    .unwrap();
    stage_danger_entry_ash_wallet_restore_v3(
        &mut transaction,
        ACCOUNT_ID,
        SECOND_CHARACTER_ID,
        SECOND_RESTORE_POINT_ID,
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_world_locations (namespace_id,account_id,character_id, \
         character_version,location_kind,location_content_id,instance_lineage_id, \
         entry_restore_point_id) VALUES ($1,$2,$3,2,2,'world.core_microrealm_01',$4,$5)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(SECOND_CHARACTER_ID.as_slice())
    .bind(SECOND_LINEAGE_ID.as_slice())
    .bind(SECOND_RESTORE_POINT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE characters SET character_state_version=2 WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(SECOND_CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_progression SET current_health=50,progression_version=2 \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(SECOND_CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_life_metrics SET lifetime_ticks=19990,permadeath_combat_ticks=17990, \
         life_metrics_version=2 WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(SECOND_CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    let checkpoint_payload = [2_u8];
    let checkpoint_payload_digest = blake3::hash(&checkpoint_payload);
    sqlx::query(
        "INSERT INTO character_danger_checkpoints (namespace_id,account_id,character_id, \
         lineage_id,checkpoint_tick,component_mask,composite_digest,character_version, \
         progression_version,inventory_version,oath_bargain_version,records_blake3, \
         assets_blake3,localization_blake3,checkpoint_schema_version,checkpoint_payload, \
         checkpoint_payload_digest) VALUES ($1,$2,$3,$4,19990,15,$5,2,2,1,1,$6,$7,$8,1,$9,$10)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(SECOND_CHARACTER_ID.as_slice())
    .bind(SECOND_LINEAGE_ID.as_slice())
    .bind([104_u8; 32].as_slice())
    .bind(CORE_WORLD_RECORDS_BLAKE3)
    .bind(CORE_WORLD_ASSETS_BLAKE3)
    .bind(CORE_WORLD_LOCALIZATION_BLAKE3)
    .bind(checkpoint_payload.as_slice())
    .bind(checkpoint_payload_digest.as_bytes().as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_life_deeds (namespace_id,account_id,character_id,deed_id, \
         reward_event_id,source_content_id,deed_kind,achieved_tick,content_revision) \
         VALUES ($1,$2,$3,$4,$5,'boss.sir_caldus',0,19000,$6)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(SECOND_CHARACTER_ID.as_slice())
    .bind(DEED_ID)
    .bind(SECOND_DEED_REWARD_ID.as_slice())
    .bind(CORE_ITEM_CONTENT_REVISION)
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn promote_fixture_lineage_to_active(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let changed = sqlx::query(
        "UPDATE character_instance_lineages SET lineage_state=1 \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND lineage_id=$4 \
           AND lineage_state=0 AND closed_at IS NULL",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(LINEAGE_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(changed, 1);
    transaction.commit().await.unwrap();
}

#[allow(
    clippy::too_many_lines,
    reason = "one complete canonical request graph remains contiguous for authority review"
)]
fn request(ids: RequestIds) -> DurableDeathCommitRequestV1 {
    let trace = vec![
        DurableCombatTraceEntryV1 {
            ordinal: 0,
            event_tick: 19_990,
            event_ordinal: 0,
            source_content_id: "miniboss.sepulcher_knight".into(),
            source_entity_id: Some([81; 16]),
            pattern_id: Some("miniboss.sepulcher_knight.charge_lane".into()),
            attack_id: "miniboss.sepulcher_knight.charge_lane".into(),
            raw_damage: 10,
            final_damage: 10,
            damage_type: DurableDamageTypeV1::Physical,
            pre_health: 60,
            post_health: 50,
            source_x_milli_tiles: 1_250,
            source_y_milli_tiles: -500,
            network_state: DurableNetworkStateV1::Connected,
            recall_state: DurableRecallStateV1::Inactive,
            lethal: false,
            statuses: vec![DurableTraceStatusV1 {
                ordinal: 0,
                status_id: "status.hex".into(),
                remaining_ticks: 30,
                stack_count: 1,
            }],
        },
        DurableCombatTraceEntryV1 {
            ordinal: 1,
            event_tick: 20_000,
            event_ordinal: 0,
            source_content_id: "miniboss.sepulcher_knight".into(),
            source_entity_id: Some([81; 16]),
            pattern_id: Some("miniboss.sepulcher_knight.charge_lane".into()),
            attack_id: "miniboss.sepulcher_knight.charge_lane".into(),
            raw_damage: 60,
            final_damage: 60,
            damage_type: DurableDamageTypeV1::Physical,
            pre_health: 50,
            post_health: 0,
            source_x_milli_tiles: 1_250,
            source_y_milli_tiles: -500,
            network_state: DurableNetworkStateV1::Connected,
            recall_state: DurableRecallStateV1::Inactive,
            lethal: true,
            statuses: vec![],
        },
    ];
    let destruction = vec![
        DurableDestructionEntryV1::Item {
            ordinal: 0,
            content_id: ITEM_TEMPLATE_ID.into(),
            item_uid: ITEM_UID,
            location: DurableDestructionLocationV1::Equipment {
                slot: DurableEquipmentSlotV1::Weapon,
            },
            pre_item_version: 2,
            post_item_version: 3,
            ledger_event_id: derive_durable_death_item_ledger_event_id(
                ids.death_id,
                ids.mutation_id,
                ITEM_UID,
            ),
        },
        DurableDestructionEntryV1::RunMaterial {
            ordinal: 1,
            material_id: MATERIAL_ID.into(),
            destroyed_quantity: 7,
            pre_material_quantity: 7,
            pre_material_version: 1,
            post_material_version: 2,
        },
    ];
    let versions = DeathAggregateVersionsV1 {
        account: DeathVersionAdvanceV1 { pre: 1, post: 2 },
        character: DeathVersionAdvanceV1 { pre: 2, post: 3 },
        progression: DeathVersionAdvanceV1 { pre: 2, post: 3 },
        inventory: DeathVersionAdvanceV1 { pre: 2, post: 3 },
        oath_bargain: DeathVersionAdvanceV1 { pre: 1, post: 2 },
        life_metrics: DeathVersionAdvanceV1 { pre: 2, post: 3 },
    };
    let event = DurableDeathEventV1 {
        schema_version: DURABLE_DEATH_SCHEMA_VERSION,
        namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
        death_id: ids.death_id,
        account_id: ACCOUNT_ID,
        character_id: CHARACTER_ID,
        former_roster_ordinal: 1,
        mutation_id: ids.mutation_id,
        bargain_cleanup_event_id: derived_id(
            "gravebound.death.bargain-cleanup-id.v1",
            &[ids.death_id.as_slice(), ids.mutation_id.as_slice()],
        ),
        canonical_request_hash: [1; 32],
        content_revision: CORE_ITEM_CONTENT_REVISION.into(),
        records_blake3: RECORDS_BLAKE3.into(),
        assets_blake3: ASSETS_BLAKE3.into(),
        localization_blake3: LOCALIZATION_BLAKE3.into(),
        presentation: DurableDeathPresentationAuthorityV1::core(),
        instance_id: INSTANCE_ID,
        lineage_id: LINEAGE_ID,
        restore_point_id: RESTORE_POINT_ID,
        region_id: "region.core.microrealm".into(),
        room_id: "room.core.sepulcher".into(),
        provenance: DurableDeathProvenanceV1::OrdinaryGameplay,
        death_tick: 20_000,
        committed_at_unix_ms: ISSUED_AT_UNIX_MS,
        cause: DurableDeathCauseV1::DirectHit,
        killer_content_id: "miniboss.sepulcher_knight".into(),
        killer_pattern_id: Some("miniboss.sepulcher_knight.charge_lane".into()),
        killer_attack_id: "miniboss.sepulcher_knight.charge_lane".into(),
        raw_damage: 60,
        final_damage: 60,
        damage_type: DurableDamageTypeV1::Physical,
        pre_hit_health: 50,
        source_x_milli_tiles: 1_250,
        source_y_milli_tiles: -500,
        network_state: DurableNetworkStateV1::Connected,
        recall_state: DurableRecallStateV1::Inactive,
        telemetry: DurableDeathTelemetryContextV1::Observed {
            schema_version: DURABLE_DEATH_TELEMETRY_CONTEXT_SCHEMA_VERSION,
            party_size: 1,
            boss_phase_id: Some("boss.caldus.phase_2".into()),
            contribution: Some(persistence::DurableDeathContributionV1 {
                contribution_centi_units: 360_000,
                reference_health: 7_200,
            }),
            network_health: DurableDeathNetworkHealthV1 {
                transport_generation: 1,
                sampled_at_unix_ms: ISSUED_AT_UNIX_MS,
                ping_millis: 80,
                jitter_millis: 12,
                loss_basis_points: 100,
                correction_count: None,
            },
        },
        lifetime_ticks: 20_000,
        permadeath_combat_ticks: 18_000,
        versions,
        trace_entry_count: 2,
        trace_digest: canonical_digest("gravebound.durable-death.trace.v1", &trace),
        destruction_entry_count: 2,
        destruction_digest: canonical_digest(
            "gravebound.durable-death.destruction.v1",
            &destruction,
        ),
    };
    let projections = DurableSummaryProjectionsV1 {
        lost: vec![
            projection(
                0,
                DurableSummaryProjectionKindV1::LostItem,
                ITEM_TEMPLATE_ID,
                1,
                Some(ITEM_UID),
            ),
            projection(
                1,
                DurableSummaryProjectionKindV1::LostRunMaterial,
                MATERIAL_ID,
                7,
                None,
            ),
        ],
        preserved: vec![
            fixed_projection(
                0,
                DurableSummaryProjectionKindV1::PreservedAccountRecords,
                "projection.preserved.account_records",
            ),
            fixed_projection(
                1,
                DurableSummaryProjectionKindV1::PreservedCurrency,
                "projection.preserved.currency",
            ),
            fixed_projection(
                2,
                DurableSummaryProjectionKindV1::PreservedVault,
                "projection.preserved.vault",
            ),
            fixed_projection(
                3,
                DurableSummaryProjectionKindV1::PreservedCosmetics,
                "projection.preserved.cosmetics",
            ),
            fixed_projection(
                4,
                DurableSummaryProjectionKindV1::PreservedRecipes,
                "projection.preserved.recipes",
            ),
        ],
        created: vec![
            fixed_projection(
                0,
                DurableSummaryProjectionKindV1::CreatedMemorial,
                "projection.created.memorial",
            ),
            fixed_projection(
                1,
                DurableSummaryProjectionKindV1::CreatedEcho,
                "projection.created.echo",
            ),
        ],
    };
    let mut summary = DurableDeathSummaryV1 {
        schema_version: DURABLE_DEATH_SCHEMA_VERSION,
        namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
        death_id: ids.death_id,
        summary_revision: DURABLE_DEATH_SUMMARY_REVISION,
        hero_label_key: "hero.core.grave_arbalist".into(),
        character_name_snapshot: "Hosted Hero".into(),
        class_id: "class.grave_arbalist".into(),
        level: 10,
        oath_id: None,
        bargains: vec![],
        lifetime_ms: 666_666,
        final_deed_id: DEED_ID.into(),
        lethal_trace_ordinal: 1,
        last_five_damage: vec![
            DurableSummaryDamageReferenceV1 {
                ordinal: 0,
                trace_ordinal: 0,
            },
            DurableSummaryDamageReferenceV1 {
                ordinal: 1,
                trace_ordinal: 1,
            },
        ],
        projections,
        echo_outcome: DurableEchoOutcomeV1::Available,
        content_revision: CORE_ITEM_CONTENT_REVISION.into(),
        snapshot_digest: [0; 32],
    };
    summary.snapshot_digest = summary.expected_snapshot_digest().unwrap();
    let mut memorial = DurableMemorialRecordV1 {
        schema_version: DURABLE_DEATH_SCHEMA_VERSION,
        namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
        death_id: ids.death_id,
        account_id: ACCOUNT_ID,
        death_at_unix_ms: ISSUED_AT_UNIX_MS,
        summary_revision: DURABLE_DEATH_SUMMARY_REVISION,
        summary_snapshot_digest: summary.snapshot_digest,
        presentation_key: "memorial.presentation.core_default".into(),
        presentation_digest: [0; 32],
    };
    memorial.presentation_digest = memorial.expected_presentation_digest().unwrap();
    let mut echo = DurableEchoRecordV1 {
        schema_version: DURABLE_DEATH_SCHEMA_VERSION,
        namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
        echo_id: ids.echo_id,
        death_id: ids.death_id,
        account_id: ACCOUNT_ID,
        character_name_snapshot: summary.character_name_snapshot.clone(),
        class_id: summary.class_id.clone(),
        oath_id: None,
        level: 10,
        appearance_snapshot_id: persistence::CORE_ECHO_BASE_SILHOUETTE_ID.into(),
        appearance_theme_id: persistence::CORE_ECHO_PRESENTATION_PLACEHOLDER_ID.into(),
        weapon_signature_tag: None,
        relic_signature_tag: None,
        bargains: vec![],
        deed_tags: vec![DurableOrderedContentIdV1 {
            ordinal: 0,
            content_id: DEED_ID.into(),
        }],
        killer_content_id: event.killer_content_id.clone(),
        killer_pattern_id: event.killer_pattern_id.clone(),
        death_region_id: event.region_id.clone(),
        power_band: 1,
        created_at_unix_ms: ISSUED_AT_UNIX_MS,
        state: DurableEchoStateV1::Available,
        content_revision: CORE_ITEM_CONTENT_REVISION.into(),
        snapshot_digest: [0; 32],
    };
    echo.snapshot_digest = echo.expected_snapshot_digest().unwrap();
    let echo_envelope = DurableEchoEnvelopeV1 {
        created: echo,
        creation_transition: DurableEchoTransitionV1 {
            echo_id: ids.echo_id,
            echo_death_id: ids.death_id,
            ordinal: 0,
            previous_state: None,
            next_state: DurableEchoStateV1::Dormant,
            reason: DurableEchoTransitionReasonV1::EligibleDeath,
            source_death_id: Some(ids.death_id),
            trigger_death_id: ids.death_id,
            committed_at_unix_ms: ISSUED_AT_UNIX_MS,
        },
        preexisting_available_echo_id: None,
        promotion: Some(DurableEchoTransitionV1 {
            echo_id: ids.echo_id,
            echo_death_id: ids.death_id,
            ordinal: 1,
            previous_state: Some(DurableEchoStateV1::Dormant),
            next_state: DurableEchoStateV1::Available,
            reason: DurableEchoTransitionReasonV1::OldestDormantPromotion,
            source_death_id: None,
            trigger_death_id: ids.death_id,
            committed_at_unix_ms: ISSUED_AT_UNIX_MS,
        }),
    };
    DurableDeathCommitRequestV1::seal(
        AuthoritativeDeathPlanV1 {
            schema_version: DURABLE_DEATH_SCHEMA_VERSION,
            event,
            trace,
            destruction,
            summary,
            memorial,
            echo: Some(echo_envelope),
        },
        ISSUED_AT_UNIX_MS,
    )
    .unwrap()
}

fn zero_custody_request(ids: RequestIds) -> DurableDeathCommitRequestV1 {
    let mut plan = request(ids).plan;
    plan.destruction.clear();
    plan.event.versions.inventory = DeathVersionAdvanceV1 { pre: 1, post: 2 };
    plan.event.destruction_entry_count = 0;
    plan.event.destruction_digest =
        canonical_digest("gravebound.durable-death.destruction.v1", &plan.destruction);
    plan.summary.projections.lost.clear();
    plan.summary.snapshot_digest = plan.summary.expected_snapshot_digest().unwrap();
    plan.memorial.summary_snapshot_digest = plan.summary.snapshot_digest;
    plan.memorial.presentation_digest = plan.memorial.expected_presentation_digest().unwrap();
    let echo = plan
        .echo
        .as_mut()
        .expect("qualifying zero-custody death retains the mandatory Echo");
    echo.created.power_band = 1;
    echo.created.snapshot_digest = echo.created.expected_snapshot_digest().unwrap();
    DurableDeathCommitRequestV1::seal(plan, ISSUED_AT_UNIX_MS).unwrap()
}

fn second_zero_custody_request(
    ids: RequestIds,
    provenance: DurableDeathProvenanceV1,
) -> DurableDeathCommitRequestV1 {
    let mut plan = zero_custody_request(ids).plan;
    plan.event.character_id = SECOND_CHARACTER_ID;
    plan.event.former_roster_ordinal = 2;
    plan.event.instance_id = SECOND_INSTANCE_ID;
    plan.event.lineage_id = SECOND_LINEAGE_ID;
    plan.event.restore_point_id = SECOND_RESTORE_POINT_ID;
    plan.event.versions.account = DeathVersionAdvanceV1 { pre: 2, post: 3 };
    plan.event.provenance = provenance;
    if provenance == DurableDeathProvenanceV1::OrdinaryGameplay {
        let echo = plan
            .echo
            .as_mut()
            .expect("later ordinary death retains the mandatory Echo projector");
        echo.created.state = DurableEchoStateV1::Dormant;
        echo.created.snapshot_digest = echo.created.expected_snapshot_digest().unwrap();
        echo.preexisting_available_echo_id = Some(RequestIds::primary().echo_id);
        echo.promotion = None;
        plan.summary.echo_outcome = DurableEchoOutcomeV1::Dormant;
        plan.summary.snapshot_digest = plan.summary.expected_snapshot_digest().unwrap();
        plan.memorial.summary_snapshot_digest = plan.summary.snapshot_digest;
        plan.memorial.presentation_digest = plan.memorial.expected_presentation_digest().unwrap();
    } else {
        plan.echo = None;
        plan.summary.echo_outcome = DurableEchoOutcomeV1::NotEligible;
        plan.summary.snapshot_digest = plan.summary.expected_snapshot_digest().unwrap();
        plan.memorial.summary_snapshot_digest = plan.summary.snapshot_digest;
        plan.memorial.presentation_digest = plan.memorial.expected_presentation_digest().unwrap();
    }
    DurableDeathCommitRequestV1::seal(plan, ISSUED_AT_UNIX_MS).unwrap()
}

/// Hosted lethal evidence required jointly by canonical GDD `DTH-001`, Content Spec
/// `CONT-ECHO-009`, and Roadmap `GB-M03-02`/`06`/`13`. The first tick is committed through the
/// production live repository; the lethal suffix remains sealed for the atomic death writer.
#[derive(Clone)]
struct HostedDeathTraceEvidence {
    lethal_request: LiveDamageTraceTickRequestV1,
    full_window: Vec<StoredLiveDamageTraceSnapshotEntryV1>,
}

impl HostedDeathTraceEvidence {
    fn promotion_for(&self, death: &DurableDeathCommitRequestV1) -> DurableDeathTracePromotionV1 {
        DurableDeathTracePromotionV1::seal(death, self.lethal_request.clone(), &self.full_window)
            .unwrap()
    }

    fn altered_predecessor_promotion_for(
        &self,
        death: &DurableDeathCommitRequestV1,
    ) -> DurableDeathTracePromotionV1 {
        let mut command = self.lethal_request.command.clone();
        command.expected_previous.as_mut().unwrap().result_digest[0] ^= 1;
        let lethal_request = LiveDamageTraceTickRequestV1::seal(command).unwrap();
        DurableDeathTracePromotionV1::seal(death, lethal_request, &self.full_window).unwrap()
    }
}

async fn seed_hosted_death_trace(
    persistence: &PostgresPersistence,
    death: &DurableDeathCommitRequestV1,
) -> HostedDeathTraceEvidence {
    let event = &death.plan.event;
    let (nonlethal_trace_tick_id, lethal_trace_tick_id) =
        if event.character_id == SECOND_CHARACTER_ID {
            (SECOND_NONLETHAL_TRACE_TICK_ID, SECOND_LETHAL_TRACE_TICK_ID)
        } else {
            (NONLETHAL_TRACE_TICK_ID, LETHAL_TRACE_TICK_ID)
        };
    let danger = LiveDamageTraceDangerAuthorityV1 {
        lineage_id: event.lineage_id,
        restore_point_id: event.restore_point_id,
        checkpoint_tick: 19_990,
    };
    let content = LiveDamageTraceContentAuthorityV1::core();
    let nonlethal_entry = live_trace_entry(&death.plan.trace[0]);
    let nonlethal_request = LiveDamageTraceTickRequestV1::seal(LiveDamageTraceTickCommandV1 {
        account_id: event.account_id,
        character_id: event.character_id,
        trace_tick_id: nonlethal_trace_tick_id,
        expected_character_version: death.plan.event.versions.character.pre,
        expected_previous: None,
        event_tick: death.plan.trace[0].event_tick,
        danger: danger.clone(),
        content: content.clone(),
        entries: vec![nonlethal_entry],
        issued_at_unix_ms: ISSUED_AT_UNIX_MS,
    })
    .unwrap();
    let stored = persistence
        .transact_live_damage_trace_tick_v1(&nonlethal_request)
        .await
        .unwrap();
    let head = match stored {
        LiveDamageTraceTickTransactionV1::Committed(stored) => stored.head(),
        LiveDamageTraceTickTransactionV1::Replayed(_) => {
            panic!("fixture reset must produce one fresh retained nonlethal receipt")
        }
    };

    let snapshot = persistence
        .load_live_damage_trace_snapshot_v1(event.account_id, event.character_id)
        .await
        .unwrap();
    assert_eq!(
        snapshot.character_version,
        death.plan.event.versions.character.pre
    );
    assert_eq!(snapshot.danger, danger);
    assert_eq!(snapshot.content, content.clone());
    assert_eq!(snapshot.head.as_ref(), Some(&head));
    assert_eq!(snapshot.entries.len(), 1);

    let lethal_entry = live_trace_entry(death.plan.trace.last().unwrap());
    let lethal_request = LiveDamageTraceTickRequestV1::seal(LiveDamageTraceTickCommandV1 {
        account_id: event.account_id,
        character_id: event.character_id,
        trace_tick_id: lethal_trace_tick_id,
        expected_character_version: death.plan.event.versions.character.pre,
        expected_previous: Some(head),
        event_tick: death.plan.event.death_tick,
        danger,
        content,
        entries: vec![lethal_entry.clone()],
        issued_at_unix_ms: ISSUED_AT_UNIX_MS,
    })
    .unwrap();
    let mut full_window = snapshot.entries;
    full_window.push(StoredLiveDamageTraceSnapshotEntryV1 {
        trace_tick_id: lethal_trace_tick_id,
        event_tick: death.plan.event.death_tick,
        entry: lethal_entry,
    });
    let evidence = HostedDeathTraceEvidence {
        lethal_request,
        full_window,
    };
    evidence
        .promotion_for(death)
        .validate_against(death, &evidence.full_window)
        .unwrap();
    evidence
}

fn live_trace_entry(entry: &DurableCombatTraceEntryV1) -> LiveDamageTraceEntryV1 {
    LiveDamageTraceEntryV1 {
        event_ordinal: entry.event_ordinal,
        cause: LiveDamageTraceCauseV1::DirectHit,
        source_content_id: entry.source_content_id.clone(),
        source_entity_id: entry.source_entity_id,
        source_sim_entity_id: entry.source_entity_id.map(|_| SOURCE_SIM_ENTITY_ID),
        pattern_id: entry.pattern_id.clone(),
        attack_id: entry.attack_id.clone(),
        raw_damage: entry.raw_damage,
        final_damage: entry.final_damage,
        damage_type: match entry.damage_type {
            DurableDamageTypeV1::Physical => LiveDamageTraceDamageTypeV1::Physical,
            DurableDamageTypeV1::Veil => LiveDamageTraceDamageTypeV1::Veil,
        },
        pre_health: entry.pre_health,
        post_health: entry.post_health,
        source_x_milli_tiles: entry.source_x_milli_tiles,
        source_y_milli_tiles: entry.source_y_milli_tiles,
        network_state: match entry.network_state {
            DurableNetworkStateV1::Connected => LiveDamageTraceNetworkStateV1::Connected,
            DurableNetworkStateV1::Degraded => LiveDamageTraceNetworkStateV1::Degraded,
            DurableNetworkStateV1::LinkLost => LiveDamageTraceNetworkStateV1::LinkLost,
            DurableNetworkStateV1::Reattached => LiveDamageTraceNetworkStateV1::Reattached,
        },
        recall_state: match entry.recall_state {
            DurableRecallStateV1::Inactive => LiveDamageTraceRecallStateV1::Inactive,
            DurableRecallStateV1::Channeling => LiveDamageTraceRecallStateV1::Channeling,
            DurableRecallStateV1::CompletionPending => {
                LiveDamageTraceRecallStateV1::CompletionPending
            }
        },
        lethal: entry.lethal,
        statuses: entry
            .statuses
            .iter()
            .map(|status| LiveDamageTraceStatusV1 {
                status_ordinal: status.ordinal,
                status_id: status.status_id.clone(),
                remaining_ticks: status.remaining_ticks,
                stack_count: status.stack_count,
            })
            .collect(),
    }
}

fn projection(
    ordinal: u16,
    kind: DurableSummaryProjectionKindV1,
    content_id: &str,
    quantity: u32,
    item_uid: Option<[u8; 16]>,
) -> DurableSummaryProjectionEntryV1 {
    DurableSummaryProjectionEntryV1 {
        ordinal,
        kind,
        content_id: content_id.into(),
        quantity,
        item_uid,
    }
}

fn fixed_projection(
    ordinal: u16,
    kind: DurableSummaryProjectionKindV1,
    content_id: &str,
) -> DurableSummaryProjectionEntryV1 {
    projection(ordinal, kind, content_id, 1, None)
}

fn canonical_digest<T: Serialize>(context: &str, value: &T) -> [u8; 32] {
    blake3::derive_key(context, &postcard::to_stdvec(value).unwrap())
}

fn derived_id(context: &str, parts: &[&[u8]]) -> [u8; 16] {
    let mut hasher = blake3::Hasher::new_derive_key(context);
    for part in parts {
        hasher.update(&(part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    let mut value = [0_u8; 16];
    value.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
    if value == [0; 16] {
        value[15] = 1;
    }
    value
}

#[allow(
    clippy::too_many_lines,
    reason = "the explicit table allowlist keeps every terminal-graph count query auditable"
)]
async fn count(persistence: &PostgresPersistence, table: &str) -> i64 {
    let query = match table {
        "accounts" => "SELECT count(*) FROM accounts WHERE namespace_id=$1 AND account_id=$2",
        "characters" => "SELECT count(*) FROM characters WHERE namespace_id=$1 AND account_id=$2",
        "item_instances" => {
            "SELECT count(*) FROM item_instances WHERE namespace_id=$1 AND account_id=$2"
        }
        "item_ledger_events" => {
            "SELECT count(*) FROM item_ledger_events WHERE namespace_id=$1 AND account_id=$2"
        }
        "character_run_material_stacks" => {
            "SELECT count(*) FROM character_run_material_stacks \
             WHERE namespace_id=$1 AND account_id=$2"
        }
        "entry_restore_inventory_items_v3" => {
            "SELECT count(*) FROM entry_restore_inventory_items_v3 \
             WHERE namespace_id=$1 AND account_id=$2"
        }
        "character_life_outbox" => {
            "SELECT count(*) FROM character_life_outbox WHERE namespace_id=$1 AND account_id=$2"
        }
        "character_danger_checkpoints" => {
            "SELECT count(*) FROM character_danger_checkpoints \
             WHERE namespace_id=$1 AND account_id=$2"
        }
        "death_events" => {
            "SELECT count(*) FROM death_events WHERE namespace_id=$1 AND account_id=$2"
        }
        "death_combat_trace_entries" => {
            "SELECT count(*) FROM death_combat_trace_entries AS trace \
             JOIN death_events AS death USING (namespace_id,death_id) \
             WHERE trace.namespace_id=$1 AND death.account_id=$2"
        }
        "death_summary_snapshots" => {
            "SELECT count(*) FROM death_summary_snapshots AS summary \
             JOIN death_events AS death USING (namespace_id,death_id) \
             WHERE summary.namespace_id=$1 AND death.account_id=$2"
        }
        "memorial_records" => {
            "SELECT count(*) FROM memorial_records WHERE namespace_id=$1 AND account_id=$2"
        }
        "death_destruction_entries" => {
            "SELECT count(*) FROM death_destruction_entries AS destroyed \
             JOIN death_events AS death USING (namespace_id,death_id) \
             WHERE destroyed.namespace_id=$1 AND death.account_id=$2"
        }
        "death_mutation_results" => {
            "SELECT count(*) FROM death_mutation_results WHERE namespace_id=$1 AND account_id=$2"
        }
        "death_audit_events" => {
            "SELECT count(*) FROM death_audit_events WHERE namespace_id=$1 AND account_id=$2"
        }
        "echo_records" => {
            "SELECT count(*) FROM echo_records WHERE namespace_id=$1 AND account_id=$2"
        }
        "echo_state_transitions" => {
            "SELECT count(*) FROM echo_state_transitions AS transition \
             JOIN echo_records AS echo USING (namespace_id,echo_id) \
             WHERE transition.namespace_id=$1 AND echo.account_id=$2"
        }
        "death_outbox_events" => {
            "SELECT count(*) FROM death_outbox_events AS outbox \
             JOIN death_events AS death USING (namespace_id,death_id) \
             WHERE outbox.namespace_id=$1 AND death.account_id=$2"
        }
        "death_successor_presets_v1" => {
            "SELECT count(*) FROM death_successor_presets_v1 \
             WHERE namespace_id=$1 AND account_id=$2"
        }
        "successor_roster_reservations_v1" => {
            "SELECT count(*) FROM successor_roster_reservations_v1 \
             WHERE namespace_id=$1 AND account_id=$2"
        }
        "character_live_damage_trace_ingest_receipts_v1" => {
            "SELECT count(*) FROM character_live_damage_trace_ingest_receipts_v1 \
             WHERE namespace_id=$1 AND account_id=$2"
        }
        "death_live_trace_sets_v1" => {
            "SELECT count(*) FROM death_live_trace_sets_v1 \
             WHERE namespace_id=$1 AND account_id=$2"
        }
        "death_live_trace_receipt_links_v1" => {
            "SELECT count(*) FROM death_live_trace_receipt_links_v1 AS link \
             JOIN death_events AS death USING (namespace_id,death_id) \
             WHERE link.namespace_id=$1 AND death.account_id=$2"
        }
        "death_live_trace_entry_provenance_v1" => {
            "SELECT count(*) FROM death_live_trace_entry_provenance_v1 AS provenance \
             JOIN death_events AS death USING (namespace_id,death_id) \
             WHERE provenance.namespace_id=$1 AND death.account_id=$2"
        }
        "death_live_trace_promotion_conflict_audits_v1" => {
            "SELECT count(*) FROM death_live_trace_promotion_conflict_audits_v1 \
             WHERE namespace_id=$1 AND account_id=$2"
        }
        "onboarding_outbox_events_v1" => {
            "SELECT count(*) FROM onboarding_outbox_events_v1 \
             WHERE namespace_id=$1 AND account_id=$2"
        }
        "item_ledger_telemetry_outbox_v1" => {
            "SELECT count(*) FROM item_ledger_telemetry_outbox_v1 \
             WHERE namespace_id=$1 AND account_id=$2"
        }
        _ => panic!("unsupported fixture table {table}"),
    };
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let value = sqlx::query_scalar::<_, i64>(query)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ACCOUNT_ID.as_slice())
        .fetch_one(transaction.connection())
        .await
        .unwrap();
    transaction.rollback().await.unwrap();
    value
}

async fn assert_successor_death_authority(
    persistence: &PostgresPersistence,
    death: &DurableDeathCommitRequestV1,
    expected_reservation_state: i16,
    expected_superseded_by_death_id: Option<[u8; 16]>,
) {
    // Authorities: Production GDD DTH-020/021 and TECH-021/023, Content Spec
    // CONT-CATALOG-003, and Roadmap GB-M03-07 require the immutable preset and roster
    // reservation to be part of the committed ordinary-death graph, never reconstructed later.
    let expected = DurableSuccessorPresetV1::from_death_plan(&death.plan)
        .unwrap()
        .expect("ordinary death carries universal successor authority");
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let row = sqlx::query(
        "SELECT preset.former_character_id,preset.preset_revision, \
                preset.former_roster_ordinal,preset.class_id,preset.appearance_kind, \
                preset.base_silhouette_id,preset.content_revision,preset.preset_hash, \
                reservation.reservation_state,reservation.superseded_by_death_id, \
                preset.created_at = reservation.reserved_at AS one_commit_timestamp \
         FROM death_successor_presets_v1 AS preset \
         JOIN successor_roster_reservations_v1 AS reservation \
           USING (namespace_id,account_id,death_id,former_roster_ordinal) \
         WHERE preset.namespace_id=$1 AND preset.account_id=$2 AND preset.death_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(death.plan.event.account_id.as_slice())
    .bind(death.plan.event.death_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(
        row.get::<Vec<u8>, _>("former_character_id"),
        expected.former_character_id
    );
    assert_eq!(row.get::<i16, _>("preset_revision"), 1);
    assert_eq!(
        row.get::<i16, _>("former_roster_ordinal"),
        i16::from(expected.former_roster_ordinal)
    );
    assert_eq!(row.get::<String, _>("class_id"), expected.class_id);
    assert_eq!(
        row.get::<i16, _>("appearance_kind"),
        i16::try_from(expected.appearance_kind).unwrap()
    );
    assert_eq!(
        row.get::<String, _>("base_silhouette_id"),
        expected.base_silhouette_id
    );
    assert_eq!(
        row.get::<String, _>("content_revision"),
        expected.content_revision
    );
    assert_eq!(row.get::<Vec<u8>, _>("preset_hash"), expected.preset_hash);
    assert_eq!(
        row.get::<i16, _>("reservation_state"),
        expected_reservation_state
    );
    assert_eq!(
        row.get::<Option<Vec<u8>>, _>("superseded_by_death_id"),
        expected_superseded_by_death_id.map(Vec::from)
    );
    assert!(row.get::<bool, _>("one_commit_timestamp"));
    transaction.rollback().await.unwrap();
}

async fn assert_no_successor_death_authority(
    persistence: &PostgresPersistence,
    death_id: [u8; 16],
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let counts = sqlx::query(
        "SELECT \
            (SELECT count(*) FROM death_successor_presets_v1 \
             WHERE namespace_id=$1 AND account_id=$2 AND death_id=$3) AS preset_count, \
            (SELECT count(*) FROM successor_roster_reservations_v1 \
             WHERE namespace_id=$1 AND account_id=$2 AND death_id=$3) AS reservation_count",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(death_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(counts.get::<i64, _>("preset_count"), 0);
    assert_eq!(counts.get::<i64, _>("reservation_count"), 0);
    transaction.rollback().await.unwrap();
}

async fn select_second_character_after_primary_death(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let changed = sqlx::query(
        "UPDATE accounts SET selected_character_id=$1 \
         WHERE namespace_id=$2 AND account_id=$3 AND state_version=2 \
           AND selected_character_id IS NULL",
    )
    .bind(SECOND_CHARACTER_ID.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(changed, 1);
    transaction.commit().await.unwrap();
}

async fn assert_death_closed_lineage(connection: &mut sqlx::PgConnection) {
    let lineage_state: i16 = sqlx::query_scalar(
        "SELECT lineage_state FROM character_instance_lineages \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND lineage_id=$4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(LINEAGE_ID.as_slice())
    .fetch_one(connection)
    .await
    .unwrap();
    assert_eq!(
        lineage_state, 3,
        "durable death must seal either open phase"
    );
}

fn assert_terminal_root(root: &sqlx::postgres::PgRow, expected_inventory_version: i64) {
    assert_eq!(root.get::<i16, _>("life_state"), 1);
    assert_eq!(root.get::<Option<i16>, _>("roster_ordinal"), None);
    assert_eq!(root.get::<i64, _>("character_state_version"), 3);
    assert_eq!(root.get::<i64, _>("state_version"), 2);
    assert_eq!(
        root.get::<Option<Vec<u8>>, _>("selected_character_id"),
        None
    );
    assert_eq!(root.get::<i32, _>("current_health"), 0);
    assert_eq!(root.get::<i64, _>("progression_version"), 3);
    assert_eq!(
        root.get::<i64, _>("inventory_version"),
        expected_inventory_version
    );
    assert_eq!(root.get::<i64, _>("oath_bargain_version"), 2);
    assert_eq!(root.get::<i64, _>("lifetime_ticks"), 20_000);
    assert_eq!(root.get::<i64, _>("permadeath_combat_ticks"), 18_000);
    assert_eq!(root.get::<i64, _>("life_metrics_version"), 3);
}

async fn assert_safe_storage_preserved(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let safe_storage = sqlx::query(
        "SELECT item_uid,character_id,item_version,security_state,location_kind,slot_index, \
                destruction_reason,terminal_death_id \
         FROM item_instances WHERE namespace_id=$1 AND account_id=$2 AND item_uid IN ($3,$4) \
         ORDER BY item_uid",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_SAFE_ITEM_UID.as_slice())
    .bind(VAULT_ITEM_UID.as_slice())
    .fetch_all(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(safe_storage.len(), 2);
    assert_eq!(
        safe_storage[0].get::<Vec<u8>, _>("item_uid"),
        CHARACTER_SAFE_ITEM_UID
    );
    assert_eq!(
        safe_storage[0].get::<Option<Vec<u8>>, _>("character_id"),
        Some(CHARACTER_ID.to_vec())
    );
    assert_eq!(safe_storage[0].get::<i16, _>("location_kind"), 5);
    assert_eq!(safe_storage[0].get::<Option<i16>, _>("slot_index"), Some(0));
    assert_eq!(
        safe_storage[1].get::<Vec<u8>, _>("item_uid"),
        VAULT_ITEM_UID
    );
    assert_eq!(
        safe_storage[1].get::<Option<Vec<u8>>, _>("character_id"),
        None
    );
    assert_eq!(safe_storage[1].get::<i16, _>("location_kind"), 6);
    assert_eq!(safe_storage[1].get::<Option<i16>, _>("slot_index"), Some(0));
    for row in &safe_storage {
        assert_eq!(row.get::<i64, _>("item_version"), 1);
        assert_eq!(row.get::<i16, _>("security_state"), 0);
        assert_eq!(row.get::<Option<String>, _>("destruction_reason"), None);
        assert_eq!(row.get::<Option<Vec<u8>>, _>("terminal_death_id"), None);
    }
}

async fn normalized_live_trace_counts(persistence: &PostgresPersistence) -> (i64, i64, i64) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let counts = sqlx::query_as(
        "SELECT \
            (SELECT count(*) FROM character_live_damage_trace_ticks_v1 \
             WHERE namespace_id=$1 AND account_id=$2), \
            (SELECT count(*) FROM character_live_damage_trace_entries_v1 \
             WHERE namespace_id=$1 AND account_id=$2), \
            (SELECT count(*) FROM character_live_damage_trace_statuses_v1 \
             WHERE namespace_id=$1 AND account_id=$2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    counts
}

async fn assert_normalized_live_trace_absent(persistence: &PostgresPersistence) {
    // GDD TECH-023, Content Production Spec exact encounter authority, and Roadmap
    // GB-M03-02/06/13 require terminal cleanup only after the immutable trace has committed.
    assert_eq!(normalized_live_trace_counts(persistence).await, (0, 0, 0));
}

async fn assert_complete_graph(persistence: &PostgresPersistence, ids: RequestIds) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let root = sqlx::query(
        "SELECT character.life_state,character.roster_ordinal,character.character_state_version, \
                account.state_version,account.selected_character_id,progression.current_health, \
                progression.progression_version,inventory.inventory_version, \
                oath.oath_bargain_version,life.lifetime_ticks,life.permadeath_combat_ticks, \
                life.life_metrics_version \
         FROM characters AS character JOIN accounts AS account USING (namespace_id,account_id) \
         JOIN character_progression AS progression USING (namespace_id,account_id,character_id) \
         JOIN character_inventories AS inventory USING (namespace_id,account_id,character_id) \
         JOIN character_oath_bargain_state AS oath USING (namespace_id,account_id,character_id) \
         JOIN character_life_metrics AS life USING (namespace_id,account_id,character_id) \
         WHERE character.namespace_id=$1 AND character.account_id=$2 AND character.character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_terminal_root(&root, 3);
    assert_death_closed_lineage(transaction.connection()).await;

    let item = sqlx::query(
        "SELECT item_version,security_state,location_kind,destruction_reason FROM item_instances \
         WHERE namespace_id=$1 AND item_uid=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ITEM_UID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(item.get::<i64, _>("item_version"), 3);
    assert_eq!(item.get::<i16, _>("security_state"), 3);
    assert_eq!(item.get::<i16, _>("location_kind"), 4);
    assert_eq!(item.get::<String, _>("destruction_reason"), "permadeath");
    let material = sqlx::query(
        "SELECT quantity,material_version,security_state,terminal_reason,terminal_death_id \
         FROM character_run_material_stacks WHERE namespace_id=$1 AND account_id=$2 \
         AND character_id=$3 AND material_id=$4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(MATERIAL_ID)
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(material.get::<i32, _>("quantity"), 0);
    assert_eq!(material.get::<i64, _>("material_version"), 2);
    assert_eq!(material.get::<i16, _>("security_state"), 3);
    assert_eq!(material.get::<String, _>("terminal_reason"), "permadeath");
    assert_eq!(
        material.get::<Vec<u8>, _>("terminal_death_id"),
        ids.death_id
    );
    let echo = sqlx::query(
        "SELECT state,power_band FROM echo_records WHERE namespace_id=$1 AND echo_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.echo_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(echo.get::<i16, _>("state"), 1);
    assert_eq!(echo.get::<i16, _>("power_band"), 1);
    transaction.rollback().await.unwrap();
    assert_safe_storage_preserved(persistence).await;
    assert_normalized_live_trace_absent(persistence).await;

    for (table, expected) in [
        ("character_danger_checkpoints", 0),
        ("item_ledger_events", 2),
        ("death_events", 1),
        ("death_combat_trace_entries", 2),
        ("death_summary_snapshots", 1),
        ("memorial_records", 1),
        ("death_destruction_entries", 2),
        ("death_mutation_results", 1),
        ("death_audit_events", 1),
        ("echo_records", 1),
        ("echo_state_transitions", 2),
        ("death_outbox_events", 3),
        ("death_successor_presets_v1", 1),
        ("successor_roster_reservations_v1", 1),
        ("character_live_damage_trace_ingest_receipts_v1", 2),
        ("death_live_trace_sets_v1", 1),
        ("death_live_trace_receipt_links_v1", 2),
        ("death_live_trace_entry_provenance_v1", 2),
        ("death_live_trace_promotion_conflict_audits_v1", 0),
    ] {
        assert_eq!(count(persistence, table).await, expected, "{table}");
    }
    assert_successor_death_authority(persistence, &request(ids), 0, None).await;
}

async fn assert_zero_custody_complete_graph(persistence: &PostgresPersistence, ids: RequestIds) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let root = sqlx::query(
        "SELECT character.life_state,character.roster_ordinal,character.character_state_version, \
                account.state_version,account.selected_character_id,progression.current_health, \
                progression.progression_version,inventory.inventory_version, \
                oath.oath_bargain_version,life.lifetime_ticks,life.permadeath_combat_ticks, \
                life.life_metrics_version \
         FROM characters AS character JOIN accounts AS account USING (namespace_id,account_id) \
         JOIN character_progression AS progression USING (namespace_id,account_id,character_id) \
         JOIN character_inventories AS inventory USING (namespace_id,account_id,character_id) \
         JOIN character_oath_bargain_state AS oath USING (namespace_id,account_id,character_id) \
         JOIN character_life_metrics AS life USING (namespace_id,account_id,character_id) \
         WHERE character.namespace_id=$1 AND character.account_id=$2 AND character.character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_terminal_root(&root, 2);
    assert_death_closed_lineage(transaction.connection()).await;
    let echo = sqlx::query(
        "SELECT state,power_band FROM echo_records WHERE namespace_id=$1 AND echo_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.echo_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(echo.get::<i16, _>("state"), 1);
    assert_eq!(echo.get::<i16, _>("power_band"), 1);
    transaction.rollback().await.unwrap();

    assert_normalized_live_trace_absent(persistence).await;
    for (table, expected) in [
        ("item_instances", 0),
        ("item_ledger_events", 0),
        ("character_run_material_stacks", 0),
        ("entry_restore_inventory_items_v3", 0),
        ("character_danger_checkpoints", 0),
        ("character_life_outbox", 1),
        ("death_events", 1),
        ("death_combat_trace_entries", 2),
        ("death_summary_snapshots", 1),
        ("memorial_records", 1),
        ("death_destruction_entries", 0),
        ("death_mutation_results", 1),
        ("death_audit_events", 1),
        ("echo_records", 1),
        ("echo_state_transitions", 2),
        ("death_outbox_events", 3),
        ("death_successor_presets_v1", 1),
        ("successor_roster_reservations_v1", 1),
        ("character_live_damage_trace_ingest_receipts_v1", 2),
        ("death_live_trace_sets_v1", 1),
        ("death_live_trace_receipt_links_v1", 2),
        ("death_live_trace_entry_provenance_v1", 2),
        ("death_live_trace_promotion_conflict_audits_v1", 0),
    ] {
        assert_eq!(count(persistence, table).await, expected, "{table}");
    }
    assert_successor_death_authority(persistence, &zero_custody_request(ids), 0, None).await;

    let latest = persistence
        .load_latest_committed_death_view(ACCOUNT_ID)
        .await
        .unwrap()
        .expect("zero-custody death remains the latest committed death");
    assert_eq!(latest.death_id, ids.death_id);
    assert_eq!(latest.destruction_entry_count, 0);
    let summary = persistence
        .load_owned_death_summary_view(ACCOUNT_ID, ids.death_id, 0, 32)
        .await
        .unwrap();
    assert_eq!(summary.lost_total_count, 0);
    assert!(summary.lost.is_empty());
    assert!(summary.next_lost_ordinal.is_none());
    assert_eq!(summary.preserved.len(), 5);
    assert_eq!(summary.created.len(), 2);
    assert_eq!(summary.echo_outcome, DurableEchoOutcomeV1::Available);
    let memorial = persistence
        .load_death_memorial_page(ACCOUNT_ID, None, 32)
        .await
        .unwrap();
    assert_eq!(memorial.entries.len(), 1);
    assert_eq!(memorial.entries[0].cursor.death_id, ids.death_id);
    assert_eq!(
        memorial.entries[0].echo_outcome,
        DurableEchoOutcomeV1::Available
    );
}

async fn assert_rollback_pristine(persistence: &PostgresPersistence) {
    // GDD TECH-021/023, Content Spec CONT-ECHO-009, and Roadmap GB-M03-02D/06/13 require every
    // participant to roll back together. Check both the live authority heads and every immutable
    // family that could otherwise leak a partial terminal result.
    for (table, expected) in [
        ("character_danger_checkpoints", 1),
        ("character_life_outbox", 0),
        ("item_ledger_events", 1),
        ("death_events", 0),
        ("death_combat_trace_entries", 0),
        ("death_summary_snapshots", 0),
        ("memorial_records", 0),
        ("death_destruction_entries", 0),
        ("death_mutation_results", 0),
        ("death_audit_events", 0),
        ("death_outbox_events", 0),
        ("death_successor_presets_v1", 0),
        ("successor_roster_reservations_v1", 0),
        ("echo_records", 0),
        ("echo_state_transitions", 0),
        ("character_live_damage_trace_ingest_receipts_v1", 1),
        ("death_live_trace_sets_v1", 0),
        ("death_live_trace_receipt_links_v1", 0),
        ("death_live_trace_entry_provenance_v1", 0),
        ("death_live_trace_promotion_conflict_audits_v1", 0),
    ] {
        assert_eq!(count(persistence, table).await, expected, "{table}");
    }
    assert_eq!(normalized_live_trace_counts(persistence).await, (1, 1, 1));
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let row = sqlx::query(
        "SELECT account.state_version,account.selected_character_id,character.life_state, \
                character.roster_ordinal,character.character_state_version,item.item_version, \
                item.security_state,item.location_kind,item.destruction_reason IS NULL \
                    AS item_not_destroyed,item.terminal_death_id IS NULL AS item_not_terminal, \
                oath.oath_bargain_version,progression.current_health, \
                progression.progression_version,inventory.inventory_version, \
                life.lifetime_ticks,life.permadeath_combat_ticks,life.life_metrics_version, \
                world.character_version AS world_character_version,world.location_kind \
                    AS world_location_kind,world.instance_lineage_id,world.entry_restore_point_id, \
                root.restore_state,root.death_mutation_id IS NULL AS root_not_terminal, \
                lineage.lineage_state,lineage.closed_at IS NULL AS lineage_open, \
                material.quantity,material.material_version,material.security_state \
                    AS material_security_state,material.terminal_reason IS NULL \
                    AS material_not_terminal,material.terminal_death_id IS NULL \
                    AS material_without_death \
         FROM accounts AS account JOIN characters AS character USING (namespace_id,account_id) \
         JOIN item_instances AS item USING (namespace_id,account_id,character_id) \
         JOIN character_oath_bargain_state AS oath USING (namespace_id,account_id,character_id) \
         JOIN character_progression AS progression USING (namespace_id,account_id,character_id) \
         JOIN character_inventories AS inventory USING (namespace_id,account_id,character_id) \
         JOIN character_life_metrics AS life USING (namespace_id,account_id,character_id) \
         JOIN character_world_locations AS world USING (namespace_id,account_id,character_id) \
         JOIN character_entry_restore_points AS root USING (namespace_id,account_id,character_id) \
         JOIN character_instance_lineages AS lineage USING (namespace_id,account_id,character_id) \
         JOIN character_run_material_stacks AS material \
             USING (namespace_id,account_id,character_id) \
         WHERE account.namespace_id=$1 AND account.account_id=$2 AND character.character_id=$3 \
           AND item.item_uid=$4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(ITEM_UID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(row.get::<i64, _>("state_version"), 1);
    assert_eq!(row.get::<Vec<u8>, _>("selected_character_id"), CHARACTER_ID);
    assert_eq!(row.get::<i16, _>("life_state"), 0);
    assert_eq!(row.get::<Option<i16>, _>("roster_ordinal"), Some(1));
    assert_eq!(row.get::<i64, _>("character_state_version"), 2);
    assert_eq!(row.get::<i64, _>("item_version"), 2);
    assert_eq!(row.get::<i16, _>("security_state"), 1);
    assert_eq!(row.get::<i16, _>("location_kind"), 0);
    assert!(row.get::<bool, _>("item_not_destroyed"));
    assert!(row.get::<bool, _>("item_not_terminal"));
    assert_eq!(row.get::<i64, _>("oath_bargain_version"), 1);
    assert_eq!(row.get::<i32, _>("current_health"), 50);
    assert_eq!(row.get::<i64, _>("progression_version"), 2);
    assert_eq!(row.get::<i64, _>("inventory_version"), 2);
    assert_eq!(row.get::<i64, _>("lifetime_ticks"), 19_990);
    assert_eq!(row.get::<i64, _>("permadeath_combat_ticks"), 17_990);
    assert_eq!(row.get::<i64, _>("life_metrics_version"), 2);
    assert_eq!(row.get::<i64, _>("world_character_version"), 2);
    assert_eq!(row.get::<i16, _>("world_location_kind"), 2);
    assert_eq!(row.get::<Vec<u8>, _>("instance_lineage_id"), LINEAGE_ID);
    assert_eq!(
        row.get::<Vec<u8>, _>("entry_restore_point_id"),
        RESTORE_POINT_ID
    );
    assert_eq!(row.get::<i16, _>("restore_state"), 0);
    assert!(row.get::<bool, _>("root_not_terminal"));
    assert_eq!(row.get::<i16, _>("lineage_state"), 0);
    assert!(row.get::<bool, _>("lineage_open"));
    assert_eq!(row.get::<i32, _>("quantity"), 7);
    assert_eq!(row.get::<i64, _>("material_version"), 1);
    assert_eq!(row.get::<i16, _>("material_security_state"), 2);
    assert!(row.get::<bool, _>("material_not_terminal"));
    assert!(row.get::<bool, _>("material_without_death"));
    transaction.rollback().await.unwrap();
}

async fn corrupt_result_hash(persistence: &PostgresPersistence, hash: [u8; 32]) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query("ALTER TABLE death_mutation_results DISABLE TRIGGER death_results_immutable")
        .execute(transaction.connection())
        .await
        .unwrap();
    let changed = sqlx::query(
        "UPDATE death_mutation_results SET result_hash=$1 WHERE namespace_id=$2 \
         AND account_id=$3 AND mutation_id=$4",
    )
    .bind(hash.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(RequestIds::primary().mutation_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(changed, 1);
    sqlx::query("ALTER TABLE death_mutation_results ENABLE TRIGGER death_results_immutable")
        .execute(transaction.connection())
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn stored_receipt_window_digest(persistence: &PostgresPersistence) -> [u8; 32] {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let value: Vec<u8> = sqlx::query_scalar(
        "SELECT receipt_window_digest FROM death_live_trace_sets_v1 \
         WHERE namespace_id=$1 AND death_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(RequestIds::primary().death_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    value.try_into().unwrap()
}

async fn set_promotion_root_hashes(
    persistence: &PostgresPersistence,
    receipt_window_digest: [u8; 32],
    promotion_digest: [u8; 32],
    terminal_payload_hash: [u8; 32],
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "ALTER TABLE death_live_trace_sets_v1 \
         DISABLE TRIGGER death_live_trace_root_immutable_v1",
    )
    .execute(transaction.connection())
    .await
    .unwrap();
    let changed = sqlx::query(
        "UPDATE death_live_trace_sets_v1 SET receipt_window_digest=$1,promotion_digest=$2,\
            terminal_payload_hash=$3 WHERE namespace_id=$4 AND death_id=$5",
    )
    .bind(receipt_window_digest.as_slice())
    .bind(promotion_digest.as_slice())
    .bind(terminal_payload_hash.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(RequestIds::primary().death_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(changed, 1);
    sqlx::query(
        "ALTER TABLE death_live_trace_sets_v1 \
         ENABLE TRIGGER death_live_trace_root_immutable_v1",
    )
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn rewrite_durable_trace_attack_id(
    persistence: &PostgresPersistence,
    replacement: &str,
) -> String {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let original: String = sqlx::query_scalar(
        "SELECT attack_id FROM death_combat_trace_entries \
         WHERE namespace_id=$1 AND death_id=$2 AND trace_ordinal=0 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(RequestIds::primary().death_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    sqlx::query("ALTER TABLE death_combat_trace_entries DISABLE TRIGGER death_trace_immutable")
        .execute(transaction.connection())
        .await
        .unwrap();
    let changed = sqlx::query(
        "UPDATE death_combat_trace_entries SET attack_id=$1 \
         WHERE namespace_id=$2 AND death_id=$3 AND trace_ordinal=0",
    )
    .bind(replacement)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(RequestIds::primary().death_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(changed, 1);
    sqlx::query("ALTER TABLE death_combat_trace_entries ENABLE TRIGGER death_trace_immutable")
        .execute(transaction.connection())
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    original
}

async fn rewrite_first_durable_trace_status_remaining_ticks(
    persistence: &PostgresPersistence,
    replacement: u32,
) -> u32 {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let row = sqlx::query(
        "SELECT trace_ordinal,status_ordinal,remaining_ticks \
         FROM death_combat_trace_statuses WHERE namespace_id=$1 AND death_id=$2 \
         ORDER BY trace_ordinal,status_ordinal LIMIT 1 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(RequestIds::primary().death_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let trace_ordinal: i16 = row.get("trace_ordinal");
    let status_ordinal: i16 = row.get("status_ordinal");
    let original = u32::try_from(row.get::<i32, _>("remaining_ticks")).unwrap();
    sqlx::query(
        "ALTER TABLE death_combat_trace_statuses DISABLE TRIGGER death_trace_status_immutable",
    )
    .execute(transaction.connection())
    .await
    .unwrap();
    let changed = sqlx::query(
        "UPDATE death_combat_trace_statuses SET remaining_ticks=$1 \
         WHERE namespace_id=$2 AND death_id=$3 AND trace_ordinal=$4 AND status_ordinal=$5",
    )
    .bind(i32::try_from(replacement).unwrap())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(RequestIds::primary().death_id.as_slice())
    .bind(trace_ordinal)
    .bind(status_ordinal)
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(changed, 1);
    sqlx::query(
        "ALTER TABLE death_combat_trace_statuses ENABLE TRIGGER death_trace_status_immutable",
    )
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    original
}

#[derive(Clone, Copy)]
struct SignatureRowCorruption {
    label: &'static str,
    table: &'static str,
    triggers: &'static [&'static str],
    corrupt_sql: &'static str,
    restore_sql: &'static str,
}

const SIGNATURE_ROW_CORRUPTIONS: [SignatureRowCorruption; 4] = [
    SignatureRowCorruption {
        label: "death summary",
        table: "death_summary_snapshots",
        triggers: &["death_summary_immutable"],
        corrupt_sql: "UPDATE death_summary_snapshots \
            SET hero_label_key='hero.core.corrupt_signature' \
            WHERE namespace_id=$1 AND death_id=$2",
        restore_sql: "UPDATE death_summary_snapshots \
            SET hero_label_key='hero.core.grave_arbalist' \
            WHERE namespace_id=$1 AND death_id=$2",
    },
    SignatureRowCorruption {
        label: "Memorial",
        table: "memorial_records",
        triggers: &["memorial_records_immutable"],
        corrupt_sql: "UPDATE memorial_records \
            SET presentation_key='memorial.presentation.corrupt_signature' \
            WHERE namespace_id=$1 AND death_id=$2",
        restore_sql: "UPDATE memorial_records \
            SET presentation_key='memorial.presentation.core_default' \
            WHERE namespace_id=$1 AND death_id=$2",
    },
    SignatureRowCorruption {
        label: "destruction source",
        table: "death_destruction_entries",
        triggers: &[
            "death_destruction_immutable",
            "death_destruction_source_exact",
        ],
        corrupt_sql: "UPDATE death_destruction_entries SET pre_slot_index=1 \
            WHERE namespace_id=$1 AND death_id=$2 AND entry_kind=0",
        restore_sql: "UPDATE death_destruction_entries SET pre_slot_index=0 \
            WHERE namespace_id=$1 AND death_id=$2 AND entry_kind=0",
    },
    SignatureRowCorruption {
        label: "Echo snapshot",
        table: "echo_records",
        triggers: &["echo_records_transition_only"],
        corrupt_sql: "UPDATE echo_records \
            SET killer_pattern_id='pattern.core.corrupt_signature' \
            WHERE namespace_id=$1 AND death_id=$2",
        restore_sql: "UPDATE echo_records \
            SET killer_pattern_id='miniboss.sepulcher_knight.charge_lane' \
            WHERE namespace_id=$1 AND death_id=$2",
    },
];

async fn apply_signature_row_fixture_update(
    persistence: &PostgresPersistence,
    corruption: SignatureRowCorruption,
    sql: &'static str,
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    for trigger in corruption.triggers {
        let statement = format!(
            "ALTER TABLE {} DISABLE TRIGGER {}",
            corruption.table, trigger
        );
        sqlx::query(sqlx::AssertSqlSafe(statement))
            .execute(transaction.connection())
            .await
            .unwrap();
    }
    let changed = sqlx::query(sql)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(RequestIds::primary().death_id.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap()
        .rows_affected();
    assert_eq!(changed, 1, "{}", corruption.label);
    // Some row families retain independent deferred history/ownership checks while only their
    // immutability guard is bypassed. Flush those valid checks before changing trigger state;
    // PostgreSQL otherwise refuses ALTER TABLE while deferred events remain queued.
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(transaction.connection())
        .await
        .unwrap();
    for trigger in corruption.triggers.iter().rev() {
        let statement = format!(
            "ALTER TABLE {} ENABLE TRIGGER {}",
            corruption.table, trigger
        );
        sqlx::query(sqlx::AssertSqlSafe(statement))
            .execute(transaction.connection())
            .await
            .unwrap();
    }
    transaction.commit().await.unwrap();
}

async fn rewrite_bargain_cleanup_payload(
    persistence: &PostgresPersistence,
    replacement: &[u8],
) -> Vec<u8> {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let original: Vec<u8> = sqlx::query_scalar(
        "SELECT event_payload FROM character_life_outbox \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 \
           AND event_id=(SELECT bargain_cleanup_event_id FROM death_events \
             WHERE namespace_id=$1 AND death_id=$4)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(RequestIds::primary().death_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE character_life_outbox \
         DISABLE TRIGGER character_life_outbox_publish_only",
    )
    .execute(transaction.connection())
    .await
    .unwrap();
    let changed = sqlx::query(
        "UPDATE character_life_outbox SET event_payload=$1 \
         WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4 \
           AND event_id=(SELECT bargain_cleanup_event_id FROM death_events \
             WHERE namespace_id=$2 AND death_id=$5)",
    )
    .bind(replacement)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(RequestIds::primary().death_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(changed, 1);
    sqlx::query(
        "ALTER TABLE character_life_outbox \
         ENABLE TRIGGER character_life_outbox_publish_only",
    )
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    original
}

async fn assert_character_life_outbox_publish_contract(persistence: &PostgresPersistence) {
    let cleanup_predicate = "namespace_id=$1 AND account_id=$2 AND character_id=$3 \
        AND event_id=(SELECT bargain_cleanup_event_id FROM death_events \
          WHERE namespace_id=$1 AND death_id=$4)";

    let mut altered = persistence.begin_transaction().await.unwrap();
    let update_payload = format!(
        "UPDATE character_life_outbox SET event_payload=decode('ff','hex') \
         WHERE {cleanup_predicate}"
    );
    assert!(
        sqlx::query(sqlx::AssertSqlSafe(update_payload))
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(ACCOUNT_ID.as_slice())
            .bind(CHARACTER_ID.as_slice())
            .bind(RequestIds::primary().death_id.as_slice())
            .execute(altered.connection())
            .await
            .is_err(),
        "character-life payload mutation bypassed the append-only trigger"
    );
    altered.rollback().await.unwrap();

    let mut published = persistence.begin_transaction().await.unwrap();
    let publish = format!(
        "UPDATE character_life_outbox SET published_at=transaction_timestamp() \
         WHERE {cleanup_predicate}"
    );
    let changed = sqlx::query(sqlx::AssertSqlSafe(publish))
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ACCOUNT_ID.as_slice())
        .bind(CHARACTER_ID.as_slice())
        .bind(RequestIds::primary().death_id.as_slice())
        .execute(published.connection())
        .await
        .unwrap()
        .rows_affected();
    assert_eq!(changed, 1);
    published.commit().await.unwrap();

    let mut republished = persistence.begin_transaction().await.unwrap();
    let republish = format!(
        "UPDATE character_life_outbox \
         SET published_at=transaction_timestamp()+interval '1 millisecond' \
         WHERE {cleanup_predicate}"
    );
    assert!(
        sqlx::query(sqlx::AssertSqlSafe(republish))
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(ACCOUNT_ID.as_slice())
            .bind(CHARACTER_ID.as_slice())
            .bind(RequestIds::primary().death_id.as_slice())
            .execute(republished.connection())
            .await
            .is_err(),
        "character-life outbox accepted a second publication"
    );
    republished.rollback().await.unwrap();

    let mut deleted = persistence.begin_transaction().await.unwrap();
    let delete = format!("DELETE FROM character_life_outbox WHERE {cleanup_predicate}");
    assert!(
        sqlx::query(sqlx::AssertSqlSafe(delete))
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(ACCOUNT_ID.as_slice())
            .bind(CHARACTER_ID.as_slice())
            .bind(RequestIds::primary().death_id.as_slice())
            .execute(deleted.connection())
            .await
            .is_err(),
        "character-life outbox accepted a direct delete"
    );
    deleted.rollback().await.unwrap();
}

async fn rewrite_death_committed_outbox_payload(
    persistence: &PostgresPersistence,
    replacement: &[u8],
) -> Vec<u8> {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let original: Vec<u8> = sqlx::query_scalar(
        "SELECT event_payload FROM death_outbox_events \
         WHERE namespace_id=$1 AND death_id=$2 AND event_type='death_committed'",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(RequestIds::primary().death_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    sqlx::query("ALTER TABLE death_outbox_events DISABLE TRIGGER death_outbox_publish_only")
        .execute(transaction.connection())
        .await
        .unwrap();
    let changed = sqlx::query(
        "UPDATE death_outbox_events SET event_payload=$1 \
         WHERE namespace_id=$2 AND death_id=$3 AND event_type='death_committed'",
    )
    .bind(replacement)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(RequestIds::primary().death_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(changed, 1);
    sqlx::query("ALTER TABLE death_outbox_events ENABLE TRIGGER death_outbox_publish_only")
        .execute(transaction.connection())
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    original
}

struct RemovedDeathCommittedOutboxRow {
    event_id: Vec<u8>,
    event_type: String,
    echo_id: Option<Vec<u8>>,
    echo_transition_ordinal: Option<i16>,
    trigger_death_id: Option<Vec<u8>>,
    event_payload: Vec<u8>,
    created_at: String,
    published_at: Option<String>,
}

async fn remove_death_committed_outbox_row(
    persistence: &PostgresPersistence,
) -> RemovedDeathCommittedOutboxRow {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query("ALTER TABLE death_outbox_events DISABLE TRIGGER death_outbox_publish_only")
        .execute(transaction.connection())
        .await
        .unwrap();
    let removed = sqlx::query(
        "DELETE FROM death_outbox_events \
         WHERE namespace_id=$1 AND death_id=$2 AND event_type='death_committed' \
         RETURNING event_id,event_type,echo_id,echo_transition_ordinal,trigger_death_id,\
           event_payload,created_at::text AS created_at,published_at::text AS published_at",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(RequestIds::primary().death_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    sqlx::query("ALTER TABLE death_outbox_events ENABLE TRIGGER death_outbox_publish_only")
        .execute(transaction.connection())
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    RemovedDeathCommittedOutboxRow {
        event_id: removed.get("event_id"),
        event_type: removed.get("event_type"),
        echo_id: removed.get("echo_id"),
        echo_transition_ordinal: removed.get("echo_transition_ordinal"),
        trigger_death_id: removed.get("trigger_death_id"),
        event_payload: removed.get("event_payload"),
        created_at: removed.get("created_at"),
        published_at: removed.get("published_at"),
    }
}

async fn restore_death_committed_outbox_row(
    persistence: &PostgresPersistence,
    row: &RemovedDeathCommittedOutboxRow,
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query("ALTER TABLE death_outbox_events DISABLE TRIGGER death_outbox_insert_window")
        .execute(transaction.connection())
        .await
        .unwrap();
    let inserted = sqlx::query(
        "INSERT INTO death_outbox_events (namespace_id,death_id,event_id,event_type,echo_id,\
           echo_transition_ordinal,trigger_death_id,event_payload,created_at,published_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9::timestamptz,$10::timestamptz)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(RequestIds::primary().death_id.as_slice())
    .bind(&row.event_id)
    .bind(&row.event_type)
    .bind(&row.echo_id)
    .bind(row.echo_transition_ordinal)
    .bind(&row.trigger_death_id)
    .bind(&row.event_payload)
    .bind(&row.created_at)
    .bind(&row.published_at)
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(inserted, 1);
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(transaction.connection())
        .await
        .unwrap();
    sqlx::query("ALTER TABLE death_outbox_events ENABLE TRIGGER death_outbox_insert_window")
        .execute(transaction.connection())
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn assert_death_committed_outbox_cardinality(
    persistence: &PostgresPersistence,
    baseline: &persistence::StoredCoreDeathTerminalSignatureV1,
) {
    let mut duplicate = persistence.begin_transaction().await.unwrap();
    sqlx::query("ALTER TABLE death_outbox_events DISABLE TRIGGER death_outbox_insert_window")
        .execute(duplicate.connection())
        .await
        .unwrap();
    let duplicate_error = sqlx::query(
        "INSERT INTO death_outbox_events (namespace_id,death_id,event_id,event_type,echo_id,\
           echo_transition_ordinal,trigger_death_id,event_payload,created_at,published_at) \
         SELECT namespace_id,death_id,$1,event_type,echo_id,echo_transition_ordinal,\
           trigger_death_id,event_payload,created_at,published_at \
         FROM death_outbox_events \
         WHERE namespace_id=$2 AND death_id=$3 AND event_type='death_committed'",
    )
    .bind(uuid_v7(90).as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(RequestIds::primary().death_id.as_slice())
    .execute(duplicate.connection())
    .await
    .expect_err("duplicate death_committed outbox row was accepted");
    match duplicate_error {
        sqlx::Error::Database(database) => {
            assert_eq!(database.code().as_deref(), Some("23505"));
            assert_eq!(
                database.constraint(),
                Some("one_death_committed_outbox_event")
            );
        }
        other => panic!("duplicate death outbox failed unexpectedly: {other:?}"),
    }
    duplicate.rollback().await.unwrap();
    assert_eq!(canonical_terminal_signature(persistence).await, *baseline);

    let removed = remove_death_committed_outbox_row(persistence).await;
    assert!(matches!(
        persistence
            .load_core_death_terminal_signature_v1(ACCOUNT_ID, CHARACTER_ID)
            .await,
        Err(PersistenceError::CorruptStoredDeathTerminalSignature)
    ));
    restore_death_committed_outbox_row(persistence, &removed).await;
    assert_eq!(canonical_terminal_signature(persistence).await, *baseline);
}

async fn assert_altered_signature_rows_fail_closed(
    persistence: &PostgresPersistence,
    baseline: &persistence::StoredCoreDeathTerminalSignatureV1,
) {
    assert_character_life_outbox_publish_contract(persistence).await;
    assert_eq!(canonical_terminal_signature(persistence).await, *baseline);

    for corruption in SIGNATURE_ROW_CORRUPTIONS {
        apply_signature_row_fixture_update(persistence, corruption, corruption.corrupt_sql).await;
        assert!(
            matches!(
                persistence
                    .load_core_death_terminal_signature_v1(ACCOUNT_ID, CHARACTER_ID)
                    .await,
                Err(PersistenceError::CorruptStoredDeathTerminalSignature)
            ),
            "{} corruption was accepted",
            corruption.label
        );
        apply_signature_row_fixture_update(persistence, corruption, corruption.restore_sql).await;
        assert_eq!(canonical_terminal_signature(persistence).await, *baseline);
    }

    let cleanup_payload = rewrite_bargain_cleanup_payload(persistence, &[0xff]).await;
    assert!(matches!(
        persistence
            .load_core_death_terminal_signature_v1(ACCOUNT_ID, CHARACTER_ID)
            .await,
        Err(PersistenceError::CorruptStoredDeathTerminalSignature)
    ));
    rewrite_bargain_cleanup_payload(persistence, &cleanup_payload).await;
    assert_eq!(canonical_terminal_signature(persistence).await, *baseline);

    let outbox_payload = rewrite_death_committed_outbox_payload(persistence, &[0xff]).await;
    assert!(matches!(
        persistence
            .load_core_death_terminal_signature_v1(ACCOUNT_ID, CHARACTER_ID)
            .await,
        Err(PersistenceError::CorruptStoredDeathTerminalSignature)
    ));
    rewrite_death_committed_outbox_payload(persistence, &outbox_payload).await;
    assert_eq!(canonical_terminal_signature(persistence).await, *baseline);
}

#[derive(Clone, Copy)]
enum FaultTriggerTiming {
    Before {
        operation: &'static str,
        predicate: Option<&'static str>,
    },
    DeferredAfterInsert,
}

#[derive(Clone, Copy)]
struct DurableDeathFaultBoundary {
    label: &'static str,
    table: &'static str,
    timing: FaultTriggerTiming,
}

const DURABLE_DEATH_FAULT_BOUNDARIES: [DurableDeathFaultBoundary; 9] = [
    DurableDeathFaultBoundary {
        label: "bargain cleanup",
        table: "character_oath_bargain_state",
        timing: FaultTriggerTiming::Before {
            operation: "UPDATE",
            predicate: None,
        },
    },
    DurableDeathFaultBoundary {
        label: "item ledger",
        table: "item_ledger_events",
        timing: FaultTriggerTiming::Before {
            operation: "INSERT",
            predicate: Some("NEW.terminal_death_id IS NOT NULL"),
        },
    },
    DurableDeathFaultBoundary {
        label: "successor preset",
        table: "death_successor_presets_v1",
        timing: FaultTriggerTiming::Before {
            operation: "INSERT",
            predicate: None,
        },
    },
    DurableDeathFaultBoundary {
        label: "successor reservation",
        table: "successor_roster_reservations_v1",
        timing: FaultTriggerTiming::Before {
            operation: "INSERT",
            predicate: None,
        },
    },
    DurableDeathFaultBoundary {
        label: "memorial snapshot",
        table: "memorial_records",
        timing: FaultTriggerTiming::Before {
            operation: "INSERT",
            predicate: None,
        },
    },
    DurableDeathFaultBoundary {
        label: "Echo Dormant transition",
        table: "echo_state_transitions",
        timing: FaultTriggerTiming::Before {
            operation: "INSERT",
            predicate: Some("NEW.transition_ordinal = 0"),
        },
    },
    DurableDeathFaultBoundary {
        label: "Echo Available promotion",
        table: "echo_state_transitions",
        timing: FaultTriggerTiming::Before {
            operation: "INSERT",
            predicate: Some("NEW.transition_ordinal = 1"),
        },
    },
    DurableDeathFaultBoundary {
        label: "terminal receipt",
        table: "death_mutation_results",
        timing: FaultTriggerTiming::Before {
            operation: "INSERT",
            predicate: None,
        },
    },
    DurableDeathFaultBoundary {
        label: "deferred outbox commit",
        table: "death_outbox_events",
        timing: FaultTriggerTiming::DeferredAfterInsert,
    },
];

async fn install_durable_death_fault(
    persistence: &PostgresPersistence,
    boundary: DurableDeathFaultBoundary,
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let drop_trigger = format!(
        "DROP TRIGGER IF EXISTS fixture_reject_durable_death_boundary ON {}",
        boundary.table
    );
    // Identifiers and operations come only from the closed constant matrix above; no external or
    // runtime-authored value enters this test-only DDL.
    sqlx::query(sqlx::AssertSqlSafe(drop_trigger))
        .execute(transaction.connection())
        .await
        .unwrap();
    sqlx::query(
        "CREATE OR REPLACE FUNCTION fixture_reject_durable_death_boundary_v1() \
         RETURNS TRIGGER LANGUAGE plpgsql AS $$ BEGIN \
         RAISE EXCEPTION 'injected durable-death boundary rejection'; END $$",
    )
    .execute(transaction.connection())
    .await
    .unwrap();
    let create_trigger = match boundary.timing {
        FaultTriggerTiming::Before {
            operation,
            predicate,
        } => {
            let predicate = predicate.map_or_else(String::new, |value| format!(" WHEN ({value})"));
            format!(
                "CREATE TRIGGER fixture_reject_durable_death_boundary BEFORE {operation} \
                 ON {} FOR EACH ROW{predicate} \
                 EXECUTE FUNCTION fixture_reject_durable_death_boundary_v1()",
                boundary.table
            )
        }
        FaultTriggerTiming::DeferredAfterInsert => format!(
            "CREATE CONSTRAINT TRIGGER fixture_reject_durable_death_boundary AFTER INSERT \
             ON {} DEFERRABLE INITIALLY DEFERRED FOR EACH ROW \
             EXECUTE FUNCTION fixture_reject_durable_death_boundary_v1()",
            boundary.table
        ),
    };
    sqlx::query(sqlx::AssertSqlSafe(create_trigger))
        .execute(transaction.connection())
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn remove_durable_death_fault(
    persistence: &PostgresPersistence,
    boundary: DurableDeathFaultBoundary,
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let drop_trigger = format!(
        "DROP TRIGGER IF EXISTS fixture_reject_durable_death_boundary ON {}",
        boundary.table
    );
    sqlx::query(sqlx::AssertSqlSafe(drop_trigger))
        .execute(transaction.connection())
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION IF EXISTS fixture_reject_durable_death_boundary_v1()")
        .execute(transaction.connection())
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn install_serialization_retry_fault(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query("DROP TRIGGER IF EXISTS fixture_retry_durable_death_serialization ON death_events")
        .execute(transaction.connection())
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION IF EXISTS fixture_retry_durable_death_serialization_v1()")
        .execute(transaction.connection())
        .await
        .unwrap();
    sqlx::query("DROP SEQUENCE IF EXISTS fixture_durable_death_serialization_attempt_v1")
        .execute(transaction.connection())
        .await
        .unwrap();
    sqlx::query("CREATE SEQUENCE fixture_durable_death_serialization_attempt_v1 START WITH 1")
        .execute(transaction.connection())
        .await
        .unwrap();
    sqlx::query(
        "CREATE FUNCTION fixture_retry_durable_death_serialization_v1() \
         RETURNS TRIGGER LANGUAGE plpgsql AS $$ DECLARE attempt BIGINT; BEGIN \
         attempt := nextval('fixture_durable_death_serialization_attempt_v1'); \
         IF attempt = 1 THEN RAISE EXCEPTION 'injected serialization victim' \
             USING ERRCODE = '40001'; END IF; RETURN NEW; END $$",
    )
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER fixture_retry_durable_death_serialization BEFORE INSERT ON death_events \
         FOR EACH ROW EXECUTE FUNCTION fixture_retry_durable_death_serialization_v1()",
    )
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn remove_serialization_retry_fault(persistence: &PostgresPersistence) -> i64 {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let attempts: i64 =
        sqlx::query_scalar("SELECT last_value FROM fixture_durable_death_serialization_attempt_v1")
            .fetch_one(transaction.connection())
            .await
            .unwrap();
    sqlx::query("DROP TRIGGER IF EXISTS fixture_retry_durable_death_serialization ON death_events")
        .execute(transaction.connection())
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION IF EXISTS fixture_retry_durable_death_serialization_v1()")
        .execute(transaction.connection())
        .await
        .unwrap();
    sqlx::query("DROP SEQUENCE IF EXISTS fixture_durable_death_serialization_attempt_v1")
        .execute(transaction.connection())
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    attempts
}

#[derive(Clone, Copy)]
struct ImmutableDeathHistoryRow {
    label: &'static str,
    update_sql: &'static str,
    delete_sql: &'static str,
}

const IMMUTABLE_DEATH_HISTORY_ROWS: [ImmutableDeathHistoryRow; 17] = [
    ImmutableDeathHistoryRow {
        label: "death root",
        update_sql: "UPDATE death_events SET lifetime_ticks=lifetime_ticks \
            WHERE namespace_id=$1 AND death_id=$2",
        delete_sql: "DELETE FROM death_events WHERE namespace_id=$1 AND death_id=$2",
    },
    ImmutableDeathHistoryRow {
        label: "combat trace entry",
        update_sql: "UPDATE death_combat_trace_entries SET attack_id=attack_id \
            WHERE namespace_id=$1 AND death_id=$2 AND trace_ordinal=0",
        delete_sql: "DELETE FROM death_combat_trace_entries \
            WHERE namespace_id=$1 AND death_id=$2 AND trace_ordinal=0",
    },
    ImmutableDeathHistoryRow {
        label: "combat trace status",
        update_sql: "UPDATE death_combat_trace_statuses SET remaining_ticks=remaining_ticks \
            WHERE namespace_id=$1 AND death_id=$2 AND trace_ordinal=0 AND status_ordinal=0",
        delete_sql: "DELETE FROM death_combat_trace_statuses \
            WHERE namespace_id=$1 AND death_id=$2 AND trace_ordinal=0 AND status_ordinal=0",
    },
    ImmutableDeathHistoryRow {
        label: "death summary",
        update_sql: "UPDATE death_summary_snapshots SET lifetime_ms=lifetime_ms \
            WHERE namespace_id=$1 AND death_id=$2",
        delete_sql: "DELETE FROM death_summary_snapshots \
            WHERE namespace_id=$1 AND death_id=$2",
    },
    ImmutableDeathHistoryRow {
        label: "summary damage reference",
        update_sql: "UPDATE death_summary_damage_entries SET trace_ordinal=trace_ordinal \
            WHERE namespace_id=$1 AND death_id=$2 AND damage_ordinal=0",
        delete_sql: "DELETE FROM death_summary_damage_entries \
            WHERE namespace_id=$1 AND death_id=$2 AND damage_ordinal=0",
    },
    ImmutableDeathHistoryRow {
        label: "summary projection",
        update_sql: "UPDATE death_summary_projection_entries SET quantity=quantity \
            WHERE namespace_id=$1 AND death_id=$2 AND section_kind=0 AND entry_ordinal=0",
        delete_sql: "DELETE FROM death_summary_projection_entries \
            WHERE namespace_id=$1 AND death_id=$2 AND section_kind=0 AND entry_ordinal=0",
    },
    ImmutableDeathHistoryRow {
        label: "Memorial",
        update_sql: "UPDATE memorial_records SET presentation_key=presentation_key \
            WHERE namespace_id=$1 AND death_id=$2",
        delete_sql: "DELETE FROM memorial_records WHERE namespace_id=$1 AND death_id=$2",
    },
    ImmutableDeathHistoryRow {
        label: "destruction ledger",
        update_sql: "UPDATE death_destruction_entries SET quantity=quantity \
            WHERE namespace_id=$1 AND death_id=$2 AND destruction_ordinal=0",
        delete_sql: "DELETE FROM death_destruction_entries \
            WHERE namespace_id=$1 AND death_id=$2 AND destruction_ordinal=0",
    },
    ImmutableDeathHistoryRow {
        label: "terminal receipt",
        update_sql: "UPDATE death_mutation_results SET result_code=result_code \
            WHERE namespace_id=$1 AND death_id=$2",
        delete_sql: "DELETE FROM death_mutation_results \
            WHERE namespace_id=$1 AND death_id=$2",
    },
    ImmutableDeathHistoryRow {
        label: "death audit",
        update_sql: "UPDATE death_audit_events SET event_kind=event_kind \
            WHERE namespace_id=$1 AND death_id=$2",
        delete_sql: "DELETE FROM death_audit_events WHERE namespace_id=$1 AND death_id=$2",
    },
    ImmutableDeathHistoryRow {
        label: "death outbox payload",
        update_sql: "UPDATE death_outbox_events SET event_payload=event_payload \
            WHERE namespace_id=$1 AND death_id=$2 AND event_type='death_committed'",
        delete_sql: "DELETE FROM death_outbox_events \
            WHERE namespace_id=$1 AND death_id=$2 AND event_type='death_committed'",
    },
    ImmutableDeathHistoryRow {
        label: "Echo snapshot",
        update_sql: "UPDATE echo_records SET snapshot_digest=snapshot_digest \
            WHERE namespace_id=$1 AND death_id=$2",
        delete_sql: "DELETE FROM echo_records WHERE namespace_id=$1 AND death_id=$2",
    },
    ImmutableDeathHistoryRow {
        label: "Echo deed",
        update_sql: "UPDATE echo_deed_tags SET deed_tag=deed_tag \
            WHERE namespace_id=$1 AND echo_id=(SELECT echo_id FROM echo_records \
                WHERE namespace_id=$1 AND death_id=$2) AND deed_ordinal=0",
        delete_sql: "DELETE FROM echo_deed_tags \
            WHERE namespace_id=$1 AND echo_id=(SELECT echo_id FROM echo_records \
                WHERE namespace_id=$1 AND death_id=$2) AND deed_ordinal=0",
    },
    ImmutableDeathHistoryRow {
        label: "Echo transition",
        update_sql: "UPDATE echo_state_transitions SET reason_kind=reason_kind \
            WHERE namespace_id=$1 AND echo_id=(SELECT echo_id FROM echo_records \
                WHERE namespace_id=$1 AND death_id=$2) AND transition_ordinal=0",
        delete_sql: "DELETE FROM echo_state_transitions \
            WHERE namespace_id=$1 AND echo_id=(SELECT echo_id FROM echo_records \
                WHERE namespace_id=$1 AND death_id=$2) AND transition_ordinal=0",
    },
    ImmutableDeathHistoryRow {
        label: "destroyed item",
        update_sql: "UPDATE item_instances SET item_version=item_version \
            WHERE namespace_id=$1 AND terminal_death_id=$2",
        delete_sql: "DELETE FROM item_instances \
            WHERE namespace_id=$1 AND terminal_death_id=$2",
    },
    ImmutableDeathHistoryRow {
        label: "permadeath item ledger",
        update_sql: "UPDATE item_ledger_events SET event_kind=event_kind \
            WHERE namespace_id=$1 AND terminal_death_id=$2",
        delete_sql: "DELETE FROM item_ledger_events \
            WHERE namespace_id=$1 AND terminal_death_id=$2",
    },
    ImmutableDeathHistoryRow {
        label: "destroyed run material",
        update_sql: "UPDATE character_run_material_stacks SET quantity=quantity \
            WHERE namespace_id=$1 AND terminal_death_id=$2",
        delete_sql: "DELETE FROM character_run_material_stacks \
            WHERE namespace_id=$1 AND terminal_death_id=$2",
    },
];

async fn assert_immutable_death_history_rejects_direct_mutation(
    persistence: &PostgresPersistence,
    baseline: &persistence::StoredCoreDeathTerminalSignatureV1,
) {
    // GDD DTH-001/TECH-021, Content CONT-ECHO-009, and Roadmap GB-M03-02D/06/13 require
    // the committed terminal graph to remain an immutable historical authority. Each operation
    // uses a fresh transaction because PostgreSQL correctly aborts the transaction on rejection.
    for row in IMMUTABLE_DEATH_HISTORY_ROWS {
        for (operation, sql) in [("UPDATE", row.update_sql), ("DELETE", row.delete_sql)] {
            let mut transaction = persistence.begin_transaction().await.unwrap();
            let rejected = sqlx::query(sql)
                .bind(WIPEABLE_CORE_NAMESPACE)
                .bind(RequestIds::primary().death_id.as_slice())
                .execute(transaction.connection())
                .await;
            assert!(
                rejected.is_err(),
                "{} {} must reject without disabling production triggers",
                row.label,
                operation
            );
            transaction.rollback().await.unwrap();
            assert_eq!(
                canonical_terminal_signature(persistence).await,
                *baseline,
                "{} {} changed the canonical terminal signature",
                row.label,
                operation
            );
        }
    }
}

async fn assert_post_death_rejection(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let rejected = sqlx::query(
        "UPDATE character_progression SET total_xp=total_xp+1 WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await;
    assert!(rejected.is_err());
    transaction.rollback().await.unwrap();
}

async fn assert_trace_promotion_conflict_audit(
    persistence: &PostgresPersistence,
    stored: &DurableDeathTracePromotionV1,
    attempted: &DurableDeathTracePromotionV1,
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let row = sqlx::query(
        "SELECT conflict_code,stored_promotion_digest,attempted_promotion_digest, \
                stored_terminal_payload_hash,attempted_terminal_payload_hash \
         FROM death_live_trace_promotion_conflict_audits_v1 \
         WHERE namespace_id=$1 AND account_id=$2 AND death_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(RequestIds::primary().death_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(row.get::<i16, _>("conflict_code"), 0);
    assert_eq!(
        row.get::<Vec<u8>, _>("stored_promotion_digest"),
        stored.promotion_digest()
    );
    assert_eq!(
        row.get::<Vec<u8>, _>("attempted_promotion_digest"),
        attempted.promotion_digest()
    );
    assert_eq!(
        row.get::<Vec<u8>, _>("stored_terminal_payload_hash"),
        stored.terminal_payload_hash()
    );
    assert_eq!(
        row.get::<Vec<u8>, _>("attempted_terminal_payload_hash"),
        attempted.terminal_payload_hash()
    );
    transaction.rollback().await.unwrap();
    assert_eq!(
        count(persistence, "death_live_trace_promotion_conflict_audits_v1").await,
        1
    );
}

async fn assert_trace_promotion_history_is_sealed(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    assert!(
        sqlx::query(
            "UPDATE death_live_trace_sets_v1 SET promotion_digest=promotion_digest \
             WHERE namespace_id=$1 AND death_id=$2",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(RequestIds::primary().death_id.as_slice())
        .execute(transaction.connection())
        .await
        .is_err()
    );
    transaction.rollback().await.unwrap();

    let mut transaction = persistence.begin_transaction().await.unwrap();
    assert!(
        sqlx::query(
            "DELETE FROM death_live_trace_receipt_links_v1 \
             WHERE namespace_id=$1 AND death_id=$2 AND receipt_ordinal=0",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(RequestIds::primary().death_id.as_slice())
        .execute(transaction.connection())
        .await
        .is_err()
    );
    transaction.rollback().await.unwrap();

    let mut transaction = persistence.begin_transaction().await.unwrap();
    assert!(
        sqlx::query(
            "INSERT INTO death_live_trace_entry_provenance_v1 (namespace_id,death_id,\
                trace_ordinal,receipt_ordinal,trace_tick_id,event_tick,event_ordinal,cause_kind,\
                source_entity_id,source_sim_entity_id,status_count,live_entry_digest) \
             SELECT namespace_id,death_id,3000,receipt_ordinal,trace_tick_id,event_tick,\
                event_ordinal+1000,cause_kind,source_entity_id,source_sim_entity_id,status_count,\
                live_entry_digest FROM death_live_trace_entry_provenance_v1 \
             WHERE namespace_id=$1 AND death_id=$2 AND trace_ordinal=0",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(RequestIds::primary().death_id.as_slice())
        .execute(transaction.connection())
        .await
        .is_err()
    );
    transaction.rollback().await.unwrap();
}

async fn assert_cross_account_promotion_guards(persistence: &PostgresPersistence) {
    for (constraint_name, constraint_sql, include_outbox) in [
        (
            "echo_promotion_trigger_account_exact",
            "SET CONSTRAINTS echo_promotion_trigger_account_exact IMMEDIATE",
            false,
        ),
        (
            "echo_promotion_outbox_trigger_exact",
            "SET CONSTRAINTS echo_promotion_outbox_trigger_exact IMMEDIATE",
            true,
        ),
    ] {
        let mut transaction = persistence.begin_transaction().await.unwrap();
        sqlx::query(
            "CREATE TEMP TABLE fixture_foreign_death ON COMMIT DROP AS \
             SELECT * FROM death_events WHERE namespace_id=$1 AND death_id=$2",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(RequestIds::primary().death_id.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
        let foreign_death_id = uuid_v7(91);
        sqlx::query("UPDATE fixture_foreign_death SET death_id=$1,account_id=$2,mutation_id=$3")
            .bind(foreign_death_id.as_slice())
            .bind([92_u8; 16].as_slice())
            .bind([93_u8; 16].as_slice())
            .execute(transaction.connection())
            .await
            .unwrap();
        // The adversarial fixture bypasses the older death graph only to materialize a durable
        // foreign-account trigger candidate. The 0042 guards themselves remain enabled.
        sqlx::query("ALTER TABLE death_events DISABLE TRIGGER ALL")
            .execute(transaction.connection())
            .await
            .unwrap();
        sqlx::query("INSERT INTO death_events SELECT * FROM fixture_foreign_death")
            .execute(transaction.connection())
            .await
            .unwrap();
        sqlx::query("ALTER TABLE death_events ENABLE TRIGGER ALL")
            .execute(transaction.connection())
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO echo_state_transitions \
             (namespace_id,echo_id,transition_ordinal,previous_state,next_state,reason_kind, \
              source_death_id,trigger_death_id) VALUES ($1,$2,2,0,1,1,NULL,$3)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(RequestIds::primary().echo_id.as_slice())
        .bind(foreign_death_id.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
        if include_outbox {
            sqlx::query(
                "INSERT INTO death_outbox_events \
                 (namespace_id,death_id,event_id,event_type,echo_id,event_payload, \
                  echo_transition_ordinal,trigger_death_id) \
                 VALUES ($1,$2,$3,'echo_promoted',$4,$5,2,$6)",
            )
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(RequestIds::primary().death_id.as_slice())
            .bind([94_u8; 16].as_slice())
            .bind(RequestIds::primary().echo_id.as_slice())
            .bind([1_u8].as_slice())
            .bind(foreign_death_id.as_slice())
            .execute(transaction.connection())
            .await
            .unwrap();
        }
        let rejected = sqlx::query(constraint_sql)
            .execute(transaction.connection())
            .await;
        assert!(
            rejected.is_err(),
            "{constraint_name} accepted cross-account promotion authority"
        );
        transaction.rollback().await.unwrap();
    }
}

async fn wipe_disposable_database(persistence: &PostgresPersistence) {
    persistence.reset_disposable_identity_data().await.unwrap();
    for table in [
        "accounts",
        "characters",
        "item_instances",
        "character_run_material_stacks",
        "death_events",
        "death_summary_snapshots",
        "memorial_records",
        "death_destruction_entries",
        "death_mutation_results",
        "echo_records",
        "echo_state_transitions",
        "death_outbox_events",
        "onboarding_outbox_events_v1",
        "item_ledger_telemetry_outbox_v1",
    ] {
        assert_eq!(count(persistence, table).await, 0, "{table}");
    }
}

async fn assert_committed_death_views(persistence: &PostgresPersistence, ids: RequestIds) {
    let latest = persistence
        .load_latest_committed_death_view(ACCOUNT_ID)
        .await
        .unwrap()
        .expect("committed account death");
    assert_eq!(latest.death_id, ids.death_id);
    assert_eq!(latest.character_id, CHARACTER_ID);
    assert_eq!(latest.content_revision, CORE_ITEM_CONTENT_REVISION);
    assert_eq!(
        latest.presentation.records_blake3,
        CORE_DEATH_VIEW_RECORDS_BLAKE3
    );
    assert_eq!(
        latest.presentation.assets_blake3,
        CORE_DEATH_VIEW_ASSETS_BLAKE3
    );
    assert_eq!(
        latest.presentation.localization_blake3,
        CORE_DEATH_VIEW_LOCALIZATION_BLAKE3
    );
    assert_stored_death_authorities(persistence, ids.death_id).await;

    let summary = persistence
        .load_owned_death_summary_view(ACCOUNT_ID, ids.death_id, 0, 32)
        .await
        .unwrap();
    assert_eq!(summary.death_id, ids.death_id);
    assert_eq!(summary.last_five_damage.len(), 2);
    assert_eq!(summary.lost.len(), usize::from(summary.lost_total_count));
    assert!(summary.next_lost_ordinal.is_none());
    assert_eq!(
        summary.presentation,
        DurableDeathPresentationAuthorityV1::core()
    );

    let memorials = persistence
        .load_death_memorial_page(ACCOUNT_ID, None, 32)
        .await
        .unwrap();
    assert_eq!(memorials.entries.len(), 1);
    assert_eq!(memorials.entries[0].cursor.death_id, ids.death_id);
    assert_eq!(
        memorials.entries[0].presentation,
        DurableDeathPresentationAuthorityV1::core()
    );
    assert!(memorials.next_cursor.is_none());

    let trace = persistence
        .load_owned_death_trace_page(ACCOUNT_ID, ids.death_id, 0, 8)
        .await
        .unwrap();
    assert_eq!(
        trace.presentation,
        DurableDeathPresentationAuthorityV1::core()
    );
    assert_eq!(trace.entries.len(), 2);
    assert!(trace.entries.last().is_some_and(|entry| entry.lethal));
    assert!(trace.next_ordinal.is_none());

    assert_eq!(
        persistence
            .load_owned_death_summary_view([229; 16], ids.death_id, 0, 1)
            .await,
        Err(DeathViewReadError::DeathNotOwned)
    );
    assert_eq!(
        persistence
            .load_owned_death_trace_page(ACCOUNT_ID, ids.death_id, trace.total_entry_count, 1)
            .await,
        Err(DeathViewReadError::PageOutOfRange)
    );
}

async fn assert_stored_death_authorities(persistence: &PostgresPersistence, death_id: [u8; 16]) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let authority = sqlx::query(
        "SELECT world_records_blake3,world_assets_blake3,world_localization_blake3,\
                presentation_records_blake3,presentation_assets_blake3,\
                presentation_localization_blake3 \
         FROM death_events WHERE namespace_id=$1 AND death_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(death_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    for (column, expected) in [
        ("world_records_blake3", RECORDS_BLAKE3),
        ("world_assets_blake3", ASSETS_BLAKE3),
        ("world_localization_blake3", LOCALIZATION_BLAKE3),
        (
            "presentation_records_blake3",
            CORE_DEATH_VIEW_RECORDS_BLAKE3,
        ),
        ("presentation_assets_blake3", CORE_DEATH_VIEW_ASSETS_BLAKE3),
        (
            "presentation_localization_blake3",
            CORE_DEATH_VIEW_LOCALIZATION_BLAKE3,
        ),
    ] {
        assert_eq!(authority.get::<String, _>(column), expected, "{column}");
    }
    transaction.rollback().await.unwrap();
}

async fn assert_stored_tel_003_context_and_redacted_projection(
    persistence: &PostgresPersistence,
    death_id: [u8; 16],
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let row = sqlx::query(
        "SELECT party_size,boss_phase_id,contribution_centi_units,\
                contribution_reference_health,network_source_kind,\
                network_transport_generation,network_sampled_at_unix_ms,\
                network_ping_millis,network_jitter_millis,\
                network_loss_basis_points,network_correction_count \
         FROM death_events WHERE namespace_id=$1 AND death_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(death_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(row.get::<i16, _>("party_size"), 1);
    assert_eq!(row.get::<String, _>("boss_phase_id"), "boss.caldus.phase_2");
    assert_eq!(row.get::<i64, _>("contribution_centi_units"), 360_000);
    assert_eq!(row.get::<i64, _>("contribution_reference_health"), 7_200);
    assert_eq!(row.get::<i16, _>("network_source_kind"), 1);
    assert_eq!(row.get::<i64, _>("network_transport_generation"), 1);
    assert_eq!(
        row.get::<i64, _>("network_sampled_at_unix_ms"),
        i64::try_from(ISSUED_AT_UNIX_MS).unwrap()
    );
    assert_eq!(row.get::<i32, _>("network_ping_millis"), 80);
    assert_eq!(row.get::<i32, _>("network_jitter_millis"), 12);
    assert_eq!(row.get::<i32, _>("network_loss_basis_points"), 100);
    assert_eq!(row.get::<Option<i64>, _>("network_correction_count"), None);
    transaction.rollback().await.unwrap();

    let signature = canonical_terminal_signature(persistence).await;
    assert!(matches!(
        signature.plan.event.telemetry,
        DurableDeathTelemetryContextV1::Observed {
            schema_version: DURABLE_DEATH_TELEMETRY_CONTEXT_SCHEMA_VERSION,
            party_size: 1,
            boss_phase_id: Some(ref phase),
            contribution: Some(ref contribution),
            network_health: DurableDeathNetworkHealthV1 {
                transport_generation: 1,
                ping_millis: 80,
                jitter_millis: 12,
                loss_basis_points: 100,
                correction_count: None,
                ..
            },
        } if phase == "boss.caldus.phase_2"
            && contribution.contribution_centi_units == 360_000
            && contribution.reference_health == 7_200
    ));

    let mut adapter = PostgresM03TelemetryOutboxAdapter::from_persistence(
        persistence.clone(),
        TelemetryPseudonymizationKeyV1::new([0x78; 32]).unwrap(),
    );
    let records = adapter.poll_unpublished(16).await.unwrap();
    let death = records
        .into_iter()
        .find(|record| record.envelope().event_name() == "character_died")
        .expect("committed death telemetry projection");
    let mut pipeline = TelemetryPipeline::new(
        TelemetryPipelineMode::Enabled,
        TelemetryConnectivity::Online,
        16,
    )
    .unwrap();
    assert_eq!(
        pipeline.ingest_committed(death),
        TelemetryIngestOutcome::Queued
    );
    let documents = pipeline.prepare_redacted_batch(16).unwrap();
    let json: serde_json::Value = serde_json::from_str(&documents[0].json).unwrap();
    assert_eq!(json["event_schema_version"], 2);
    let payload = &json["event"]["payload"];
    assert_eq!(payload["boss_phase_id"], "boss.caldus.phase_2");
    assert_eq!(payload["party_size"], 1);
    assert_eq!(payload["contribution_basis_points"], 5_000);
    assert_eq!(payload["network_health"]["ping_millis"], 80);
    assert_eq!(payload["network_health"]["jitter_millis"], 12);
    assert_eq!(payload["network_health"]["loss_basis_points"], 100);
    assert!(payload["network_health"]["correction_count"].is_null());
}

/// Hosted restart projection required by GDD TECH-015/021/023, Content CONT-BOSS-005 and
/// CONT-HUB-002, and Roadmap GB-M03-06/08: the existing death graph is the only terminal writer.
async fn assert_committed_terminal_recovery(
    persistence: &PostgresPersistence,
    expected: &persistence::StoredCommittedDeathResultV1,
    promotion: &DurableDeathTracePromotionV1,
) {
    let terminal = persistence
        .load_committed_death_terminal_v1(ACCOUNT_ID, CHARACTER_ID)
        .await
        .unwrap()
        .expect("committed terminal recovery projection");
    terminal.validate().unwrap();
    assert_eq!(&terminal.result, expected);
    assert_eq!(terminal.result_hash, expected.digest().unwrap());
    assert_eq!(terminal.lineage_id, LINEAGE_ID);
    assert_eq!(terminal.restore_point_id, RESTORE_POINT_ID);
    assert_eq!(terminal.death_tick, 20_000);
    assert_eq!(terminal.promotion_digest, promotion.promotion_digest());
    assert_eq!(
        terminal.terminal_payload_hash,
        promotion.terminal_payload_hash()
    );
    let bootstrap = persistence
        .load_private_life_bootstrap_v1(ACCOUNT_ID)
        .await
        .unwrap();
    assert!(matches!(
        bootstrap.state,
        StoredPrivateLifeBootstrapStateV1::DeathCommitted(ref stored)
            if stored.result == *expected
    ));
    let process_restart = persistence
        .resolve_private_life_process_restart_v1(ACCOUNT_ID)
        .await
        .unwrap();
    assert!(process_restart.crash_restore.is_none());
    assert!(matches!(
        process_restart.bootstrap.state,
        StoredPrivateLifeBootstrapStateV1::DeathCommitted(_)
    ));
    assert_eq!(
        persistence
            .load_committed_death_terminal_v1([229; 16], CHARACTER_ID)
            .await
            .unwrap(),
        None
    );
    assert!(matches!(
        persistence
            .load_committed_death_terminal_v1([0; 16], CHARACTER_ID)
            .await,
        Err(PersistenceError::DurableDeathBindingMismatch)
    ));
}

async fn canonical_terminal_signature(
    persistence: &PostgresPersistence,
) -> persistence::StoredCoreDeathTerminalSignatureV1 {
    let signature = persistence
        .load_core_death_terminal_signature_v1(ACCOUNT_ID, CHARACTER_ID)
        .await
        .unwrap()
        .expect("committed canonical terminal signature");
    signature.canonical_bytes().unwrap();
    assert_ne!(signature.digest().unwrap(), [0; 32]);
    signature
}

async fn emulate_schema_78_columns_added_to_a_legacy_death(
    persistence: &PostgresPersistence,
    death_id: [u8; 16],
) {
    // ADD COLUMN leaves a schema-77 row NULL without invoking immutable-history triggers. The
    // disposable fixture reaches that exact post-migration shape by temporarily disabling only
    // user triggers in one transaction; rollback would restore both data and trigger state.
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query("ALTER TABLE death_events DISABLE TRIGGER USER")
        .execute(transaction.connection())
        .await
        .unwrap();
    assert_eq!(
        sqlx::query(
            "UPDATE death_events SET \
                party_size=NULL,boss_phase_id=NULL,contribution_centi_units=NULL,\
                contribution_reference_health=NULL,network_source_kind=NULL,\
                network_transport_generation=NULL,network_sampled_at_unix_ms=NULL,\
                network_ping_millis=NULL,network_jitter_millis=NULL,\
                network_loss_basis_points=NULL,network_correction_count=NULL \
             WHERE namespace_id=$1 AND death_id=$2",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(death_id.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap()
        .rows_affected(),
        1
    );
    sqlx::query("ALTER TABLE death_events ENABLE TRIGGER USER")
        .execute(transaction.connection())
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

#[test]
fn hosted_fixture_request_and_content_authority_are_canonical() {
    let content = content_authority();
    content.validate().unwrap();
    let request = request(RequestIds::primary());
    request.validate().unwrap();
    assert!(content.matches_event(&request.plan.event));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn schema_78_upgrade_preserves_legacy_death_signature_and_exact_replay() {
    let persistence = disposable_database().await;
    let content = content_authority();
    reset_fixture(&persistence).await;
    terminal_telemetry::start_skewed_session(&persistence, ACCOUNT_ID, TELEMETRY_SESSION_ID).await;
    let death = request(RequestIds::primary());
    let evidence = seed_hosted_death_trace(&persistence, &death).await;
    let promotion = evidence.promotion_for(&death);
    let fresh = persistence
        .transact_durable_death(&death, &content, &promotion)
        .await
        .unwrap();
    assert!(matches!(
        canonical_terminal_signature(&persistence)
            .await
            .plan
            .event
            .telemetry,
        DurableDeathTelemetryContextV1::Observed { .. }
    ));

    emulate_schema_78_columns_added_to_a_legacy_death(&persistence, death.plan.event.death_id)
        .await;
    persistence.close().await;

    let restarted = reconnect_database().await;
    let upgraded = canonical_terminal_signature(&restarted).await;
    assert_eq!(
        upgraded.plan.event.telemetry,
        DurableDeathTelemetryContextV1::HistoricalUnavailable
    );
    assert_eq!(
        upgraded.plan.canonical_plan_hash().unwrap(),
        death.canonical_plan_hash
    );
    let replay = restarted
        .transact_durable_death(&death, &content, &promotion)
        .await
        .unwrap();
    assert!(replay.is_replay());
    assert_eq!(replay.result(), fresh.result());

    let mut adapter = PostgresM03TelemetryOutboxAdapter::from_persistence(
        restarted.clone(),
        TelemetryPseudonymizationKeyV1::new([0x78; 32]).unwrap(),
    );
    let historical = adapter
        .poll_unpublished(16)
        .await
        .unwrap()
        .into_iter()
        .find(|record| record.envelope().event_name() == "character_died")
        .expect("legacy death remains projectable");
    let mut pipeline = TelemetryPipeline::new(
        TelemetryPipelineMode::Enabled,
        TelemetryConnectivity::Online,
        16,
    )
    .unwrap();
    assert_eq!(
        pipeline.ingest_committed(historical),
        TelemetryIngestOutcome::Queued
    );
    let json: serde_json::Value =
        serde_json::from_str(&pipeline.prepare_redacted_batch(16).unwrap()[0].json).unwrap();
    assert!(json["event"]["payload"]["party_size"].is_null());
    assert!(json["event"]["payload"]["network_health"].is_null());

    reset_fixture(&restarted).await;
    restarted.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn safe_equipped_item_in_danger_fails_closed_without_terminal_writes() {
    let persistence = disposable_database().await;
    let content = content_authority();
    reset_fixture(&persistence).await;
    let death = request(RequestIds::primary());
    let evidence = seed_hosted_death_trace(&persistence, &death).await;
    let promotion = evidence.promotion_for(&death);

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let changed = sqlx::query(
        "UPDATE item_instances SET security_state=0 WHERE namespace_id=$1 AND account_id=$2 \
         AND character_id=$3 AND item_uid=$4 AND location_kind=0 AND security_state=1",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(ITEM_UID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(changed, 1);
    transaction.commit().await.unwrap();

    assert!(matches!(
        persistence
            .transact_durable_death(&death, &content, &promotion)
            .await,
        Err(PersistenceError::CorruptStoredDurableDeath)
    ));
    assert_safe_storage_preserved(&persistence).await;

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let restored = sqlx::query(
        "UPDATE item_instances SET security_state=1 WHERE namespace_id=$1 AND account_id=$2 \
         AND character_id=$3 AND item_uid=$4 AND location_kind=0 AND security_state=0",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(ITEM_UID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(restored, 1);
    transaction.commit().await.unwrap();
    assert_rollback_pristine(&persistence).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn black_unique_equipment_uses_the_repaired_atomic_echo_power_band() {
    // Authorities: GDD ECH-002/TECH-022, Content Spec CONT-ECHO-001, and roadmap
    // GB-M03-06/13 require the locked pre-destruction item state and deferred SQL graph to agree.
    let persistence = disposable_database().await;
    let content = content_authority();
    reset_fixture(&persistence).await;

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let changed = sqlx::query(
        "UPDATE item_instances SET item_level=20,rarity=5 WHERE namespace_id=$1 AND account_id=$2 \
         AND character_id=$3 AND item_uid=$4 AND location_kind=0 AND security_state=1",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(ITEM_UID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(changed, 1);
    transaction.commit().await.unwrap();

    let mut plan = request(RequestIds::primary()).plan;
    let echo = plan.echo.as_mut().expect("qualifying death has an Echo");
    echo.created.power_band = 2;
    echo.created.snapshot_digest = echo.created.expected_snapshot_digest().unwrap();
    let death = DurableDeathCommitRequestV1::seal(plan, ISSUED_AT_UNIX_MS).unwrap();
    let evidence = seed_hosted_death_trace(&persistence, &death).await;
    let promotion = evidence.promotion_for(&death);

    assert!(matches!(
        persistence
            .transact_durable_death(&death, &content, &promotion)
            .await
            .unwrap(),
        DurableDeathTransactionV1::Fresh(_)
    ));
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let stored_band: i16 = sqlx::query_scalar(
        "SELECT power_band FROM echo_records WHERE namespace_id=$1 AND echo_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(RequestIds::primary().echo_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(stored_band, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn qualifying_zero_custody_death_commits_complete_graph_and_replays_exactly() {
    // Authorities: Gravebound_Production_GDD_v1_Canonical.md DTH-001/TECH-021/022,
    // Gravebound_Content_Production_Spec_v1.md CONT-ECHO-009, and
    // Gravebound_Development_Roadmap_v1.md GB-M03-06/13 require the complete qualifying terminal
    // graph even when the character owns no at-risk item or run-material custody.
    let persistence = disposable_database().await;
    let content = content_authority();
    reset_zero_custody_fixture(&persistence).await;
    let death = zero_custody_request(RequestIds::primary());
    assert!(death.plan.echo.is_some());
    assert!(death.plan.destruction.is_empty());
    assert!(death.plan.summary.projections.lost.is_empty());
    assert_eq!(death.plan.event.destruction_entry_count, 0);
    assert_eq!(death.plan.event.versions.inventory.pre, 1);
    assert_eq!(death.plan.event.versions.inventory.post, 2);
    let evidence = seed_hosted_death_trace(&persistence, &death).await;
    let promotion = evidence.promotion_for(&death);

    let fresh = persistence
        .transact_durable_death(&death, &content, &promotion)
        .await
        .unwrap();
    assert!(matches!(fresh, DurableDeathTransactionV1::Fresh(_)));
    assert_eq!(fresh.result().echo_outcome, DurableEchoOutcomeV1::Available);
    assert_eq!(
        fresh.result().created_echo_id,
        Some(RequestIds::primary().echo_id)
    );
    assert_eq!(
        fresh.result().promoted_echo_id,
        Some(RequestIds::primary().echo_id)
    );
    assert_zero_custody_complete_graph(&persistence, RequestIds::primary()).await;
    assert_committed_terminal_recovery(&persistence, fresh.result(), &promotion).await;
    assert_stored_death_authorities(&persistence, RequestIds::primary().death_id).await;
    let committed_signature = canonical_terminal_signature(&persistence).await;

    let replay = persistence
        .transact_durable_death(&death, &content, &promotion)
        .await
        .unwrap();
    assert!(replay.is_replay());
    assert_eq!(replay.result(), fresh.result());
    assert_eq!(
        canonical_terminal_signature(&persistence).await,
        committed_signature
    );
    assert_zero_custody_complete_graph(&persistence, RequestIds::primary()).await;
    persistence.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn later_ordinary_death_supersedes_the_prior_successor_reservation() {
    // Authorities: Production GDD DTH-020/021 and UI-007-009, Content Spec
    // CONT-CATALOG-003, and Roadmap GB-M03-07 require the latest unconsumed ordinary death to own
    // the exact reusable roster slot without manufacturing a second active reservation.
    let persistence = disposable_database().await;
    let content = content_authority();
    reset_zero_custody_fixture(&persistence).await;
    stage_second_zero_custody_character(&persistence).await;

    let primary = zero_custody_request(RequestIds::primary());
    let primary_evidence = seed_hosted_death_trace(&persistence, &primary).await;
    let primary_promotion = primary_evidence.promotion_for(&primary);
    let primary_fresh = persistence
        .transact_durable_death(&primary, &content, &primary_promotion)
        .await
        .unwrap();
    assert!(matches!(primary_fresh, DurableDeathTransactionV1::Fresh(_)));
    assert_successor_death_authority(&persistence, &primary, 0, None).await;
    select_second_character_after_primary_death(&persistence).await;

    let secondary = second_zero_custody_request(
        RequestIds::secondary(),
        DurableDeathProvenanceV1::OrdinaryGameplay,
    );
    let secondary_evidence = seed_hosted_death_trace(&persistence, &secondary).await;
    let secondary_promotion = secondary_evidence.promotion_for(&secondary);
    let secondary_fresh = persistence
        .transact_durable_death(&secondary, &content, &secondary_promotion)
        .await
        .unwrap();
    assert!(matches!(
        secondary_fresh,
        DurableDeathTransactionV1::Fresh(_)
    ));
    assert_successor_death_authority(
        &persistence,
        &primary,
        2,
        Some(RequestIds::secondary().death_id),
    )
    .await;
    assert_successor_death_authority(&persistence, &secondary, 0, None).await;
    assert_eq!(count(&persistence, "death_successor_presets_v1").await, 2);
    assert_eq!(
        count(&persistence, "successor_roster_reservations_v1").await,
        2
    );
    let bootstrap = persistence
        .load_private_life_bootstrap_v1(ACCOUNT_ID)
        .await
        .unwrap();
    assert!(matches!(
        bootstrap.state,
        StoredPrivateLifeBootstrapStateV1::DeathCommitted(ref terminal)
            if terminal.result.death_id == RequestIds::secondary().death_id
    ));

    let replay = persistence
        .transact_durable_death(&secondary, &content, &secondary_promotion)
        .await
        .unwrap();
    assert!(replay.is_replay());
    assert_eq!(replay.result(), secondary_fresh.result());
    assert_successor_death_authority(
        &persistence,
        &primary,
        2,
        Some(RequestIds::secondary().death_id),
    )
    .await;
    assert_successor_death_authority(&persistence, &secondary, 0, None).await;
    persistence.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn incident_and_administrative_deaths_preserve_prior_successor_authority() {
    // The same three authorities and accepted SPEC-CONFLICT-031 reserve successor recovery for
    // normal player-visible deaths. Incident/admin records remain durable but may neither mint nor
    // supersede that authority.
    let persistence = disposable_database().await;
    let content = content_authority();
    for provenance in [
        DurableDeathProvenanceV1::VerifiedServerIncident,
        DurableDeathProvenanceV1::AdministrativeAction,
    ] {
        reset_zero_custody_fixture(&persistence).await;
        stage_second_zero_custody_character(&persistence).await;
        let primary = zero_custody_request(RequestIds::primary());
        let primary_evidence = seed_hosted_death_trace(&persistence, &primary).await;
        let primary_promotion = primary_evidence.promotion_for(&primary);
        persistence
            .transact_durable_death(&primary, &content, &primary_promotion)
            .await
            .unwrap();
        assert_successor_death_authority(&persistence, &primary, 0, None).await;
        select_second_character_after_primary_death(&persistence).await;

        let nonordinary = second_zero_custody_request(RequestIds::incident(), provenance);
        let evidence = seed_hosted_death_trace(&persistence, &nonordinary).await;
        let promotion = evidence.promotion_for(&nonordinary);
        let fresh = persistence
            .transact_durable_death(&nonordinary, &content, &promotion)
            .await
            .unwrap();
        assert!(matches!(fresh, DurableDeathTransactionV1::Fresh(_)));
        assert_successor_death_authority(&persistence, &primary, 0, None).await;
        assert_no_successor_death_authority(&persistence, nonordinary.plan.event.death_id).await;
        assert_eq!(count(&persistence, "death_successor_presets_v1").await, 1);
        assert_eq!(
            count(&persistence, "successor_roster_reservations_v1").await,
            1
        );
        let bootstrap = persistence
            .load_private_life_bootstrap_v1(ACCOUNT_ID)
            .await
            .unwrap();
        assert!(matches!(
            bootstrap.state,
            StoredPrivateLifeBootstrapStateV1::DeathCommitted(ref terminal)
                if terminal.result.death_id == RequestIds::primary().death_id
                    && terminal.result.character_id == CHARACTER_ID
        ));

        let replay = persistence
            .transact_durable_death(&nonordinary, &content, &promotion)
            .await
            .unwrap();
        assert!(replay.is_replay());
        assert_eq!(replay.result(), fresh.result());
        assert_successor_death_authority(&persistence, &primary, 0, None).await;
        assert_no_successor_death_authority(&persistence, nonordinary.plan.event.death_id).await;
    }
    persistence.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn death_without_telemetry_session_commits_unbound() {
    let persistence = disposable_database().await;
    let content = content_authority();
    reset_fixture(&persistence).await;
    let request = request(RequestIds::primary());
    let evidence = seed_hosted_death_trace(&persistence, &request).await;
    let promotion = evidence.promotion_for(&request);
    assert!(matches!(
        persistence
            .transact_durable_death(&request, &content, &promotion)
            .await
            .unwrap(),
        DurableDeathTransactionV1::Fresh(_)
    ));
    terminal_telemetry::assert_unbound_terminal(
        &persistence,
        terminal_telemetry::TerminalFamily::Death,
    )
    .await;
    persistence.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
#[allow(
    clippy::too_many_lines,
    reason = "one hosted gate proves commit, replay, races, rejection, corruption, and wipe ownership"
)]
async fn complete_durable_death_graph_is_atomic_replayable_terminal_and_wipeable() {
    let persistence = disposable_database().await;
    let content = content_authority();
    reset_fixture(&persistence).await;
    terminal_telemetry::start_skewed_session(&persistence, ACCOUNT_ID, TELEMETRY_SESSION_ID).await;
    let primary = request(RequestIds::primary());
    let evidence = seed_hosted_death_trace(&persistence, &primary).await;
    let primary_promotion = evidence.promotion_for(&primary);
    let fresh = persistence
        .transact_durable_death(&primary, &content, &primary_promotion)
        .await
        .unwrap();
    assert!(matches!(fresh, DurableDeathTransactionV1::Fresh(_)));
    assert_complete_graph(&persistence, RequestIds::primary()).await;
    assert_stored_tel_003_context_and_redacted_projection(
        &persistence,
        RequestIds::primary().death_id,
    )
    .await;
    assert_committed_terminal_recovery(&persistence, fresh.result(), &primary_promotion).await;
    let before_restart_signature = canonical_terminal_signature(&persistence).await;
    assert_eq!(
        persistence
            .load_core_death_terminal_signature_v1([229; 16], CHARACTER_ID)
            .await
            .unwrap(),
        None
    );
    assert!(matches!(
        persistence
            .load_core_death_terminal_signature_v1([0; 16], CHARACTER_ID)
            .await,
        Err(PersistenceError::CorruptStoredDeathTerminalSignature)
    ));
    assert_trace_promotion_history_is_sealed(&persistence).await;
    assert_cross_account_promotion_guards(&persistence).await;
    persistence.close().await;

    let restarted = reconnect_database().await;
    let after_reconnect_signature = canonical_terminal_signature(&restarted).await;
    assert_eq!(
        after_reconnect_signature.canonical_bytes().unwrap(),
        before_restart_signature.canonical_bytes().unwrap()
    );
    let replay = restarted
        .transact_durable_death(&primary, &content, &primary_promotion)
        .await
        .unwrap();
    assert!(replay.is_replay());
    assert_eq!(replay.result(), fresh.result());
    let after_replay_signature = canonical_terminal_signature(&restarted).await;
    assert_eq!(after_replay_signature, before_restart_signature);
    assert_normalized_live_trace_absent(&restarted).await;
    assert_committed_terminal_recovery(&restarted, fresh.result(), &primary_promotion).await;
    assert_committed_death_views(&restarted, RequestIds::primary()).await;
    assert_immutable_death_history_rejects_direct_mutation(&restarted, &before_restart_signature)
        .await;
    let replay_after_immutability_matrix = restarted
        .transact_durable_death(&primary, &content, &primary_promotion)
        .await
        .unwrap();
    assert!(replay_after_immutability_matrix.is_replay());
    assert_eq!(replay_after_immutability_matrix.result(), fresh.result());
    assert_eq!(
        canonical_terminal_signature(&restarted).await,
        before_restart_signature
    );
    let altered_promotion = evidence.altered_predecessor_promotion_for(&primary);
    assert!(matches!(
        restarted
            .transact_durable_death(&primary, &content, &altered_promotion)
            .await,
        Err(PersistenceError::DurableDeathTracePromotionConflict)
    ));
    assert_trace_promotion_conflict_audit(&restarted, &primary_promotion, &altered_promotion).await;
    let changed_payload = request(RequestIds::changed_payload());
    let changed_payload_promotion = evidence.promotion_for(&changed_payload);
    assert!(matches!(
        restarted
            .transact_durable_death(&changed_payload, &content, &changed_payload_promotion,)
            .await,
        Err(PersistenceError::DurableDeathIdempotencyConflict)
    ));
    let changed_final_identity = request(RequestIds::changed_final_identity());
    let changed_final_promotion = evidence.promotion_for(&changed_final_identity);
    assert!(matches!(
        restarted
            .transact_durable_death(&changed_final_identity, &content, &changed_final_promotion,)
            .await,
        Err(PersistenceError::DurableDeathIdempotencyConflict)
    ));
    assert_post_death_rejection(&restarted).await;
    terminal_telemetry::assert_bound_immutable_restart_poll_ack(
        &restarted,
        terminal_telemetry::TerminalFamily::Death,
        TELEMETRY_SESSION_ID,
        "character_died",
    )
    .await;

    reset_fixture(&restarted).await;
    let concurrent_request = request(RequestIds::primary());
    let concurrent_evidence = seed_hosted_death_trace(&restarted, &concurrent_request).await;
    let concurrent_promotion = concurrent_evidence.promotion_for(&concurrent_request);
    let left_persistence = restarted.clone();
    let right_persistence = restarted.clone();
    let (left, right) = tokio::join!(
        left_persistence.transact_durable_death(
            &concurrent_request,
            &content,
            &concurrent_promotion,
        ),
        right_persistence.transact_durable_death(
            &concurrent_request,
            &content,
            &concurrent_promotion,
        ),
    );
    let left = left.unwrap();
    let right = right.unwrap();
    assert_ne!(left.is_replay(), right.is_replay());
    assert_eq!(left.result(), right.result());
    assert_complete_graph(&restarted, RequestIds::primary()).await;

    for boundary in DURABLE_DEATH_FAULT_BOUNDARIES {
        reset_fixture(&restarted).await;
        let rejected_request = request(RequestIds::primary());
        let rejected_evidence = seed_hosted_death_trace(&restarted, &rejected_request).await;
        let rejected_promotion = rejected_evidence.promotion_for(&rejected_request);
        install_durable_death_fault(&restarted, boundary).await;
        let rejected = restarted
            .transact_durable_death(&rejected_request, &content, &rejected_promotion)
            .await;
        remove_durable_death_fault(&restarted, boundary).await;
        assert!(
            matches!(rejected, Err(PersistenceError::Database(_))),
            "{} failpoint must reject the transaction",
            boundary.label
        );
        assert_rollback_pristine(&restarted).await;

        let retry = restarted
            .transact_durable_death(&rejected_request, &content, &rejected_promotion)
            .await
            .unwrap();
        assert!(
            matches!(retry, DurableDeathTransactionV1::Fresh(_)),
            "{} retry must converge to one fresh commit",
            boundary.label
        );
        let replay = restarted
            .transact_durable_death(&rejected_request, &content, &rejected_promotion)
            .await
            .unwrap();
        assert!(replay.is_replay(), "{} exact replay", boundary.label);
        assert_eq!(replay.result(), retry.result(), "{} result", boundary.label);
        assert_complete_graph(&restarted, RequestIds::primary()).await;
    }

    reset_fixture(&restarted).await;
    let serialization_request = request(RequestIds::primary());
    let serialization_evidence = seed_hosted_death_trace(&restarted, &serialization_request).await;
    let serialization_promotion = serialization_evidence.promotion_for(&serialization_request);
    install_serialization_retry_fault(&restarted).await;
    let serialization_result = restarted
        .transact_durable_death(&serialization_request, &content, &serialization_promotion)
        .await;
    let serialization_attempts = remove_serialization_retry_fault(&restarted).await;
    let serialization_fresh = serialization_result.unwrap();
    assert!(matches!(
        serialization_fresh,
        DurableDeathTransactionV1::Fresh(_)
    ));
    assert_eq!(serialization_attempts, 2);
    let serialization_replay = restarted
        .transact_durable_death(&serialization_request, &content, &serialization_promotion)
        .await
        .unwrap();
    assert!(serialization_replay.is_replay());
    assert_eq!(serialization_replay.result(), serialization_fresh.result());
    assert_complete_graph(&restarted, RequestIds::primary()).await;

    reset_fixture(&restarted).await;
    // Authorities: Gravebound_Production_GDD_v1_Canonical.md TECH-023,
    // Gravebound_Content_Production_Spec_v1.md CONT-HUB-002, and
    // Gravebound_Development_Roadmap_v1.md GB-M03-03/06 require terminal death to close either
    // schema-open lineage phase, while state-0 journeys above retain the production entry path.
    promote_fixture_lineage_to_active(&restarted).await;
    let final_evidence = seed_hosted_death_trace(&restarted, &primary).await;
    let final_promotion = final_evidence.promotion_for(&primary);
    let committed = restarted
        .transact_durable_death(&primary, &content, &final_promotion)
        .await
        .unwrap();
    assert!(matches!(committed, DurableDeathTransactionV1::Fresh(_)));

    // The canonical GDD, content spec, and roadmap jointly require restart-safe exact trace
    // authority. Simulate storage corruption beneath the immutable triggers: root hashes remain
    // coherent, so only semantic durable/provenance revalidation can reject these alterations.
    let original_attack_id =
        rewrite_durable_trace_attack_id(&restarted, "attack.core.corrupt_trace").await;
    assert!(matches!(
        restarted
            .transact_durable_death(&primary, &content, &final_promotion)
            .await,
        Err(PersistenceError::CorruptStoredDurableDeath)
    ));
    assert!(matches!(
        restarted
            .load_committed_death_terminal_v1(ACCOUNT_ID, CHARACTER_ID)
            .await,
        Err(PersistenceError::CorruptStoredDurableDeath)
    ));
    assert!(matches!(
        restarted
            .load_core_death_terminal_signature_v1(ACCOUNT_ID, CHARACTER_ID)
            .await,
        Err(PersistenceError::CorruptStoredDurableDeath)
    ));
    rewrite_durable_trace_attack_id(&restarted, &original_attack_id).await;

    let original_status_ticks =
        rewrite_first_durable_trace_status_remaining_ticks(&restarted, 31).await;
    assert_ne!(original_status_ticks, 31);
    assert!(matches!(
        restarted
            .transact_durable_death(&primary, &content, &final_promotion)
            .await,
        Err(PersistenceError::CorruptStoredDurableDeath)
    ));
    assert!(matches!(
        restarted
            .load_committed_death_terminal_v1(ACCOUNT_ID, CHARACTER_ID)
            .await,
        Err(PersistenceError::CorruptStoredDurableDeath)
    ));
    assert!(matches!(
        restarted
            .load_core_death_terminal_signature_v1(ACCOUNT_ID, CHARACTER_ID)
            .await,
        Err(PersistenceError::CorruptStoredDurableDeath)
    ));
    rewrite_first_durable_trace_status_remaining_ticks(&restarted, original_status_ticks).await;
    assert!(
        restarted
            .transact_durable_death(&primary, &content, &final_promotion)
            .await
            .unwrap()
            .is_replay()
    );
    assert_committed_terminal_recovery(&restarted, committed.result(), &final_promotion).await;

    let canonical_receipt_window_digest = stored_receipt_window_digest(&restarted).await;
    let corrupt_promotion_digest = [221; 32];
    let corrupt_terminal_payload_hash = canonical_death_terminal_payload_hash_v1(
        primary.canonical_request_hash,
        corrupt_promotion_digest,
    )
    .unwrap();
    set_promotion_root_hashes(
        &restarted,
        [220; 32],
        corrupt_promotion_digest,
        corrupt_terminal_payload_hash,
    )
    .await;
    assert!(matches!(
        restarted
            .transact_durable_death(&primary, &content, &final_promotion)
            .await,
        Err(PersistenceError::CorruptStoredDurableDeath)
    ));
    assert!(matches!(
        restarted
            .load_committed_death_terminal_v1(ACCOUNT_ID, CHARACTER_ID)
            .await,
        Err(PersistenceError::CorruptStoredDurableDeath)
    ));
    assert!(matches!(
        restarted
            .load_core_death_terminal_signature_v1(ACCOUNT_ID, CHARACTER_ID)
            .await,
        Err(PersistenceError::CorruptStoredDurableDeath)
    ));
    set_promotion_root_hashes(
        &restarted,
        canonical_receipt_window_digest,
        final_promotion.promotion_digest(),
        final_promotion.terminal_payload_hash(),
    )
    .await;
    assert!(
        restarted
            .transact_durable_death(&primary, &content, &final_promotion)
            .await
            .unwrap()
            .is_replay()
    );
    assert_committed_terminal_recovery(&restarted, committed.result(), &final_promotion).await;
    let restored_signature = canonical_terminal_signature(&restarted).await;
    assert_altered_signature_rows_fail_closed(&restarted, &restored_signature).await;
    assert_death_committed_outbox_cardinality(&restarted, &restored_signature).await;
    corrupt_result_hash(&restarted, [222; 32]).await;
    assert!(matches!(
        restarted
            .transact_durable_death(&primary, &content, &final_promotion)
            .await,
        Err(PersistenceError::CorruptStoredDurableDeath)
    ));
    assert!(matches!(
        restarted
            .load_core_death_terminal_signature_v1(ACCOUNT_ID, CHARACTER_ID)
            .await,
        Err(PersistenceError::CorruptStoredDurableDeath)
    ));
    assert_complete_graph(&restarted, RequestIds::primary()).await;
    wipe_disposable_database(&restarted).await;
    restarted.close().await;
}
