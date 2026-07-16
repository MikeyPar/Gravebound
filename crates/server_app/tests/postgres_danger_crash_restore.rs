//! Hosted `PostgreSQL` proof for the complete danger crash-restore coordinator.
//!
//! Authorities: canonical GDD TECH-015/020/021/023, Content CONT-014/CONT-HUB-002,
//! Development Roadmap GB-M03-02/06/08, and accepted SPEC-CONFLICT-027/028.

use std::path::PathBuf;

use persistence::{
    AshMutationKind, AshMutationRequest, AshWalletTransaction, CORE_ITEM_CONTENT_REVISION,
    CORE_PROGRESSION_RECORDS_BLAKE3, CORE_WORLD_ASSETS_BLAKE3, CORE_WORLD_LOCALIZATION_BLAKE3,
    CORE_WORLD_RECORDS_BLAKE3, DangerCrashItemChangeKind, DangerCrashRestoreCode,
    DangerCrashRestoreRequest, DangerCrashRestoreTransaction, LifeClockCheckpointCommandV1,
    LifeClockCheckpointRequestV1, LifeClockCheckpointTransactionV1, LifeClockContentAuthorityV1,
    LifeClockDangerAuthorityV1, LifeClockStateV1, LifeDeedCompletionCommandV2,
    LifeDeedCompletionRequestV2, LifeDeedCompletionTransactionV2, LifeDeedContentAuthorityV2,
    PRODUCTION_RECALL_CONTRACT_VERSION_V1, PersistenceConfig, PersistenceError,
    PostgresPersistence, ProductionRecallCommitRequestV1, ProductionRecallExpectedVersionsV1,
    ProductionRecallTransactionV1, ProductionRecallTriggerV1, StoredDangerCheckpoint,
    StoredWorldFlowRevisionV1, WIPEABLE_CORE_NAMESPACE,
};
use protocol::{
    ManifestHash, WireText, WorldFlowContentRevisionV1, WorldFlowFrame, WorldFlowRequest,
    WorldFlowResult, WorldTransferCommand, WorldTransferMutation, WorldTransferPayload,
    WorldTransferResultCode,
};
use server_app::{
    AccountId, AuthenticatedAccount, AuthenticatedNamespace, IdentityClock,
    PostgresDangerEntryAshWalletProviderV3, PostgresDangerEntryInventoryProviderV3,
    PostgresDangerEntryLifeMetricsProviderV3, PostgresDangerEntryOathBargainProviderV3,
    PostgresDormantWorldFlowCoordinator, PostgresProgressionRestoreProvider, WorldFlowIdGenerator,
};

const ACCOUNT_ID: [u8; 16] = [141; 16];
const CHARACTER_ID: [u8; 16] = [142; 16];
const FIRST_TRANSFER_ID: [u8; 16] = [143; 16];
const FIRST_LINEAGE_ID: [u8; 16] = [144; 16];
const FIRST_RESTORE_ID: [u8; 16] = [145; 16];
const SECOND_TRANSFER_ID: [u8; 16] = [146; 16];
const SECOND_LINEAGE_ID: [u8; 16] = [147; 16];
const SECOND_RESTORE_ID: [u8; 16] = [148; 16];
const HALL_ID: &str = "hub.lantern_halls_01";

fn hash(seed: u8) -> [u8; 32] {
    [seed; 32]
}

const ENTRY_EQUIPMENT: [u8; 16] = [151; 16];
const ENTRY_BELT: [u8; 16] = [152; 16];
const ENTRY_BACKPACK: [u8; 16] = [153; 16];
const FIELD_REPLACEMENT: [u8; 16] = [154; 16];
const POST_ENTRY_BACKPACK: [u8; 16] = [155; 16];

type StoredItemProjection = (Vec<u8>, i16, i16, Option<i16>, Option<String>);

#[derive(Debug, Clone, Copy)]
struct FixedIds {
    transfer: [u8; 16],
    lineage: [u8; 16],
    restore: [u8; 16],
}

impl WorldFlowIdGenerator for FixedIds {
    fn next_transfer_id(&self) -> [u8; 16] {
        self.transfer
    }

    fn next_lineage_id(&self) -> [u8; 16] {
        self.lineage
    }

    fn next_restore_point_id(&self) -> [u8; 16] {
        self.restore
    }
}

#[derive(Debug, Clone, Copy)]
struct FixedClock;

impl IdentityClock for FixedClock {
    fn unix_millis(&self) -> u64 {
        10_000
    }
}

fn content_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content")
}

fn revision() -> WorldFlowContentRevisionV1 {
    let world = sim_content::load_core_development_world_flow(&content_root()).unwrap();
    let hashes = world.hashes();
    WorldFlowContentRevisionV1 {
        records_blake3: ManifestHash::new(hashes.records_blake3.clone()).unwrap(),
        assets_blake3: ManifestHash::new(hashes.assets_blake3.clone()).unwrap(),
        localization_blake3: ManifestHash::new(hashes.localization_blake3.clone()).unwrap(),
    }
}

