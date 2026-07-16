//! Replay-first serializable writer for the GB-M03 successful-extraction terminal.

use std::collections::BTreeMap;

use sim_core::{
    CHARACTER_SAFE_CAPACITY, DurableStorageSlot, EQUIPMENT_SLOT_COUNT, ExtractionInventorySnapshot,
    ItemUid, RUN_BACKPACK_CAPACITY, TERMINAL_BELT_CAPACITY, TERMINAL_OVERFLOW_CAPACITY,
    TERMINAL_RESOLUTION_HOLD_CAPACITY, TerminalInventoryLocation, TerminalMaterialSnapshot,
    VAULT_CAPACITY, plan_successful_extraction,
};
use sqlx::{PgConnection, Row};

use crate::{
    PersistenceError, PostgresPersistence, PreparedProductionExtractionV1,
    ProductionExtractionCommitRequestV1, ProductionExtractionTransactionV1,
    ProductionExtractionVersionAdvanceV1, ProductionExtractionVersionsV1,
    StoredExtractionLocationV1, StoredProductionExtractionMaterialCreditV1,
    StoredProductionExtractionPlacementV1, StoredProductionExtractionResultV1,
    WIPEABLE_CORE_NAMESPACE, canonical_production_extraction_plan_hash_v1,
    is_retryable_transaction_failure, stage_danger_checkpoint_cleanup,
};

const MAX_TRANSACTION_ATTEMPTS: u8 = 3;
const ID_BYTES: usize = 16;
const HASH_BYTES: usize = 32;
const RESTORE_ACTIVE: i16 = 0;
const RESTORE_EXTRACTION_COMMITTED: i16 = 1;
const LINEAGE_CLOSED_SUCCESS: i16 = 2;
const LOCATION_SAFE: i16 = 1;
const LOCATION_DANGER: i16 = 2;
const SECURITY_NORMAL: i16 = 0;
const SECURITY_AT_RISK_EQUIPPED: i16 = 1;
const SECURITY_AT_RISK_PENDING: i16 = 2;
const SECURITY_SAFE: i16 = 0;
const MATERIAL_EXTRACTED: i16 = 4;

const ITEM_LEDGER_ID_CONTEXT: &str = "gravebound.production-extraction-item-ledger.v1";
const MATERIAL_LEDGER_ID_CONTEXT: &str = "gravebound.production-extraction-material-ledger.v1";
const ACCEPTED_AUDIT_ID_CONTEXT: &str = "gravebound.production-extraction-audit.v1";
const CONFLICT_AUDIT_ID_CONTEXT: &str = "gravebound.production-extraction-conflict-audit.v1";
const OUTBOX_ID_CONTEXT: &str = "gravebound.production-extraction-outbox.v1";

#[derive(Debug, Clone)]
struct LockedRoot {
    records_blake3: String,
    assets_blake3: String,
    localization_blake3: String,
    restore_state: i16,
}

#[derive(Debug, Clone)]
struct LockedItem {
    item_uid: [u8; ID_BYTES],
    template_id: String,
    content_revision: String,
    item_kind: i16,
    item_version: u64,
    security_state: i16,
    location_kind: i16,
    slot_index: u16,
}

#[derive(Debug, Clone)]
struct LockedWallet {
    quantity: u32,
    wallet_cap: u32,
    version: u64,
}

#[derive(Debug, Clone)]
struct LockedPouch {
    material_id: String,
    quantity: u16,
    version: u64,
}

#[derive(Debug)]
struct LockedAuthority {
    account_version: u64,
    character_version: u64,
    world_version: u64,
    inventory_version: u64,
    life_metrics_version: u64,
    root: LockedRoot,
}

#[derive(Debug)]
struct LockedProductionExtractionPlan {
    authority: LockedAuthority,
    wallets: BTreeMap<String, LockedWallet>,
    pouches: BTreeMap<String, LockedPouch>,
    committed_at_unix_ms: u64,
    placements: Vec<StoredProductionExtractionPlacementV1>,
    material_credits: Vec<StoredProductionExtractionMaterialCreditV1>,
    post_account_version: u64,
    post_inventory_version: u64,
    storage_resolution_required: bool,
}

impl LockedProductionExtractionPlan {
    fn canonical_plan_hash(&self) -> Result<[u8; HASH_BYTES], PersistenceError> {
        canonical_production_extraction_plan_hash_v1(&self.placements, &self.material_credits)
    }

    fn stored_result(
        &self,
        request: &ProductionExtractionCommitRequestV1,
        canonical_request_hash: [u8; HASH_BYTES],
        canonical_plan_hash: [u8; HASH_BYTES],
    ) -> Result<StoredProductionExtractionResultV1, PersistenceError> {
        let result = StoredProductionExtractionResultV1 {
            contract_version: request.contract_version,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            account_id: request.account_id,
            character_id: request.character_id,
            mutation_id: request.mutation_id,
            terminal_id: request.terminal_id,
            extraction_request_id: request.extraction_request_id,
            extraction_receipt_id: request.extraction_receipt_id,
            canonical_request_hash,
            canonical_plan_hash,
            result_code: 1,
            issued_at_unix_ms: request.issued_at_unix_ms,
            observed_tick: request.observed_tick,
            committed_at_unix_ms: self.committed_at_unix_ms,
            destination_content_id: crate::PRODUCTION_EXTRACTION_HALL_ID.into(),
            versions: ProductionExtractionVersionsV1 {
                account: ProductionExtractionVersionAdvanceV1 {
                    pre: self.authority.account_version,
                    post: self.post_account_version,
                },
                character: advance(self.authority.character_version)?,
                world: advance(self.authority.world_version)?,
                inventory: ProductionExtractionVersionAdvanceV1 {
                    pre: self.authority.inventory_version,
                    post: self.post_inventory_version,
                },
                life_metrics: advance(self.authority.life_metrics_version)?,
            },
            placements: self.placements.clone(),
            material_credits: self.material_credits.clone(),
            storage_resolution_required: self.storage_resolution_required,
        };
        result.validate()?;
        Ok(result)
    }
}

