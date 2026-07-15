use std::{path::PathBuf, sync::Arc};

use persistence::{
    CORE_ITEM_CONTENT_REVISION, PersistenceConfig, PersistenceTransaction, PostgresPersistence,
    WIPEABLE_CORE_NAMESPACE,
};
use protocol::{
    ManifestHash, WireText, WorldFlowContentRevisionV1, WorldFlowFrame, WorldFlowRequest,
    WorldFlowResult, WorldTransferCommand, WorldTransferMutation, WorldTransferPayload,
    WorldTransferResultCode,
};
use server_app::{
    AccountId, AshWalletRestoreV3, AuthenticatedAccount, AuthenticatedNamespace,
    CrashRestoreContext, DangerEntrySnapshotV3, EntryCaptureContext, EntryRestoreProvider,
    IdentityClock, InventorySecurityRestoreV3, LifeMetricsRestoreV3, OathBargainRestoreV3,
    PostgresDangerEntryAshWalletProviderV3, PostgresDangerEntryInventoryProviderV3,
    PostgresDangerEntryLifeMetricsProviderV3, PostgresDangerEntryOathBargainProviderV3,
    PostgresDormantWorldFlowCoordinator, PostgresProgressionRestoreProvider,
    PostgresSafeInventoryService, ProgressionRestoreV1, RestorePointError, SafeAggregateVersionsV3,
    SafeInventoryServiceError, SafeInventoryTransferCommand, SafeInventoryTransferKind,
    WorldFlowIdGenerator,
};

const ACCOUNT_ID: [u8; 16] = [81; 16];
const CHARACTER_ID: [u8; 16] = [82; 16];
const FOREIGN_ACCOUNT_ID: [u8; 16] = [83; 16];
const FOREIGN_CHARACTER_ID: [u8; 16] = [84; 16];
const TRANSFER_ID: [u8; 16] = [85; 16];
const LINEAGE_ID: [u8; 16] = [86; 16];
const RESTORE_ID: [u8; 16] = [87; 16];
const HALL_ID: &str = "hub.lantern_halls_01";
const WORLD_ID: &str = "world.core_microrealm_01";
const LAYOUT_ID: &str = "layout.core_private_life_01";

fn content_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content")
}

async fn disposable_database() -> PostgresPersistence {
    let config = PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL");
    let persistence = PostgresPersistence::connect(&config).await.unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence.migrate().await.unwrap();
    persistence
}