fn authenticated() -> AuthenticatedAccount {
    AuthenticatedAccount {
        account_id: AccountId::new(ACCOUNT_ID).unwrap(),
        namespace: AuthenticatedNamespace::WipeableTest,
    }
}

fn coordinator(
    persistence: PostgresPersistence,
    ids: FixedIds,
) -> PostgresDormantWorldFlowCoordinator<
    FixedIds,
    FixedClock,
    PostgresDangerEntryInventoryProviderV3,
    PostgresDangerEntryOathBargainProviderV3,
    PostgresDangerEntryLifeMetricsProviderV3,
    PostgresDangerEntryAshWalletProviderV3,
> {
    let progression = sim_content::load_core_development_progression(&content_root()).unwrap();
    PostgresDormantWorldFlowCoordinator::new(
        persistence,
        ids,
        FixedClock,
        revision(),
        PostgresProgressionRestoreProvider::new(&progression).unwrap(),
        PostgresDangerEntryInventoryProviderV3,
        PostgresDangerEntryOathBargainProviderV3,
        PostgresDangerEntryLifeMetricsProviderV3,
        PostgresDangerEntryAshWalletProviderV3,
    )
}

fn entry_frame(mutation: u8, character_version: u64) -> WorldFlowFrame {
    let payload = WorldTransferPayload {
        content_revision: revision(),
        command: WorldTransferCommand::UsePortal {
            portal_id: WireText::new("station.realm_gate").unwrap(),
        },
    };
    WorldFlowFrame {
        sequence: u32::from(mutation),
        request: WorldFlowRequest::Transfer(WorldTransferMutation {
            mutation_id: [mutation; 16],
            character_id: CHARACTER_ID,
            expected_character_version: character_version,
            issued_at_unix_millis: 9_000,
            payload_hash: payload.canonical_hash(),
            payload,
        }),
    }
}

fn transfer_code(result: &WorldFlowResult) -> WorldTransferResultCode {
    match result {
        WorldFlowResult::Transfer { code, .. } | WorldFlowResult::Error { code, .. } => *code,
        WorldFlowResult::Location { .. } => panic!("unexpected location projection"),
    }
}

fn crash_request(restore_point_id: [u8; 16], mutation_id: [u8; 16]) -> DangerCrashRestoreRequest {
    let mut request = DangerCrashRestoreRequest {
        account_id: ACCOUNT_ID,
        character_id: CHARACTER_ID,
        restore_point_id,
        mutation_id,
        request_hash: [0; 32],
    };
    request.request_hash = request.expected_request_hash();
    request
}

async fn disposable_database() -> PostgresPersistence {
    let config = PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL");
    let persistence = PostgresPersistence::connect(&config).await.unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence.migrate().await.unwrap();
    persistence
}

#[allow(
    clippy::too_many_lines,
    reason = "fixture builds one complete restore aggregate"
)]
async fn reset_fixture(persistence: &PostgresPersistence) {
    // Recall, extraction, and final-death histories are deliberately row-immutable. Integration
    // isolation must therefore use the opt-in disposable-database reset rather than cascading an
    // account DELETE through those production guards.
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
        "INSERT INTO character_world_locations (namespace_id,account_id,character_id, \
         character_version,location_kind,location_content_id,safe_arrival_kind) \
         VALUES ($1,$2,$3,1,1,$4,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(HALL_ID)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_progression (namespace_id,account_id,character_id,total_xp,level, \
         current_health,progression_version) VALUES ($1,$2,$3,0,1,120,1)",
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
    let life_metrics_updated = sqlx::query(
        "UPDATE character_life_metrics SET lifetime_ticks=100,permadeath_combat_ticks=40 \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 \
           AND life_metrics_version=1",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(life_metrics_updated, 1);
    transaction.commit().await.unwrap();
}

#[allow(
    clippy::too_many_arguments,
    reason = "the fixture inserts one complete authoritative item projection"
)]
async fn insert_item(
    persistence: &PostgresPersistence,
    item_uid: [u8; 16],
    template_id: &str,
    item_kind: i16,
    security_state: i16,
    location_kind: i16,
    slot_index: Option<i16>,
    item_version: i64,
) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "INSERT INTO item_instances (namespace_id,item_uid,account_id,character_id,template_id, \
         content_revision,item_kind,item_level,rarity,creation_kind,creation_request_id,roll_index, \
         unit_ordinal,item_version,security_state,location_kind,slot_index,provenance_kind, \
         salvage_band,salvage_value) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,0,$2,0,0,$10,$11,$12,$13,0,0,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(item_uid.as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(template_id)
    .bind(CORE_ITEM_CONTENT_REVISION)
    .bind(item_kind)
    .bind((item_kind == 0).then_some(1_i16))
    .bind((item_kind == 0).then_some(0_i16))
    .bind(item_version)
    .bind(security_state)
    .bind(location_kind)
    .bind(slot_index)
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn seed_entry_loadout(persistence: &PostgresPersistence) {
    insert_item(
        persistence,
        ENTRY_EQUIPMENT,
        "item.weapon.crossbow.pine_crossbow",
        0,
        0,
        0,
        Some(0),
        1,
    )
    .await;
    insert_item(
        persistence,
        ENTRY_BELT,
        "item.consumable.tonic",
        1,
        0,
        1,
        Some(0),
        1,
    )
    .await;
    insert_item(
        persistence,
        ENTRY_BACKPACK,
        "item.consumable.tonic",
        1,
        2,
        2,
        Some(0),
        1,
    )
    .await;
}

