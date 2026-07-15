//! Single-writer `PostgreSQL` repository for one complete durable permadeath graph.
//!
//! The lock and write order is part of the persistence contract. Account authority is locked
//! first, followed by character/root/world authority, aggregate components, at-risk custody,
//! deeds, and account Echo state. All mutable terminal state is finalized before the immutable
//! `death_events` root is inserted; migrations 0037 and 0039 then close and seal the normalized
//! graph, including Oath/Bargain receipts and terminal item custody.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use sqlx::{PgConnection, Row};

use crate::{
    AuthoritativeDeathPlanV1, DurableDamageTypeV1, DurableDeathCauseV1,
    DurableDeathCommitRequestV1, DurableDeathContentAuthorityV1, DurableDestructionEntryV1,
    DurableDestructionLocationV1, DurableEchoEnvelopeV1, DurableEchoOutcomeV1, DurableEchoStateV1,
    DurableEquipmentSlotV1, DurableNetworkStateV1, DurableRecallStateV1,
    DurableSummaryProjectionEntryV1, DurableSummaryProjectionKindV1, PersistenceError,
    PersistenceTransaction, PostgresPersistence, StoredCommittedDeathResultV1,
    WIPEABLE_CORE_NAMESPACE,
    bargain_cleanup::{
        BargainLifeCleanupCommand, BargainLifeEndReason, cleanup_bargains_for_life_end,
    },
    derive_durable_death_bargain_cleanup_event_id,
};

const MAX_TRANSACTION_ATTEMPTS: u8 = 3;
const DEATH_CONFLICT_AUDIT_ID_CONTEXT: &str = "gravebound.death.conflict-audit-id.v1";
const DEATH_CONFLICT_AUDIT_DIGEST_CONTEXT: &str = "gravebound.death.conflict-audit.v1";
const DEATH_ACCEPTED_AUDIT_ID_CONTEXT: &str = "gravebound.death.accepted-audit-id.v1";
const DEATH_OUTBOX_ID_CONTEXT: &str = "gravebound.death.outbox-id.v1";

const LIFE_STATE_LIVING: i16 = 0;
const LIFE_STATE_DEAD: i16 = 1;
const WORLD_LOCATION_DANGER: i16 = 2;
const LINEAGE_ACTIVE: i16 = 1;
const LINEAGE_DEATH_FAILED: i16 = 3;
const RESTORE_ACTIVE: i16 = 0;
const RESTORE_DEATH_COMMITTED: i16 = 2;
const SECURITY_AT_RISK_EQUIPPED: i16 = 1;
const SECURITY_AT_RISK_PENDING: i16 = 2;
const SECURITY_DESTROYED: i16 = 3;
const LOCATION_DESTROYED: i16 = 4;

/// Whether the exact committed result was created by this call or loaded after a retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DurableDeathTransactionV1 {
    Fresh(StoredCommittedDeathResultV1),
    Replayed(StoredCommittedDeathResultV1),
}

impl DurableDeathTransactionV1 {
    pub const fn result(&self) -> &StoredCommittedDeathResultV1 {
        match self {
            Self::Fresh(result) | Self::Replayed(result) => result,
        }
    }

    pub const fn is_replay(&self) -> bool {
        matches!(self, Self::Replayed(_))
    }
}

#[derive(Debug)]
struct AccountLock {
    version: u64,
    selected_character_id: Option<[u8; 16]>,
}

#[derive(Debug)]
struct CharacterLock {
    roster_ordinal: Option<u8>,
    class_id: String,
    level: u8,
    oath_id: Option<String>,
    life_state: i16,
    version: u64,
}

#[derive(Debug)]
struct OathBargainLock {
    version: u64,
}

#[derive(Debug)]
struct LockedAggregateComponents<'a> {
    account: &'a AccountLock,
    character: &'a CharacterLock,
    progression: &'a ProgressionLock,
    inventory_version: u64,
    oath_bargain: &'a OathBargainLock,
    life: &'a LifeLock,
}

#[derive(Debug)]
struct RootLock {
    lineage_id: [u8; 16],
    restore_state: i16,
    account_version: u64,
    character_version: u64,
    progression_version: u64,
    inventory_version: u64,
    oath_bargain_version: u64,
    life_metrics_version: u64,
    records_blake3: String,
    assets_blake3: String,
    localization_blake3: String,
}

#[derive(Debug)]
struct ProgressionLock {
    level: u8,
    current_health: i32,
    version: u64,
}

#[derive(Debug)]
struct LifeLock {
    lifetime_ticks: u64,
    permadeath_combat_ticks: u64,
    version: u64,
    entry_lifetime_ticks: u64,
    entry_permadeath_combat_ticks: u64,
    entry_version: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TerminalRuntimePrestate {
    durable_health: i32,
    durable_lifetime_ticks: u64,
    durable_combat_ticks: u64,
    durable_life_version: u64,
    entry_lifetime_ticks: u64,
    entry_combat_ticks: u64,
    entry_life_version: u64,
    root_entry_life_version: u64,
    terminal_lifetime_ticks: u64,
    terminal_combat_ticks: u64,
    expected_pre_life_version: u64,
}

impl TerminalRuntimePrestate {
    const fn stored_history_valid(self) -> bool {
        self.durable_health > 0
            && self.entry_life_version == self.root_entry_life_version
            && self.entry_life_version <= self.durable_life_version
            && self.entry_combat_ticks <= self.entry_lifetime_ticks
            && self.entry_lifetime_ticks <= self.durable_lifetime_ticks
            && self.entry_combat_ticks <= self.durable_combat_ticks
            && self.durable_combat_ticks <= self.durable_lifetime_ticks
    }

    const fn request_is_monotonic(self) -> bool {
        self.durable_life_version == self.expected_pre_life_version
            && self.durable_lifetime_ticks <= self.terminal_lifetime_ticks
            && self.durable_combat_ticks <= self.terminal_combat_ticks
            && self.terminal_combat_ticks <= self.terminal_lifetime_ticks
    }
}

#[derive(Debug, Clone)]
struct ItemLock {
    item_uid: [u8; 16],
    template_id: String,
    content_revision: String,
    item_version: u64,
    security_state: i16,
    location_kind: i16,
    slot_index: Option<i16>,
    instance_id: Option<[u8; 16]>,
    pickup_id: Option<[u8; 16]>,
}

#[derive(Debug)]
struct MaterialLock {
    material_id: String,
    quantity: u32,
    version: u64,
}

#[derive(Debug)]
struct EchoLock {
    echo_id: [u8; 16],
    death_id: [u8; 16],
    state: i16,
    tail_ordinal: u16,
}

#[derive(Debug)]
struct StoredResultRow {
    account_id: [u8; 16],
    character_id: [u8; 16],
    mutation_id: [u8; 16],
    death_id: [u8; 16],
    contract: String,
    request_hash: [u8; 32],
    result_code: i16,
    payload: Vec<u8>,
    digest: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ItemLocationBinding {
    location_kind: i16,
    slot_index: Option<i16>,
    instance_id: Option<[u8; 16]>,
    pickup_id: Option<[u8; 16]>,
    expected_security: i16,
}

impl ItemLocationBinding {
    fn from_location(location: &DurableDestructionLocationV1) -> Self {
        match location {
            DurableDestructionLocationV1::Equipment { slot } => Self {
                location_kind: 0,
                slot_index: Some(equipment_slot(*slot)),
                instance_id: None,
                pickup_id: None,
                expected_security: SECURITY_AT_RISK_EQUIPPED,
            },
            DurableDestructionLocationV1::Belt { index } => Self {
                location_kind: 1,
                slot_index: Some(i16::from(*index)),
                instance_id: None,
                pickup_id: None,
                expected_security: SECURITY_AT_RISK_EQUIPPED,
            },
            DurableDestructionLocationV1::RunBackpack { index } => Self {
                location_kind: 2,
                slot_index: Some(i16::from(*index)),
                instance_id: None,
                pickup_id: None,
                expected_security: SECURITY_AT_RISK_PENDING,
            },
            DurableDestructionLocationV1::PersonalGround {
                instance_id,
                pickup_id,
            } => Self {
                location_kind: 3,
                slot_index: None,
                instance_id: Some(*instance_id),
                pickup_id: Some(*pickup_id),
                expected_security: SECURITY_AT_RISK_PENDING,
            },
        }
    }
}

impl PostgresPersistence {
    /// Commits one complete death graph, or returns the exact stored result after response loss.
    pub async fn transact_durable_death(
        &self,
        request: &DurableDeathCommitRequestV1,
        content: &DurableDeathContentAuthorityV1,
    ) -> Result<DurableDeathTransactionV1, PersistenceError> {
        content.validate()?;
        request.validate()?;
        if !content.matches_event(&request.plan.event) {
            return Err(PersistenceError::DurableDeathContentMismatch);
        }
        let event = &request.plan.event;
        if event.bargain_cleanup_event_id
            != derive_durable_death_bargain_cleanup_event_id(event.death_id, event.mutation_id)
        {
            return Err(PersistenceError::DurableDeathBindingMismatch);
        }
        for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self.transact_durable_death_once(request, content).await {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS
                        && crate::is_retryable_transaction_failure(&error) => {}
                result => return result,
            }
        }
        unreachable!("bounded durable-death transaction loop always returns")
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the audited single-writer lock and publication order is intentionally contiguous"
    )]
    async fn transact_durable_death_once(
        &self,
        request: &DurableDeathCommitRequestV1,
        content: &DurableDeathContentAuthorityV1,
    ) -> Result<DurableDeathTransactionV1, PersistenceError> {
        let mut transaction = self.begin_transaction().await?;
        let account = lock_account(transaction.connection(), request.plan.event.account_id).await?;

        if let Some(stored) = load_result_by_mutation(transaction.connection(), request).await? {
            return finish_replay_or_conflict(transaction, request, stored).await;
        }
        if let Some(stored) =
            load_result_by_final_identity(transaction.connection(), request).await?
        {
            return finish_conflict(transaction, request, &stored).await;
        }

        let committed_at_unix_ms = transaction_timestamp_ms(transaction.connection()).await?;
        let mut committed_request = request.clone();
        committed_request.bind_commit_time(committed_at_unix_ms)?;
        let plan = &committed_request.plan;
        let event = &plan.event;

        let character = lock_character(transaction.connection(), event).await?;
        let root = lock_root(transaction.connection(), event).await?;
        validate_account_character_root(&account, &character, &root, plan, content)?;
        lock_and_validate_world(transaction.connection(), plan, &character, content).await?;
        let progression = lock_progression(transaction.connection(), event).await?;
        let inventory_version = lock_inventory(transaction.connection(), event).await?;
        let items = lock_at_risk_items(transaction.connection(), event).await?;
        let materials = lock_at_risk_materials(transaction.connection(), event).await?;
        let oath_bargain = lock_oath_bargain(transaction.connection(), event).await?;
        let life = lock_life(transaction.connection(), event).await?;
        validate_components(
            &LockedAggregateComponents {
                account: &account,
                character: &character,
                progression: &progression,
                inventory_version,
                oath_bargain: &oath_bargain,
                life: &life,
            },
            &root,
            plan,
        )?;
        clear_terminal_danger_checkpoint(transaction.connection(), event).await?;
        validate_destruction_sources(
            &items,
            &materials,
            &plan.destruction,
            plan.echo.as_ref(),
            content,
        )?;
        validate_deeds(transaction.connection(), plan).await?;
        let echoes = lock_echoes(transaction.connection(), event.account_id).await?;
        validate_echo_prestate(&echoes, plan)?;
        cleanup_and_validate_bargains(&mut transaction, plan, oath_bargain.version).await?;

        finalize_aggregate_heads(transaction.connection(), plan, &progression, &life).await?;
        destroy_items(transaction.connection(), plan).await?;
        destroy_materials(transaction.connection(), plan).await?;
        insert_death_event(transaction.connection(), &committed_request).await?;
        finalize_character_identity(transaction.connection(), plan).await?;
        insert_trace(transaction.connection(), plan).await?;
        insert_summary(transaction.connection(), plan).await?;
        insert_memorial(transaction.connection(), plan).await?;
        insert_destruction(transaction.connection(), plan).await?;
        write_echo_projection(transaction.connection(), plan).await?;

        let result = StoredCommittedDeathResultV1::from_request(&committed_request)?;
        insert_result(transaction.connection(), &committed_request, &result).await?;
        insert_accepted_audit(transaction.connection(), &committed_request, &result).await?;
        insert_outbox(transaction.connection(), plan, &result).await?;
        force_deferred_constraints(transaction.connection()).await?;
        transaction.commit().await?;
        Ok(DurableDeathTransactionV1::Fresh(result))
    }
}