impl PostgresPersistence {
    /// Plans one exact extraction from locked durable custody and rolls the transaction back.
    ///
    /// No gameplay row, audit, receipt, or outbox record is written. The returned hashes are the
    /// only material the shared terminal coordinator may use for its extraction candidate.
    pub async fn prepare_production_extraction_v1(
        &self,
        request: &ProductionExtractionCommitRequestV1,
    ) -> Result<PreparedProductionExtractionV1, PersistenceError> {
        request.validate()?;
        for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self.prepare_production_extraction_once_v1(request).await {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS
                        && is_retryable_transaction_failure(&error) => {}
                outcome => return outcome,
            }
        }
        Err(PersistenceError::ProductionExtractionTerminalSuperseded)
    }

    async fn prepare_production_extraction_once_v1(
        &self,
        request: &ProductionExtractionCommitRequestV1,
    ) -> Result<PreparedProductionExtractionV1, PersistenceError> {
        let request_hash = request.canonical_hash()?;
        let mut transaction = self.begin_transaction().await?;
        lock_terminal_identities(transaction.connection(), request).await?;
        lock_account(transaction.connection(), request.account_id).await?;
        if let Some(stored) = load_existing_terminal(
            transaction.connection(),
            request.account_id,
            request.mutation_id,
            request.extraction_request_id,
            request.terminal_id,
            request.extraction_receipt_id,
        )
        .await?
        {
            transaction.rollback().await?;
            if exact_request_replay(&stored, request, request_hash) {
                return PreparedProductionExtractionV1::new(
                    request.clone(),
                    request_hash,
                    stored.canonical_plan_hash,
                    true,
                );
            }
            if stored.canonical_request_hash == request_hash {
                return Err(PersistenceError::CorruptStoredExtraction);
            }
            return Err(PersistenceError::ExtractionIdempotencyConflict);
        }
        let plan = lock_and_plan_production_extraction(transaction.connection(), request).await?;
        let plan_hash = plan.canonical_plan_hash()?;
        transaction.rollback().await?;
        PreparedProductionExtractionV1::new(request.clone(), request_hash, plan_hash, false)
    }

    pub async fn commit_production_extraction_v1(
        &self,
        request: &ProductionExtractionCommitRequestV1,
        expected_plan_hash: [u8; HASH_BYTES],
    ) -> Result<ProductionExtractionTransactionV1, PersistenceError> {
        request.validate()?;
        if expected_plan_hash == [0; HASH_BYTES] {
            return Err(PersistenceError::ProductionExtractionPlanChanged);
        }
        for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self
                .commit_production_extraction_once_v1(request, expected_plan_hash)
                .await
            {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS
                        && is_retryable_transaction_failure(&error) => {}
                outcome => return outcome,
            }
        }
        Err(PersistenceError::ProductionExtractionTerminalSuperseded)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the complete extraction write is deliberately one auditable serializable state machine"
    )]
    async fn commit_production_extraction_once_v1(
        &self,
        request: &ProductionExtractionCommitRequestV1,
        expected_plan_hash: [u8; HASH_BYTES],
    ) -> Result<ProductionExtractionTransactionV1, PersistenceError> {
        let request_hash = request.canonical_hash()?;
        let mut transaction = self.begin_transaction().await?;
        lock_terminal_identities(transaction.connection(), request).await?;
        lock_account(transaction.connection(), request.account_id).await?;

        if let Some(stored) = load_existing_terminal(
            transaction.connection(),
            request.account_id,
            request.mutation_id,
            request.extraction_request_id,
            request.terminal_id,
            request.extraction_receipt_id,
        )
        .await?
        {
            if exact_request_replay(&stored, request, request_hash) {
                if stored.canonical_plan_hash != expected_plan_hash {
                    transaction.rollback().await?;
                    return Err(PersistenceError::ProductionExtractionPlanChanged);
                }
                transaction.rollback().await?;
                return Ok(ProductionExtractionTransactionV1::Replayed(stored));
            }
            if stored.canonical_request_hash == request_hash {
                return Err(PersistenceError::CorruptStoredExtraction);
            }
            insert_conflict_audit(transaction.connection(), &stored, request, request_hash).await?;
            transaction.commit().await?;
            return Ok(ProductionExtractionTransactionV1::Conflict {
                extraction_request_id: stored.extraction_request_id,
                terminal_id: stored.terminal_id,
            });
        }

        let plan = lock_and_plan_production_extraction(transaction.connection(), request).await?;
        let canonical_plan_hash = plan.canonical_plan_hash()?;
        if canonical_plan_hash != expected_plan_hash {
            transaction.rollback().await?;
            return Err(PersistenceError::ProductionExtractionPlanChanged);
        }
        let result = plan.stored_result(request, request_hash, canonical_plan_hash)?;
        let result_payload = result.encode()?;
        let result_hash = result.digest()?;

        insert_terminal_root(
            transaction.connection(),
            request,
            &plan.authority,
            &result,
            result_hash,
            &result_payload,
        )
        .await?;
        apply_item_placements(transaction.connection(), request, &result).await?;
        apply_material_credits(
            transaction.connection(),
            request,
            &result,
            &plan.wallets,
            &plan.pouches,
        )
        .await?;
        apply_aggregate_heads(transaction.connection(), request, &result).await?;
        close_danger_root(transaction.connection(), request).await?;
        project_production_seam(transaction.connection(), request, &result, result_hash).await?;
        stage_danger_checkpoint_cleanup(
            &mut transaction,
            request.account_id,
            request.character_id,
            request.instance_lineage_id,
        )
        .await?;
        insert_terminal_audit_and_outbox(
            transaction.connection(),
            request,
            result_hash,
            &result_payload,
        )
        .await?;
        force_deferred_constraints(transaction.connection()).await?;
        transaction.commit().await?;
        Ok(ProductionExtractionTransactionV1::Fresh(result))
    }
}

fn exact_request_replay(
    stored: &StoredProductionExtractionResultV1,
    request: &ProductionExtractionCommitRequestV1,
    request_hash: [u8; HASH_BYTES],
) -> bool {
    stored.canonical_request_hash == request_hash
        && stored.account_id == request.account_id
        && stored.character_id == request.character_id
        && stored.mutation_id == request.mutation_id
        && stored.terminal_id == request.terminal_id
        && stored.extraction_request_id == request.extraction_request_id
        && stored.extraction_receipt_id == request.extraction_receipt_id
}

async fn lock_and_plan_production_extraction(
    connection: &mut PgConnection,
    request: &ProductionExtractionCommitRequestV1,
) -> Result<LockedProductionExtractionPlan, PersistenceError> {
    let authority = lock_and_validate_authority(connection, request).await?;
    reject_unresolved_reward_mutation(connection, request).await?;
    let (items, mut inventory_snapshot) =
        load_inventory_snapshot(connection, request, &authority).await?;
    let (wallets, pouches, material_snapshots) =
        load_material_snapshot(connection, request).await?;
    let committed_at_unix_micros = transaction_time_unix_micros(connection).await?;
    inventory_snapshot.committed_at_unix_micros = committed_at_unix_micros;
    inventory_snapshot.materials = material_snapshots;
    let plan = plan_successful_extraction(&inventory_snapshot)
        .map_err(|_| PersistenceError::ProductionExtractionPlanningFailed)?;
    let placements = build_placements(&items, request, &plan.placements)?;
    let material_credits = build_material_credits(request, &plan.material_credits)?;
    Ok(LockedProductionExtractionPlan {
        authority,
        wallets,
        pouches,
        committed_at_unix_ms: committed_at_unix_micros / 1_000,
        placements,
        material_credits,
        post_account_version: plan.post_account_version,
        post_inventory_version: plan.post_inventory_version,
        storage_resolution_required: plan.resolution_required(),
    })
}

