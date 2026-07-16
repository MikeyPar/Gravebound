//! Replay-first serializable writer for the GB-M03 Emergency Recall terminal.

use std::collections::BTreeMap;

use sim_core::{
    DurableStorageSlot, EQUIPMENT_SLOT_COUNT, ItemUid, RUN_BACKPACK_CAPACITY,
    RecallInventorySnapshot, RecallItemLocation, RecallMaterialSnapshot, RecallPersonalGroundStack,
    TERMINAL_BELT_CAPACITY, plan_emergency_recall,
};
use sqlx::{PgConnection, Row};

use crate::{
    PersistenceError, PostgresPersistence, PreparedProductionRecallV1,
    ProductionRecallCommitRequestV1, ProductionRecallTransactionV1,
    ProductionRecallVersionAdvanceV1, ProductionRecallVersionsV1, StoredProductionRecallItemV1,
    StoredProductionRecallMaterialDestructionV1, StoredProductionRecallResultV1,
    StoredRecallLocationV1, WIPEABLE_CORE_NAMESPACE, canonical_production_recall_plan_hash_v1,
    is_retryable_transaction_failure, stage_danger_checkpoint_cleanup,
};

const MAX_TRANSACTION_ATTEMPTS: u8 = 3;
const ID_BYTES: usize = 16;
const HASH_BYTES: usize = 32;
const RESTORE_ACTIVE: i16 = 0;
const RESTORE_RECALL_COMMITTED: i16 = 3;
const LINEAGE_CLOSED_SUCCESS: i16 = 2;
const LOCATION_SAFE: i16 = 1;
const LOCATION_DANGER: i16 = 2;
const SECURITY_NORMAL: i16 = 0;
const SECURITY_AT_RISK_EQUIPPED: i16 = 1;
const SECURITY_AT_RISK_PENDING: i16 = 2;
const SECURITY_DESTROYED: i16 = 3;
const MATERIAL_DESTROYED: i16 = 3;

const STABILIZATION_LEDGER_ID_CONTEXT: &str =
    "gravebound.production-recall-stabilization-ledger.v1";
const DESTRUCTION_LEDGER_ID_CONTEXT: &str = "gravebound.production-recall-destruction-ledger.v1";
const MATERIAL_DESTRUCTION_ID_CONTEXT: &str =
    "gravebound.production-recall-material-destruction.v1";
const ACCEPTED_AUDIT_ID_CONTEXT: &str = "gravebound.production-recall-audit.v1";
const CONFLICT_AUDIT_ID_CONTEXT: &str = "gravebound.production-recall-conflict-audit.v1";
const OUTBOX_ID_CONTEXT: &str = "gravebound.production-recall-outbox.v1";
const BINDING_LOCK_ID_CONTEXT: &str = "gravebound.production-recall-binding-lock.v1";

#[derive(Debug, Clone)]
struct LockedRecallRoot {
    records_blake3: String,
    assets_blake3: String,
    localization_blake3: String,
    restore_state: i16,
}

#[derive(Debug, Clone)]
struct LockedRecallItem {
    item_uid: [u8; ID_BYTES],
    template_id: String,
    content_revision: String,
    item_kind: i16,
    item_version: u64,
    security_state: i16,
    location_kind: i16,
    slot_index: Option<u16>,
    instance_id: Option<[u8; ID_BYTES]>,
    pickup_id: Option<[u8; ID_BYTES]>,
    expires_at_tick: Option<u64>,
}

#[derive(Debug, Clone)]
struct LockedRecallMaterial {
    material_id: String,
    quantity: u16,
    version: u64,
}

#[derive(Debug)]
struct LockedRecallAuthority {
    account_version: u64,
    character_version: u64,
    world_version: u64,
    inventory_version: u64,
    life_metrics_version: u64,
    lifetime_ticks: u64,
    permadeath_combat_ticks: u64,
    progression_version: u64,
    oath_bargain_version: u64,
    ash_wallet_version: u64,
    source_content_id: String,
    root: LockedRecallRoot,
}

#[derive(Debug)]
struct LockedProductionRecallPlan {
    authority: LockedRecallAuthority,
    materials: BTreeMap<String, LockedRecallMaterial>,
    stabilized_items: Vec<StoredProductionRecallItemV1>,
    destroyed_items: Vec<StoredProductionRecallItemV1>,
    destroyed_materials: Vec<StoredProductionRecallMaterialDestructionV1>,
    post_inventory_version: u64,
}

impl LockedProductionRecallPlan {
    fn canonical_plan_hash(&self) -> Result<[u8; HASH_BYTES], PersistenceError> {
        canonical_production_recall_plan_hash_v1(
            &self.stabilized_items,
            &self.destroyed_items,
            &self.destroyed_materials,
        )
    }

    fn stored_result(
        &self,
        request: &ProductionRecallCommitRequestV1,
        canonical_request_hash: [u8; HASH_BYTES],
        canonical_plan_hash: [u8; HASH_BYTES],
        committed_at_unix_ms: u64,
    ) -> Result<StoredProductionRecallResultV1, PersistenceError> {
        let result = StoredProductionRecallResultV1 {
            contract_version: request.contract_version,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            account_id: request.account_id,
            character_id: request.character_id,
            mutation_id: request.mutation_id,
            terminal_id: request.terminal_id,
            canonical_request_hash,
            canonical_plan_hash,
            result_code: 1,
            trigger: request.trigger,
            request_sequence: request.request_sequence,
            explicit_client_tick: request.explicit_client_tick,
            issued_at_unix_ms: request.issued_at_unix_ms,
            trigger_started_tick: request.trigger_started_tick,
            completion_tick: request.completion_tick,
            committed_at_unix_ms,
            source_content_id: self.authority.source_content_id.clone(),
            destination_content_id: crate::PRODUCTION_RECALL_HALL_ID.into(),
            versions: ProductionRecallVersionsV1 {
                account: unchanged(self.authority.account_version),
                character: advance(self.authority.character_version)?,
                world: advance(self.authority.world_version)?,
                inventory: ProductionRecallVersionAdvanceV1 {
                    pre: self.authority.inventory_version,
                    post: self.post_inventory_version,
                },
                life_metrics: advance(self.authority.life_metrics_version)?,
                progression: unchanged(self.authority.progression_version),
                oath_bargain: unchanged(self.authority.oath_bargain_version),
                ash_wallet: unchanged(self.authority.ash_wallet_version),
            },
            pre_lifetime_ticks: self.authority.lifetime_ticks,
            post_lifetime_ticks: request.final_lifetime_ticks,
            pre_permadeath_combat_ticks: self.authority.permadeath_combat_ticks,
            post_permadeath_combat_ticks: request.final_permadeath_combat_ticks,
            stabilized_items: self.stabilized_items.clone(),
            destroyed_items: self.destroyed_items.clone(),
            destroyed_materials: self.destroyed_materials.clone(),
        };
        result.validate()?;
        Ok(result)
    }
}