/// Removes the opaque live Bell Debt checkpoint before the immutable death root is published.
/// A checkpoint from another lineage/content authority is corruption, not state that terminal
/// resolution may silently discard. Any later error rolls this deletion back with the death.
async fn clear_terminal_danger_checkpoint(
    connection: &mut PgConnection,
    event: &crate::DurableDeathEventV1,
) -> Result<(), PersistenceError> {
    let deleted = sqlx::query(
        "DELETE FROM character_danger_checkpoints \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 \
         RETURNING lineage_id,records_blake3,assets_blake3,localization_blake3",
    )
    .bind(&event.namespace_id)
    .bind(event.account_id.as_slice())
    .bind(event.character_id.as_slice())
    .fetch_optional(connection)
    .await?;
    if let Some(row) = deleted
        && (exact_id(row.try_get("lineage_id")?)? != event.lineage_id
            || row.try_get::<String, _>("records_blake3")? != event.records_blake3
            || row.try_get::<String, _>("assets_blake3")? != event.assets_blake3
            || row.try_get::<String, _>("localization_blake3")? != event.localization_blake3)
    {
        return Err(PersistenceError::DurableDeathBindingMismatch);
    }
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

async fn lock_account(
    connection: &mut PgConnection,
    account_id: [u8; 16],
) -> Result<AccountLock, PersistenceError> {
    let row = sqlx::query(
        "SELECT state_version, selected_character_id FROM accounts \
         WHERE namespace_id=$1 AND account_id=$2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_optional(connection)
    .await?
    .ok_or(PersistenceError::DurableDeathOwnerNotFound)?;
    Ok(AccountLock {
        version: positive(row.try_get("state_version")?)?,
        selected_character_id: optional_id(row.try_get("selected_character_id")?)?,
    })
}

async fn lock_character(
    connection: &mut PgConnection,
    event: &crate::DurableDeathEventV1,
) -> Result<CharacterLock, PersistenceError> {
    let row = sqlx::query(
        "SELECT roster_ordinal, class_id, level::smallint AS level, oath_id, life_state, character_state_version \
         FROM characters WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(&event.namespace_id)
    .bind(event.account_id.as_slice())
    .bind(event.character_id.as_slice())
    .fetch_optional(connection)
    .await?
    .ok_or(PersistenceError::DurableDeathOwnerNotFound)?;
    let roster_ordinal = row
        .try_get::<Option<i16>, _>("roster_ordinal")?
        .map(u8_value)
        .transpose()?;
    Ok(CharacterLock {
        roster_ordinal,
        class_id: row.try_get("class_id")?,
        level: u8_value(row.try_get("level")?)?,
        oath_id: row.try_get("oath_id")?,
        life_state: row.try_get("life_state")?,
        version: positive(row.try_get("character_state_version")?)?,
    })
}

async fn lock_root(
    connection: &mut PgConnection,
    event: &crate::DurableDeathEventV1,
) -> Result<RootLock, PersistenceError> {
    let row = sqlx::query(
        "SELECT lineage_id, restore_state, account_version, character_version, \
                progression_version, inventory_version, oath_bargain_version, \
                life_metrics_version, \
                records_blake3, assets_blake3, localization_blake3 \
         FROM character_entry_restore_points WHERE namespace_id=$1 AND account_id=$2 \
           AND character_id=$3 AND restore_point_id=$4 FOR UPDATE",
    )
    .bind(&event.namespace_id)
    .bind(event.account_id.as_slice())
    .bind(event.character_id.as_slice())
    .bind(event.restore_point_id.as_slice())
    .fetch_optional(connection)
    .await?
    .ok_or(PersistenceError::DurableDeathBindingMismatch)?;
    Ok(RootLock {
        lineage_id: exact_id(row.try_get("lineage_id")?)?,
        restore_state: row.try_get("restore_state")?,
        account_version: positive(row.try_get("account_version")?)?,
        character_version: positive(row.try_get("character_version")?)?,
        progression_version: positive(row.try_get("progression_version")?)?,
        inventory_version: positive(row.try_get("inventory_version")?)?,
        oath_bargain_version: positive(row.try_get("oath_bargain_version")?)?,
        life_metrics_version: positive(row.try_get("life_metrics_version")?)?,
        records_blake3: row.try_get("records_blake3")?,
        assets_blake3: row.try_get("assets_blake3")?,
        localization_blake3: row.try_get("localization_blake3")?,
    })
}

fn validate_account_character_root(
    account: &AccountLock,
    character: &CharacterLock,
    root: &RootLock,
    plan: &AuthoritativeDeathPlanV1,
    content: &DurableDeathContentAuthorityV1,
) -> Result<(), PersistenceError> {
    let event = &plan.event;
    if !matches!(character.life_state, LIFE_STATE_LIVING | LIFE_STATE_DEAD) {
        return Err(PersistenceError::CorruptStoredDurableDeath);
    }
    if character.life_state != LIFE_STATE_LIVING || root.restore_state != RESTORE_ACTIVE {
        return Err(PersistenceError::DurableDeathTerminalSuperseded);
    }
    if account.selected_character_id != Some(event.character_id)
        || character.roster_ordinal != Some(event.former_roster_ordinal)
        || character.class_id != plan.summary.class_id
        || character.level != plan.summary.level
        || character.oath_id != plan.summary.oath_id
        || root.lineage_id != event.lineage_id
    {
        return Err(PersistenceError::DurableDeathBindingMismatch);
    }
    if root.records_blake3 != content.records_blake3
        || root.assets_blake3 != content.assets_blake3
        || root.localization_blake3 != content.localization_blake3
    {
        return Err(PersistenceError::DurableDeathContentMismatch);
    }
    if root.account_version > account.version || root.character_version > character.version {
        return Err(PersistenceError::CorruptStoredDurableDeath);
    }
    Ok(())
}

async fn lock_and_validate_world(
    connection: &mut PgConnection,
    plan: &AuthoritativeDeathPlanV1,
    character: &CharacterLock,
    content: &DurableDeathContentAuthorityV1,
) -> Result<(), PersistenceError> {
    let event = &plan.event;
    let row = sqlx::query(
        "SELECT world.character_version, world.location_kind, world.instance_lineage_id, \
                world.entry_restore_point_id, lineage.lineage_state, lineage.records_blake3, \
                lineage.assets_blake3, lineage.localization_blake3 \
         FROM character_world_locations AS world \
         JOIN character_instance_lineages AS lineage \
           ON lineage.namespace_id=world.namespace_id AND lineage.account_id=world.account_id \
          AND lineage.character_id=world.character_id \
          AND lineage.lineage_id=world.instance_lineage_id \
         WHERE world.namespace_id=$1 AND world.account_id=$2 AND world.character_id=$3 \
         FOR UPDATE OF world, lineage",
    )
    .bind(&event.namespace_id)
    .bind(event.account_id.as_slice())
    .bind(event.character_id.as_slice())
    .fetch_optional(connection)
    .await?
    .ok_or(PersistenceError::DurableDeathBindingMismatch)?;
    let lineage_state: i16 = row.try_get("lineage_state")?;
    if matches!(lineage_state, 2 | 3) {
        return Err(PersistenceError::DurableDeathTerminalSuperseded);
    }
    if positive(row.try_get("character_version")?)? != character.version {
        return Err(PersistenceError::CorruptStoredDurableDeath);
    }
    if row.try_get::<i16, _>("location_kind")? != WORLD_LOCATION_DANGER
        || optional_id(row.try_get("instance_lineage_id")?)? != Some(event.lineage_id)
        || optional_id(row.try_get("entry_restore_point_id")?)? != Some(event.restore_point_id)
        || lineage_state != LINEAGE_ACTIVE
    {
        return Err(PersistenceError::DurableDeathBindingMismatch);
    }
    if row.try_get::<String, _>("records_blake3")? != content.records_blake3
        || row.try_get::<String, _>("assets_blake3")? != content.assets_blake3
        || row.try_get::<String, _>("localization_blake3")? != content.localization_blake3
    {
        return Err(PersistenceError::DurableDeathContentMismatch);
    }
    Ok(())
}

async fn lock_progression(
    connection: &mut PgConnection,
    event: &crate::DurableDeathEventV1,
) -> Result<ProgressionLock, PersistenceError> {
    let row = sqlx::query(
        "SELECT level, current_health, progression_version FROM character_progression \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(&event.namespace_id)
    .bind(event.account_id.as_slice())
    .bind(event.character_id.as_slice())
    .fetch_optional(connection)
    .await?
    .ok_or(PersistenceError::CorruptStoredDurableDeath)?;
    Ok(ProgressionLock {
        level: u8_value(row.try_get("level")?)?,
        current_health: row.try_get("current_health")?,
        version: positive(row.try_get("progression_version")?)?,
    })
}

async fn lock_inventory(
    connection: &mut PgConnection,
    event: &crate::DurableDeathEventV1,
) -> Result<u64, PersistenceError> {
    let value: Option<i64> = sqlx::query_scalar(
        "SELECT inventory_version FROM character_inventories WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(&event.namespace_id)
    .bind(event.account_id.as_slice())
    .bind(event.character_id.as_slice())
    .fetch_optional(connection)
    .await?;
    positive(value.ok_or(PersistenceError::CorruptStoredDurableDeath)?)
}

async fn lock_oath_bargain(
    connection: &mut PgConnection,
    event: &crate::DurableDeathEventV1,
) -> Result<OathBargainLock, PersistenceError> {
    let value: Option<i64> = sqlx::query_scalar(
        "SELECT oath_bargain_version FROM character_oath_bargain_state \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(&event.namespace_id)
    .bind(event.account_id.as_slice())
    .bind(event.character_id.as_slice())
    .fetch_optional(connection)
    .await?;
    Ok(OathBargainLock {
        version: positive(value.ok_or(PersistenceError::CorruptStoredDurableDeath)?)?,
    })
}

async fn lock_life(
    connection: &mut PgConnection,
    event: &crate::DurableDeathEventV1,
) -> Result<LifeLock, PersistenceError> {
    let row = sqlx::query(
        "SELECT live.lifetime_ticks, live.permadeath_combat_ticks, live.life_metrics_version, \
                entry.captured_lifetime_ticks, entry.rollback_permadeath_combat_ticks, \
                entry.life_metrics_version AS entry_life_metrics_version \
         FROM character_life_metrics AS live \
         JOIN entry_restore_life_metrics_v3 AS entry \
           ON entry.namespace_id=live.namespace_id AND entry.account_id=live.account_id \
          AND entry.character_id=live.character_id AND entry.restore_point_id=$4 \
         WHERE live.namespace_id=$1 AND live.account_id=$2 AND live.character_id=$3 \
         FOR UPDATE OF live, entry",
    )
    .bind(&event.namespace_id)
    .bind(event.account_id.as_slice())
    .bind(event.character_id.as_slice())
    .bind(event.restore_point_id.as_slice())
    .fetch_optional(connection)
    .await?
    .ok_or(PersistenceError::CorruptStoredDurableDeath)?;
    Ok(LifeLock {
        lifetime_ticks: nonnegative(row.try_get("lifetime_ticks")?)?,
        permadeath_combat_ticks: nonnegative(row.try_get("permadeath_combat_ticks")?)?,
        version: positive(row.try_get("life_metrics_version")?)?,
        entry_lifetime_ticks: nonnegative(row.try_get("captured_lifetime_ticks")?)?,
        entry_permadeath_combat_ticks: nonnegative(
            row.try_get("rollback_permadeath_combat_ticks")?,
        )?,
        entry_version: positive(row.try_get("entry_life_metrics_version")?)?,
    })
}

fn validate_components(
    components: &LockedAggregateComponents<'_>,
    root: &RootLock,
    plan: &AuthoritativeDeathPlanV1,
) -> Result<(), PersistenceError> {
    let event = &plan.event;
    let current_versions = (
        components.account.version,
        components.character.version,
        components.progression.version,
        components.inventory_version,
        components.oath_bargain.version,
        components.life.version,
    );
    if current_versions
        != (
            event.versions.account.pre,
            event.versions.character.pre,
            event.versions.progression.pre,
            event.versions.inventory.pre,
            event.versions.oath_bargain.pre,
            event.versions.life_metrics.pre,
        )
    {
        return Err(PersistenceError::DurableDeathVersionMismatch {
            account: components.account.version,
            character: components.character.version,
            progression: components.progression.version,
            inventory: components.inventory_version,
            oath_bargain: components.oath_bargain.version,
            life_metrics: components.life.version,
        });
    }
    if root.account_version > components.account.version
        || root.character_version > components.character.version
        || root.progression_version > components.progression.version
        || root.inventory_version > components.inventory_version
        || root.oath_bargain_version > components.oath_bargain.version
        || root.life_metrics_version > components.life.version
    {
        return Err(PersistenceError::CorruptStoredDurableDeath);
    }
    if components.progression.level != plan.summary.level {
        return Err(PersistenceError::DurableDeathBindingMismatch);
    }
    let runtime = terminal_runtime_prestate(components.progression, components.life, root, event);
    if !runtime.stored_history_valid() {
        return Err(PersistenceError::CorruptStoredDurableDeath);
    }
    if !runtime.request_is_monotonic() {
        return Err(PersistenceError::DurableDeathBindingMismatch);
    }
    Ok(())
}

fn terminal_runtime_prestate(
    progression: &ProgressionLock,
    life: &LifeLock,
    root: &RootLock,
    event: &crate::DurableDeathEventV1,
) -> TerminalRuntimePrestate {
    TerminalRuntimePrestate {
        durable_health: progression.current_health,
        durable_lifetime_ticks: life.lifetime_ticks,
        durable_combat_ticks: life.permadeath_combat_ticks,
        durable_life_version: life.version,
        entry_lifetime_ticks: life.entry_lifetime_ticks,
        entry_combat_ticks: life.entry_permadeath_combat_ticks,
        entry_life_version: life.entry_version,
        root_entry_life_version: root.life_metrics_version,
        terminal_lifetime_ticks: event.lifetime_ticks,
        terminal_combat_ticks: event.permadeath_combat_ticks,
        expected_pre_life_version: event.versions.life_metrics.pre,
    }
}

async fn lock_at_risk_items(
    connection: &mut PgConnection,
    event: &crate::DurableDeathEventV1,
) -> Result<BTreeMap<[u8; 16], ItemLock>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT item_uid, template_id, content_revision, item_version, security_state, \
                location_kind, slot_index, instance_id, pickup_id \
         FROM item_instances WHERE namespace_id=$1 \
           AND account_id=$2 AND character_id=$3 AND security_state IN (1,2) \
         ORDER BY item_uid FOR UPDATE",
    )
    .bind(&event.namespace_id)
    .bind(event.account_id.as_slice())
    .bind(event.character_id.as_slice())
    .fetch_all(connection)
    .await?;
    let mut items = BTreeMap::new();
    for row in rows {
        let item = ItemLock {
            item_uid: exact_id(row.try_get("item_uid")?)?,
            template_id: row.try_get("template_id")?,
            content_revision: row.try_get("content_revision")?,
            item_version: positive(row.try_get("item_version")?)?,
            security_state: row.try_get("security_state")?,
            location_kind: row.try_get("location_kind")?,
            slot_index: row.try_get("slot_index")?,
            instance_id: optional_id(row.try_get("instance_id")?)?,
            pickup_id: optional_id(row.try_get("pickup_id")?)?,
        };
        let item_id = item.item_uid;
        if items.insert(item_id, item).is_some() {
            return Err(PersistenceError::CorruptStoredDurableDeath);
        }
    }
    Ok(items)
}

async fn lock_at_risk_materials(
    connection: &mut PgConnection,
    event: &crate::DurableDeathEventV1,
) -> Result<BTreeMap<String, MaterialLock>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT material_id, quantity, material_version FROM character_run_material_stacks \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 \
           AND security_state=2 AND quantity>0 ORDER BY material_id COLLATE \"C\" FOR UPDATE",
    )
    .bind(&event.namespace_id)
    .bind(event.account_id.as_slice())
    .bind(event.character_id.as_slice())
    .fetch_all(connection)
    .await?;
    let mut materials = BTreeMap::new();
    for row in rows {
        let material = MaterialLock {
            material_id: row.try_get("material_id")?,
            quantity: u32_value(row.try_get("quantity")?)?,
            version: positive(row.try_get("material_version")?)?,
        };
        let key = material.material_id.clone();
        if materials.insert(key, material).is_some() {
            return Err(PersistenceError::CorruptStoredDurableDeath);
        }
    }
    Ok(materials)
}

async fn cleanup_and_validate_bargains(
    transaction: &mut PersistenceTransaction<'_>,
    plan: &AuthoritativeDeathPlanV1,
    locked_version: u64,
) -> Result<(), PersistenceError> {
    let event = &plan.event;
    let derived_event_id =
        derive_durable_death_bargain_cleanup_event_id(event.death_id, event.mutation_id);
    if event.bargain_cleanup_event_id != derived_event_id {
        return Err(PersistenceError::DurableDeathBindingMismatch);
    }
    if locked_version != event.versions.oath_bargain.pre {
        return Err(PersistenceError::DurableDeathVersionMismatch {
            account: event.versions.account.pre,
            character: event.versions.character.pre,
            progression: event.versions.progression.pre,
            inventory: event.versions.inventory.pre,
            oath_bargain: locked_version,
            life_metrics: event.versions.life_metrics.pre,
        });
    }
    let pre_version = i64_value(event.versions.oath_bargain.pre)?;
    let post_version = i64_value(event.versions.oath_bargain.post)?;
    let result = cleanup_bargains_for_life_end(
        transaction,
        &BargainLifeCleanupCommand {
            account_id: event.account_id,
            character_id: event.character_id,
            event_id: event.bargain_cleanup_event_id,
            reason: BargainLifeEndReason::Death,
            expected_oath_bargain_version: pre_version,
        },
    )
    .await
    .map_err(|error| match error {
        PersistenceError::BargainCharacterNotFound
        | PersistenceError::BargainCleanupVersionMismatch
        | PersistenceError::CorruptBargainCleanup => PersistenceError::CorruptStoredDurableDeath,
        other => other,
    })?;
    if result.active_bargains.len() != plan.summary.bargains.len()
        || result
            .active_bargains
            .iter()
            .zip(&plan.summary.bargains)
            .any(|(stored, snapshot)| stored.bargain_id != snapshot.content_id)
    {
        return Err(PersistenceError::DurableDeathBindingMismatch);
    }
    if result.pre_oath_bargain_version != pre_version
        || result.post_oath_bargain_version != post_version
    {
        return Err(PersistenceError::CorruptStoredDurableDeath);
    }
    let stored_post_version: Option<i64> = sqlx::query_scalar(
        "SELECT oath_bargain_version FROM character_oath_bargain_state \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(&event.namespace_id)
    .bind(event.account_id.as_slice())
    .bind(event.character_id.as_slice())
    .fetch_optional(transaction.connection())
    .await?;
    let active_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM character_active_bargains WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3",
    )
    .bind(&event.namespace_id)
    .bind(event.account_id.as_slice())
    .bind(event.character_id.as_slice())
    .fetch_one(transaction.connection())
    .await?;
    let receipt = sqlx::query(
        "SELECT event_type, aggregate_version, event_payload FROM character_life_outbox \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND event_id=$4",
    )
    .bind(&event.namespace_id)
    .bind(event.account_id.as_slice())
    .bind(event.character_id.as_slice())
    .bind(event.bargain_cleanup_event_id.as_slice())
    .fetch_optional(transaction.connection())
    .await?;
    let receipt_valid = receipt.is_some_and(|row| {
        row.try_get::<String, _>("event_type").ok().as_deref() == Some("bargains_cleared_death")
            && row.try_get::<i64, _>("aggregate_version").ok() == Some(post_version)
            && row.try_get::<Vec<u8>, _>("event_payload").ok().as_deref()
                == Some(result.event_payload.as_slice())
    });
    if stored_post_version != Some(post_version) || active_count != 0 || !receipt_valid {
        return Err(PersistenceError::CorruptStoredDurableDeath);
    }
    Ok(())
}

fn validate_destruction_sources(
    items: &BTreeMap<[u8; 16], ItemLock>,
    materials: &BTreeMap<String, MaterialLock>,
    destruction: &[DurableDestructionEntryV1],
    echo: Option<&DurableEchoEnvelopeV1>,
    content: &DurableDeathContentAuthorityV1,
) -> Result<(), PersistenceError> {
    let (weapon_signature_tag, relic_signature_tag) = expected_echo_signatures(items, content)?;
    if echo.is_some_and(|envelope| {
        envelope.created.weapon_signature_tag != weapon_signature_tag
            || envelope.created.relic_signature_tag != relic_signature_tag
    }) {
        return Err(PersistenceError::DurableDeathContentMismatch);
    }

    let mut expected_items = BTreeSet::new();
    let mut expected_materials = BTreeSet::new();
    for entry in destruction {
        match entry {
            DurableDestructionEntryV1::Item {
                content_id,
                item_uid,
                location,
                pre_item_version,
                ..
            } => {
                let binding = ItemLocationBinding::from_location(location);
                let item = items
                    .get(item_uid)
                    .ok_or(PersistenceError::DurableDeathBindingMismatch)?;
                if item.template_id != *content_id
                    || item.item_version != *pre_item_version
                    || item.security_state != binding.expected_security
                    || item.location_kind != binding.location_kind
                    || item.slot_index != binding.slot_index
                    || item.instance_id != binding.instance_id
                    || item.pickup_id != binding.pickup_id
                    || !expected_items.insert(*item_uid)
                {
                    return Err(PersistenceError::DurableDeathBindingMismatch);
                }
            }
            DurableDestructionEntryV1::RunMaterial {
                material_id,
                destroyed_quantity,
                pre_material_version,
                ..
            } => {
                let material = materials
                    .get(material_id)
                    .ok_or(PersistenceError::DurableDeathBindingMismatch)?;
                if material.quantity != *destroyed_quantity
                    || material.version != *pre_material_version
                    || !expected_materials.insert(material_id.clone())
                {
                    return Err(PersistenceError::DurableDeathBindingMismatch);
                }
            }
        }
    }
    if expected_items.len() != items.len() || expected_materials.len() != materials.len() {
        return Err(PersistenceError::DurableDeathBindingMismatch);
    }
    Ok(())
}

fn expected_echo_signatures(
    items: &BTreeMap<[u8; 16], ItemLock>,
    content: &DurableDeathContentAuthorityV1,
) -> Result<(Option<String>, Option<String>), PersistenceError> {
    let mut weapon_signature_tag = None;
    let mut relic_signature_tag = None;
    let mut weapon_seen = false;
    let mut relic_seen = false;
    for item in items.values() {
        let authority = content
            .item(&item.template_id)
            .ok_or(PersistenceError::DurableDeathContentMismatch)?;
        if item.content_revision != content.content_revision {
            return Err(PersistenceError::DurableDeathContentMismatch);
        }
        if item.location_kind == 0 {
            match item.slot_index {
                Some(0) if !weapon_seen => {
                    weapon_seen = true;
                    weapon_signature_tag.clone_from(&authority.echo_signature_tag);
                }
                Some(1) if !relic_seen => {
                    relic_seen = true;
                    relic_signature_tag.clone_from(&authority.echo_signature_tag);
                }
                Some(0 | 1) => return Err(PersistenceError::CorruptStoredDurableDeath),
                _ => {}
            }
        }
    }
    Ok((weapon_signature_tag, relic_signature_tag))
}

async fn validate_deeds(
    connection: &mut PgConnection,
    plan: &AuthoritativeDeathPlanV1,
) -> Result<(), PersistenceError> {
    let event = &plan.event;
    let rows = sqlx::query(
        "SELECT deed_id, deed_kind, achieved_tick FROM character_life_deeds \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 \
           AND achieved_tick <= $4 AND content_revision=$5 \
         ORDER BY achieved_tick DESC, deed_id COLLATE \"C\" DESC FOR UPDATE",
    )
    .bind(&event.namespace_id)
    .bind(event.account_id.as_slice())
    .bind(event.character_id.as_slice())
    .bind(i64_value(event.death_tick)?)
    .bind(&event.content_revision)
    .fetch_all(connection)
    .await?;
    let mut boss_count = 0_usize;
    let mut major_events = BTreeSet::new();
    let mut deed_ids = BTreeSet::new();
    let mut latest = None;
    for row in rows {
        let deed_id: String = row.try_get("deed_id")?;
        let deed_kind: i16 = row.try_get("deed_kind")?;
        if latest.is_none() {
            latest = Some(deed_id.clone());
        }
        match deed_kind {
            0 => boss_count += 1,
            1 => {
                major_events.insert(deed_id.clone());
            }
            _ => return Err(PersistenceError::CorruptStoredDurableDeath),
        }
        deed_ids.insert(deed_id);
    }
    let expected_final_deed = latest.as_deref().unwrap_or("deed.none");
    let eligible = plan.summary.level == 10
        && event.permadeath_combat_ticks >= 18_000
        && (boss_count > 0 || major_events.len() >= 2);
    if plan.summary.final_deed_id != expected_final_deed || eligible != plan.echo.is_some() {
        return Err(PersistenceError::DurableDeathBindingMismatch);
    }
    if let Some(echo) = &plan.echo
        && (echo.created.deed_tags.is_empty()
            || echo
                .created
                .deed_tags
                .iter()
                .any(|tag| !deed_ids.contains(&tag.content_id)))
    {
        return Err(PersistenceError::DurableDeathBindingMismatch);
    }
    Ok(())
}

async fn lock_echoes(
    connection: &mut PgConnection,
    account_id: [u8; 16],
) -> Result<Vec<EchoLock>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT echo.echo_id, echo.death_id, echo.state, \
                COALESCE((SELECT max(transition.transition_ordinal) \
                          FROM echo_state_transitions AS transition \
                          WHERE transition.namespace_id=echo.namespace_id \
                            AND transition.echo_id=echo.echo_id), -1) AS tail_ordinal \
         FROM echo_records AS echo WHERE echo.namespace_id=$1 AND echo.account_id=$2 \
         ORDER BY echo.created_at, echo.echo_id FOR UPDATE OF echo",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            let tail: i16 = row.try_get("tail_ordinal")?;
            if tail < 0 {
                return Err(PersistenceError::CorruptStoredDurableDeath);
            }
            Ok(EchoLock {
                echo_id: exact_id(row.try_get("echo_id")?)?,
                death_id: exact_id(row.try_get("death_id")?)?,
                state: row.try_get("state")?,
                tail_ordinal: u16::try_from(tail)
                    .map_err(|_| PersistenceError::CorruptStoredDurableDeath)?,
            })
        })
        .collect()
}

