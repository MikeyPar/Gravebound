use std::path::{Path, PathBuf};

use persistence::{PersistenceConfig, PostgresPersistence, WIPEABLE_CORE_NAMESPACE};
use protocol::ManifestHash;
use server_app::{
    AccountId, AuthenticatedAccount, AuthenticatedNamespace, CrashRestoreContext,
    EntryCaptureContext, EntryRestoreProvider, PostgresProgressionAwardService,
    PostgresProgressionRestoreProvider, ProgressionAwardCode, ProgressionAwardCommand,
    ProgressionAwardEvidence, ProgressionAwardPayload, RestorePointError,
};
use sim_core::{
    EncounterXpEvidence, NormalXpEvidence, RewardLifeState, RewardRecallState, RewardTrustState,
};

const ACCOUNT_ID: [u8; 16] = [61; 16];
const CHARACTER_ID: [u8; 16] = [62; 16];
const FIRST_RESTORE_ID: [u8; 16] = [63; 16];
const FIRST_LINEAGE_ID: [u8; 16] = [64; 16];
const SECOND_RESTORE_ID: [u8; 16] = [65; 16];
const SECOND_LINEAGE_ID: [u8; 16] = [66; 16];

fn content_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
}

fn authenticated() -> AuthenticatedAccount {
    AuthenticatedAccount {
        account_id: AccountId::new(ACCOUNT_ID).unwrap(),
        namespace: AuthenticatedNamespace::WipeableTest,
    }
}

async fn disposable_database() -> PostgresPersistence {
    let config = PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL");
    let persistence = PostgresPersistence::connect(&config).await.unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence.migrate().await.unwrap();
    persistence
}

fn progression_revision() -> ManifestHash {
    let content = sim_content::load_core_development_progression(&content_root()).unwrap();
    ManifestHash::new(content.hashes().records_blake3.clone()).unwrap()
}

fn ordinary_award(event: u8, expected_version: u64) -> ProgressionAwardCommand {
    let payload = ProgressionAwardPayload {
        character_id: CHARACTER_ID,
        expected_progression_version: expected_version,
        source_content_id: "enemy.drowned_pilgrim".to_owned(),
        progression_content_revision: progression_revision(),
        evidence: ProgressionAwardEvidence::Ordinary(NormalXpEvidence {
            living_at_enemy_death: true,
            delta_x_milli_tiles: 1_000,
            delta_y_milli_tiles: 0,
            contribution_window_ticks: 300,
            actual_health_damage_to_enemy: 1,
            effective_support_to_qualifying_player: false,
        }),
    };
    ProgressionAwardCommand {
        reward_event_id: [event; 16],
        payload_hash: payload.canonical_hash(),
        payload,
    }
}

fn caldus_award(expected_version: u64) -> ProgressionAwardCommand {
    let payload = ProgressionAwardPayload {
        character_id: CHARACTER_ID,
        expected_progression_version: expected_version,
        source_content_id: "boss.sir_caldus".to_owned(),
        progression_content_revision: progression_revision(),
        evidence: ProgressionAwardEvidence::Encounter(EncounterXpEvidence {
            active_ticks: 600,
            present_ticks: 600,
            longest_inactivity_ticks: 0,
            encounter_contribution_reference_health: 4_200,
            direct_damage: 420,
            effective_healing_to_others: 0,
            damage_prevented_on_others: 0,
            qualifying_objective_credits: 0,
            life_state: RewardLifeState::Living,
            recall_state: RewardRecallState::Eligible,
            trust_state: RewardTrustState::Valid,
        }),
    };
    ProgressionAwardCommand {
        reward_event_id: [72; 16],
        payload_hash: payload.canonical_hash(),
        payload,
    }
}