impl PostgresPersistence {
    /// Plans one exact Recall from locked durable custody and rolls the transaction back.
    ///
    /// Successful preparation is read-only. An altered replay writes only the mandatory
    /// deduplicated conflict audit before returning the typed conflict.
    pub async fn prepare_production_recall_v1(
        &self,
        request: &ProductionRecallCommitRequestV1,
    ) -> Result<PreparedProductionRecallV1, PersistenceError> {
        request.validate()?;
        for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self.prepare_production_recall_once_v1(request).await {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS
                        && is_retryable_transaction_failure(&error) => {}
                outcome => return outcome,
            }
        }
        Err(PersistenceError::ProductionRecallTerminalSuperseded)
    }

    async fn prepare_production_recall_once_v1(
        &self,
        request: &ProductionRecallCommitRequestV1,
    ) -> Result<PreparedProductionRecallV1, PersistenceError> {
        let request_hash = request.canonical_hash()?;
        let mut transaction = self.begin_transaction().await?;
        lock_recall_terminal_identities(transaction.connection(), request).await?;
        lock_recall_account(transaction.connection(), request.account_id).await?;
        if let Some(stored) =
            load_existing_recall_terminal(transaction.connection(), request).await?
        {
            if exact_recall_request_replay(&stored, request, request_hash) {
                transaction.rollback().await?;
                return PreparedProductionRecallV1::seal(
                    request.clone(),
                    request_hash,
                    stored.canonical_plan_hash,
                    true,
                );
            }
            if stored.canonical_request_hash == request_hash {
                transaction.rollback().await?;
                return Err(PersistenceError::CorruptStoredRecall);
            }
            insert_recall_conflict_audit(transaction.connection(), &stored, request, request_hash)
                .await?;
            transaction.commit().await?;
            return Err(PersistenceError::RecallIdempotencyConflict);
        }
        let plan = lock_and_plan_production_recall(transaction.connection(), request).await?;
        let plan_hash = plan.canonical_plan_hash()?;
        transaction.rollback().await?;
        PreparedProductionRecallV1::seal(request.clone(), request_hash, plan_hash, false)
    }

    pub async fn commit_production_recall_v1(
        &self,
        request: &ProductionRecallCommitRequestV1,
        expected_plan_hash: [u8; HASH_BYTES],
    ) -> Result<ProductionRecallTransactionV1, PersistenceError> {
        request.validate()?;
        if expected_plan_hash == [0; HASH_BYTES] {
            return Err(PersistenceError::ProductionRecallPlanChanged);
        }
        for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self
                .commit_production_recall_once_v1(request, expected_plan_hash)
                .await
            {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS
                        && is_retryable_transaction_failure(&error) => {}
                outcome => return outcome,
            }
        }
        Err(PersistenceError::ProductionRecallTerminalSuperseded)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the complete Recall write is deliberately one auditable serializable state machine"
    )]
    async fn commit_production_recall_once_v1(
        &self,
        request: &ProductionRecallCommitRequestV1,
        expected_plan_hash: [u8; HASH_BYTES],
    ) -> Result<ProductionRecallTransactionV1, PersistenceError> {
        let request_hash = request.canonical_hash()?;
        let mut transaction = self.begin_transaction().await?;
        lock_recall_terminal_identities(transaction.connection(), request).await?;
        lock_recall_account(transaction.connection(), request.account_id).await?;

        if let Some(stored) =
            load_existing_recall_terminal(transaction.connection(), request).await?
        {
            if exact_recall_request_replay(&stored, request, request_hash) {
                if stored.canonical_plan_hash != expected_plan_hash {
                    transaction.rollback().await?;
                    return Err(PersistenceError::ProductionRecallPlanChanged);
                }
                transaction.rollback().await?;
                return Ok(ProductionRecallTransactionV1::Replayed(stored));
            }
            if stored.canonical_request_hash == request_hash {
                transaction.rollback().await?;
                return Err(PersistenceError::CorruptStoredRecall);
            }
            insert_recall_conflict_audit(transaction.connection(), &stored, request, request_hash)
                .await?;
            transaction.commit().await?;
            return Ok(ProductionRecallTransactionV1::Conflict {
                terminal_id: stored.terminal_id,
            });
        }

        let plan = lock_and_plan_production_recall(transaction.connection(), request).await?;
        let canonical_plan_hash = plan.canonical_plan_hash()?;
        if canonical_plan_hash != expected_plan_hash {
            transaction.rollback().await?;
            return Err(PersistenceError::ProductionRecallPlanChanged);
        }
        let committed_at_unix_ms = transaction_timestamp_ms(transaction.connection()).await?;
        let result = plan.stored_result(
            request,
            request_hash,
            canonical_plan_hash,
            committed_at_unix_ms,
        )?;
        let result_payload = result.encode()?;
        let result_hash = result.digest()?;

        insert_recall_terminal_root(
            transaction.connection(),
            request,
            &plan.authority,
            &result,
            result_hash,
            &result_payload,
        )
        .await?;
        apply_recall_items(transaction.connection(), request, &result).await?;
        apply_recall_materials(transaction.connection(), request, &result, &plan.materials).await?;
        apply_recall_aggregate_heads(transaction.connection(), request, &result).await?;
        close_recall_danger_root(transaction.connection(), request).await?;
        stage_danger_checkpoint_cleanup(
            &mut transaction,
            request.account_id,
            request.character_id,
            request.instance_lineage_id,
        )
        .await?;
        insert_recall_audit_and_outbox(
            transaction.connection(),
            request,
            result_hash,
            &result_payload,
        )
        .await?;
        force_deferred_constraints(transaction.connection()).await?;
        transaction.commit().await?;
        Ok(ProductionRecallTransactionV1::Fresh(result))
    }
}

fn exact_recall_request_replay(
    stored: &StoredProductionRecallResultV1,
    request: &ProductionRecallCommitRequestV1,
    request_hash: [u8; HASH_BYTES],
) -> bool {
    stored.canonical_request_hash == request_hash
        && stored.account_id == request.account_id
        && stored.character_id == request.character_id
        && stored.mutation_id == request.mutation_id
        && stored.terminal_id == request.terminal_id
        && stored.trigger == request.trigger
        && stored.request_sequence == request.request_sequence
        && stored.explicit_client_tick == request.explicit_client_tick
        && stored.trigger_started_tick == request.trigger_started_tick
        && stored.completion_tick == request.completion_tick
        && stored.post_lifetime_ticks == request.final_lifetime_ticks
        && stored.post_permadeath_combat_ticks == request.final_permadeath_combat_ticks
}

async fn lock_and_plan_production_recall(
    connection: &mut PgConnection,
    request: &ProductionRecallCommitRequestV1,
) -> Result<LockedProductionRecallPlan, PersistenceError> {
    let authority = lock_and_validate_recall_authority(connection, request).await?;
    reject_recall_unresolved_reward_mutation(connection, request).await?;
    let (items, snapshot) = load_recall_inventory_snapshot(connection, request, &authority).await?;
    let (materials, material_snapshots) =
        load_recall_material_snapshot(connection, request).await?;
    let snapshot = RecallInventorySnapshot {
        materials: material_snapshots,
        ..snapshot
    };
    let plan = plan_emergency_recall(&snapshot)
        .map_err(|_| PersistenceError::ProductionRecallPlanningFailed)?;
    let stabilized_items = build_recall_items(
        &items,
        request,
        &plan.stabilized_items,
        RecallProjectionKind::Stabilized,
    )?;
    let destroyed_items = build_recall_items(
        &items,
        request,
        &plan.destroyed_items,
        RecallProjectionKind::Destroyed,
    )?;
    let destroyed_materials =
        build_recall_materials(request, &plan.destroyed_materials, &materials)?;
    Ok(LockedProductionRecallPlan {
        authority,
        materials,
        stabilized_items,
        destroyed_items,
        destroyed_materials,
        post_inventory_version: plan.post_inventory_version,
    })
}

async fn lock_recall_terminal_identities(
    connection: &mut PgConnection,
    request: &ProductionRecallCommitRequestV1,
) -> Result<(), PersistenceError> {
    let binding_id = derived_id(
        BINDING_LOCK_ID_CONTEXT,
        &[
            &request.account_id,
            &request.character_id,
            &request.entry_restore_point_id,
            &request.instance_lineage_id,
        ],
    );
    let mut lock_keys = [
        recall_terminal_advisory_key(0, request.mutation_id),
        recall_terminal_advisory_key(1, request.terminal_id),
        recall_terminal_advisory_key(2, binding_id),
    ];
    lock_keys.sort_unstable();
    for lock_key in lock_keys {
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(lock_key)
            .execute(&mut *connection)
            .await?;
    }
    Ok(())
}

