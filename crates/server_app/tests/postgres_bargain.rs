use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use persistence::{
    BargainDeclinedEventV1, BargainLifeCleanupCommand, BargainLifeCleanupEventV1,
    BargainLifeEndReason, BargainOfferedEventV1, PersistenceConfig, PersistenceError,
    PostgresPersistence, WIPEABLE_CORE_NAMESPACE, cleanup_bargains_for_life_end,
};
use protocol::{
    BargainContentRevisionV1, BargainDecision, BargainDecisionFrame, BargainDecisionPayload,
    BargainOfferCell, BargainOfferState, BargainResultCode, BargainViewFrame, ManifestHash,
    WireText,
};
use server_app::{
    AccountId, AuthenticatedAccount, AuthenticatedNamespace, CoreDurableB3Resolution,
    CoreDurableB3RewardCommit, EntryCaptureContext, EntryRestoreProvider, IdentityClock,
    PostgresBargainService, PostgresCoreB3RewardCoordinator, PostgresProgressionAwardService,
    PostgresProgressionRestoreProvider, ProgressionAwardCode, ProgressionAwardCommand,
    ProgressionAwardEvidence, ProgressionAwardPayload, SecretRewardEpoch,
};
use sim_core::{
    EncounterXpEvidence, EntityId, RewardLifeState, RewardRecallState, RewardTrustState,
    SpawnInstanceId, Tick,
};
use sqlx::Row;

const SELECT_ACCOUNT_ID: [u8; 16] = [111; 16];
const SELECT_CHARACTER_ID: [u8; 16] = [2; 16];
const SELECT_RESTORE_ID: [u8; 16] = [112; 16];
const SELECT_LINEAGE_ID: [u8; 16] = [113; 16];
const SELECT_REWARD_ID: [u8; 16] = [1; 16];
const SELECT_MUTATION_ID: [u8; 16] = [114; 16];

const REFUSE_ACCOUNT_ID: [u8; 16] = [121; 16];
const REFUSE_CHARACTER_ID: [u8; 16] = [122; 16];
const REFUSE_RESTORE_ID: [u8; 16] = [123; 16];
const REFUSE_LINEAGE_ID: [u8; 16] = [124; 16];
const REFUSE_REWARD_ID: [u8; 16] = [125; 16];
const REFUSE_MUTATION_ID: [u8; 16] = [126; 16];

const B3_FIXTURE: FixtureIds = FixtureIds {
    account: [131; 16],
    character: [132; 16],
    restore: [133; 16],
    lineage: [134; 16],
    reward: [135; 16],
    mutation: [136; 16],
};

const B3_LOW_FIXTURE: FixtureIds = FixtureIds {
    account: [141; 16],
    character: [142; 16],
    restore: [143; 16],
    lineage: [144; 16],
    reward: [145; 16],
    mutation: [146; 16],
};
const B3_INELIGIBLE_FIXTURE: FixtureIds = FixtureIds {
    account: [171; 16],
    character: [172; 16],
    restore: [173; 16],
    lineage: [174; 16],
    reward: [175; 16],
    mutation: [176; 16],
};

const CORE_LAYOUT_ID: &str = "layout.core_private_life_01";
const CORE_SOURCE_ID: &str = "miniboss.sepulcher_knight";

#[derive(Debug, Clone, Copy)]
struct FixtureIds {
    account: [u8; 16],
    character: [u8; 16],
    restore: [u8; 16],
    lineage: [u8; 16],
    reward: [u8; 16],
    mutation: [u8; 16],
}

const SELECT_FIXTURE: FixtureIds = FixtureIds {
    account: SELECT_ACCOUNT_ID,
    character: SELECT_CHARACTER_ID,
    restore: SELECT_RESTORE_ID,
    lineage: SELECT_LINEAGE_ID,
    reward: SELECT_REWARD_ID,
    mutation: SELECT_MUTATION_ID,
};

const REFUSE_FIXTURE: FixtureIds = FixtureIds {
    account: REFUSE_ACCOUNT_ID,
    character: REFUSE_CHARACTER_ID,
    restore: REFUSE_RESTORE_ID,
    lineage: REFUSE_LINEAGE_ID,
    reward: REFUSE_REWARD_ID,
    mutation: REFUSE_MUTATION_ID,
};

#[derive(Debug, Clone, Copy)]
struct FixedClock;

impl IdentityClock for FixedClock {
    fn unix_millis(&self) -> u64 {
        10_000
    }
}

struct TerminalExpectation<'a> {
    offer_state: i16,
    selected: Option<&'a str>,
    version: i64,
    active_count: i64,
    outbox_count: i64,
    payload: &'a [u8],
}

fn content_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
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

fn authenticated(ids: FixtureIds) -> AuthenticatedAccount {
    AuthenticatedAccount {
        account_id: AccountId::new(ids.account).unwrap(),
        namespace: AuthenticatedNamespace::WipeableTest,
    }
}

fn bargain_revision(content: &sim_content::CompiledOathBargainCatalog) -> BargainContentRevisionV1 {
    let hashes = content.hashes();
    BargainContentRevisionV1 {
        records_blake3: ManifestHash::new(hashes.records_blake3.clone()).unwrap(),
        assets_blake3: ManifestHash::new(hashes.assets_blake3.clone()).unwrap(),
        localization_blake3: ManifestHash::new(hashes.localization_blake3.clone()).unwrap(),
    }
}