async fn reset_fixture(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
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
         VALUES ($1, $2, $3, 1, 'class.grave_arbalist', 1, NULL, 0, 0, 1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE accounts SET selected_character_id = $1 WHERE namespace_id = $2 AND account_id = $3",
    )
    .bind(CHARACTER_ID.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_world_locations (namespace_id, account_id, character_id, \
         character_version, location_kind, location_content_id, safe_arrival_kind) \
         VALUES ($1, $2, $3, 1, 1, 'hub.lantern_halls_01', 0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_progression (namespace_id, account_id, character_id, total_xp, \
         level, current_health, progression_version) VALUES ($1, $2, $3, 0, 1, 120, 1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_inventories (namespace_id, account_id, character_id, \
         inventory_version) VALUES ($1, $2, $3, 1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_oath_bargain_state (namespace_id, account_id, character_id, \
         earned_bargain_slots, oath_bargain_version) VALUES ($1, $2, $3, 0, 1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ash_wallets (namespace_id, account_id, balance, wallet_version) \
         VALUES ($1, $2, 0, 1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

#[allow(
    clippy::too_many_lines,
    reason = "the hosted fixture keeps the complete V3 entry graph and location transition auditable"
)]
async fn begin_danger_entry(
    persistence: &PostgresPersistence,
    provider: &PostgresProgressionRestoreProvider,
    lineage_id: [u8; 16],
    restore_id: [u8; 16],
    character_version: i64,
    account_version: i64,
    progression_version: i64,
) {
    let world = sim_content::load_core_development_world_flow(&content_root()).unwrap();
    let hashes = world.hashes();
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO character_instance_lineages (namespace_id, account_id, character_id, \
         lineage_id, content_id, lineage_state, records_blake3, assets_blake3, \
         localization_blake3) VALUES ($1, $2, $3, $4, 'world.core_microrealm_01', 0, $5, $6, $7)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(lineage_id.as_slice())
    .bind(&hashes.records_blake3)
    .bind(&hashes.assets_blake3)
    .bind(&hashes.localization_blake3)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_entry_restore_points (namespace_id, account_id, character_id, \
         restore_point_id, lineage_id, source_location_id, restore_location_id, \
         snapshot_contract_version, account_version, character_version, progression_version, \
         inventory_version, oath_bargain_version, life_metrics_version, ash_wallet_version, \
         component_mask, composite_digest, \
         restore_state, records_blake3, assets_blake3, localization_blake3) \
         VALUES ($1, $2, $3, $4, $5, 'hub.lantern_halls_01', 'hub.lantern_halls_01', \
         3, $6, $7, $8, 1, 1, 1, 1, 31, $9, 0, $10, $11, $12)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(restore_id.as_slice())
    .bind(lineage_id.as_slice())
    .bind(account_version)
    .bind(character_version)
    .bind(progression_version)
    .bind([91_u8; 32].as_slice())
    .bind(&hashes.records_blake3)
    .bind(&hashes.assets_blake3)
    .bind(&hashes.localization_blake3)
    .execute(transaction.connection())
    .await
    .unwrap();
    let snapshot = provider
        .capture(
            &mut transaction,
            EntryCaptureContext {
                account_id: ACCOUNT_ID,
                character_id: CHARACTER_ID,
                restore_point_id: restore_id,
                mutation_id: [9; 16],
                safe_placement_count: 0,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        snapshot.progression_version,
        u64::try_from(progression_version).unwrap()
    );
    persistence::stage_danger_entry_inventory_restore_v3(
        &mut transaction,
        ACCOUNT_ID,
        CHARACTER_ID,
        restore_id,
        [9; 16],
        0,
    )
    .await
    .unwrap();
    persistence::stage_danger_entry_oath_bargain_restore_v3(
        &mut transaction,
        ACCOUNT_ID,
        CHARACTER_ID,
        restore_id,
    )
    .await
    .unwrap();
    persistence::stage_danger_entry_life_metrics_restore_v3(
        &mut transaction,
        ACCOUNT_ID,
        CHARACTER_ID,
        restore_id,
    )
    .await
    .unwrap();
    persistence::stage_danger_entry_ash_wallet_restore_v3(
        &mut transaction,
        ACCOUNT_ID,
        CHARACTER_ID,
        restore_id,
    )
    .await
    .unwrap();
    let next_character_version = character_version + 1;
    sqlx::query(
        "UPDATE characters SET character_state_version = $1 WHERE namespace_id = $2 \
         AND account_id = $3 AND character_id = $4",
    )
    .bind(next_character_version)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_world_locations SET character_version = $1, location_kind = 2, \
         location_content_id = 'world.core_microrealm_01', safe_arrival_kind = NULL, \
         instance_lineage_id = $2, entry_restore_point_id = $3 WHERE namespace_id = $4 \
         AND account_id = $5 AND character_id = $6",
    )
    .bind(next_character_version)
    .bind(lineage_id.as_slice())
    .bind(restore_id.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn finalize_to_hall(
    persistence: &PostgresPersistence,
    lineage_id: [u8; 16],
    restore_id: [u8; 16],
    restore_state: i16,
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let current_version: i64 = sqlx::query_scalar(
        "SELECT character_state_version FROM characters WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let next_version = current_version + 1;
    sqlx::query(
        "UPDATE character_entry_restore_points SET restore_state = $1, \
         consumed_at = transaction_timestamp() WHERE namespace_id = $2 AND restore_point_id = $3",
    )
    .bind(restore_state)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(restore_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_instance_lineages SET lineage_state = 2, closed_at = transaction_timestamp() \
         WHERE namespace_id = $1 AND lineage_id = $2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(lineage_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_world_locations SET character_version = $1, location_kind = 1, \
         location_content_id = 'hub.lantern_halls_01', safe_arrival_kind = 0, \
         instance_lineage_id = NULL, entry_restore_point_id = NULL WHERE namespace_id = $2 \
         AND account_id = $3 AND character_id = $4",
    )
    .bind(next_version)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE characters SET character_state_version = $1 WHERE namespace_id = $2 \
         AND account_id = $3 AND character_id = $4",
    )
    .bind(next_version)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn assert_first_restore_rows(persistence: &PostgresPersistence) {
    let mut verification = persistence.begin_transaction().await.unwrap();
    let progression: (i32, i16, i32, i64) = sqlx::query_as(
        "SELECT total_xp, level, current_health, progression_version FROM character_progression \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    let revoked_receipts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM character_xp_award_results WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3 AND entry_restore_point_id = $4 \
         AND revoked_by_restore_point_id = $4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(FIRST_RESTORE_ID.as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    let first_clears: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM account_boss_first_clears WHERE namespace_id = $1 AND account_id = $2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    verification.rollback().await.unwrap();
    assert_eq!(progression, (0, 1, 120, 4));
    assert_eq!(revoked_receipts, 2);
    assert_eq!(first_clears, 0);
}

async fn award_safe_and_assert_unbound(
    persistence: &PostgresPersistence,
    awards: &PostgresProgressionAwardService,
) {
    finalize_to_hall(persistence, FIRST_LINEAGE_ID, FIRST_RESTORE_ID, 1).await;
    let safe = ordinary_award(73, 4);
    assert_eq!(
        awards.award(authenticated(), &safe).await.code,
        ProgressionAwardCode::Accepted
    );
    let mut verification = persistence.begin_transaction().await.unwrap();
    let safe_binding: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT entry_restore_point_id FROM character_xp_award_results WHERE namespace_id = $1 \
         AND account_id = $2 AND reward_event_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(safe.reward_event_id.as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    verification.rollback().await.unwrap();
    assert!(safe_binding.is_none());
}

async fn assert_final_resolution_wins(
    persistence: &PostgresPersistence,
    restores: &PostgresProgressionRestoreProvider,
) {
    begin_danger_entry(
        persistence,
        restores,
        SECOND_LINEAGE_ID,
        SECOND_RESTORE_ID,
        3,
        1,
        5,
    )
    .await;
    // DeathCommitted (2) is now valid only with the complete durable death graph. This fixture
    // needs any final outcome to prove crash restore loses, so use ExtractionCommitted (3).
    finalize_to_hall(persistence, SECOND_LINEAGE_ID, SECOND_RESTORE_ID, 3).await;
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let result = restores
        .restore_and_revoke_post_entry(
            &mut transaction,
            CrashRestoreContext {
                account_id: ACCOUNT_ID,
                character_id: CHARACTER_ID,
                restore_point_id: SECOND_RESTORE_ID,
            },
        )
        .await;
    assert!(matches!(result, Err(RestorePointError::RestoreSuperseded)));
    transaction.rollback().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn danger_xp_restore_is_exact_replay_safe_and_final_resolution_aware() {
    let persistence = disposable_database().await;
    reset_fixture(&persistence).await;
    let progression_content =
        sim_content::load_core_development_progression(&content_root()).unwrap();
    let oath_bargain_content =
        sim_content::load_core_development_oaths_bargains(&content_root()).unwrap();
    let awards = PostgresProgressionAwardService::new(
        persistence.clone(),
        &progression_content,
        &oath_bargain_content,
    )
    .unwrap();
    let restores = PostgresProgressionRestoreProvider::new(&progression_content).unwrap();

    begin_danger_entry(
        &persistence,
        &restores,
        FIRST_LINEAGE_ID,
        FIRST_RESTORE_ID,
        1,
        1,
        1,
    )
    .await;
    let ordinary = ordinary_award(71, 1);
    let (first, replay) = tokio::join!(
        awards.award(authenticated(), &ordinary),
        awards.award(authenticated(), &ordinary)
    );
    assert_eq!(first, replay);
    assert_eq!(first.code, ProgressionAwardCode::Accepted);
    let caldus = caldus_award(2);
    let boss = awards.award(authenticated(), &caldus).await;
    assert_eq!(boss.code, ProgressionAwardCode::Accepted);
    assert!(boss.first_clear_awarded);

    let mut transaction = persistence.begin_transaction().await.unwrap();
    restores
        .restore_and_revoke_post_entry(
            &mut transaction,
            CrashRestoreContext {
                account_id: ACCOUNT_ID,
                character_id: CHARACTER_ID,
                restore_point_id: FIRST_RESTORE_ID,
            },
        )
        .await
        .unwrap();
    transaction.commit().await.unwrap();

    let revoked = awards.award(authenticated(), &ordinary).await;
    assert_eq!(revoked.code, ProgressionAwardCode::RevokedByCrashRestore);
    assert!(revoked.projection.is_none());
    let mut conflict = ordinary.clone();
    conflict.payload.source_content_id = "enemy.root_thrall".to_owned();
    conflict.payload_hash = conflict.payload.canonical_hash();
    assert_eq!(
        awards.award(authenticated(), &conflict).await.code,
        ProgressionAwardCode::IdempotencyConflict
    );

    assert_first_restore_rows(&persistence).await;
    award_safe_and_assert_unbound(&persistence, &awards).await;
    assert_final_resolution_wins(&persistence, &restores).await;
    persistence.close().await;
}
