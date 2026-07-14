use persistence::{
    CORE_ITEM_CONTENT_REVISION, PersistenceConfig, PostgresPersistence, WIPEABLE_CORE_NAMESPACE,
};
use protocol::{
    SafeInventoryDestinationV1, SafeInventoryTransferFrameV1, SafeInventoryTransferKindV1,
    SafeInventoryTransferPayloadV1,
};
use server_app::{PostgresSafeInventoryService, SafeInventoryServiceError};

// The mandatory PostgreSQL job shares one database across integration binaries. Keep this
// fixture's identities disjoint so cleanup never depends on another test's foreign-key graph.
const ACCOUNT_ID: [u8; 16] = [210; 16];
const CHARACTER_ID: [u8; 16] = [211; 16];
const ITEM_UID: [u8; 16] = [212; 16];
const CREATION_REQUEST_ID: [u8; 16] = [213; 16];
const MUTATION_ID: [u8; 16] = [214; 16];

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
    sqlx::query("DELETE FROM accounts WHERE namespace_id = $1 AND account_id = $2")
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
        "INSERT INTO characters (namespace_id,account_id,character_id,roster_ordinal,class_id, \
         level,oath_id,life_state,security_state,character_state_version) \
         VALUES ($1,$2,$3,1,'class.grave_arbalist',1,NULL,0,0,1)",
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
        "INSERT INTO character_inventories \
         (namespace_id,account_id,character_id,inventory_version) VALUES ($1,$2,$3,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_world_locations (namespace_id,account_id,character_id, \
         character_version,location_kind,location_content_id,safe_arrival_kind) \
         VALUES ($1,$2,$3,1,1,'hub.lantern_halls_01',0)",
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
         salvage_band,salvage_value) VALUES ($1,$2,$3,$4,'item.weapon.crossbow.pine_crossbow', \
         $5,0,1,0,0,$6,0,0,1,0,5,0,0,0,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ITEM_UID.as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(CORE_ITEM_CONTENT_REVISION)
    .bind(CREATION_REQUEST_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn insert_equipment(
    transaction: &mut persistence::PersistenceTransaction<'_>,
    item_uid: [u8; 16],
    character_id: Option<[u8; 16]>,
    location_kind: i16,
    slot_index: i16,
) {
    sqlx::query(
        "INSERT INTO item_instances (namespace_id,item_uid,account_id,character_id,template_id, \
         content_revision,item_kind,item_level,rarity,creation_kind,creation_request_id,roll_index, \
         unit_ordinal,item_version,security_state,location_kind,slot_index,provenance_kind, \
         salvage_band,salvage_value) VALUES ($1,$2,$3,$4,'item.weapon.crossbow.pine_crossbow', \
         $5,0,1,0,0,$2,0,0,1,0,$6,$7,0,0,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(item_uid.as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(character_id.map(|id| id.to_vec()))
    .bind(CORE_ITEM_CONTENT_REVISION)
    .bind(location_kind)
    .bind(slot_index)
    .execute(transaction.connection())
    .await
    .unwrap();
}

async fn seed_final_vault_slot_race(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    insert_equipment(&mut transaction, [215; 16], Some(CHARACTER_ID), 5, 1).await;
    for slot in 0_i16..159 {
        let item_uid = (30_000_u128 + u128::try_from(slot).unwrap()).to_be_bytes();
        insert_equipment(&mut transaction, item_uid, None, 6, slot).await;
    }
    transaction.commit().await.unwrap();
}

fn transfer_frame() -> SafeInventoryTransferFrameV1 {
    let payload = SafeInventoryTransferPayloadV1 {
        kind: SafeInventoryTransferKindV1::CharacterSafeToVault,
        source_slot_index: 0,
        expected_account_version: 1,
        expected_inventory_version: 1,
    };
    SafeInventoryTransferFrameV1 {
        mutation_id: MUTATION_ID,
        character_id: CHARACTER_ID,
        issued_at_unix_millis: 1,
        payload_hash: payload.canonical_hash(),
        payload,
    }
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn safe_inventory_service_derives_placement_and_replays_after_restart() {
    let persistence = disposable_database().await;
    seed_fixture(&persistence).await;
    let service = PostgresSafeInventoryService::new(persistence.clone());
    let frame = transfer_frame();
    let committed = service.transfer_frame(ACCOUNT_ID, &frame).await.unwrap();
    assert!(!committed.replayed);
    assert_eq!(
        (committed.account_version, committed.inventory_version),
        (2, 2)
    );
    assert_eq!(committed.placements.len(), 1);
    assert_eq!(
        committed.placements[0].destination,
        SafeInventoryDestinationV1::Vault { slot_index: 0 }
    );
    assert_eq!(committed.placements[0].item_version, 2);
    persistence.close().await;

    let restarted = disposable_database().await;
    let restarted_service = PostgresSafeInventoryService::new(restarted.clone());
    let replay = restarted_service
        .transfer_frame(ACCOUNT_ID, &frame)
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.result_hash, committed.result_hash);

    let mut changed = frame;
    changed.payload.source_slot_index = 1;
    changed.payload_hash = changed.payload.canonical_hash();
    assert!(matches!(
        restarted_service.transfer_frame(ACCOUNT_ID, &changed).await,
        Err(SafeInventoryServiceError::IdempotencyConflict)
    ));
    assert_committed_state(&restarted).await;
    restarted.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn concurrent_claims_for_final_vault_slot_have_one_winner() {
    let persistence = disposable_database().await;
    seed_fixture(&persistence).await;
    seed_final_vault_slot_race(&persistence).await;
    let service = PostgresSafeInventoryService::new(persistence.clone());
    let first_payload = SafeInventoryTransferPayloadV1 {
        kind: SafeInventoryTransferKindV1::CharacterSafeToVault,
        source_slot_index: 0,
        expected_account_version: 1,
        expected_inventory_version: 1,
    };
    let second_payload = SafeInventoryTransferPayloadV1 {
        source_slot_index: 1,
        ..first_payload
    };
    let first = SafeInventoryTransferFrameV1 {
        mutation_id: [216; 16],
        character_id: CHARACTER_ID,
        issued_at_unix_millis: 1,
        payload_hash: first_payload.canonical_hash(),
        payload: first_payload,
    };
    let second = SafeInventoryTransferFrameV1 {
        mutation_id: [217; 16],
        character_id: CHARACTER_ID,
        issued_at_unix_millis: 2,
        payload_hash: second_payload.canonical_hash(),
        payload: second_payload,
    };
    let (left, right) = tokio::join!(
        service.transfer_frame(ACCOUNT_ID, &first),
        service.transfer_frame(ACCOUNT_ID, &second),
    );
    assert!(matches!(
        (&left, &right),
        (Ok(_), Err(SafeInventoryServiceError::StaleVersion))
            | (Err(SafeInventoryServiceError::StaleVersion), Ok(_))
    ));

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let counts: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM item_instances WHERE namespace_id=$1 AND account_id=$2 \
         AND location_kind=6),(SELECT count(*) FROM item_instances WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3 AND location_kind=5), \
         (SELECT count(*) FROM item_ledger_events WHERE namespace_id=$1 AND account_id=$2 \
         AND mutation_id IN ($4,$5)),(SELECT count(*) FROM safe_inventory_mutations \
         WHERE namespace_id=$1 AND account_id=$2 AND mutation_id IN ($4,$5))",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind([216_u8; 16].as_slice())
    .bind([217_u8; 16].as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(counts, (160, 1, 1, 1));
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn injected_ledger_failure_rolls_back_item_versions_and_receipt() {
    let persistence = disposable_database().await;
    seed_fixture(&persistence).await;
    let mut setup = persistence.begin_transaction().await.unwrap();
    sqlx::query("DROP TRIGGER IF EXISTS safe_inventory_test_failure ON item_ledger_events")
        .execute(setup.connection())
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION IF EXISTS gravebound_test_fail_safe_inventory_ledger()")
        .execute(setup.connection())
        .await
        .unwrap();
    sqlx::query(
        "CREATE FUNCTION gravebound_test_fail_safe_inventory_ledger() RETURNS trigger \
         LANGUAGE plpgsql AS $$ BEGIN IF NEW.mutation_id = decode(repeat('dc',16),'hex') THEN \
         RAISE EXCEPTION 'injected safe inventory ledger failure'; END IF; RETURN NEW; END; $$",
    )
    .execute(setup.connection())
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER safe_inventory_test_failure BEFORE INSERT ON item_ledger_events \
         FOR EACH ROW EXECUTE FUNCTION gravebound_test_fail_safe_inventory_ledger()",
    )
    .execute(setup.connection())
    .await
    .unwrap();
    setup.commit().await.unwrap();

    let payload = SafeInventoryTransferPayloadV1 {
        kind: SafeInventoryTransferKindV1::CharacterSafeToVault,
        source_slot_index: 0,
        expected_account_version: 1,
        expected_inventory_version: 1,
    };
    let frame = SafeInventoryTransferFrameV1 {
        mutation_id: [220; 16],
        character_id: CHARACTER_ID,
        issued_at_unix_millis: 1,
        payload_hash: payload.canonical_hash(),
        payload,
    };
    assert!(matches!(
        PostgresSafeInventoryService::new(persistence.clone())
            .transfer_frame(ACCOUNT_ID, &frame)
            .await,
        Err(SafeInventoryServiceError::Persistence)
    ));

    let mut verify = persistence.begin_transaction().await.unwrap();
    sqlx::query("DROP TRIGGER safe_inventory_test_failure ON item_ledger_events")
        .execute(verify.connection())
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION gravebound_test_fail_safe_inventory_ledger()")
        .execute(verify.connection())
        .await
        .unwrap();
    let state: (Option<Vec<u8>>, i16, i16, i64, i64, i64) = sqlx::query_as(
        "SELECT x.character_id,x.location_kind,x.slot_index,x.item_version,a.state_version, \
         i.inventory_version FROM item_instances x JOIN accounts a USING(namespace_id,account_id) \
         JOIN character_inventories i USING(namespace_id,account_id,character_id) \
         WHERE x.namespace_id=$1 AND x.account_id=$2 AND x.item_uid=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(ITEM_UID.as_slice())
    .fetch_one(verify.connection())
    .await
    .unwrap();
    let durable_rows: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM item_ledger_events WHERE namespace_id=$1 \
         AND account_id=$2 AND mutation_id=$3) + (SELECT count(*) FROM safe_inventory_mutations \
         WHERE namespace_id=$1 AND account_id=$2 AND mutation_id=$3)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind([220_u8; 16].as_slice())
    .fetch_one(verify.connection())
    .await
    .unwrap();
    verify.commit().await.unwrap();
    assert_eq!(state, (Some(CHARACTER_ID.to_vec()), 5, 0, 1, 1, 1));
    assert_eq!(durable_rows, 0);
}

async fn assert_committed_state(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let item: (Option<Vec<u8>>, i16, i16, i64) = sqlx::query_as(
        "SELECT character_id,security_state,location_kind,item_version FROM item_instances \
         WHERE namespace_id=$1 AND account_id=$2 AND item_uid=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(ITEM_UID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let ledger_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM item_ledger_events WHERE namespace_id=$1 AND account_id=$2 \
         AND item_uid=$3 AND mutation_id=$4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(ITEM_UID.as_slice())
    .bind(MUTATION_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(item, (None, 0, 6, 2));
    assert_eq!(ledger_count, 1);
}
