use persistence::{
    AuthoritativeDeathPlanV1, CORE_ITEM_CONTENT_REVISION, DURABLE_DEATH_SCHEMA_VERSION,
    DURABLE_DEATH_SUMMARY_REVISION, DeathAggregateVersionsV1, DeathVersionAdvanceV1,
    DeathViewReadError, DurableCombatTraceEntryV1, DurableDamageTypeV1, DurableDeathCauseV1,
    DurableDeathCommitRequestV1, DurableDeathContentAuthorityV1, DurableDeathEventV1,
    DurableDeathItemContentAuthorityV1, DurableDeathSummaryV1, DurableDeathTransactionV1,
    DurableDestructionEntryV1, DurableDestructionLocationV1, DurableEchoEnvelopeV1,
    DurableEchoOutcomeV1, DurableEchoRecordV1, DurableEchoStateV1, DurableEchoTransitionReasonV1,
    DurableEchoTransitionV1, DurableEquipmentSlotV1, DurableMemorialRecordV1,
    DurableNetworkStateV1, DurableOrderedContentIdV1, DurableRecallStateV1,
    DurableSummaryDamageReferenceV1, DurableSummaryProjectionEntryV1,
    DurableSummaryProjectionKindV1, DurableSummaryProjectionsV1, DurableTraceStatusV1,
    PersistenceConfig, PersistenceError, PostgresPersistence, WIPEABLE_CORE_NAMESPACE,
    stage_danger_entry_ash_wallet_restore_v3, stage_danger_entry_inventory_restore_v3,
    stage_danger_entry_life_metrics_restore_v3, stage_danger_entry_oath_bargain_restore_v3,
};
use serde::Serialize;
use sqlx::Row;

const ACCOUNT_ID: [u8; 16] = [230; 16];
const CHARACTER_ID: [u8; 16] = [231; 16];
const LINEAGE_ID: [u8; 16] = [232; 16];
const RESTORE_POINT_ID: [u8; 16] = [233; 16];
const INSTANCE_ID: [u8; 16] = [234; 16];
const ITEM_UID: [u8; 16] = [235; 16];
const ITEM_LEDGER_ID: [u8; 16] = [236; 16];
const ENTRY_MUTATION_ID: [u8; 16] = [237; 16];
const DEED_REWARD_ID: [u8; 16] = [238; 16];
const MATERIAL_ID: &str = "material.core.iron";
const ITEM_TEMPLATE_ID: &str = "item.weapon.crossbow.pine_crossbow";
const DEED_ID: &str = "deed.core.sepulcher_knight_defeated";
const RECORDS_BLAKE3: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const ASSETS_BLAKE3: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const LOCALIZATION_BLAKE3: &str =
    "3333333333333333333333333333333333333333333333333333333333333333";
const ISSUED_AT_UNIX_MS: u64 = 1;

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

