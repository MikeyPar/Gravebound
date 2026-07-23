use persistence::{
    CORE_ITEM_CONTENT_REVISION, CORE_WORLD_ASSETS_BLAKE3, CORE_WORLD_LOCALIZATION_BLAKE3,
    CORE_WORLD_RECORDS_BLAKE3, DangerCheckpointDelete, DangerCheckpointWrite,
    EXPECTED_SCHEMA_VERSION, LifeClockCheckpointCommandV1, LifeClockCheckpointRequestV1,
    LifeClockCheckpointTransactionV1, LifeClockContentAuthorityV1, LifeClockDangerAuthorityV1,
    LifeClockStateV1, LiveDamageTraceCauseV1, LiveDamageTraceContentAuthorityV1,
    LiveDamageTraceDamageTypeV1, LiveDamageTraceDangerAuthorityV1, LiveDamageTraceEntryV1,
    LiveDamageTraceHeadV1, LiveDamageTraceNetworkStateV1, LiveDamageTraceRecallStateV1,
    LiveDamageTraceStatusV1, LiveDamageTraceTickCommandV1, LiveDamageTraceTickRequestV1,
    LiveDamageTraceTickTransactionV1, PersistenceConfig, PersistenceError, PersistenceTransaction,
    PostgresPersistence, StoredCharacter, StoredDangerCheckpoint, StoredMutation,
    StoredSafeInventoryCommand, StoredSafeInventoryCommandKind, StoredSafeInventoryLocation,
    StoredSafeInventoryPlacement, StoredWorldFlowRevisionV1, WIPEABLE_CORE_NAMESPACE,
};
const ACCOUNT_A: [u8; 16] = [1; 16];
const ACCOUNT_B: [u8; 16] = [2; 16];
const ACCOUNT_ROLLBACK: [u8; 16] = [3; 16];
const CHARACTER_A: [u8; 16] = [11; 16];
const CHARACTER_B: [u8; 16] = [12; 16];
const FOREIGN_CHARACTER: [u8; 16] = [21; 16];
const CHECKPOINT_LINEAGE: [u8; 16] = [71; 16];
const CHECKPOINT_RESTORE: [u8; 16] = [72; 16];
const EXPECTED_PUBLIC_TABLES: &[&str] = &[
    "_sqlx_migrations",
    "account_boss_first_clears",
    "account_material_ledger_events_v1",
    "account_material_wallet_balances_v1",
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
    "character_extraction_terminal_results_v1",
    "character_instance_lineages",
    "character_inventories",
    "character_life_clock_checkpoint_receipts_v1",
    "character_life_clock_conflict_audits_v1",
    "character_life_deed_completion_receipts_v1",
    "character_life_deed_completion_receipts_v2",
    "character_life_deed_conflict_audits_v2",
    "character_life_deed_revocations_v2",
    "character_life_deeds",
    "character_life_metrics",
    "character_life_outbox",
    "character_live_damage_trace_conflict_audits_v1",
    "character_live_damage_trace_entries_v1",
    "character_live_damage_trace_ingest_receipts_v1",
    "character_live_damage_trace_statuses_v1",
    "character_live_damage_trace_ticks_v1",
    "character_oath_bargain_state",
    "character_oath_mutation_results",
    "character_private_route_generation_heads_v1",
    "character_progression",
    "character_recall_terminal_results_v1",
    "character_run_material_stacks",
    "character_world_locations",
    "character_world_transfer_results",
    "character_xp_award_results",
    "characters",
    "core_consumable_use_receipts_v1",
    "core_telemetry_sessions_v1",
    "crash_outbox_events_v1",
    "currency_ledger_events",
    "danger_crash_restore_ash_changes",
    "danger_crash_restore_bargain_changes",
    "danger_crash_restore_conflict_audits",
    "danger_crash_restore_item_changes",
    "danger_crash_restore_material_changes",
    "danger_crash_restore_request_results",
    "danger_crash_restore_results",
    "death_audit_events",
    "death_combat_trace_entries",
    "death_combat_trace_statuses",
    "death_destruction_entries",
    "death_events",
    "death_live_trace_entry_provenance_v1",
    "death_live_trace_promotion_conflict_audits_v1",
    "death_live_trace_receipt_links_v1",
    "death_live_trace_sets_v1",
    "death_mutation_results",
    "death_outbox_events",
    "death_successor_presets_v1",
    "death_summary_bargains",
    "death_summary_damage_entries",
    "death_summary_projection_entries",
    "death_summary_snapshots",
    "echo_bargain_snapshots",
    "echo_deed_tags",
    "echo_records",
    "echo_state_transitions",
    "entry_restore_active_bargains_v2",
    "entry_restore_active_bargains_v3",
    "entry_restore_ash_wallet_v3",
    "entry_restore_inventory_items_v1",
    "entry_restore_inventory_items_v3",
    "entry_restore_inventory_v1",
    "entry_restore_inventory_v3",
    "entry_restore_life_metrics_v2",
    "entry_restore_life_metrics_v3",
    "entry_restore_oath_bargain_v2",
    "entry_restore_oath_bargain_v3",
    "entry_restore_progression_v1",
    "entry_restore_progression_v3",
    "extraction_terminal_audit_events_v1",
    "extraction_terminal_conflict_audits_v1",
    "extraction_terminal_item_placements_v1",
    "extraction_terminal_material_credits_v1",
    "extraction_terminal_outbox_events_v1",
    "field_equipment_mutations",
    "gravebound_namespaces",
    "item_instances",
    "item_ledger_events",
    "item_ledger_telemetry_outbox_v1",
    "memorial_records",
    "onboarding_outbox_events_v1",
    "private_route_generation_allocations_v1",
    "production_extraction_intent_acceptances_v1",
    "production_extraction_intent_conflict_audits_v1",
    "recall_terminal_audit_events_v1",
    "recall_terminal_conflict_audits_v1",
    "recall_terminal_item_destructions_v1",
    "recall_terminal_item_stabilizations_v1",
    "recall_terminal_material_destructions_v1",
    "recall_terminal_outbox_events_v1",
    "resolution_hold_item_transitions_v1",
    "resolution_hold_mutation_audit_events_v1",
    "resolution_hold_mutation_conflict_audits_v1",
    "resolution_hold_mutation_outbox_events_v1",
    "resolution_hold_mutation_results_v1",
    "reward_requests",
    "reward_result_entries",
    "safe_inventory_mutations",
    "safe_inventory_placements",
    "session_outbox_events_v1",
    "starter_initializer_results",
    "successor_creation_receipts_v1",
    "successor_mutation_audit_events_v1",
    "successor_mutation_conflict_audits_v1",
    "successor_mutation_outbox_events_v1",
    "successor_mutation_results_v1",
    "successor_roster_reservations_v1",
    "support_character_lookup_v1",
    "support_character_transition_lookup_v1",
    "support_death_lookup_v1",
    "support_death_transition_lookup_v1",
    "support_item_lookup_v1",
    "support_item_transition_lookup_v1",
    "support_lookup_audit_events_v1",
];

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
    assert_eq!(tables, EXPECTED_PUBLIC_TABLES);
    for prohibited in ["vault", "currency_ledger", "purchases"] {
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
async fn safe_inventory_transfer_is_atomic_replay_safe_and_restart_durable() {
    const ACCOUNT: [u8; 16] = [230; 16];
    const CHARACTER: [u8; 16] = [231; 16];
    const ITEM: [u8; 16] = [232; 16];
    let persistence = disposable_database().await;
    seed_safe_inventory_fixture(&persistence, ACCOUNT, CHARACTER, ITEM).await;

    let snapshot = persistence
        .load_safe_inventory_snapshot(ACCOUNT, CHARACTER)
        .await
        .unwrap();
    assert_eq!(
        (snapshot.account_version, snapshot.inventory_version),
        (1, 1)
    );
    assert_eq!(snapshot.character_safe[0].item_uid, ITEM);
    let command = StoredSafeInventoryCommand {
        mutation_id: [233; 16],
        canonical_request_hash: [234; 32],
        result_hash: [235; 32],
        kind: StoredSafeInventoryCommandKind::CharacterSafeToVault,
        source_slot_index: 0,
        expected_account_version: 1,
        expected_inventory_version: 1,
        placements: vec![StoredSafeInventoryPlacement {
            item_uid: ITEM,
            source: StoredSafeInventoryLocation::CharacterSafe(0),
            destination: StoredSafeInventoryLocation::Vault(0),
            expected_item_version: 1,
        }],
    };
    let committed = persistence
        .commit_safe_inventory_transfer(ACCOUNT, CHARACTER, &command)
        .await
        .unwrap();
    assert!(!committed.replayed);
    assert_eq!(
        (
            committed.post_account_version,
            committed.post_inventory_version
        ),
        (2, 2)
    );
    persistence.close().await;

    let restarted = disposable_database().await;
    let replay = restarted
        .commit_safe_inventory_transfer(ACCOUNT, CHARACTER, &command)
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.result_hash, committed.result_hash);
    let mut changed = command.clone();
    changed.canonical_request_hash[0] ^= 1;
    assert!(matches!(
        restarted
            .commit_safe_inventory_transfer(ACCOUNT, CHARACTER, &changed)
            .await,
        Err(PersistenceError::SafeInventoryIdempotencyConflict)
    ));
    assert_safe_inventory_commit(&restarted, ACCOUNT, CHARACTER, ITEM).await;
    restarted.close().await;
}

