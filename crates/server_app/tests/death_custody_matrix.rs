//! Hosted full-custody permadeath evidence for `GB-M03-06C` and `GB-M03-02D`.
//!
//! Authorities:
//! - `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-001`, `LOOT-002`, `LOOT-033`,
//!   `LOOT-050`, and `LOOT-060`;
//! - `Gravebound_Content_Production_Spec_v1.md`: Core item identities and `CONT-HUB-002`;
//! - `Gravebound_Development_Roadmap_v1.md`: `GB-M03-02`, `GB-M03-06`, and the M03 atomicity,
//!   restart, and nonduplication gates;
//! - accepted `SPEC-CONFLICT-009` and `SPEC-CONFLICT-028`.
//!
//! The fixture enters through the production server planner and `PostgreSQL` terminal writer. It
//! fills every Equipment/Belt/RunBackpack capacity, includes `PersonalGround` and all Core run
//! material families, preserves real CharacterSafe/Vault rows, and replays the stored result.

use persistence::{
    DurableDeathItemContentAuthorityV1, DurableDeathTransactionV1, DurableDestructionLocationV1,
    DurableEquipmentSlotV1, PersistenceConfig, PostgresPersistence, WIPEABLE_CORE_NAMESPACE,
};
use server_app::{DeathAtRiskItem, DeathAtRiskRunMaterial, DeathCustodySnapshot};
use sqlx::Row;

#[path = "support/death_measurement.rs"]
mod death_measurement;
#[path = "support/durable_death.rs"]
mod durable_death_fixture;

const RELIC_ID: &str = "item.relic.arbalist.long_lens";
const ARMOR_ID: &str = "item.armor.ashplate.t1";
const CHARM_ID: &str = "item.charm.bell_locket.t1";
const TONIC_ID: &str = "consumable.red_tonic";
const FUNERAL_ROOT_ID: &str = "material.funeral_root";
const SALTGLASS_ID: &str = "material.saltglass_shard";
const CHARACTER_SAFE_UID: [u8; 16] = [160; 16];
const VAULT_UID: [u8; 16] = [161; 16];

fn persistence_config() -> PersistenceConfig {
    PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL")
}

fn gear(
    uid: u8,
    content_id: &str,
    location: DurableDestructionLocationV1,
    version: u64,
) -> DeathAtRiskItem {
    DeathAtRiskItem {
        content_id: content_id.into(),
        item_uid: [uid; 16],
        location,
        item_version: version,
    }
}

fn enabled_items() -> Vec<DurableDeathItemContentAuthorityV1> {
    [
        TONIC_ID,
        ARMOR_ID,
        CHARM_ID,
        RELIC_ID,
        durable_death_fixture::ITEM_TEMPLATE_ID,
    ]
    .into_iter()
    .map(|template_id| DurableDeathItemContentAuthorityV1 {
        template_id: template_id.into(),
        echo_signature_tag: None,
    })
    .collect()
}