async fn lock_recall_account(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
) -> Result<(), PersistenceError> {
    let found: Option<i32> = sqlx::query_scalar(
        "SELECT 1 FROM accounts WHERE namespace_id=$1 AND account_id=$2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_optional(connection)
    .await?;
    if found.is_none() {
        return Err(PersistenceError::ProductionRecallOwnerNotFound);
    }
    Ok(())
}

async fn load_existing_recall_terminal(
    connection: &mut PgConnection,
    request: &ProductionRecallCommitRequestV1,
) -> Result<Option<StoredProductionRecallResultV1>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT account_id,character_id,mutation_id,terminal_id,
                canonical_request_hash,result_hash,result_payload
         FROM character_recall_terminal_results_v1
         WHERE namespace_id=$1
           AND ((account_id=$2 AND mutation_id=$3)
             OR terminal_id=$4
             OR (account_id=$2 AND character_id=$5
                 AND entry_restore_point_id=$6 AND instance_lineage_id=$7))
         ORDER BY terminal_id
         FOR SHARE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(request.terminal_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.entry_restore_point_id.as_slice())
    .bind(request.instance_lineage_id.as_slice())
    .fetch_all(connection)
    .await?;
    let [row] = rows.as_slice() else {
        return if rows.is_empty() {
            Ok(None)
        } else {
            Err(PersistenceError::CorruptStoredRecall)
        };
    };
    let result = StoredProductionRecallResultV1::decode(
        row.try_get::<Vec<u8>, _>("result_payload")?.as_slice(),
    )?;
    if result.account_id != exact_id(row.try_get("account_id")?)?
        || result.character_id != exact_id(row.try_get("character_id")?)?
        || result.mutation_id != exact_id(row.try_get("mutation_id")?)?
        || result.terminal_id != exact_id(row.try_get("terminal_id")?)?
        || result.canonical_request_hash != exact_hash(row.try_get("canonical_request_hash")?)?
        || result.digest()? != exact_hash(row.try_get("result_hash")?)?
    {
        return Err(PersistenceError::CorruptStoredRecall);
    }
    Ok(Some(result))
}

async fn insert_recall_conflict_audit(
    connection: &mut PgConnection,
    stored: &StoredProductionRecallResultV1,
    attempted: &ProductionRecallCommitRequestV1,
    attempted_hash: [u8; HASH_BYTES],
) -> Result<(), PersistenceError> {
    if attempted_hash == stored.canonical_request_hash {
        return Err(PersistenceError::CorruptStoredRecall);
    }
    let audit_id = derived_id(
        CONFLICT_AUDIT_ID_CONTEXT,
        &[
            &stored.terminal_id,
            &attempted_hash,
            &attempted.mutation_id,
            &attempted.terminal_id,
        ],
    );
    sqlx::query(
        "INSERT INTO recall_terminal_conflict_audits_v1
         (namespace_id,stored_terminal_id,conflict_audit_id,
          attempted_account_id,attempted_character_id,attempted_mutation_id,
          attempted_terminal_id,attempted_trigger_kind,stored_request_hash,
          attempted_request_hash,attempted_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,transaction_timestamp())
         ON CONFLICT (namespace_id,stored_terminal_id,attempted_request_hash) DO NOTHING",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(stored.terminal_id.as_slice())
    .bind(audit_id.as_slice())
    .bind(attempted.account_id.as_slice())
    .bind(attempted.character_id.as_slice())
    .bind(attempted.mutation_id.as_slice())
    .bind(attempted.terminal_id.as_slice())
    .bind(i16::from(recall_trigger_code(attempted.trigger)))
    .bind(stored.canonical_request_hash.as_slice())
    .bind(attempted_hash.as_slice())
    .execute(connection)
    .await?;
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "lock order and every preserved authority comparison are intentionally visible together"
)]
async fn lock_and_validate_recall_authority(
    connection: &mut PgConnection,
    request: &ProductionRecallCommitRequestV1,
) -> Result<LockedRecallAuthority, PersistenceError> {
    let account = sqlx::query(
        "SELECT state_version,selected_character_id FROM accounts
         WHERE namespace_id=$1 AND account_id=$2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ProductionRecallOwnerNotFound)?;
    let character = sqlx::query(
        "SELECT life_state,security_state,character_state_version FROM characters
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ProductionRecallOwnerNotFound)?;
    let inventory = sqlx::query(
        "SELECT inventory_version FROM character_inventories
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ProductionRecallOwnerNotFound)?;
    let root = sqlx::query(
        "SELECT restore_state,records_blake3,assets_blake3,localization_blake3
         FROM character_entry_restore_points
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
           AND restore_point_id=$4 AND lineage_id=$5 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.entry_restore_point_id.as_slice())
    .bind(request.instance_lineage_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ProductionRecallBindingMismatch)?;
    let world = sqlx::query(
        "SELECT character_version,location_kind,location_content_id,
                instance_lineage_id,entry_restore_point_id
         FROM character_world_locations
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ProductionRecallBindingMismatch)?;
    let lineage = sqlx::query(
        "SELECT content_id,lineage_state,records_blake3,assets_blake3,localization_blake3
         FROM character_instance_lineages
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND lineage_id=$4
         FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.instance_lineage_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ProductionRecallBindingMismatch)?;
    let progression = sqlx::query(
        "SELECT progression_version FROM character_progression
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ProductionRecallOwnerNotFound)?;
    let oath_bargain = sqlx::query(
        "SELECT oath_bargain_version FROM character_oath_bargain_state
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ProductionRecallOwnerNotFound)?;
    let life = sqlx::query(
        "SELECT lifetime_ticks,permadeath_combat_ticks,life_metrics_version
         FROM character_life_metrics
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ProductionRecallOwnerNotFound)?;
    let ash = sqlx::query(
        "SELECT wallet_version FROM ash_wallets
         WHERE namespace_id=$1 AND account_id=$2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ProductionRecallOwnerNotFound)?;

    let account_version = positive(account.try_get("state_version")?)?;
    let character_version = positive(character.try_get("character_state_version")?)?;
    let world_version = positive(world.try_get("character_version")?)?;
    let inventory_version = positive(inventory.try_get("inventory_version")?)?;
    let life_metrics_version = positive(life.try_get("life_metrics_version")?)?;
    let progression_version = positive(progression.try_get("progression_version")?)?;
    let oath_bargain_version = positive(oath_bargain.try_get("oath_bargain_version")?)?;
    let ash_wallet_version = positive(ash.try_get("wallet_version")?)?;
    if account_version != request.expected_versions.account
        || character_version != request.expected_versions.character
        || world_version != request.expected_versions.world
        || inventory_version != request.expected_versions.inventory
        || life_metrics_version != request.expected_versions.life_metrics
        || progression_version != request.expected_versions.progression
        || oath_bargain_version != request.expected_versions.oath_bargain
        || ash_wallet_version != request.expected_versions.ash_wallet
    {
        return Err(PersistenceError::ProductionRecallVersionMismatch {
            account: account_version,
            character: character_version,
            world: world_version,
            inventory: inventory_version,
            life_metrics: life_metrics_version,
            progression: progression_version,
            oath_bargain: oath_bargain_version,
            ash_wallet: ash_wallet_version,
        });
    }

    let lifetime_ticks = nonnegative(life.try_get("lifetime_ticks")?)?;
    let permadeath_combat_ticks = nonnegative(life.try_get("permadeath_combat_ticks")?)?;
    let root = LockedRecallRoot {
        records_blake3: root.try_get("records_blake3")?,
        assets_blake3: root.try_get("assets_blake3")?,
        localization_blake3: root.try_get("localization_blake3")?,
        restore_state: root.try_get("restore_state")?,
    };
    let source_content_id: String = lineage.try_get("content_id")?;
    if root.records_blake3 != request.content_revision.records_blake3
        || root.assets_blake3 != request.content_revision.assets_blake3
        || root.localization_blake3 != request.content_revision.localization_blake3
        || lineage.try_get::<String, _>("records_blake3")? != root.records_blake3
        || lineage.try_get::<String, _>("assets_blake3")? != root.assets_blake3
        || lineage.try_get::<String, _>("localization_blake3")? != root.localization_blake3
    {
        return Err(PersistenceError::ProductionRecallContentMismatch);
    }
    if optional_id(account.try_get("selected_character_id")?)? != Some(request.character_id)
        || character.try_get::<i16, _>("life_state")? != 0
        || character.try_get::<i16, _>("security_state")? != SECURITY_NORMAL
        || root.restore_state != RESTORE_ACTIVE
        || world.try_get::<i16, _>("location_kind")? != LOCATION_DANGER
        || world.try_get::<String, _>("location_content_id")? != source_content_id
        || optional_id(world.try_get("instance_lineage_id")?)? != Some(request.instance_lineage_id)
        || optional_id(world.try_get("entry_restore_point_id")?)?
            != Some(request.entry_restore_point_id)
        || !matches!(lineage.try_get::<i16, _>("lineage_state")?, 0 | 1)
        || request.final_lifetime_ticks < lifetime_ticks
        || request.final_permadeath_combat_ticks < permadeath_combat_ticks
    {
        return Err(PersistenceError::ProductionRecallBindingMismatch);
    }
    Ok(LockedRecallAuthority {
        account_version,
        character_version,
        world_version,
        inventory_version,
        life_metrics_version,
        lifetime_ticks,
        permadeath_combat_ticks,
        progression_version,
        oath_bargain_version,
        ash_wallet_version,
        source_content_id,
        root,
    })
}