async fn enter_danger(
    persistence: &PostgresPersistence,
    ids: FixedIds,
    mutation: u8,
    character_version: u64,
) {
    let result = coordinator(persistence.clone(), ids)
        .handle(authenticated(), &entry_frame(mutation, character_version))
        .await;
    assert_eq!(transfer_code(&result), WorldTransferResultCode::Accepted);
}

async fn checkpoint_and_advance_first_danger_clock(persistence: &PostgresPersistence) {
    let checkpoint = StoredDangerCheckpoint {
        account_id: ACCOUNT_ID,
        character_id: CHARACTER_ID,
        lineage_id: FIRST_LINEAGE_ID,
        checkpoint_tick: 1,
        content_revision: StoredWorldFlowRevisionV1 {
            records_blake3: CORE_WORLD_RECORDS_BLAKE3.to_owned(),
            assets_blake3: CORE_WORLD_ASSETS_BLAKE3.to_owned(),
            localization_blake3: CORE_WORLD_LOCALIZATION_BLAKE3.to_owned(),
        },
        composite_digest: hash(21),
        character_version: 2,
        progression_version: 1,
        // Danger entry risks the seeded equipment and Belt stack atomically, advancing the
        // inventory aggregate once before any checkpoint may bind to it.
        inventory_version: 2,
        oath_bargain_version: 1,
        checkpoint_schema_version: 1,
        checkpoint_payload: vec![1],
        checkpoint_payload_digest: *blake3::hash(&[1]).as_bytes(),
    };
    persistence
        .write_danger_checkpoint(&checkpoint)
        .await
        .unwrap();
    let request = LifeClockCheckpointRequestV1::seal(LifeClockCheckpointCommandV1 {
        account_id: ACCOUNT_ID,
        character_id: CHARACTER_ID,
        checkpoint_id: [149; 16],
        expected_character_version: 2,
        expected_life_metrics_version: 1,
        authoritative_tick: 50,
        state: LifeClockStateV1::DangerControllable,
        advanced_ticks: 50,
        danger: Some(LifeClockDangerAuthorityV1 {
            lineage_id: FIRST_LINEAGE_ID,
            restore_point_id: FIRST_RESTORE_ID,
            entry_life_metrics_version: 1,
            entry_permadeath_combat_ticks: 40,
        }),
        content: LifeClockContentAuthorityV1::core(),
        issued_at_unix_ms: 10_000,
    })
    .unwrap();
    assert!(matches!(
        persistence
            .transact_life_clock_checkpoint_v1(&request)
            .await
            .unwrap(),
        LifeClockCheckpointTransactionV1::Committed(_)
    ));
}

