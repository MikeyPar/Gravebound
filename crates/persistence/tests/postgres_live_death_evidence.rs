//! Hosted `PostgreSQL` proof for reward-qualified live-deed persistence (`GB-M03-06B`).
//!
//! Authorities:
//! - canonical GDD `ECH-001`, `TECH-021`, and `TECH-023`;
//! - Content Production Spec Core Sepulcher Knight and Sir Caldus reward/XP bindings;
//! - Development Roadmap `GB-M03-06`/`13` replay, restart, and atomicity gates.
//!
//! Run only against the guarded disposable database. The fixture deliberately uses normalized,
//! durable reward/progression rows rather than caller assertions so it exercises the production
//! repository boundary and its deferred database graph.

use persistence::{
    CORE_ITEM_CONTENT_REVISION, CORE_PROGRESSION_RECORDS_BLAKE3, CORE_WORLD_ASSETS_BLAKE3,
    CORE_WORLD_LOCALIZATION_BLAKE3, CORE_WORLD_RECORDS_BLAKE3, LifeDeedCompletionCommandV2,
    LifeDeedCompletionRequestV2, LifeDeedCompletionTransactionV2, LifeDeedContentAuthorityV2,
    LifeDeedKindV2, LifeDeedProjectionOutcomeV2, PersistenceConfig, PersistenceError,
    PostgresPersistence, WIPEABLE_CORE_NAMESPACE, stage_danger_entry_ash_wallet_restore_v3,
    stage_danger_entry_inventory_restore_v3, stage_danger_entry_life_metrics_restore_v3,
    stage_danger_entry_oath_bargain_restore_v3,
};
use sqlx::Row;

const HALL_ID: &str = "hub.lantern_halls_01";
const WORLD_ID: &str = "world.core_microrealm_01";
const LAYOUT_ID: &str = "layout.core_private_life_01";
const DEED_SEPULCHER: &str = "deed.core.sepulcher_knight_defeated";
const DEED_CALDUS: &str = "deed.core.sir_caldus_defeated";
const SOURCE_SEPULCHER: &str = "miniboss.sepulcher_knight";
const SOURCE_CALDUS: &str = "boss.sir_caldus";
const REWARD_SEPULCHER: &str = "reward.miniboss_t1";
const REWARD_CALDUS: &str = "reward.boss_caldus";
const XP_SEPULCHER: &str = "xp.miniboss_t1";
const XP_CALDUS: &str = "xp.boss_caldus";
const ISSUED_AT_UNIX_MS: u64 = 1;

#[derive(Clone, Copy, Debug)]
struct FixtureIds {
    account: [u8; 16],
    character: [u8; 16],
    lineage: [u8; 16],
    restore: [u8; 16],
    entry_mutation: [u8; 16],
}