async fn insert_character(
    transaction: &mut PersistenceTransaction<'_>,
    account_id: [u8; 16],
    character_id: [u8; 16],
) {
    sqlx::query(
        "INSERT INTO accounts (namespace_id, account_id, state_version, slot_capacity) \
         VALUES ($1, $2, 1, 2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ash_wallets (namespace_id, account_id, balance, wallet_version) \
         VALUES ($1, $2, 0, 1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO characters (namespace_id, account_id, character_id, roster_ordinal, \
         class_id, level, oath_id, life_state, security_state, character_state_version) \
         VALUES ($1, $2, $3, 1, 'class.grave_arbalist', 1, NULL, 0, 0, 1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE accounts SET selected_character_id = $1 WHERE namespace_id = $2 \
         AND account_id = $3",
    )
    .bind(character_id.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_world_locations (namespace_id, account_id, character_id, \
         character_version, location_kind, location_content_id, safe_arrival_kind) \
         VALUES ($1, $2, $3, 1, 1, $4, 0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(HALL_ID)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_progression (namespace_id, account_id, character_id, total_xp, \
         level, current_health, progression_version) VALUES ($1, $2, $3, 0, 1, 120, 1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_life_metrics (namespace_id, account_id, character_id, \
         lifetime_ticks, permadeath_combat_ticks, life_metrics_version) \
         VALUES ($1, $2, $3, 0, 0, 1) \
         ON CONFLICT (namespace_id, account_id, character_id) DO NOTHING",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_inventories (namespace_id, account_id, character_id, \
         inventory_version) VALUES ($1, $2, $3, 1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_oath_bargain_state (namespace_id, account_id, character_id, \
         earned_bargain_slots, oath_bargain_version) VALUES ($1, $2, $3, 0, 1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
}

async fn reset_fixture(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query("DELETE FROM accounts WHERE namespace_id = $1 AND account_id IN ($2, $3)")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ACCOUNT_ID.as_slice())
        .bind(FOREIGN_ACCOUNT_ID.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
    insert_character(&mut transaction, ACCOUNT_ID, CHARACTER_ID).await;
    insert_character(&mut transaction, FOREIGN_ACCOUNT_ID, FOREIGN_CHARACTER_ID).await;
    transaction.commit().await.unwrap();
}

async fn insert_safe_equipment(
    transaction: &mut PersistenceTransaction<'_>,
    item_uid: [u8; 16],
    character_id: Option<[u8; 16]>,
    security_state: i16,
    location_kind: i16,
    slot_index: i16,
) {
    sqlx::query(
        "INSERT INTO item_instances (namespace_id,item_uid,account_id,character_id,template_id, \
         content_revision,item_kind,item_level,rarity,creation_kind,creation_request_id,roll_index, \
         unit_ordinal,item_version,security_state,location_kind,slot_index,provenance_kind, \
         salvage_band,salvage_value) VALUES ($1,$2,$3,$4,'item.weapon.crossbow.pine_crossbow', \
         $5,0,1,0,0,$2,0,0,1,$6,$7,$8,0,0,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(item_uid.as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(character_id.map(|id| id.to_vec()))
    .bind(CORE_ITEM_CONTENT_REVISION)
    .bind(security_state)
    .bind(location_kind)
    .bind(slot_index)
    .execute(transaction.connection())
    .await
    .unwrap();
}

async fn seed_character_safe_item(persistence: &PostgresPersistence, item_uid: [u8; 16]) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    insert_safe_equipment(&mut transaction, item_uid, Some(CHARACTER_ID), 0, 5, 0).await;
    transaction.commit().await.unwrap();
}

async fn insert_safe_belt_unit(
    transaction: &mut PersistenceTransaction<'_>,
    item_uid: [u8; 16],
    slot_index: i16,
) {
    sqlx::query(
        "INSERT INTO item_instances (namespace_id,item_uid,account_id,character_id,template_id, \
         content_revision,item_kind,item_level,rarity,creation_kind,creation_request_id,roll_index, \
         unit_ordinal,item_version,security_state,location_kind,slot_index,provenance_kind, \
         salvage_band,salvage_value) VALUES ($1,$2,$3,$4,'item.consumable.tonic', \
         $5,1,NULL,NULL,0,$2,0,0,1,0,1,$6,0,0,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(item_uid.as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(CORE_ITEM_CONTENT_REVISION)
    .bind(slot_index)
    .execute(transaction.connection())
    .await
    .unwrap();
}

async fn seed_entry_loadout(persistence: &PostgresPersistence) -> [[u8; 16]; 3] {
    let identities = [[61; 16], [62; 16], [63; 16]];
    let mut transaction = persistence.begin_transaction().await.unwrap();
    insert_safe_equipment(&mut transaction, identities[0], Some(CHARACTER_ID), 0, 0, 0).await;
    insert_safe_belt_unit(&mut transaction, identities[1], 0).await;
    insert_safe_belt_unit(&mut transaction, identities[2], 0).await;
    transaction.commit().await.unwrap();
    identities
}

async fn seed_deliberate_risk_item(persistence: &PostgresPersistence, item_uid: [u8; 16]) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    insert_safe_equipment(&mut transaction, item_uid, Some(CHARACTER_ID), 2, 2, 0).await;
    transaction.commit().await.unwrap();
}

async fn fill_vault(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    for slot in 0_i16..160 {
        let item_uid = (10_000_u128 + u128::try_from(slot).unwrap()).to_be_bytes();
        insert_safe_equipment(&mut transaction, item_uid, None, 0, 6, slot).await;
    }
    transaction.commit().await.unwrap();
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

fn authenticated(account_id: [u8; 16]) -> AuthenticatedAccount {
    AuthenticatedAccount {
        account_id: AccountId::new(account_id).unwrap(),
        namespace: AuthenticatedNamespace::WipeableTest,
    }
}

fn frame(sequence: u32, mutation_id: u8, character_id: [u8; 16], version: u64) -> WorldFlowFrame {
    let payload = WorldTransferPayload {
        content_revision: revision(),
        command: WorldTransferCommand::UsePortal {
            portal_id: WireText::new("station.realm_gate").unwrap(),
        },
    };
    WorldFlowFrame {
        sequence,
        request: WorldFlowRequest::Transfer(WorldTransferMutation {
            mutation_id: [mutation_id; 16],
            character_id,
            expected_character_version: version,
            issued_at_unix_millis: 9_000,
            payload_hash: payload.canonical_hash(),
            payload,
        }),
    }
}

#[derive(Debug, Clone, Copy)]
struct FixedIds;

impl WorldFlowIdGenerator for FixedIds {
    fn next_transfer_id(&self) -> [u8; 16] {
        TRANSFER_ID
    }

    fn next_lineage_id(&self) -> [u8; 16] {
        LINEAGE_ID
    }

    fn next_restore_point_id(&self) -> [u8; 16] {
        RESTORE_ID
    }
}

#[derive(Debug, Clone, Copy)]
struct FixedClock;

impl IdentityClock for FixedClock {
    fn unix_millis(&self) -> u64 {
        10_000
    }
}

#[derive(Debug, Clone, Copy)]
struct FailingInventory;

impl EntryRestoreProvider for FailingInventory {
    type Snapshot = InventorySecurityRestoreV3;

    async fn capture<'a>(
        &'a self,
        _transaction: &'a mut PersistenceTransaction<'_>,
        _context: EntryCaptureContext,
    ) -> Result<Self::Snapshot, RestorePointError> {
        Err(RestorePointError::IncompleteRestorePoint)
    }

    async fn restore_and_revoke_post_entry<'a>(
        &'a self,
        _transaction: &'a mut PersistenceTransaction<'_>,
        _context: CrashRestoreContext,
    ) -> Result<(), RestorePointError> {
        Err(RestorePointError::IncompleteRestorePoint)
    }
}

fn coordinator<Inventory>(
    persistence: PostgresPersistence,
    inventory: Inventory,
) -> PostgresDormantWorldFlowCoordinator<
    FixedIds,
    FixedClock,
    Inventory,
    PostgresDangerEntryOathBargainProviderV3,
    PostgresDangerEntryLifeMetricsProviderV3,
    PostgresDangerEntryAshWalletProviderV3,
>
where
    Inventory: EntryRestoreProvider<Snapshot = InventorySecurityRestoreV3>,
{
    let progression = sim_content::load_core_development_progression(&content_root()).unwrap();
    PostgresDormantWorldFlowCoordinator::new(
        persistence,
        FixedIds,
        FixedClock,
        revision(),
        PostgresProgressionRestoreProvider::new(&progression).unwrap(),
        inventory,
        PostgresDangerEntryOathBargainProviderV3,
        PostgresDangerEntryLifeMetricsProviderV3,
        PostgresDangerEntryAshWalletProviderV3,
    )
}

fn code(result: &WorldFlowResult) -> WorldTransferResultCode {
    match result {
        WorldFlowResult::Transfer { code, .. } | WorldFlowResult::Error { code, .. } => *code,
        WorldFlowResult::Location { .. } => panic!("unexpected location result"),
    }
}

async fn aggregate_counts(persistence: &PostgresPersistence) -> (i64, i64, i64, i64) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let counts = sqlx::query_as(
        "SELECT (SELECT count(*) FROM character_instance_lineages WHERE account_id = $1), \
                (SELECT count(*) FROM character_entry_restore_points WHERE account_id = $1), \
                (SELECT count(*) FROM entry_restore_progression_v1 WHERE account_id = $1), \
                (SELECT count(*) FROM character_world_transfer_results WHERE account_id = $1)",
    )
    .bind(ACCOUNT_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    counts
}

#[derive(Debug, sqlx::FromRow)]
struct StoredRootProjection {
    content_id: String,
    layout_id: String,
    source_location_id: String,
    records_blake3: String,
    assets_blake3: String,
    localization_blake3: String,
    account_version: i64,
    character_version: i64,
    progression_version: i64,
    inventory_version: i64,
    oath_bargain_version: i64,
    life_metrics_version: i64,
    ash_wallet_version: i64,
    snapshot_contract_version: i16,
    component_mask: i16,
    composite_digest: Vec<u8>,
}

async fn assert_committed_danger_root(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let root = sqlx::query_as::<_, StoredRootProjection>(
        "SELECT l.content_id, l.layout_id, r.source_location_id, r.records_blake3, \
                r.assets_blake3, r.localization_blake3, r.account_version, \
                r.character_version, r.progression_version, r.inventory_version, \
                r.oath_bargain_version, r.life_metrics_version, r.ash_wallet_version, \
                r.snapshot_contract_version, r.component_mask, r.composite_digest \
         FROM character_instance_lineages l JOIN character_entry_restore_points r \
         USING (namespace_id, account_id, character_id, lineage_id) \
         WHERE l.namespace_id = $1 AND l.account_id = $2 AND l.character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(root.content_id, WORLD_ID);
    assert_eq!(root.layout_id, LAYOUT_ID);
    assert_eq!(root.source_location_id, HALL_ID);
    let required_revision = revision();
    assert_eq!(
        root.records_blake3,
        required_revision.records_blake3.as_str()
    );
    assert_eq!(root.assets_blake3, required_revision.assets_blake3.as_str());
    assert_eq!(
        root.localization_blake3,
        required_revision.localization_blake3.as_str()
    );
    assert_eq!(
        (
            root.account_version,
            root.character_version,
            root.progression_version,
            root.inventory_version,
            root.oath_bargain_version,
            root.life_metrics_version,
            root.ash_wallet_version,
            root.snapshot_contract_version,
            root.component_mask,
        ),
        (1, 1, 1, 1, 1, 1, 1, 3, 31)
    );
    assert_eq!(root.composite_digest, expected_snapshot(required_revision));
    assert!(matches!(
        persistence
            .world_location(ACCOUNT_ID, CHARACTER_ID)
            .await
            .unwrap(),
        Some(persistence::StoredWorldLocation::Danger {
            character_version: 2,
            location_content_id,
            instance_lineage_id: LINEAGE_ID,
            entry_restore_point_id: RESTORE_ID,
        }) if location_content_id == WORLD_ID
    ));
}

fn expected_snapshot(revision: WorldFlowContentRevisionV1) -> Vec<u8> {
    DangerEntrySnapshotV3 {
        character_id: CHARACTER_ID,
        content_revision: revision,
        progression: ProgressionRestoreV1 {
            level: 1,
            xp: 0,
            current_health: 120,
            progression_version: 1,
        },
        inventory: InventorySecurityRestoreV3 {
            baseline_items: vec![],
            pre_inventory_version: 1,
            inventory_version: 1,
            safe_placement_count: 0,
        },
        oath_bargains: OathBargainRestoreV3 {
            oath_id: None,
            active_bargains: vec![],
            earned_bargain_slots: 0,
            oath_bargain_version: 1,
        },
        life_metrics: LifeMetricsRestoreV3 {
            lifetime_ticks: 0,
            permadeath_combat_ticks: 0,
            life_metrics_version: 1,
        },
        ash_wallet: AshWalletRestoreV3 {
            ash_wallet_version: 1,
        },
        versions: SafeAggregateVersionsV3 {
            account_version: 1,
            character_version: 1,
            progression_version: 1,
            inventory_version: 1,
            oath_bargain_version: 1,
            life_metrics_version: 1,
            ash_wallet_version: 1,
        },
    }
    .composite_digest()
    .unwrap()
    .to_vec()
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn danger_entry_commits_complete_root_and_replays_after_pool_restart() {
    let persistence = disposable_database().await;
    reset_fixture(&persistence).await;
    let request = frame(1, 91, CHARACTER_ID, 1);
    let service = coordinator(persistence.clone(), PostgresDangerEntryInventoryProviderV3);
    let accepted = service.handle(authenticated(ACCOUNT_ID), &request).await;
    assert_eq!(code(&accepted), WorldTransferResultCode::Accepted);
    assert_eq!(aggregate_counts(&persistence).await, (1, 1, 1, 1));

    assert_committed_danger_root(&persistence).await;

    drop(service);
    persistence.close().await;
    let restarted = disposable_database().await;
    let replay = coordinator(restarted.clone(), PostgresDangerEntryInventoryProviderV3)
        .handle(
            authenticated(ACCOUNT_ID),
            &WorldFlowFrame {
                sequence: 9,
                ..request.clone()
            },
        )
        .await;
    assert_eq!(code(&replay), WorldTransferResultCode::Accepted);
    assert_eq!(aggregate_counts(&restarted).await, (1, 1, 1, 1));

    let mut conflict = request;
    let WorldFlowRequest::Transfer(ref mut mutation) = conflict.request else {
        unreachable!();
    };
    mutation.expected_character_version = 2;
    let conflicted = coordinator(restarted.clone(), PostgresDangerEntryInventoryProviderV3)
        .handle(authenticated(ACCOUNT_ID), &conflict)
        .await;
    assert_eq!(
        code(&conflicted),
        WorldTransferResultCode::IdempotencyConflict
    );
    assert_eq!(aggregate_counts(&restarted).await, (1, 1, 1, 1));
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn danger_entry_atomically_risks_loadout_and_advances_combined_inventory_once() {
    let persistence = disposable_database().await;
    reset_fixture(&persistence).await;
    let risk_items = seed_entry_loadout(&persistence).await;
    let safe_item = [64; 16];
    seed_character_safe_item(&persistence, safe_item).await;

    let accepted = coordinator(persistence.clone(), PostgresDangerEntryInventoryProviderV3)
        .handle(authenticated(ACCOUNT_ID), &frame(1, 92, CHARACTER_ID, 1))
        .await;
    assert_eq!(code(&accepted), WorldTransferResultCode::Accepted);

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let versions: (i64, i64, i64) = sqlx::query_as(
        "SELECT a.state_version, i.inventory_version, r.inventory_version \
         FROM accounts a JOIN character_inventories i USING (namespace_id,account_id) \
         JOIN character_entry_restore_points r USING (namespace_id,account_id,character_id) \
         WHERE a.namespace_id=$1 AND a.account_id=$2 AND i.character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(versions, (2, 2, 2));
    let component: (i64, i64, i16, i16) = sqlx::query_as(
        "SELECT pre_inventory_version,post_inventory_version,baseline_item_count, \
         safe_placement_count FROM entry_restore_inventory_v3 \
         WHERE namespace_id=$1 AND restore_point_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(RESTORE_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(component, (1, 2, 3, 1));
    let rows: Vec<(Vec<u8>, i16, i16, i64)> = sqlx::query_as(
        "SELECT item_uid,location_kind,security_state,item_version FROM item_instances \
         WHERE namespace_id=$1 AND item_uid = ANY($2) ORDER BY location_kind,slot_index,item_uid",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(risk_items.iter().map(|id| id.to_vec()).collect::<Vec<_>>())
    .fetch_all(transaction.connection())
    .await
    .unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0], (risk_items[0].to_vec(), 0, 1, 2));
    assert_eq!(rows[1], (risk_items[1].to_vec(), 1, 1, 2));
    assert_eq!(rows[2], (risk_items[2].to_vec(), 1, 1, 2));
    let risk_ledgers: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM item_ledger_events WHERE namespace_id=$1 \
         AND mutation_id=$2 AND post_security_state=1",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind([92_u8; 16].as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(risk_ledgers, 3);
    let safe_projection: (i16, i16, i64) = sqlx::query_as(
        "SELECT location_kind,security_state,item_version FROM item_instances \
         WHERE namespace_id=$1 AND item_uid=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(safe_item.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert_eq!(safe_projection, (6, 0, 2));
    transaction.rollback().await.unwrap();
    persistence.close().await;
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn danger_entry_deposits_character_safe_before_ids_and_captures_post_versions() {
    const SAFE_ITEM: [u8; 16] = [121; 16];
    let persistence = disposable_database().await;
    reset_fixture(&persistence).await;
    seed_character_safe_item(&persistence, SAFE_ITEM).await;

    let accepted = coordinator(persistence.clone(), PostgresDangerEntryInventoryProviderV3)
        .handle(authenticated(ACCOUNT_ID), &frame(1, 98, CHARACTER_ID, 1))
        .await;
    assert_eq!(code(&accepted), WorldTransferResultCode::Accepted);

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let item: (Option<Vec<u8>>, i16, i16, i64) = sqlx::query_as(
        "SELECT character_id,location_kind,slot_index,item_version FROM item_instances \
         WHERE namespace_id=$1 AND account_id=$2 AND item_uid=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(SAFE_ITEM.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let versions: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT a.state_version,i.inventory_version,r.account_version,r.inventory_version \
         FROM accounts a JOIN character_inventories i USING (namespace_id,account_id) \
         JOIN character_entry_restore_points r USING (namespace_id,account_id,character_id) \
         WHERE a.namespace_id=$1 AND a.account_id=$2 AND i.character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let ledger_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM item_ledger_events WHERE namespace_id=$1 AND account_id=$2 \
         AND item_uid=$3 AND mutation_id=$4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(SAFE_ITEM.as_slice())
    .bind([98_u8; 16].as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(item, (None, 6, 0, 2));
    assert_eq!(versions, (2, 2, 2, 2));
    assert_eq!(ledger_count, 1);
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn full_vault_rejects_before_item_version_identity_restore_or_location_change() {
    const SAFE_ITEM: [u8; 16] = [122; 16];
    let persistence = disposable_database().await;
    reset_fixture(&persistence).await;
    seed_character_safe_item(&persistence, SAFE_ITEM).await;
    fill_vault(&persistence).await;

    let rejected = coordinator(persistence.clone(), PostgresDangerEntryInventoryProviderV3)
        .handle(authenticated(ACCOUNT_ID), &frame(1, 99, CHARACTER_ID, 1))
        .await;
    assert_eq!(
        code(&rejected),
        WorldTransferResultCode::StorageResolutionRequired
    );
    assert_eq!(aggregate_counts(&persistence).await, (0, 0, 0, 1));

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let state: (Option<Vec<u8>>, i16, i16, i64, i64, i64, i16) = sqlx::query_as(
        "SELECT x.character_id,x.location_kind,x.slot_index,x.item_version,a.state_version, \
         i.inventory_version,w.location_kind FROM item_instances x JOIN accounts a \
         USING (namespace_id,account_id) JOIN character_inventories i \
         USING (namespace_id,account_id,character_id) JOIN character_world_locations w \
         USING (namespace_id,account_id,character_id) WHERE x.namespace_id=$1 \
         AND x.account_id=$2 AND x.item_uid=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(SAFE_ITEM.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let ledger_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM item_ledger_events WHERE namespace_id=$1 AND account_id=$2 \
         AND mutation_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind([99_u8; 16].as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(state, (Some(CHARACTER_ID.to_vec()), 5, 0, 1, 1, 1, 1));
    assert_eq!(ledger_count, 0);
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn deliberate_risk_item_remains_pending_and_permits_danger_entry() {
    const PENDING_ITEM: [u8; 16] = [123; 16];
    let persistence = disposable_database().await;
    reset_fixture(&persistence).await;
    seed_deliberate_risk_item(&persistence, PENDING_ITEM).await;

    let accepted = coordinator(persistence.clone(), PostgresDangerEntryInventoryProviderV3)
        .handle(authenticated(ACCOUNT_ID), &frame(1, 100, CHARACTER_ID, 1))
        .await;
    assert_eq!(code(&accepted), WorldTransferResultCode::Accepted);

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let item: (Option<Vec<u8>>, i16, i16, i64) = sqlx::query_as(
        "SELECT character_id,security_state,location_kind,item_version FROM item_instances \
         WHERE namespace_id=$1 AND account_id=$2 AND item_uid=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(PENDING_ITEM.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let versions: (i64, i64) = sqlx::query_as(
        "SELECT state_version,inventory_version FROM accounts JOIN character_inventories \
         USING (namespace_id,account_id) WHERE namespace_id=$1 AND account_id=$2 \
         AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(item, (Some(CHARACTER_ID.to_vec()), 2, 2, 1));
    assert_eq!(versions, (1, 1));
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn concurrent_manual_transfer_and_entry_have_one_serial_storage_move() {
    const SAFE_ITEM: [u8; 16] = [124; 16];
    let persistence = disposable_database().await;
    reset_fixture(&persistence).await;
    seed_character_safe_item(&persistence, SAFE_ITEM).await;
    let entry = coordinator(persistence.clone(), PostgresDangerEntryInventoryProviderV3);
    let manual = PostgresSafeInventoryService::new(persistence.clone());
    let entry_frame = frame(1, 101, CHARACTER_ID, 1);
    let manual_command = SafeInventoryTransferCommand {
        mutation_id: [102; 16],
        kind: SafeInventoryTransferKind::CharacterSafeToVault,
        source_slot_index: 0,
        expected_account_version: 1,
        expected_inventory_version: 1,
    };

    let (entry_result, manual_result) = tokio::join!(
        entry.handle(authenticated(ACCOUNT_ID), &entry_frame),
        manual.transfer(ACCOUNT_ID, CHARACTER_ID, manual_command),
    );
    assert_eq!(code(&entry_result), WorldTransferResultCode::Accepted);
    assert!(matches!(
        manual_result,
        Ok(_)
            | Err(SafeInventoryServiceError::StaleVersion
                | SafeInventoryServiceError::BindingMismatch
                | SafeInventoryServiceError::HallBinding)
    ));

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let item: (Option<Vec<u8>>, i16, i64) = sqlx::query_as(
        "SELECT character_id,location_kind,item_version FROM item_instances \
         WHERE namespace_id=$1 AND account_id=$2 AND item_uid=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(SAFE_ITEM.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    let ledgers: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM item_ledger_events WHERE namespace_id=$1 AND account_id=$2 \
         AND item_uid=$3 AND mutation_id IN ($4,$5)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(SAFE_ITEM.as_slice())
    .bind([101_u8; 16].as_slice())
    .bind([102_u8; 16].as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(item, (None, 6, 2));
    assert_eq!(ledgers, 1);
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn concurrent_entry_has_one_lineage_and_provider_failure_rolls_back_every_row() {
    let persistence = disposable_database().await;
    reset_fixture(&persistence).await;
    let first = Arc::new(coordinator(
        persistence.clone(),
        PostgresDangerEntryInventoryProviderV3,
    ));
    let second = Arc::clone(&first);
    let first_frame = frame(1, 92, CHARACTER_ID, 1);
    let second_frame = frame(2, 93, CHARACTER_ID, 1);
    let (left, right) = tokio::join!(
        first.handle(authenticated(ACCOUNT_ID), &first_frame),
        second.handle(authenticated(ACCOUNT_ID), &second_frame),
    );
    let left_code = code(&left);
    let right_code = code(&right);
    assert!(
        matches!(
            (left_code, right_code),
            (
                WorldTransferResultCode::Accepted,
                WorldTransferResultCode::StateVersionMismatch
                    | WorldTransferResultCode::ServiceUnavailable
            ) | (
                WorldTransferResultCode::StateVersionMismatch
                    | WorldTransferResultCode::ServiceUnavailable,
                WorldTransferResultCode::Accepted
            )
        ),
        "unexpected concurrent results: left={left_code:?}, right={right_code:?}"
    );
    let transient_frame = match (left_code, right_code) {
        (WorldTransferResultCode::ServiceUnavailable, _) => Some(&first_frame),
        (_, WorldTransferResultCode::ServiceUnavailable) => Some(&second_frame),
        _ => None,
    };
    if let Some(frame) = transient_frame {
        let retried = first.handle(authenticated(ACCOUNT_ID), frame).await;
        assert_eq!(
            code(&retried),
            WorldTransferResultCode::StateVersionMismatch,
            "a serialization loser must become a durable state-version rejection on retry"
        );
    }
    assert_eq!(aggregate_counts(&persistence).await, (1, 1, 1, 2));

    reset_fixture(&persistence).await;
    let failed = coordinator(persistence.clone(), FailingInventory)
        .handle(authenticated(ACCOUNT_ID), &frame(3, 94, CHARACTER_ID, 1))
        .await;
    assert_eq!(
        code(&failed),
        WorldTransferResultCode::IncompleteRestorePoint
    );
    assert_eq!(aggregate_counts(&persistence).await, (0, 0, 0, 0));
    assert!(matches!(
        persistence
            .world_location(ACCOUNT_ID, CHARACTER_ID)
            .await
            .unwrap(),
        Some(persistence::StoredWorldLocation::Safe {
            character_version: 1,
            ..
        })
    ));
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn stale_foreign_and_corrupt_state_fail_closed_without_danger_allocation() {
    let persistence = disposable_database().await;
    reset_fixture(&persistence).await;
    let service = coordinator(persistence.clone(), PostgresDangerEntryInventoryProviderV3);
    let stale = service
        .handle(authenticated(ACCOUNT_ID), &frame(1, 95, CHARACTER_ID, 2))
        .await;
    assert_eq!(code(&stale), WorldTransferResultCode::StateVersionMismatch);
    let foreign = service
        .handle(
            authenticated(ACCOUNT_ID),
            &frame(2, 96, FOREIGN_CHARACTER_ID, 1),
        )
        .await;
    assert_eq!(code(&foreign), WorldTransferResultCode::CharacterNotOwned);
    assert_eq!(aggregate_counts(&persistence).await, (0, 0, 0, 1));

    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query(
        "UPDATE characters SET character_state_version = 2 WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let corrupt = service
        .handle(authenticated(ACCOUNT_ID), &frame(3, 97, CHARACTER_ID, 1))
        .await;
    assert_eq!(code(&corrupt), WorldTransferResultCode::ServiceUnavailable);
    assert_eq!(aggregate_counts(&persistence).await, (0, 0, 0, 1));
}
