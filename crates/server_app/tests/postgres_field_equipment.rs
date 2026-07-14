use std::path::{Path, PathBuf};

use persistence::{PersistenceConfig, PostgresPersistence, WIPEABLE_CORE_NAMESPACE};
use server_app::{
    FieldEquipmentConfirmCommand, FieldEquipmentPreviewSource, FieldEquipmentServiceError,
    PostgresFieldEquipmentService, initialize_postgres_starter,
};

// Keep this fixture's identity range isolated from the other PostgreSQL gates. The CI job
// intentionally runs those gates against one database, so reusing an account would make this
// test's cleanup depend on another test's foreign-key graph.
const ACCOUNT_ID: [u8; 16] = [201; 16];
const CHARACTER_ID: [u8; 16] = [202; 16];
const REWARD_UID: [u8; 16] = [203; 16];
const REWARD_REQUEST_ID: [u8; 16] = [204; 16];

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

async fn seed_fixture(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "DELETE FROM field_equipment_mutations WHERE namespace_id = $1 AND account_id = $2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query("DELETE FROM accounts WHERE namespace_id = $1 AND account_id = $2")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ACCOUNT_ID.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO accounts (namespace_id, account_id, state_version, slot_capacity) \
         VALUES ($1, $2, 1, 2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO characters (namespace_id, account_id, character_id, roster_ordinal, \
         class_id, level, oath_id, life_state, security_state, character_state_version) \
         VALUES ($1, $2, $3, 1, 'class.grave_arbalist', 4, NULL, 0, 0, 1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    initialize_postgres_starter(persistence, ACCOUNT_ID, CHARACTER_ID)
        .await
        .unwrap();

    let revision = sim_content::load_core_development_items(&content_root())
        .unwrap()
        .revision_label()
        .to_owned();
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO item_instances (namespace_id, item_uid, account_id, character_id, \
         template_id, content_revision, item_kind, item_level, rarity, creation_kind, \
         creation_request_id, roll_index, unit_ordinal, item_version, security_state, \
         location_kind, slot_index, provenance_kind, salvage_band, salvage_value) \
         VALUES ($1,$2,$3,$4,'item.weapon.crossbow.grave_repeater',$5,0,4,1,1,$6,0,0,1,2,2,0,1,1,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(REWARD_UID.as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(revision)
    .bind(REWARD_REQUEST_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO item_ledger_events (namespace_id, ledger_event_id, item_uid, account_id, \
         character_id, mutation_id, event_kind, source_kind, pre_item_version, post_item_version, \
         post_security_state, post_location_kind) VALUES ($1,$2,$2,$3,$4,$5,0,1,0,1,2,2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(REWARD_UID.as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(REWARD_REQUEST_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_inventories SET inventory_version = inventory_version + 1 \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn field_equipment_swap_is_atomic_replay_safe_and_restart_durable() {
    let persistence = disposable_database().await;
    seed_fixture(&persistence).await;
    let service =
        PostgresFieldEquipmentService::load(persistence.clone(), &content_root()).unwrap();
    let source = FieldEquipmentPreviewSource::RunBackpack { slot_index: 0 };
    let preview = service
        .preview(ACCOUNT_ID, CHARACTER_ID, source, 100)
        .await
        .unwrap();
    assert_eq!(preview.mutation.incoming.item_uid.bytes(), REWARD_UID);
    assert_eq!(
        preview.mutation.replaced.as_ref().unwrap().template_id,
        "item.weapon.crossbow.pine_crossbow"
    );
    assert_eq!(
        preview.mutation.replacement_destination,
        sim_core::ReplacementDestination::RunBackpack { slot_index: 0 }
    );
    let wire = preview.wire_projection(CHARACTER_ID).unwrap();
    wire.validate().unwrap();
    assert_eq!(wire.inventory_version, preview.mutation.inventory_version);
    assert_eq!(wire.preview_hash, preview.mutation.preview_hash);
    assert!(matches!(
        wire.replacement_destination,
        protocol::FieldEquipmentReplacementDestinationV1::RunBackpack { slot_index: 0 }
    ));

    let command = FieldEquipmentConfirmCommand {
        command_id: [120; 16],
        source,
        preview_hash: preview.mutation.preview_hash,
        now_tick: 100,
    };
    let committed = service
        .confirm(ACCOUNT_ID, CHARACTER_ID, command)
        .await
        .unwrap();
    assert!(!committed.result.replayed);
    assert_eq!(
        committed.result.post_inventory_version,
        committed.result.pre_inventory_version + 1
    );

    let restarted =
        PostgresFieldEquipmentService::load(persistence.clone(), &content_root()).unwrap();
    let replay = restarted
        .confirm(ACCOUNT_ID, CHARACTER_ID, command)
        .await
        .unwrap();
    assert!(replay.result.replayed);
    assert_eq!(replay.result.result_hash, committed.result.result_hash);
    let mut changed = command;
    changed.preview_hash[0] ^= 1;
    assert!(matches!(
        restarted.confirm(ACCOUNT_ID, CHARACTER_ID, changed).await,
        Err(FieldEquipmentServiceError::IdempotencyConflict)
    ));

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let equipped: (Vec<u8>, i64) = sqlx::query_as(
        "SELECT item_uid, item_version FROM item_instances WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3 AND location_kind = 0 AND slot_index = 0",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let backpack_template: String = sqlx::query_scalar(
        "SELECT template_id FROM item_instances WHERE namespace_id = $1 AND account_id = $2 \
         AND character_id = $3 AND location_kind = 2 AND slot_index = 0 AND item_kind = 0",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let transitions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM item_ledger_events WHERE namespace_id = $1 AND account_id = $2 \
         AND character_id = $3 AND mutation_id = $4 AND event_kind = 1",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(command.command_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(equipped, (REWARD_UID.to_vec(), 2));
    assert_eq!(backpack_template, "item.weapon.crossbow.pine_crossbow");
    assert_eq!(transitions, 2);
}