async fn reject_recall_unresolved_reward_mutation(
    connection: &mut PgConnection,
    request: &ProductionRecallCommitRequestV1,
) -> Result<(), PersistenceError> {
    let unresolved: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT reward_request_id
         FROM reward_requests
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND request_state=0
         ORDER BY reward_request_id
         LIMIT 1
         FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_optional(connection)
    .await?;
    if unresolved.is_some() {
        return Err(PersistenceError::ProductionRecallUnresolvedMutation);
    }
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "one contiguous loader makes every durable Recall custody shape auditable"
)]
async fn load_recall_inventory_snapshot(
    connection: &mut PgConnection,
    request: &ProductionRecallCommitRequestV1,
    authority: &LockedRecallAuthority,
) -> Result<
    (
        BTreeMap<[u8; ID_BYTES], LockedRecallItem>,
        RecallInventorySnapshot,
    ),
    PersistenceError,
> {
    let rows = sqlx::query(
        "SELECT item_uid,template_id,content_revision,item_kind,item_version,security_state,
                location_kind,slot_index,instance_id,pickup_id,expires_at_tick
         FROM item_instances
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
           AND location_kind IN (0,1,2,3)
         ORDER BY item_uid
         FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_all(connection)
    .await?;
    let mut items = BTreeMap::new();
    let mut equipped = vec![DurableStorageSlot::Empty; EQUIPMENT_SLOT_COUNT];
    let mut belt = vec![DurableStorageSlot::Empty; TERMINAL_BELT_CAPACITY];
    let mut backpack = vec![DurableStorageSlot::Empty; RUN_BACKPACK_CAPACITY];
    let mut ground = BTreeMap::<([u8; ID_BYTES], [u8; ID_BYTES], u64), DurableStorageSlot>::new();
    for row in rows {
        let item = LockedRecallItem {
            item_uid: exact_id(row.try_get("item_uid")?)?,
            template_id: row.try_get("template_id")?,
            content_revision: row.try_get("content_revision")?,
            item_kind: row.try_get("item_kind")?,
            item_version: positive(row.try_get("item_version")?)?,
            security_state: row.try_get("security_state")?,
            location_kind: row.try_get("location_kind")?,
            slot_index: optional_u16(row.try_get("slot_index")?)?,
            instance_id: optional_id(row.try_get("instance_id")?)?,
            pickup_id: optional_id(row.try_get("pickup_id")?)?,
            expires_at_tick: optional_positive(row.try_get("expires_at_tick")?)?,
        };
        let expected_security = match item.location_kind {
            0 | 1 => SECURITY_AT_RISK_EQUIPPED,
            2 | 3 => SECURITY_AT_RISK_PENDING,
            _ => return Err(PersistenceError::CorruptStoredRecall),
        };
        if item.security_state != expected_security {
            return Err(PersistenceError::ProductionRecallBindingMismatch);
        }
        if item.content_revision != crate::CORE_ITEM_CONTENT_REVISION {
            return Err(PersistenceError::ProductionRecallContentMismatch);
        }
        match item.location_kind {
            0 => append_recall_slot(
                &mut equipped,
                item.slot_index
                    .ok_or(PersistenceError::CorruptStoredRecall)?,
                &item,
            )?,
            1 => append_recall_slot(
                &mut belt,
                item.slot_index
                    .ok_or(PersistenceError::CorruptStoredRecall)?,
                &item,
            )?,
            2 => append_recall_slot(
                &mut backpack,
                item.slot_index
                    .ok_or(PersistenceError::CorruptStoredRecall)?,
                &item,
            )?,
            3 => {
                let key = (
                    item.instance_id
                        .ok_or(PersistenceError::CorruptStoredRecall)?,
                    item.pickup_id
                        .ok_or(PersistenceError::CorruptStoredRecall)?,
                    item.expires_at_tick
                        .ok_or(PersistenceError::CorruptStoredRecall)?,
                );
                let slot = ground.entry(key).or_insert(DurableStorageSlot::Empty);
                append_recall_stack(slot, &item)?;
            }
            _ => unreachable!(),
        }
        if items.insert(item.item_uid, item).is_some() {
            return Err(PersistenceError::CorruptStoredRecall);
        }
    }
    let personal_ground = ground
        .into_iter()
        .map(
            |((instance_id, pickup_id, expires_at_tick), stack)| RecallPersonalGroundStack {
                instance_id,
                pickup_id,
                expires_at_tick,
                stack,
            },
        )
        .collect();
    Ok((
        items,
        RecallInventorySnapshot {
            account_version: authority.account_version,
            inventory_version: authority.inventory_version,
            equipped,
            belt,
            run_backpack: backpack,
            personal_ground,
            materials: Vec::new(),
        },
    ))
}

fn append_recall_slot(
    slots: &mut [DurableStorageSlot],
    index: u16,
    item: &LockedRecallItem,
) -> Result<(), PersistenceError> {
    let slot = slots
        .get_mut(usize::from(index))
        .ok_or(PersistenceError::CorruptStoredRecall)?;
    append_recall_stack(slot, item)
}

fn append_recall_stack(
    slot: &mut DurableStorageSlot,
    item: &LockedRecallItem,
) -> Result<(), PersistenceError> {
    let uid = ItemUid::new(item.item_uid).map_err(|_| PersistenceError::CorruptStoredRecall)?;
    match (item.item_kind, slot) {
        (0, slot @ DurableStorageSlot::Empty) => {
            *slot = DurableStorageSlot::Equipment { item_uid: uid };
        }
        (1, slot @ DurableStorageSlot::Empty) => {
            *slot = DurableStorageSlot::Consumable {
                template_id: item.template_id.clone(),
                item_uids: vec![uid],
            };
        }
        (
            1,
            DurableStorageSlot::Consumable {
                template_id,
                item_uids,
            },
        ) if *template_id == item.template_id => {
            item_uids.push(uid);
            item_uids.sort_unstable();
        }
        _ => return Err(PersistenceError::CorruptStoredRecall),
    }
    Ok(())
}

async fn load_recall_material_snapshot(
    connection: &mut PgConnection,
    request: &ProductionRecallCommitRequestV1,
) -> Result<
    (
        BTreeMap<String, LockedRecallMaterial>,
        Vec<RecallMaterialSnapshot>,
    ),
    PersistenceError,
> {
    let rows = sqlx::query(
        "SELECT material_id,quantity,material_version
         FROM character_run_material_stacks
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
           AND security_state=2 AND quantity>0
         ORDER BY material_id COLLATE \"C\"
         FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_all(connection)
    .await?;
    let mut materials = BTreeMap::new();
    let mut snapshots = Vec::new();
    for row in rows {
        let material = LockedRecallMaterial {
            material_id: row.try_get("material_id")?,
            quantity: u16_from_i32(row.try_get("quantity")?)?,
            version: positive(row.try_get("material_version")?)?,
        };
        snapshots.push(RecallMaterialSnapshot {
            material_id: material.material_id.clone(),
            pending_quantity: material.quantity,
            pouch_version: material.version,
        });
        if materials
            .insert(material.material_id.clone(), material)
            .is_some()
        {
            return Err(PersistenceError::CorruptStoredRecall);
        }
    }
    Ok((materials, snapshots))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecallProjectionKind {
    Stabilized,
    Destroyed,
}

fn build_recall_items(
    items: &BTreeMap<[u8; ID_BYTES], LockedRecallItem>,
    request: &ProductionRecallCommitRequestV1,
    planned: &[sim_core::RecallItemMutation],
    kind: RecallProjectionKind,
) -> Result<Vec<StoredProductionRecallItemV1>, PersistenceError> {
    planned
        .iter()
        .enumerate()
        .map(|(index, mutation)| {
            let item_uid = mutation.item_uid.bytes();
            let item = items
                .get(&item_uid)
                .ok_or(PersistenceError::CorruptStoredRecall)?;
            let source = stored_recall_location(mutation.source);
            if !recall_source_matches(item, source) {
                return Err(PersistenceError::CorruptStoredRecall);
            }
            let context = match kind {
                RecallProjectionKind::Stabilized => STABILIZATION_LEDGER_ID_CONTEXT,
                RecallProjectionKind::Destroyed => DESTRUCTION_LEDGER_ID_CONTEXT,
            };
            Ok(StoredProductionRecallItemV1 {
                ordinal: u16::try_from(index).map_err(|_| PersistenceError::CorruptStoredRecall)?,
                item_uid,
                template_id: item.template_id.clone(),
                content_revision: item.content_revision.clone(),
                item_kind: u8::try_from(item.item_kind)
                    .map_err(|_| PersistenceError::CorruptStoredRecall)?,
                source,
                pre_item_version: item.item_version,
                post_item_version: item
                    .item_version
                    .checked_add(1)
                    .ok_or(PersistenceError::CorruptStoredRecall)?,
                ledger_event_id: derived_id(context, &[&request.terminal_id, &item_uid]),
            })
        })
        .collect()
}

fn build_recall_materials(
    request: &ProductionRecallCommitRequestV1,
    planned: &[sim_core::RecallMaterialDestruction],
    materials: &BTreeMap<String, LockedRecallMaterial>,
) -> Result<Vec<StoredProductionRecallMaterialDestructionV1>, PersistenceError> {
    planned
        .iter()
        .enumerate()
        .map(|(index, destruction)| {
            let stored = materials
                .get(&destruction.material_id)
                .ok_or(PersistenceError::CorruptStoredRecall)?;
            if stored.quantity != destruction.destroyed_quantity
                || stored.version != destruction.pre_pouch_version
            {
                return Err(PersistenceError::CorruptStoredRecall);
            }
            Ok(StoredProductionRecallMaterialDestructionV1 {
                ordinal: u8::try_from(index).map_err(|_| PersistenceError::CorruptStoredRecall)?,
                material_id: destruction.material_id.clone(),
                destroyed_quantity: u8::try_from(destruction.destroyed_quantity)
                    .map_err(|_| PersistenceError::CorruptStoredRecall)?,
                pre_pouch_version: destruction.pre_pouch_version,
                post_pouch_version: destruction.post_pouch_version,
                destruction_event_id: derived_id(
                    MATERIAL_DESTRUCTION_ID_CONTEXT,
                    &[&request.terminal_id, destruction.material_id.as_bytes()],
                ),
            })
        })
        .collect()
}

#[allow(
    clippy::too_many_lines,
    reason = "the normalized terminal root intentionally binds every preserved and advanced axis"
)]
async fn insert_recall_terminal_root(
    connection: &mut PgConnection,
    request: &ProductionRecallCommitRequestV1,
    authority: &LockedRecallAuthority,
    result: &StoredProductionRecallResultV1,
    result_hash: [u8; HASH_BYTES],
    result_payload: &[u8],
) -> Result<(), PersistenceError> {
    let sequence = request.request_sequence.map(i64::from);
    let explicit_client_tick = request.explicit_client_tick.map(i64_value).transpose()?;
    let inserted = sqlx::query(
        "INSERT INTO character_recall_terminal_results_v1
         (namespace_id,account_id,character_id,mutation_id,terminal_id,
          contract_version,terminal_kind,trigger_kind,explicit_request_sequence,
          canonical_request_hash,canonical_plan_hash,result_hash,result_payload,
          instance_lineage_id,entry_restore_point_id,source_content_id,
          destination_content_id,records_blake3,assets_blake3,localization_blake3,
          issued_at,trigger_started_tick,completion_tick,
          pre_character_security_state,post_character_security_state,
          pre_account_version,post_account_version,pre_character_version,
          post_character_version,pre_world_version,post_world_version,
          pre_inventory_version,post_inventory_version,pre_life_metrics_version,
          post_life_metrics_version,pre_lifetime_ticks,post_lifetime_ticks,
          pre_permadeath_combat_ticks,post_permadeath_combat_ticks,
          preserved_progression_version,preserved_oath_bargain_version,
          preserved_ash_wallet_version,stabilized_item_count,destroyed_item_count,
          destroyed_material_stack_count,explicit_client_tick,result_code)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,
                 $18,$19,$20,to_timestamp($21::double precision/1000.0),$22,$23,
                 0,0,$24,$25,$26,$27,$28,$29,$30,$31,$32,$33,$34,$35,$36,$37,
                 $38,$39,$40,$41,$42,$43,$44,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(request.terminal_id.as_slice())
    .bind(i16::try_from(request.contract_version).map_err(|_| corrupt())?)
    .bind(i16::from(request.trigger.terminal_kind()))
    .bind(i16::from(recall_trigger_code(request.trigger)))
    .bind(sequence)
    .bind(result.canonical_request_hash.as_slice())
    .bind(result.canonical_plan_hash.as_slice())
    .bind(result_hash.as_slice())
    .bind(result_payload)
    .bind(request.instance_lineage_id.as_slice())
    .bind(request.entry_restore_point_id.as_slice())
    .bind(&authority.source_content_id)
    .bind(crate::PRODUCTION_RECALL_HALL_ID)
    .bind(&authority.root.records_blake3)
    .bind(&authority.root.assets_blake3)
    .bind(&authority.root.localization_blake3)
    .bind(i64_value(request.issued_at_unix_ms)?)
    .bind(i64_value(request.trigger_started_tick)?)
    .bind(i64_value(request.completion_tick)?)
    .bind(i64_value(result.versions.account.pre)?)
    .bind(i64_value(result.versions.account.post)?)
    .bind(i64_value(result.versions.character.pre)?)
    .bind(i64_value(result.versions.character.post)?)
    .bind(i64_value(result.versions.world.pre)?)
    .bind(i64_value(result.versions.world.post)?)
    .bind(i64_value(result.versions.inventory.pre)?)
    .bind(i64_value(result.versions.inventory.post)?)
    .bind(i64_value(result.versions.life_metrics.pre)?)
    .bind(i64_value(result.versions.life_metrics.post)?)
    .bind(i64_value(result.pre_lifetime_ticks)?)
    .bind(i64_value(result.post_lifetime_ticks)?)
    .bind(i64_value(result.pre_permadeath_combat_ticks)?)
    .bind(i64_value(result.post_permadeath_combat_ticks)?)
    .bind(i64_value(result.versions.progression.pre)?)
    .bind(i64_value(result.versions.oath_bargain.pre)?)
    .bind(i64_value(result.versions.ash_wallet.pre)?)
    .bind(i16::try_from(result.stabilized_items.len()).map_err(|_| corrupt())?)
    .bind(i32::try_from(result.destroyed_items.len()).map_err(|_| corrupt())?)
    .bind(i16::try_from(result.destroyed_materials.len()).map_err(|_| corrupt())?)
    .bind(explicit_client_tick)
    .execute(connection)
    .await?
    .rows_affected();
    expect_one(inserted)
}

async fn apply_recall_items(
    connection: &mut PgConnection,
    request: &ProductionRecallCommitRequestV1,
    result: &StoredProductionRecallResultV1,
) -> Result<(), PersistenceError> {
    for item in &result.stabilized_items {
        apply_recall_stabilization(connection, request, item).await?;
    }
    for item in &result.destroyed_items {
        apply_recall_destruction(connection, request, item).await?;
    }
    Ok(())
}

async fn apply_recall_stabilization(
    connection: &mut PgConnection,
    request: &ProductionRecallCommitRequestV1,
    item: &StoredProductionRecallItemV1,
) -> Result<(), PersistenceError> {
    let source_slot = item
        .source
        .slot_index()
        .ok_or(PersistenceError::CorruptStoredRecall)?;
    let updated = sqlx::query(
        "UPDATE item_instances
         SET item_version=$1,security_state=0,terminal_recall_id=$2,
             recalled_at=transaction_timestamp(),updated_at=transaction_timestamp()
         WHERE namespace_id=$3 AND account_id=$4 AND character_id=$5 AND item_uid=$6
           AND item_version=$7 AND security_state=1
           AND location_kind=$8 AND slot_index=$9",
    )
    .bind(i64_value(item.post_item_version)?)
    .bind(request.terminal_id.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(item.item_uid.as_slice())
    .bind(i64_value(item.pre_item_version)?)
    .bind(item.source.durable_kind())
    .bind(i16_value(source_slot)?)
    .execute(&mut *connection)
    .await?
    .rows_affected();
    if updated != 1 {
        return Err(PersistenceError::ProductionRecallBindingMismatch);
    }
    insert_recall_item_ledger(connection, request, item, RecallProjectionKind::Stabilized).await?;
    sqlx::query(
        "INSERT INTO recall_terminal_item_stabilizations_v1
         (namespace_id,account_id,character_id,terminal_id,mutation_id,
          stabilization_ordinal,item_uid,template_id,content_revision,item_kind,
          source_kind,source_slot_index,pre_item_version,post_item_version,
          pre_security_state,post_security_state,destination_kind,
          ledger_event_id,ledger_event_kind,ledger_source_kind)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,1,0,$11,$15,1,6)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.terminal_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(i16::try_from(item.ordinal).map_err(|_| corrupt())?)
    .bind(item.item_uid.as_slice())
    .bind(&item.template_id)
    .bind(&item.content_revision)
    .bind(i16::from(item.item_kind))
    .bind(item.source.durable_kind())
    .bind(i16_value(source_slot)?)
    .bind(i64_value(item.pre_item_version)?)
    .bind(i64_value(item.post_item_version)?)
    .bind(item.ledger_event_id.as_slice())
    .execute(connection)
    .await?;
    Ok(())
}

async fn apply_recall_destruction(
    connection: &mut PgConnection,
    request: &ProductionRecallCommitRequestV1,
    item: &StoredProductionRecallItemV1,
) -> Result<(), PersistenceError> {
    let source_slot = item.source.slot_index().map(i16_value).transpose()?;
    let source_instance = item.source.instance_id().map(|value| value.to_vec());
    let source_pickup = item.source.pickup_id().map(|value| value.to_vec());
    let source_expiry = item.source.expires_at_tick().map(i64_value).transpose()?;
    let updated = sqlx::query(
        "UPDATE item_instances
         SET item_version=$1,security_state=3,location_kind=4,slot_index=NULL,
             instance_id=NULL,pickup_id=NULL,expires_at_tick=NULL,
             destruction_reason='recall',terminal_recall_id=$2,
             recalled_at=transaction_timestamp(),updated_at=transaction_timestamp()
         WHERE namespace_id=$3 AND account_id=$4 AND character_id=$5 AND item_uid=$6
           AND item_version=$7 AND security_state=2 AND location_kind=$8
           AND slot_index IS NOT DISTINCT FROM $9
           AND instance_id IS NOT DISTINCT FROM $10
           AND pickup_id IS NOT DISTINCT FROM $11
           AND expires_at_tick IS NOT DISTINCT FROM $12",
    )
    .bind(i64_value(item.post_item_version)?)
    .bind(request.terminal_id.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(item.item_uid.as_slice())
    .bind(i64_value(item.pre_item_version)?)
    .bind(item.source.durable_kind())
    .bind(source_slot)
    .bind(source_instance)
    .bind(source_pickup)
    .bind(source_expiry)
    .execute(&mut *connection)
    .await?
    .rows_affected();
    if updated != 1 {
        return Err(PersistenceError::ProductionRecallBindingMismatch);
    }
    insert_recall_item_ledger(connection, request, item, RecallProjectionKind::Destroyed).await?;
    sqlx::query(
        "INSERT INTO recall_terminal_item_destructions_v1
         (namespace_id,account_id,character_id,terminal_id,mutation_id,
          destruction_ordinal,item_uid,template_id,content_revision,item_kind,
          source_kind,source_slot_index,source_instance_id,source_pickup_id,
          source_expires_at_tick,pre_item_version,post_item_version,
          pre_security_state,post_security_state,destination_kind,
          destruction_reason,ledger_event_id,ledger_event_kind,ledger_source_kind)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,
                 $16,$17,2,3,4,'recall',$18,2,6)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.terminal_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(i32::from(item.ordinal))
    .bind(item.item_uid.as_slice())
    .bind(&item.template_id)
    .bind(&item.content_revision)
    .bind(i16::from(item.item_kind))
    .bind(item.source.durable_kind())
    .bind(source_slot)
    .bind(item.source.instance_id().map(|value| value.to_vec()))
    .bind(item.source.pickup_id().map(|value| value.to_vec()))
    .bind(source_expiry)
    .bind(i64_value(item.pre_item_version)?)
    .bind(i64_value(item.post_item_version)?)
    .bind(item.ledger_event_id.as_slice())
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_recall_item_ledger(
    connection: &mut PgConnection,
    request: &ProductionRecallCommitRequestV1,
    item: &StoredProductionRecallItemV1,
    kind: RecallProjectionKind,
) -> Result<(), PersistenceError> {
    let (event_kind, post_security, post_location, reason) = match kind {
        RecallProjectionKind::Stabilized => {
            (1_i16, SECURITY_NORMAL, item.source.durable_kind(), None)
        }
        RecallProjectionKind::Destroyed => (2_i16, SECURITY_DESTROYED, 4_i16, Some("recall")),
    };
    sqlx::query(
        "INSERT INTO item_ledger_events
         (namespace_id,ledger_event_id,item_uid,account_id,character_id,mutation_id,
          event_kind,source_kind,pre_item_version,post_item_version,
          pre_security_state,post_security_state,pre_location_kind,
          post_location_kind,reason,terminal_recall_id)
         VALUES ($1,$2,$3,$4,$5,$6,$7,6,$8,$9,$10,$11,$12,$13,$14,$15)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(item.ledger_event_id.as_slice())
    .bind(item.item_uid.as_slice())
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(event_kind)
    .bind(i64_value(item.pre_item_version)?)
    .bind(i64_value(item.post_item_version)?)
    .bind(source_security(item.source))
    .bind(post_security)
    .bind(item.source.durable_kind())
    .bind(post_location)
    .bind(reason)
    .bind(request.terminal_id.as_slice())
    .execute(connection)
    .await?;
    Ok(())
}

async fn apply_recall_materials(
    connection: &mut PgConnection,
    request: &ProductionRecallCommitRequestV1,
    result: &StoredProductionRecallResultV1,
    materials: &BTreeMap<String, LockedRecallMaterial>,
) -> Result<(), PersistenceError> {
    for destruction in &result.destroyed_materials {
        let material = materials
            .get(&destruction.material_id)
            .ok_or(PersistenceError::CorruptStoredRecall)?;
        if material.quantity != u16::from(destruction.destroyed_quantity)
            || material.version != destruction.pre_pouch_version
        {
            return Err(PersistenceError::CorruptStoredRecall);
        }
        let updated = sqlx::query(
            "UPDATE character_run_material_stacks
             SET quantity=0,material_version=$1,security_state=$2,
                 terminal_reason='recall',terminal_restore_point_id=NULL,
                 terminal_death_id=NULL,terminal_extraction_id=NULL,extracted_at=NULL,
                 terminal_recall_id=$3,recalled_at=transaction_timestamp(),
                 updated_at=transaction_timestamp()
             WHERE namespace_id=$4 AND account_id=$5 AND character_id=$6
               AND material_id=$7 AND quantity=$8 AND material_version=$9
               AND security_state=2",
        )
        .bind(i64_value(destruction.post_pouch_version)?)
        .bind(MATERIAL_DESTROYED)
        .bind(request.terminal_id.as_slice())
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(request.character_id.as_slice())
        .bind(&destruction.material_id)
        .bind(i32::from(destruction.destroyed_quantity))
        .bind(i64_value(destruction.pre_pouch_version)?)
        .execute(&mut *connection)
        .await?
        .rows_affected();
        if updated != 1 {
            return Err(PersistenceError::ProductionRecallBindingMismatch);
        }
        sqlx::query(
            "INSERT INTO recall_terminal_material_destructions_v1
             (namespace_id,account_id,character_id,terminal_id,mutation_id,
              destruction_ordinal,material_id,destroyed_quantity,
              pre_pouch_version,post_pouch_version,destruction_event_id)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(request.character_id.as_slice())
        .bind(request.terminal_id.as_slice())
        .bind(request.mutation_id.as_slice())
        .bind(i16::from(destruction.ordinal))
        .bind(&destruction.material_id)
        .bind(i32::from(destruction.destroyed_quantity))
        .bind(i64_value(destruction.pre_pouch_version)?)
        .bind(i64_value(destruction.post_pouch_version)?)
        .bind(destruction.destruction_event_id.as_slice())
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

async fn apply_recall_aggregate_heads(
    connection: &mut PgConnection,
    request: &ProductionRecallCommitRequestV1,
    result: &StoredProductionRecallResultV1,
) -> Result<(), PersistenceError> {
    expect_one(
        sqlx::query(
            "UPDATE characters
             SET character_state_version=$1,updated_at=transaction_timestamp()
             WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4
               AND life_state=0 AND security_state=0 AND character_state_version=$5",
        )
        .bind(i64_value(result.versions.character.post)?)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(request.character_id.as_slice())
        .bind(i64_value(result.versions.character.pre)?)
        .execute(&mut *connection)
        .await?
        .rows_affected(),
    )?;
    expect_one(
        sqlx::query(
            "UPDATE character_world_locations
             SET character_version=$1,location_kind=$2,location_content_id=$3,
                 safe_arrival_kind=0,safe_spawn_id=NULL,instance_lineage_id=NULL,
                 entry_restore_point_id=NULL,updated_at=transaction_timestamp()
             WHERE namespace_id=$4 AND account_id=$5 AND character_id=$6
               AND character_version=$7 AND location_kind=2
               AND instance_lineage_id=$8 AND entry_restore_point_id=$9",
        )
        .bind(i64_value(result.versions.world.post)?)
        .bind(LOCATION_SAFE)
        .bind(crate::PRODUCTION_RECALL_HALL_ID)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(request.character_id.as_slice())
        .bind(i64_value(result.versions.world.pre)?)
        .bind(request.instance_lineage_id.as_slice())
        .bind(request.entry_restore_point_id.as_slice())
        .execute(&mut *connection)
        .await?
        .rows_affected(),
    )?;
    expect_one(
        sqlx::query(
            "UPDATE character_inventories
             SET inventory_version=$1,updated_at=transaction_timestamp()
             WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4
               AND inventory_version=$5",
        )
        .bind(i64_value(result.versions.inventory.post)?)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(request.character_id.as_slice())
        .bind(i64_value(result.versions.inventory.pre)?)
        .execute(&mut *connection)
        .await?
        .rows_affected(),
    )?;
    expect_one(
        sqlx::query(
            "UPDATE character_life_metrics
             SET lifetime_ticks=$1,permadeath_combat_ticks=$2,life_metrics_version=$3,
                 updated_at=transaction_timestamp()
             WHERE namespace_id=$4 AND account_id=$5 AND character_id=$6
               AND lifetime_ticks=$7 AND permadeath_combat_ticks=$8
               AND life_metrics_version=$9",
        )
        .bind(i64_value(result.post_lifetime_ticks)?)
        .bind(i64_value(result.post_permadeath_combat_ticks)?)
        .bind(i64_value(result.versions.life_metrics.post)?)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(request.character_id.as_slice())
        .bind(i64_value(result.pre_lifetime_ticks)?)
        .bind(i64_value(result.pre_permadeath_combat_ticks)?)
        .bind(i64_value(result.versions.life_metrics.pre)?)
        .execute(connection)
        .await?
        .rows_affected(),
    )?;
    Ok(())
}

async fn close_recall_danger_root(
    connection: &mut PgConnection,
    request: &ProductionRecallCommitRequestV1,
) -> Result<(), PersistenceError> {
    expect_one(
        sqlx::query(
            "UPDATE character_entry_restore_points
             SET restore_state=$1,consumed_at=transaction_timestamp(),recall_terminal_id=$2
             WHERE namespace_id=$3 AND account_id=$4 AND character_id=$5
               AND restore_point_id=$6 AND lineage_id=$7 AND restore_state=0",
        )
        .bind(RESTORE_RECALL_COMMITTED)
        .bind(request.terminal_id.as_slice())
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(request.character_id.as_slice())
        .bind(request.entry_restore_point_id.as_slice())
        .bind(request.instance_lineage_id.as_slice())
        .execute(&mut *connection)
        .await?
        .rows_affected(),
    )?;
    expect_one(
        sqlx::query(
            "UPDATE character_instance_lineages
             SET lineage_state=$1,closed_at=transaction_timestamp()
             WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4
               AND lineage_id=$5 AND lineage_state IN (0,1)",
        )
        .bind(LINEAGE_CLOSED_SUCCESS)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(request.character_id.as_slice())
        .bind(request.instance_lineage_id.as_slice())
        .execute(connection)
        .await?
        .rows_affected(),
    )?;
    Ok(())
}

async fn insert_recall_audit_and_outbox(
    connection: &mut PgConnection,
    request: &ProductionRecallCommitRequestV1,
    result_hash: [u8; HASH_BYTES],
    result_payload: &[u8],
) -> Result<(), PersistenceError> {
    let event_type = recall_event_type(request.trigger);
    let audit_id = derived_id(
        ACCEPTED_AUDIT_ID_CONTEXT,
        &[&request.terminal_id, &result_hash],
    );
    let event_id = derived_id(OUTBOX_ID_CONTEXT, &[&request.terminal_id, &result_hash]);
    sqlx::query(
        "INSERT INTO recall_terminal_audit_events_v1
         (namespace_id,account_id,character_id,terminal_id,audit_event_id,
          event_type,event_digest)
         VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.terminal_id.as_slice())
    .bind(audit_id.as_slice())
    .bind(event_type)
    .bind(result_hash.as_slice())
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "INSERT INTO recall_terminal_outbox_events_v1
         (namespace_id,account_id,character_id,terminal_id,event_id,event_type,event_payload)
         VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.terminal_id.as_slice())
    .bind(event_id.as_slice())
    .bind(event_type)
    .bind(result_payload)
    .execute(connection)
    .await?;
    Ok(())
}

async fn transaction_timestamp_ms(connection: &mut PgConnection) -> Result<u64, PersistenceError> {
    let value: i64 = sqlx::query_scalar(
        "SELECT floor(extract(epoch FROM transaction_timestamp()) * 1000)::bigint",
    )
    .fetch_one(connection)
    .await?;
    positive(value)
}

async fn force_deferred_constraints(connection: &mut PgConnection) -> Result<(), PersistenceError> {
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(connection)
        .await?;
    Ok(())
}

fn stored_recall_location(location: RecallItemLocation) -> StoredRecallLocationV1 {
    match location {
        RecallItemLocation::Equipped(index) => StoredRecallLocationV1::Equipped(index),
        RecallItemLocation::Belt(index) => StoredRecallLocationV1::Belt(index),
        RecallItemLocation::RunBackpack(index) => StoredRecallLocationV1::RunBackpack(index),
        RecallItemLocation::PersonalGround {
            instance_id,
            pickup_id,
            expires_at_tick,
        } => StoredRecallLocationV1::PersonalGround {
            instance_id,
            pickup_id,
            expires_at_tick,
        },
    }
}

fn recall_source_matches(item: &LockedRecallItem, source: StoredRecallLocationV1) -> bool {
    item.location_kind == source.durable_kind()
        && item.slot_index == source.slot_index()
        && item.instance_id == source.instance_id()
        && item.pickup_id == source.pickup_id()
        && item.expires_at_tick == source.expires_at_tick()
}

const fn source_security(location: StoredRecallLocationV1) -> i16 {
    match location {
        StoredRecallLocationV1::Equipped(_) | StoredRecallLocationV1::Belt(_) => {
            SECURITY_AT_RISK_EQUIPPED
        }
        StoredRecallLocationV1::RunBackpack(_) | StoredRecallLocationV1::PersonalGround { .. } => {
            SECURITY_AT_RISK_PENDING
        }
    }
}

const fn recall_trigger_code(trigger: crate::ProductionRecallTriggerV1) -> u8 {
    match trigger {
        crate::ProductionRecallTriggerV1::Explicit => 0,
        crate::ProductionRecallTriggerV1::LinkLost => 1,
    }
}

const fn recall_event_type(trigger: crate::ProductionRecallTriggerV1) -> &'static str {
    match trigger {
        crate::ProductionRecallTriggerV1::Explicit => "emergency_recall_committed",
        crate::ProductionRecallTriggerV1::LinkLost => "disconnect_recovery_committed",
    }
}

fn unchanged(version: u64) -> ProductionRecallVersionAdvanceV1 {
    ProductionRecallVersionAdvanceV1 {
        pre: version,
        post: version,
    }
}

fn advance(pre: u64) -> Result<ProductionRecallVersionAdvanceV1, PersistenceError> {
    Ok(ProductionRecallVersionAdvanceV1 {
        pre,
        post: pre.checked_add(1).ok_or_else(corrupt)?,
    })
}

fn derived_id(context: &str, parts: &[&[u8]]) -> [u8; ID_BYTES] {
    let mut hasher = blake3::Hasher::new_derive_key(context);
    for part in parts {
        hasher.update(&(part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    let mut value = [0_u8; ID_BYTES];
    value.copy_from_slice(&hasher.finalize().as_bytes()[..ID_BYTES]);
    if value == [0; ID_BYTES] {
        value[ID_BYTES - 1] = 1;
    }
    value
}

fn recall_terminal_advisory_key(axis: u8, identity: [u8; ID_BYTES]) -> i64 {
    let mut hasher =
        blake3::Hasher::new_derive_key("gravebound.production-recall-advisory-lock.v1");
    hasher.update(&[axis]);
    hasher.update(&identity);
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&hasher.finalize().as_bytes()[..8]);
    i64::from_be_bytes(bytes)
}

fn exact_id(value: Vec<u8>) -> Result<[u8; ID_BYTES], PersistenceError> {
    value.try_into().map_err(|_| corrupt())
}

fn optional_id(value: Option<Vec<u8>>) -> Result<Option<[u8; ID_BYTES]>, PersistenceError> {
    value.map(exact_id).transpose()
}

fn exact_hash(value: Vec<u8>) -> Result<[u8; HASH_BYTES], PersistenceError> {
    value.try_into().map_err(|_| corrupt())
}

fn positive(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(corrupt)
}

fn nonnegative(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value).map_err(|_| corrupt())
}

fn optional_positive(value: Option<i64>) -> Result<Option<u64>, PersistenceError> {
    value.map(positive).transpose()
}

fn optional_u16(value: Option<i16>) -> Result<Option<u16>, PersistenceError> {
    value
        .map(|value| u16::try_from(value).map_err(|_| corrupt()))
        .transpose()
}

fn u16_from_i32(value: i32) -> Result<u16, PersistenceError> {
    u16::try_from(value).map_err(|_| corrupt())
}

fn i16_value(value: u16) -> Result<i16, PersistenceError> {
    i16::try_from(value).map_err(|_| corrupt())
}

fn i64_value(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| corrupt())
}

fn expect_one(rows: u64) -> Result<(), PersistenceError> {
    if rows == 1 { Ok(()) } else { Err(corrupt()) }
}

const fn corrupt() -> PersistenceError {
    PersistenceError::CorruptStoredRecall
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_recall_ids_are_domain_separated_and_nonzero() {
        let parts = [&[1_u8; 16][..], &[2_u8; 32][..]];
        let stabilized = derived_id(STABILIZATION_LEDGER_ID_CONTEXT, &parts);
        let destroyed = derived_id(DESTRUCTION_LEDGER_ID_CONTEXT, &parts);
        let material = derived_id(MATERIAL_DESTRUCTION_ID_CONTEXT, &parts);
        assert_ne!(stabilized, [0; 16]);
        assert_ne!(stabilized, destroyed);
        assert_ne!(destroyed, material);
    }

    #[test]
    fn advisory_keys_bind_axis_and_identity() {
        let identity = [7; 16];
        assert_eq!(
            recall_terminal_advisory_key(0, identity),
            recall_terminal_advisory_key(0, identity)
        );
        assert_ne!(
            recall_terminal_advisory_key(0, identity),
            recall_terminal_advisory_key(1, identity)
        );
    }

    #[test]
    fn stored_locations_match_append_only_custody_discriminants() {
        assert_eq!(StoredRecallLocationV1::Equipped(0).durable_kind(), 0);
        assert_eq!(StoredRecallLocationV1::Belt(0).durable_kind(), 1);
        assert_eq!(StoredRecallLocationV1::RunBackpack(0).durable_kind(), 2);
        assert_eq!(
            StoredRecallLocationV1::PersonalGround {
                instance_id: [1; 16],
                pickup_id: [2; 16],
                expires_at_tick: 3,
            }
            .durable_kind(),
            3
        );
    }

    #[test]
    fn trigger_codes_and_event_types_are_disjoint() {
        assert_eq!(
            recall_trigger_code(crate::ProductionRecallTriggerV1::Explicit),
            0
        );
        assert_eq!(
            recall_trigger_code(crate::ProductionRecallTriggerV1::LinkLost),
            1
        );
        assert_ne!(
            recall_event_type(crate::ProductionRecallTriggerV1::Explicit),
            recall_event_type(crate::ProductionRecallTriggerV1::LinkLost)
        );
    }
}