#[allow(
    clippy::too_many_lines,
    reason = "the hosted fixture keeps the complete V3 danger root explicit and auditable"
)]
async fn reset_fixture(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query("DELETE FROM accounts WHERE namespace_id=$1 AND account_id=$2")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ACCOUNT_ID.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
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
    sqlx::query(
        "INSERT INTO character_instance_lineages (namespace_id,account_id,character_id,lineage_id, \
         content_id,layout_id,lineage_state,records_blake3,assets_blake3,localization_blake3) \
         VALUES ($1,$2,$3,$4,'world.core_microrealm_01','layout.core_private_life_01',1,$5,$6,$7)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(LINEAGE_ID.as_slice())
    .bind(RECORDS_BLAKE3)
    .bind(ASSETS_BLAKE3)
    .bind(LOCALIZATION_BLAKE3)
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
         'hub.lantern_halls_01',3,1,1,1,2,1,1,1,31,$6,0,$7,$8,$9)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(RESTORE_POINT_ID.as_slice())
    .bind(LINEAGE_ID.as_slice())
    .bind([91_u8; 32].as_slice())
    .bind(RECORDS_BLAKE3)
    .bind(ASSETS_BLAKE3)
    .bind(LOCALIZATION_BLAKE3)
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
    stage_danger_entry_inventory_restore_v3(
        &mut transaction,
        ACCOUNT_ID,
        CHARACTER_ID,
        RESTORE_POINT_ID,
        ENTRY_MUTATION_ID,
        0,
    )
    .await
    .unwrap();
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
         checkpoint_payload_digest) VALUES ($1,$2,$3,$4,19990,15,$5,2,2,2,1,$6,$7,$8,1,$9,$10)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(LINEAGE_ID.as_slice())
    .bind([94_u8; 32].as_slice())
    .bind(RECORDS_BLAKE3)
    .bind(ASSETS_BLAKE3)
    .bind(LOCALIZATION_BLAKE3)
    .bind(checkpoint_payload.as_slice())
    .bind(checkpoint_payload_digest.as_bytes().as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
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
    sqlx::query(
        "INSERT INTO character_life_deeds (namespace_id,account_id,character_id,deed_id, \
         reward_event_id,source_content_id,deed_kind,achieved_tick,content_revision) \
         VALUES ($1,$2,$3,$4,$5,'boss.sepulcher_knight',0,19000,$6)",
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
    reason = "one complete canonical request graph remains contiguous for authority review"
)]
fn request(ids: RequestIds) -> DurableDeathCommitRequestV1 {
    let trace = vec![
        DurableCombatTraceEntryV1 {
            ordinal: 0,
            event_tick: 19_900,
            event_ordinal: 0,
            source_content_id: "enemy.sepulcher_knight".into(),
            source_entity_id: Some([81; 16]),
            pattern_id: Some("pattern.sepulcher.lance".into()),
            attack_id: "attack.sepulcher.lance".into(),
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
            source_content_id: "enemy.sepulcher_knight".into(),
            source_entity_id: Some([81; 16]),
            pattern_id: Some("pattern.sepulcher.lance".into()),
            attack_id: "attack.sepulcher.lance".into(),
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
            ledger_event_id: ITEM_LEDGER_ID,
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
        instance_id: INSTANCE_ID,
        lineage_id: LINEAGE_ID,
        restore_point_id: RESTORE_POINT_ID,
        region_id: "region.core.microrealm".into(),
        room_id: "room.core.sepulcher".into(),
        death_tick: 20_000,
        committed_at_unix_ms: ISSUED_AT_UNIX_MS,
        cause: DurableDeathCauseV1::DirectHit,
        killer_content_id: "enemy.sepulcher_knight".into(),
        killer_pattern_id: Some("pattern.sepulcher.lance".into()),
        killer_attack_id: "attack.sepulcher.lance".into(),
        raw_damage: 60,
        final_damage: 60,
        damage_type: DurableDamageTypeV1::Physical,
        pre_hit_health: 50,
        source_x_milli_tiles: 1_250,
        source_y_milli_tiles: -500,
        network_state: DurableNetworkStateV1::Connected,
        recall_state: DurableRecallStateV1::Inactive,
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
        appearance_snapshot_id: "appearance.default.grave_arbalist".into(),
        appearance_theme_id: "theme.echo.arbalist_ash".into(),
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

async fn count(persistence: &PostgresPersistence, table: &str) -> i64 {
    let query = match table {
        "accounts" => "SELECT count(*) FROM accounts WHERE namespace_id=$1 AND account_id=$2",
        "characters" => "SELECT count(*) FROM characters WHERE namespace_id=$1 AND account_id=$2",
        "item_instances" => {
            "SELECT count(*) FROM item_instances WHERE namespace_id=$1 AND account_id=$2"
        }
        "character_run_material_stacks" => {
            "SELECT count(*) FROM character_run_material_stacks \
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
    assert_eq!(root.get::<i64, _>("inventory_version"), 3);
    assert_eq!(root.get::<i64, _>("oath_bargain_version"), 2);
    assert_eq!(root.get::<i64, _>("lifetime_ticks"), 20_000);
    assert_eq!(root.get::<i64, _>("permadeath_combat_ticks"), 18_000);
    assert_eq!(root.get::<i64, _>("life_metrics_version"), 3);

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

    for (table, expected) in [
        ("character_danger_checkpoints", 0),
        ("death_events", 1),
        ("death_combat_trace_entries", 2),
        ("death_summary_snapshots", 1),
        ("memorial_records", 1),
        ("death_destruction_entries", 2),
        ("death_mutation_results", 1),
        ("echo_records", 1),
        ("echo_state_transitions", 2),
        ("death_outbox_events", 3),
    ] {
        assert_eq!(count(persistence, table).await, expected, "{table}");
    }
}

async fn assert_rollback_pristine(persistence: &PostgresPersistence) {
    assert_eq!(count(persistence, "character_danger_checkpoints").await, 1);
    assert_eq!(count(persistence, "death_events").await, 0);
    assert_eq!(count(persistence, "death_mutation_results").await, 0);
    assert_eq!(count(persistence, "echo_records").await, 0);
    assert_eq!(count(persistence, "character_life_outbox").await, 0);
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let row = sqlx::query(
        "SELECT account.state_version,account.selected_character_id,character.life_state, \
                character.roster_ordinal,character.character_state_version,item.item_version, \
                item.security_state,item.location_kind,oath.oath_bargain_version, \
                progression.current_health,life.lifetime_ticks,life.permadeath_combat_ticks \
         FROM accounts AS account JOIN characters AS character USING (namespace_id,account_id) \
         JOIN item_instances AS item USING (namespace_id,account_id,character_id) \
         JOIN character_oath_bargain_state AS oath USING (namespace_id,account_id,character_id) \
         JOIN character_progression AS progression USING (namespace_id,account_id,character_id) \
         JOIN character_life_metrics AS life USING (namespace_id,account_id,character_id) \
         WHERE account.namespace_id=$1 AND account.account_id=$2 AND character.character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
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
    assert_eq!(row.get::<i64, _>("oath_bargain_version"), 1);
    assert_eq!(row.get::<i32, _>("current_health"), 50);
    assert_eq!(row.get::<i64, _>("lifetime_ticks"), 19_990);
    assert_eq!(row.get::<i64, _>("permadeath_combat_ticks"), 17_990);
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

async fn set_summary_insert_rejection(persistence: &PostgresPersistence, enabled: bool) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "DROP TRIGGER IF EXISTS fixture_reject_durable_death_summary \
         ON death_summary_snapshots",
    )
    .execute(transaction.connection())
    .await
    .unwrap();
    if enabled {
        sqlx::query(
            "CREATE OR REPLACE FUNCTION fixture_reject_durable_death_summary_v1() \
             RETURNS TRIGGER LANGUAGE plpgsql AS $$ BEGIN \
             RAISE EXCEPTION 'injected durable-death summary rejection'; END $$",
        )
        .execute(transaction.connection())
        .await
        .unwrap();
        sqlx::query(
            "CREATE TRIGGER fixture_reject_durable_death_summary BEFORE INSERT \
             ON death_summary_snapshots FOR EACH ROW \
             EXECUTE FUNCTION fixture_reject_durable_death_summary_v1()",
        )
        .execute(transaction.connection())
        .await
        .unwrap();
    } else {
        sqlx::query("DROP FUNCTION IF EXISTS fixture_reject_durable_death_summary_v1()")
            .execute(transaction.connection())
            .await
            .unwrap();
    }
    transaction.commit().await.unwrap();
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

async fn wipe_account(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let deleted = sqlx::query("DELETE FROM accounts WHERE namespace_id=$1 AND account_id=$2")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ACCOUNT_ID.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap()
        .rows_affected();
    assert_eq!(deleted, 1);
    transaction.commit().await.unwrap();
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
    assert_eq!(latest.records_blake3, RECORDS_BLAKE3);
    assert_eq!(latest.assets_blake3, ASSETS_BLAKE3);
    assert_eq!(latest.localization_blake3, LOCALIZATION_BLAKE3);

    let summary = persistence
        .load_owned_death_summary_view(ACCOUNT_ID, ids.death_id, 0, 32)
        .await
        .unwrap();
    assert_eq!(summary.death_id, ids.death_id);
    assert_eq!(summary.last_five_damage.len(), 2);
    assert_eq!(summary.lost.len(), usize::from(summary.lost_total_count));
    assert!(summary.next_lost_ordinal.is_none());
    assert_eq!(summary.records_blake3, RECORDS_BLAKE3);

    let memorials = persistence
        .load_death_memorial_page(ACCOUNT_ID, None, 32)
        .await
        .unwrap();
    assert_eq!(memorials.entries.len(), 1);
    assert_eq!(memorials.entries[0].cursor.death_id, ids.death_id);
    assert_eq!(memorials.entries[0].records_blake3, RECORDS_BLAKE3);
    assert!(memorials.next_cursor.is_none());

    let trace = persistence
        .load_owned_death_trace_page(ACCOUNT_ID, ids.death_id, 0, 8)
        .await
        .unwrap();
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

#[test]
fn hosted_fixture_request_and_content_authority_are_canonical() {
    let content = content_authority();
    content.validate().unwrap();
    let request = request(RequestIds::primary());
    request.validate().unwrap();
    assert!(content.matches_event(&request.plan.event));
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
    let primary = request(RequestIds::primary());
    let fresh = persistence
        .transact_durable_death(&primary, &content)
        .await
        .unwrap();
    assert!(matches!(fresh, DurableDeathTransactionV1::Fresh(_)));
    assert_complete_graph(&persistence, RequestIds::primary()).await;
    assert_cross_account_promotion_guards(&persistence).await;
    persistence.close().await;

    let restarted = reconnect_database().await;
    let replay = restarted
        .transact_durable_death(&primary, &content)
        .await
        .unwrap();
    assert!(replay.is_replay());
    assert_eq!(replay.result(), fresh.result());
    assert_committed_death_views(&restarted, RequestIds::primary()).await;
    assert!(matches!(
        restarted
            .transact_durable_death(&request(RequestIds::changed_payload()), &content)
            .await,
        Err(PersistenceError::DurableDeathIdempotencyConflict)
    ));
    assert!(matches!(
        restarted
            .transact_durable_death(&request(RequestIds::changed_final_identity()), &content)
            .await,
        Err(PersistenceError::DurableDeathIdempotencyConflict)
    ));
    assert_post_death_rejection(&restarted).await;

    reset_fixture(&restarted).await;
    let concurrent_request = request(RequestIds::primary());
    let left_persistence = restarted.clone();
    let right_persistence = restarted.clone();
    let (left, right) = tokio::join!(
        left_persistence.transact_durable_death(&concurrent_request, &content),
        right_persistence.transact_durable_death(&concurrent_request, &content),
    );
    let left = left.unwrap();
    let right = right.unwrap();
    assert_ne!(left.is_replay(), right.is_replay());
    assert_eq!(left.result(), right.result());
    assert_complete_graph(&restarted, RequestIds::primary()).await;

    reset_fixture(&restarted).await;
    let rejected_request = request(RequestIds::primary());
    set_summary_insert_rejection(&restarted, true).await;
    assert!(matches!(
        restarted
            .transact_durable_death(&rejected_request, &content)
            .await,
        Err(PersistenceError::Database(_))
    ));
    set_summary_insert_rejection(&restarted, false).await;
    assert_rollback_pristine(&restarted).await;

    reset_fixture(&restarted).await;
    let committed = restarted
        .transact_durable_death(&primary, &content)
        .await
        .unwrap();
    assert!(matches!(committed, DurableDeathTransactionV1::Fresh(_)));
    corrupt_result_hash(&restarted, [222; 32]).await;
    assert!(matches!(
        restarted.transact_durable_death(&primary, &content).await,
        Err(PersistenceError::CorruptStoredDurableDeath)
    ));
    assert_complete_graph(&restarted, RequestIds::primary()).await;
    wipe_account(&restarted).await;
    restarted.close().await;
}