async fn lock_terminal_identities(
    connection: &mut PgConnection,
    request: &ProductionExtractionCommitRequestV1,
) -> Result<(), PersistenceError> {
    let mut lock_keys = [
        terminal_advisory_key(0, request.extraction_request_id),
        terminal_advisory_key(1, request.terminal_id),
        terminal_advisory_key(2, request.extraction_receipt_id),
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

async fn lock_account(
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
        return Err(PersistenceError::ProductionExtractionOwnerNotFound);
    }
    Ok(())
}

async fn load_existing_terminal(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    mutation_id: [u8; ID_BYTES],
    extraction_request_id: [u8; ID_BYTES],
    terminal_id: [u8; ID_BYTES],
    extraction_receipt_id: [u8; ID_BYTES],
) -> Result<Option<StoredProductionExtractionResultV1>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT account_id,character_id,mutation_id,terminal_id,extraction_request_id,
                extraction_receipt_id,canonical_request_hash,result_hash,result_payload
         FROM character_extraction_terminal_results_v1
         WHERE namespace_id=$1
           AND ((account_id=$2 AND mutation_id=$3)
             OR extraction_request_id=$4
             OR terminal_id=$5
             OR extraction_receipt_id=$6)
         ORDER BY terminal_id FOR SHARE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(mutation_id.as_slice())
    .bind(extraction_request_id.as_slice())
    .bind(terminal_id.as_slice())
    .bind(extraction_receipt_id.as_slice())
    .fetch_all(connection)
    .await?;
    let [row] = rows.as_slice() else {
        return if rows.is_empty() {
            Ok(None)
        } else {
            Err(PersistenceError::CorruptStoredExtraction)
        };
    };
    let result = StoredProductionExtractionResultV1::decode(
        row.try_get::<Vec<u8>, _>("result_payload")?.as_slice(),
    )?;
    if result.account_id != exact_id(row.try_get("account_id")?)?
        || result.character_id != exact_id(row.try_get("character_id")?)?
        || result.mutation_id != exact_id(row.try_get("mutation_id")?)?
        || result.terminal_id != exact_id(row.try_get("terminal_id")?)?
        || result.extraction_request_id != exact_id(row.try_get("extraction_request_id")?)?
        || result.extraction_receipt_id != exact_id(row.try_get("extraction_receipt_id")?)?
        || result.canonical_request_hash != exact_hash(row.try_get("canonical_request_hash")?)?
        || result.digest()? != exact_hash(row.try_get("result_hash")?)?
    {
        return Err(PersistenceError::CorruptStoredExtraction);
    }
    Ok(Some(result))
}

async fn insert_conflict_audit(
    connection: &mut PgConnection,
    stored: &StoredProductionExtractionResultV1,
    attempted: &ProductionExtractionCommitRequestV1,
    attempted_hash: [u8; HASH_BYTES],
) -> Result<(), PersistenceError> {
    if attempted_hash == stored.canonical_request_hash {
        return Err(PersistenceError::CorruptStoredExtraction);
    }
    let audit_id = derived_id(
        CONFLICT_AUDIT_ID_CONTEXT,
        &[
            &stored.extraction_request_id,
            &attempted_hash,
            &attempted.mutation_id,
        ],
    );
    sqlx::query(
        "INSERT INTO extraction_terminal_conflict_audits_v1
         (namespace_id,extraction_request_id,conflict_audit_id,attempted_account_id,
          attempted_character_id,attempted_mutation_id,stored_request_hash,
          attempted_request_hash,attempted_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,to_timestamp($9::double precision/1000.0))
         ON CONFLICT (namespace_id,extraction_request_id,attempted_request_hash) DO NOTHING",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(stored.extraction_request_id.as_slice())
    .bind(audit_id.as_slice())
    .bind(attempted.account_id.as_slice())
    .bind(attempted.character_id.as_slice())
    .bind(attempted.mutation_id.as_slice())
    .bind(stored.canonical_request_hash.as_slice())
    .bind(attempted_hash.as_slice())
    .bind(i64_value(attempted.issued_at_unix_ms)?)
    .execute(connection)
    .await?;
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "lock order and every authority comparison are intentionally visible together"
)]
async fn lock_and_validate_authority(
    connection: &mut PgConnection,
    request: &ProductionExtractionCommitRequestV1,
) -> Result<LockedAuthority, PersistenceError> {
    let account = sqlx::query(
        "SELECT state_version,selected_character_id FROM accounts
         WHERE namespace_id=$1 AND account_id=$2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ProductionExtractionOwnerNotFound)?;
    let character = sqlx::query(
        "SELECT life_state,security_state,character_state_version FROM characters
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ProductionExtractionOwnerNotFound)?;
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
    .ok_or(PersistenceError::ProductionExtractionBindingMismatch)?;
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
    .ok_or(PersistenceError::ProductionExtractionBindingMismatch)?;
    let lineage = sqlx::query(
        "SELECT lineage_state,records_blake3,assets_blake3,localization_blake3
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
    .ok_or(PersistenceError::ProductionExtractionBindingMismatch)?;
    let seam = sqlx::query(
        "SELECT account_id,character_id,encounter_id,instance_lineage_id,
                entry_restore_point_id,exit_instance_id,exit_content_id,
                attempt_ordinal,party_slot,participant_entity_id,expected_character_version,
                records_blake3,assets_blake3,localization_blake3,extraction_state,authority_kind
         FROM character_extraction_results
         WHERE namespace_id=$1 AND extraction_request_id=$2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.extraction_request_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ProductionExtractionBindingMismatch)?;
    let party_slot: i16 = seam.try_get("party_slot")?;
    let owner = sqlx::query(
        "SELECT owner.account_id AS owner_account_id,
                owner.character_id AS owner_character_id,
                owner.participant_entity_id AS owner_participant_entity_id,
                owner.reward_result_hash AS owner_reward_result_hash,
                reward.account_id AS reward_account_id,
                reward.character_id AS reward_character_id,
                reward.source_instance_id AS reward_source_instance_id,
                reward.reward_table_id,reward.content_revision,reward.result_hash,
                reward.request_state
         FROM caldus_victory_exit_owners AS owner
         JOIN reward_requests AS reward
           ON reward.namespace_id=owner.namespace_id
          AND reward.reward_request_id=owner.reward_request_id
         WHERE owner.namespace_id=$1 AND owner.encounter_id=$2 AND owner.party_slot=$3
         FOR SHARE OF owner,reward",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.encounter_id.as_slice())
    .bind(party_slot)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ProductionExtractionBindingMismatch)?;
    let exit = sqlx::query(
        "SELECT instance_lineage_id,exit_instance_id,attempt_ordinal FROM caldus_victory_exits
         WHERE namespace_id=$1 AND encounter_id=$2 FOR SHARE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.encounter_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ProductionExtractionBindingMismatch)?;
    let life = sqlx::query(
        "SELECT life_metrics_version FROM character_life_metrics
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ProductionExtractionOwnerNotFound)?;
    let inventory = sqlx::query(
        "SELECT inventory_version FROM character_inventories
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ProductionExtractionOwnerNotFound)?;

    let account_version = positive(account.try_get("state_version")?)?;
    let character_version = positive(character.try_get("character_state_version")?)?;
    let world_version = positive(world.try_get("character_version")?)?;
    let inventory_version = positive(inventory.try_get("inventory_version")?)?;
    let life_metrics_version = positive(life.try_get("life_metrics_version")?)?;
    if account_version != request.expected_versions.account
        || character_version != request.expected_versions.character
        || world_version != request.expected_versions.world
        || inventory_version != request.expected_versions.inventory
        || life_metrics_version != request.expected_versions.life_metrics
    {
        return Err(PersistenceError::ProductionExtractionVersionMismatch {
            account: account_version,
            character: character_version,
            world: world_version,
            inventory: inventory_version,
            life_metrics: life_metrics_version,
        });
    }
    let selected = optional_id(account.try_get("selected_character_id")?)?;
    let root_lock = LockedRoot {
        records_blake3: root.try_get("records_blake3")?,
        assets_blake3: root.try_get("assets_blake3")?,
        localization_blake3: root.try_get("localization_blake3")?,
        restore_state: root.try_get("restore_state")?,
    };
    if root_lock.records_blake3 != request.content_revision.records_blake3
        || root_lock.assets_blake3 != request.content_revision.assets_blake3
        || root_lock.localization_blake3 != request.content_revision.localization_blake3
        || lineage.try_get::<String, _>("records_blake3")? != root_lock.records_blake3
        || lineage.try_get::<String, _>("assets_blake3")? != root_lock.assets_blake3
        || lineage.try_get::<String, _>("localization_blake3")? != root_lock.localization_blake3
        || seam.try_get::<String, _>("records_blake3")? != root_lock.records_blake3
        || seam.try_get::<String, _>("assets_blake3")? != root_lock.assets_blake3
        || seam.try_get::<String, _>("localization_blake3")? != root_lock.localization_blake3
    {
        return Err(PersistenceError::ProductionExtractionContentMismatch);
    }
    if selected != Some(request.character_id)
        || character.try_get::<i16, _>("life_state")? != 0
        || character.try_get::<i16, _>("security_state")? != SECURITY_NORMAL
        || root_lock.restore_state != RESTORE_ACTIVE
        || world.try_get::<i16, _>("location_kind")? != LOCATION_DANGER
        || optional_id(world.try_get("instance_lineage_id")?)? != Some(request.instance_lineage_id)
        || optional_id(world.try_get("entry_restore_point_id")?)?
            != Some(request.entry_restore_point_id)
        || !matches!(lineage.try_get::<i16, _>("lineage_state")?, 0 | 1)
        || exact_id(seam.try_get("account_id")?)? != request.account_id
        || exact_id(seam.try_get("character_id")?)? != request.character_id
        || exact_id(seam.try_get("encounter_id")?)? != request.encounter_id
        || exact_id(seam.try_get("instance_lineage_id")?)? != request.instance_lineage_id
        || exact_id(seam.try_get("entry_restore_point_id")?)? != request.entry_restore_point_id
        || exact_id(seam.try_get("exit_instance_id")?)? != request.exit_instance_id
        || seam.try_get::<String, _>("exit_content_id")? != crate::PRODUCTION_EXTRACTION_EXIT_ID
        || seam.try_get::<i32, _>("attempt_ordinal")?
            != exit.try_get::<i32, _>("attempt_ordinal")?
        || !(0..=7).contains(&party_slot)
        || exact_entity(seam.try_get("participant_entity_id")?)?
            != exact_entity(owner.try_get("owner_participant_entity_id")?)?
        || positive(seam.try_get("expected_character_version")?)? != character_version
        || seam.try_get::<i16, _>("extraction_state")? != 0
        || seam.try_get::<Option<i16>, _>("authority_kind")?.is_some()
        || exact_id(exit.try_get("instance_lineage_id")?)? != request.instance_lineage_id
        || exact_id(exit.try_get("exit_instance_id")?)? != request.exit_instance_id
        || exact_id(owner.try_get("owner_account_id")?)? != request.account_id
        || exact_id(owner.try_get("owner_character_id")?)? != request.character_id
        || exact_id(owner.try_get("reward_account_id")?)? != request.account_id
        || exact_id(owner.try_get("reward_character_id")?)? != request.character_id
        || exact_id(owner.try_get("reward_source_instance_id")?)? != request.encounter_id
        || owner.try_get::<String, _>("reward_table_id")? != "reward.boss_caldus"
        || owner.try_get::<String, _>("content_revision")? != crate::CORE_ITEM_CONTENT_REVISION
        || owner.try_get::<i16, _>("request_state")? != 1
        || exact_hash(owner.try_get("owner_reward_result_hash")?)?
            != exact_hash(owner.try_get("result_hash")?)?
    {
        return Err(PersistenceError::ProductionExtractionBindingMismatch);
    }
    Ok(LockedAuthority {
        account_version,
        character_version,
        world_version,
        inventory_version,
        life_metrics_version,
        root: root_lock,
    })
}

async fn reject_unresolved_reward_mutation(
    connection: &mut PgConnection,
    request: &ProductionExtractionCommitRequestV1,
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
        return Err(PersistenceError::ProductionExtractionUnresolvedMutation);
    }
    Ok(())
}