fn validate_echo_prestate(
    echoes: &[EchoLock],
    plan: &AuthoritativeDeathPlanV1,
) -> Result<(), PersistenceError> {
    let available: Vec<_> = echoes.iter().filter(|echo| echo.state == 1).collect();
    if available.len() > 1 || echoes.iter().any(|echo| !(0..=4).contains(&echo.state)) {
        return Err(PersistenceError::CorruptStoredDurableDeath);
    }
    let Some(envelope) = &plan.echo else {
        return Ok(());
    };
    match (
        envelope.preexisting_available_echo_id,
        envelope.promotion.as_ref(),
    ) {
        (Some(expected_available), None) => {
            if available.len() != 1 || available[0].echo_id != expected_available {
                return Err(PersistenceError::DurableDeathBindingMismatch);
            }
        }
        (None, Some(promotion)) => {
            if !available.is_empty() {
                return Err(PersistenceError::DurableDeathBindingMismatch);
            }
            let oldest_existing = echoes.iter().find(|echo| echo.state == 0);
            match oldest_existing {
                Some(oldest)
                    if promotion.echo_id == oldest.echo_id
                        && promotion.echo_death_id == oldest.death_id
                        && promotion.ordinal == oldest.tail_ordinal.saturating_add(1) => {}
                None if promotion.echo_id == envelope.created.echo_id
                    && promotion.echo_death_id == envelope.created.death_id
                    && promotion.ordinal == 1 => {}
                _ => return Err(PersistenceError::DurableDeathBindingMismatch),
            }
        }
        _ => return Err(PersistenceError::DurableDeathBindingMismatch),
    }
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "the terminal aggregate update order is kept contiguous for persistence audit"
)]
async fn finalize_aggregate_heads(
    connection: &mut PgConnection,
    plan: &AuthoritativeDeathPlanV1,
    progression: &ProgressionLock,
    life: &LifeLock,
) -> Result<(), PersistenceError> {
    let event = &plan.event;
    expect_one(
        sqlx::query(
            "UPDATE accounts SET state_version=$1, selected_character_id=NULL, \
                    updated_at=transaction_timestamp() WHERE namespace_id=$2 AND account_id=$3 \
                  AND state_version=$4 AND selected_character_id=$5",
        )
        .bind(i64_value(event.versions.account.post)?)
        .bind(&event.namespace_id)
        .bind(event.account_id.as_slice())
        .bind(i64_value(event.versions.account.pre)?)
        .bind(event.character_id.as_slice())
        .execute(&mut *connection)
        .await?
        .rows_affected(),
    )?;
    expect_one(
        sqlx::query(
            "UPDATE character_world_locations SET character_version=$1, \
                    updated_at=transaction_timestamp() WHERE namespace_id=$2 AND account_id=$3 \
                  AND character_id=$4 AND character_version=$5 AND location_kind=$6 \
                  AND instance_lineage_id=$7 AND entry_restore_point_id=$8",
        )
        .bind(i64_value(event.versions.character.post)?)
        .bind(&event.namespace_id)
        .bind(event.account_id.as_slice())
        .bind(event.character_id.as_slice())
        .bind(i64_value(event.versions.character.pre)?)
        .bind(WORLD_LOCATION_DANGER)
        .bind(event.lineage_id.as_slice())
        .bind(event.restore_point_id.as_slice())
        .execute(&mut *connection)
        .await?
        .rows_affected(),
    )?;
    expect_one(
        sqlx::query(
            "UPDATE character_progression SET current_health=0, progression_version=$1, \
                    updated_at=transaction_timestamp() WHERE namespace_id=$2 AND account_id=$3 \
                  AND character_id=$4 AND progression_version=$5 AND current_health=$6 \
                  AND current_health>0",
        )
        .bind(i64_value(event.versions.progression.post)?)
        .bind(&event.namespace_id)
        .bind(event.account_id.as_slice())
        .bind(event.character_id.as_slice())
        .bind(i64_value(event.versions.progression.pre)?)
        .bind(progression.current_health)
        .execute(&mut *connection)
        .await?
        .rows_affected(),
    )?;
    expect_one(
        sqlx::query(
            "UPDATE character_inventories SET inventory_version=$1, \
                    updated_at=transaction_timestamp() WHERE namespace_id=$2 AND account_id=$3 \
                  AND character_id=$4 AND inventory_version=$5",
        )
        .bind(i64_value(event.versions.inventory.post)?)
        .bind(&event.namespace_id)
        .bind(event.account_id.as_slice())
        .bind(event.character_id.as_slice())
        .bind(i64_value(event.versions.inventory.pre)?)
        .execute(&mut *connection)
        .await?
        .rows_affected(),
    )?;
    expect_one(
        sqlx::query(
            "UPDATE character_life_metrics SET lifetime_ticks=$1, \
                    permadeath_combat_ticks=$2, life_metrics_version=$3, \
                    updated_at=transaction_timestamp() WHERE namespace_id=$4 AND account_id=$5 \
                  AND character_id=$6 AND life_metrics_version=$7 \
                  AND lifetime_ticks=$8 AND permadeath_combat_ticks=$9",
        )
        .bind(i64_value(event.lifetime_ticks)?)
        .bind(i64_value(event.permadeath_combat_ticks)?)
        .bind(i64_value(event.versions.life_metrics.post)?)
        .bind(&event.namespace_id)
        .bind(event.account_id.as_slice())
        .bind(event.character_id.as_slice())
        .bind(i64_value(event.versions.life_metrics.pre)?)
        .bind(i64_value(life.lifetime_ticks)?)
        .bind(i64_value(life.permadeath_combat_ticks)?)
        .execute(&mut *connection)
        .await?
        .rows_affected(),
    )?;
    expect_one(
        sqlx::query(
            "UPDATE character_entry_restore_points SET restore_state=$1, death_mutation_id=$2, \
                    consumed_at=transaction_timestamp() WHERE namespace_id=$3 AND account_id=$4 \
                  AND character_id=$5 AND restore_point_id=$6 AND lineage_id=$7 \
                  AND restore_state=$8 AND death_mutation_id IS NULL",
        )
        .bind(RESTORE_DEATH_COMMITTED)
        .bind(event.mutation_id.as_slice())
        .bind(&event.namespace_id)
        .bind(event.account_id.as_slice())
        .bind(event.character_id.as_slice())
        .bind(event.restore_point_id.as_slice())
        .bind(event.lineage_id.as_slice())
        .bind(RESTORE_ACTIVE)
        .execute(&mut *connection)
        .await?
        .rows_affected(),
    )?;
    expect_one(
        sqlx::query(
            "UPDATE character_instance_lineages SET lineage_state=$1, \
                    closed_at=transaction_timestamp() WHERE namespace_id=$2 AND account_id=$3 \
                  AND character_id=$4 AND lineage_id=$5 AND lineage_state=$6",
        )
        .bind(LINEAGE_DEATH_FAILED)
        .bind(&event.namespace_id)
        .bind(event.account_id.as_slice())
        .bind(event.character_id.as_slice())
        .bind(event.lineage_id.as_slice())
        .bind(LINEAGE_ACTIVE)
        .execute(&mut *connection)
        .await?
        .rows_affected(),
    )?;
    Ok(())
}