async fn reset_level_five_fixture(persistence: &PostgresPersistence, ids: FixtureIds) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query("DELETE FROM accounts WHERE namespace_id = $1 AND account_id = $2")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ids.account.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO accounts (namespace_id, account_id, state_version, slot_capacity, \
         selected_character_id) VALUES ($1, $2, 1, 2, NULL)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO characters (namespace_id, account_id, character_id, roster_ordinal, \
         class_id, level, oath_id, life_state, security_state, character_state_version) \
         VALUES ($1, $2, $3, 1, 'class.grave_arbalist', 5, NULL, 0, 0, 1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE accounts SET selected_character_id = $1 WHERE namespace_id = $2 AND account_id = $3",
    )
    .bind(ids.character.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_world_locations (namespace_id, account_id, character_id, \
         character_version, location_kind, location_content_id, safe_arrival_kind) \
         VALUES ($1, $2, $3, 1, 1, 'hub.lantern_halls_01', 0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_progression (namespace_id, account_id, character_id, total_xp, \
         level, current_health, progression_version) VALUES ($1, $2, $3, 700, 5, 136, 1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_inventories (namespace_id, account_id, character_id, \
         inventory_version) VALUES ($1, $2, $3, 1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_oath_bargain_state (namespace_id, account_id, character_id, \
         earned_bargain_slots, oath_bargain_version) VALUES ($1, $2, $3, 0, 1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ash_wallets (namespace_id, account_id, balance, wallet_version) \
         VALUES ($1, $2, 0, 1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn lower_fixture_to_level_four(persistence: &PostgresPersistence, ids: FixtureIds) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "UPDATE characters SET level = 4 WHERE namespace_id = $1 AND account_id = $2 \
         AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_progression SET total_xp = 450, level = 4, current_health = 128 \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

fn b3_handoff(run_ordinal: u32) -> sim_content::CoreB3RewardHandoff {
    sim_content::CoreB3RewardHandoff {
        activation_ordinal: 1,
        instance_id: SpawnInstanceId {
            run_ordinal,
            spawn_ordinal: 76,
        },
        actor_id: EntityId::new(1_000).unwrap(),
        participant_id: EntityId::new(9_000).unwrap(),
        death_tick: Tick(1_000),
        reward_due_tick: Tick(1_008),
        reward_profile_id: "reward.miniboss_t1".into(),
        xp_profile_id: "xp.miniboss_t1".into(),
        active_ticks: 120,
        present_ticks: 120,
        direct_damage: 1_600,
        reference_health: 1_600,
        longest_inactivity_ticks: 0,
        life_state: RewardLifeState::Living,
        recall_state: RewardRecallState::Eligible,
        trust_state: RewardTrustState::Valid,
    }
}

fn b3_coordinator(persistence: &PostgresPersistence) -> PostgresCoreB3RewardCoordinator {
    PostgresCoreB3RewardCoordinator::load(
        persistence.clone(),
        &content_root(),
        SecretRewardEpoch::new("core-b3-hosted-v1", [0x5a; 32]).unwrap(),
    )
    .unwrap()
}

fn granted_b3(resolution: CoreDurableB3Resolution) -> CoreDurableB3RewardCommit {
    match resolution {
        CoreDurableB3Resolution::Granted(commit) => commit,
        CoreDurableB3Resolution::Ineligible(_) => panic!("fixture must be reward eligible"),
    }
}

async fn assert_b3_reward_items(
    persistence: &PostgresPersistence,
    ids: FixtureIds,
    reward_event_id: [u8; 16],
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let reward_items = sqlx::query(
        "SELECT item_uid, creation_request_id, item_kind, item_level, rarity, security_state, \
                location_kind, provenance_kind FROM item_instances WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3 AND creation_kind = 1 \
         AND creation_request_id = $4 ORDER BY roll_index, unit_ordinal",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .bind(reward_event_id.as_slice())
    .fetch_all(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert!((1..=2).contains(&reward_items.len()));
    let item_ids = reward_items
        .iter()
        .map(|row| row.get::<Vec<u8>, _>("item_uid"))
        .collect::<BTreeSet<_>>();
    assert_eq!(item_ids.len(), reward_items.len());
    for row in reward_items {
        assert_eq!(
            row.get::<Vec<u8>, _>("creation_request_id"),
            reward_event_id
        );
        assert_eq!(row.get::<i16, _>("item_kind"), 0);
        assert!((1..=10).contains(&row.get::<i16, _>("item_level")));
        assert_eq!(row.get::<i16, _>("rarity"), 1);
        assert_eq!(row.get::<i16, _>("security_state"), 2);
        assert!(matches!(row.get::<i16, _>("location_kind"), 2 | 3));
        assert_eq!(row.get::<i16, _>("provenance_kind"), 1);
    }
}

async fn assert_b3_no_offer_disposition(
    persistence: &PostgresPersistence,
    ids: FixtureIds,
    reward_event_id: [u8; 16],
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let disposition: (i16, i64, i64, i64) = sqlx::query_as(
        "SELECT result_code, pre_oath_bargain_version, post_oath_bargain_version, \
                (SELECT count(*) FROM bargain_offers WHERE namespace_id = $1 AND account_id = $2) \
         FROM bargain_milestone_results WHERE namespace_id = $1 AND account_id = $2 \
         AND source_reward_event_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(reward_event_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(disposition, (3, 1, 1, 0));
}

#[allow(
    clippy::too_many_lines,
    reason = "the hosted fixture keeps the complete V3 entry graph and Bargain binding auditable"
)]
async fn begin_core_danger_entry(
    persistence: &PostgresPersistence,
    restores: &PostgresProgressionRestoreProvider,
    ids: FixtureIds,
) {
    let world = sim_content::load_core_development_world_flow(&content_root()).unwrap();
    let hashes = world.hashes();
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO character_instance_lineages (namespace_id, account_id, character_id, \
         lineage_id, content_id, layout_id, lineage_state, records_blake3, assets_blake3, \
         localization_blake3) VALUES ($1, $2, $3, $4, 'world.core_microrealm_01', $5, 0, $6, $7, $8)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .bind(ids.lineage.as_slice())
    .bind(CORE_LAYOUT_ID)
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
         3, 1, 1, 1, 1, 1, 1, 1, 31, $6, 0, $7, $8, $9)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .bind(ids.restore.as_slice())
    .bind(ids.lineage.as_slice())
    .bind([91_u8; 32].as_slice())
    .bind(&hashes.records_blake3)
    .bind(&hashes.assets_blake3)
    .bind(&hashes.localization_blake3)
    .execute(transaction.connection())
    .await
    .unwrap();
    restores
        .capture(
            &mut transaction,
            EntryCaptureContext {
                account_id: ids.account,
                character_id: ids.character,
                restore_point_id: ids.restore,
                mutation_id: [9; 16],
                safe_placement_count: 0,
            },
        )
        .await
        .unwrap();
    let inventory = persistence::stage_danger_entry_inventory_restore_v3(
        &mut transaction,
        ids.account,
        ids.character,
        ids.restore,
        [9; 16],
        0,
    )
    .await
    .unwrap();
    let oath = persistence::stage_danger_entry_oath_bargain_restore_v3(
        &mut transaction,
        ids.account,
        ids.character,
        ids.restore,
    )
    .await
    .unwrap();
    let life = persistence::stage_danger_entry_life_metrics_restore_v3(
        &mut transaction,
        ids.account,
        ids.character,
        ids.restore,
    )
    .await
    .unwrap();
    let ash = persistence::stage_danger_entry_ash_wallet_restore_v3(
        &mut transaction,
        ids.account,
        ids.character,
        ids.restore,
    )
    .await
    .unwrap();
    assert_eq!(inventory.post_inventory_version, 1);
    assert_eq!(oath.oath_bargain_version, 1);
    assert_eq!(life.life_metrics_version, 1);
    assert_eq!(ash.ash_wallet_version, 1);
    sqlx::query(
        "UPDATE characters SET character_state_version = 2 WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_world_locations SET character_version = 2, location_kind = 2, \
         location_content_id = 'world.core_microrealm_01', safe_arrival_kind = NULL, \
         instance_lineage_id = $1, entry_restore_point_id = $2 WHERE namespace_id = $3 \
         AND account_id = $4 AND character_id = $5",
    )
    .bind(ids.lineage.as_slice())
    .bind(ids.restore.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

fn sepulcher_award(ids: FixtureIds, progression_revision: ManifestHash) -> ProgressionAwardCommand {
    let payload = ProgressionAwardPayload {
        character_id: ids.character,
        expected_progression_version: 1,
        source_content_id: CORE_SOURCE_ID.to_owned(),
        progression_content_revision: progression_revision,
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
        reward_event_id: ids.reward,
        payload_hash: payload.canonical_hash(),
        payload,
    }
}

fn view_frame(
    ids: FixtureIds,
    sequence: u32,
    revision: BargainContentRevisionV1,
) -> BargainViewFrame {
    BargainViewFrame {
        sequence,
        character_id: ids.character,
        content_revision: revision,
    }
}

fn decision_frame(
    ids: FixtureIds,
    expected_version: u64,
    decision: BargainDecision,
    revision: BargainContentRevisionV1,
) -> BargainDecisionFrame {
    let payload = BargainDecisionPayload {
        character_id: ids.character,
        offer_id: ids.reward,
        decision,
        content_revision: revision,
        confirmed: true,
    };
    BargainDecisionFrame {
        mutation_id: ids.mutation,
        expected_oath_bargain_version: expected_version,
        payload_hash: payload.canonical_hash(),
        issued_at_unix_millis: 9_999,
        payload,
    }
}

async fn create_offer(
    persistence: &PostgresPersistence,
    ids: FixtureIds,
) -> (
    sim_content::CompiledOathBargainCatalog,
    BargainContentRevisionV1,
) {
    reset_level_five_fixture(persistence, ids).await;
    let progression = sim_content::load_core_development_progression(&content_root()).unwrap();
    let oath_bargain = sim_content::load_core_development_oaths_bargains(&content_root()).unwrap();
    let restores = PostgresProgressionRestoreProvider::new(&progression).unwrap();
    begin_core_danger_entry(persistence, &restores, ids).await;
    let progression_revision =
        ManifestHash::new(progression.hashes().records_blake3.clone()).unwrap();
    let awards =
        PostgresProgressionAwardService::new(persistence.clone(), &progression, &oath_bargain)
            .unwrap();
    let award = sepulcher_award(ids, progression_revision);
    let (first, replay) = tokio::join!(
        awards.award(authenticated(ids), &award),
        awards.award(authenticated(ids), &award)
    );
    assert_eq!(first, replay);
    assert_eq!(first.code, ProgressionAwardCode::Accepted);
    assert_eq!(first.applied_xp, 120);
    assert_eq!(first.projection.as_ref().unwrap().level, 5);
    let authority = awards
        .award_with_milestone(authenticated(ids), &award)
        .await;
    assert_eq!(authority.outcome, first);
    let milestone = authority
        .bargain_milestone
        .expect("replayed progression returns immutable milestone");
    assert_eq!(milestone.source_reward_event_id, ids.reward);
    assert_eq!(milestone.character_id, ids.character);
    assert_eq!(milestone.instance_lineage_id, ids.lineage);
    assert_eq!(milestone.payload_hash, award.payload_hash);
    assert_eq!(milestone.result_code, 0);
    assert_eq!(milestone.offer_id, Some(ids.reward));

    let revision = bargain_revision(&oath_bargain);
    (oath_bargain, revision)
}

#[allow(clippy::too_many_lines)] // One read transaction proves the cross-domain atomic commit.
async fn assert_open_offer_rows(
    persistence: &PostgresPersistence,
    ids: FixtureIds,
    content: &sim_content::CompiledOathBargainCatalog,
) -> Vec<(i16, String, Vec<u8>)> {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let progression: (i32, i16, i64) = sqlx::query_as(
        "SELECT total_xp, level, progression_version FROM character_progression \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let life: (i16, i64) = sqlx::query_as(
        "SELECT earned_bargain_slots, oath_bargain_version FROM character_oath_bargain_state \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let milestone = sqlx::query(
        "SELECT count(*) OVER () AS row_count, result_code, pre_oath_bargain_version, \
         post_oath_bargain_version, pre_earned_bargain_slots, post_earned_bargain_slots, \
         offer_id, ash_mutation_id, milestone_id, source_content_id, source_layout_id, \
         instance_lineage_id, entry_restore_point_id FROM bargain_milestone_results \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let offer = sqlx::query(
        "SELECT count(*) OVER () AS row_count, offer_id, source_reward_event_id, \
         source_content_id, source_layout_id, instance_lineage_id, entry_restore_point_id, \
         content_version, records_blake3, assets_blake3, localization_blake3, offer_state, \
         selected_bargain_id, created_oath_bargain_version, resolved_oath_bargain_version \
         FROM bargain_offers WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let candidates = sqlx::query_as::<_, (i16, String, Vec<u8>)>(
        "SELECT candidate_ordinal, bargain_id, score FROM bargain_offer_candidates \
         WHERE namespace_id = $1 AND account_id = $2 AND offer_id = $3 ORDER BY candidate_ordinal",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.reward.as_slice())
    .fetch_all(transaction.connection())
    .await
    .unwrap();
    let xp_receipts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM character_xp_award_results WHERE namespace_id = $1 \
         AND account_id = $2 AND reward_event_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.reward.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let ash_results: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ash_mutation_results WHERE namespace_id = $1 \
         AND account_id = $2 AND mutation_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.reward.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let offered_event: (i64, Vec<u8>) = sqlx::query_as(
        "SELECT aggregate_version, event_payload FROM character_life_outbox \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3 \
         AND event_type = 'bargain_offered'",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();

    assert_eq!(progression, (820, 5, 2));
    assert_eq!(life, (1, 2));
    assert_eq!(milestone.get::<i64, _>("row_count"), 1);
    assert_eq!(milestone.get::<i16, _>("result_code"), 0);
    assert_eq!(milestone.get::<i64, _>("pre_oath_bargain_version"), 1);
    assert_eq!(milestone.get::<i64, _>("post_oath_bargain_version"), 2);
    assert_eq!(milestone.get::<i16, _>("pre_earned_bargain_slots"), 0);
    assert_eq!(milestone.get::<i16, _>("post_earned_bargain_slots"), 1);
    assert_eq!(milestone.get::<Vec<u8>, _>("offer_id"), ids.reward);
    assert!(
        milestone
            .get::<Option<Vec<u8>>, _>("ash_mutation_id")
            .is_none()
    );
    assert_eq!(
        milestone.get::<String, _>("milestone_id"),
        "milestone.core.sepulcher_knight_first_clear"
    );
    assert_eq!(
        milestone.get::<String, _>("source_content_id"),
        CORE_SOURCE_ID
    );
    assert_eq!(
        milestone.get::<String, _>("source_layout_id"),
        CORE_LAYOUT_ID
    );
    assert_eq!(
        milestone.get::<Vec<u8>, _>("instance_lineage_id"),
        ids.lineage
    );
    assert_eq!(
        milestone.get::<Vec<u8>, _>("entry_restore_point_id"),
        ids.restore
    );

    let hashes = content.hashes();
    let content_version = format!("core-dev.blake3.{}", hashes.manifest_blake3);
    assert_eq!(offer.get::<i64, _>("row_count"), 1);
    assert_eq!(offer.get::<Vec<u8>, _>("offer_id"), ids.reward);
    assert_eq!(
        offer.get::<Vec<u8>, _>("source_reward_event_id"),
        ids.reward
    );
    assert_eq!(offer.get::<String, _>("source_content_id"), CORE_SOURCE_ID);
    assert_eq!(offer.get::<String, _>("source_layout_id"), CORE_LAYOUT_ID);
    assert_eq!(offer.get::<Vec<u8>, _>("instance_lineage_id"), ids.lineage);
    assert_eq!(
        offer.get::<Vec<u8>, _>("entry_restore_point_id"),
        ids.restore
    );
    assert_eq!(offer.get::<String, _>("content_version"), content_version);
    assert_eq!(
        offer.get::<String, _>("records_blake3"),
        hashes.records_blake3
    );
    assert_eq!(
        offer.get::<String, _>("assets_blake3"),
        hashes.assets_blake3
    );
    assert_eq!(
        offer.get::<String, _>("localization_blake3"),
        hashes.localization_blake3
    );
    assert_eq!(offer.get::<i16, _>("offer_state"), 0);
    assert!(
        offer
            .get::<Option<String>, _>("selected_bargain_id")
            .is_none()
    );
    assert_eq!(offer.get::<i64, _>("created_oath_bargain_version"), 2);
    assert!(
        offer
            .get::<Option<i64>, _>("resolved_oath_bargain_version")
            .is_none()
    );
    assert_eq!(xp_receipts, 1);
    assert_eq!(ash_results, 0);
    assert_eq!(offered_event.0, 2);
    let offered_event = BargainOfferedEventV1::decode(&offered_event.1).unwrap();
    assert_eq!(offered_event.offer_id, ids.reward);
    assert_eq!(offered_event.source_reward_event_id, ids.reward);
    assert_eq!(offered_event.source_content_id, CORE_SOURCE_ID);
    assert_eq!(offered_event.source_layout_id, CORE_LAYOUT_ID);
    assert_eq!(offered_event.instance_lineage_id, ids.lineage);
    assert_eq!(offered_event.entry_restore_point_id, ids.restore);
    assert_eq!(offered_event.content_version, content_version);
    assert_eq!(offered_event.records_blake3, hashes.records_blake3);
    assert_eq!(offered_event.assets_blake3, hashes.assets_blake3);
    assert_eq!(
        offered_event.localization_blake3,
        hashes.localization_blake3
    );
    assert_eq!(offered_event.oath_bargain_version, 2);

    let enabled = content
        .bargains()
        .values()
        .filter(|record| record.header.enabled)
        .map(|record| record.header.id.as_str())
        .collect::<Vec<_>>();
    let expected =
        sim_core::plan_bargain_offer(ids.reward, ids.character, &content_version, &enabled)
            .unwrap();
    assert_eq!(candidates.len(), 3);
    assert_eq!(offered_event.candidates.len(), candidates.len());
    for (index, ((ordinal, bargain_id, score), expected)) in
        candidates.iter().zip(&expected).enumerate()
    {
        assert_eq!(*ordinal, i16::try_from(index).unwrap());
        assert_eq!(bargain_id, &expected.bargain_id);
        assert_eq!(score.as_slice(), expected.score);
        assert_eq!(offered_event.candidates[index].candidate_ordinal, *ordinal);
        assert_eq!(offered_event.candidates[index].bargain_id, *bargain_id);
        assert_eq!(offered_event.candidates[index].score.as_slice(), score);
    }
    candidates
}

async fn assert_declined_event(
    persistence: &PostgresPersistence,
    ids: FixtureIds,
    content: &sim_content::CompiledOathBargainCatalog,
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let rows: Vec<(i64, Vec<u8>)> = sqlx::query_as(
        "SELECT aggregate_version, event_payload FROM character_life_outbox \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3 \
         AND event_type = 'bargain_declined'",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .fetch_all(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, 2);
    let event = BargainDeclinedEventV1::decode(&rows[0].1).unwrap();
    assert_eq!(event.mutation_id, ids.mutation);
    assert_eq!(event.offer_id, ids.reward);
    assert_eq!(event.oath_bargain_version, 2);
    assert_eq!(event.source_content_id, CORE_SOURCE_ID);
    assert_eq!(event.source_layout_id, CORE_LAYOUT_ID);
    assert_eq!(event.instance_lineage_id, ids.lineage);
    assert_eq!(event.entry_restore_point_id, ids.restore);
    let hashes = content.hashes();
    assert_eq!(event.records_blake3, hashes.records_blake3);
    assert_eq!(event.assets_blake3, hashes.assets_blake3);
    assert_eq!(event.localization_blake3, hashes.localization_blake3);
    assert_eq!(event.candidates.len(), 3);
}

async fn assert_terminal_rows(
    persistence: &PostgresPersistence,
    ids: FixtureIds,
    expected: TerminalExpectation<'_>,
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let life: (i16, i64) = sqlx::query_as(
        "SELECT earned_bargain_slots, oath_bargain_version FROM character_oath_bargain_state \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let offer: (i16, Option<String>, Option<i64>) = sqlx::query_as(
        "SELECT offer_state, selected_bargain_id, resolved_oath_bargain_version FROM bargain_offers \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3 AND offer_id = $4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .bind(ids.reward.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM character_active_bargains WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let outbox: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM character_life_outbox WHERE namespace_id = $1 AND account_id = $2 \
         AND character_id = $3 AND event_type = 'bargain_selected'",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let receipts = sqlx::query(
        "SELECT count(*) OVER () AS row_count, result_payload FROM bargain_decision_results \
         WHERE namespace_id = $1 AND account_id = $2 AND mutation_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.mutation.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();

    assert_eq!(life, (1, expected.version));
    assert_eq!(offer.0, expected.offer_state);
    assert_eq!(offer.1.as_deref(), expected.selected);
    assert_eq!(offer.2, Some(expected.version));
    assert_eq!(active, expected.active_count);
    assert_eq!(outbox, expected.outbox_count);
    assert_eq!(receipts.get::<i64, _>("row_count"), 1);
    assert_eq!(
        receipts.get::<Vec<u8>, _>("result_payload"),
        expected.payload
    );
}

async fn assert_life_cleanup_participant(
    persistence: &PostgresPersistence,
    ids: FixtureIds,
    selected_id: &str,
) {
    let command = BargainLifeCleanupCommand {
        account_id: ids.account,
        character_id: ids.character,
        event_id: [91; 16],
        reason: BargainLifeEndReason::Death,
        expected_oath_bargain_version: 3,
    };
    let mut stale = command.clone();
    stale.expected_oath_bargain_version = 2;
    let mut transaction = persistence.begin_transaction().await.unwrap();
    assert!(matches!(
        cleanup_bargains_for_life_end(&mut transaction, &stale).await,
        Err(PersistenceError::BargainCleanupVersionMismatch)
    ));
    transaction.rollback().await.unwrap();

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let result = cleanup_bargains_for_life_end(&mut transaction, &command)
        .await
        .unwrap();
    assert_eq!(result.pre_oath_bargain_version, 3);
    assert_eq!(result.post_oath_bargain_version, 4);
    assert!(!result.removed_danger_checkpoint);
    assert_eq!(result.active_bargains.len(), 1);
    assert_eq!(result.active_bargains[0].bargain_id, selected_id);
    assert_eq!(result.active_bargains[0].acquisition_ordinal, 1);
    let acquired_by_offer_id = result.active_bargains[0].acquired_by_offer_id;
    let expected_event_payload = result.event_payload.clone();
    transaction.commit().await.unwrap();

    let mut verification = persistence.begin_transaction().await.unwrap();
    let life: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT ob.oath_bargain_version, \
         (SELECT count(*) FROM character_active_bargains ab WHERE ab.namespace_id = ob.namespace_id \
          AND ab.account_id = ob.account_id AND ab.character_id = ob.character_id), \
         (SELECT count(*) FROM bargain_offers bo WHERE bo.namespace_id = ob.namespace_id \
          AND bo.account_id = ob.account_id AND bo.character_id = ob.character_id), \
         (SELECT count(*) FROM bargain_decision_results br WHERE br.namespace_id = ob.namespace_id \
          AND br.account_id = ob.account_id AND br.character_id = ob.character_id) \
         FROM character_oath_bargain_state ob WHERE ob.namespace_id = $1 AND ob.account_id = $2 \
         AND ob.character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    let cleanup_event: (String, i64, Vec<u8>) = sqlx::query_as(
        "SELECT event_type, aggregate_version, event_payload FROM character_life_outbox \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3 \
         AND event_type = 'bargains_cleared_death'",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ids.account.as_slice())
    .bind(ids.character.as_slice())
    .fetch_one(verification.connection())
    .await
    .unwrap();
    verification.rollback().await.unwrap();
    assert_eq!(life, (4, 0, 1, 1));
    let decoded_event = BargainLifeCleanupEventV1::decode(&cleanup_event.2).unwrap();
    assert_eq!(decoded_event.reason, BargainLifeEndReason::Death);
    assert_eq!(decoded_event.pre_oath_bargain_version, 3);
    assert_eq!(decoded_event.post_oath_bargain_version, 4);
    assert_eq!(decoded_event.active_bargains.len(), 1);
    assert_eq!(decoded_event.active_bargains[0].bargain_id, selected_id);
    assert_eq!(decoded_event.active_bargains[0].acquisition_ordinal, 1);
    assert_eq!(
        decoded_event.active_bargains[0].acquired_by_offer_id,
        acquired_by_offer_id
    );
    assert_eq!(
        cleanup_event,
        (
            "bargains_cleared_death".to_owned(),
            4,
            expected_event_payload
        )
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn postgres_b3_coordinator_commits_reward_progression_and_milestone_then_replays() {
    let persistence = disposable_database().await;
    reset_level_five_fixture(&persistence, B3_FIXTURE).await;
    let progression_content =
        sim_content::load_core_development_progression(&content_root()).unwrap();
    let restores = PostgresProgressionRestoreProvider::new(&progression_content).unwrap();
    begin_core_danger_entry(&persistence, &restores, B3_FIXTURE).await;
    let coordinator = b3_coordinator(&persistence);
    let handoff = b3_handoff(7);
    let first = granted_b3(
        coordinator
            .commit(
                authenticated(B3_FIXTURE),
                B3_FIXTURE.character,
                B3_FIXTURE.lineage,
                1_008,
                &handoff,
            )
            .await
            .unwrap(),
    );
    assert!(!first.reward_replayed());
    assert_eq!(first.progression().code, ProgressionAwardCode::Accepted);
    assert_eq!(first.progression().base_xp, 120);
    assert_eq!(first.bargain_offer_id(), Some(first.reward_event_id()));
    assert!(first.no_offer_resolution().is_none());
    assert_b3_reward_items(&persistence, B3_FIXTURE, first.reward_event_id()).await;

    drop(coordinator);
    let restarted = b3_coordinator(&persistence);
    let replay = granted_b3(
        restarted
            .commit(
                authenticated(B3_FIXTURE),
                B3_FIXTURE.character,
                B3_FIXTURE.lineage,
                1_008,
                &handoff,
            )
            .await
            .unwrap(),
    );
    assert!(replay.reward_replayed());
    assert_eq!(replay.reward_event_id(), first.reward_event_id());
    assert_eq!(replay.source_instance_id(), first.source_instance_id());
    assert_eq!(replay.reward_result_hash(), first.reward_result_hash());
    assert_eq!(
        replay.progression_payload_hash(),
        first.progression_payload_hash()
    );
    assert_eq!(replay.progression(), first.progression());
    assert_eq!(replay.bargain_offer_id(), first.bargain_offer_id());

    reset_level_five_fixture(&persistence, B3_LOW_FIXTURE).await;
    lower_fixture_to_level_four(&persistence, B3_LOW_FIXTURE).await;
    begin_core_danger_entry(&persistence, &restores, B3_LOW_FIXTURE).await;
    let low_handoff = b3_handoff(8);
    let low = granted_b3(
        restarted
            .commit(
                authenticated(B3_LOW_FIXTURE),
                B3_LOW_FIXTURE.character,
                B3_LOW_FIXTURE.lineage,
                1_008,
                &low_handoff,
            )
            .await
            .unwrap(),
    );
    assert_eq!(low.progression().code, ProgressionAwardCode::Accepted);
    assert_eq!(low.progression().base_xp, 120);
    assert!(low.bargain_offer_id().is_none());
    assert_eq!(
        low.no_offer_resolution().unwrap().resolution(),
        sim_content::CoreFixedDungeonRestResolution::NoOffer
    );

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT \
           (SELECT count(*) FROM reward_requests WHERE namespace_id = $1 AND account_id = $2), \
           (SELECT count(*) FROM character_xp_award_results WHERE namespace_id = $1 AND account_id = $2), \
           (SELECT count(*) FROM bargain_milestone_results WHERE namespace_id = $1 AND account_id = $2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(B3_FIXTURE.account.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(counts, (1, 1, 1));

    assert_b3_no_offer_disposition(&persistence, B3_LOW_FIXTURE, low.reward_event_id()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn postgres_b3_ineligible_terminal_grants_nothing_and_replays_without_stranding() {
    let persistence = disposable_database().await;
    let progression_content =
        sim_content::load_core_development_progression(&content_root()).unwrap();
    let restores = PostgresProgressionRestoreProvider::new(&progression_content).unwrap();
    reset_level_five_fixture(&persistence, B3_INELIGIBLE_FIXTURE).await;
    begin_core_danger_entry(&persistence, &restores, B3_INELIGIBLE_FIXTURE).await;
    let coordinator = b3_coordinator(&persistence);
    let mut ineligible_handoff = b3_handoff(9);
    ineligible_handoff.active_ticks = 700;
    ineligible_handoff.present_ticks = 700;
    ineligible_handoff.longest_inactivity_ticks = 601;
    let ineligible = coordinator
        .commit(
            authenticated(B3_INELIGIBLE_FIXTURE),
            B3_INELIGIBLE_FIXTURE.character,
            B3_INELIGIBLE_FIXTURE.lineage,
            1_008,
            &ineligible_handoff,
        )
        .await
        .unwrap();
    assert!(matches!(ineligible, CoreDurableB3Resolution::Ineligible(_)));
    assert_eq!(
        ineligible.progression().code,
        ProgressionAwardCode::NotEligible
    );
    assert!(ineligible.reward_result_hash().is_none());
    assert!(ineligible.bargain_offer_id().is_none());
    let replay = coordinator
        .commit(
            authenticated(B3_INELIGIBLE_FIXTURE),
            B3_INELIGIBLE_FIXTURE.character,
            B3_INELIGIBLE_FIXTURE.lineage,
            1_008,
            &ineligible_handoff,
        )
        .await
        .unwrap();
    assert_eq!(replay, ineligible);

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let ineligible_counts: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT \
           (SELECT count(*) FROM reward_requests WHERE namespace_id = $1 AND account_id = $2), \
           (SELECT count(*) FROM character_xp_award_results WHERE namespace_id = $1 AND account_id = $2), \
           (SELECT count(*) FROM bargain_milestone_results WHERE namespace_id = $1 AND account_id = $2), \
           (SELECT count(*) FROM item_instances WHERE namespace_id = $1 AND account_id = $2 \
            AND creation_kind = 1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(B3_INELIGIBLE_FIXTURE.account.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(ineligible_counts, (0, 1, 0, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
#[allow(clippy::too_many_lines)] // The restart boundary and its before/after invariants stay visible.
async fn postgres_bargain_selection_is_atomic_concurrent_replay_safe_and_restart_durable() {
    let persistence = disposable_database().await;
    let (content, revision) = create_offer(&persistence, SELECT_FIXTURE).await;
    let candidates = assert_open_offer_rows(&persistence, SELECT_FIXTURE, &content).await;
    let service = PostgresBargainService::new(persistence.clone(), FixedClock, &content).unwrap();
    let available = service
        .view(
            authenticated(SELECT_FIXTURE),
            &view_frame(SELECT_FIXTURE, 1, revision.clone()),
        )
        .await;
    assert_eq!(available.code, BargainResultCode::Available);
    let projected_offer = available.projection.unwrap().offer.unwrap();
    assert_eq!(projected_offer.state, BargainOfferState::Open);
    assert_eq!(projected_offer.cells.len(), 3);
    let projected_ids = projected_offer
        .cells
        .iter()
        .map(|cell| match cell {
            BargainOfferCell::Available { bargain_id, .. } => bargain_id.as_str(),
            BargainOfferCell::Unavailable => panic!("Core offer must have three candidates"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        projected_ids,
        candidates
            .iter()
            .map(|(_, bargain_id, _)| bargain_id.as_str())
            .collect::<Vec<_>>()
    );
    let selected_id = candidates[0].1.clone();
    let frame = decision_frame(
        SELECT_FIXTURE,
        2,
        BargainDecision::Select {
            bargain_id: WireText::new(selected_id.clone()).unwrap(),
        },
        revision.clone(),
    );
    let (first, replay) = tokio::join!(
        service.decide(authenticated(SELECT_FIXTURE), &frame),
        service.decide(authenticated(SELECT_FIXTURE), &frame)
    );
    assert_eq!(first, replay);
    assert_eq!(first.code, BargainResultCode::Accepted);
    let payload = postcard::to_stdvec(&first).unwrap();
    assert_terminal_rows(
        &persistence,
        SELECT_FIXTURE,
        TerminalExpectation {
            offer_state: 1,
            selected: Some(&selected_id),
            version: 3,
            active_count: 1,
            outbox_count: 1,
            payload: &payload,
        },
    )
    .await;
    let conflict = decision_frame(SELECT_FIXTURE, 2, BargainDecision::Refuse, revision.clone());
    assert_eq!(
        service
            .decide(authenticated(SELECT_FIXTURE), &conflict)
            .await
            .code,
        BargainResultCode::IdempotencyConflict
    );

    drop(service);
    persistence.close().await;
    let restarted_persistence = PostgresPersistence::connect(&persistence_config())
        .await
        .unwrap();
    let restarted =
        PostgresBargainService::new(restarted_persistence.clone(), FixedClock, &content).unwrap();
    assert_eq!(
        restarted
            .decide(authenticated(SELECT_FIXTURE), &frame)
            .await,
        first
    );
    let durable = restarted
        .view(
            authenticated(SELECT_FIXTURE),
            &view_frame(SELECT_FIXTURE, 2, revision),
        )
        .await;
    assert_eq!(durable.code, BargainResultCode::NoOffer);
    let projection = durable.projection.unwrap();
    assert!(projection.offer.is_none());
    assert_eq!(projection.earned_bargain_slots, 1);
    assert_eq!(projection.oath_bargain_version, 3);
    assert_eq!(
        projection
            .active_bargain_ids
            .iter()
            .map(protocol::WireText::as_str)
            .collect::<Vec<_>>(),
        vec![selected_id.as_str()]
    );
    assert_terminal_rows(
        &restarted_persistence,
        SELECT_FIXTURE,
        TerminalExpectation {
            offer_state: 1,
            selected: Some(&selected_id),
            version: 3,
            active_count: 1,
            outbox_count: 1,
            payload: &payload,
        },
    )
    .await;
    assert_life_cleanup_participant(&restarted_persistence, SELECT_FIXTURE, &selected_id).await;
    restarted_persistence.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn postgres_bargain_refusal_is_concurrent_replay_safe_and_restart_durable() {
    let persistence = disposable_database().await;
    let (content, revision) = create_offer(&persistence, REFUSE_FIXTURE).await;
    assert_open_offer_rows(&persistence, REFUSE_FIXTURE, &content).await;
    let service = PostgresBargainService::new(persistence.clone(), FixedClock, &content).unwrap();
    let frame = decision_frame(REFUSE_FIXTURE, 2, BargainDecision::Refuse, revision.clone());
    let (first, replay) = tokio::join!(
        service.decide(authenticated(REFUSE_FIXTURE), &frame),
        service.decide(authenticated(REFUSE_FIXTURE), &frame)
    );
    assert_eq!(first, replay);
    assert_eq!(first.code, BargainResultCode::Refused);
    let payload = postcard::to_stdvec(&first).unwrap();
    assert_terminal_rows(
        &persistence,
        REFUSE_FIXTURE,
        TerminalExpectation {
            offer_state: 2,
            selected: None,
            version: 2,
            active_count: 0,
            outbox_count: 0,
            payload: &payload,
        },
    )
    .await;
    assert_declined_event(&persistence, REFUSE_FIXTURE, &content).await;
    let conflict = decision_frame(
        REFUSE_FIXTURE,
        2,
        BargainDecision::Select {
            bargain_id: WireText::new("bargain.bell_debt".to_owned()).unwrap(),
        },
        revision.clone(),
    );
    assert_eq!(
        service
            .decide(authenticated(REFUSE_FIXTURE), &conflict)
            .await
            .code,
        BargainResultCode::IdempotencyConflict
    );

    drop(service);
    persistence.close().await;
    let restarted_persistence = PostgresPersistence::connect(&persistence_config())
        .await
        .unwrap();
    let restarted =
        PostgresBargainService::new(restarted_persistence.clone(), FixedClock, &content).unwrap();
    assert_eq!(
        restarted
            .decide(authenticated(REFUSE_FIXTURE), &frame)
            .await,
        first
    );
    let durable = restarted
        .view(
            authenticated(REFUSE_FIXTURE),
            &view_frame(REFUSE_FIXTURE, 2, revision),
        )
        .await;
    assert_eq!(durable.code, BargainResultCode::NoOffer);
    let projection = durable.projection.unwrap();
    assert!(projection.offer.is_none());
    assert_eq!(projection.earned_bargain_slots, 1);
    assert_eq!(projection.oath_bargain_version, 2);
    assert!(projection.active_bargain_ids.is_empty());
    assert_terminal_rows(
        &restarted_persistence,
        REFUSE_FIXTURE,
        TerminalExpectation {
            offer_state: 2,
            selected: None,
            version: 2,
            active_count: 0,
            outbox_count: 0,
            payload: &payload,
        },
    )
    .await;
    assert_declined_event(&restarted_persistence, REFUSE_FIXTURE, &content).await;
    restarted_persistence.close().await;
}