fn full_custody() -> DeathCustodySnapshot {
    let identity = durable_death_fixture::PRIMARY_IDENTITY;
    let mut items = vec![
        gear(
            identity.item_uid[0],
            durable_death_fixture::ITEM_TEMPLATE_ID,
            DurableDestructionLocationV1::Equipment {
                slot: DurableEquipmentSlotV1::Weapon,
            },
            2,
        ),
        gear(
            101,
            RELIC_ID,
            DurableDestructionLocationV1::Equipment {
                slot: DurableEquipmentSlotV1::Relic,
            },
            2,
        ),
        gear(
            102,
            ARMOR_ID,
            DurableDestructionLocationV1::Equipment {
                slot: DurableEquipmentSlotV1::Armor,
            },
            2,
        ),
        gear(
            103,
            CHARM_ID,
            DurableDestructionLocationV1::Equipment {
                slot: DurableEquipmentSlotV1::Charm,
            },
            2,
        ),
    ];
    for slot in 0_u8..=1 {
        for unit in 0_u8..6 {
            items.push(gear(
                110 + slot * 6 + unit,
                TONIC_ID,
                DurableDestructionLocationV1::Belt { index: slot },
                2,
            ));
        }
    }
    for slot in 0_u8..8 {
        items.push(gear(
            130 + slot,
            durable_death_fixture::ITEM_TEMPLATE_ID,
            DurableDestructionLocationV1::RunBackpack { index: slot },
            1,
        ));
    }
    for (uid, pickup) in [(140_u8, 150_u8), (141, 151)] {
        items.push(gear(
            uid,
            durable_death_fixture::ITEM_TEMPLATE_ID,
            DurableDestructionLocationV1::PersonalGround {
                instance_id: identity.instance_id,
                pickup_id: [pickup; 16],
            },
            1,
        ));
    }
    DeathCustodySnapshot {
        items,
        run_materials: vec![
            DeathAtRiskRunMaterial {
                material_id: durable_death_fixture::MATERIAL_ID.into(),
                quantity: 7,
                material_version: 1,
            },
            DeathAtRiskRunMaterial {
                material_id: FUNERAL_ROOT_ID.into(),
                quantity: 5,
                material_version: 1,
            },
            DeathAtRiskRunMaterial {
                material_id: SALTGLASS_ID.into(),
                quantity: 9,
                material_version: 1,
            },
        ],
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "the hosted fixture keeps every destructive and preserved custody row explicit"
)]
async fn seed_full_custody(persistence: &PostgresPersistence) {
    let identity = durable_death_fixture::PRIMARY_IDENTITY;
    let account_id = durable_death_fixture::ACCOUNT_ID;
    let character_id = identity.character_id;
    let mut transaction = persistence.begin_transaction().await.unwrap();

    for (uid, template_id, slot) in [
        (101_u8, RELIC_ID, 1_i16),
        (102, ARMOR_ID, 2),
        (103, CHARM_ID, 3),
    ] {
        sqlx::query(
            "INSERT INTO item_instances (namespace_id,item_uid,account_id,character_id,template_id, \
             content_revision,item_kind,item_level,rarity,creation_kind,creation_request_id, \
             roll_index,unit_ordinal,item_version,security_state,location_kind,slot_index, \
             provenance_kind,salvage_band,salvage_value) \
             VALUES ($1,$2,$3,$4,$5,$6,0,10,0,1,$2,0,0,2,1,0,$7,1,1,1)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind([uid; 16].as_slice())
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .bind(template_id)
        .bind(persistence::CORE_ITEM_CONTENT_REVISION)
        .bind(slot)
        .execute(transaction.connection())
        .await
        .unwrap();
    }
    for slot in 0_i16..=1 {
        for unit in 0_i16..6 {
            let uid = u8::try_from(110 + slot * 6 + unit).unwrap();
            sqlx::query(
                "INSERT INTO item_instances (namespace_id,item_uid,account_id,character_id, \
                 template_id,content_revision,item_kind,creation_kind,creation_request_id, \
                 roll_index,unit_ordinal,item_version,security_state,location_kind,slot_index, \
                 provenance_kind,salvage_band,salvage_value) \
                 VALUES ($1,$2,$3,$4,$5,$6,1,1,$2,0,$7,2,1,1,$8,1,0,0)",
            )
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind([uid; 16].as_slice())
            .bind(account_id.as_slice())
            .bind(character_id.as_slice())
            .bind(TONIC_ID)
            .bind(persistence::CORE_ITEM_CONTENT_REVISION)
            .bind(unit)
            .bind(slot)
            .execute(transaction.connection())
            .await
            .unwrap();
        }
    }
    for slot in 0_i16..8 {
        let uid = u8::try_from(130 + slot).unwrap();
        sqlx::query(
            "INSERT INTO item_instances (namespace_id,item_uid,account_id,character_id,template_id, \
             content_revision,item_kind,item_level,rarity,creation_kind,creation_request_id, \
             roll_index,unit_ordinal,item_version,security_state,location_kind,slot_index, \
             provenance_kind,salvage_band,salvage_value) \
             VALUES ($1,$2,$3,$4,$5,$6,0,10,0,1,$2,0,0,1,2,2,$7,1,1,1)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind([uid; 16].as_slice())
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .bind(durable_death_fixture::ITEM_TEMPLATE_ID)
        .bind(persistence::CORE_ITEM_CONTENT_REVISION)
        .bind(slot)
        .execute(transaction.connection())
        .await
        .unwrap();
    }
    for (uid, pickup) in [(140_u8, 150_u8), (141, 151)] {
        sqlx::query(
            "INSERT INTO item_instances (namespace_id,item_uid,account_id,character_id,template_id, \
             content_revision,item_kind,item_level,rarity,creation_kind,creation_request_id, \
             roll_index,unit_ordinal,item_version,security_state,location_kind,instance_id,pickup_id, \
             expires_at_tick,provenance_kind,salvage_band,salvage_value) \
             VALUES ($1,$2,$3,$4,$5,$6,0,10,0,1,$2,0,0,1,2,3,$7,$8,30000,1,1,1)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind([uid; 16].as_slice())
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .bind(durable_death_fixture::ITEM_TEMPLATE_ID)
        .bind(persistence::CORE_ITEM_CONTENT_REVISION)
        .bind(identity.instance_id.as_slice())
        .bind([pickup; 16].as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO item_instances (namespace_id,item_uid,account_id,character_id,template_id, \
         content_revision,item_kind,item_level,rarity,creation_kind,creation_request_id,roll_index, \
         unit_ordinal,item_version,security_state,location_kind,slot_index,provenance_kind, \
         salvage_band,salvage_value) \
         VALUES ($1,$2,$3,$4,$5,$6,0,10,0,1,$2,0,0,1,0,5,0,1,1,1), \
                ($1,$7,$3,NULL,$5,$6,0,10,0,1,$7,0,0,1,0,6,0,1,1,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(CHARACTER_SAFE_UID.as_slice())
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(durable_death_fixture::ITEM_TEMPLATE_ID)
    .bind(persistence::CORE_ITEM_CONTENT_REVISION)
    .bind(VAULT_UID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    for (material_id, quantity) in [(FUNERAL_ROOT_ID, 5_i32), (SALTGLASS_ID, 9_i32)] {
        sqlx::query(
            "INSERT INTO character_run_material_stacks (namespace_id,account_id,character_id, \
             material_id,quantity,material_version,security_state) VALUES ($1,$2,$3,$4,$5,1,2)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .bind(material_id)
        .bind(quantity)
        .execute(transaction.connection())
        .await
        .unwrap();
    }
    let changed = sqlx::query(
        "UPDATE character_inventories SET inventory_version=3 WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3 AND inventory_version=2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(changed, 1);
    transaction.commit().await.unwrap();
}

async fn assert_full_custody_result(
    persistence: &PostgresPersistence,
    death_id: [u8; 16],
    expected_destruction_count: usize,
) {
    let identity = durable_death_fixture::PRIMARY_IDENTITY;
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let counts = sqlx::query(
        "SELECT \
            (SELECT count(*) FROM item_instances WHERE namespace_id=$1 AND account_id=$2 \
             AND character_id=$3 AND terminal_death_id=$4 AND security_state=3 \
             AND location_kind=4 AND destruction_reason='permadeath') AS destroyed_items, \
            (SELECT count(*) FROM item_ledger_events WHERE namespace_id=$1 AND account_id=$2 \
             AND character_id=$3 AND terminal_death_id=$4 AND event_kind=2 \
             AND source_kind=3 AND reason='permadeath') AS death_ledgers, \
            (SELECT count(*) FROM death_destruction_entries WHERE namespace_id=$1 \
             AND death_id=$4) AS destruction_entries, \
            (SELECT count(*) FROM item_instances WHERE namespace_id=$1 AND account_id=$2 \
             AND character_id=$3 AND security_state IN (1,2)) AS remaining_at_risk",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(durable_death_fixture::ACCOUNT_ID.as_slice())
    .bind(identity.character_id.as_slice())
    .bind(death_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(counts.get::<i64, _>("destroyed_items"), 26);
    assert_eq!(counts.get::<i64, _>("death_ledgers"), 26);
    assert_eq!(
        counts.get::<i64, _>("destruction_entries"),
        i64::try_from(expected_destruction_count).unwrap()
    );
    assert_eq!(counts.get::<i64, _>("remaining_at_risk"), 0);

    let safe_rows = sqlx::query(
        "SELECT item_uid,character_id,item_version,security_state,location_kind,slot_index, \
                terminal_death_id FROM item_instances WHERE namespace_id=$1 AND account_id=$2 \
                AND item_uid IN ($3,$4) ORDER BY item_uid",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(durable_death_fixture::ACCOUNT_ID.as_slice())
    .bind(CHARACTER_SAFE_UID.as_slice())
    .bind(VAULT_UID.as_slice())
    .fetch_all(transaction.connection())
    .await
    .unwrap();
    assert_eq!(safe_rows.len(), 2);
    assert_eq!(safe_rows[0].get::<i16, _>("location_kind"), 5);
    assert_eq!(
        safe_rows[0].get::<Option<Vec<u8>>, _>("character_id"),
        Some(identity.character_id.to_vec())
    );
    assert_eq!(safe_rows[1].get::<i16, _>("location_kind"), 6);
    assert_eq!(safe_rows[1].get::<Option<Vec<u8>>, _>("character_id"), None);
    for row in safe_rows {
        assert_eq!(row.get::<i64, _>("item_version"), 1);
        assert_eq!(row.get::<i16, _>("security_state"), 0);
        assert_eq!(row.get::<Option<Vec<u8>>, _>("terminal_death_id"), None);
        assert_eq!(row.get::<Option<i16>, _>("slot_index"), Some(0));
    }

    let materials = sqlx::query(
        "SELECT material_id,quantity,material_version,security_state,terminal_reason, \
                terminal_death_id FROM character_run_material_stacks WHERE namespace_id=$1 \
                AND account_id=$2 AND character_id=$3 ORDER BY material_id COLLATE \"C\"",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(durable_death_fixture::ACCOUNT_ID.as_slice())
    .bind(identity.character_id.as_slice())
    .fetch_all(transaction.connection())
    .await
    .unwrap();
    assert_eq!(materials.len(), 3);
    for row in materials {
        assert_eq!(row.get::<i32, _>("quantity"), 0);
        assert_eq!(row.get::<i64, _>("material_version"), 2);
        assert_eq!(row.get::<i16, _>("security_state"), 3);
        assert_eq!(row.get::<String, _>("terminal_reason"), "permadeath");
        assert_eq!(row.get::<Vec<u8>, _>("terminal_death_id"), death_id);
    }
    transaction.rollback().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn full_custody_death_is_canonical_atomic_preserving_and_replay_safe() {
    let persistence = PostgresPersistence::connect(&persistence_config())
        .await
        .unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence.migrate().await.unwrap();

    let mut scenario = durable_death_fixture::DurableDeathScenarioV1::primary_eligible();
    scenario.inventory_pre_version = 3;
    durable_death_fixture::seed_danger_root_for(&persistence, &scenario).await;
    seed_full_custody(&persistence).await;
    let custody = full_custody();
    let prepared = durable_death_fixture::prepare_death_for_with_custody(
        persistence.clone(),
        &scenario,
        // CONT-ECHO-001: four level-10 Worn slots produce functional 100 tenths;
        // round_half_up((level 100 + functional 100) / 2) is index 100, hence Band 2.
        2,
        custody.clone(),
        enabled_items(),
    )
    .await;
    assert_eq!(
        prepared.request().plan.destruction.len(),
        custody.items.len() + custody.run_materials.len()
    );
    let plan = &prepared.request().plan;
    let echo = &plan
        .echo
        .as_ref()
        .expect("eligible full-custody death must create one Echo")
        .created;
    assert_eq!(echo.power_band, 2);
    assert_eq!(
        echo.character_name_snapshot,
        plan.summary.character_name_snapshot
    );
    assert_eq!(echo.class_id, plan.summary.class_id);
    assert_eq!(echo.oath_id, plan.summary.oath_id);
    assert_eq!(echo.level, plan.summary.level);
    assert_eq!(echo.killer_content_id, plan.event.killer_content_id);
    assert_eq!(echo.killer_pattern_id, plan.event.killer_pattern_id);
    assert_eq!(echo.death_region_id, plan.event.region_id);

    let fresh = persistence
        .transact_durable_death(prepared.request(), prepared.content(), prepared.promotion())
        .await
        .unwrap();
    assert!(matches!(fresh, DurableDeathTransactionV1::Fresh(_)));
    let first_signature = persistence
        .load_core_death_terminal_signature_v1(
            durable_death_fixture::ACCOUNT_ID,
            scenario.identity.character_id,
        )
        .await
        .unwrap()
        .unwrap();
    first_signature.canonical_bytes().unwrap();
    assert_eq!(first_signature.echoes.len(), 1);
    assert_eq!(first_signature.echoes[0].power_band, 2);

    let replay = persistence
        .transact_durable_death(prepared.request(), prepared.content(), prepared.promotion())
        .await
        .unwrap();
    assert!(replay.is_replay());
    assert_eq!(fresh.result(), replay.result());
    let replay_signature = persistence
        .load_core_death_terminal_signature_v1(
            durable_death_fixture::ACCOUNT_ID,
            scenario.identity.character_id,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first_signature, replay_signature);

    assert_full_custody_result(
        &persistence,
        scenario.identity.death_id,
        custody.items.len() + custody.run_materials.len(),
    )
    .await;
    assert!(
        death_measurement::PostgresResidueSnapshotV1::capture(&persistence)
            .await
            .unwrap()
            .is_zero()
    );
}