async fn seed_safe_inventory_fixture(
    persistence: &PostgresPersistence,
    account_id: [u8; 16],
    character_id: [u8; 16],
    item_uid: [u8; 16],
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query("DELETE FROM accounts WHERE namespace_id = $1 AND account_id = $2")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
    insert_account(&mut transaction, &account_id).await.unwrap();
    insert_character(&mut transaction, &account_id, &character_id, 1)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE accounts SET selected_character_id = $1 WHERE namespace_id = $2 AND account_id = $3",
    )
    .bind(character_id.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_inventories (namespace_id,account_id,character_id,inventory_version) \
         VALUES ($1,$2,$3,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_world_locations (namespace_id,account_id,character_id, \
         character_version,location_kind,location_content_id,safe_arrival_kind) \
         VALUES ($1,$2,$3,1,1,'hub.lantern_halls_01',0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
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
    .bind(item_uid.as_slice())
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(CORE_ITEM_CONTENT_REVISION)
    .bind([236_u8; 16].as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn assert_safe_inventory_commit(
    persistence: &PostgresPersistence,
    account_id: [u8; 16],
    character_id: [u8; 16],
    item_uid: [u8; 16],
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let item: (Option<Vec<u8>>, i16, i16, i64) = sqlx::query_as(
        "SELECT character_id,security_state,location_kind,item_version FROM item_instances \
         WHERE namespace_id = $1 AND account_id = $2 AND item_uid = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(item_uid.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let versions: (i64, i64) = sqlx::query_as(
        "SELECT a.state_version,i.inventory_version FROM accounts a JOIN character_inventories i \
         ON i.namespace_id=a.namespace_id AND i.account_id=a.account_id \
         WHERE a.namespace_id=$1 AND a.account_id=$2 AND i.character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let ledgers: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM item_ledger_events WHERE namespace_id=$1 AND account_id=$2 \
         AND item_uid=$3 AND mutation_id=$4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(item_uid.as_slice())
    .bind([233_u8; 16].as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(item, (None, 0, 6, 2));
    assert_eq!(versions, (2, 2));
    assert_eq!(ledgers, 1);
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
    let ash_wallet: (i32, i64) = sqlx::query_as(
        "SELECT balance, wallet_version FROM ash_wallets \
         WHERE namespace_id = $1 AND account_id = $2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(initial_location, (1, 0));
    assert_eq!(ash_wallet, (0, 1));
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

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
#[allow(
    clippy::too_many_lines,
    reason = "one linear hosted fixture proves all clock states, replay, restart, races, and bounds"
)]
async fn authoritative_life_clocks_are_exact_replayable_and_restart_safe() {
    const SAFE_ACCOUNT: [u8; 16] = [81; 16];
    const SAFE_CHARACTER: [u8; 16] = [82; 16];

    let persistence = disposable_database().await;
    clear_accounts(&persistence).await;
    let mut transaction = persistence.begin_transaction().await.unwrap();
    insert_account(&mut transaction, &SAFE_ACCOUNT)
        .await
        .unwrap();
    insert_character(&mut transaction, &SAFE_ACCOUNT, &SAFE_CHARACTER, 1)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO character_world_locations (namespace_id,account_id,character_id, \
         character_version,location_kind,safe_arrival_kind) VALUES ($1,$2,$3,1,0,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(SAFE_ACCOUNT.as_slice())
    .bind(SAFE_CHARACTER.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE accounts SET selected_character_id=$1 WHERE namespace_id=$2 AND account_id=$3",
    )
    .bind(SAFE_CHARACTER.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(SAFE_ACCOUNT.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();

    let select = clock_request(
        SAFE_ACCOUNT,
        SAFE_CHARACTER,
        [83; 16],
        1,
        1,
        30,
        30,
        LifeClockStateV1::CharacterSelect,
        None,
    );
    let loading = clock_request(
        SAFE_ACCOUNT,
        SAFE_CHARACTER,
        [84; 16],
        1,
        2,
        60,
        30,
        LifeClockStateV1::Loading,
        None,
    );
    let offline = clock_request(
        SAFE_ACCOUNT,
        SAFE_CHARACTER,
        [85; 16],
        1,
        3,
        90,
        30,
        LifeClockStateV1::Offline,
        None,
    );
    for request in [&select, &loading, &offline] {
        assert!(matches!(
            persistence
                .transact_life_clock_checkpoint_v1(request)
                .await
                .unwrap(),
            LifeClockCheckpointTransactionV1::Committed(_)
        ));
    }
    let excluded_head = persistence
        .load_life_clock_head_v1(SAFE_ACCOUNT, SAFE_CHARACTER)
        .await
        .unwrap();
    assert_eq!(excluded_head.lifetime_ticks, 0);
    assert_eq!(excluded_head.permadeath_combat_ticks, 0);
    assert_eq!(excluded_head.life_metrics_version, 4);
    assert_eq!(excluded_head.authoritative_tick, 90);

    assert!(matches!(
        persistence
            .transact_life_clock_checkpoint_v1(&offline)
            .await
            .unwrap(),
        LifeClockCheckpointTransactionV1::Replayed(_)
    ));
    let mut changed_offline = offline.command.clone();
    changed_offline.issued_at_unix_ms += 1;
    let changed_offline = LifeClockCheckpointRequestV1::seal(changed_offline).unwrap();
    assert!(matches!(
        persistence
            .transact_life_clock_checkpoint_v1(&changed_offline)
            .await,
        Err(PersistenceError::LifeClockIdempotencyConflict)
    ));
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let conflict_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM character_life_clock_conflict_audits_v1 \
         WHERE namespace_id=$1 AND account_id=$2 AND checkpoint_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(SAFE_ACCOUNT.as_slice())
    .bind([85_u8; 16].as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(conflict_count, 1);

    let stale_character = clock_request(
        SAFE_ACCOUNT,
        SAFE_CHARACTER,
        [86; 16],
        2,
        4,
        120,
        30,
        LifeClockStateV1::Loading,
        None,
    );
    assert!(matches!(
        persistence
            .transact_life_clock_checkpoint_v1(&stale_character)
            .await,
        Err(PersistenceError::LifeClockCharacterVersionMismatch {
            expected: 2,
            actual: 1
        })
    ));

    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "UPDATE character_world_locations SET location_kind=1, \
         location_content_id='hub.lantern_halls_01',safe_arrival_kind=0 \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(SAFE_ACCOUNT.as_slice())
    .bind(SAFE_CHARACTER.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let hall = clock_request(
        SAFE_ACCOUNT,
        SAFE_CHARACTER,
        [87; 16],
        1,
        4,
        120,
        30,
        LifeClockStateV1::HallControllable,
        None,
    );
    persistence
        .transact_life_clock_checkpoint_v1(&hall)
        .await
        .unwrap();
    let concurrent_a = clock_request(
        SAFE_ACCOUNT,
        SAFE_CHARACTER,
        [88; 16],
        1,
        5,
        150,
        30,
        LifeClockStateV1::HallControllable,
        None,
    );
    let concurrent_b = clock_request(
        SAFE_ACCOUNT,
        SAFE_CHARACTER,
        [89; 16],
        1,
        5,
        150,
        30,
        LifeClockStateV1::HallControllable,
        None,
    );
    let (result_a, result_b) = tokio::join!(
        persistence.transact_life_clock_checkpoint_v1(&concurrent_a),
        persistence.transact_life_clock_checkpoint_v1(&concurrent_b)
    );
    assert_eq!(
        usize::from(result_a.is_ok()) + usize::from(result_b.is_ok()),
        1
    );
    for result in [result_a, result_b] {
        assert!(
            result.is_ok()
                || matches!(
                    result,
                    Err(PersistenceError::LifeClockMetricsVersionMismatch {
                        expected: 5,
                        actual: 6
                    })
                )
        );
    }
    let safe_head = persistence
        .load_life_clock_head_v1(SAFE_ACCOUNT, SAFE_CHARACTER)
        .await
        .unwrap();
    assert_eq!(safe_head.lifetime_ticks, 60);
    assert_eq!(safe_head.permadeath_combat_ticks, 0);
    assert_eq!(safe_head.life_metrics_version, 6);
    assert_eq!(safe_head.authoritative_tick, 150);
    assert_eq!(safe_head.link_lost_ticks, 0);
    assert!(safe_head.danger.is_none());

    persistence.close().await;
    let persistence = disposable_database().await;
    assert_eq!(
        persistence
            .load_life_clock_head_v1(SAFE_ACCOUNT, SAFE_CHARACTER)
            .await
            .unwrap(),
        safe_head
    );

    clear_accounts(&persistence).await;
    insert_danger_checkpoint_fixture(&persistence).await;
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "UPDATE accounts SET selected_character_id=$1 WHERE namespace_id=$2 AND account_id=$3",
    )
    .bind(CHARACTER_A.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let fresh_danger_head = persistence
        .load_life_clock_head_v1(ACCOUNT_A, CHARACTER_A)
        .await
        .unwrap();
    assert_eq!(fresh_danger_head.authoritative_tick, 0);
    assert!(fresh_danger_head.danger.is_some());
    persistence
        .write_danger_checkpoint(&danger_checkpoint(1, vec![1]))
        .await
        .unwrap();
    let danger = LifeClockDangerAuthorityV1 {
        lineage_id: CHECKPOINT_LINEAGE,
        restore_point_id: CHECKPOINT_RESTORE,
        entry_life_metrics_version: 1,
        entry_permadeath_combat_ticks: 0,
    };
    let danger_requests = [
        clock_request(
            ACCOUNT_A,
            CHARACTER_A,
            [90; 16],
            2,
            1,
            30,
            30,
            LifeClockStateV1::DangerLoading,
            Some(danger.clone()),
        ),
        clock_request(
            ACCOUNT_A,
            CHARACTER_A,
            [91; 16],
            2,
            2,
            60,
            30,
            LifeClockStateV1::DangerStaging,
            Some(danger.clone()),
        ),
        clock_request(
            ACCOUNT_A,
            CHARACTER_A,
            [92; 16],
            2,
            3,
            90,
            30,
            LifeClockStateV1::DangerControllable,
            Some(danger.clone()),
        ),
        clock_request(
            ACCOUNT_A,
            CHARACTER_A,
            [93; 16],
            2,
            4,
            179,
            89,
            LifeClockStateV1::DangerLinkLost,
            Some(danger.clone()),
        ),
        clock_request(
            ACCOUNT_A,
            CHARACTER_A,
            [94; 16],
            2,
            5,
            180,
            1,
            LifeClockStateV1::DangerControllable,
            Some(danger.clone()),
        ),
        clock_request(
            ACCOUNT_A,
            CHARACTER_A,
            [95; 16],
            2,
            6,
            270,
            90,
            LifeClockStateV1::DangerLinkLost,
            Some(danger.clone()),
        ),
    ];
    for request in &danger_requests {
        persistence
            .transact_life_clock_checkpoint_v1(request)
            .await
            .unwrap();
    }
    assert!(matches!(
        persistence
            .transact_life_clock_checkpoint_v1(&clock_request(
                ACCOUNT_A,
                CHARACTER_A,
                [96; 16],
                2,
                7,
                271,
                1,
                LifeClockStateV1::DangerLinkLost,
                Some(danger.clone()),
            ))
            .await,
        Err(PersistenceError::LifeClockTerminalResolutionRequired)
    ));
    assert!(matches!(
        persistence
            .transact_life_clock_checkpoint_v1(&clock_request(
                ACCOUNT_A,
                CHARACTER_A,
                [97; 16],
                2,
                7,
                271,
                1,
                LifeClockStateV1::DangerControllable,
                Some(danger.clone()),
            ))
            .await,
        Err(PersistenceError::LifeClockTerminalResolutionRequired)
    ));
    assert!(matches!(
        persistence
            .transact_life_clock_checkpoint_v1(&danger_requests[0])
            .await
            .unwrap(),
        LifeClockCheckpointTransactionV1::Replayed(_)
    ));
    let danger_head = persistence
        .load_life_clock_head_v1(ACCOUNT_A, CHARACTER_A)
        .await
        .unwrap();
    assert_eq!(danger_head.lifetime_ticks, 210);
    assert_eq!(danger_head.permadeath_combat_ticks, 270);
    assert_eq!(danger_head.life_metrics_version, 7);
    assert_eq!(danger_head.authoritative_tick, 270);
    assert_eq!(danger_head.link_lost_ticks, 90);
    assert_eq!(danger_head.danger, Some(danger));

    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "UPDATE character_life_metrics SET lifetime_ticks=211 WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .bind(CHARACTER_A.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    assert!(matches!(
        persistence
            .load_life_clock_head_v1(ACCOUNT_A, CHARACTER_A)
            .await,
        Err(PersistenceError::CorruptStoredLifeClock)
    ));
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "UPDATE character_life_metrics SET lifetime_ticks=210 WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .bind(CHARACTER_A.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();

    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "DELETE FROM character_danger_checkpoints WHERE namespace_id=$1 AND account_id=$2 \
         AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .bind(CHARACTER_A.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    assert_eq!(
        persistence
            .load_life_clock_head_v1(ACCOUNT_A, CHARACTER_A)
            .await
            .unwrap(),
        danger_head
    );
    persistence
        .write_danger_checkpoint(&danger_checkpoint(1, vec![1]))
        .await
        .unwrap();
    assert_eq!(
        persistence
            .load_life_clock_head_v1(ACCOUNT_A, CHARACTER_A)
            .await
            .unwrap(),
        danger_head
    );

    persistence.close().await;
    let persistence = disposable_database().await;
    assert_eq!(
        persistence
            .load_life_clock_head_v1(ACCOUNT_A, CHARACTER_A)
            .await
            .unwrap(),
        danger_head
    );
    persistence.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
#[allow(
    clippy::too_many_lines,
    reason = "one linear hosted fixture proves retained replay, pruning, restart, terminal, and corruption gates"
)]
async fn retained_live_trace_accepts_precheckpoint_damage_then_replays_after_pruning_and_restart() {
    let persistence = disposable_database().await;
    clear_accounts(&persistence).await;
    insert_danger_checkpoint_fixture(&persistence).await;
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "UPDATE accounts SET selected_character_id=$1 WHERE namespace_id=$2 AND account_id=$3",
    )
    .bind(CHARACTER_A.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let pre_checkpoint = persistence
        .load_live_damage_trace_snapshot_v1(ACCOUNT_A, CHARACTER_A)
        .await
        .unwrap();
    assert_eq!(pre_checkpoint.danger.checkpoint_tick, 0);
    assert_eq!(pre_checkpoint.through_tick, 0);

    let empty = persistence
        .load_live_damage_trace_snapshot_v1(ACCOUNT_A, CHARACTER_A)
        .await
        .unwrap();
    assert_eq!(empty.character_version, 2);
    assert_eq!(empty.through_tick, 0);
    assert!(empty.head.is_none());
    assert!(empty.entries.is_empty());

    let first = trace_request([101; 16], 100, 0, 120, 108, None);
    let first_stored = match persistence
        .transact_live_damage_trace_tick_v1(&first)
        .await
        .unwrap()
    {
        LiveDamageTraceTickTransactionV1::Committed(stored) => stored,
        LiveDamageTraceTickTransactionV1::Replayed(_) => panic!("first tick unexpectedly replayed"),
    };
    let first_head = first_stored.head();
    assert_eq!(
        persistence
            .load_live_damage_trace_snapshot_v1(ACCOUNT_A, CHARACTER_A)
            .await
            .unwrap()
            .entries
            .len(),
        1
    );
    assert!(matches!(
        persistence
            .transact_live_damage_trace_tick_v1(&first)
            .await
            .unwrap(),
        LiveDamageTraceTickTransactionV1::Replayed(_)
    ));
    let mut changed = first.command.clone();
    changed.entries[0].statuses[0].remaining_ticks += 1;
    let changed = LiveDamageTraceTickRequestV1::seal(changed).unwrap();
    assert!(matches!(
        persistence
            .transact_live_damage_trace_tick_v1(&changed)
            .await,
        Err(PersistenceError::LiveDamageTraceIdempotencyConflict)
    ));

    assert!(matches!(
        persistence
            .transact_live_damage_trace_tick_v1(&trace_request([105; 16], 101, 0, 108, 107, None,))
            .await,
        Err(PersistenceError::LiveDamageTracePredecessorMismatch)
    ));
    let second = trace_request([102; 16], 401, 0, 108, 96, Some(first_head));
    let interleaved = trace_request(
        [106; 16],
        401,
        0,
        108,
        96,
        second.command.expected_previous.clone(),
    );
    let (second_result, interleaved_result) = tokio::join!(
        persistence.transact_live_damage_trace_tick_v1(&second),
        persistence.transact_live_damage_trace_tick_v1(&interleaved),
    );
    let second_stored = match (second_result, interleaved_result) {
        (
            Ok(LiveDamageTraceTickTransactionV1::Committed(stored)),
            Err(PersistenceError::LiveDamageTracePredecessorMismatch),
        )
        | (
            Err(PersistenceError::LiveDamageTracePredecessorMismatch),
            Ok(LiveDamageTraceTickTransactionV1::Committed(stored)),
        ) => stored,
        results => panic!("interleaved predecessor CAS was not single-winner: {results:?}"),
    };
    let second_head = second_stored.head();
    assert!(matches!(
        persistence
            .transact_live_damage_trace_tick_v1(&first)
            .await
            .unwrap(),
        LiveDamageTraceTickTransactionV1::Replayed(_)
    ));
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let (receipts, payloads, conflicts): (i64, i64, i64) = sqlx::query_as(
        "SELECT \
          (SELECT count(*) FROM character_live_damage_trace_ingest_receipts_v1 WHERE account_id=$1), \
          (SELECT count(*) FROM character_live_damage_trace_ticks_v1 WHERE account_id=$1), \
          (SELECT count(*) FROM character_live_damage_trace_conflict_audits_v1 WHERE account_id=$1)",
    )
    .bind(ACCOUNT_A.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!((receipts, payloads, conflicts), (2, 1, 1));

    let mut lethal = trace_request([103; 16], 402, 0, 96, 0, Some(second_head.clone())).command;
    lethal.entries[0].final_damage = 96;
    lethal.entries[0].lethal = true;
    let lethal = LiveDamageTraceTickRequestV1::seal(lethal).unwrap();
    assert!(matches!(
        persistence
            .transact_live_damage_trace_tick_v1(&lethal)
            .await,
        Err(PersistenceError::LiveDamageTraceTerminalStagingRequired)
    ));
    persistence.close().await;

    let persistence = disposable_database().await;
    let snapshot = persistence
        .load_live_damage_trace_snapshot_v1(ACCOUNT_A, CHARACTER_A)
        .await
        .unwrap();
    assert_eq!(snapshot.through_tick, 401);
    assert_eq!(snapshot.character_version, 2);
    assert_eq!(snapshot.head, Some(second_head.clone()));
    assert_eq!(snapshot.entries[0].entry.source_sim_entity_id, Some(42));
    persistence
        .transact_live_damage_trace_tick_v1(&trace_request(
            [104; 16],
            450,
            0,
            96,
            84,
            Some(second_head.clone()),
        ))
        .await
        .unwrap();
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "DELETE FROM character_live_damage_trace_ticks_v1 \
         WHERE namespace_id=$1 AND account_id=$2 AND trace_tick_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .bind(second_head.trace_tick_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    assert!(matches!(
        persistence
            .load_live_damage_trace_snapshot_v1(ACCOUNT_A, CHARACTER_A)
            .await,
        Err(PersistenceError::CorruptStoredLiveDamageTrace)
    ));
    persistence.close().await;
}

fn trace_request(
    trace_tick_id: [u8; 16],
    event_tick: u64,
    checkpoint_tick: u64,
    pre_health: u32,
    post_health: u32,
    expected_previous: Option<LiveDamageTraceHeadV1>,
) -> LiveDamageTraceTickRequestV1 {
    LiveDamageTraceTickRequestV1::seal(LiveDamageTraceTickCommandV1 {
        account_id: ACCOUNT_A,
        character_id: CHARACTER_A,
        trace_tick_id,
        expected_character_version: 2,
        expected_previous,
        event_tick,
        danger: LiveDamageTraceDangerAuthorityV1 {
            lineage_id: CHECKPOINT_LINEAGE,
            restore_point_id: CHECKPOINT_RESTORE,
            checkpoint_tick,
        },
        content: LiveDamageTraceContentAuthorityV1::core(),
        entries: vec![LiveDamageTraceEntryV1 {
            event_ordinal: 0,
            cause: LiveDamageTraceCauseV1::DirectHit,
            source_content_id: "enemy.bell_reed".to_owned(),
            source_entity_id: Some([103; 16]),
            source_sim_entity_id: Some(42),
            pattern_id: Some("pattern.enemy.bell_reed.gap_ring".to_owned()),
            attack_id: "pattern.enemy.bell_reed.gap_ring".to_owned(),
            raw_damage: pre_health - post_health,
            final_damage: pre_health - post_health,
            damage_type: LiveDamageTraceDamageTypeV1::Veil,
            pre_health,
            post_health,
            source_x_milli_tiles: 2_000,
            source_y_milli_tiles: -1_000,
            network_state: LiveDamageTraceNetworkStateV1::Connected,
            recall_state: LiveDamageTraceRecallStateV1::Inactive,
            lethal: post_health == 0,
            statuses: vec![LiveDamageTraceStatusV1 {
                status_ordinal: 0,
                status_id: "status.bleed".to_owned(),
                remaining_ticks: 30,
                stack_count: 1,
            }],
        }],
        issued_at_unix_ms: 1,
    })
    .unwrap()
}

#[allow(clippy::too_many_arguments)]
fn clock_request(
    account_id: [u8; 16],
    character_id: [u8; 16],
    checkpoint_id: [u8; 16],
    expected_character_version: u64,
    expected_life_metrics_version: u64,
    authoritative_tick: u64,
    advanced_ticks: u32,
    state: LifeClockStateV1,
    danger: Option<LifeClockDangerAuthorityV1>,
) -> LifeClockCheckpointRequestV1 {
    LifeClockCheckpointRequestV1::seal(LifeClockCheckpointCommandV1 {
        account_id,
        character_id,
        checkpoint_id,
        expected_character_version,
        expected_life_metrics_version,
        authoritative_tick,
        state,
        advanced_ticks,
        danger,
        content: LifeClockContentAuthorityV1::core(),
        issued_at_unix_ms: 1,
    })
    .unwrap()
}

fn danger_checkpoint(tick: i64, payload: Vec<u8>) -> StoredDangerCheckpoint {
    StoredDangerCheckpoint {
        account_id: ACCOUNT_A,
        character_id: CHARACTER_A,
        lineage_id: CHECKPOINT_LINEAGE,
        checkpoint_tick: tick,
        content_revision: StoredWorldFlowRevisionV1 {
            records_blake3: CORE_WORLD_RECORDS_BLAKE3.to_owned(),
            assets_blake3: CORE_WORLD_ASSETS_BLAKE3.to_owned(),
            localization_blake3: CORE_WORLD_LOCALIZATION_BLAKE3.to_owned(),
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

#[allow(
    clippy::too_many_lines,
    reason = "the hosted fixture constructs one auditable component-complete danger graph"
)]
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
        "INSERT INTO ash_wallets (namespace_id, account_id, balance, wallet_version) \
         VALUES ($1, $2, 0, 1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
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
    .bind(CORE_WORLD_RECORDS_BLAKE3)
    .bind(CORE_WORLD_ASSETS_BLAKE3)
    .bind(CORE_WORLD_LOCALIZATION_BLAKE3)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_entry_restore_points (namespace_id, account_id, character_id, \
         restore_point_id, lineage_id, source_location_id, restore_location_id, \
         snapshot_contract_version, account_version, character_version, progression_version, \
         inventory_version, oath_bargain_version, life_metrics_version, ash_wallet_version, \
         component_mask, composite_digest, restore_state, \
         records_blake3, assets_blake3, localization_blake3) VALUES ($1, $2, $3, $4, $5, \
         'hub.lantern_halls_01', 'hub.lantern_halls_01', 3, 1, 1, 1, 1, 1, 1, 1, 31, \
         $6, 0, $7, $8, $9)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .bind(CHARACTER_A.as_slice())
    .bind(CHECKPOINT_RESTORE.as_slice())
    .bind(CHECKPOINT_LINEAGE.as_slice())
    .bind([91_u8; 32].as_slice())
    .bind(CORE_WORLD_RECORDS_BLAKE3)
    .bind(CORE_WORLD_ASSETS_BLAKE3)
    .bind(CORE_WORLD_LOCALIZATION_BLAKE3)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO entry_restore_progression_v3 \
         (namespace_id,account_id,character_id,restore_point_id,level,total_xp,current_health, \
          progression_version,component_digest) VALUES ($1,$2,$3,$4,1,0,120,1,$5)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .bind(CHARACTER_A.as_slice())
    .bind(CHECKPOINT_RESTORE.as_slice())
    .bind([92_u8; 32].as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO entry_restore_progression_v1 \
         (namespace_id,account_id,character_id,restore_point_id,level,total_xp,current_health, \
          progression_version) VALUES ($1,$2,$3,$4,1,0,120,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .bind(CHARACTER_A.as_slice())
    .bind(CHECKPOINT_RESTORE.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    persistence::stage_danger_entry_inventory_restore_v3(
        &mut transaction,
        ACCOUNT_A,
        CHARACTER_A,
        CHECKPOINT_RESTORE,
        [89_u8; 16],
        0,
    )
    .await
    .unwrap();
    persistence::stage_danger_entry_oath_bargain_restore_v3(
        &mut transaction,
        ACCOUNT_A,
        CHARACTER_A,
        CHECKPOINT_RESTORE,
    )
    .await
    .unwrap();
    persistence::stage_danger_entry_life_metrics_restore_v3(
        &mut transaction,
        ACCOUNT_A,
        CHARACTER_A,
        CHECKPOINT_RESTORE,
    )
    .await
    .unwrap();
    persistence::stage_danger_entry_ash_wallet_restore_v3(
        &mut transaction,
        ACCOUNT_A,
        CHARACTER_A,
        CHECKPOINT_RESTORE,
    )
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
    let result = sqlx::query(
        "UPDATE accounts SET selected_character_id = $1 \
         WHERE namespace_id = $2 AND account_id = $3",
    )
    .bind(FOREIGN_CHARACTER.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_A.as_slice())
    .execute(transaction.connection())
    .await;
    assert!(result.is_err());
    transaction.rollback().await.unwrap();
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
