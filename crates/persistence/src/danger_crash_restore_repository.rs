//! Single-writer `PostgreSQL` coordinator for exact danger crash restoration.
//!
//! The lock order in this module is part of the durable contract: account, character, restore
//! root, world location and lineage, progression, inventory, run materials, Oath/Bargain, life
//! metrics, then Ash. Every domain mutation and its normalized receipt commits in one
//! `SERIALIZABLE` transaction.

use std::collections::{BTreeMap, BTreeSet};

use sqlx::{PgConnection, Row};

use crate::{
    AshMutationCode, AshMutationKind, AshMutationRequest, AshWalletTransaction,
    DangerCrashAshChange, DangerCrashBargainChange, DangerCrashBargainRecordKind,
    DangerCrashItemChange, DangerCrashItemChangeKind, DangerCrashMaterialChange,
    DangerCrashRestoreCode, DangerCrashRestoreReceipt, DangerCrashRestoreRequest,
    DangerCrashRestoreTransaction, DangerCrashRestoreVersions, PersistenceError,
    PostgresPersistence, StoredProgression, WIPEABLE_CORE_NAMESPACE,
    ash_wallet::apply_ash_mutation_on_connection,
    danger_entry_restore::{
        DangerEntryActiveBargainDigestV3, DangerEntryAshDigestV3,
        DangerEntryContentRevisionDigestV3, DangerEntryInventoryDigestV3,
        DangerEntryInventoryItemDigestV3, DangerEntryInventoryLocationDigestV3,
        DangerEntryInventorySecurityDigestV3, DangerEntryLifeDigestV3, DangerEntryOathDigestV3,
        DangerEntryProgressionDigestV3, DangerEntrySnapshotDigestV3, DangerEntryVersionsDigestV3,
        StoredDangerEntryActiveBargainV3, StoredDangerEntryInventoryItemV3, ash_digest,
        inventory_digest, life_digest, oath_digest,
    },
    items::CORE_ITEM_CONTENT_REVISION,
    progression_restore::progression_component_digest,
};

const MAX_TRANSACTION_ATTEMPTS: u8 = 3;
const HALL_ID: &str = "hub.lantern_halls_01";
const LINEAGE_CRASH_FAILED: i16 = 3;
const RESTORE_ACTIVE: i16 = 0;
const RESTORE_CRASHED: i16 = 4;

#[derive(Debug)]
struct AccountLock {
    version: u64,
}

#[derive(Debug)]
struct CharacterLock {
    version: u64,
    life_state: i16,
}

#[derive(Debug)]
struct RootLock {
    lineage_id: [u8; 16],
    restore_location_id: String,
    restore_state: i16,
    crash_restore_mutation_id: Option<[u8; 16]>,
    account_version: u64,
    character_version: u64,
    progression_version: u64,
    inventory_version: u64,
    oath_bargain_version: u64,
    life_metrics_version: u64,
    ash_wallet_version: u64,
    records_blake3: String,
    assets_blake3: String,
    localization_blake3: String,
    composite_digest: [u8; 32],
}

#[derive(Debug)]
struct StoredRequestRow {
    account_id: [u8; 16],
    character_id: [u8; 16],
    restore_point_id: [u8; 16],
    mutation_id: [u8; 16],
    request_hash: [u8; 32],
    outcome_code: DangerCrashRestoreCode,
    observed_restore_state: i16,
    committed_mutation_id: Option<[u8; 16]>,
    payload: Vec<u8>,
    digest: [u8; 32],
}

#[derive(Debug)]
struct ProgressionLock {
    level: i16,
    total_xp: i32,
    current_health: i32,
    live_version: u64,
    snapshot_version: u64,
}

#[derive(Debug, Clone)]
struct BaselineItem {
    item_uid: [u8; 16],
    template_id: String,
    content_revision: String,
    item_kind: i16,
    creation_kind: i16,
    creation_request_id: [u8; 16],
    roll_index: i32,
    unit_ordinal: i32,
    provenance_kind: i16,
    location_kind: i16,
    slot_index: i16,
    entry_item_version: u64,
    entry_security_state: i16,
}

#[derive(Debug, Clone)]
struct LiveItem {
    item_uid: [u8; 16],
    template_id: String,
    content_revision: String,
    item_kind: i16,
    creation_kind: i16,
    creation_request_id: [u8; 16],
    roll_index: i32,
    unit_ordinal: i32,
    provenance_kind: i16,
    item_version: u64,
    security_state: i16,
    location_kind: i16,
    destruction_reason: Option<String>,
    proven_prior_crash_revocation: bool,
}

#[derive(Debug)]
struct InventoryLock {
    version: u64,
    pre_snapshot_version: u64,
    snapshot_version: u64,
    safe_placement_count: u16,
    baseline: Vec<BaselineItem>,
    live: BTreeMap<[u8; 16], LiveItem>,
}

#[derive(Debug)]
struct MaterialLock {
    material_id: String,
    quantity: i32,
    version: u64,
}

#[derive(Debug)]
struct OathBargainLock {
    oath_id: Option<String>,
    earned_slots: i16,
    version: u64,
    snapshot_version: u64,
    active: Vec<BaselineBargain>,
}

#[derive(Debug)]
struct BaselineBargain {
    bargain_id: String,
    acquisition_ordinal: i16,
    acquired_by_offer_id: [u8; 16],
    source_reward_event_id: [u8; 16],
    content_version: String,
    records_blake3: String,
    assets_blake3: String,
    localization_blake3: String,
}

#[derive(Debug)]
struct LifeLock {
    lifetime_ticks: u64,
    permadeath_combat_ticks: u64,
    rollback_permadeath_combat_ticks: u64,
    version: u64,
    snapshot_version: u64,
    captured_lifetime_ticks: u64,
}

#[derive(Debug)]
struct AshLock {
    snapshot_version: u64,
    wallet_version: u64,
    wallet_balance: i32,
    earns: Vec<AshEarn>,
}

#[derive(Debug)]
struct AshEarn {
    mutation_id: [u8; 16],
    amount: i32,
    content_version: String,
}

impl PostgresPersistence {
    /// Applies an exact danger-entry restore or returns the durable terminal result that won.
    pub async fn transact_danger_crash_restore(
        &self,
        request: &DangerCrashRestoreRequest,
    ) -> Result<DangerCrashRestoreTransaction, PersistenceError> {
        request.validate()?;
        for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self.transact_danger_crash_restore_once(request).await {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS
                        && crate::is_retryable_transaction_failure(&error) => {}
                result => return result,
            }
        }
        unreachable!("bounded danger crash-restore transaction loop always returns")
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the ordered cross-aggregate lock and commit sequence is intentionally contiguous for atomicity audit"
    )]
    async fn transact_danger_crash_restore_once(
        &self,
        request: &DangerCrashRestoreRequest,
    ) -> Result<DangerCrashRestoreTransaction, PersistenceError> {
        let mut transaction = self.begin_transaction().await?;
        let account = lock_account(transaction.connection(), request.account_id).await?;

        if let Some(stored) = load_request_result(transaction.connection(), request).await? {
            if stored.request_hash != request.request_hash
                || stored.character_id != request.character_id
                || stored.restore_point_id != request.restore_point_id
            {
                let audit_id = request.conflict_audit_id(stored.request_hash);
                insert_conflict_audit(transaction.connection(), request, &stored, audit_id).await?;
                transaction.commit().await?;
                return Ok(DangerCrashRestoreTransaction::Conflict {
                    mutation_id: request.mutation_id,
                    stored_request_hash: stored.request_hash,
                    attempted_request_hash: request.request_hash,
                    audit_id,
                });
            }
            let receipt = decode_stored_receipt(&stored)?;
            transaction.rollback().await?;
            return Ok(DangerCrashRestoreTransaction::Replayed(receipt));
        }

        let character = lock_character(transaction.connection(), request).await?;
        let root = lock_root(transaction.connection(), request).await?;
        if account.version < root.account_version || character.version < root.character_version {
            return Err(PersistenceError::CorruptStoredDangerCrashRestore);
        }
        lock_world_and_lineage(transaction.connection(), request, &root, character.version).await?;

        if root.restore_state != RESTORE_ACTIVE {
            let receipt = terminal_receipt(request, &root)?;
            insert_request_result(transaction.connection(), &receipt).await?;
            force_deferred_constraints(transaction.connection()).await?;
            transaction.commit().await?;
            return Ok(DangerCrashRestoreTransaction::Fresh(receipt));
        }
        if character.life_state != 0 || root.restore_location_id != HALL_ID {
            return Err(PersistenceError::CorruptStoredDangerCrashRestore);
        }

        let progression = lock_progression(transaction.connection(), request).await?;
        let inventory = lock_inventory(transaction.connection(), request).await?;
        let materials = lock_materials(transaction.connection(), request).await?;
        let oath_bargain = lock_oath_bargain(transaction.connection(), request).await?;
        let life = lock_life_metrics(transaction.connection(), request).await?;
        let ash = lock_ash_snapshot(transaction.connection(), request).await?;
        validate_root_snapshot(
            &root,
            request.character_id,
            &progression,
            &inventory,
            &oath_bargain,
            &life,
            ash.snapshot_version,
        )?;

        let post_progression_version = increment(progression.live_version)?;
        restore_progression(
            transaction.connection(),
            request,
            &progression,
            post_progression_version,
        )
        .await?;
        let post_inventory_version = increment(inventory.version)?;
        let item_changes = restore_inventory(
            transaction.connection(),
            request,
            &inventory,
            post_inventory_version,
        )
        .await?;
        let material_changes =
            revoke_materials(transaction.connection(), request, &materials).await?;
        let post_oath_bargain_version = increment(oath_bargain.version)?;
        let bargain_changes = restore_oath_bargain(
            transaction.connection(),
            request,
            &oath_bargain,
            post_oath_bargain_version,
        )
        .await?;
        let post_life_metrics_version = increment(life.version)?;
        restore_life_metrics(
            transaction.connection(),
            request,
            &life,
            post_life_metrics_version,
        )
        .await?;
        let ash_changes = compensate_ash(transaction.connection(), request, &ash).await?;
        let post_ash_wallet_version = lock_ash_version(transaction.connection(), request).await?;

        let post_account_version = increment(account.version)?;
        let post_character_version = increment(character.version)?;
        return_to_hall(
            transaction.connection(),
            request,
            &root,
            post_account_version,
            post_character_version,
        )
        .await?;

        set_restored_component_versions(
            transaction.connection(),
            request,
            post_progression_version,
            post_inventory_version,
            post_oath_bargain_version,
            post_life_metrics_version,
            post_ash_wallet_version,
        )
        .await?;

        let receipt = DangerCrashRestoreReceipt {
            contract: crate::DANGER_CRASH_RESTORE_CONTRACT.into(),
            account_id: request.account_id,
            character_id: request.character_id,
            restore_point_id: request.restore_point_id,
            request_mutation_id: request.mutation_id,
            request_hash: request.request_hash,
            code: DangerCrashRestoreCode::Restored,
            committed_mutation_id: Some(request.mutation_id),
            versions: Some(DangerCrashRestoreVersions {
                account: post_account_version,
                character: post_character_version,
                progression: post_progression_version,
                inventory: post_inventory_version,
                oath_bargain: post_oath_bargain_version,
                life_metrics: post_life_metrics_version,
                ash_wallet: post_ash_wallet_version,
            }),
            item_changes,
            material_changes,
            bargain_changes,
            ash_changes,
        };
        receipt.validate()?;
        insert_normalized_result(transaction.connection(), &receipt).await?;
        consume_root(transaction.connection(), request).await?;
        insert_request_result(transaction.connection(), &receipt).await?;
        force_deferred_constraints(transaction.connection()).await?;
        transaction.commit().await?;
        Ok(DangerCrashRestoreTransaction::Fresh(receipt))
    }
}