async fn advance_progression_for_deed(connection: &mut sqlx::PgConnection) {
    let updated = sqlx::query(
        "WITH advanced AS ( \
             UPDATE character_progression SET total_xp=120,level=2,progression_version=2, \
                    updated_at=transaction_timestamp() \
             WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 \
                   AND progression_version=1 RETURNING 1) \
         UPDATE characters SET level=2,updated_at=transaction_timestamp() \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 \
               AND EXISTS (SELECT 1 FROM advanced)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(connection)
    .await
    .unwrap();
    assert_eq!(updated.rows_affected(), 1);
}

async fn commit_sepulcher_deed(
    persistence: &PostgresPersistence,
    lineage_id: [u8; 16],
    restore_point_id: [u8; 16],
    completion_id: [u8; 16],
) {
    let source_instance_id = [completion_id[0].wrapping_add(1); 16];
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let inventory_version: i64 = sqlx::query_scalar(
        "SELECT inventory_version FROM character_inventories WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    advance_progression_for_deed(transaction.connection()).await;
    sqlx::query(
        "INSERT INTO reward_requests (namespace_id,reward_request_id,account_id,character_id, \
         source_instance_id,reward_table_id,content_revision,epoch_id,canonical_request_hash, \
         plan_hash,result_hash,audit_digest,pre_inventory_version,post_inventory_version, \
         request_state,reward_item_count) \
         VALUES ($1,$2,$3,$4,$5,'reward.miniboss_t1',$6,'crash-deed-hosted-v1',$7,$8,$9,$10, \
                 $11,$11,1,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(completion_id.as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(source_instance_id.as_slice())
    .bind(CORE_ITEM_CONTENT_REVISION)
    .bind(hash(31).as_slice())
    .bind(hash(32).as_slice())
    .bind(hash(33).as_slice())
    .bind(hash(34).as_slice())
    .bind(inventory_version)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_xp_award_results (namespace_id,account_id,character_id, \
         reward_event_id,payload_hash,source_content_id,xp_profile_id, \
         progression_content_revision,eligibility_kind,eligible,encounter_active_ticks, \
         encounter_present_ticks,encounter_longest_inactivity_ticks,encounter_reference_health, \
         encounter_direct_damage,encounter_effective_healing,encounter_damage_prevented, \
         encounter_objective_credits,encounter_life_state,encounter_recall_state, \
         encounter_trust_state,first_clear_awarded,base_xp,bonus_xp,requested_xp,applied_xp, \
         discarded_xp,pre_total_xp,post_total_xp,pre_level,post_level,pre_progression_version, \
         post_progression_version,result_code,result_payload,entry_restore_point_id) \
         VALUES ($1,$2,$3,$4,$5,'miniboss.sepulcher_knight','xp.miniboss_t1',$6,1,TRUE, \
                 300,300,0,1200,1,0,0,0,0,0,0,FALSE,120,0,120,120,0,0,120,1,2,1,2,0,$7,$8)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(completion_id.as_slice())
    .bind(hash(35).as_slice())
    .bind(CORE_PROGRESSION_RECORDS_BLAKE3)
    .bind([1_u8].as_slice())
    .bind(restore_point_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();

    let deed = LifeDeedCompletionRequestV2::seal(LifeDeedCompletionCommandV2 {
        account_id: ACCOUNT_ID,
        character_id: CHARACTER_ID,
        completion_id,
        expected_character_version: 2,
        expected_life_metrics_version: 1,
        lineage_id,
        restore_point_id,
        achieved_tick: 500,
        content: LifeDeedContentAuthorityV2::core(),
        issued_at_unix_ms: 10_000,
    })
    .unwrap();
    assert!(matches!(
        persistence
            .transact_life_deed_completion_v2(&deed)
            .await
            .unwrap(),
        LifeDeedCompletionTransactionV2::Committed(_)
    ));
}

#[allow(
    clippy::too_many_lines,
    reason = "the fixture stages one coherent danger mutation graph before the crash"
)]
async fn stage_danger_mutations(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "UPDATE character_progression SET current_health=0,progression_version=2 \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_life_metrics SET lifetime_ticks=150,permadeath_combat_ticks=90, \
         life_metrics_version=2 WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_inventories SET inventory_version=3 WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE item_instances SET item_version=3,security_state=2,location_kind=2,slot_index=1 \
         WHERE namespace_id=$1 AND item_uid=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ENTRY_EQUIPMENT.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    insert_item_ledger(
        transaction.connection(),
        [161; 16],
        ENTRY_EQUIPMENT,
        [162; 16],
        1,
        2,
        2,
        1,
        2,
        0,
        2,
        None,
    )
    .await;
    sqlx::query(
        "UPDATE item_instances SET item_version=3,security_state=4,location_kind=7, \
         destruction_reason='consumed' WHERE namespace_id=$1 AND item_uid=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ENTRY_BELT.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    insert_item_ledger(
        transaction.connection(),
        [163; 16],
        ENTRY_BELT,
        [164; 16],
        3,
        0,
        2,
        1,
        4,
        1,
        7,
        Some("consumed"),
    )
    .await;
    sqlx::query(
        "INSERT INTO character_run_material_stacks (namespace_id,account_id,character_id, \
         material_id,quantity,material_version,security_state) VALUES ($1,$2,$3,'material.iron',7,1,2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();

    insert_item(
        persistence,
        FIELD_REPLACEMENT,
        "item.weapon.crossbow.pine_crossbow",
        0,
        1,
        0,
        Some(0),
        1,
    )
    .await;
    insert_item(
        persistence,
        POST_ENTRY_BACKPACK,
        "item.consumable.tonic",
        1,
        2,
        2,
        Some(2),
        1,
    )
    .await;
}

#[allow(
    clippy::too_many_arguments,
    reason = "the fixture records every immutable item-ledger axis explicitly"
)]
async fn insert_item_ledger(
    connection: &mut sqlx::PgConnection,
    ledger_event_id: [u8; 16],
    item_uid: [u8; 16],
    mutation_id: [u8; 16],
    event_kind: i16,
    source_kind: i16,
    pre_version: i64,
    pre_security: i16,
    post_security: i16,
    pre_location: i16,
    post_location: i16,
    reason: Option<&str>,
) {
    sqlx::query(
        "INSERT INTO item_ledger_events (namespace_id,ledger_event_id,item_uid,account_id, \
         character_id,mutation_id,event_kind,source_kind,pre_item_version,post_item_version, \
         pre_security_state,post_security_state,pre_location_kind,post_location_kind,reason) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$9+1,$10,$11,$12,$13,$14)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ledger_event_id.as_slice())
    .bind(item_uid.as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(mutation_id.as_slice())
    .bind(event_kind)
    .bind(source_kind)
    .bind(pre_version)
    .bind(pre_security)
    .bind(post_security)
    .bind(pre_location)
    .bind(post_location)
    .bind(reason)
    .execute(connection)
    .await
    .unwrap();
}

async fn earn_ash(
    persistence: &PostgresPersistence,
    mutation_id: [u8; 16],
    restore_point_id: Option<[u8; 16]>,
) {
    let mut request = AshMutationRequest {
        account_id: ACCOUNT_ID,
        mutation_id,
        payload_hash: [0; 32],
        expected_wallet_version: 1,
        kind: AshMutationKind::Earn,
        amount: 10,
        reason_code: "minor_realm_event".into(),
        source_id: "fixture.crash-restore".into(),
        content_version: CORE_ITEM_CONTENT_REVISION.into(),
        entry_restore_point_id: restore_point_id,
    };
    request.payload_hash = request.expected_payload_hash();
    assert!(matches!(
        persistence.transact_ash_mutation(&request).await.unwrap(),
        AshWalletTransaction::Committed(_)
    ));
}

#[allow(
    clippy::too_many_lines,
    reason = "the assertion audits the full cross-aggregate crash result in one snapshot"
)]
async fn assert_fresh_projection(persistence: &PostgresPersistence) {
    assert!(
        persistence
            .danger_checkpoint(ACCOUNT_ID, CHARACTER_ID)
            .await
            .unwrap()
            .is_none(),
        "crash restore must remove the process-resume checkpoint and its live trace payload"
    );
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let root: (i16, Option<Vec<u8>>, i16) = sqlx::query_as(
        "SELECT restore_state,crash_restore_mutation_id,component_mask \
         FROM character_entry_restore_points WHERE namespace_id=$1 AND restore_point_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(FIRST_RESTORE_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(root, (4, Some([171_u8; 16].to_vec()), 31));
    let components: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT p.restored_progression_version,i.restored_inventory_version, \
         o.restored_oath_bargain_version,l.restored_life_metrics_version, \
         a.restored_ash_wallet_version FROM entry_restore_progression_v3 p \
         JOIN entry_restore_inventory_v3 i USING (namespace_id,restore_point_id) \
         JOIN entry_restore_oath_bargain_v3 o USING (namespace_id,restore_point_id) \
         JOIN entry_restore_life_metrics_v3 l USING (namespace_id,restore_point_id) \
         JOIN entry_restore_ash_wallet_v3 a USING (namespace_id,restore_point_id) \
         WHERE p.namespace_id=$1 AND p.restore_point_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(FIRST_RESTORE_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(components, (3, 4, 2, 3, 3));
    let aggregates: (i64, i64, i64, i32, i64, i64, i64, i64, i16, String) = sqlx::query_as(
        "SELECT a.state_version,c.character_state_version,p.progression_version,p.current_health, \
         i.inventory_version,o.oath_bargain_version,l.life_metrics_version, \
         l.permadeath_combat_ticks,w.location_kind,w.location_content_id \
         FROM accounts a JOIN characters c USING (namespace_id,account_id) \
         JOIN character_progression p USING (namespace_id,account_id,character_id) \
         JOIN character_inventories i USING (namespace_id,account_id,character_id) \
         JOIN character_oath_bargain_state o USING (namespace_id,account_id,character_id) \
         JOIN character_life_metrics l USING (namespace_id,account_id,character_id) \
         JOIN character_world_locations w USING (namespace_id,account_id,character_id) \
         WHERE a.namespace_id=$1 AND a.account_id=$2 AND c.character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(aggregates, (2, 3, 3, 120, 4, 2, 3, 40, 1, HALL_ID.into()));
    let lifetime: i64 = sqlx::query_scalar(
        "SELECT lifetime_ticks FROM character_life_metrics WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(lifetime, 150);
    let items: Vec<StoredItemProjection> = sqlx::query_as(
        "SELECT item_uid,security_state,location_kind,slot_index,destruction_reason \
         FROM item_instances WHERE namespace_id=$1 AND account_id=$2 ORDER BY item_uid",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .fetch_all(transaction.connection())
    .await
    .unwrap();
    assert_eq!(items.len(), 5);
    assert_eq!(items[0], (ENTRY_EQUIPMENT.to_vec(), 0, 0, Some(0), None));
    assert_eq!(items[1], (ENTRY_BELT.to_vec(), 0, 1, Some(0), None));
    assert_eq!(items[2], (ENTRY_BACKPACK.to_vec(), 2, 2, Some(0), None));
    for item in &items[3..] {
        assert_eq!(
            (item.1, item.2, item.3, item.4.as_deref()),
            (3, 4, None, Some("crash_revoked"))
        );
    }
    let material: (i32, i64, i16, Option<Vec<u8>>) = sqlx::query_as(
        "SELECT quantity,material_version,security_state,terminal_restore_point_id \
         FROM character_run_material_stacks WHERE namespace_id=$1 AND account_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(material, (0, 2, 3, Some(FIRST_RESTORE_ID.to_vec())));
    let ash: (i32, i64, i64) = sqlx::query_as(
        "SELECT w.balance,w.wallet_version,count(c.*) FROM ash_wallets w \
         LEFT JOIN danger_crash_restore_ash_changes c ON c.namespace_id=w.namespace_id \
         AND c.account_id=w.account_id WHERE w.namespace_id=$1 AND w.account_id=$2 \
         GROUP BY w.balance,w.wallet_version",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(ash, (0, 3, 1));
    let life_deed_sidecar: (i16, i32, i32, i32) = sqlx::query_as(
        "SELECT life_deed_contract_version,revoked_life_deed_count, \
                octet_length(life_deed_projection_digest), \
                octet_length(life_deed_revocation_digest) \
         FROM danger_crash_restore_results WHERE namespace_id=$1 AND account_id=$2 \
           AND mutation_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind([171_u8; 16].as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(life_deed_sidecar, (1, 0, 32, 32));
    let lineage_state: i16 = sqlx::query_scalar(
        "SELECT lineage_state FROM character_instance_lineages WHERE namespace_id=$1 \
         AND lineage_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(FIRST_LINEAGE_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(lineage_state, 3);
    transaction.rollback().await.unwrap();
}

async fn assert_no_partial_restore(persistence: &PostgresPersistence, restore_id: [u8; 16]) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let state: (i16, i32, i64, i64) = sqlx::query_as(
        "SELECT r.restore_state,p.current_health,p.progression_version, \
         (SELECT count(*) FROM danger_crash_restore_request_results q \
          WHERE q.namespace_id=r.namespace_id AND q.restore_point_id=r.restore_point_id) \
         FROM character_entry_restore_points r JOIN character_progression p \
         USING (namespace_id,account_id,character_id) WHERE r.namespace_id=$1 \
         AND r.restore_point_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(restore_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(state, (0, 0, 2, 0));
    transaction.rollback().await.unwrap();
}

async fn commit_recall_terminal(persistence: &PostgresPersistence, ids: FixedIds) {
    let request = ProductionRecallCommitRequestV1 {
        contract_version: PRODUCTION_RECALL_CONTRACT_VERSION_V1,
        namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
        account_id: ACCOUNT_ID,
        character_id: CHARACTER_ID,
        mutation_id: [198; 16],
        terminal_id: [199; 16],
        trigger: ProductionRecallTriggerV1::Explicit,
        request_sequence: Some(1),
        explicit_client_tick: Some(1),
        instance_lineage_id: ids.lineage,
        entry_restore_point_id: ids.restore,
        expected_versions: ProductionRecallExpectedVersionsV1 {
            account: 1,
            character: 2,
            world: 2,
            inventory: 2,
            life_metrics: 1,
            progression: 1,
            oath_bargain: 1,
            ash_wallet: 1,
        },
        content_revision: StoredWorldFlowRevisionV1 {
            records_blake3: CORE_WORLD_RECORDS_BLAKE3.into(),
            assets_blake3: CORE_WORLD_ASSETS_BLAKE3.into(),
            localization_blake3: CORE_WORLD_LOCALIZATION_BLAKE3.into(),
        },
        issued_at_unix_ms: 1,
        trigger_started_tick: 1_000,
        completion_tick: 1_012,
        final_lifetime_ticks: 100,
        final_permadeath_combat_ticks: 40,
    };
    let prepared = persistence
        .prepare_production_recall_v1(&request)
        .await
        .unwrap();
    assert!(!prepared.replayed());
    assert!(matches!(
        persistence
            .commit_production_recall_v1(&request, prepared.canonical_plan_hash())
            .await
            .unwrap(),
        ProductionRecallTransactionV1::Fresh(_)
    ));
}

async fn assert_terminal_precedence(persistence: &PostgresPersistence) {
    for (state, expected, ids, entry_mutation, crash_mutation) in [
        (
            1_i16,
            DangerCrashRestoreCode::ExtractionCommitted,
            FixedIds {
                transfer: [181; 16],
                lineage: [185; 16],
                restore: [189; 16],
            },
            190,
            [193; 16],
        ),
        (
            3_i16,
            DangerCrashRestoreCode::RecallCommitted,
            FixedIds {
                transfer: [183; 16],
                lineage: [187; 16],
                restore: [192; 16],
            },
            196,
            [197; 16],
        ),
    ] {
        reset_fixture(persistence).await;
        seed_entry_loadout(persistence).await;
        enter_danger(persistence, ids, entry_mutation, 1).await;
        let restore_id = ids.restore;
        if state == 3 {
            commit_recall_terminal(persistence, ids).await;
        } else {
            let mut transaction = persistence.begin_transaction().await.unwrap();
            sqlx::query(
                "UPDATE character_entry_restore_points SET restore_state=$1, \
                 consumed_at=transaction_timestamp() \
                 WHERE namespace_id=$2 AND restore_point_id=$3",
            )
            .bind(state)
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(restore_id.as_slice())
            .execute(transaction.connection())
            .await
            .unwrap();
            transaction.commit().await.unwrap();
        }
        let request = crash_request(restore_id, crash_mutation);
        let first = persistence
            .transact_danger_crash_restore(&request)
            .await
            .unwrap();
        let DangerCrashRestoreTransaction::Fresh(receipt) = first else {
            panic!("terminal winner must produce a fresh durable receipt");
        };
        assert_eq!(receipt.code, expected);
        assert!(matches!(
            persistence.transact_danger_crash_restore(&request).await.unwrap(),
            DangerCrashRestoreTransaction::Replayed(replay) if replay == receipt
        ));
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
#[allow(clippy::too_many_lines)]
async fn danger_crash_restore_is_exact_atomic_replay_safe_and_terminal_authoritative() {
    let persistence = disposable_database().await;

    reset_fixture(&persistence).await;
    seed_entry_loadout(&persistence).await;
    enter_danger(
        &persistence,
        FixedIds {
            transfer: FIRST_TRANSFER_ID,
            lineage: FIRST_LINEAGE_ID,
            restore: FIRST_RESTORE_ID,
        },
        91,
        1,
    )
    .await;
    checkpoint_and_advance_first_danger_clock(&persistence).await;
    stage_danger_mutations(&persistence).await;
    earn_ash(&persistence, [165; 16], Some(FIRST_RESTORE_ID)).await;

    let request = crash_request(FIRST_RESTORE_ID, [171; 16]);
    let (left, right) = tokio::join!(
        persistence.transact_danger_crash_restore(&request),
        persistence.transact_danger_crash_restore(&request),
    );
    let (receipt, concurrent_replay) = match (left.unwrap(), right.unwrap()) {
        (
            DangerCrashRestoreTransaction::Fresh(fresh),
            DangerCrashRestoreTransaction::Replayed(replay),
        )
        | (
            DangerCrashRestoreTransaction::Replayed(replay),
            DangerCrashRestoreTransaction::Fresh(fresh),
        ) => (fresh, replay),
        outcomes => panic!("concurrent identical requests were not fresh/replayed: {outcomes:?}"),
    };
    assert_eq!(concurrent_replay, receipt);
    assert_eq!(receipt.code, DangerCrashRestoreCode::Restored);
    assert_eq!(
        receipt
            .item_changes
            .iter()
            .filter(|change| change.kind == DangerCrashItemChangeKind::Restored)
            .count(),
        3
    );
    assert_eq!(
        receipt
            .item_changes
            .iter()
            .filter(|change| change.kind == DangerCrashItemChangeKind::Revoked)
            .count(),
        2
    );
    assert_eq!(
        (receipt.material_changes.len(), receipt.ash_changes.len()),
        (1, 1)
    );
    assert_fresh_projection(&persistence).await;

    // Simulate response loss and process restart: the same request must replay byte-identically.
    persistence.close().await;
    let restarted = disposable_database().await;
    assert!(matches!(
        restarted.transact_danger_crash_restore(&request).await.unwrap(),
        DangerCrashRestoreTransaction::Replayed(replay) if replay == receipt
    ));
    let restored_clock = restarted
        .load_life_clock_head_v1(ACCOUNT_ID, CHARACTER_ID)
        .await
        .unwrap();
    assert_eq!(restored_clock.lifetime_ticks, 150);
    assert_eq!(restored_clock.permadeath_combat_ticks, 40);
    assert_eq!(restored_clock.life_metrics_version, 3);
    assert_eq!(restored_clock.authoritative_tick, 50);
    assert!(restored_clock.danger.is_none());
    let counts: (i64, i64, i64) = {
        let mut transaction = restarted.begin_transaction().await.unwrap();
        let counts = sqlx::query_as(
            "SELECT (SELECT count(*) FROM danger_crash_restore_results WHERE namespace_id=$1 \
                     AND restore_point_id=$2), \
                    (SELECT count(*) FROM danger_crash_restore_request_results WHERE namespace_id=$1 \
                     AND restore_point_id=$2), \
                    (SELECT count(*) FROM item_ledger_events WHERE namespace_id=$1 \
                     AND mutation_id=$3)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(FIRST_RESTORE_ID.as_slice())
        .bind([171_u8; 16].as_slice())
        .fetch_one(transaction.connection())
        .await
        .unwrap();
        transaction.rollback().await.unwrap();
        counts
    };
    assert_eq!(counts, (1, 1, 5));

    let changed = crash_request([172; 16], request.mutation_id);
    let conflict = restarted
        .transact_danger_crash_restore(&changed)
        .await
        .unwrap();
    assert!(matches!(
        conflict,
        DangerCrashRestoreTransaction::Conflict {
            mutation_id,
            stored_request_hash,
            attempted_request_hash,
            ..
        } if mutation_id == request.mutation_id
            && stored_request_hash == request.request_hash
            && attempted_request_hash == changed.request_hash
    ));
    let conflict_count: i64 = {
        let mut transaction = restarted.begin_transaction().await.unwrap();
        let count = sqlx::query_scalar(
            "SELECT count(*) FROM danger_crash_restore_conflict_audits WHERE namespace_id=$1 \
             AND account_id=$2 AND mutation_id=$3",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ACCOUNT_ID.as_slice())
        .bind(request.mutation_id.as_slice())
        .fetch_one(transaction.connection())
        .await
        .unwrap();
        transaction.rollback().await.unwrap();
        count
    };
    assert_eq!(conflict_count, 1);

    // A later danger life must ignore retained crash-revoked audit rows from the prior root.
    enter_danger(
        &restarted,
        FixedIds {
            transfer: SECOND_TRANSFER_ID,
            lineage: SECOND_LINEAGE_ID,
            restore: SECOND_RESTORE_ID,
        },
        92,
        3,
    )
    .await;
    let second_request = crash_request(SECOND_RESTORE_ID, [173; 16]);
    assert!(matches!(
        restarted
            .transact_danger_crash_restore(&second_request)
            .await
            .unwrap(),
        DangerCrashRestoreTransaction::Fresh(second)
            if second.code == DangerCrashRestoreCode::Restored
    ));

    // Corrupt live content authority after a valid V3 capture; every earlier staged write rolls back.
    reset_fixture(&restarted).await;
    seed_entry_loadout(&restarted).await;
    enter_danger(
        &restarted,
        FixedIds {
            transfer: [201; 16],
            lineage: [202; 16],
            restore: [203; 16],
        },
        94,
        1,
    )
    .await;
    let mut corrupt = restarted.begin_transaction().await.unwrap();
    sqlx::query(
        "UPDATE character_progression SET current_health=0,progression_version=2 \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(corrupt.connection())
    .await
    .unwrap();
    sqlx::query("UPDATE item_instances SET template_id='item.corrupt.authority' WHERE namespace_id=$1 AND item_uid=$2")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ENTRY_EQUIPMENT.as_slice())
        .execute(corrupt.connection())
        .await
        .unwrap();
    corrupt.commit().await.unwrap();
    assert!(matches!(
        restarted
            .transact_danger_crash_restore(&crash_request([203; 16], [204; 16]))
            .await,
        Err(PersistenceError::CorruptStoredDangerCrashRestore)
    ));
    assert_no_partial_restore(&restarted, [203; 16]).await;

    // An unrelated safe-account wallet mutation after entry is ambiguous and must not be guessed.
    reset_fixture(&restarted).await;
    seed_entry_loadout(&restarted).await;
    enter_danger(
        &restarted,
        FixedIds {
            transfer: [211; 16],
            lineage: [212; 16],
            restore: [213; 16],
        },
        95,
        1,
    )
    .await;
    let mut zero_health = restarted.begin_transaction().await.unwrap();
    sqlx::query(
        "UPDATE character_progression SET current_health=0,progression_version=2 \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(zero_health.connection())
    .await
    .unwrap();
    zero_health.commit().await.unwrap();
    earn_ash(&restarted, [214; 16], None).await;
    assert!(matches!(
        restarted
            .transact_danger_crash_restore(&crash_request([213; 16], [215; 16]))
            .await,
        Err(PersistenceError::DangerCrashRestoreAmbiguousAsh)
    ));
    assert_no_partial_restore(&restarted, [213; 16]).await;

    assert_terminal_precedence(&restarted).await;
    restarted.close().await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn danger_crash_restore_revokes_root_deeds_and_rebuilds_projection() {
    let persistence = disposable_database().await;
    reset_fixture(&persistence).await;
    let ids = FixedIds {
        transfer: [221; 16],
        lineage: [222; 16],
        restore: [223; 16],
    };
    enter_danger(&persistence, ids, 96, 1).await;
    commit_sepulcher_deed(&persistence, ids.lineage, ids.restore, [224; 16]).await;

    let request = crash_request(ids.restore, [225; 16]);
    let DangerCrashRestoreTransaction::Fresh(receipt) = persistence
        .transact_danger_crash_restore(&request)
        .await
        .unwrap()
    else {
        panic!("fresh root-bound deed crash must commit exactly once")
    };
    assert_eq!(receipt.code, DangerCrashRestoreCode::Restored);
    let versions = receipt.versions.as_ref().unwrap();
    assert_eq!((versions.progression, versions.life_metrics), (3, 3));

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let sidecar: (i16, i32, i32, i32) = sqlx::query_as(
        "SELECT life_deed_contract_version,revoked_life_deed_count, \
                octet_length(life_deed_projection_digest), \
                octet_length(life_deed_revocation_digest) \
         FROM danger_crash_restore_results WHERE namespace_id=$1 AND account_id=$2 \
           AND mutation_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(request.mutation_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(sidecar, (1, 1, 32, 32));
    let graph: (i64, i64, bool, i64) = sqlx::query_as(
        "SELECT \
           (SELECT count(*) FROM character_life_deed_revocations_v2 WHERE namespace_id=$1 \
             AND account_id=$2 AND crash_mutation_id=$3), \
           (SELECT count(*) FROM character_life_deeds WHERE namespace_id=$1 \
             AND account_id=$2 AND character_id=$4), \
           (SELECT revoked_by_restore_point_id=$5 FROM character_xp_award_results \
             WHERE namespace_id=$1 AND account_id=$2 AND reward_event_id=$6), \
           (SELECT life_metrics_version FROM character_life_metrics WHERE namespace_id=$1 \
             AND account_id=$2 AND character_id=$4)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(ids.restore.as_slice())
    .bind([224_u8; 16].as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(graph, (1, 0, true, 3));

    persistence.close().await;
    let restarted = disposable_database().await;
    assert!(matches!(
        restarted.transact_danger_crash_restore(&request).await.unwrap(),
        DangerCrashRestoreTransaction::Replayed(replay) if replay == receipt
    ));
    restarted.close().await;
}