async fn load_inventory_snapshot(
    connection: &mut PgConnection,
    request: &ProductionExtractionCommitRequestV1,
    authority: &LockedAuthority,
) -> Result<
    (
        BTreeMap<[u8; ID_BYTES], LockedItem>,
        ExtractionInventorySnapshot,
    ),
    PersistenceError,
> {
    let rows = sqlx::query(
        "SELECT item_uid,template_id,content_revision,item_kind,item_version,security_state,
                location_kind,slot_index,character_id
         FROM item_instances
         WHERE namespace_id=$1 AND account_id=$2
           AND ((character_id=$3 AND location_kind IN (0,1,2,5,9))
             OR (character_id IS NULL AND location_kind IN (6,8)))
         ORDER BY item_uid FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_all(connection)
    .await?;
    let mut items = BTreeMap::new();
    let mut equipped = vec![DurableStorageSlot::Empty; EQUIPMENT_SLOT_COUNT];
    let mut belt = vec![DurableStorageSlot::Empty; TERMINAL_BELT_CAPACITY];
    let mut safe = vec![DurableStorageSlot::Empty; CHARACTER_SAFE_CAPACITY];
    let mut vault = vec![DurableStorageSlot::Empty; VAULT_CAPACITY];
    let mut overflow = vec![DurableStorageSlot::Empty; TERMINAL_OVERFLOW_CAPACITY];
    let mut backpack = vec![DurableStorageSlot::Empty; RUN_BACKPACK_CAPACITY];
    let mut hold = vec![DurableStorageSlot::Empty; TERMINAL_RESOLUTION_HOLD_CAPACITY];
    for row in rows {
        let item = LockedItem {
            item_uid: exact_id(row.try_get("item_uid")?)?,
            template_id: row.try_get("template_id")?,
            content_revision: row.try_get("content_revision")?,
            item_kind: row.try_get("item_kind")?,
            item_version: positive(row.try_get("item_version")?)?,
            security_state: row.try_get("security_state")?,
            location_kind: row.try_get("location_kind")?,
            slot_index: u16_value(row.try_get("slot_index")?)?,
        };
        let expected_security = match item.location_kind {
            0 | 1 => SECURITY_AT_RISK_EQUIPPED,
            2 => SECURITY_AT_RISK_PENDING,
            5 | 6 | 8 | 9 => SECURITY_SAFE,
            _ => return Err(PersistenceError::CorruptStoredExtraction),
        };
        if item.security_state != expected_security {
            return Err(PersistenceError::ProductionExtractionBindingMismatch);
        }
        if item.content_revision != crate::CORE_ITEM_CONTENT_REVISION {
            return Err(PersistenceError::ProductionExtractionContentMismatch);
        }
        let target = match item.location_kind {
            0 => &mut equipped,
            1 => &mut belt,
            2 => &mut backpack,
            5 => &mut safe,
            6 => &mut vault,
            8 => &mut overflow,
            9 => &mut hold,
            _ => unreachable!(),
        };
        append_slot(target, &item)?;
        if items.insert(item.item_uid, item).is_some() {
            return Err(PersistenceError::CorruptStoredExtraction);
        }
    }
    Ok((
        items,
        ExtractionInventorySnapshot {
            account_version: authority.account_version,
            inventory_version: authority.inventory_version,
            committed_at_unix_micros: 1,
            equipped,
            belt,
            character_safe: safe,
            vault,
            overflow,
            run_backpack: backpack,
            resolution_hold: hold,
            materials: Vec::new(),
        },
    ))
}

fn append_slot(
    slots: &mut [DurableStorageSlot],
    item: &LockedItem,
) -> Result<(), PersistenceError> {
    let index = usize::from(item.slot_index);
    let slot = slots
        .get_mut(index)
        .ok_or(PersistenceError::CorruptStoredExtraction)?;
    let uid = ItemUid::new(item.item_uid).map_err(|_| PersistenceError::CorruptStoredExtraction)?;
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
        _ => return Err(PersistenceError::CorruptStoredExtraction),
    }
    Ok(())
}

