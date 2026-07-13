use std::{path::PathBuf, sync::Arc};

use persistence::{
    PersistenceConfig, PersistenceTransaction, PostgresPersistence, WIPEABLE_CORE_NAMESPACE,
};
use protocol::{
    ManifestHash, WireText, WorldFlowContentRevisionV1, WorldFlowFrame, WorldFlowRequest,
    WorldFlowResult, WorldTransferCommand, WorldTransferMutation, WorldTransferPayload,
    WorldTransferResultCode,
};
use server_app::{
    AccountId, AuthenticatedAccount, AuthenticatedNamespace, BeltStackV1, CrashRestoreContext,
    EntryCaptureContext, EntryRestoreProvider, IdentityClock, InventorySecurityRestoreV1,
    OathBargainRestoreV1, PostgresDormantWorldFlowCoordinator, PostgresProgressionRestoreProvider,
    RestorePointError, WorldFlowIdGenerator,
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
struct PostgresFixtureInventory;

impl EntryRestoreProvider for PostgresFixtureInventory {
    type Snapshot = InventorySecurityRestoreV1;

    async fn capture<'a>(
        &'a self,
        transaction: &'a mut PersistenceTransaction<'_>,
        context: EntryCaptureContext,
    ) -> Result<Self::Snapshot, RestorePointError> {
        let inventory_version: i64 = sqlx::query_scalar(
            "SELECT inventory_version FROM character_inventories WHERE namespace_id = $1 \
             AND account_id = $2 AND character_id = $3 FOR UPDATE",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(context.account_id.as_slice())
        .bind(context.character_id.as_slice())
        .fetch_one(transaction.connection())
        .await
        .map_err(|_| RestorePointError::Persistence)?;
        Ok(InventorySecurityRestoreV1 {
            equipment: [None; 4],
            belt: [
                BeltStackV1 {
                    consumable_id: None,
                    unit_uids: vec![],
                },
                BeltStackV1 {
                    consumable_id: None,
                    unit_uids: vec![],
                },
            ],
            inventory_version: u64::try_from(inventory_version)
                .map_err(|_| RestorePointError::InvalidInventory)?,
        })
    }

    async fn restore_and_revoke_post_entry<'a>(
        &'a self,
        _transaction: &'a mut PersistenceTransaction<'_>,
        _context: CrashRestoreContext,
    ) -> Result<(), RestorePointError> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct PostgresFixtureOathBargains;

impl EntryRestoreProvider for PostgresFixtureOathBargains {
    type Snapshot = OathBargainRestoreV1;

    async fn capture<'a>(
        &'a self,
        transaction: &'a mut PersistenceTransaction<'_>,
        context: EntryCaptureContext,
    ) -> Result<Self::Snapshot, RestorePointError> {
        let (oath_id, earned_slots, version): (Option<String>, i16, i64) = sqlx::query_as(
            "SELECT c.oath_id, ob.earned_bargain_slots, ob.oath_bargain_version \
             FROM characters c JOIN character_oath_bargain_state ob \
             USING (namespace_id, account_id, character_id) WHERE c.namespace_id = $1 \
             AND c.account_id = $2 AND c.character_id = $3 FOR UPDATE OF c, ob",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(context.account_id.as_slice())
        .bind(context.character_id.as_slice())
        .fetch_one(transaction.connection())
        .await
        .map_err(|_| RestorePointError::Persistence)?;
        let active = sqlx::query_scalar::<_, String>(
            "SELECT bargain_id FROM character_active_bargains WHERE namespace_id = $1 \
             AND account_id = $2 AND character_id = $3 ORDER BY acquisition_ordinal FOR UPDATE",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(context.account_id.as_slice())
        .bind(context.character_id.as_slice())
        .fetch_all(transaction.connection())
        .await
        .map_err(|_| RestorePointError::Persistence)?
        .into_iter()
        .map(|id| WireText::new(id).map_err(|_| RestorePointError::InvalidOathBargains))
        .collect::<Result<Vec<_>, _>>()?;
        Ok(OathBargainRestoreV1 {
            oath_id: oath_id
                .map(|id| WireText::new(id).map_err(|_| RestorePointError::InvalidOathBargains))
                .transpose()?,
            active_bargain_ids: active,
            earned_bargain_slots: u8::try_from(earned_slots)
                .map_err(|_| RestorePointError::InvalidOathBargains)?,
            oath_bargain_version: u64::try_from(version)
                .map_err(|_| RestorePointError::InvalidOathBargains)?,
        })
    }

    async fn restore_and_revoke_post_entry<'a>(
        &'a self,
        _transaction: &'a mut PersistenceTransaction<'_>,
        _context: CrashRestoreContext,
    ) -> Result<(), RestorePointError> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct FailingInventory;

impl EntryRestoreProvider for FailingInventory {
    type Snapshot = InventorySecurityRestoreV1;

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
) -> PostgresDormantWorldFlowCoordinator<FixedIds, FixedClock, Inventory, PostgresFixtureOathBargains>
where
    Inventory: EntryRestoreProvider<Snapshot = InventorySecurityRestoreV1>,
{
    let progression = sim_content::load_core_development_progression(&content_root()).unwrap();
    PostgresDormantWorldFlowCoordinator::new(
        persistence,
        FixedIds,
        FixedClock,
        revision(),
        PostgresProgressionRestoreProvider::new(&progression).unwrap(),
        inventory,
        PostgresFixtureOathBargains,
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

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn danger_entry_commits_complete_root_and_replays_after_pool_restart() {
    let persistence = disposable_database().await;
    reset_fixture(&persistence).await;
    let request = frame(1, 91, CHARACTER_ID, 1);
    let service = coordinator(persistence.clone(), PostgresFixtureInventory);
    let accepted = service.handle(authenticated(ACCOUNT_ID), &request).await;
    assert_eq!(code(&accepted), WorldTransferResultCode::Accepted);
    assert_eq!(aggregate_counts(&persistence).await, (1, 1, 1, 1));

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let root: (
        String,
        String,
        String,
        i64,
        i64,
        i64,
        i64,
        i64,
        i16,
        Vec<u8>,
    ) = sqlx::query_as(
        "SELECT l.content_id, l.layout_id, r.source_location_id, r.account_version, \
                    r.character_version, r.progression_version, r.inventory_version, \
                    r.oath_bargain_version, r.component_mask, r.composite_digest \
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
    assert_eq!(root.0, WORLD_ID);
    assert_eq!(root.1, LAYOUT_ID);
    assert_eq!(root.2, HALL_ID);
    assert_eq!(
        (root.3, root.4, root.5, root.6, root.7, root.8),
        (1, 1, 1, 1, 1, 7)
    );
    assert_eq!(root.9.len(), 32);
    transaction.rollback().await.unwrap();

    drop(service);
    persistence.close().await;
    let restarted = disposable_database().await;
    let replay = coordinator(restarted.clone(), PostgresFixtureInventory)
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
    let conflicted = coordinator(restarted.clone(), PostgresFixtureInventory)
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
async fn concurrent_entry_has_one_lineage_and_provider_failure_rolls_back_every_row() {
    let persistence = disposable_database().await;
    reset_fixture(&persistence).await;
    let first = Arc::new(coordinator(persistence.clone(), PostgresFixtureInventory));
    let second = Arc::clone(&first);
    let first_frame = frame(1, 92, CHARACTER_ID, 1);
    let second_frame = frame(2, 93, CHARACTER_ID, 1);
    let (left, right) = tokio::join!(
        first.handle(authenticated(ACCOUNT_ID), &first_frame),
        second.handle(authenticated(ACCOUNT_ID), &second_frame),
    );
    assert!(matches!(
        (code(&left), code(&right)),
        (
            WorldTransferResultCode::Accepted,
            WorldTransferResultCode::StateVersionMismatch
        ) | (
            WorldTransferResultCode::StateVersionMismatch,
            WorldTransferResultCode::Accepted
        )
    ));
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
    let service = coordinator(persistence.clone(), PostgresFixtureInventory);
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
