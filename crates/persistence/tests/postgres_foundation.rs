use persistence::{
    DangerCheckpointDelete, DangerCheckpointWrite, EXPECTED_SCHEMA_VERSION, PersistenceConfig,
    PersistenceError, PersistenceTransaction, PostgresPersistence, StoredCharacter,
    StoredDangerCheckpoint, StoredMutation, StoredWorldFlowRevisionV1, WIPEABLE_CORE_NAMESPACE,
};
const ACCOUNT_A: [u8; 16] = [1; 16];
const ACCOUNT_B: [u8; 16] = [2; 16];
const ACCOUNT_ROLLBACK: [u8; 16] = [3; 16];
const CHARACTER_A: [u8; 16] = [11; 16];
const CHARACTER_B: [u8; 16] = [12; 16];
const FOREIGN_CHARACTER: [u8; 16] = [21; 16];
const CHECKPOINT_LINEAGE: [u8; 16] = [71; 16];
const CHECKPOINT_RESTORE: [u8; 16] = [72; 16];

async fn disposable_database() -> PostgresPersistence {
    let config = PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL");
    let persistence = PostgresPersistence::connect(&config).await.unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence.migrate().await.unwrap();
    persistence
}

async fn clear_accounts(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query("DELETE FROM accounts WHERE namespace_id = $1")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .execute(transaction.connection())
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn insert_account(
    transaction: &mut PersistenceTransaction<'_>,
    account_id: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO accounts \
         (namespace_id, account_id, state_version, slot_capacity) VALUES ($1, $2, 1, 2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id)
    .execute(transaction.connection())
    .await?;
    Ok(())
}

async fn insert_character(
    transaction: &mut PersistenceTransaction<'_>,
    account_id: &[u8],
    character_id: &[u8],
    ordinal: i16,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO characters \
         (namespace_id, account_id, character_id, roster_ordinal, class_id, level, \
          oath_id, life_state, security_state) \
         VALUES ($1, $2, $3, $4, 'class.grave_arbalist', 1, NULL, 0, 0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id)
    .bind(character_id)
    .bind(ordinal)
    .execute(transaction.connection())
    .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn migrations_are_idempotent_exact_and_ready() {
    let persistence = disposable_database().await;

    persistence.migrate().await.unwrap();
    let readiness = persistence.readiness().await.unwrap();
    assert_eq!(readiness.schema_version, EXPECTED_SCHEMA_VERSION);
    assert_eq!(readiness.namespace, WIPEABLE_CORE_NAMESPACE);
    assert!(readiness.wipeable);

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = 'public' ORDER BY table_name",
    )
    .fetch_all(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(
        tables,
        [
            "_sqlx_migrations",
            "account_boss_first_clears",
            "account_mutation_results",
            "accounts",
            "ash_mutation_results",
            "ash_wallets",
            "bargain_decision_results",
            "bargain_milestone_results",
            "bargain_offer_candidates",
            "bargain_offers",
            "caldus_victory_exit_owners",
            "caldus_victory_exits",
            "character_active_bargains",
            "character_danger_checkpoints",
            "character_entry_restore_points",
            "character_extraction_results",
            "character_instance_lineages",
            "character_inventories",
            "character_life_outbox",
            "character_oath_bargain_state",
            "character_oath_mutation_results",
            "character_progression",
            "character_world_locations",
            "character_world_transfer_results",
            "character_xp_award_results",
            "characters",
            "currency_ledger_events",
            "entry_restore_progression_v1",
            "field_equipment_mutations",
            "gravebound_namespaces",
            "item_instances",
            "item_ledger_events",
            "reward_requests",
            "reward_result_entries",
            "safe_inventory_mutations",
            "safe_inventory_placements",
            "starter_initializer_results",
        ]
    );
    for prohibited in ["vault", "memorials", "currency_ledger"] {
        assert!(!tables.iter().any(|table| table == prohibited));
    }
    persistence.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
#[allow(
    clippy::too_many_lines,
    reason = "the linear PostgreSQL fixture keeps schema custody assertions reviewable"
)]
async fn safe_storage_locations_preserve_legacy_shapes_and_enforce_custody() {
    const ACCOUNT: [u8; 16] = [240; 16];
    const CHARACTER: [u8; 16] = [241; 16];
    let persistence = disposable_database().await;

    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query("DELETE FROM accounts WHERE namespace_id = $1 AND account_id = $2")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ACCOUNT.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
    insert_account(&mut transaction, &ACCOUNT).await.unwrap();
    insert_character(&mut transaction, &ACCOUNT, &CHARACTER, 1)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO character_inventories (namespace_id, account_id, character_id, \
         inventory_version) VALUES ($1, $2, $3, 1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT.as_slice())
    .bind(CHARACTER.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    for (item_uid, creation_request_id, location_kind, slot_index) in [
        ([242_u8; 16], [243_u8; 16], 0_i16, 0_i16),
        ([244_u8; 16], [245_u8; 16], 5_i16, 7_i16),
    ] {
        sqlx::query(
            "INSERT INTO item_instances (namespace_id,item_uid,account_id,character_id, \
             template_id,content_revision,item_kind,item_level,rarity,creation_kind, \
             creation_request_id,roll_index,unit_ordinal,item_version,security_state, \
             location_kind,slot_index,provenance_kind,salvage_band,salvage_value) \
             VALUES ($1,$2,$3,$4,'item.weapon.crossbow.pine_crossbow',$5,0,1,0,0, \
             $6,0,0,1,$7,$8,$9,0,0,0)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(item_uid.as_slice())
        .bind(ACCOUNT.as_slice())
        .bind(CHARACTER.as_slice())
        .bind(format!("core-dev.blake3.{}", "a".repeat(64)))
        .bind(creation_request_id.as_slice())
        .bind(i16::from(location_kind == 0))
        .bind(location_kind)
        .bind(slot_index)
        .execute(transaction.connection())
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO item_instances (namespace_id,item_uid,account_id,character_id, \
         template_id,content_revision,item_kind,item_level,rarity,creation_kind, \
         creation_request_id,roll_index,unit_ordinal,item_version,security_state, \
         location_kind,slot_index,provenance_kind,salvage_band,salvage_value) \
         VALUES ($1,$2,$3,NULL,'item.weapon.crossbow.pine_crossbow',$4,0,1,0,0, \
         $5,0,0,1,0,6,159,0,0,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind([246_u8; 16].as_slice())
    .bind(ACCOUNT.as_slice())
    .bind(format!("core-dev.blake3.{}", "b".repeat(64)))
    .bind([247_u8; 16].as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO item_ledger_events (namespace_id,ledger_event_id,item_uid,account_id, \
         character_id,mutation_id,event_kind,source_kind,pre_item_version,post_item_version, \
         post_security_state,post_location_kind) VALUES ($1,$2,$3,$4,$5,$6,0,0,0,1,0,5)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind([248_u8; 16].as_slice())
    .bind([244_u8; 16].as_slice())
    .bind(ACCOUNT.as_slice())
    .bind(CHARACTER.as_slice())
    .bind([249_u8; 16].as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();

    let mut invalid_vault = persistence.begin_transaction().await.unwrap();
    let result = sqlx::query(
        "UPDATE item_instances SET location_kind = 6, security_state = 0 \
         WHERE namespace_id = $1 AND item_uid = $2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind([244_u8; 16].as_slice())
    .execute(invalid_vault.connection())
    .await;
    assert!(result.is_err(), "Vault custody retained a character");
    invalid_vault.rollback().await.unwrap();

    let mut invalid_safe = persistence.begin_transaction().await.unwrap();
    let result = sqlx::query(
        "UPDATE item_instances SET location_kind = 5, slot_index = 0 \
         WHERE namespace_id = $1 AND item_uid = $2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind([246_u8; 16].as_slice())
    .execute(invalid_safe.connection())
    .await;
    assert!(
        result.is_err(),
        "CharacterSafe custody accepted no character"
    );
    invalid_safe.rollback().await.unwrap();

    let mut cleanup = persistence.begin_transaction().await.unwrap();
    sqlx::query("DELETE FROM accounts WHERE namespace_id = $1 AND account_id = $2")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ACCOUNT.as_slice())
        .execute(cleanup.connection())
        .await
        .unwrap();
    cleanup.commit().await.unwrap();
    persistence.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn identity_schema_enforces_transactions_ownership_and_bounds() {
    let persistence = disposable_database().await;
    clear_accounts(&persistence).await;
    insert_valid_fixture(&persistence).await;
    assert_bound_rejections(&persistence).await;
    assert_selected_character_ownership(&persistence).await;
    assert_rollback_and_cascade(&persistence).await;
    persistence.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn typed_identity_store_round_trips_under_one_lock() {
    let persistence = disposable_database().await;
    clear_accounts(&persistence).await;
    let written = persistence
        .transact_identity(ACCOUNT_A, 1, 2, |aggregate| {
            assert_eq!((aggregate.state_version, aggregate.slot_capacity), (1, 2));
            aggregate.state_version = 2;
            aggregate.characters.push(StoredCharacter {
                character_id: CHARACTER_A,
                roster_ordinal: 1,
                class_id: "class.grave_arbalist".to_owned(),
                level: 1,
                oath_id: None,
                life_state: 0,
                security_state: 0,
                character_state_version: 1,
            });
            aggregate.mutations.push(StoredMutation {
                mutation_id: [31; 16],
                payload_hash: [41; 32],
                result_payload: vec![51; 32],
            });
            aggregate.selected_character_id = Some(CHARACTER_A);
            Ok(aggregate.clone())
        })
        .await
        .unwrap();
    let loaded = persistence
        .transact_identity(ACCOUNT_A, 1, 2, |aggregate| Ok(aggregate.clone()))
        .await
        .unwrap();
    assert_eq!(loaded, written);
    assert_eq!(
        persistence
            .identity_character_owner(CHARACTER_A)
            .await
            .unwrap(),
        Some(ACCOUNT_A)
    );
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let initial_location: (i64, i16) = sqlx::query_as(
        "SELECT character_version, location_kind FROM character_world_locations \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .bind(CHARACTER_A.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(initial_location, (1, 0));
    persistence.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
#[allow(clippy::too_many_lines)] // Keep the pre/post-safe durable ordering visible end to end.
async fn danger_checkpoints_are_version_bound_replay_safe_and_monotonic() {
    let persistence = disposable_database().await;
    clear_accounts(&persistence).await;
    insert_danger_checkpoint_fixture(&persistence).await;

    let first = danger_checkpoint(900, vec![1, 2, 3]);
    assert_eq!(
        persistence.write_danger_checkpoint(&first).await.unwrap(),
        DangerCheckpointWrite::Created
    );
    assert_eq!(
        persistence.write_danger_checkpoint(&first).await.unwrap(),
        DangerCheckpointWrite::Replayed
    );

    let changed_same_tick = danger_checkpoint(900, vec![4, 5, 6]);
    assert!(matches!(
        persistence
            .write_danger_checkpoint(&changed_same_tick)
            .await,
        Err(PersistenceError::DangerCheckpointReplayConflict)
    ));

    let later = danger_checkpoint(930, vec![7, 8, 9]);
    assert_eq!(
        persistence.write_danger_checkpoint(&later).await.unwrap(),
        DangerCheckpointWrite::Advanced
    );
    assert!(matches!(
        persistence.write_danger_checkpoint(&first).await,
        Err(PersistenceError::StaleDangerCheckpoint)
    ));
    assert_eq!(
        persistence
            .danger_checkpoint(ACCOUNT_A, CHARACTER_A)
            .await
            .unwrap(),
        Some(later.clone())
    );

    let lower_concurrent = danger_checkpoint(960, vec![10]);
    let higher_concurrent = danger_checkpoint(990, vec![11]);
    let (lower_result, higher_result) = tokio::join!(
        persistence.write_danger_checkpoint(&lower_concurrent),
        persistence.write_danger_checkpoint(&higher_concurrent)
    );
    assert!(matches!(
        lower_result,
        Ok(DangerCheckpointWrite::Advanced) | Err(PersistenceError::StaleDangerCheckpoint)
    ));
    assert_eq!(higher_result.unwrap(), DangerCheckpointWrite::Advanced);
    assert_eq!(
        persistence
            .danger_checkpoint(ACCOUNT_A, CHARACTER_A)
            .await
            .unwrap(),
        Some(higher_concurrent)
    );
    assert!(matches!(
        persistence
            .delete_danger_checkpoint_after_safe_transfer(
                ACCOUNT_A,
                CHARACTER_A,
                CHECKPOINT_LINEAGE,
            )
            .await,
        Err(PersistenceError::DangerCheckpointFinalizationNotCommitted)
    ));
    assert!(
        persistence
            .danger_checkpoint(ACCOUNT_A, CHARACTER_A)
            .await
            .unwrap()
            .is_some()
    );

    let mut stale_version = danger_checkpoint(1_020, vec![12]);
    stale_version.inventory_version += 1;
    assert!(matches!(
        persistence.write_danger_checkpoint(&stale_version).await,
        Err(PersistenceError::StaleDangerCheckpoint)
    ));

    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "UPDATE character_danger_checkpoints SET checkpoint_payload = $1 \
         WHERE namespace_id = $2 AND account_id = $3 AND character_id = $4",
    )
    .bind([99_u8].as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .bind(CHARACTER_A.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    assert!(matches!(
        persistence.danger_checkpoint(ACCOUNT_A, CHARACTER_A).await,
        Err(PersistenceError::CorruptStoredDangerCheckpoint)
    ));

    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "UPDATE character_danger_checkpoints SET checkpoint_payload = $1 \
         WHERE namespace_id = $2 AND account_id = $3 AND character_id = $4",
    )
    .bind([11_u8].as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .bind(CHARACTER_A.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_world_locations SET location_kind = 1, \
         location_content_id = 'hub.lantern_halls_01', safe_arrival_kind = 0, \
         instance_lineage_id = NULL, entry_restore_point_id = NULL WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .bind(CHARACTER_A.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_instance_lineages SET lineage_state = 2, \
         closed_at = transaction_timestamp() WHERE namespace_id = $1 AND lineage_id = $2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(CHECKPOINT_LINEAGE.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    assert_eq!(
        persistence
            .delete_danger_checkpoint_after_safe_transfer(
                ACCOUNT_A,
                CHARACTER_A,
                CHECKPOINT_LINEAGE,
            )
            .await
            .unwrap(),
        DangerCheckpointDelete::Deleted
    );
    assert_eq!(
        persistence
            .delete_danger_checkpoint_after_safe_transfer(
                ACCOUNT_A,
                CHARACTER_A,
                CHECKPOINT_LINEAGE,
            )
            .await
            .unwrap(),
        DangerCheckpointDelete::Absent
    );
    persistence.close().await;
}

fn danger_checkpoint(tick: i64, payload: Vec<u8>) -> StoredDangerCheckpoint {
    StoredDangerCheckpoint {
        account_id: ACCOUNT_A,
        character_id: CHARACTER_A,
        lineage_id: CHECKPOINT_LINEAGE,
        checkpoint_tick: tick,
        content_revision: StoredWorldFlowRevisionV1 {
            records_blake3: "1".repeat(64),
            assets_blake3: "2".repeat(64),
            localization_blake3: "3".repeat(64),
        },
        composite_digest: [81; 32],
        character_version: 2,
        progression_version: 1,
        inventory_version: 1,
        oath_bargain_version: 1,
        checkpoint_schema_version: 1,
        checkpoint_payload_digest: *blake3::hash(&payload).as_bytes(),
        checkpoint_payload: payload,
    }
}

async fn insert_danger_checkpoint_fixture(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    insert_account(&mut transaction, &ACCOUNT_A).await.unwrap();
    insert_character(&mut transaction, &ACCOUNT_A, &CHARACTER_A, 1)
        .await
        .unwrap();
    for statement in [
        "INSERT INTO character_progression (namespace_id, account_id, character_id, total_xp, \
         level, current_health, progression_version) VALUES ($1, $2, $3, 0, 1, 120, 1)",
        "INSERT INTO character_inventories (namespace_id, account_id, character_id, \
         inventory_version) VALUES ($1, $2, $3, 1)",
        "INSERT INTO character_oath_bargain_state (namespace_id, account_id, character_id, \
         earned_bargain_slots, oath_bargain_version) VALUES ($1, $2, $3, 0, 1)",
    ] {
        sqlx::query(statement)
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(ACCOUNT_A.as_slice())
            .bind(CHARACTER_A.as_slice())
            .execute(transaction.connection())
            .await
            .unwrap();
    }
    sqlx::query(
        "INSERT INTO character_instance_lineages (namespace_id, account_id, character_id, \
         lineage_id, content_id, layout_id, lineage_state, records_blake3, assets_blake3, \
         localization_blake3) VALUES ($1, $2, $3, $4, 'world.core_microrealm_01', \
         'layout.core_private_life_01', 0, $5, $6, $7)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .bind(CHARACTER_A.as_slice())
    .bind(CHECKPOINT_LINEAGE.as_slice())
    .bind("1".repeat(64))
    .bind("2".repeat(64))
    .bind("3".repeat(64))
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_entry_restore_points (namespace_id, account_id, character_id, \
         restore_point_id, lineage_id, source_location_id, restore_location_id, \
         snapshot_contract_version, account_version, character_version, progression_version, \
         inventory_version, oath_bargain_version, component_mask, composite_digest, restore_state, \
         records_blake3, assets_blake3, localization_blake3) VALUES ($1, $2, $3, $4, $5, \
         'hub.lantern_halls_01', 'hub.lantern_halls_01', 1, 1, 1, 1, 1, 1, 7, $6, 0, $7, $8, $9)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .bind(CHARACTER_A.as_slice())
    .bind(CHECKPOINT_RESTORE.as_slice())
    .bind(CHECKPOINT_LINEAGE.as_slice())
    .bind([91_u8; 32].as_slice())
    .bind("1".repeat(64))
    .bind("2".repeat(64))
    .bind("3".repeat(64))
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_world_locations (namespace_id, account_id, character_id, \
         character_version, location_kind, location_content_id, instance_lineage_id, \
         entry_restore_point_id) VALUES ($1, $2, $3, 2, 2, 'world.core_microrealm_01', $4, $5)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .bind(CHARACTER_A.as_slice())
    .bind(CHECKPOINT_LINEAGE.as_slice())
    .bind(CHECKPOINT_RESTORE.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE characters SET character_state_version = 2 WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .bind(CHARACTER_A.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn insert_valid_fixture(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    insert_account(&mut transaction, &ACCOUNT_A).await.unwrap();
    insert_account(&mut transaction, &ACCOUNT_B).await.unwrap();
    insert_character(&mut transaction, &ACCOUNT_A, &CHARACTER_A, 1)
        .await
        .unwrap();
    insert_character(&mut transaction, &ACCOUNT_A, &CHARACTER_B, 2)
        .await
        .unwrap();
    insert_character(&mut transaction, &ACCOUNT_B, &FOREIGN_CHARACTER, 1)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO account_mutation_results \
         (namespace_id, account_id, mutation_id, payload_hash, result_payload) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .bind([31_u8; 16].as_slice())
    .bind([41_u8; 32].as_slice())
    .bind([51_u8; 32].as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE accounts SET selected_character_id = $1 \
         WHERE namespace_id = $2 AND account_id = $3",
    )
    .bind(CHARACTER_A.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn assert_bound_rejections(persistence: &PostgresPersistence) {
    let mut invalid_account = persistence.begin_transaction().await.unwrap();
    assert!(
        insert_account(&mut invalid_account, &[1_u8; 15])
            .await
            .is_err()
    );
    invalid_account.rollback().await.unwrap();

    let mut duplicate_slot = persistence.begin_transaction().await.unwrap();
    assert!(
        insert_character(&mut duplicate_slot, &ACCOUNT_A, &[13; 16], 2)
            .await
            .is_err()
    );
    duplicate_slot.rollback().await.unwrap();

    let mut invalid_mutation = persistence.begin_transaction().await.unwrap();
    let result = sqlx::query(
        "INSERT INTO account_mutation_results \
         (namespace_id, account_id, mutation_id, payload_hash, result_payload) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .bind([61_u8; 16].as_slice())
    .bind([71_u8; 31].as_slice())
    .bind(Vec::<u8>::new())
    .execute(invalid_mutation.connection())
    .await;
    assert!(result.is_err());
    invalid_mutation.rollback().await.unwrap();
}

async fn assert_selected_character_ownership(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "UPDATE accounts SET selected_character_id = $1 \
         WHERE namespace_id = $2 AND account_id = $3",
    )
    .bind(FOREIGN_CHARACTER.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    assert!(transaction.commit().await.is_err());
}

async fn assert_rollback_and_cascade(persistence: &PostgresPersistence) {
    let mut rolled_back = persistence.begin_transaction().await.unwrap();
    insert_account(&mut rolled_back, &ACCOUNT_ROLLBACK)
        .await
        .unwrap();
    rolled_back.rollback().await.unwrap();

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM accounts WHERE namespace_id = $1 AND account_id = $2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ROLLBACK.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(count, 0);
    sqlx::query("DELETE FROM accounts WHERE namespace_id = $1 AND account_id = $2")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ACCOUNT_A.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
    transaction.commit().await.unwrap();

    let mut verification = persistence.begin_transaction().await.unwrap();
    let characters: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM characters WHERE namespace_id = $1 AND account_id = $2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    let mutations: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM account_mutation_results \
         WHERE namespace_id = $1 AND account_id = $2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    verification.rollback().await.unwrap();
    assert_eq!((characters, mutations), (0, 0));
}