async fn lock_account(
    connection: &mut PgConnection,
    account_id: [u8; 16],
) -> Result<AccountLock, PersistenceError> {
    let row = sqlx::query(
        "SELECT state_version FROM accounts \
         WHERE namespace_id=$1 AND account_id=$2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_optional(connection)
    .await?
    .ok_or(PersistenceError::DangerCrashRestoreOwnerNotFound)?;
    Ok(AccountLock {
        version: positive(row.try_get("state_version")?)?,
    })
}

async fn lock_character(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
) -> Result<CharacterLock, PersistenceError> {
    let row = sqlx::query(
        "SELECT character_state_version, life_state FROM characters WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_optional(connection)
    .await?
    .ok_or(PersistenceError::DangerCrashRestoreOwnerNotFound)?;
    Ok(CharacterLock {
        version: positive(row.try_get("character_state_version")?)?,
        life_state: row.try_get("life_state")?,
    })
}

async fn lock_root(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
) -> Result<RootLock, PersistenceError> {
    let row = sqlx::query(
        "SELECT lineage_id, source_location_id, restore_location_id, restore_state, \
                crash_restore_mutation_id, snapshot_contract_version, component_mask, \
                account_version, character_version, progression_version, inventory_version, \
                oath_bargain_version, life_metrics_version, ash_wallet_version, \
                records_blake3, assets_blake3, localization_blake3, composite_digest \
         FROM character_entry_restore_points WHERE namespace_id=$1 AND account_id=$2 \
         AND character_id=$3 AND restore_point_id=$4 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.restore_point_id.as_slice())
    .fetch_optional(connection)
    .await?
    .ok_or(PersistenceError::DangerCrashRestorePointNotFound)?;
    let snapshot_contract_version: i16 = row.try_get("snapshot_contract_version")?;
    let component_mask: i16 = row.try_get("component_mask")?;
    let source_location_id: String = row.try_get("source_location_id")?;
    let value = RootLock {
        lineage_id: fixed(row.try_get("lineage_id")?)?,
        restore_location_id: row.try_get("restore_location_id")?,
        restore_state: row.try_get("restore_state")?,
        crash_restore_mutation_id: optional_fixed(row.try_get("crash_restore_mutation_id")?)?,
        account_version: positive(row.try_get("account_version")?)?,
        character_version: positive(row.try_get("character_version")?)?,
        progression_version: positive(row.try_get("progression_version")?)?,
        inventory_version: positive(row.try_get("inventory_version")?)?,
        oath_bargain_version: positive(row.try_get("oath_bargain_version")?)?,
        life_metrics_version: positive(row.try_get("life_metrics_version")?)?,
        ash_wallet_version: positive(row.try_get("ash_wallet_version")?)?,
        records_blake3: row.try_get("records_blake3")?,
        assets_blake3: row.try_get("assets_blake3")?,
        localization_blake3: row.try_get("localization_blake3")?,
        composite_digest: fixed(row.try_get("composite_digest")?)?,
    };
    if snapshot_contract_version != 3
        || component_mask != 31
        || source_location_id != HALL_ID
        || value.restore_location_id != HALL_ID
        || !is_lower_hex_hash(&value.records_blake3)
        || !is_lower_hex_hash(&value.assets_blake3)
        || !is_lower_hex_hash(&value.localization_blake3)
        || value.composite_digest == [0; 32]
    {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    Ok(value)
}

async fn lock_world_and_lineage(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
    root: &RootLock,
    character_version: u64,
) -> Result<(), PersistenceError> {
    let world = sqlx::query(
        "SELECT character_version, location_kind, location_content_id, instance_lineage_id, \
                entry_restore_point_id \
         FROM character_world_locations WHERE namespace_id=$1 AND account_id=$2 \
         AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::CorruptStoredDangerCrashRestore)?;
    let lineage = sqlx::query(
        "SELECT lineage_state, content_id, layout_id, records_blake3, assets_blake3, \
                localization_blake3 FROM character_instance_lineages WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3 AND lineage_id=$4 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(root.lineage_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::CorruptStoredDangerCrashRestore)?;
    let lineage_state: i16 = lineage.try_get("lineage_state")?;
    let lineage_content_id: String = lineage.try_get("content_id")?;
    let lineage_layout_id: String = lineage.try_get("layout_id")?;
    if lineage_layout_id != "layout.core_private_life_01"
        || lineage.try_get::<String, _>("records_blake3")? != root.records_blake3
        || lineage.try_get::<String, _>("assets_blake3")? != root.assets_blake3
        || lineage.try_get::<String, _>("localization_blake3")? != root.localization_blake3
    {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    if root.restore_state == RESTORE_ACTIVE
        && (world.try_get::<i16, _>("location_kind")? != 2
            || positive(world.try_get("character_version")?)? != character_version
            || world.try_get::<Option<String>, _>("location_content_id")?
                != Some(lineage_content_id)
            || optional_fixed(world.try_get("instance_lineage_id")?)? != Some(root.lineage_id)
            || optional_fixed(world.try_get("entry_restore_point_id")?)?
                != Some(request.restore_point_id)
            || !matches!(lineage_state, 0 | 1))
    {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    Ok(())
}

fn terminal_receipt(
    request: &DangerCrashRestoreRequest,
    root: &RootLock,
) -> Result<DangerCrashRestoreReceipt, PersistenceError> {
    let (code, committed_mutation_id) = match root.restore_state {
        1 => (DangerCrashRestoreCode::ExtractionCommitted, None),
        2 => (DangerCrashRestoreCode::DeathCommitted, None),
        3 => (DangerCrashRestoreCode::RecallCommitted, None),
        RESTORE_CRASHED => (
            DangerCrashRestoreCode::AlreadyCrashRestored,
            Some(
                root.crash_restore_mutation_id
                    .ok_or(PersistenceError::CorruptStoredDangerCrashRestore)?,
            ),
        ),
        _ => return Err(PersistenceError::CorruptStoredDangerCrashRestore),
    };
    let receipt = DangerCrashRestoreReceipt {
        contract: crate::DANGER_CRASH_RESTORE_CONTRACT.into(),
        account_id: request.account_id,
        character_id: request.character_id,
        restore_point_id: request.restore_point_id,
        request_mutation_id: request.mutation_id,
        request_hash: request.request_hash,
        code,
        committed_mutation_id,
        versions: None,
        item_changes: Vec::new(),
        material_changes: Vec::new(),
        bargain_changes: Vec::new(),
        ash_changes: Vec::new(),
    };
    receipt.validate()?;
    Ok(receipt)
}

async fn force_deferred_constraints(connection: &mut PgConnection) -> Result<(), PersistenceError> {
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(connection)
        .await?;
    Ok(())
}

fn positive(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(PersistenceError::CorruptStoredDangerCrashRestore)
}

fn nonnegative(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value).map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)
}

fn increment(value: u64) -> Result<u64, PersistenceError> {
    value
        .checked_add(1)
        .ok_or(PersistenceError::CorruptStoredDangerCrashRestore)
}

fn as_i64(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)
}

fn fixed<const N: usize>(value: Vec<u8>) -> Result<[u8; N], PersistenceError> {
    value
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)
}

fn optional_fixed<const N: usize>(
    value: Option<Vec<u8>>,
) -> Result<Option<[u8; N]>, PersistenceError> {
    value.map(fixed).transpose()
}

fn is_lower_hex_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

async fn lock_progression(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
) -> Result<ProgressionLock, PersistenceError> {
    let row = sqlx::query(
        "SELECT snapshot.level, snapshot.total_xp, snapshot.current_health, \
                snapshot.progression_version AS snapshot_version, \
                snapshot.component_digest, \
                live.progression_version AS live_version \
         FROM entry_restore_progression_v3 snapshot JOIN character_progression live \
           USING (namespace_id, account_id, character_id) \
         WHERE snapshot.namespace_id=$1 AND snapshot.account_id=$2 \
           AND snapshot.character_id=$3 AND snapshot.restore_point_id=$4 \
           AND snapshot.restored_progression_version IS NULL FOR UPDATE OF snapshot, live",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.restore_point_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::CorruptStoredDangerCrashRestore)?;
    sqlx::query(
        "SELECT reward_event_id FROM character_xp_award_results WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3 AND entry_restore_point_id=$4 \
         ORDER BY reward_event_id FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.restore_point_id.as_slice())
    .fetch_all(connection)
    .await?;
    let snapshot_version = positive(row.try_get("snapshot_version")?)?;
    let value = ProgressionLock {
        level: row.try_get("level")?,
        total_xp: row.try_get("total_xp")?,
        current_health: row.try_get("current_health")?,
        live_version: positive(row.try_get("live_version")?)?,
        snapshot_version,
    };
    let stored_digest: [u8; 32] = fixed(row.try_get("component_digest")?)?;
    let progression_digest = progression_component_digest(&StoredProgression {
        total_xp: value.total_xp,
        level: value.level,
        current_health: value.current_health,
        progression_version: as_i64(snapshot_version)?,
    })
    .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?;
    if value.live_version < snapshot_version || progression_digest != stored_digest {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    Ok(value)
}

async fn restore_progression(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
    progression: &ProgressionLock,
    post_version: u64,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "DELETE FROM account_boss_first_clears clear USING character_xp_award_results award \
         WHERE clear.namespace_id=award.namespace_id AND clear.account_id=award.account_id \
           AND clear.reward_event_id=award.reward_event_id AND award.namespace_id=$1 \
           AND award.account_id=$2 AND award.character_id=$3 \
           AND award.entry_restore_point_id=$4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.restore_point_id.as_slice())
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "UPDATE character_xp_award_results SET revoked_by_restore_point_id=$1, \
                revoked_at=transaction_timestamp(), revocation_progression_version=$2 \
         WHERE namespace_id=$3 AND account_id=$4 AND character_id=$5 \
           AND entry_restore_point_id=$1 AND revoked_by_restore_point_id IS NULL",
    )
    .bind(request.restore_point_id.as_slice())
    .bind(as_i64(post_version)?)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .execute(&mut *connection)
    .await?;
    let inventory_updated = sqlx::query(
        "UPDATE character_progression SET level=$1, total_xp=$2, current_health=$3, \
                progression_version=$4, updated_at=transaction_timestamp() \
         WHERE namespace_id=$5 AND account_id=$6 AND character_id=$7 \
           AND progression_version=$8",
    )
    .bind(progression.level)
    .bind(progression.total_xp)
    .bind(progression.current_health)
    .bind(as_i64(post_version)?)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(as_i64(progression.live_version)?)
    .execute(&mut *connection)
    .await?
    .rows_affected();
    if inventory_updated != 1 {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    sqlx::query(
        "UPDATE characters SET level=$1 WHERE namespace_id=$2 AND account_id=$3 \
         AND character_id=$4",
    )
    .bind(i32::from(progression.level))
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .execute(connection)
    .await?;
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "the parent, exact baseline identities, and complete live custody are one lock snapshot"
)]
async fn lock_inventory(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
) -> Result<InventoryLock, PersistenceError> {
    let version = positive(
        sqlx::query_scalar::<_, i64>(
            "SELECT inventory_version FROM character_inventories WHERE namespace_id=$1 \
             AND account_id=$2 AND character_id=$3 FOR UPDATE",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(request.character_id.as_slice())
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(PersistenceError::CorruptStoredDangerCrashRestore)?,
    )?;
    let parent = sqlx::query(
        "SELECT baseline_item_count, pre_inventory_version, post_inventory_version, \
                safe_placement_count, component_digest FROM entry_restore_inventory_v3 \
         WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3 AND restore_point_id=$4 \
         AND restored_inventory_version IS NULL FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.restore_point_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::CorruptStoredDangerCrashRestore)?;
    let parent_count: i16 = parent.try_get("baseline_item_count")?;
    let pre_snapshot_version = positive(parent.try_get("pre_inventory_version")?)?;
    let snapshot_version = positive(parent.try_get("post_inventory_version")?)?;
    let safe_placement_count = u16::try_from(parent.try_get::<i16, _>("safe_placement_count")?)
        .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?;
    if version < snapshot_version {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    let baseline_rows = sqlx::query(
        "SELECT item_uid, template_id, content_revision, item_kind, creation_kind, \
                creation_request_id, roll_index, unit_ordinal, provenance_kind, \
                location_kind, slot_index, entry_item_version, entry_security_state \
         FROM entry_restore_inventory_items_v3 \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND restore_point_id=$4 \
         ORDER BY item_ordinal FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.restore_point_id.as_slice())
    .fetch_all(&mut *connection)
    .await?;
    if usize::try_from(parent_count).ok() != Some(baseline_rows.len()) {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    let mut baseline = Vec::with_capacity(baseline_rows.len());
    for row in baseline_rows {
        baseline.push(BaselineItem {
            item_uid: fixed(row.try_get("item_uid")?)?,
            template_id: row.try_get("template_id")?,
            content_revision: row.try_get("content_revision")?,
            item_kind: row.try_get("item_kind")?,
            creation_kind: row.try_get("creation_kind")?,
            creation_request_id: fixed(row.try_get("creation_request_id")?)?,
            roll_index: row.try_get("roll_index")?,
            unit_ordinal: row.try_get("unit_ordinal")?,
            provenance_kind: row.try_get("provenance_kind")?,
            location_kind: row.try_get("location_kind")?,
            slot_index: row.try_get("slot_index")?,
            entry_item_version: positive(row.try_get("entry_item_version")?)?,
            entry_security_state: row.try_get("entry_security_state")?,
        });
    }
    validate_inventory_snapshot(
        pre_snapshot_version,
        snapshot_version,
        safe_placement_count,
        &baseline,
    )?;
    let live_rows = sqlx::query(
        "SELECT item.item_uid, item.template_id, item.content_revision, item.item_kind, \
                item.creation_kind, \
                creation_request_id, roll_index, unit_ordinal, provenance_kind, item_version, \
                security_state, location_kind, destruction_reason, EXISTS ( \
                    SELECT 1 FROM danger_crash_restore_item_changes change \
                    JOIN danger_crash_restore_results result \
                      ON result.namespace_id=change.namespace_id \
                     AND result.account_id=change.account_id \
                     AND result.character_id=change.character_id \
                     AND result.restore_point_id=change.restore_point_id \
                     AND result.mutation_id=change.mutation_id \
                    WHERE change.namespace_id=item.namespace_id \
                      AND change.account_id=item.account_id \
                      AND change.character_id=item.character_id \
                      AND change.item_uid=item.item_uid AND change.change_kind=1 \
                      AND change.post_item_version=item.item_version \
                      AND change.post_security_state=item.security_state \
                      AND change.post_location_kind=item.location_kind \
                      AND change.ledger_reason='crash_revoked' \
                ) AS proven_prior_crash_revocation \
         FROM item_instances item WHERE item.namespace_id=$1 \
         AND item.account_id=$2 AND item.character_id=$3 ORDER BY item.item_uid FOR UPDATE OF item",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_all(connection)
    .await?;
    let mut live = BTreeMap::new();
    for row in live_rows {
        let item = LiveItem {
            item_uid: fixed(row.try_get("item_uid")?)?,
            template_id: row.try_get("template_id")?,
            content_revision: row.try_get("content_revision")?,
            item_kind: row.try_get("item_kind")?,
            creation_kind: row.try_get("creation_kind")?,
            creation_request_id: fixed(row.try_get("creation_request_id")?)?,
            roll_index: row.try_get("roll_index")?,
            unit_ordinal: row.try_get("unit_ordinal")?,
            provenance_kind: row.try_get("provenance_kind")?,
            item_version: positive(row.try_get("item_version")?)?,
            security_state: row.try_get("security_state")?,
            location_kind: row.try_get("location_kind")?,
            destruction_reason: row.try_get("destruction_reason")?,
            proven_prior_crash_revocation: row.try_get("proven_prior_crash_revocation")?,
        };
        if live.insert(item.item_uid, item).is_some() {
            return Err(PersistenceError::CorruptStoredDangerCrashRestore);
        }
    }
    let digest_items = baseline
        .iter()
        .map(|item| StoredDangerEntryInventoryItemV3 {
            item_uid: item.item_uid,
            template_id: item.template_id.clone(),
            content_revision: item.content_revision.clone(),
            item_kind: item.item_kind,
            creation_kind: item.creation_kind,
            creation_request_id: item.creation_request_id,
            roll_index: item.roll_index,
            unit_ordinal: item.unit_ordinal,
            provenance_kind: item.provenance_kind,
            location_kind: item.location_kind,
            slot_index: item.slot_index,
            entry_item_version: item.entry_item_version,
            entry_security_state: item.entry_security_state,
        })
        .collect::<Vec<_>>();
    let stored_digest: [u8; 32] = fixed(parent.try_get("component_digest")?)?;
    if inventory_digest(
        pre_snapshot_version,
        snapshot_version,
        safe_placement_count,
        &digest_items,
    ) != stored_digest
    {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    Ok(InventoryLock {
        version,
        pre_snapshot_version,
        snapshot_version,
        safe_placement_count,
        baseline,
        live,
    })
}

#[allow(
    clippy::too_many_lines,
    reason = "item custody neutralization, exact restoration, ledgers, and normalized changes form one auditable operation"
)]
async fn restore_inventory(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
    inventory: &InventoryLock,
    post_inventory_version: u64,
) -> Result<Vec<DangerCrashItemChange>, PersistenceError> {
    let baseline_ids = inventory
        .baseline
        .iter()
        .map(|item| item.item_uid)
        .collect::<BTreeSet<_>>();
    let mut changes = Vec::new();
    let mut revocable = Vec::new();

    for item in inventory
        .live
        .values()
        .filter(|item| !baseline_ids.contains(&item.item_uid))
    {
        if classify_active_danger_custody(item)? {
            revocable.push(item);
        }
    }

    for item in revocable {
        changes.push(apply_item_change(connection, request, item, None).await?);
    }

    for baseline in &inventory.baseline {
        let live = inventory
            .live
            .get(&baseline.item_uid)
            .ok_or(PersistenceError::CorruptStoredDangerCrashRestore)?;
        validate_baseline_identity(baseline, live)?;
        classify_active_danger_custody(live)?;
        sqlx::query(
            "UPDATE item_instances SET security_state=3, location_kind=4, slot_index=NULL, \
                    instance_id=NULL, pickup_id=NULL, expires_at_tick=NULL, \
                    destruction_reason='crash_revoked', updated_at=transaction_timestamp() \
             WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND item_uid=$4",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(request.character_id.as_slice())
        .bind(baseline.item_uid.as_slice())
        .execute(&mut *connection)
        .await?;
    }
    for baseline in &inventory.baseline {
        let live = inventory
            .live
            .get(&baseline.item_uid)
            .ok_or(PersistenceError::CorruptStoredDangerCrashRestore)?;
        changes.push(apply_item_change(connection, request, live, Some(baseline)).await?);
    }
    changes.sort_by_key(|change| {
        (
            match change.kind {
                DangerCrashItemChangeKind::Restored => 0_i16,
                DangerCrashItemChangeKind::Revoked => 1_i16,
            },
            change.pre_location_kind,
            change.item_uid,
        )
    });
    let inventory_updated = sqlx::query(
        "UPDATE character_inventories SET inventory_version=$1, \
                updated_at=transaction_timestamp() WHERE namespace_id=$2 AND account_id=$3 \
           AND character_id=$4 AND inventory_version=$5",
    )
    .bind(as_i64(post_inventory_version)?)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(as_i64(inventory.version)?)
    .execute(connection)
    .await?
    .rows_affected();
    if inventory_updated != 1 {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    Ok(changes)
}

fn validate_baseline_identity(
    baseline: &BaselineItem,
    live: &LiveItem,
) -> Result<(), PersistenceError> {
    if baseline.template_id != live.template_id
        || baseline.content_revision != live.content_revision
        || baseline.item_kind != live.item_kind
        || baseline.creation_kind != live.creation_kind
        || baseline.creation_request_id != live.creation_request_id
        || baseline.roll_index != live.roll_index
        || baseline.unit_ordinal != live.unit_ordinal
        || baseline.provenance_kind != live.provenance_kind
        || live.item_version < baseline.entry_item_version
    {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    Ok(())
}

fn validate_inventory_snapshot(
    pre_version: u64,
    post_version: u64,
    safe_placement_count: u16,
    baseline: &[BaselineItem],
) -> Result<(), PersistenceError> {
    if baseline.len() > 64
        || safe_placement_count > 48
        || post_version < pre_version
        || post_version > pre_version.saturating_add(1)
    {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    let mut equipment_slots = BTreeSet::new();
    let mut belt_stacks = BTreeMap::<i16, (&str, &str, usize)>::new();
    let mut backpack_stacks = BTreeMap::<i16, (i16, &str, &str, usize)>::new();
    for (index, item) in baseline.iter().enumerate() {
        let canonical_shape = match item.location_kind {
            0 => {
                item.item_kind == 0
                    && item.entry_security_state == 1
                    && (0..=3).contains(&item.slot_index)
            }
            1 => {
                item.item_kind == 1
                    && item.entry_security_state == 1
                    && (0..=1).contains(&item.slot_index)
            }
            2 => {
                matches!(item.item_kind, 0 | 1)
                    && item.entry_security_state == 2
                    && (0..=7).contains(&item.slot_index)
            }
            _ => false,
        };
        if item.item_uid == [0; 16]
            || item.creation_request_id == [0; 16]
            || !(3..=96).contains(&item.template_id.len())
            || item.content_revision != CORE_ITEM_CONTENT_REVISION
            || !(0..=3).contains(&item.creation_kind)
            || !(0..=65_535).contains(&item.roll_index)
            || !(0..=65_535).contains(&item.unit_ordinal)
            || !(0..=7).contains(&item.provenance_kind)
            || !canonical_shape
            || index > 0
                && (
                    baseline[index - 1].location_kind,
                    baseline[index - 1].slot_index,
                    baseline[index - 1].item_uid,
                ) >= (item.location_kind, item.slot_index, item.item_uid)
        {
            return Err(PersistenceError::CorruptStoredDangerCrashRestore);
        }
        match item.location_kind {
            0 if !equipment_slots.insert(item.slot_index) => {
                return Err(PersistenceError::CorruptStoredDangerCrashRestore);
            }
            1 => {
                let stack = belt_stacks.entry(item.slot_index).or_insert((
                    &item.template_id,
                    &item.content_revision,
                    0,
                ));
                stack.2 += 1;
                if stack.0 != item.template_id || stack.1 != item.content_revision || stack.2 > 6 {
                    return Err(PersistenceError::CorruptStoredDangerCrashRestore);
                }
            }
            2 => {
                let stack = backpack_stacks.entry(item.slot_index).or_insert((
                    item.item_kind,
                    &item.template_id,
                    &item.content_revision,
                    0,
                ));
                stack.3 += 1;
                if stack.0 != item.item_kind
                    || stack.1 != item.template_id
                    || stack.2 != item.content_revision
                    || (item.item_kind == 0 && stack.3 > 1)
                    || (item.item_kind == 1 && stack.3 > 6)
                {
                    return Err(PersistenceError::CorruptStoredDangerCrashRestore);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn classify_active_danger_custody(item: &LiveItem) -> Result<bool, PersistenceError> {
    match (
        item.security_state,
        item.location_kind,
        item.destruction_reason.as_deref(),
    ) {
        (1, 0 | 1, None) | (2, 2 | 3, None) => Ok(true),
        (3, 4, Some("ground_expired")) | (4, 7, Some("consumed")) => Ok(false),
        (3, 4, Some("crash_revoked")) if item.proven_prior_crash_revocation => Ok(false),
        _ => Err(PersistenceError::CorruptStoredDangerCrashRestore),
    }
}

async fn apply_item_change(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
    live: &LiveItem,
    baseline: Option<&BaselineItem>,
) -> Result<DangerCrashItemChange, PersistenceError> {
    let post_item_version = increment(live.item_version)?;
    let (kind, security, location, slot, reason) = if let Some(baseline) = baseline {
        (
            DangerCrashItemChangeKind::Restored,
            if baseline.location_kind < 2 { 0 } else { 2 },
            baseline.location_kind,
            Some(baseline.slot_index),
            "crash_restored",
        )
    } else {
        (
            DangerCrashItemChangeKind::Revoked,
            3,
            4,
            None,
            "crash_revoked",
        )
    };
    let changed = sqlx::query(
        "UPDATE item_instances SET item_version=$1, security_state=$2, location_kind=$3, \
                slot_index=$4, instance_id=NULL, pickup_id=NULL, expires_at_tick=NULL, \
                destruction_reason=$5, updated_at=transaction_timestamp() \
         WHERE namespace_id=$6 AND account_id=$7 AND character_id=$8 AND item_uid=$9 \
           AND item_version=$10",
    )
    .bind(as_i64(post_item_version)?)
    .bind(security)
    .bind(location)
    .bind(slot)
    .bind(if baseline.is_some() {
        None
    } else {
        Some(reason)
    })
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(live.item_uid.as_slice())
    .bind(as_i64(live.item_version)?)
    .execute(&mut *connection)
    .await?
    .rows_affected();
    if changed != 1 {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    let ledger_event_id = request.item_ledger_event_id(live.item_uid);
    sqlx::query(
        "INSERT INTO item_ledger_events (namespace_id, ledger_event_id, item_uid, account_id, \
         character_id, mutation_id, event_kind, source_kind, pre_item_version, post_item_version, \
         pre_security_state, post_security_state, pre_location_kind, post_location_kind, reason) \
         VALUES ($1,$2,$3,$4,$5,$6,4,4,$7,$8,$9,$10,$11,$12,$13)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ledger_event_id.as_slice())
    .bind(live.item_uid.as_slice())
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(as_i64(live.item_version)?)
    .bind(as_i64(post_item_version)?)
    .bind(live.security_state)
    .bind(security)
    .bind(live.location_kind)
    .bind(location)
    .bind(reason)
    .execute(connection)
    .await?;
    Ok(DangerCrashItemChange {
        kind,
        item_uid: live.item_uid,
        ledger_event_id,
        pre_item_version: live.item_version,
        post_item_version,
        pre_security_state: live.security_state,
        post_security_state: security,
        pre_location_kind: live.location_kind,
        post_location_kind: location,
        post_slot_index: slot,
    })
}

async fn lock_materials(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
) -> Result<Vec<MaterialLock>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT material_id, quantity, material_version FROM character_run_material_stacks \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND security_state=2 \
         ORDER BY material_id COLLATE \"C\" FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            let value = MaterialLock {
                material_id: row.try_get("material_id")?,
                quantity: row.try_get("quantity")?,
                version: positive(row.try_get("material_version")?)?,
            };
            if value.quantity <= 0 || !(3..=96).contains(&value.material_id.len()) {
                return Err(PersistenceError::CorruptStoredDangerCrashRestore);
            }
            Ok(value)
        })
        .collect()
}

async fn revoke_materials(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
    materials: &[MaterialLock],
) -> Result<Vec<DangerCrashMaterialChange>, PersistenceError> {
    let mut changes = Vec::with_capacity(materials.len());
    for material in materials {
        let post_version = increment(material.version)?;
        let material_updated = sqlx::query(
            "UPDATE character_run_material_stacks SET quantity=0, material_version=$1, \
                    security_state=3, terminal_reason='crash_revoked', \
                    terminal_restore_point_id=$2, updated_at=transaction_timestamp() \
             WHERE namespace_id=$3 AND account_id=$4 AND character_id=$5 AND material_id=$6 \
               AND material_version=$7 AND security_state=2",
        )
        .bind(as_i64(post_version)?)
        .bind(request.restore_point_id.as_slice())
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(request.character_id.as_slice())
        .bind(&material.material_id)
        .bind(as_i64(material.version)?)
        .execute(&mut *connection)
        .await?
        .rows_affected();
        if material_updated != 1 {
            return Err(PersistenceError::CorruptStoredDangerCrashRestore);
        }
        changes.push(DangerCrashMaterialChange {
            material_id: material.material_id.clone(),
            pre_quantity: u32::try_from(material.quantity)
                .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?,
            pre_material_version: material.version,
            post_material_version: post_version,
        });
    }
    Ok(changes)
}

#[allow(
    clippy::too_many_lines,
    reason = "snapshot parent, canonical children, live rows, and source records share one ordered lock phase"
)]
async fn lock_oath_bargain(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
) -> Result<OathBargainLock, PersistenceError> {
    let row = sqlx::query(
        "SELECT snapshot.oath_id, snapshot.earned_bargain_slots, snapshot.active_bargain_count, \
                snapshot.component_digest, \
                snapshot.oath_bargain_version AS snapshot_version, \
                state.oath_bargain_version AS live_version \
         FROM entry_restore_oath_bargain_v3 snapshot JOIN character_oath_bargain_state state \
           USING (namespace_id, account_id, character_id) WHERE snapshot.namespace_id=$1 \
           AND snapshot.account_id=$2 AND snapshot.character_id=$3 \
           AND snapshot.restore_point_id=$4 AND snapshot.restored_oath_bargain_version IS NULL \
         FOR UPDATE OF snapshot, state",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.restore_point_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::CorruptStoredDangerCrashRestore)?;
    let active_rows = sqlx::query(
        "SELECT bargain_id, acquisition_ordinal, acquired_by_offer_id, source_reward_event_id, \
                content_version, records_blake3, assets_blake3, localization_blake3 \
         FROM entry_restore_active_bargains_v3 WHERE namespace_id=$1 AND restore_point_id=$2 \
         ORDER BY acquisition_ordinal FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.restore_point_id.as_slice())
    .fetch_all(&mut *connection)
    .await?;
    sqlx::query(
        "SELECT bargain_id FROM character_active_bargains WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3 ORDER BY acquisition_ordinal FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_all(&mut *connection)
    .await?;
    for statement in [
        "SELECT offer_id FROM bargain_offers WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND entry_restore_point_id=$4 ORDER BY offer_id FOR UPDATE",
        "SELECT source_reward_event_id FROM bargain_milestone_results WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND entry_restore_point_id=$4 ORDER BY source_reward_event_id FOR UPDATE",
    ] {
        sqlx::query(statement)
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(request.account_id.as_slice())
            .bind(request.character_id.as_slice())
            .bind(request.restore_point_id.as_slice())
            .fetch_all(&mut *connection)
            .await?;
    }
    sqlx::query(
        "SELECT decision.mutation_id FROM bargain_decision_results decision \
         JOIN bargain_offers offer USING (namespace_id, account_id, character_id, offer_id) \
         WHERE decision.namespace_id=$1 AND decision.account_id=$2 AND decision.character_id=$3 \
           AND offer.entry_restore_point_id=$4 ORDER BY decision.mutation_id FOR UPDATE OF decision",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.restore_point_id.as_slice())
    .fetch_all(connection)
    .await?;
    let active = active_rows
        .into_iter()
        .map(|row| {
            Ok(BaselineBargain {
                bargain_id: row.try_get("bargain_id")?,
                acquisition_ordinal: row.try_get("acquisition_ordinal")?,
                acquired_by_offer_id: fixed(row.try_get("acquired_by_offer_id")?)?,
                source_reward_event_id: fixed(row.try_get("source_reward_event_id")?)?,
                content_version: row.try_get("content_version")?,
                records_blake3: row.try_get("records_blake3")?,
                assets_blake3: row.try_get("assets_blake3")?,
                localization_blake3: row.try_get("localization_blake3")?,
            })
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    let earned_slots: i16 = row.try_get("earned_bargain_slots")?;
    let active_bargain_count: i16 = row.try_get("active_bargain_count")?;
    if !(0..=3).contains(&earned_slots)
        || usize::try_from(active_bargain_count).ok() != Some(active.len())
        || active.len() > usize::try_from(earned_slots).unwrap_or_default()
        || row
            .try_get::<Option<String>, _>("oath_id")?
            .as_ref()
            .is_some_and(|value| !(3..=96).contains(&value.len()))
        || active.iter().enumerate().any(|(index, bargain)| {
            usize::try_from(bargain.acquisition_ordinal).ok() != Some(index + 1)
                || !(3..=96).contains(&bargain.bargain_id.len())
                || bargain.acquired_by_offer_id == [0; 16]
                || bargain.source_reward_event_id == [0; 16]
                || !(1..=96).contains(&bargain.content_version.len())
                || !is_lower_hex_hash(&bargain.records_blake3)
                || !is_lower_hex_hash(&bargain.assets_blake3)
                || !is_lower_hex_hash(&bargain.localization_blake3)
        })
        || active
            .iter()
            .map(|bargain| &bargain.bargain_id)
            .collect::<BTreeSet<_>>()
            .len()
            != active.len()
        || active
            .iter()
            .map(|bargain| bargain.acquired_by_offer_id)
            .collect::<BTreeSet<_>>()
            .len()
            != active.len()
    {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    let snapshot_version = positive(row.try_get("snapshot_version")?)?;
    let value = OathBargainLock {
        oath_id: row.try_get("oath_id")?,
        earned_slots,
        version: positive(row.try_get("live_version")?)?,
        snapshot_version,
        active,
    };
    let digest_active = value
        .active
        .iter()
        .map(|bargain| {
            Ok(StoredDangerEntryActiveBargainV3 {
                acquisition_ordinal: u8::try_from(bargain.acquisition_ordinal)
                    .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?,
                bargain_id: bargain.bargain_id.clone(),
                acquired_by_offer_id: bargain.acquired_by_offer_id,
                source_reward_event_id: bargain.source_reward_event_id,
                content_version: bargain.content_version.clone(),
                records_blake3: bargain.records_blake3.clone(),
                assets_blake3: bargain.assets_blake3.clone(),
                localization_blake3: bargain.localization_blake3.clone(),
            })
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    let stored_digest: [u8; 32] = fixed(row.try_get("component_digest")?)?;
    if value.version < snapshot_version
        || oath_digest(
            value.oath_id.as_deref(),
            u8::try_from(value.earned_slots)
                .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?,
            snapshot_version,
            &digest_active,
        ) != stored_digest
    {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    Ok(value)
}

#[allow(
    clippy::too_many_lines,
    reason = "root-bound record revocation and exact snapshot restoration remain contiguous for auditability"
)]
async fn restore_oath_bargain(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
    snapshot: &OathBargainLock,
    post_version: u64,
) -> Result<Vec<DangerCrashBargainChange>, PersistenceError> {
    let mut changes = Vec::new();
    let offer_rows = sqlx::query(
        "UPDATE bargain_offers SET revoked_by_restore_point_id=$1, \
                revoked_at=transaction_timestamp() WHERE namespace_id=$2 AND account_id=$3 \
           AND character_id=$4 AND entry_restore_point_id=$1 \
           AND revoked_by_restore_point_id IS NULL RETURNING offer_id",
    )
    .bind(request.restore_point_id.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_all(&mut *connection)
    .await?;
    for row in offer_rows {
        changes.push(DangerCrashBargainChange {
            kind: DangerCrashBargainRecordKind::Offer,
            record_id: fixed(row.try_get("offer_id")?)?,
        });
    }
    let milestone_rows = sqlx::query(
        "UPDATE bargain_milestone_results SET revoked_by_restore_point_id=$1, \
                revoked_at=transaction_timestamp() WHERE namespace_id=$2 AND account_id=$3 \
           AND character_id=$4 AND entry_restore_point_id=$1 \
           AND revoked_by_restore_point_id IS NULL RETURNING source_reward_event_id",
    )
    .bind(request.restore_point_id.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_all(&mut *connection)
    .await?;
    for row in milestone_rows {
        changes.push(DangerCrashBargainChange {
            kind: DangerCrashBargainRecordKind::Milestone,
            record_id: fixed(row.try_get("source_reward_event_id")?)?,
        });
    }
    let decision_rows = sqlx::query(
        "UPDATE bargain_decision_results decision SET revoked_by_restore_point_id=$1, \
                revoked_at=transaction_timestamp() FROM bargain_offers offer \
         WHERE decision.namespace_id=$2 AND decision.account_id=$3 AND decision.character_id=$4 \
           AND offer.namespace_id=decision.namespace_id AND offer.account_id=decision.account_id \
           AND offer.character_id=decision.character_id AND offer.offer_id=decision.offer_id \
           AND offer.entry_restore_point_id=$1 AND decision.revoked_by_restore_point_id IS NULL \
         RETURNING decision.mutation_id",
    )
    .bind(request.restore_point_id.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_all(&mut *connection)
    .await?;
    for row in decision_rows {
        changes.push(DangerCrashBargainChange {
            kind: DangerCrashBargainRecordKind::Decision,
            record_id: fixed(row.try_get("mutation_id")?)?,
        });
    }
    changes.sort_by_key(|change| {
        (
            match change.kind {
                DangerCrashBargainRecordKind::Offer => 0_i16,
                DangerCrashBargainRecordKind::Milestone => 1_i16,
                DangerCrashBargainRecordKind::Decision => 2_i16,
            },
            change.record_id,
        )
    });

    sqlx::query(
        "DELETE FROM character_active_bargains WHERE namespace_id=$1 AND account_id=$2 \
         AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .execute(&mut *connection)
    .await?;
    for bargain in &snapshot.active {
        sqlx::query(
            "INSERT INTO character_active_bargains (namespace_id, account_id, character_id, \
             bargain_id, acquisition_ordinal, acquired_by_offer_id) VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(request.character_id.as_slice())
        .bind(&bargain.bargain_id)
        .bind(bargain.acquisition_ordinal)
        .bind(bargain.acquired_by_offer_id.as_slice())
        .execute(&mut *connection)
        .await?;
    }
    let state_changed = sqlx::query(
        "UPDATE character_oath_bargain_state SET earned_bargain_slots=$1, \
                oath_bargain_version=$2, updated_at=transaction_timestamp() \
         WHERE namespace_id=$3 AND account_id=$4 AND character_id=$5 \
           AND oath_bargain_version=$6",
    )
    .bind(snapshot.earned_slots)
    .bind(as_i64(post_version)?)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(as_i64(snapshot.version)?)
    .execute(&mut *connection)
    .await?
    .rows_affected();
    if state_changed != 1 {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    sqlx::query(
        "UPDATE characters SET oath_id=$1 WHERE namespace_id=$2 AND account_id=$3 \
         AND character_id=$4",
    )
    .bind(snapshot.oath_id.as_deref())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .execute(connection)
    .await?;
    Ok(changes)
}

async fn lock_life_metrics(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
) -> Result<LifeLock, PersistenceError> {
    let row = sqlx::query(
        "SELECT live.lifetime_ticks, live.permadeath_combat_ticks, \
                live.life_metrics_version, snapshot.life_metrics_version AS snapshot_version, \
                snapshot.component_digest, \
                snapshot.captured_lifetime_ticks, \
                snapshot.rollback_permadeath_combat_ticks \
         FROM character_life_metrics live JOIN entry_restore_life_metrics_v3 snapshot \
           USING (namespace_id, account_id, character_id) WHERE snapshot.namespace_id=$1 \
           AND snapshot.account_id=$2 AND snapshot.character_id=$3 \
           AND snapshot.restore_point_id=$4 AND snapshot.restored_life_metrics_version IS NULL \
         FOR UPDATE OF live, snapshot",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.restore_point_id.as_slice())
    .fetch_optional(connection)
    .await?
    .ok_or(PersistenceError::CorruptStoredDangerCrashRestore)?;
    let snapshot_version = positive(row.try_get("snapshot_version")?)?;
    let captured_lifetime_ticks = nonnegative(row.try_get("captured_lifetime_ticks")?)?;
    let value = LifeLock {
        lifetime_ticks: nonnegative(row.try_get("lifetime_ticks")?)?,
        permadeath_combat_ticks: nonnegative(row.try_get("permadeath_combat_ticks")?)?,
        rollback_permadeath_combat_ticks: nonnegative(
            row.try_get("rollback_permadeath_combat_ticks")?,
        )?,
        version: positive(row.try_get("life_metrics_version")?)?,
        snapshot_version,
        captured_lifetime_ticks,
    };
    let stored_digest: [u8; 32] = fixed(row.try_get("component_digest")?)?;
    if value.lifetime_ticks < captured_lifetime_ticks
        || value.permadeath_combat_ticks < value.rollback_permadeath_combat_ticks
        || value.version < snapshot_version
        || life_digest(
            captured_lifetime_ticks,
            value.rollback_permadeath_combat_ticks,
            snapshot_version,
        ) != stored_digest
    {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    Ok(value)
}

async fn restore_life_metrics(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
    life: &LifeLock,
    post_version: u64,
) -> Result<(), PersistenceError> {
    let changed = sqlx::query(
        "UPDATE character_life_metrics SET lifetime_ticks=$1, permadeath_combat_ticks=$2, \
                life_metrics_version=$3, updated_at=transaction_timestamp() \
         WHERE namespace_id=$4 AND account_id=$5 AND character_id=$6 \
           AND life_metrics_version=$7",
    )
    .bind(as_i64(life.lifetime_ticks)?)
    .bind(as_i64(life.rollback_permadeath_combat_ticks)?)
    .bind(as_i64(post_version)?)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(as_i64(life.version)?)
    .execute(connection)
    .await?
    .rows_affected();
    if changed != 1 {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "Ash snapshot, wallet, and every post-snapshot advancement are inspected under one final lock phase"
)]
async fn lock_ash_snapshot(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
) -> Result<AshLock, PersistenceError> {
    let snapshot = sqlx::query(
        "SELECT ash_wallet_version, component_digest FROM entry_restore_ash_wallet_v3 \
             WHERE namespace_id=$1 \
             AND account_id=$2 AND character_id=$3 AND restore_point_id=$4 \
             AND restored_ash_wallet_version IS NULL FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.restore_point_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::CorruptStoredDangerCrashRestore)?;
    let snapshot_version = positive(snapshot.try_get("ash_wallet_version")?)?;
    let stored_digest: [u8; 32] = fixed(snapshot.try_get("component_digest")?)?;
    if ash_digest(snapshot_version) != stored_digest {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    let wallet = sqlx::query(
        "SELECT balance, wallet_version FROM ash_wallets WHERE namespace_id=$1 \
         AND account_id=$2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::CorruptStoredDangerCrashRestore)?;
    let wallet_balance: i32 = wallet.try_get("balance")?;
    let wallet_version = positive(wallet.try_get("wallet_version")?)?;
    if wallet_version < snapshot_version || wallet_balance < 0 {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    let rows = sqlx::query(
        "SELECT mutation_id, mutation_kind, requested_amount, content_version, \
                pre_wallet_version, post_wallet_version, entry_restore_point_id, \
                reversed_by_restore_point_id \
         FROM ash_mutation_results WHERE namespace_id=$1 AND account_id=$2 \
           AND result_code=0 AND post_wallet_version > $3 \
         ORDER BY post_wallet_version, mutation_id FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(as_i64(snapshot_version)?)
    .fetch_all(&mut *connection)
    .await?;
    let mut earns = Vec::with_capacity(rows.len());
    let mut total = 0_i32;
    for (index, row) in rows.into_iter().enumerate() {
        let kind: i16 = row.try_get("mutation_kind")?;
        let entry_restore_point_id = optional_fixed(row.try_get("entry_restore_point_id")?)?;
        let reversed: Option<Vec<u8>> = row.try_get("reversed_by_restore_point_id")?;
        validate_ash_advancement(
            request.restore_point_id,
            snapshot_version,
            index,
            kind,
            entry_restore_point_id,
            reversed.is_some(),
            positive(row.try_get("pre_wallet_version")?)?,
            positive(row.try_get("post_wallet_version")?)?,
        )?;
        let amount: i32 = row.try_get("requested_amount")?;
        total = total
            .checked_add(amount)
            .ok_or(PersistenceError::DangerCrashRestoreAmbiguousAsh)?;
        earns.push(AshEarn {
            mutation_id: fixed(row.try_get("mutation_id")?)?,
            amount,
            content_version: row.try_get("content_version")?,
        });
    }
    let expected_wallet_version = snapshot_version
        .checked_add(
            u64::try_from(earns.len())
                .map_err(|_| PersistenceError::DangerCrashRestoreAmbiguousAsh)?,
        )
        .ok_or(PersistenceError::DangerCrashRestoreAmbiguousAsh)?;
    if wallet_version != expected_wallet_version {
        return Err(PersistenceError::DangerCrashRestoreAmbiguousAsh);
    }
    if total > wallet_balance {
        return Err(PersistenceError::DangerCrashRestoreAmbiguousAsh);
    }
    Ok(AshLock {
        snapshot_version,
        wallet_version,
        wallet_balance,
        earns,
    })
}

async fn compensate_ash(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
    ash: &AshLock,
) -> Result<Vec<DangerCrashAshChange>, PersistenceError> {
    let mut changes = Vec::with_capacity(ash.earns.len());
    let mut remaining_balance = ash.wallet_balance;
    for earn in &ash.earns {
        let compensation_id = request.ash_compensation_mutation_id(earn.mutation_id);
        let expected_version = i64::try_from(
            changes
                .last()
                .map_or(ash.wallet_version, |change: &DangerCrashAshChange| {
                    change.post_wallet_version
                }),
        )
        .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?;
        let mut mutation = AshMutationRequest {
            account_id: request.account_id,
            mutation_id: compensation_id,
            payload_hash: [0; 32],
            expected_wallet_version: expected_version,
            kind: AshMutationKind::Spend,
            amount: earn.amount,
            reason_code: "danger_crash_restore".into(),
            source_id: "danger_crash_restore".into(),
            content_version: earn.content_version.clone(),
            entry_restore_point_id: Some(request.restore_point_id),
        };
        mutation.payload_hash = mutation.expected_payload_hash();
        let result = apply_ash_mutation_on_connection(connection, &mutation).await?;
        let stored = result.result();
        if !matches!(result, AshWalletTransaction::Committed(_))
            || stored.code != AshMutationCode::Accepted
        {
            return Err(PersistenceError::DangerCrashRestoreAmbiguousAsh);
        }
        let reversed = sqlx::query(
            "UPDATE ash_mutation_results SET reversed_by_restore_point_id=$1, \
                    reversed_by_mutation_id=$2, reversed_at=transaction_timestamp() \
             WHERE namespace_id=$3 AND account_id=$4 AND mutation_id=$5 \
               AND entry_restore_point_id=$1 AND result_code=0 AND mutation_kind=0 \
               AND reversed_by_restore_point_id IS NULL",
        )
        .bind(request.restore_point_id.as_slice())
        .bind(compensation_id.as_slice())
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(earn.mutation_id.as_slice())
        .execute(&mut *connection)
        .await?
        .rows_affected();
        if reversed != 1 {
            return Err(PersistenceError::DangerCrashRestoreAmbiguousAsh);
        }
        remaining_balance = remaining_balance
            .checked_sub(earn.amount)
            .ok_or(PersistenceError::DangerCrashRestoreAmbiguousAsh)?;
        if stored.after_balance != remaining_balance {
            return Err(PersistenceError::DangerCrashRestoreAmbiguousAsh);
        }
        changes.push(DangerCrashAshChange {
            original_mutation_id: earn.mutation_id,
            compensation_mutation_id: compensation_id,
            amount: u32::try_from(earn.amount)
                .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?,
            pre_wallet_version: positive(stored.pre_wallet_version)?,
            post_wallet_version: positive(stored.post_wallet_version)?,
        });
    }
    Ok(changes)
}

#[allow(
    clippy::too_many_arguments,
    reason = "every accepted Ash ledger axis is authoritative"
)]
fn validate_ash_advancement(
    restore_point_id: [u8; 16],
    snapshot_version: u64,
    index: usize,
    kind: i16,
    entry_restore_point_id: Option<[u8; 16]>,
    reversed: bool,
    pre_wallet_version: u64,
    post_wallet_version: u64,
) -> Result<(), PersistenceError> {
    let expected_post_version = snapshot_version
        .checked_add(
            u64::try_from(index + 1)
                .map_err(|_| PersistenceError::DangerCrashRestoreAmbiguousAsh)?,
        )
        .ok_or(PersistenceError::DangerCrashRestoreAmbiguousAsh)?;
    if kind != 0
        || entry_restore_point_id != Some(restore_point_id)
        || reversed
        || pre_wallet_version.checked_add(1) != Some(expected_post_version)
        || post_wallet_version != expected_post_version
    {
        return Err(PersistenceError::DangerCrashRestoreAmbiguousAsh);
    }
    Ok(())
}

async fn lock_ash_version(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
) -> Result<u64, PersistenceError> {
    positive(
        sqlx::query_scalar::<_, i64>(
            "SELECT wallet_version FROM ash_wallets WHERE namespace_id=$1 AND account_id=$2 \
             FOR UPDATE",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .fetch_optional(connection)
        .await?
        .ok_or(PersistenceError::CorruptStoredDangerCrashRestore)?,
    )
}

async fn return_to_hall(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
    root: &RootLock,
    post_account_version: u64,
    post_character_version: u64,
) -> Result<(), PersistenceError> {
    let account_changed = sqlx::query(
        "UPDATE accounts SET state_version=$1, updated_at=transaction_timestamp() \
         WHERE namespace_id=$2 AND account_id=$3",
    )
    .bind(as_i64(post_account_version)?)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .execute(&mut *connection)
    .await?
    .rows_affected();
    let character_changed = sqlx::query(
        "UPDATE characters SET character_state_version=$1, updated_at=transaction_timestamp() \
         WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4",
    )
    .bind(as_i64(post_character_version)?)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .execute(&mut *connection)
    .await?
    .rows_affected();
    let world_changed = sqlx::query(
        "UPDATE character_world_locations SET character_version=$1, location_kind=1, \
                location_content_id=$2, safe_arrival_kind=0, safe_spawn_id=NULL, \
                instance_lineage_id=NULL, entry_restore_point_id=NULL, \
                updated_at=transaction_timestamp() WHERE namespace_id=$3 AND account_id=$4 \
           AND character_id=$5",
    )
    .bind(as_i64(post_character_version)?)
    .bind(HALL_ID)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .execute(&mut *connection)
    .await?
    .rows_affected();
    let lineage_changed = sqlx::query(
        "UPDATE character_instance_lineages SET lineage_state=$1, \
                closed_at=transaction_timestamp() WHERE namespace_id=$2 AND account_id=$3 \
           AND character_id=$4 AND lineage_id=$5 AND lineage_state IN (0,1)",
    )
    .bind(LINEAGE_CRASH_FAILED)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(root.lineage_id.as_slice())
    .execute(connection)
    .await?
    .rows_affected();
    if account_changed != 1 || character_changed != 1 || world_changed != 1 || lineage_changed != 1
    {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "the five mandatory component completions share one root"
)]
async fn set_restored_component_versions(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
    progression: u64,
    inventory: u64,
    oath_bargain: u64,
    life_metrics: u64,
    ash: u64,
) -> Result<(), PersistenceError> {
    for (statement, version) in [
        (
            "UPDATE entry_restore_progression_v3 SET restored_progression_version=$1 \
             WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4 \
               AND restore_point_id=$5 AND restored_progression_version IS NULL",
            progression,
        ),
        (
            "UPDATE entry_restore_inventory_v3 SET restored_inventory_version=$1 \
             WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4 \
               AND restore_point_id=$5 AND restored_inventory_version IS NULL",
            inventory,
        ),
        (
            "UPDATE entry_restore_oath_bargain_v3 SET restored_oath_bargain_version=$1 \
             WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4 \
               AND restore_point_id=$5 AND restored_oath_bargain_version IS NULL",
            oath_bargain,
        ),
        (
            "UPDATE entry_restore_life_metrics_v3 SET restored_life_metrics_version=$1 \
             WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4 \
               AND restore_point_id=$5 AND restored_life_metrics_version IS NULL",
            life_metrics,
        ),
        (
            "UPDATE entry_restore_ash_wallet_v3 SET restored_ash_wallet_version=$1 \
             WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4 \
               AND restore_point_id=$5 AND restored_ash_wallet_version IS NULL",
            ash,
        ),
    ] {
        let changed = sqlx::query(statement)
            .bind(as_i64(version)?)
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(request.account_id.as_slice())
            .bind(request.character_id.as_slice())
            .bind(request.restore_point_id.as_slice())
            .execute(&mut *connection)
            .await?
            .rows_affected();
        if changed != 1 {
            return Err(PersistenceError::CorruptStoredDangerCrashRestore);
        }
    }
    Ok(())
}

async fn consume_root(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
) -> Result<(), PersistenceError> {
    let changed = sqlx::query(
        "UPDATE character_entry_restore_points SET restore_state=4, \
                crash_restore_mutation_id=$1, consumed_at=transaction_timestamp() \
         WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4 \
           AND restore_point_id=$5 AND restore_state=0",
    )
    .bind(request.mutation_id.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.restore_point_id.as_slice())
    .execute(connection)
    .await?
    .rows_affected();
    if changed != 1 {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "the exact server V3 postcard material is assembled in declaration order for parity audit"
)]
fn validate_root_snapshot(
    root: &RootLock,
    character_id: [u8; 16],
    progression: &ProgressionLock,
    inventory: &InventoryLock,
    oath_bargain: &OathBargainLock,
    life: &LifeLock,
    ash_snapshot_version: u64,
) -> Result<(), PersistenceError> {
    if root.progression_version != progression.snapshot_version
        || root.inventory_version != inventory.snapshot_version
        || root.oath_bargain_version != oath_bargain.snapshot_version
        || root.life_metrics_version != life.snapshot_version
        || root.ash_wallet_version != ash_snapshot_version
    {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    let baseline_items = inventory
        .baseline
        .iter()
        .map(|item| {
            Ok(DangerEntryInventoryItemDigestV3 {
                item_uid: item.item_uid,
                template_id: item.template_id.clone(),
                content_revision: item.content_revision.clone(),
                creation_kind: u8::try_from(item.creation_kind)
                    .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?,
                creation_request_id: item.creation_request_id,
                roll_index: u16::try_from(item.roll_index)
                    .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?,
                unit_ordinal: u16::try_from(item.unit_ordinal)
                    .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?,
                provenance_kind: u8::try_from(item.provenance_kind)
                    .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?,
                location: match item.location_kind {
                    0 => DangerEntryInventoryLocationDigestV3::Equipment,
                    1 => DangerEntryInventoryLocationDigestV3::Belt,
                    2 => DangerEntryInventoryLocationDigestV3::RunBackpack,
                    _ => return Err(PersistenceError::CorruptStoredDangerCrashRestore),
                },
                slot_index: u8::try_from(item.slot_index)
                    .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?,
                item_version: item.entry_item_version,
                security: match item.entry_security_state {
                    1 => DangerEntryInventorySecurityDigestV3::AtRiskEquipped,
                    2 => DangerEntryInventorySecurityDigestV3::AtRiskPending,
                    _ => return Err(PersistenceError::CorruptStoredDangerCrashRestore),
                },
            })
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    let active_bargains = oath_bargain
        .active
        .iter()
        .map(|bargain| {
            Ok(DangerEntryActiveBargainDigestV3 {
                acquisition_ordinal: u8::try_from(bargain.acquisition_ordinal)
                    .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?,
                bargain_id: bargain.bargain_id.clone(),
                acquired_by_offer_id: bargain.acquired_by_offer_id,
                source_reward_event_id: bargain.source_reward_event_id,
                content_version: bargain.content_version.clone(),
                content_revision: DangerEntryContentRevisionDigestV3 {
                    records_blake3: bargain.records_blake3.clone(),
                    assets_blake3: bargain.assets_blake3.clone(),
                    localization_blake3: bargain.localization_blake3.clone(),
                },
            })
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    let snapshot = DangerEntrySnapshotDigestV3 {
        character_id,
        content_revision: DangerEntryContentRevisionDigestV3 {
            records_blake3: root.records_blake3.clone(),
            assets_blake3: root.assets_blake3.clone(),
            localization_blake3: root.localization_blake3.clone(),
        },
        progression: DangerEntryProgressionDigestV3 {
            level: u16::try_from(progression.level)
                .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?,
            xp: u32::try_from(progression.total_xp)
                .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?,
            current_health: u32::try_from(progression.current_health)
                .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?,
            progression_version: progression.snapshot_version,
        },
        inventory: DangerEntryInventoryDigestV3 {
            baseline_items,
            pre_inventory_version: inventory.pre_snapshot_version,
            inventory_version: inventory.snapshot_version,
            safe_placement_count: inventory.safe_placement_count,
        },
        oath_bargains: DangerEntryOathDigestV3 {
            oath_id: oath_bargain.oath_id.clone(),
            active_bargains,
            earned_bargain_slots: u8::try_from(oath_bargain.earned_slots)
                .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?,
            oath_bargain_version: oath_bargain.snapshot_version,
        },
        life_metrics: DangerEntryLifeDigestV3 {
            lifetime_ticks: life.captured_lifetime_ticks,
            permadeath_combat_ticks: life.rollback_permadeath_combat_ticks,
            life_metrics_version: life.snapshot_version,
        },
        ash_wallet: DangerEntryAshDigestV3 {
            ash_wallet_version: ash_snapshot_version,
        },
        versions: DangerEntryVersionsDigestV3 {
            account_version: root.account_version,
            character_version: root.character_version,
            progression_version: root.progression_version,
            inventory_version: root.inventory_version,
            oath_bargain_version: root.oath_bargain_version,
            life_metrics_version: root.life_metrics_version,
            ash_wallet_version: root.ash_wallet_version,
        },
    };
    if snapshot.composite_digest()? != root.composite_digest {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    Ok(())
}

async fn load_request_result(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
) -> Result<Option<StoredRequestRow>, PersistenceError> {
    let row = sqlx::query(
        "SELECT account_id, character_id, restore_point_id, mutation_id, request_hash, \
                outcome_code, observed_restore_state, committed_mutation_id, \
                result_payload, result_digest \
         FROM danger_crash_restore_request_results WHERE namespace_id=$1 AND account_id=$2 \
           AND mutation_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .fetch_optional(connection)
    .await?;
    row.map(|row| {
        Ok(StoredRequestRow {
            account_id: fixed(row.try_get("account_id")?)?,
            character_id: fixed(row.try_get("character_id")?)?,
            restore_point_id: fixed(row.try_get("restore_point_id")?)?,
            mutation_id: fixed(row.try_get("mutation_id")?)?,
            request_hash: fixed(row.try_get("request_hash")?)?,
            outcome_code: DangerCrashRestoreCode::from_code(row.try_get("outcome_code")?)
                .ok_or(PersistenceError::CorruptStoredDangerCrashRestore)?,
            observed_restore_state: row.try_get("observed_restore_state")?,
            committed_mutation_id: optional_fixed(row.try_get("committed_mutation_id")?)?,
            payload: row.try_get("result_payload")?,
            digest: fixed(row.try_get("result_digest")?)?,
        })
    })
    .transpose()
}

fn decode_stored_receipt(
    stored: &StoredRequestRow,
) -> Result<DangerCrashRestoreReceipt, PersistenceError> {
    let receipt: DangerCrashRestoreReceipt = postcard::from_bytes(&stored.payload)
        .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?;
    receipt.validate()?;
    if receipt.account_id != stored.account_id
        || receipt.character_id != stored.character_id
        || receipt.restore_point_id != stored.restore_point_id
        || receipt.request_mutation_id != stored.mutation_id
        || receipt.request_hash != stored.request_hash
        || receipt.code != stored.outcome_code
        || receipt.code.restore_state() != stored.observed_restore_state
        || receipt.committed_mutation_id != stored.committed_mutation_id
        || receipt.digest() != stored.digest
        || receipt.payload()? != stored.payload
    {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    Ok(receipt)
}

async fn insert_conflict_audit(
    connection: &mut PgConnection,
    request: &DangerCrashRestoreRequest,
    stored: &StoredRequestRow,
    audit_id: [u8; 16],
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO danger_crash_restore_conflict_audits \
         (namespace_id, account_id, character_id, restore_point_id, mutation_id, \
          stored_request_hash, attempted_request_hash, audit_id) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT \
         (namespace_id, account_id, mutation_id, attempted_request_hash) DO NOTHING",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(stored.character_id.as_slice())
    .bind(stored.restore_point_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(stored.request_hash.as_slice())
    .bind(request.request_hash.as_slice())
    .bind(audit_id.as_slice())
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_request_result(
    connection: &mut PgConnection,
    receipt: &DangerCrashRestoreReceipt,
) -> Result<(), PersistenceError> {
    let payload = receipt.payload()?;
    if payload.len() > 65_536 {
        return Err(PersistenceError::CorruptStoredDangerCrashRestore);
    }
    let committed = receipt
        .committed_mutation_id
        .as_ref()
        .map(<[u8; 16]>::as_slice);
    sqlx::query(
        "INSERT INTO danger_crash_restore_request_results \
         (namespace_id, account_id, character_id, restore_point_id, mutation_id, request_hash, \
          outcome_code, observed_restore_state, committed_mutation_id, result_payload, result_digest) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(receipt.account_id.as_slice())
    .bind(receipt.character_id.as_slice())
    .bind(receipt.restore_point_id.as_slice())
    .bind(receipt.request_mutation_id.as_slice())
    .bind(receipt.request_hash.as_slice())
    .bind(receipt.code as i16)
    .bind(receipt.code.restore_state())
    .bind(committed)
    .bind(payload)
    .bind(receipt.digest().as_slice())
    .execute(connection)
    .await?;
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "the normalized parent and four canonically ordered child sets are one durable receipt graph"
)]
async fn insert_normalized_result(
    connection: &mut PgConnection,
    receipt: &DangerCrashRestoreReceipt,
) -> Result<(), PersistenceError> {
    let versions = receipt
        .versions
        .as_ref()
        .ok_or(PersistenceError::CorruptStoredDangerCrashRestore)?;
    let restored_count = receipt
        .item_changes
        .iter()
        .filter(|change| change.kind == DangerCrashItemChangeKind::Restored)
        .count();
    let revoked_count = receipt.item_changes.len() - restored_count;
    sqlx::query(
        "INSERT INTO danger_crash_restore_results \
         (namespace_id, account_id, character_id, restore_point_id, mutation_id, request_hash, \
          result_code, post_account_version, post_character_version, post_progression_version, \
          post_inventory_version, post_oath_bargain_version, post_life_metrics_version, \
          post_ash_wallet_version, restored_item_count, revoked_item_count, \
          revoked_material_count, revoked_bargain_record_count, compensated_ash_count, result_digest) \
         VALUES ($1,$2,$3,$4,$5,$6,0,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(receipt.account_id.as_slice())
    .bind(receipt.character_id.as_slice())
    .bind(receipt.restore_point_id.as_slice())
    .bind(receipt.request_mutation_id.as_slice())
    .bind(receipt.request_hash.as_slice())
    .bind(as_i64(versions.account)?)
    .bind(as_i64(versions.character)?)
    .bind(as_i64(versions.progression)?)
    .bind(as_i64(versions.inventory)?)
    .bind(as_i64(versions.oath_bargain)?)
    .bind(as_i64(versions.life_metrics)?)
    .bind(as_i64(versions.ash_wallet)?)
    .bind(i32::try_from(restored_count).map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?)
    .bind(i32::try_from(revoked_count).map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?)
    .bind(i32::try_from(receipt.material_changes.len()).map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?)
    .bind(i32::try_from(receipt.bargain_changes.len()).map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?)
    .bind(i32::try_from(receipt.ash_changes.len()).map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?)
    .bind(receipt.digest().as_slice())
    .execute(&mut *connection)
    .await?;

    for (ordinal, change) in receipt.item_changes.iter().enumerate() {
        let kind = match change.kind {
            DangerCrashItemChangeKind::Restored => 0_i16,
            DangerCrashItemChangeKind::Revoked => 1_i16,
        };
        sqlx::query(
            "INSERT INTO danger_crash_restore_item_changes \
             (namespace_id, account_id, character_id, restore_point_id, mutation_id, \
              change_ordinal, change_kind, item_uid, ledger_event_id, ledger_event_kind, \
              ledger_source_kind, ledger_reason, pre_item_version, post_item_version, \
              pre_security_state, post_security_state, pre_location_kind, post_location_kind, \
              post_slot_index) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,4,4,$10,$11,$12,$13,$14,$15,$16,$17)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(receipt.account_id.as_slice())
        .bind(receipt.character_id.as_slice())
        .bind(receipt.restore_point_id.as_slice())
        .bind(receipt.request_mutation_id.as_slice())
        .bind(i32::try_from(ordinal).map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?)
        .bind(kind)
        .bind(change.item_uid.as_slice())
        .bind(change.ledger_event_id.as_slice())
        .bind(if kind == 0 { "crash_restored" } else { "crash_revoked" })
        .bind(as_i64(change.pre_item_version)?)
        .bind(as_i64(change.post_item_version)?)
        .bind(change.pre_security_state)
        .bind(change.post_security_state)
        .bind(change.pre_location_kind)
        .bind(change.post_location_kind)
        .bind(change.post_slot_index)
        .execute(&mut *connection)
        .await?;
    }
    for (ordinal, change) in receipt.material_changes.iter().enumerate() {
        sqlx::query(
            "INSERT INTO danger_crash_restore_material_changes \
             (namespace_id, account_id, character_id, restore_point_id, mutation_id, \
              change_ordinal, material_id, pre_quantity, pre_material_version, post_material_version) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(receipt.account_id.as_slice())
        .bind(receipt.character_id.as_slice())
        .bind(receipt.restore_point_id.as_slice())
        .bind(receipt.request_mutation_id.as_slice())
        .bind(i32::try_from(ordinal).map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?)
        .bind(&change.material_id)
        .bind(i32::try_from(change.pre_quantity).map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?)
        .bind(as_i64(change.pre_material_version)?)
        .bind(as_i64(change.post_material_version)?)
        .execute(&mut *connection)
        .await?;
    }
    for (ordinal, change) in receipt.bargain_changes.iter().enumerate() {
        let kind = match change.kind {
            DangerCrashBargainRecordKind::Offer => 0_i16,
            DangerCrashBargainRecordKind::Milestone => 1_i16,
            DangerCrashBargainRecordKind::Decision => 2_i16,
        };
        sqlx::query(
            "INSERT INTO danger_crash_restore_bargain_changes \
             (namespace_id, account_id, character_id, restore_point_id, mutation_id, \
              change_ordinal, record_kind, record_id) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(receipt.account_id.as_slice())
        .bind(receipt.character_id.as_slice())
        .bind(receipt.restore_point_id.as_slice())
        .bind(receipt.request_mutation_id.as_slice())
        .bind(
            i32::try_from(ordinal)
                .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?,
        )
        .bind(kind)
        .bind(change.record_id.as_slice())
        .execute(&mut *connection)
        .await?;
    }
    for (ordinal, change) in receipt.ash_changes.iter().enumerate() {
        sqlx::query(
            "INSERT INTO danger_crash_restore_ash_changes \
             (namespace_id, account_id, character_id, restore_point_id, mutation_id, \
              change_ordinal, original_ash_mutation_id, compensation_ash_mutation_id, amount, \
              pre_wallet_version, post_wallet_version) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(receipt.account_id.as_slice())
        .bind(receipt.character_id.as_slice())
        .bind(receipt.restore_point_id.as_slice())
        .bind(receipt.request_mutation_id.as_slice())
        .bind(
            i32::try_from(ordinal)
                .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?,
        )
        .bind(change.original_mutation_id.as_slice())
        .bind(change.compensation_mutation_id.as_slice())
        .bind(
            i32::try_from(change.amount)
                .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?,
        )
        .bind(as_i64(change.pre_wallet_version)?)
        .bind(as_i64(change.post_wallet_version)?)
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> DangerCrashRestoreRequest {
        let mut request = DangerCrashRestoreRequest {
            account_id: [1; 16],
            character_id: [2; 16],
            restore_point_id: [3; 16],
            mutation_id: [4; 16],
            request_hash: [0; 32],
        };
        request.request_hash = request.expected_request_hash();
        request
    }

    fn root_lock(state: i16, crash_mutation: Option<[u8; 16]>) -> RootLock {
        RootLock {
            lineage_id: [5; 16],
            restore_location_id: HALL_ID.into(),
            restore_state: state,
            crash_restore_mutation_id: crash_mutation,
            account_version: 1,
            character_version: 1,
            progression_version: 1,
            inventory_version: 1,
            oath_bargain_version: 1,
            life_metrics_version: 1,
            ash_wallet_version: 1,
            records_blake3: "a".repeat(64),
            assets_blake3: "b".repeat(64),
            localization_blake3: "c".repeat(64),
            composite_digest: [1; 32],
        }
    }

    #[test]
    fn terminal_precedence_maps_append_only_root_states() {
        let request = request();
        for (state, code) in [
            (1, DangerCrashRestoreCode::ExtractionCommitted),
            (2, DangerCrashRestoreCode::DeathCommitted),
            (3, DangerCrashRestoreCode::RecallCommitted),
        ] {
            let receipt = terminal_receipt(&request, &root_lock(state, None)).unwrap();
            assert_eq!(receipt.code, code);
            assert!(receipt.committed_mutation_id.is_none());
        }
        let committed = [9; 16];
        let receipt = terminal_receipt(&request, &root_lock(4, Some(committed))).unwrap();
        assert_eq!(receipt.code, DangerCrashRestoreCode::AlreadyCrashRestored);
        assert_eq!(receipt.committed_mutation_id, Some(committed));
    }

    #[test]
    fn baseline_identity_validation_is_exact_and_location_independent() {
        let baseline = BaselineItem {
            item_uid: [1; 16],
            template_id: "item.test".into(),
            content_revision: "core-dev.blake3.test".into(),
            item_kind: 0,
            creation_kind: 1,
            creation_request_id: [2; 16],
            roll_index: 3,
            unit_ordinal: 4,
            provenance_kind: 1,
            location_kind: 0,
            slot_index: 1,
            entry_item_version: 7,
            entry_security_state: 1,
        };
        let mut live = LiveItem {
            item_uid: baseline.item_uid,
            template_id: baseline.template_id.clone(),
            content_revision: baseline.content_revision.clone(),
            item_kind: baseline.item_kind,
            creation_kind: baseline.creation_kind,
            creation_request_id: baseline.creation_request_id,
            roll_index: baseline.roll_index,
            unit_ordinal: baseline.unit_ordinal,
            provenance_kind: baseline.provenance_kind,
            item_version: 8,
            security_state: 2,
            location_kind: 3,
            destruction_reason: None,
            proven_prior_crash_revocation: false,
        };
        assert!(validate_baseline_identity(&baseline, &live).is_ok());
        live.item_version = baseline.entry_item_version - 1;
        assert!(validate_baseline_identity(&baseline, &live).is_err());
        live.item_version = baseline.entry_item_version;
        live.provenance_kind += 1;
        assert!(validate_baseline_identity(&baseline, &live).is_err());
    }

    #[test]
    fn nonbaseline_custody_revokes_only_pending_danger_and_rejects_safe_split_authority() {
        let mut item = LiveItem {
            item_uid: [1; 16],
            template_id: "item.test".into(),
            content_revision: "core-dev.blake3.test".into(),
            item_kind: 0,
            creation_kind: 1,
            creation_request_id: [2; 16],
            roll_index: 0,
            unit_ordinal: 0,
            provenance_kind: 1,
            item_version: 1,
            security_state: 2,
            location_kind: 3,
            destruction_reason: None,
            proven_prior_crash_revocation: false,
        };
        assert!(classify_active_danger_custody(&item).unwrap());
        item.security_state = 3;
        item.location_kind = 4;
        item.destruction_reason = Some("ground_expired".into());
        assert!(!classify_active_danger_custody(&item).unwrap());
        item.security_state = 0;
        item.location_kind = 5;
        item.destruction_reason = None;
        assert!(classify_active_danger_custody(&item).is_err());
        item.security_state = 3;
        item.location_kind = 4;
        item.destruction_reason = Some("crash_revoked".into());
        assert!(classify_active_danger_custody(&item).is_err());
        item.proven_prior_crash_revocation = true;
        assert!(!classify_active_danger_custody(&item).unwrap());
    }

    #[test]
    fn ash_advancement_requires_contiguous_unreversed_root_earns() {
        let root = [3; 16];
        assert!(validate_ash_advancement(root, 5, 0, 0, Some(root), false, 5, 6).is_ok());
        assert!(validate_ash_advancement(root, 5, 0, 0, Some([9; 16]), false, 5, 6).is_err());
        assert!(validate_ash_advancement(root, 5, 0, 1, Some(root), false, 5, 6).is_err());
        assert!(validate_ash_advancement(root, 5, 1, 0, Some(root), false, 5, 6).is_err());
    }

    #[test]
    fn stored_receipt_requires_exact_payload_digest_and_binding() {
        let request = request();
        let receipt = terminal_receipt(&request, &root_lock(2, None)).unwrap();
        let mut stored = StoredRequestRow {
            account_id: request.account_id,
            character_id: request.character_id,
            restore_point_id: request.restore_point_id,
            mutation_id: request.mutation_id,
            request_hash: request.request_hash,
            outcome_code: receipt.code,
            observed_restore_state: receipt.code.restore_state(),
            committed_mutation_id: receipt.committed_mutation_id,
            payload: receipt.payload().unwrap(),
            digest: receipt.digest(),
        };
        assert_eq!(decode_stored_receipt(&stored).unwrap(), receipt);
        stored.digest[0] ^= 1;
        assert!(decode_stored_receipt(&stored).is_err());
    }
}