async fn load_material_snapshot(
    connection: &mut PgConnection,
    request: &ProductionExtractionCommitRequestV1,
) -> Result<
    (
        BTreeMap<String, LockedWallet>,
        BTreeMap<String, LockedPouch>,
        Vec<TerminalMaterialSnapshot>,
    ),
    PersistenceError,
> {
    let wallet_rows = sqlx::query(
        "SELECT material_id,quantity,wallet_cap,material_version
         FROM account_material_wallet_balances_v1
         WHERE namespace_id=$1 AND account_id=$2
         ORDER BY material_id COLLATE \"C\" FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .fetch_all(&mut *connection)
    .await?;
    let pouch_rows = sqlx::query(
        "SELECT material_id,quantity,material_version
         FROM character_run_material_stacks
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
           AND security_state=2 AND quantity>0
         ORDER BY material_id COLLATE \"C\" FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_all(connection)
    .await?;
    let mut wallets = BTreeMap::new();
    for row in wallet_rows {
        let material_id: String = row.try_get("material_id")?;
        let wallet = LockedWallet {
            quantity: u32_value(row.try_get("quantity")?)?,
            wallet_cap: u32_value(row.try_get("wallet_cap")?)?,
            version: positive(row.try_get("material_version")?)?,
        };
        if wallets.insert(material_id, wallet).is_some() {
            return Err(PersistenceError::CorruptStoredExtraction);
        }
    }
    if wallets.len() != 4 {
        return Err(PersistenceError::CorruptStoredExtraction);
    }
    let mut pouches = BTreeMap::new();
    let mut snapshots = Vec::new();
    for row in pouch_rows {
        let pouch = LockedPouch {
            material_id: row.try_get("material_id")?,
            quantity: u16_from_i32(row.try_get("quantity")?)?,
            version: positive(row.try_get("material_version")?)?,
        };
        let wallet = wallets
            .get(&pouch.material_id)
            .ok_or(PersistenceError::CorruptStoredExtraction)?;
        snapshots.push(TerminalMaterialSnapshot {
            material_id: pouch.material_id.clone(),
            safe_quantity: wallet.quantity,
            pending_quantity: pouch.quantity,
            wallet_cap: wallet.wallet_cap,
            wallet_version: wallet.version,
            pouch_version: pouch.version,
        });
        if pouches.insert(pouch.material_id.clone(), pouch).is_some() {
            return Err(PersistenceError::CorruptStoredExtraction);
        }
    }
    Ok((wallets, pouches, snapshots))
}

fn build_placements(
    items: &BTreeMap<[u8; ID_BYTES], LockedItem>,
    request: &ProductionExtractionCommitRequestV1,
    planned: &[sim_core::TerminalItemPlacement],
) -> Result<Vec<StoredProductionExtractionPlacementV1>, PersistenceError> {
    planned
        .iter()
        .enumerate()
        .map(|(index, placement)| {
            let item_uid = placement.item_uid.bytes();
            let item = items
                .get(&item_uid)
                .ok_or(PersistenceError::CorruptStoredExtraction)?;
            let source = stored_location(placement.source);
            if source.durable_kind() != item.location_kind || source.slot_index() != item.slot_index
            {
                return Err(PersistenceError::CorruptStoredExtraction);
            }
            Ok(StoredProductionExtractionPlacementV1 {
                ordinal: u16::try_from(index)
                    .map_err(|_| PersistenceError::CorruptStoredExtraction)?,
                item_uid,
                template_id: item.template_id.clone(),
                item_kind: u8::try_from(item.item_kind)
                    .map_err(|_| PersistenceError::CorruptStoredExtraction)?,
                source,
                destination: stored_location(placement.destination),
                pre_item_version: item.item_version,
                post_item_version: item
                    .item_version
                    .checked_add(1)
                    .ok_or(PersistenceError::CorruptStoredExtraction)?,
                ledger_event_id: derived_id(
                    ITEM_LEDGER_ID_CONTEXT,
                    &[&request.terminal_id, &item_uid],
                ),
            })
        })
        .collect()
}

fn build_material_credits(
    request: &ProductionExtractionCommitRequestV1,
    planned: &[sim_core::TerminalMaterialCredit],
) -> Result<Vec<StoredProductionExtractionMaterialCreditV1>, PersistenceError> {
    planned
        .iter()
        .enumerate()
        .map(|(index, credit)| {
            Ok(StoredProductionExtractionMaterialCreditV1 {
                ordinal: u8::try_from(index)
                    .map_err(|_| PersistenceError::CorruptStoredExtraction)?,
                material_id: credit.material_id.clone(),
                credited_quantity: u8::try_from(credit.credited_quantity)
                    .map_err(|_| PersistenceError::CorruptStoredExtraction)?,
                wallet_cap: material_wallet_cap(&credit.material_id)?,
                pre_wallet_quantity: credit.pre_safe_quantity,
                post_wallet_quantity: credit.post_safe_quantity,
                pre_wallet_version: credit.pre_wallet_version,
                post_wallet_version: credit.post_wallet_version,
                pre_pouch_version: credit.pre_pouch_version,
                post_pouch_version: credit.post_pouch_version,
                wallet_ledger_event_id: derived_id(
                    MATERIAL_LEDGER_ID_CONTEXT,
                    &[&request.terminal_id, credit.material_id.as_bytes()],
                ),
            })
        })
        .collect()
}

fn material_wallet_cap(material_id: &str) -> Result<u32, PersistenceError> {
    match material_id {
        "material.bell_brass" | "material.funeral_root" | "material.saltglass_shard" => Ok(999),
        "material.echo_ember" => Ok(99),
        _ => Err(PersistenceError::CorruptStoredExtraction),
    }
}

async fn insert_terminal_root(
    connection: &mut PgConnection,
    request: &ProductionExtractionCommitRequestV1,
    authority: &LockedAuthority,
    result: &StoredProductionExtractionResultV1,
    result_hash: [u8; HASH_BYTES],
    result_payload: &[u8],
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO character_extraction_terminal_results_v1
         (namespace_id,account_id,character_id,mutation_id,terminal_id,
          extraction_request_id,extraction_receipt_id,contract_version,terminal_kind,
          canonical_request_hash,canonical_plan_hash,result_hash,result_payload,
          encounter_id,instance_lineage_id,entry_restore_point_id,exit_instance_id,
          source_content_id,destination_content_id,records_blake3,assets_blake3,
          localization_blake3,issued_at,observed_tick,committed_tick,committed_at,
          pre_character_security_state,post_character_security_state,
          pre_account_version,post_account_version,pre_character_version,
          post_character_version,pre_world_version,post_world_version,
          pre_inventory_version,post_inventory_version,pre_life_metrics_version,
          post_life_metrics_version,placement_count,material_credit_count,
          storage_resolution_required)
         VALUES ($1,$2,$3,$4,$5,$6,$7,1,2,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,
                 $18,$19,$20,to_timestamp($21::double precision/1000.0),$22,$22,
                 transaction_timestamp(),0,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32,
                 $33,$34,$35,$36)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(request.terminal_id.as_slice())
    .bind(request.extraction_request_id.as_slice())
    .bind(request.extraction_receipt_id.as_slice())
    .bind(result.canonical_request_hash.as_slice())
    .bind(result.canonical_plan_hash.as_slice())
    .bind(result_hash.as_slice())
    .bind(result_payload)
    .bind(request.encounter_id.as_slice())
    .bind(request.instance_lineage_id.as_slice())
    .bind(request.entry_restore_point_id.as_slice())
    .bind(request.exit_instance_id.as_slice())
    .bind(crate::PRODUCTION_EXTRACTION_EXIT_ID)
    .bind(crate::PRODUCTION_EXTRACTION_HALL_ID)
    .bind(&authority.root.records_blake3)
    .bind(&authority.root.assets_blake3)
    .bind(&authority.root.localization_blake3)
    .bind(i64_value(request.issued_at_unix_ms)?)
    .bind(i64_value(request.observed_tick)?)
    .bind(i16::from(result.storage_resolution_required))
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
    .bind(i16::try_from(result.placements.len()).map_err(|_| corrupt())?)
    .bind(i16::try_from(result.material_credits.len()).map_err(|_| corrupt())?)
    .bind(result.storage_resolution_required)
    .execute(connection)
    .await?
    .rows_affected()
    .eq(&1)
    .then_some(())
    .ok_or_else(corrupt)
}

async fn apply_item_placements(
    connection: &mut PgConnection,
    request: &ProductionExtractionCommitRequestV1,
    result: &StoredProductionExtractionResultV1,
) -> Result<(), PersistenceError> {
    for placement in &result.placements {
        let destination_kind = placement.destination.durable_kind();
        let destination_slot = i16_value(placement.destination.slot_index())?;
        let character_id = if matches!(
            placement.destination,
            StoredExtractionLocationV1::Vault(_) | StoredExtractionLocationV1::Overflow(_)
        ) {
            None
        } else {
            Some(request.character_id.as_slice())
        };
        let updated = sqlx::query(
            "UPDATE item_instances SET character_id=$1,item_version=$2,security_state=0,
                    location_kind=$3,slot_index=$4,instance_id=NULL,pickup_id=NULL,
                    expires_at_tick=NULL,destruction_reason=NULL,terminal_extraction_id=$5,
                    extracted_at=transaction_timestamp(),
                    overflow_expires_at=CASE WHEN $3=8
                        THEN transaction_timestamp()+INTERVAL '72 hours' ELSE NULL END,
                    updated_at=transaction_timestamp()
             WHERE namespace_id=$6 AND account_id=$7 AND item_uid=$8
               AND item_version=$9 AND security_state=$10
               AND location_kind=$11 AND slot_index=$12",
        )
        .bind(character_id)
        .bind(i64_value(placement.post_item_version)?)
        .bind(destination_kind)
        .bind(destination_slot)
        .bind(request.terminal_id.as_slice())
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(placement.item_uid.as_slice())
        .bind(i64_value(placement.pre_item_version)?)
        .bind(source_security(placement.source))
        .bind(placement.source.durable_kind())
        .bind(i16_value(placement.source.slot_index())?)
        .execute(&mut *connection)
        .await?
        .rows_affected();
        if updated != 1 {
            return Err(PersistenceError::ProductionExtractionBindingMismatch);
        }
        sqlx::query(
            "INSERT INTO item_ledger_events
             (namespace_id,ledger_event_id,item_uid,account_id,character_id,mutation_id,
              event_kind,source_kind,pre_item_version,post_item_version,pre_security_state,
              post_security_state,pre_location_kind,post_location_kind,terminal_extraction_id)
             VALUES ($1,$2,$3,$4,$5,$6,1,5,$7,$8,$9,0,$10,$11,$12)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(placement.ledger_event_id.as_slice())
        .bind(placement.item_uid.as_slice())
        .bind(request.account_id.as_slice())
        .bind(request.character_id.as_slice())
        .bind(request.mutation_id.as_slice())
        .bind(i64_value(placement.pre_item_version)?)
        .bind(i64_value(placement.post_item_version)?)
        .bind(source_security(placement.source))
        .bind(placement.source.durable_kind())
        .bind(placement.destination.durable_kind())
        .bind(request.terminal_id.as_slice())
        .execute(&mut *connection)
        .await?;
        sqlx::query(
            "INSERT INTO extraction_terminal_item_placements_v1
             (namespace_id,account_id,character_id,terminal_id,mutation_id,
              placement_ordinal,item_uid,template_id,item_kind,source_kind,
              source_slot_index,destination_kind,destination_slot_index,
              pre_item_version,post_item_version,pre_security_state,post_security_state,
              ledger_event_id,ledger_event_kind,ledger_source_kind)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,0,$17,1,5)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(request.character_id.as_slice())
        .bind(request.terminal_id.as_slice())
        .bind(request.mutation_id.as_slice())
        .bind(i16::try_from(placement.ordinal).map_err(|_| corrupt())?)
        .bind(placement.item_uid.as_slice())
        .bind(&placement.template_id)
        .bind(i16::from(placement.item_kind))
        .bind(placement.source.durable_kind())
        .bind(i16_value(placement.source.slot_index())?)
        .bind(placement.destination.durable_kind())
        .bind(i16_value(placement.destination.slot_index())?)
        .bind(i64_value(placement.pre_item_version)?)
        .bind(i64_value(placement.post_item_version)?)
        .bind(source_security(placement.source))
        .bind(placement.ledger_event_id.as_slice())
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

async fn apply_material_credits(
    connection: &mut PgConnection,
    request: &ProductionExtractionCommitRequestV1,
    result: &StoredProductionExtractionResultV1,
    wallets: &BTreeMap<String, LockedWallet>,
    pouches: &BTreeMap<String, LockedPouch>,
) -> Result<(), PersistenceError> {
    for credit in &result.material_credits {
        let wallet = wallets.get(&credit.material_id).ok_or_else(corrupt)?;
        let pouch = pouches.get(&credit.material_id).ok_or_else(corrupt)?;
        validate_material_credit_prestate(credit, wallet, pouch)?;
        update_material_credit_balances(connection, request, credit).await?;
        insert_material_credit_records(connection, request, credit).await?;
    }
    Ok(())
}

fn validate_material_credit_prestate(
    credit: &StoredProductionExtractionMaterialCreditV1,
    wallet: &LockedWallet,
    pouch: &LockedPouch,
) -> Result<(), PersistenceError> {
    if wallet.quantity != credit.pre_wallet_quantity
        || wallet.wallet_cap != credit.wallet_cap
        || wallet.version != credit.pre_wallet_version
        || u32::from(pouch.quantity) != u32::from(credit.credited_quantity)
        || pouch.version != credit.pre_pouch_version
    {
        return Err(corrupt());
    }
    Ok(())
}

async fn update_material_credit_balances(
    connection: &mut PgConnection,
    request: &ProductionExtractionCommitRequestV1,
    credit: &StoredProductionExtractionMaterialCreditV1,
) -> Result<(), PersistenceError> {
    let wallet_updated = sqlx::query(
        "UPDATE account_material_wallet_balances_v1
         SET quantity=$1,material_version=$2,updated_at=transaction_timestamp()
         WHERE namespace_id=$3 AND account_id=$4 AND material_id=$5
           AND quantity=$6 AND wallet_cap=$7 AND material_version=$8",
    )
    .bind(i32_value(credit.post_wallet_quantity)?)
    .bind(i64_value(credit.post_wallet_version)?)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(&credit.material_id)
    .bind(i32_value(credit.pre_wallet_quantity)?)
    .bind(i32_value(credit.wallet_cap)?)
    .bind(i64_value(credit.pre_wallet_version)?)
    .execute(&mut *connection)
    .await?
    .rows_affected();
    let pouch_updated = sqlx::query(
        "UPDATE character_run_material_stacks
         SET quantity=0,material_version=$1,security_state=$2,
             terminal_reason='extraction',terminal_restore_point_id=NULL,
             terminal_death_id=NULL,terminal_extraction_id=$3,
             extracted_at=transaction_timestamp(),updated_at=transaction_timestamp()
         WHERE namespace_id=$4 AND account_id=$5 AND character_id=$6
           AND material_id=$7 AND quantity=$8 AND material_version=$9
           AND security_state=2",
    )
    .bind(i64_value(credit.post_pouch_version)?)
    .bind(MATERIAL_EXTRACTED)
    .bind(request.terminal_id.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(&credit.material_id)
    .bind(i32::from(credit.credited_quantity))
    .bind(i64_value(credit.pre_pouch_version)?)
    .execute(&mut *connection)
    .await?
    .rows_affected();
    if wallet_updated != 1 || pouch_updated != 1 {
        return Err(PersistenceError::ProductionExtractionBindingMismatch);
    }
    Ok(())
}

async fn insert_material_credit_records(
    connection: &mut PgConnection,
    request: &ProductionExtractionCommitRequestV1,
    credit: &StoredProductionExtractionMaterialCreditV1,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO account_material_ledger_events_v1
         (namespace_id,account_id,material_id,ledger_event_id,mutation_id,
          terminal_id,event_kind,delta,pre_balance,post_balance,
          pre_wallet_version,post_wallet_version)
         VALUES ($1,$2,$3,$4,$5,$6,0,$7,$8,$9,$10,$11)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(&credit.material_id)
    .bind(credit.wallet_ledger_event_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(request.terminal_id.as_slice())
    .bind(i32::from(credit.credited_quantity))
    .bind(i32_value(credit.pre_wallet_quantity)?)
    .bind(i32_value(credit.post_wallet_quantity)?)
    .bind(i64_value(credit.pre_wallet_version)?)
    .bind(i64_value(credit.post_wallet_version)?)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "INSERT INTO extraction_terminal_material_credits_v1
         (namespace_id,account_id,character_id,terminal_id,mutation_id,
          credit_ordinal,material_id,credited_quantity,wallet_cap,
          pre_wallet_quantity,post_wallet_quantity,pre_wallet_version,
          post_wallet_version,pre_pouch_quantity,post_pouch_quantity,
          pre_pouch_version,post_pouch_version,wallet_ledger_event_id)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$8,0,$14,$15,$16)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.terminal_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(i16::from(credit.ordinal))
    .bind(&credit.material_id)
    .bind(i32::from(credit.credited_quantity))
    .bind(i32_value(credit.wallet_cap)?)
    .bind(i32_value(credit.pre_wallet_quantity)?)
    .bind(i32_value(credit.post_wallet_quantity)?)
    .bind(i64_value(credit.pre_wallet_version)?)
    .bind(i64_value(credit.post_wallet_version)?)
    .bind(i64_value(credit.pre_pouch_version)?)
    .bind(i64_value(credit.post_pouch_version)?)
    .bind(credit.wallet_ledger_event_id.as_slice())
    .execute(connection)
    .await?;
    Ok(())
}

async fn apply_aggregate_heads(
    connection: &mut PgConnection,
    request: &ProductionExtractionCommitRequestV1,
    result: &StoredProductionExtractionResultV1,
) -> Result<(), PersistenceError> {
    if result.versions.account.post != result.versions.account.pre {
        expect_one(
            sqlx::query(
                "UPDATE accounts SET state_version=$1,updated_at=transaction_timestamp()
                 WHERE namespace_id=$2 AND account_id=$3 AND state_version=$4
                   AND selected_character_id=$5",
            )
            .bind(i64_value(result.versions.account.post)?)
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(request.account_id.as_slice())
            .bind(i64_value(result.versions.account.pre)?)
            .bind(request.character_id.as_slice())
            .execute(&mut *connection)
            .await?
            .rows_affected(),
        )?;
    }
    expect_one(
        sqlx::query(
            "UPDATE characters SET security_state=$1,character_state_version=$2,
                    updated_at=transaction_timestamp()
             WHERE namespace_id=$3 AND account_id=$4 AND character_id=$5
               AND life_state=0 AND security_state=0 AND character_state_version=$6",
        )
        .bind(i16::from(result.storage_resolution_required))
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
            "UPDATE character_world_locations SET character_version=$1,location_kind=$2,
                    location_content_id=$3,safe_arrival_kind=0,safe_spawn_id=NULL,
                    instance_lineage_id=NULL,entry_restore_point_id=NULL,
                    updated_at=transaction_timestamp()
             WHERE namespace_id=$4 AND account_id=$5 AND character_id=$6
               AND character_version=$7 AND location_kind=2
               AND instance_lineage_id=$8 AND entry_restore_point_id=$9",
        )
        .bind(i64_value(result.versions.world.post)?)
        .bind(LOCATION_SAFE)
        .bind(crate::PRODUCTION_EXTRACTION_HALL_ID)
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
            "UPDATE character_inventories SET inventory_version=$1,
                    updated_at=transaction_timestamp()
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
            "UPDATE character_life_metrics SET life_metrics_version=$1,
                    updated_at=transaction_timestamp()
             WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4
               AND life_metrics_version=$5",
        )
        .bind(i64_value(result.versions.life_metrics.post)?)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(request.character_id.as_slice())
        .bind(i64_value(result.versions.life_metrics.pre)?)
        .execute(connection)
        .await?
        .rows_affected(),
    )?;
    Ok(())
}

async fn close_danger_root(
    connection: &mut PgConnection,
    request: &ProductionExtractionCommitRequestV1,
) -> Result<(), PersistenceError> {
    expect_one(
        sqlx::query(
            "UPDATE character_entry_restore_points
             SET restore_state=$1,consumed_at=transaction_timestamp(),
                 extraction_terminal_id=$2
             WHERE namespace_id=$3 AND account_id=$4 AND character_id=$5
               AND restore_point_id=$6 AND lineage_id=$7 AND restore_state=0",
        )
        .bind(RESTORE_EXTRACTION_COMMITTED)
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
            "UPDATE character_instance_lineages SET lineage_state=$1,
                    closed_at=transaction_timestamp()
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

async fn project_production_seam(
    connection: &mut PgConnection,
    request: &ProductionExtractionCommitRequestV1,
    result: &StoredProductionExtractionResultV1,
    result_hash: [u8; HASH_BYTES],
) -> Result<(), PersistenceError> {
    expect_one(
        sqlx::query(
            "UPDATE character_extraction_results
             SET extraction_receipt_id=$1,receipt_payload_hash=$2,extraction_state=1,
                 authority_kind=1,destination_content_id=$3,safe_arrival_kind=0,
                 committed_at=transaction_timestamp(),transfer_mutation_id=$4,
                 post_character_version=$5,transferred_at=transaction_timestamp(),
                 production_mutation_id=$4
             WHERE namespace_id=$6 AND account_id=$7 AND character_id=$8
               AND extraction_request_id=$9 AND extraction_state=0",
        )
        .bind(request.extraction_receipt_id.as_slice())
        .bind(result_hash.as_slice())
        .bind(crate::PRODUCTION_EXTRACTION_HALL_ID)
        .bind(request.mutation_id.as_slice())
        .bind(i64_value(result.versions.character.post)?)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(request.character_id.as_slice())
        .bind(request.extraction_request_id.as_slice())
        .execute(connection)
        .await?
        .rows_affected(),
    )
}

async fn insert_terminal_audit_and_outbox(
    connection: &mut PgConnection,
    request: &ProductionExtractionCommitRequestV1,
    result_hash: [u8; HASH_BYTES],
    result_payload: &[u8],
) -> Result<(), PersistenceError> {
    let audit_id = derived_id(
        ACCEPTED_AUDIT_ID_CONTEXT,
        &[&request.terminal_id, &result_hash],
    );
    let event_id = derived_id(OUTBOX_ID_CONTEXT, &[&request.terminal_id, &result_hash]);
    sqlx::query(
        "INSERT INTO extraction_terminal_audit_events_v1
         (namespace_id,account_id,character_id,terminal_id,audit_event_id,
          event_type,event_digest)
         VALUES ($1,$2,$3,$4,$5,'extraction_committed',$6)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.terminal_id.as_slice())
    .bind(audit_id.as_slice())
    .bind(result_hash.as_slice())
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "INSERT INTO extraction_terminal_outbox_events_v1
         (namespace_id,account_id,character_id,terminal_id,event_id,event_type,event_payload)
         VALUES ($1,$2,$3,$4,$5,'extraction_committed',$6)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.terminal_id.as_slice())
    .bind(event_id.as_slice())
    .bind(result_payload)
    .execute(connection)
    .await?;
    Ok(())
}

async fn transaction_time_unix_micros(
    connection: &mut PgConnection,
) -> Result<u64, PersistenceError> {
    let value: i64 = sqlx::query_scalar(
        "SELECT floor(extract(epoch FROM transaction_timestamp()) * 1000000)::bigint",
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

fn stored_location(location: TerminalInventoryLocation) -> StoredExtractionLocationV1 {
    match location {
        TerminalInventoryLocation::Equipped(index) => StoredExtractionLocationV1::Equipped(index),
        TerminalInventoryLocation::Belt(index) => StoredExtractionLocationV1::Belt(index),
        TerminalInventoryLocation::RunBackpack(index) => {
            StoredExtractionLocationV1::RunBackpack(index)
        }
        TerminalInventoryLocation::CharacterSafe(index) => {
            StoredExtractionLocationV1::CharacterSafe(index)
        }
        TerminalInventoryLocation::Vault(index) => StoredExtractionLocationV1::Vault(index),
        TerminalInventoryLocation::Overflow(index) => StoredExtractionLocationV1::Overflow(index),
        TerminalInventoryLocation::ResolutionHold(index) => {
            StoredExtractionLocationV1::ResolutionHold(index)
        }
    }
}

const fn source_security(location: StoredExtractionLocationV1) -> i16 {
    match location {
        StoredExtractionLocationV1::Equipped(_) | StoredExtractionLocationV1::Belt(_) => {
            SECURITY_AT_RISK_EQUIPPED
        }
        StoredExtractionLocationV1::RunBackpack(_) => SECURITY_AT_RISK_PENDING,
        StoredExtractionLocationV1::CharacterSafe(_)
        | StoredExtractionLocationV1::Vault(_)
        | StoredExtractionLocationV1::Overflow(_)
        | StoredExtractionLocationV1::ResolutionHold(_) => SECURITY_SAFE,
    }
}

fn advance(pre: u64) -> Result<ProductionExtractionVersionAdvanceV1, PersistenceError> {
    Ok(ProductionExtractionVersionAdvanceV1 {
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

fn terminal_advisory_key(axis: u8, identity: [u8; ID_BYTES]) -> i64 {
    let mut hasher =
        blake3::Hasher::new_derive_key("gravebound.production-extraction-advisory-lock.v1");
    hasher.update(&[axis]);
    hasher.update(&identity);
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&hasher.finalize().as_bytes()[..8]);
    i64::from_be_bytes(bytes)
}

fn exact_id(value: Vec<u8>) -> Result<[u8; ID_BYTES], PersistenceError> {
    value.try_into().map_err(|_| corrupt())
}

fn exact_entity(value: Vec<u8>) -> Result<[u8; 8], PersistenceError> {
    let value: [u8; 8] = value.try_into().map_err(|_| corrupt())?;
    if value == [0; 8] {
        return Err(corrupt());
    }
    Ok(value)
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

fn u16_value(value: i16) -> Result<u16, PersistenceError> {
    u16::try_from(value).map_err(|_| corrupt())
}

fn u16_from_i32(value: i32) -> Result<u16, PersistenceError> {
    u16::try_from(value).map_err(|_| corrupt())
}

fn u32_value(value: i32) -> Result<u32, PersistenceError> {
    u32::try_from(value).map_err(|_| corrupt())
}

fn i16_value(value: u16) -> Result<i16, PersistenceError> {
    i16::try_from(value).map_err(|_| corrupt())
}

fn i32_value(value: u32) -> Result<i32, PersistenceError> {
    i32::try_from(value).map_err(|_| corrupt())
}

fn i64_value(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| corrupt())
}

fn expect_one(rows: u64) -> Result<(), PersistenceError> {
    if rows == 1 { Ok(()) } else { Err(corrupt()) }
}

const fn corrupt() -> PersistenceError {
    PersistenceError::CorruptStoredExtraction
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_terminal_ids_are_domain_separated_and_nonzero() {
        let parts = [&[1_u8; 16][..], &[2_u8; 32][..]];
        let item = derived_id(ITEM_LEDGER_ID_CONTEXT, &parts);
        let audit = derived_id(ACCEPTED_AUDIT_ID_CONTEXT, &parts);
        assert_ne!(item, [0; 16]);
        assert_ne!(item, audit);
    }

    #[test]
    fn advisory_keys_are_axis_bound_and_stable() {
        let identity = [7_u8; 16];
        assert_eq!(
            terminal_advisory_key(0, identity),
            terminal_advisory_key(0, identity)
        );
        assert_ne!(
            terminal_advisory_key(0, identity),
            terminal_advisory_key(1, identity)
        );
    }

    #[test]
    fn material_quantity_uses_the_postgresql_integer_width() {
        assert_eq!(u16_from_i32(99).unwrap(), 99);
        assert!(matches!(
            u16_from_i32(-1),
            Err(PersistenceError::CorruptStoredExtraction)
        ));
        assert!(matches!(
            u16_from_i32(i32::from(u16::MAX) + 1),
            Err(PersistenceError::CorruptStoredExtraction)
        ));
    }

    #[test]
    fn stored_locations_match_append_only_schema_discriminants() {
        assert_eq!(StoredExtractionLocationV1::Equipped(0).durable_kind(), 0);
        assert_eq!(StoredExtractionLocationV1::Belt(0).durable_kind(), 1);
        assert_eq!(StoredExtractionLocationV1::RunBackpack(0).durable_kind(), 2);
        assert_eq!(
            StoredExtractionLocationV1::CharacterSafe(0).durable_kind(),
            5
        );
        assert_eq!(StoredExtractionLocationV1::Vault(0).durable_kind(), 6);
        assert_eq!(StoredExtractionLocationV1::Overflow(0).durable_kind(), 8);
        assert_eq!(
            StoredExtractionLocationV1::ResolutionHold(0).durable_kind(),
            9
        );
    }
}