async fn finalize_character_identity(
    connection: &mut PgConnection,
    plan: &AuthoritativeDeathPlanV1,
) -> Result<(), PersistenceError> {
    let event = &plan.event;
    // Migrations 0037/0039 deliberately require the immutable death root to exist before this one
    // exceptional living -> dead identity transition. Every other mutable aggregate is finalized
    // before the root so its ordinary post-death immutability trigger remains fail-closed.
    expect_one(
        sqlx::query(
            "UPDATE characters SET roster_ordinal=NULL, life_state=$1, \
                    character_state_version=$2, updated_at=transaction_timestamp() \
             WHERE namespace_id=$3 AND account_id=$4 AND character_id=$5 \
               AND life_state=$6 AND roster_ordinal=$7 AND character_state_version=$8",
        )
        .bind(LIFE_STATE_DEAD)
        .bind(i64_value(event.versions.character.post)?)
        .bind(&event.namespace_id)
        .bind(event.account_id.as_slice())
        .bind(event.character_id.as_slice())
        .bind(LIFE_STATE_LIVING)
        .bind(i16::from(event.former_roster_ordinal))
        .bind(i64_value(event.versions.character.pre)?)
        .execute(connection)
        .await?
        .rows_affected(),
    )
}

async fn destroy_items(
    connection: &mut PgConnection,
    plan: &AuthoritativeDeathPlanV1,
) -> Result<(), PersistenceError> {
    let event = &plan.event;
    for entry in &plan.destruction {
        let DurableDestructionEntryV1::Item {
            item_uid,
            location,
            pre_item_version,
            post_item_version,
            ledger_event_id,
            ..
        } = entry
        else {
            continue;
        };
        let binding = ItemLocationBinding::from_location(location);
        expect_one(
            sqlx::query(
                "UPDATE item_instances SET item_version=$1, security_state=$2, location_kind=$3, \
                        slot_index=NULL, instance_id=NULL, pickup_id=NULL, expires_at_tick=NULL, \
                        destruction_reason='permadeath', terminal_death_id=$4, \
                        updated_at=transaction_timestamp() \
                 WHERE namespace_id=$5 AND account_id=$6 AND character_id=$7 AND item_uid=$8 \
                   AND item_version=$9 AND security_state=$10 AND location_kind=$11 \
                   AND slot_index IS NOT DISTINCT FROM $12 \
                   AND instance_id IS NOT DISTINCT FROM $13 \
                   AND pickup_id IS NOT DISTINCT FROM $14",
            )
            .bind(i64_value(*post_item_version)?)
            .bind(SECURITY_DESTROYED)
            .bind(LOCATION_DESTROYED)
            .bind(event.death_id.as_slice())
            .bind(&event.namespace_id)
            .bind(event.account_id.as_slice())
            .bind(event.character_id.as_slice())
            .bind(item_uid.as_slice())
            .bind(i64_value(*pre_item_version)?)
            .bind(binding.expected_security)
            .bind(binding.location_kind)
            .bind(binding.slot_index)
            .bind(binding.instance_id.map(|value| value.to_vec()))
            .bind(binding.pickup_id.map(|value| value.to_vec()))
            .execute(&mut *connection)
            .await?
            .rows_affected(),
        )?;
        sqlx::query(
            "INSERT INTO item_ledger_events (namespace_id, ledger_event_id, item_uid, account_id, \
                    character_id, mutation_id, terminal_death_id, event_kind, source_kind, pre_item_version, \
                    post_item_version, pre_security_state, post_security_state, pre_location_kind, \
                    post_location_kind, reason) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,2,3,$8,$9,$10,3,$11,4,'permadeath')",
        )
        .bind(&event.namespace_id)
        .bind(ledger_event_id.as_slice())
        .bind(item_uid.as_slice())
        .bind(event.account_id.as_slice())
        .bind(event.character_id.as_slice())
        .bind(event.mutation_id.as_slice())
        .bind(event.death_id.as_slice())
        .bind(i64_value(*pre_item_version)?)
        .bind(i64_value(*post_item_version)?)
        .bind(binding.expected_security)
        .bind(binding.location_kind)
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

async fn destroy_materials(
    connection: &mut PgConnection,
    plan: &AuthoritativeDeathPlanV1,
) -> Result<(), PersistenceError> {
    let event = &plan.event;
    for entry in &plan.destruction {
        let DurableDestructionEntryV1::RunMaterial {
            material_id,
            destroyed_quantity,
            pre_material_version,
            post_material_version,
            ..
        } = entry
        else {
            continue;
        };
        expect_one(
            sqlx::query(
                "UPDATE character_run_material_stacks SET quantity=0, material_version=$1, \
                        security_state=3, terminal_reason='permadeath', \
                        terminal_restore_point_id=NULL, terminal_death_id=$2, \
                        updated_at=transaction_timestamp() WHERE namespace_id=$3 AND account_id=$4 \
                      AND character_id=$5 AND material_id=$6 AND quantity=$7 \
                      AND material_version=$8 AND security_state=2",
            )
            .bind(i64_value(*post_material_version)?)
            .bind(event.death_id.as_slice())
            .bind(&event.namespace_id)
            .bind(event.account_id.as_slice())
            .bind(event.character_id.as_slice())
            .bind(material_id)
            .bind(i32_value(*destroyed_quantity)?)
            .bind(i64_value(*pre_material_version)?)
            .execute(&mut *connection)
            .await?
            .rows_affected(),
        )?;
    }
    Ok(())
}

async fn insert_death_event(
    connection: &mut PgConnection,
    request: &DurableDeathCommitRequestV1,
) -> Result<(), PersistenceError> {
    let event = &request.plan.event;
    sqlx::query(
        "INSERT INTO death_events (namespace_id, death_id, account_id, character_id, \
            contract_kind, mutation_id, canonical_request_hash, content_revision, instance_id, \
            lineage_id, restore_point_id, region_id, room_id, death_tick, cause_kind, \
            killer_content_id, killer_pattern_id, killer_attack_id, raw_damage, final_damage, \
            damage_type, pre_hit_health, source_x_milli_tiles, source_y_milli_tiles, network_state, \
            recall_state, lifetime_ticks, permadeath_combat_ticks, pre_account_version, \
            post_account_version, pre_character_version, post_character_version, \
            pre_progression_version, post_progression_version, pre_inventory_version, \
            post_inventory_version, pre_life_metrics_version, post_life_metrics_version, \
            trace_digest, former_roster_ordinal, echo_expected, preexisting_available_echo_id, \
            promoted_echo_id, world_records_blake3, world_assets_blake3, \
            world_localization_blake3, bargain_cleanup_event_id, pre_oath_bargain_version, \
            post_oath_bargain_version) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20, \
                 $21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32,$33,$34,$35,$36,$37,$38, \
                 $39,$40,$41,$42,$43,$44,$45,$46,$47,$48,$49)",
    )
    .bind(&event.namespace_id)
    .bind(event.death_id.as_slice())
    .bind(event.account_id.as_slice())
    .bind(event.character_id.as_slice())
    .bind(&request.contract)
    .bind(event.mutation_id.as_slice())
    .bind(request.canonical_request_hash.as_slice())
    .bind(&event.content_revision)
    .bind(event.instance_id.as_slice())
    .bind(event.lineage_id.as_slice())
    .bind(event.restore_point_id.as_slice())
    .bind(&event.region_id)
    .bind(&event.room_id)
    .bind(i64_value(event.death_tick)?)
    .bind(death_cause(event.cause))
    .bind(&event.killer_content_id)
    .bind(&event.killer_pattern_id)
    .bind(&event.killer_attack_id)
    .bind(i32_value(event.raw_damage)?)
    .bind(i32_value(event.final_damage)?)
    .bind(damage_type(event.damage_type))
    .bind(i32_value(event.pre_hit_health)?)
    .bind(event.source_x_milli_tiles)
    .bind(event.source_y_milli_tiles)
    .bind(network_state(event.network_state))
    .bind(recall_state(event.recall_state))
    .bind(i64_value(event.lifetime_ticks)?)
    .bind(i64_value(event.permadeath_combat_ticks)?)
    .bind(i64_value(event.versions.account.pre)?)
    .bind(i64_value(event.versions.account.post)?)
    .bind(i64_value(event.versions.character.pre)?)
    .bind(i64_value(event.versions.character.post)?)
    .bind(i64_value(event.versions.progression.pre)?)
    .bind(i64_value(event.versions.progression.post)?)
    .bind(i64_value(event.versions.inventory.pre)?)
    .bind(i64_value(event.versions.inventory.post)?)
    .bind(i64_value(event.versions.life_metrics.pre)?)
    .bind(i64_value(event.versions.life_metrics.post)?)
    .bind(event.trace_digest.as_slice())
    .bind(i16::from(event.former_roster_ordinal))
    .bind(request.plan.echo.is_some())
    .bind(
        request
            .plan
            .echo
            .as_ref()
            .and_then(|echo| echo.preexisting_available_echo_id)
            .map(|value| value.to_vec()),
    )
    .bind(
        request
            .plan
            .echo
            .as_ref()
            .and_then(|echo| echo.promotion.as_ref())
            .map(|promotion| promotion.echo_id.to_vec()),
    )
    .bind(&event.records_blake3)
    .bind(&event.assets_blake3)
    .bind(&event.localization_blake3)
    .bind(event.bargain_cleanup_event_id.as_slice())
    .bind(i64_value(event.versions.oath_bargain.pre)?)
    .bind(i64_value(event.versions.oath_bargain.post)?)
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_trace(
    connection: &mut PgConnection,
    plan: &AuthoritativeDeathPlanV1,
) -> Result<(), PersistenceError> {
    for entry in &plan.trace {
        sqlx::query(
            "INSERT INTO death_combat_trace_entries (namespace_id, death_id, trace_ordinal, \
                event_tick, event_ordinal, source_content_id, source_entity_id, pattern_id, \
                attack_id, raw_damage, final_damage, damage_type, pre_health, post_health, \
                source_x_milli_tiles, source_y_milli_tiles, network_state, recall_state, lethal) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19)",
        )
        .bind(&plan.event.namespace_id)
        .bind(plan.event.death_id.as_slice())
        .bind(i16_value(entry.ordinal)?)
        .bind(i64_value(entry.event_tick)?)
        .bind(i32_value(entry.event_ordinal)?)
        .bind(&entry.source_content_id)
        .bind(entry.source_entity_id.map(|value| value.to_vec()))
        .bind(&entry.pattern_id)
        .bind(&entry.attack_id)
        .bind(i32_value(entry.raw_damage)?)
        .bind(i32_value(entry.final_damage)?)
        .bind(damage_type(entry.damage_type))
        .bind(i32_value(entry.pre_health)?)
        .bind(i32_value(entry.post_health)?)
        .bind(entry.source_x_milli_tiles)
        .bind(entry.source_y_milli_tiles)
        .bind(network_state(entry.network_state))
        .bind(recall_state(entry.recall_state))
        .bind(entry.lethal)
        .execute(&mut *connection)
        .await?;
        for status in &entry.statuses {
            sqlx::query(
                "INSERT INTO death_combat_trace_statuses (namespace_id, death_id, trace_ordinal, \
                    status_ordinal, status_id, remaining_ticks, stack_count) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7)",
            )
            .bind(&plan.event.namespace_id)
            .bind(plan.event.death_id.as_slice())
            .bind(i16_value(entry.ordinal)?)
            .bind(i16::from(status.ordinal))
            .bind(&status.status_id)
            .bind(i32_value(status.remaining_ticks)?)
            .bind(i16_value(status.stack_count)?)
            .execute(&mut *connection)
            .await?;
        }
    }
    Ok(())
}

async fn insert_summary(
    connection: &mut PgConnection,
    plan: &AuthoritativeDeathPlanV1,
) -> Result<(), PersistenceError> {
    let summary = &plan.summary;
    sqlx::query(
        "INSERT INTO death_summary_snapshots (namespace_id, death_id, summary_revision, \
            hero_label_key, character_name_snapshot, class_id, level, oath_id, lifetime_ms, \
            final_deed_id, echo_outcome, content_revision, snapshot_digest) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
    )
    .bind(&summary.namespace_id)
    .bind(summary.death_id.as_slice())
    .bind(i16_value(summary.summary_revision)?)
    .bind(&summary.hero_label_key)
    .bind(&summary.character_name_snapshot)
    .bind(&summary.class_id)
    .bind(i16::from(summary.level))
    .bind(&summary.oath_id)
    .bind(i64_value(summary.lifetime_ms)?)
    .bind(&summary.final_deed_id)
    .bind(echo_outcome(summary.echo_outcome))
    .bind(&summary.content_revision)
    .bind(summary.snapshot_digest.as_slice())
    .execute(&mut *connection)
    .await?;
    for bargain in &summary.bargains {
        sqlx::query(
            "INSERT INTO death_summary_bargains \
                (namespace_id, death_id, bargain_ordinal, bargain_id) VALUES ($1,$2,$3,$4)",
        )
        .bind(&summary.namespace_id)
        .bind(summary.death_id.as_slice())
        .bind(i16_value(bargain.ordinal)?)
        .bind(&bargain.content_id)
        .execute(&mut *connection)
        .await?;
    }
    for reference in &summary.last_five_damage {
        sqlx::query(
            "INSERT INTO death_summary_damage_entries \
                (namespace_id, death_id, summary_ordinal, trace_ordinal) VALUES ($1,$2,$3,$4)",
        )
        .bind(&summary.namespace_id)
        .bind(summary.death_id.as_slice())
        .bind(i16::from(reference.ordinal))
        .bind(i16_value(reference.trace_ordinal)?)
        .execute(&mut *connection)
        .await?;
    }
    for (section, entries) in [
        (0_i16, summary.projections.lost.as_slice()),
        (1_i16, summary.projections.preserved.as_slice()),
        (2_i16, summary.projections.created.as_slice()),
    ] {
        for entry in entries {
            insert_projection(connection, summary, section, entry).await?;
        }
    }
    Ok(())
}

async fn insert_projection(
    connection: &mut PgConnection,
    summary: &crate::DurableDeathSummaryV1,
    section: i16,
    entry: &DurableSummaryProjectionEntryV1,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO death_summary_projection_entries (namespace_id, death_id, section_kind, \
            entry_ordinal, projection_kind, content_id, quantity, item_uid) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(&summary.namespace_id)
    .bind(summary.death_id.as_slice())
    .bind(section)
    .bind(i16_value(entry.ordinal)?)
    .bind(projection_kind(entry.kind))
    .bind(&entry.content_id)
    .bind(i32_value(entry.quantity)?)
    .bind(entry.item_uid.map(|value| value.to_vec()))
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_memorial(
    connection: &mut PgConnection,
    plan: &AuthoritativeDeathPlanV1,
) -> Result<(), PersistenceError> {
    let memorial = &plan.memorial;
    sqlx::query(
        "INSERT INTO memorial_records (namespace_id, death_id, account_id, death_at, \
            summary_revision, presentation_key, presentation_digest) \
         VALUES ($1,$2,$3,transaction_timestamp(),$4,$5,$6)",
    )
    .bind(&memorial.namespace_id)
    .bind(memorial.death_id.as_slice())
    .bind(memorial.account_id.as_slice())
    .bind(i16_value(memorial.summary_revision)?)
    .bind(&memorial.presentation_key)
    .bind(memorial.presentation_digest.as_slice())
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_destruction(
    connection: &mut PgConnection,
    plan: &AuthoritativeDeathPlanV1,
) -> Result<(), PersistenceError> {
    let event = &plan.event;
    for entry in &plan.destruction {
        match entry {
            DurableDestructionEntryV1::Item {
                ordinal,
                item_uid,
                location,
                pre_item_version,
                post_item_version,
                ledger_event_id,
                ..
            } => {
                let binding = ItemLocationBinding::from_location(location);
                sqlx::query(
                    "INSERT INTO death_destruction_entries (namespace_id, death_id, \
                        destruction_ordinal, entry_kind, item_uid, material_id, quantity, \
                        pre_location_kind, pre_slot_index, pre_instance_id, pre_pickup_id, \
                        pre_item_version, post_item_version, ledger_event_id, account_id, \
                        character_id, pre_material_version, post_material_version, \
                        pre_material_quantity) \
                     VALUES ($1,$2,$3,0,$4,NULL,1,$5,$6,$7,$8,$9,$10,$11,$12,$13,NULL,NULL,NULL)",
                )
                .bind(&event.namespace_id)
                .bind(event.death_id.as_slice())
                .bind(i16_value(*ordinal)?)
                .bind(item_uid.as_slice())
                .bind(binding.location_kind)
                .bind(binding.slot_index)
                .bind(binding.instance_id.map(|value| value.to_vec()))
                .bind(binding.pickup_id.map(|value| value.to_vec()))
                .bind(i64_value(*pre_item_version)?)
                .bind(i64_value(*post_item_version)?)
                .bind(ledger_event_id.as_slice())
                .bind(event.account_id.as_slice())
                .bind(event.character_id.as_slice())
                .execute(&mut *connection)
                .await?;
            }
            DurableDestructionEntryV1::RunMaterial {
                ordinal,
                material_id,
                destroyed_quantity,
                pre_material_quantity,
                pre_material_version,
                post_material_version,
            } => {
                sqlx::query(
                    "INSERT INTO death_destruction_entries (namespace_id, death_id, \
                        destruction_ordinal, entry_kind, item_uid, material_id, quantity, \
                        pre_location_kind, pre_slot_index, pre_instance_id, pre_pickup_id, \
                        pre_item_version, post_item_version, ledger_event_id, account_id, \
                        character_id, pre_material_version, post_material_version, \
                        pre_material_quantity) \
                     VALUES ($1,$2,$3,1,NULL,$4,$5,NULL,NULL,NULL,NULL,NULL,NULL,NULL,$6,$7,$8,$9,$10)",
                )
                .bind(&event.namespace_id)
                .bind(event.death_id.as_slice())
                .bind(i16_value(*ordinal)?)
                .bind(material_id)
                .bind(i32_value(*destroyed_quantity)?)
                .bind(event.account_id.as_slice())
                .bind(event.character_id.as_slice())
                .bind(i64_value(*pre_material_version)?)
                .bind(i64_value(*post_material_version)?)
                .bind(i32_value(*pre_material_quantity)?)
                .execute(&mut *connection)
                .await?;
            }
        }
    }
    Ok(())
}

async fn write_echo_projection(
    connection: &mut PgConnection,
    plan: &AuthoritativeDeathPlanV1,
) -> Result<(), PersistenceError> {
    let Some(envelope) = &plan.echo else {
        return Ok(());
    };
    let echo = &envelope.created;
    sqlx::query(
        "INSERT INTO echo_records (namespace_id, echo_id, death_id, account_id, \
            character_name_snapshot, class_id, oath_id, level, appearance_snapshot_id, \
            appearance_theme_id, weapon_signature_tag, relic_signature_tag, killer_content_id, \
            killer_pattern_id, death_region_id, power_band, state, content_revision, \
            snapshot_digest, created_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,0,$17,$18, \
                 transaction_timestamp())",
    )
    .bind(&echo.namespace_id)
    .bind(echo.echo_id.as_slice())
    .bind(echo.death_id.as_slice())
    .bind(echo.account_id.as_slice())
    .bind(&echo.character_name_snapshot)
    .bind(&echo.class_id)
    .bind(&echo.oath_id)
    .bind(i16::from(echo.level))
    .bind(&echo.appearance_snapshot_id)
    .bind(&echo.appearance_theme_id)
    .bind(&echo.weapon_signature_tag)
    .bind(&echo.relic_signature_tag)
    .bind(&echo.killer_content_id)
    .bind(&echo.killer_pattern_id)
    .bind(&echo.death_region_id)
    .bind(i16::from(echo.power_band))
    .bind(&echo.content_revision)
    .bind(echo.snapshot_digest.as_slice())
    .execute(&mut *connection)
    .await?;
    for bargain in &echo.bargains {
        sqlx::query(
            "INSERT INTO echo_bargain_snapshots \
                (namespace_id, echo_id, bargain_ordinal, bargain_id) VALUES ($1,$2,$3,$4)",
        )
        .bind(&echo.namespace_id)
        .bind(echo.echo_id.as_slice())
        .bind(i16_value(bargain.ordinal)?)
        .bind(&bargain.content_id)
        .execute(&mut *connection)
        .await?;
    }
    for deed in &echo.deed_tags {
        sqlx::query(
            "INSERT INTO echo_deed_tags \
                (namespace_id, echo_id, deed_ordinal, deed_tag) VALUES ($1,$2,$3,$4)",
        )
        .bind(&echo.namespace_id)
        .bind(echo.echo_id.as_slice())
        .bind(i16_value(deed.ordinal)?)
        .bind(&deed.content_id)
        .execute(&mut *connection)
        .await?;
    }
    insert_echo_transition(
        connection,
        &echo.namespace_id,
        &envelope.creation_transition,
    )
    .await?;
    if let Some(promotion) = &envelope.promotion {
        expect_one(
            sqlx::query(
                "UPDATE echo_records SET state=1 WHERE namespace_id=$1 AND account_id=$2 \
                   AND echo_id=$3 AND death_id=$4 AND state=0",
            )
            .bind(&echo.namespace_id)
            .bind(echo.account_id.as_slice())
            .bind(promotion.echo_id.as_slice())
            .bind(promotion.echo_death_id.as_slice())
            .execute(&mut *connection)
            .await?
            .rows_affected(),
        )?;
        insert_echo_transition(connection, &echo.namespace_id, promotion).await?;
    }
    Ok(())
}

async fn insert_echo_transition(
    connection: &mut PgConnection,
    namespace_id: &str,
    transition: &crate::DurableEchoTransitionV1,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO echo_state_transitions (namespace_id, echo_id, transition_ordinal, \
            previous_state, next_state, reason_kind, source_death_id, committed_at, \
            trigger_death_id) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,transaction_timestamp(),$8)",
    )
    .bind(namespace_id)
    .bind(transition.echo_id.as_slice())
    .bind(i16_value(transition.ordinal)?)
    .bind(transition.previous_state.map(echo_state))
    .bind(echo_state(transition.next_state))
    .bind(match transition.reason {
        crate::DurableEchoTransitionReasonV1::EligibleDeath => 0_i16,
        crate::DurableEchoTransitionReasonV1::OldestDormantPromotion => 1_i16,
    })
    .bind(transition.source_death_id.map(|value| value.to_vec()))
    .bind(transition.trigger_death_id.as_slice())
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_result(
    connection: &mut PgConnection,
    request: &DurableDeathCommitRequestV1,
    result: &StoredCommittedDeathResultV1,
) -> Result<(), PersistenceError> {
    let payload = result.payload()?;
    let digest = result.digest()?;
    let event = &request.plan.event;
    sqlx::query(
        "INSERT INTO death_mutation_results (namespace_id, account_id, character_id, mutation_id, \
            contract_kind, death_id, canonical_request_hash, result_code, result_payload, \
            result_hash, issued_at, committed_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,1,$8,$9, \
                 to_timestamp($10::double precision / 1000.0), transaction_timestamp())",
    )
    .bind(&event.namespace_id)
    .bind(event.account_id.as_slice())
    .bind(event.character_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(&request.contract)
    .bind(event.death_id.as_slice())
    .bind(request.canonical_request_hash.as_slice())
    .bind(payload)
    .bind(digest.as_slice())
    .bind(i64_value(request.issued_at_unix_ms)?)
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_accepted_audit(
    connection: &mut PgConnection,
    request: &DurableDeathCommitRequestV1,
    result: &StoredCommittedDeathResultV1,
) -> Result<(), PersistenceError> {
    let event = &request.plan.event;
    let digest = result.digest()?;
    let audit_id = derived_id(
        DEATH_ACCEPTED_AUDIT_ID_CONTEXT,
        &[event.death_id.as_slice(), request.mutation_id.as_slice()],
    );
    sqlx::query(
        "INSERT INTO death_audit_events (namespace_id, account_id, character_id, audit_event_id, \
            death_id, mutation_id, event_kind, event_digest, created_at) \
         VALUES ($1,$2,$3,$4,$5,$6,0,$7,transaction_timestamp())",
    )
    .bind(&event.namespace_id)
    .bind(event.account_id.as_slice())
    .bind(event.character_id.as_slice())
    .bind(audit_id.as_slice())
    .bind(event.death_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(digest.as_slice())
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_outbox(
    connection: &mut PgConnection,
    plan: &AuthoritativeDeathPlanV1,
    result: &StoredCommittedDeathResultV1,
) -> Result<(), PersistenceError> {
    let death_payload = result.payload()?;
    insert_outbox_row(
        connection,
        &plan.event.namespace_id,
        plan.event.death_id,
        "death_committed",
        None,
        None,
        None,
        &death_payload,
    )
    .await?;
    let Some(envelope) = &plan.echo else {
        return Ok(());
    };
    let created_payload = bounded_payload(&envelope.created)?;
    insert_outbox_row(
        connection,
        &plan.event.namespace_id,
        plan.event.death_id,
        "echo_created",
        Some(envelope.created.echo_id),
        Some(0),
        Some(plan.event.death_id),
        &created_payload,
    )
    .await?;
    if let Some(promotion) = &envelope.promotion {
        let promoted_payload = bounded_payload(promotion)?;
        insert_outbox_row(
            connection,
            &plan.event.namespace_id,
            promotion.echo_death_id,
            "echo_promoted",
            Some(promotion.echo_id),
            Some(promotion.ordinal),
            Some(plan.event.death_id),
            &promoted_payload,
        )
        .await?;
    }
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "one normalized outbox row has exact identity columns"
)]
async fn insert_outbox_row(
    connection: &mut PgConnection,
    namespace_id: &str,
    owner_death_id: [u8; 16],
    event_type: &str,
    echo_id: Option<[u8; 16]>,
    transition_ordinal: Option<u16>,
    trigger_death_id: Option<[u8; 16]>,
    payload: &[u8],
) -> Result<(), PersistenceError> {
    let ordinal_bytes = transition_ordinal.unwrap_or(u16::MAX).to_be_bytes();
    let event_id = derived_id(
        DEATH_OUTBOX_ID_CONTEXT,
        &[
            owner_death_id.as_slice(),
            event_type.as_bytes(),
            echo_id.as_ref().map_or(&[][..], <[u8; 16]>::as_slice),
            ordinal_bytes.as_slice(),
            trigger_death_id
                .as_ref()
                .map_or(&[][..], <[u8; 16]>::as_slice),
        ],
    );
    sqlx::query(
        "INSERT INTO death_outbox_events (namespace_id, death_id, event_id, event_type, echo_id, \
            event_payload, created_at, echo_transition_ordinal, trigger_death_id) \
         VALUES ($1,$2,$3,$4,$5,$6,transaction_timestamp(),$7,$8)",
    )
    .bind(namespace_id)
    .bind(owner_death_id.as_slice())
    .bind(event_id.as_slice())
    .bind(event_type)
    .bind(echo_id.map(|value| value.to_vec()))
    .bind(payload)
    .bind(transition_ordinal.map(i16_value).transpose()?)
    .bind(trigger_death_id.map(|value| value.to_vec()))
    .execute(connection)
    .await?;
    Ok(())
}

async fn load_result_by_mutation(
    connection: &mut PgConnection,
    request: &DurableDeathCommitRequestV1,
) -> Result<Option<StoredResultRow>, PersistenceError> {
    load_result(
        connection,
        sqlx::query(
            "SELECT account_id, character_id, mutation_id, death_id, contract_kind, \
                    canonical_request_hash, result_code, result_payload, result_hash \
             FROM death_mutation_results WHERE namespace_id=$1 AND account_id=$2 \
               AND mutation_id=$3 FOR UPDATE",
        )
        .bind(&request.plan.event.namespace_id)
        .bind(request.plan.event.account_id.as_slice())
        .bind(request.mutation_id.as_slice()),
    )
    .await
}

async fn load_result_by_final_identity(
    connection: &mut PgConnection,
    request: &DurableDeathCommitRequestV1,
) -> Result<Option<StoredResultRow>, PersistenceError> {
    load_result(
        connection,
        sqlx::query(
            "SELECT account_id, character_id, mutation_id, death_id, contract_kind, \
                    canonical_request_hash, result_code, result_payload, result_hash \
             FROM death_mutation_results WHERE namespace_id=$1 AND account_id=$2 \
               AND character_id=$3 AND contract_kind=$4 FOR UPDATE",
        )
        .bind(&request.plan.event.namespace_id)
        .bind(request.plan.event.account_id.as_slice())
        .bind(request.plan.event.character_id.as_slice())
        .bind(&request.contract),
    )
    .await
}

async fn load_result(
    connection: &mut PgConnection,
    query: sqlx::query::Query<'_, sqlx::Postgres, sqlx::postgres::PgArguments>,
) -> Result<Option<StoredResultRow>, PersistenceError> {
    query
        .fetch_optional(connection)
        .await?
        .map(|row| {
            Ok(StoredResultRow {
                account_id: exact_id(row.try_get("account_id")?)?,
                character_id: exact_id(row.try_get("character_id")?)?,
                mutation_id: exact_id(row.try_get("mutation_id")?)?,
                death_id: exact_id(row.try_get("death_id")?)?,
                contract: row.try_get("contract_kind")?,
                request_hash: exact_hash(row.try_get("canonical_request_hash")?)?,
                result_code: row.try_get("result_code")?,
                payload: row.try_get("result_payload")?,
                digest: exact_hash(row.try_get("result_hash")?)?,
            })
        })
        .transpose()
}

async fn finish_replay_or_conflict(
    transaction: crate::PersistenceTransaction<'_>,
    request: &DurableDeathCommitRequestV1,
    stored: StoredResultRow,
) -> Result<DurableDeathTransactionV1, PersistenceError> {
    if stored.account_id != request.plan.event.account_id
        || stored.character_id != request.plan.event.character_id
        || stored.mutation_id != request.mutation_id
        || stored.death_id != request.plan.event.death_id
        || stored.contract != request.contract
        || stored.request_hash != request.canonical_request_hash
    {
        return finish_conflict(transaction, request, &stored).await;
    }
    let result = decode_stored_result(&stored, request)?;
    transaction.rollback().await?;
    Ok(DurableDeathTransactionV1::Replayed(result))
}

async fn finish_conflict(
    mut transaction: crate::PersistenceTransaction<'_>,
    request: &DurableDeathCommitRequestV1,
    stored: &StoredResultRow,
) -> Result<DurableDeathTransactionV1, PersistenceError> {
    let audit_id = derived_id(
        DEATH_CONFLICT_AUDIT_ID_CONTEXT,
        &[
            stored.death_id.as_slice(),
            stored.request_hash.as_slice(),
            request.canonical_request_hash.as_slice(),
        ],
    );
    let digest = derived_hash(
        DEATH_CONFLICT_AUDIT_DIGEST_CONTEXT,
        &[
            stored.request_hash.as_slice(),
            request.canonical_request_hash.as_slice(),
            request.canonical_plan_hash.as_slice(),
        ],
    );
    sqlx::query(
        "INSERT INTO death_audit_events (namespace_id, account_id, character_id, audit_event_id, \
            death_id, mutation_id, event_kind, event_digest, created_at) \
         VALUES ($1,$2,$3,$4,$5,$6,1,$7,transaction_timestamp()) \
         ON CONFLICT (namespace_id, audit_event_id) DO NOTHING",
    )
    .bind(&request.plan.event.namespace_id)
    .bind(stored.account_id.as_slice())
    .bind(stored.character_id.as_slice())
    .bind(audit_id.as_slice())
    .bind(stored.death_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(digest.as_slice())
    .execute(transaction.connection())
    .await?;
    transaction.commit().await?;
    Err(PersistenceError::DurableDeathIdempotencyConflict)
}

fn decode_stored_result(
    stored: &StoredResultRow,
    request: &DurableDeathCommitRequestV1,
) -> Result<StoredCommittedDeathResultV1, PersistenceError> {
    if stored.result_code != 1 {
        return Err(PersistenceError::CorruptStoredDurableDeath);
    }
    let result = StoredCommittedDeathResultV1::decode(&stored.payload)?;
    if result.digest()? != stored.digest {
        return Err(PersistenceError::CorruptStoredDurableDeath);
    }
    let mut rebound = request.clone();
    rebound.bind_commit_time(result.committed_at_unix_ms)?;
    result.validate_against(&rebound)?;
    Ok(result)
}

async fn force_deferred_constraints(connection: &mut PgConnection) -> Result<(), PersistenceError> {
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(connection)
        .await?;
    Ok(())
}

fn death_cause(value: DurableDeathCauseV1) -> i16 {
    match value {
        DurableDeathCauseV1::DirectHit => 0,
        DurableDeathCauseV1::DamageOverTime => 1,
        DurableDeathCauseV1::Environment => 2,
        DurableDeathCauseV1::Disconnect => 3,
    }
}

fn damage_type(value: DurableDamageTypeV1) -> i16 {
    match value {
        DurableDamageTypeV1::Physical => 0,
        DurableDamageTypeV1::Veil => 1,
    }
}

fn network_state(value: DurableNetworkStateV1) -> i16 {
    match value {
        DurableNetworkStateV1::Connected => 0,
        DurableNetworkStateV1::Degraded => 1,
        DurableNetworkStateV1::LinkLost => 2,
        DurableNetworkStateV1::Reattached => 3,
    }
}

fn recall_state(value: DurableRecallStateV1) -> i16 {
    match value {
        DurableRecallStateV1::Inactive => 0,
        DurableRecallStateV1::Channeling => 1,
        DurableRecallStateV1::CompletionPending => 2,
    }
}

fn equipment_slot(value: DurableEquipmentSlotV1) -> i16 {
    match value {
        DurableEquipmentSlotV1::Weapon => 0,
        DurableEquipmentSlotV1::Relic => 1,
        DurableEquipmentSlotV1::Armor => 2,
        DurableEquipmentSlotV1::Charm => 3,
    }
}

fn echo_state(value: DurableEchoStateV1) -> i16 {
    match value {
        DurableEchoStateV1::Dormant => 0,
        DurableEchoStateV1::Available => 1,
    }
}

fn echo_outcome(value: DurableEchoOutcomeV1) -> i16 {
    match value {
        DurableEchoOutcomeV1::NotEligible => 0,
        DurableEchoOutcomeV1::Dormant => 1,
        DurableEchoOutcomeV1::Available => 2,
    }
}

fn projection_kind(value: DurableSummaryProjectionKindV1) -> i16 {
    match value {
        DurableSummaryProjectionKindV1::LostItem => 0,
        DurableSummaryProjectionKindV1::LostRunMaterial => 1,
        DurableSummaryProjectionKindV1::PreservedAccountRecords => 2,
        DurableSummaryProjectionKindV1::PreservedCurrency => 3,
        DurableSummaryProjectionKindV1::PreservedVault => 4,
        DurableSummaryProjectionKindV1::PreservedCosmetics => 5,
        DurableSummaryProjectionKindV1::PreservedRecipes => 6,
        DurableSummaryProjectionKindV1::CreatedMemorial => 7,
        DurableSummaryProjectionKindV1::CreatedEcho => 8,
    }
}

fn bounded_payload<T: Serialize>(value: &T) -> Result<Vec<u8>, PersistenceError> {
    let payload =
        postcard::to_stdvec(value).map_err(|_| PersistenceError::CorruptStoredDurableDeath)?;
    if payload.is_empty() || payload.len() > 65_536 {
        return Err(PersistenceError::CorruptStoredDurableDeath);
    }
    Ok(payload)
}

fn derived_hash(context: &str, parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_derive_key(context);
    for part in parts {
        hasher.update(&(part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    *hasher.finalize().as_bytes()
}

fn derived_id(context: &str, parts: &[&[u8]]) -> [u8; 16] {
    let digest = derived_hash(context, parts);
    let mut value = [0_u8; 16];
    value.copy_from_slice(&digest[..16]);
    if value == [0; 16] {
        value[15] = 1;
    }
    value
}

fn exact_id(value: Vec<u8>) -> Result<[u8; 16], PersistenceError> {
    value
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredDurableDeath)
}

fn optional_id(value: Option<Vec<u8>>) -> Result<Option<[u8; 16]>, PersistenceError> {
    value.map(exact_id).transpose()
}

fn exact_hash(value: Vec<u8>) -> Result<[u8; 32], PersistenceError> {
    value
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredDurableDeath)
}

fn positive(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(PersistenceError::CorruptStoredDurableDeath)
}

fn nonnegative(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value).map_err(|_| PersistenceError::CorruptStoredDurableDeath)
}

fn u8_value(value: i16) -> Result<u8, PersistenceError> {
    u8::try_from(value).map_err(|_| PersistenceError::CorruptStoredDurableDeath)
}

fn u32_value(value: i32) -> Result<u32, PersistenceError> {
    u32::try_from(value).map_err(|_| PersistenceError::CorruptStoredDurableDeath)
}

fn i16_value(value: u16) -> Result<i16, PersistenceError> {
    i16::try_from(value).map_err(|_| PersistenceError::CorruptStoredDurableDeath)
}

fn i32_value(value: u32) -> Result<i32, PersistenceError> {
    i32::try_from(value).map_err(|_| PersistenceError::CorruptStoredDurableDeath)
}

fn i64_value(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| PersistenceError::CorruptStoredDurableDeath)
}

fn expect_one(rows: u64) -> Result<(), PersistenceError> {
    if rows == 1 {
        Ok(())
    } else {
        Err(PersistenceError::CorruptStoredDurableDeath)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_match_the_append_only_schema_contract() {
        assert_eq!(death_cause(DurableDeathCauseV1::Disconnect), 3);
        assert_eq!(damage_type(DurableDamageTypeV1::Veil), 1);
        assert_eq!(network_state(DurableNetworkStateV1::LinkLost), 2);
        assert_eq!(recall_state(DurableRecallStateV1::CompletionPending), 2);
        assert_eq!(equipment_slot(DurableEquipmentSlotV1::Charm), 3);
        assert_eq!(echo_outcome(DurableEchoOutcomeV1::Available), 2);
        assert_eq!(
            projection_kind(DurableSummaryProjectionKindV1::CreatedEcho),
            8
        );
    }

    #[test]
    fn location_bindings_require_the_correct_risk_custody() {
        assert_eq!(
            ItemLocationBinding::from_location(&DurableDestructionLocationV1::Equipment {
                slot: DurableEquipmentSlotV1::Relic,
            }),
            ItemLocationBinding {
                location_kind: 0,
                slot_index: Some(1),
                instance_id: None,
                pickup_id: None,
                expected_security: SECURITY_AT_RISK_EQUIPPED,
            }
        );
        let instance_id = [9; 16];
        let pickup_id = [7; 16];
        assert_eq!(
            ItemLocationBinding::from_location(&DurableDestructionLocationV1::PersonalGround {
                instance_id,
                pickup_id,
            }),
            ItemLocationBinding {
                location_kind: 3,
                slot_index: None,
                instance_id: Some(instance_id),
                pickup_id: Some(pickup_id),
                expected_security: SECURITY_AT_RISK_PENDING,
            }
        );
    }

    #[test]
    fn durable_ids_are_domain_separated_and_nonzero() {
        let material = [5_u8; 32];
        let accepted = derived_id(DEATH_ACCEPTED_AUDIT_ID_CONTEXT, &[&material]);
        let conflict = derived_id(DEATH_CONFLICT_AUDIT_ID_CONTEXT, &[&material]);
        assert_ne!(accepted, [0; 16]);
        assert_ne!(accepted, conflict);
        assert_eq!(
            accepted,
            derived_id(DEATH_ACCEPTED_AUDIT_ID_CONTEXT, &[&material])
        );
    }

    #[test]
    fn numeric_conversions_fail_closed() {
        assert!(positive(0).is_err());
        assert!(nonnegative(-1).is_err());
        assert!(i16_value(u16::MAX).is_err());
        assert!(i32_value(u32::MAX).is_err());
        assert!(exact_id(vec![0; 15]).is_err());
        assert!(exact_hash(vec![0; 31]).is_err());
    }

    #[test]
    fn terminal_runtime_prestate_accepts_monotonic_runtime_authority_only() {
        let valid = TerminalRuntimePrestate {
            durable_health: 90,
            durable_lifetime_ticks: 12_100,
            durable_combat_ticks: 4_050,
            durable_life_version: 8,
            entry_lifetime_ticks: 12_000,
            entry_combat_ticks: 4_000,
            entry_life_version: 7,
            root_entry_life_version: 7,
            terminal_lifetime_ticks: 12_140,
            terminal_combat_ticks: 4_090,
            expected_pre_life_version: 8,
        };
        assert!(valid.stored_history_valid());
        assert!(valid.request_is_monotonic());

        let equality_boundary = TerminalRuntimePrestate {
            durable_lifetime_ticks: valid.entry_lifetime_ticks,
            durable_combat_ticks: valid.entry_combat_ticks,
            terminal_lifetime_ticks: valid.entry_lifetime_ticks,
            terminal_combat_ticks: valid.entry_combat_ticks,
            ..valid
        };
        assert!(equality_boundary.stored_history_valid());
        assert!(equality_boundary.request_is_monotonic());

        for invalid in [
            TerminalRuntimePrestate {
                durable_health: 0,
                ..valid
            },
            TerminalRuntimePrestate {
                durable_lifetime_ticks: valid.entry_lifetime_ticks - 1,
                ..valid
            },
            TerminalRuntimePrestate {
                terminal_lifetime_ticks: valid.durable_lifetime_ticks - 1,
                ..valid
            },
            TerminalRuntimePrestate {
                durable_combat_ticks: valid.entry_combat_ticks - 1,
                ..valid
            },
            TerminalRuntimePrestate {
                terminal_combat_ticks: valid.durable_combat_ticks - 1,
                ..valid
            },
            TerminalRuntimePrestate {
                entry_life_version: 6,
                ..valid
            },
            TerminalRuntimePrestate {
                expected_pre_life_version: 9,
                ..valid
            },
            TerminalRuntimePrestate {
                terminal_combat_ticks: valid.terminal_lifetime_ticks + 1,
                ..valid
            },
        ] {
            assert!(!(invalid.stored_history_valid() && invalid.request_is_monotonic()));
        }
    }

    #[test]
    fn destruction_sources_must_exhaust_locked_at_risk_custody() {
        let item_uid = [1; 16];
        let mut items = BTreeMap::from([(
            item_uid,
            ItemLock {
                item_uid,
                template_id: "item.core.test".into(),
                content_revision: "core-dev.blake3.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                item_version: 4,
                security_state: SECURITY_AT_RISK_EQUIPPED,
                location_kind: 0,
                slot_index: Some(0),
                instance_id: None,
                pickup_id: None,
            },
        )]);
        let materials = BTreeMap::from([(
            "material.core.test".into(),
            MaterialLock {
                material_id: "material.core.test".into(),
                quantity: 3,
                version: 8,
            },
        )]);
        let destruction = vec![
            DurableDestructionEntryV1::Item {
                ordinal: 0,
                content_id: "item.core.test".into(),
                item_uid,
                location: DurableDestructionLocationV1::Equipment {
                    slot: DurableEquipmentSlotV1::Weapon,
                },
                pre_item_version: 4,
                post_item_version: 5,
                ledger_event_id: [2; 16],
            },
            DurableDestructionEntryV1::RunMaterial {
                ordinal: 1,
                material_id: "material.core.test".into(),
                destroyed_quantity: 3,
                pre_material_quantity: 3,
                pre_material_version: 8,
                post_material_version: 9,
            },
        ];
        let revision =
            "core-dev.blake3.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let content = DurableDeathContentAuthorityV1 {
            content_revision: revision.into(),
            records_blake3: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .into(),
            assets_blake3: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .into(),
            localization_blake3: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                .into(),
            enabled_items: vec![crate::DurableDeathItemContentAuthorityV1 {
                template_id: "item.core.test".into(),
                echo_signature_tag: Some("signature.core.test".into()),
            }],
        };
        assert_eq!(
            expected_echo_signatures(&items, &content).unwrap(),
            (Some("signature.core.test".into()), None)
        );
        assert!(
            validate_destruction_sources(&items, &materials, &destruction, None, &content).is_ok()
        );
        items.insert(
            [3; 16],
            ItemLock {
                item_uid: [3; 16],
                template_id: "item.core.unplanned".into(),
                content_revision: revision.into(),
                item_version: 1,
                security_state: SECURITY_AT_RISK_PENDING,
                location_kind: 2,
                slot_index: Some(0),
                instance_id: None,
                pickup_id: None,
            },
        );
        assert!(
            validate_destruction_sources(&items, &materials, &destruction, None, &content).is_err()
        );
    }
}