impl FixtureIds {
    fn seeded(seed: u8) -> Self {
        Self {
            account: id(seed),
            character: id(seed + 1),
            lineage: id(seed + 2),
            restore: id(seed + 3),
            entry_mutation: id(seed + 4),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CoreRewardKind {
    Sepulcher,
    Caldus,
}

impl CoreRewardKind {
    const fn binding(self) -> (&'static str, &'static str, &'static str, i32) {
        match self {
            Self::Sepulcher => (SOURCE_SEPULCHER, REWARD_SEPULCHER, XP_SEPULCHER, 120),
            Self::Caldus => (SOURCE_CALDUS, REWARD_CALDUS, XP_CALDUS, 450),
        }
    }
}

#[derive(Clone, Debug)]
struct RewardFixture {
    completion: [u8; 16],
    source_instance: [u8; 16],
    reward_result_hash: [u8; 32],
    progression_payload_hash: [u8; 32],
    kind: CoreRewardKind,
    eligible: bool,
    revoked: bool,
    item_content_revision: String,
}

impl RewardFixture {
    fn terminal(seed: u8, kind: CoreRewardKind) -> Self {
        Self {
            completion: id(seed),
            source_instance: id(seed + 1),
            reward_result_hash: hash(seed + 2),
            progression_payload_hash: hash(seed + 3),
            kind,
            eligible: true,
            revoked: false,
            item_content_revision: CORE_ITEM_CONTENT_REVISION.to_owned(),
        }
    }
}

fn id(seed: u8) -> [u8; 16] {
    [seed; 16]
}

fn hash(seed: u8) -> [u8; 32] {
    [seed; 32]
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
    persistence.reset_disposable_identity_data().await.unwrap();
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
    reason = "the complete V3 danger-root fixture stays explicit for three-authority audit"
)]
async fn create_active_fixture(persistence: &PostgresPersistence, ids: FixtureIds) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO accounts (namespace_id,account_id,state_version,slot_capacity) \
         VALUES ($1,$2,1,2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ash_wallets (namespace_id,account_id,balance,wallet_version) \
         VALUES ($1,$2,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO characters (namespace_id,account_id,character_id,roster_ordinal,class_id, \
         level,oath_id,life_state,security_state,character_state_version) \
         VALUES ($1,$2,$3,1,'class.grave_arbalist',10,NULL,0,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE accounts SET selected_character_id=$1 WHERE namespace_id=$2 AND account_id=$3",
    )
    .bind(ids.character.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_progression (namespace_id,account_id,character_id,total_xp,level, \
         current_health,progression_version) VALUES ($1,$2,$3,2700,10,120,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_inventories \
         (namespace_id,account_id,character_id,inventory_version) VALUES ($1,$2,$3,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_oath_bargain_state (namespace_id,account_id,character_id, \
         earned_bargain_slots,oath_bargain_version) VALUES ($1,$2,$3,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_instance_lineages (namespace_id,account_id,character_id,lineage_id, \
         content_id,layout_id,lineage_state,records_blake3,assets_blake3,localization_blake3) \
         VALUES ($1,$2,$3,$4,$5,$6,1,$7,$8,$9)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .bind(ids.lineage.as_slice())
    .bind(WORLD_ID)
    .bind(LAYOUT_ID)
    .bind(CORE_WORLD_RECORDS_BLAKE3)
    .bind(CORE_WORLD_ASSETS_BLAKE3)
    .bind(CORE_WORLD_LOCALIZATION_BLAKE3)
    .execute(transaction.connection())
    .await
    .unwrap();
    insert_restore_root(&mut transaction, ids).await;
    insert_progression_restore_components(&mut transaction, ids).await;
    let inventory = stage_danger_entry_inventory_restore_v3(
        &mut transaction,
        ids.account,
        ids.character,
        ids.restore,
        ids.entry_mutation,
        0,
    )
    .await
    .unwrap();
    assert_eq!(inventory.post_inventory_version, 1);
    stage_danger_entry_oath_bargain_restore_v3(
        &mut transaction,
        ids.account,
        ids.character,
        ids.restore,
    )
    .await
    .unwrap();
    stage_danger_entry_life_metrics_restore_v3(
        &mut transaction,
        ids.account,
        ids.character,
        ids.restore,
    )
    .await
    .unwrap();
    stage_danger_entry_ash_wallet_restore_v3(
        &mut transaction,
        ids.account,
        ids.character,
        ids.restore,
    )
    .await
    .unwrap();
    insert_active_world_projection(&mut transaction, ids).await;
    transaction.commit().await.unwrap();
}

async fn insert_restore_root(
    transaction: &mut persistence::PersistenceTransaction<'_>,
    ids: FixtureIds,
) {
    sqlx::query(
        "INSERT INTO character_entry_restore_points (namespace_id,account_id,character_id, \
         restore_point_id,lineage_id,source_location_id,restore_location_id, \
         snapshot_contract_version,account_version,character_version,progression_version, \
         inventory_version,oath_bargain_version,life_metrics_version,ash_wallet_version, \
         component_mask,composite_digest,restore_state,records_blake3,assets_blake3, \
         localization_blake3) VALUES ($1,$2,$3,$4,$5,$6,$6,3,1,1,1,1,1,1,1,31,$7,0,$8,$9,$10)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .bind(ids.restore.as_slice())
    .bind(ids.lineage.as_slice())
    .bind(HALL_ID)
    .bind(hash(11).as_slice())
    .bind(CORE_WORLD_RECORDS_BLAKE3)
    .bind(CORE_WORLD_ASSETS_BLAKE3)
    .bind(CORE_WORLD_LOCALIZATION_BLAKE3)
    .execute(transaction.connection())
    .await
    .unwrap();
}

async fn insert_progression_restore_components(
    transaction: &mut persistence::PersistenceTransaction<'_>,
    ids: FixtureIds,
) {
    sqlx::query(
        "INSERT INTO entry_restore_progression_v3 (namespace_id,account_id,character_id, \
         restore_point_id,level,total_xp,current_health,progression_version,component_digest) \
         VALUES ($1,$2,$3,$4,10,2700,120,1,$5)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .bind(ids.restore.as_slice())
    .bind(hash(12).as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO entry_restore_progression_v1 (namespace_id,account_id,character_id, \
         restore_point_id,level,total_xp,current_health,progression_version) \
         VALUES ($1,$2,$3,$4,10,2700,120,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .bind(ids.restore.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
}

async fn insert_active_world_projection(
    transaction: &mut persistence::PersistenceTransaction<'_>,
    ids: FixtureIds,
) {
    sqlx::query(
        "INSERT INTO character_world_locations (namespace_id,account_id,character_id, \
         character_version,location_kind,location_content_id,instance_lineage_id, \
         entry_restore_point_id) VALUES ($1,$2,$3,2,2,$4,$5,$6)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .bind(WORLD_ID)
    .bind(ids.lineage.as_slice())
    .bind(ids.restore.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE characters SET character_state_version=2 WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_life_metrics SET lifetime_ticks=18000, \
         permadeath_combat_ticks=18000,life_metrics_version=2 WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
}

async fn insert_terminal_reward(
    persistence: &PostgresPersistence,
    ids: FixtureIds,
    reward: &RewardFixture,
) {
    let (source, reward_table, xp_profile, base_xp) = reward.kind.binding();
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO reward_requests (namespace_id,reward_request_id,account_id,character_id, \
         source_instance_id,reward_table_id,content_revision,epoch_id,canonical_request_hash, \
         plan_hash,result_hash,audit_digest,pre_inventory_version,post_inventory_version, \
         request_state,reward_item_count) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,'live-deed-hosted-v2',$8,$9,$10,$11,2,2,1,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(reward.completion.as_slice())
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .bind(reward.source_instance.as_slice())
    .bind(reward_table)
    .bind(&reward.item_content_revision)
    .bind(hash(21).as_slice())
    .bind(hash(22).as_slice())
    .bind(reward.reward_result_hash.as_slice())
    .bind(hash(23).as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    insert_xp_authority(&mut transaction, ids, reward, source, xp_profile, base_xp).await;
    if reward.kind == CoreRewardKind::Caldus {
        insert_caldus_victory_owner(&mut transaction, ids, reward).await;
    }
    transaction.commit().await.unwrap();
}

async fn insert_pending_reward(
    persistence: &PostgresPersistence,
    ids: FixtureIds,
    completion: [u8; 16],
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO reward_requests (namespace_id,reward_request_id,account_id,character_id, \
         source_instance_id,reward_table_id,content_revision,epoch_id,canonical_request_hash, \
         pre_inventory_version,request_state) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,'live-deed-pending-v2',$8,2,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(completion.as_slice())
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .bind(id(completion[0] + 1).as_slice())
    .bind(REWARD_SEPULCHER)
    .bind(CORE_ITEM_CONTENT_REVISION)
    .bind(hash(25).as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn insert_xp_authority(
    transaction: &mut persistence::PersistenceTransaction<'_>,
    ids: FixtureIds,
    reward: &RewardFixture,
    source: &str,
    xp_profile: &str,
    base_xp: i32,
) {
    let revoked_restore = reward.revoked.then_some(ids.restore.as_slice());
    let revoked_at = reward.revoked.then_some(1_i64);
    let revocation_version = reward.revoked.then_some(2_i64);
    sqlx::query(
        "INSERT INTO character_xp_award_results (namespace_id,account_id,character_id, \
         reward_event_id,payload_hash,source_content_id,xp_profile_id, \
         progression_content_revision,eligibility_kind,eligible,encounter_active_ticks, \
         encounter_present_ticks,encounter_longest_inactivity_ticks,encounter_reference_health, \
         encounter_direct_damage,encounter_effective_healing,encounter_damage_prevented, \
         encounter_objective_credits,encounter_life_state,encounter_recall_state, \
         encounter_trust_state,first_clear_awarded,base_xp,bonus_xp,requested_xp,applied_xp, \
         discarded_xp,pre_total_xp,post_total_xp,pre_level,post_level,pre_progression_version, \
         post_progression_version,result_code,result_payload,entry_restore_point_id, \
         revoked_by_restore_point_id,revoked_at,revocation_progression_version) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,1,$9,300,300,0,7200,1,0,0,0,0,0,0,FALSE, \
                 $10,0,$10,0,$10,2700,2700,10,10,1,1,0,$11,$12,$13, \
                 CASE WHEN $14::BIGINT IS NULL THEN NULL ELSE to_timestamp($14) END,$15)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .bind(reward.completion.as_slice())
    .bind(reward.progression_payload_hash.as_slice())
    .bind(source)
    .bind(xp_profile)
    .bind(CORE_PROGRESSION_RECORDS_BLAKE3)
    .bind(reward.eligible)
    .bind(base_xp)
    .bind([1_u8].as_slice())
    .bind(ids.restore.as_slice())
    .bind(revoked_restore)
    .bind(revoked_at)
    .bind(revocation_version)
    .execute(transaction.connection())
    .await
    .unwrap();
}

async fn insert_caldus_victory_owner(
    transaction: &mut persistence::PersistenceTransaction<'_>,
    ids: FixtureIds,
    reward: &RewardFixture,
) {
    sqlx::query(
        "INSERT INTO caldus_victory_exits (namespace_id,encounter_id,instance_lineage_id, \
         attempt_ordinal,exit_instance_id,canonical_request_hash,eligible_owner_count) \
         VALUES ($1,$2,$3,1,$4,$5,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(reward.source_instance.as_slice())
    .bind(ids.lineage.as_slice())
    .bind(id(reward.source_instance[0] + 1).as_slice())
    .bind(hash(24).as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO caldus_victory_exit_owners (namespace_id,encounter_id,party_slot, \
         participant_entity_id,account_id,character_id,reward_request_id,reward_result_hash, \
         progression_payload_hash) VALUES ($1,$2,0,$3,$4,$5,$6,$7,$8)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(reward.source_instance.as_slice())
    .bind([71_u8; 8].as_slice())
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .bind(reward.completion.as_slice())
    .bind(reward.reward_result_hash.as_slice())
    .bind(reward.progression_payload_hash.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
}

fn request(
    ids: FixtureIds,
    completion: [u8; 16],
    expected_character_version: u64,
    expected_life_metrics_version: u64,
    achieved_tick: u64,
) -> LifeDeedCompletionRequestV2 {
    LifeDeedCompletionRequestV2::seal(LifeDeedCompletionCommandV2 {
        account_id: ids.account,
        character_id: ids.character,
        completion_id: completion,
        expected_character_version,
        expected_life_metrics_version,
        lineage_id: ids.lineage,
        restore_point_id: ids.restore,
        achieved_tick,
        content: LifeDeedContentAuthorityV2::core(),
        issued_at_unix_ms: ISSUED_AT_UNIX_MS,
    })
    .unwrap()
}

async fn life_metrics_version(persistence: &PostgresPersistence, ids: FixtureIds) -> i64 {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let version = sqlx::query_scalar(
        "SELECT life_metrics_version FROM character_life_metrics \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    version
}

async fn deed_receipt_count(persistence: &PostgresPersistence, ids: FixtureIds) -> i64 {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let count = sqlx::query_scalar(
        "SELECT count(*) FROM character_life_deed_completion_receipts_v2 \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    count
}

async fn scenario_fresh_replay_restart_and_conflict(persistence: &PostgresPersistence) {
    let ids = FixtureIds::seeded(30);
    let reward = RewardFixture::terminal(200, CoreRewardKind::Sepulcher);
    create_active_fixture(persistence, ids).await;
    insert_terminal_reward(persistence, ids, &reward).await;
    let original = request(ids, reward.completion, 2, 2, 10_000);
    let committed = persistence
        .transact_life_deed_completion_v2(&original)
        .await
        .unwrap();
    let LifeDeedCompletionTransactionV2::Committed(receipt) = committed else {
        panic!("fresh live deed must commit")
    };
    assert_eq!(receipt.kind, LifeDeedKindV2::FinalDeedOnly);
    assert_eq!(
        receipt.projection_outcome,
        LifeDeedProjectionOutcomeV2::Inserted
    );
    assert_eq!(
        (
            receipt.pre_life_metrics_version,
            receipt.post_life_metrics_version
        ),
        (2, 3)
    );

    let replay = persistence
        .transact_life_deed_completion_v2(&original)
        .await
        .unwrap();
    assert!(
        matches!(replay, LifeDeedCompletionTransactionV2::Replayed(ref stored) if stored == &receipt)
    );
    let restarted = reconnect_database().await;
    let restarted_replay = restarted
        .transact_life_deed_completion_v2(&original)
        .await
        .unwrap();
    assert!(
        matches!(restarted_replay, LifeDeedCompletionTransactionV2::Replayed(ref stored) if stored == &receipt)
    );

    let changed = request(ids, reward.completion, 2, 2, 10_001);
    for _ in 0..2 {
        assert!(matches!(
            restarted.transact_life_deed_completion_v2(&changed).await,
            Err(PersistenceError::LifeDeedIdempotencyConflict)
        ));
    }
    assert_conflict_audit(&restarted, ids, &original, &changed).await;
    assert_eq!(life_metrics_version(&restarted, ids).await, 3);
    assert_eq!(deed_receipt_count(&restarted, ids).await, 1);
    restarted.close().await;
}

async fn assert_conflict_audit(
    persistence: &PostgresPersistence,
    ids: FixtureIds,
    stored: &LifeDeedCompletionRequestV2,
    attempted: &LifeDeedCompletionRequestV2,
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let row = sqlx::query(
        "SELECT count(*) OVER () AS audit_count,stored_request_hash,attempted_request_hash \
         FROM character_life_deed_conflict_audits_v2 WHERE namespace_id=$1 AND account_id=$2 \
         AND completion_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(stored.command.completion_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(row.get::<i64, _>("audit_count"), 1);
    assert_eq!(
        row.get::<Vec<u8>, _>("stored_request_hash"),
        stored.request_hash
    );
    assert_eq!(
        row.get::<Vec<u8>, _>("attempted_request_hash"),
        attempted.request_hash
    );
    transaction.rollback().await.unwrap();
}

async fn scenario_caldus_and_deterministic_tie(persistence: &PostgresPersistence) {
    let caldus_ids = FixtureIds::seeded(40);
    let caldus = RewardFixture::terminal(205, CoreRewardKind::Caldus);
    create_active_fixture(persistence, caldus_ids).await;
    insert_terminal_reward(persistence, caldus_ids, &caldus).await;
    let result = persistence
        .transact_life_deed_completion_v2(&request(caldus_ids, caldus.completion, 2, 2, 11_000))
        .await
        .unwrap();
    assert_eq!(result.receipt().kind, LifeDeedKindV2::DungeonBoss);
    assert_projection(
        persistence,
        caldus_ids,
        DEED_CALDUS,
        caldus.completion,
        0,
        11_000,
    )
    .await;

    let tie_ids = FixtureIds::seeded(50);
    let high = RewardFixture::terminal(210, CoreRewardKind::Sepulcher);
    let low = RewardFixture::terminal(209, CoreRewardKind::Sepulcher);
    let higher = RewardFixture::terminal(211, CoreRewardKind::Sepulcher);
    create_active_fixture(persistence, tie_ids).await;
    insert_terminal_reward(persistence, tie_ids, &high).await;
    insert_terminal_reward(persistence, tie_ids, &low).await;
    insert_terminal_reward(persistence, tie_ids, &higher).await;
    let first = persistence
        .transact_life_deed_completion_v2(&request(tie_ids, high.completion, 2, 2, 12_000))
        .await
        .unwrap();
    let second = persistence
        .transact_life_deed_completion_v2(&request(tie_ids, low.completion, 2, 3, 12_000))
        .await
        .unwrap();
    assert_eq!(
        first.receipt().projection_outcome,
        LifeDeedProjectionOutcomeV2::Inserted
    );
    assert_eq!(
        second.receipt().projection_outcome,
        LifeDeedProjectionOutcomeV2::RetainedNewer
    );
    assert_eq!(second.receipt().kind, LifeDeedKindV2::FinalDeedOnly);
    let third = persistence
        .transact_life_deed_completion_v2(&request(tie_ids, higher.completion, 2, 4, 12_000))
        .await
        .unwrap();
    assert_eq!(
        third.receipt().projection_outcome,
        LifeDeedProjectionOutcomeV2::Advanced
    );
    assert_projection(
        persistence,
        tie_ids,
        DEED_SEPULCHER,
        higher.completion,
        2,
        12_000,
    )
    .await;
    assert_eq!(life_metrics_version(persistence, tie_ids).await, 5);
}

async fn assert_projection(
    persistence: &PostgresPersistence,
    ids: FixtureIds,
    deed_id: &str,
    completion: [u8; 16],
    kind: i16,
    achieved_tick: i64,
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let row = sqlx::query(
        "SELECT reward_event_id,deed_kind,achieved_tick FROM character_life_deeds \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND deed_id=$4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .bind(deed_id)
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(row.get::<Vec<u8>, _>("reward_event_id"), completion);
    assert_eq!(row.get::<i16, _>("deed_kind"), kind);
    assert_eq!(row.get::<i64, _>("achieved_tick"), achieved_tick);
    transaction.rollback().await.unwrap();
}

async fn scenario_stale_foreign_unselected_and_content(persistence: &PostgresPersistence) {
    let stale_ids = FixtureIds::seeded(60);
    let stale_reward = RewardFixture::terminal(214, CoreRewardKind::Sepulcher);
    create_active_fixture(persistence, stale_ids).await;
    insert_terminal_reward(persistence, stale_ids, &stale_reward).await;
    assert!(matches!(
        persistence
            .transact_life_deed_completion_v2(&request(
                stale_ids,
                stale_reward.completion,
                1,
                2,
                13_000
            ))
            .await,
        Err(PersistenceError::LifeDeedCharacterVersionMismatch {
            expected: 1,
            actual: 2
        })
    ));
    assert!(matches!(
        persistence
            .transact_life_deed_completion_v2(&request(
                stale_ids,
                stale_reward.completion,
                2,
                1,
                13_000
            ))
            .await,
        Err(PersistenceError::LifeDeedMetricsVersionMismatch {
            expected: 1,
            actual: 2,
            ..
        })
    ));

    let foreign_ids = FixtureIds::seeded(70);
    create_active_fixture(persistence, foreign_ids).await;
    let mut foreign = request(stale_ids, stale_reward.completion, 2, 2, 13_000).command;
    foreign.character_id = foreign_ids.character;
    let foreign = LifeDeedCompletionRequestV2::seal(foreign).unwrap();
    assert!(matches!(
        persistence.transact_life_deed_completion_v2(&foreign).await,
        Err(PersistenceError::LifeDeedBindingMismatch)
    ));

    set_selected_character(persistence, stale_ids, None).await;
    assert!(matches!(
        persistence
            .transact_life_deed_completion_v2(&request(
                stale_ids,
                stale_reward.completion,
                2,
                2,
                13_000
            ))
            .await,
        Err(PersistenceError::LifeDeedBindingMismatch)
    ));
    let mut wrong_content = request(stale_ids, stale_reward.completion, 2, 2, 13_000).command;
    wrong_content.content.item_content_revision = format!("core-dev.blake3.{}", "0".repeat(64));
    assert!(matches!(
        LifeDeedCompletionRequestV2::seal(wrong_content),
        Err(PersistenceError::LifeDeedContentMismatch)
    ));
    assert_eq!(life_metrics_version(persistence, stale_ids).await, 2);
}

async fn set_selected_character(
    persistence: &PostgresPersistence,
    ids: FixtureIds,
    selected: Option<[u8; 16]>,
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "UPDATE accounts SET selected_character_id=$1 WHERE namespace_id=$2 AND account_id=$3",
    )
    .bind(selected.as_ref().map(<[u8; 16]>::as_slice))
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn scenario_dead_and_abnormal_security(persistence: &PostgresPersistence) {
    let dead_ids = FixtureIds::seeded(80);
    let dead_reward = RewardFixture::terminal(218, CoreRewardKind::Sepulcher);
    create_active_fixture(persistence, dead_ids).await;
    insert_terminal_reward(persistence, dead_ids, &dead_reward).await;
    set_fixture_life_state(persistence, dead_ids, 1).await;
    let dead_result = persistence
        .transact_life_deed_completion_v2(&request(dead_ids, dead_reward.completion, 2, 2, 14_000))
        .await;
    set_fixture_life_state(persistence, dead_ids, 0).await;
    assert!(matches!(
        dead_result,
        Err(PersistenceError::LifeDeedBindingMismatch)
    ));

    let security_ids = FixtureIds::seeded(90);
    let security_reward = RewardFixture::terminal(222, CoreRewardKind::Sepulcher);
    create_active_fixture(persistence, security_ids).await;
    insert_terminal_reward(persistence, security_ids, &security_reward).await;
    set_fixture_security_state(persistence, security_ids, 1).await;
    let security_result = persistence
        .transact_life_deed_completion_v2(&request(
            security_ids,
            security_reward.completion,
            2,
            2,
            14_000,
        ))
        .await;
    set_fixture_security_state(persistence, security_ids, 0).await;
    assert!(matches!(
        security_result,
        Err(PersistenceError::LifeDeedBindingMismatch)
    ));
}

async fn set_fixture_life_state(
    persistence: &PostgresPersistence,
    ids: FixtureIds,
    life_state: i16,
) {
    let roster_ordinal = (life_state == 0).then_some(1_i16);
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query("ALTER TABLE accounts DISABLE TRIGGER account_selected_character_live_update")
        .execute(transaction.connection())
        .await
        .unwrap();
    sqlx::query("ALTER TABLE characters DISABLE TRIGGER dead_character_terminal")
        .execute(transaction.connection())
        .await
        .unwrap();
    sqlx::query(
        "UPDATE characters SET life_state=$1,roster_ordinal=$2 WHERE namespace_id=$3 \
         AND account_id=$4 AND character_id=$5",
    )
    .bind(life_state)
    .bind(roster_ordinal)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query("ALTER TABLE characters ENABLE TRIGGER dead_character_terminal")
        .execute(transaction.connection())
        .await
        .unwrap();
    sqlx::query("ALTER TABLE accounts ENABLE TRIGGER account_selected_character_live_update")
        .execute(transaction.connection())
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn set_fixture_security_state(
    persistence: &PostgresPersistence,
    ids: FixtureIds,
    security_state: i16,
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    if security_state != 0 {
        sqlx::query("ALTER TABLE characters DROP CONSTRAINT character_security_state_core")
            .execute(transaction.connection())
            .await
            .unwrap();
    }
    sqlx::query(
        "UPDATE characters SET security_state=$1 WHERE namespace_id=$2 AND account_id=$3 \
         AND character_id=$4",
    )
    .bind(security_state)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    if security_state == 0 {
        sqlx::query(
            "ALTER TABLE characters ADD CONSTRAINT character_security_state_core \
             CHECK (security_state = 0)",
        )
        .execute(transaction.connection())
        .await
        .unwrap();
    }
    transaction.commit().await.unwrap();
}

async fn scenario_reward_failures_and_late_rollback(persistence: &PostgresPersistence) {
    let missing_ids = FixtureIds::seeded(100);
    create_active_fixture(persistence, missing_ids).await;
    insert_pending_reward(persistence, missing_ids, id(226)).await;
    assert!(matches!(
        persistence
            .transact_life_deed_completion_v2(&request(missing_ids, id(226), 2, 2, 15_000))
            .await,
        Err(PersistenceError::LifeDeedRewardNotTerminal)
    ));

    let ineligible_ids = FixtureIds::seeded(110);
    let mut ineligible = RewardFixture::terminal(230, CoreRewardKind::Sepulcher);
    ineligible.eligible = false;
    create_active_fixture(persistence, ineligible_ids).await;
    insert_terminal_reward(persistence, ineligible_ids, &ineligible).await;
    assert_reward_mismatch(persistence, ineligible_ids, &ineligible).await;

    let revoked_ids = FixtureIds::seeded(120);
    let mut revoked = RewardFixture::terminal(234, CoreRewardKind::Sepulcher);
    revoked.revoked = true;
    create_active_fixture(persistence, revoked_ids).await;
    insert_terminal_reward(persistence, revoked_ids, &revoked).await;
    assert_reward_mismatch(persistence, revoked_ids, &revoked).await;

    let revision_ids = FixtureIds::seeded(130);
    let mut wrong_revision = RewardFixture::terminal(238, CoreRewardKind::Sepulcher);
    wrong_revision.item_content_revision = format!("core-dev.blake3.{}", "0".repeat(64));
    create_active_fixture(persistence, revision_ids).await;
    insert_terminal_reward(persistence, revision_ids, &wrong_revision).await;
    assert_reward_mismatch(persistence, revision_ids, &wrong_revision).await;

    let owner_ids = FixtureIds::seeded(135);
    let missing_owner = RewardFixture::terminal(240, CoreRewardKind::Caldus);
    create_active_fixture(persistence, owner_ids).await;
    insert_terminal_reward(persistence, owner_ids, &missing_owner).await;
    delete_caldus_owner(persistence, &missing_owner).await;
    assert_reward_mismatch(persistence, owner_ids, &missing_owner).await;

    let rollback_ids = FixtureIds::seeded(140);
    let rollback_reward = RewardFixture::terminal(242, CoreRewardKind::Sepulcher);
    create_active_fixture(persistence, rollback_ids).await;
    insert_terminal_reward(persistence, rollback_ids, &rollback_reward).await;
    let mut future = request(rollback_ids, rollback_reward.completion, 2, 2, 16_000).command;
    future.issued_at_unix_ms = i64::MAX as u64;
    let future = LifeDeedCompletionRequestV2::seal(future).unwrap();
    assert!(matches!(
        persistence.transact_life_deed_completion_v2(&future).await,
        Err(PersistenceError::CorruptStoredLifeDeed)
    ));
    assert_unchanged_after_rejection(persistence, rollback_ids).await;
}

async fn delete_caldus_owner(persistence: &PostgresPersistence, reward: &RewardFixture) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query("DELETE FROM caldus_victory_exit_owners WHERE namespace_id=$1 AND encounter_id=$2")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(reward.source_instance.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn assert_reward_mismatch(
    persistence: &PostgresPersistence,
    ids: FixtureIds,
    reward: &RewardFixture,
) {
    assert!(matches!(
        persistence
            .transact_life_deed_completion_v2(&request(ids, reward.completion, 2, 2, 15_000))
            .await,
        Err(PersistenceError::LifeDeedRewardMismatch)
    ));
    assert_unchanged_after_rejection(persistence, ids).await;
}

async fn assert_unchanged_after_rejection(persistence: &PostgresPersistence, ids: FixtureIds) {
    assert_eq!(life_metrics_version(persistence, ids).await, 2);
    assert_eq!(deed_receipt_count(persistence, ids).await, 0);
    for _ in 0..12 {
        let mut transaction = persistence.begin_transaction().await.unwrap();
        let ready: i32 = sqlx::query_scalar("SELECT 1")
            .fetch_one(transaction.connection())
            .await
            .unwrap();
        assert_eq!(ready, 1);
        transaction.rollback().await.unwrap();
    }
}

async fn scenario_concurrency(persistence: &PostgresPersistence) {
    let same_ids = FixtureIds::seeded(150);
    let same_reward = RewardFixture::terminal(246, CoreRewardKind::Sepulcher);
    create_active_fixture(persistence, same_ids).await;
    insert_terminal_reward(persistence, same_ids, &same_reward).await;
    let same_request = request(same_ids, same_reward.completion, 2, 2, 17_000);
    let (left, right) = tokio::join!(
        persistence.transact_life_deed_completion_v2(&same_request),
        persistence.transact_life_deed_completion_v2(&same_request),
    );
    let transactions = [left.unwrap(), right.unwrap()];
    assert_eq!(
        transactions
            .iter()
            .filter(|result| matches!(result, LifeDeedCompletionTransactionV2::Committed(_)))
            .count(),
        1
    );
    assert_eq!(deed_receipt_count(persistence, same_ids).await, 1);

    let race_ids = FixtureIds::seeded(160);
    let first = RewardFixture::terminal(250, CoreRewardKind::Sepulcher);
    let second = RewardFixture::terminal(185, CoreRewardKind::Sepulcher);
    create_active_fixture(persistence, race_ids).await;
    insert_terminal_reward(persistence, race_ids, &first).await;
    insert_terminal_reward(persistence, race_ids, &second).await;
    let first_request = request(race_ids, first.completion, 2, 2, 18_000);
    let second_request = request(race_ids, second.completion, 2, 2, 18_001);
    let (left, right) = tokio::join!(
        persistence.transact_life_deed_completion_v2(&first_request),
        persistence.transact_life_deed_completion_v2(&second_request),
    );
    assert_distinct_race(&left, &right);
    assert_eq!(life_metrics_version(persistence, race_ids).await, 3);
    assert_eq!(deed_receipt_count(persistence, race_ids).await, 1);
}

fn assert_distinct_race(
    left: &Result<LifeDeedCompletionTransactionV2, PersistenceError>,
    right: &Result<LifeDeedCompletionTransactionV2, PersistenceError>,
) {
    let committed = [left, right]
        .into_iter()
        .filter(|result| matches!(result, Ok(LifeDeedCompletionTransactionV2::Committed(_))))
        .count();
    let stale = [left, right]
        .into_iter()
        .filter(|result| {
            matches!(
                result,
                Err(PersistenceError::LifeDeedMetricsVersionMismatch {
                    expected: 2,
                    actual: 3,
                    ..
                })
            )
        })
        .count();
    assert_eq!((committed, stale), (1, 1));
}

async fn scenario_legacy_collision_and_projection_corruption(persistence: &PostgresPersistence) {
    let legacy_ids = FixtureIds::seeded(170);
    let legacy_reward = RewardFixture::terminal(190, CoreRewardKind::Sepulcher);
    create_active_fixture(persistence, legacy_ids).await;
    insert_terminal_reward(persistence, legacy_ids, &legacy_reward).await;
    insert_legacy_receipt(persistence, legacy_ids, legacy_reward.completion).await;
    assert!(matches!(
        persistence
            .transact_life_deed_completion_v2(&request(
                legacy_ids,
                legacy_reward.completion,
                2,
                2,
                19_000,
            ))
            .await,
        Err(PersistenceError::CorruptStoredLifeDeed)
    ));
    assert_unchanged_after_rejection(persistence, legacy_ids).await;

    let corrupt_ids = FixtureIds::seeded(180);
    let corrupt_reward = RewardFixture::terminal(194, CoreRewardKind::Sepulcher);
    create_active_fixture(persistence, corrupt_ids).await;
    insert_terminal_reward(persistence, corrupt_ids, &corrupt_reward).await;
    let original = request(corrupt_ids, corrupt_reward.completion, 2, 2, 20_000);
    persistence
        .transact_life_deed_completion_v2(&original)
        .await
        .unwrap();
    force_projection_tick(persistence, corrupt_ids, 20_001).await;
    let replay = persistence
        .transact_life_deed_completion_v2(&original)
        .await;
    force_projection_tick(persistence, corrupt_ids, 20_000).await;
    assert!(matches!(
        replay,
        Err(PersistenceError::CorruptStoredLifeDeed)
    ));
    assert_eq!(deed_receipt_count(persistence, corrupt_ids).await, 1);
}

async fn insert_legacy_receipt(
    persistence: &PostgresPersistence,
    ids: FixtureIds,
    completion: [u8; 16],
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO character_life_deed_completion_receipts_v1 \
         (namespace_id,account_id,character_id,completion_id,deed_id,source_content_id,deed_kind, \
          achieved_tick,content_revision,projection_outcome,request_hash,result_digest, \
          expected_character_version,issued_at) \
         VALUES ($1,$2,$3,$4,$5,$6,2,19000,$7,0,$8,$9,2,to_timestamp(1))",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .bind(completion.as_slice())
    .bind(DEED_SEPULCHER)
    .bind(SOURCE_SEPULCHER)
    .bind(CORE_ITEM_CONTENT_REVISION)
    .bind(hash(31).as_slice())
    .bind(hash(32).as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn force_projection_tick(
    persistence: &PostgresPersistence,
    ids: FixtureIds,
    achieved_tick: i64,
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "ALTER TABLE character_life_deeds DISABLE TRIGGER life_deed_projection_self_exact_v2",
    )
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_life_deeds SET achieved_tick=$1 WHERE namespace_id=$2 \
         AND account_id=$3 AND character_id=$4 AND deed_id=$5",
    )
    .bind(achieved_tick)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .bind(DEED_SEPULCHER)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE character_life_deeds ENABLE TRIGGER life_deed_projection_self_exact_v2",
    )
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires guarded TEST_DATABASE_URL PostgreSQL"]
async fn postgres_live_deed_v2_closes_reward_replay_and_projection_authority() {
    let persistence = disposable_database().await;
    scenario_fresh_replay_restart_and_conflict(&persistence).await;
    scenario_caldus_and_deterministic_tie(&persistence).await;
    scenario_stale_foreign_unselected_and_content(&persistence).await;
    scenario_dead_and_abnormal_security(&persistence).await;
    scenario_reward_failures_and_late_rollback(&persistence).await;
    Box::pin(scenario_concurrency(&persistence)).await;
    scenario_legacy_collision_and_projection_corruption(&persistence).await;
    persistence.close().await;
}
