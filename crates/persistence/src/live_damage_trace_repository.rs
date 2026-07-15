//! Retained, serializable live damage-trace ingestion for `GB-M03-06B`.
//!
//! The contract jointly follows the canonical GDD `DTH-001`/`TECH-021`, the Content Production
//! Spec's promoted Core authority, the Development Roadmap `GB-M03-06` restart gates, and accepted
//! `SPEC-CONFLICT-009`. The server authors complete damage ticks. Normalized payload may be pruned;
//! append-only receipts and changed-payload audits may not.

use sqlx::{PgConnection, Row};

use crate::{
    CORE_WORLD_ASSETS_BLAKE3, CORE_WORLD_LOCALIZATION_BLAKE3, CORE_WORLD_RECORDS_BLAKE3,
    PersistenceError, PostgresPersistence, WIPEABLE_CORE_NAMESPACE,
    is_retryable_transaction_failure,
};

pub const LIVE_DAMAGE_TRACE_WINDOW_TICKS_V1: u64 = 300;
pub const MAX_LIVE_DAMAGE_TRACE_ENTRIES_V1: usize = 4_096;
pub const MAX_LIVE_DAMAGE_TRACE_STATUSES_PER_ENTRY_V1: usize = 32;
const ID_BYTES: usize = 16;
const HASH_BYTES: usize = 32;
const CONTRACT_VERSION: u16 = 1;
const MAX_TRANSACTION_ATTEMPTS: u8 = 3;
const REQUEST_CONTEXT: &str = "gravebound.live-damage-trace.request.v1";
const TICK_CONTEXT: &str = "gravebound.live-damage-trace.tick.v1";
const RESULT_CONTEXT: &str = "gravebound.live-damage-trace.result.v1";
const CONFLICT_CONTEXT: &str = "gravebound.live-damage-trace.conflict.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveDamageTraceContentAuthorityV1 {
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
}

impl LiveDamageTraceContentAuthorityV1 {
    #[must_use]
    pub fn core() -> Self {
        Self {
            records_blake3: CORE_WORLD_RECORDS_BLAKE3.to_owned(),
            assets_blake3: CORE_WORLD_ASSETS_BLAKE3.to_owned(),
            localization_blake3: CORE_WORLD_LOCALIZATION_BLAKE3.to_owned(),
        }
    }

    fn validate(&self) -> Result<(), PersistenceError> {
        if self != &Self::core() {
            return Err(PersistenceError::LiveDamageTraceContentMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveDamageTraceDangerAuthorityV1 {
    pub lineage_id: [u8; ID_BYTES],
    pub restore_point_id: [u8; ID_BYTES],
    pub checkpoint_tick: u64,
}

impl LiveDamageTraceDangerAuthorityV1 {
    fn validate(&self) -> Result<(), PersistenceError> {
        if all_zero(&self.lineage_id)
            || all_zero(&self.restore_point_id)
            || i64::try_from(self.checkpoint_tick).is_err()
        {
            return Err(corrupt());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveDamageTraceCauseV1 {
    DirectHit,
    DamageOverTime,
    Environment,
    Disconnect,
}

impl LiveDamageTraceCauseV1 {
    const fn code(self) -> i16 {
        match self {
            Self::DirectHit => 0,
            Self::DamageOverTime => 1,
            Self::Environment => 2,
            Self::Disconnect => 3,
        }
    }

    fn from_code(code: i16) -> Result<Self, PersistenceError> {
        match code {
            0 => Ok(Self::DirectHit),
            1 => Ok(Self::DamageOverTime),
            2 => Ok(Self::Environment),
            3 => Ok(Self::Disconnect),
            _ => Err(corrupt()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveDamageTraceDamageTypeV1 {
    Physical,
    Veil,
}

impl LiveDamageTraceDamageTypeV1 {
    const fn code(self) -> i16 {
        match self {
            Self::Physical => 0,
            Self::Veil => 1,
        }
    }
    fn from_code(code: i16) -> Result<Self, PersistenceError> {
        match code {
            0 => Ok(Self::Physical),
            1 => Ok(Self::Veil),
            _ => Err(corrupt()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveDamageTraceNetworkStateV1 {
    Connected,
    Degraded,
    LinkLost,
    Reattached,
}
impl LiveDamageTraceNetworkStateV1 {
    const fn code(self) -> i16 {
        match self {
            Self::Connected => 0,
            Self::Degraded => 1,
            Self::LinkLost => 2,
            Self::Reattached => 3,
        }
    }
    fn from_code(code: i16) -> Result<Self, PersistenceError> {
        match code {
            0 => Ok(Self::Connected),
            1 => Ok(Self::Degraded),
            2 => Ok(Self::LinkLost),
            3 => Ok(Self::Reattached),
            _ => Err(corrupt()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveDamageTraceRecallStateV1 {
    Inactive,
    Channeling,
    CompletionPending,
}
impl LiveDamageTraceRecallStateV1 {
    const fn code(self) -> i16 {
        match self {
            Self::Inactive => 0,
            Self::Channeling => 1,
            Self::CompletionPending => 2,
        }
    }
    fn from_code(code: i16) -> Result<Self, PersistenceError> {
        match code {
            0 => Ok(Self::Inactive),
            1 => Ok(Self::Channeling),
            2 => Ok(Self::CompletionPending),
            _ => Err(corrupt()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveDamageTraceStatusV1 {
    pub status_ordinal: u8,
    pub status_id: String,
    pub remaining_ticks: u32,
    pub stack_count: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveDamageTraceEntryV1 {
    pub event_ordinal: u32,
    pub cause: LiveDamageTraceCauseV1,
    pub source_content_id: String,
    pub source_entity_id: Option<[u8; ID_BYTES]>,
    pub source_sim_entity_id: Option<u64>,
    pub pattern_id: Option<String>,
    pub attack_id: String,
    pub raw_damage: u32,
    pub final_damage: u32,
    pub damage_type: LiveDamageTraceDamageTypeV1,
    pub pre_health: u32,
    pub post_health: u32,
    pub source_x_milli_tiles: i32,
    pub source_y_milli_tiles: i32,
    pub network_state: LiveDamageTraceNetworkStateV1,
    pub recall_state: LiveDamageTraceRecallStateV1,
    pub lethal: bool,
    pub statuses: Vec<LiveDamageTraceStatusV1>,
}

impl LiveDamageTraceEntryV1 {
    fn validate(&self) -> Result<(), PersistenceError> {
        if i32::try_from(self.event_ordinal).is_err()
            || !stable_id(&self.source_content_id)
            || self.source_entity_id == Some([0; ID_BYTES])
            || self.source_entity_id.is_some() != self.source_sim_entity_id.is_some()
            || self.source_sim_entity_id == Some(0)
            || self.pattern_id.as_deref().is_some_and(|id| !stable_id(id))
            || !stable_id(&self.attack_id)
            || self.pre_health == 0
            || i32::try_from(self.raw_damage).is_err()
            || i32::try_from(self.final_damage).is_err()
            || i32::try_from(self.pre_health).is_err()
            || i32::try_from(self.post_health).is_err()
            || self.post_health != self.pre_health.saturating_sub(self.final_damage)
            || self.lethal != (self.post_health == 0)
            || self.statuses.len() > MAX_LIVE_DAMAGE_TRACE_STATUSES_PER_ENTRY_V1
        {
            return Err(corrupt());
        }
        let mut previous: Option<&str> = None;
        for (index, status) in self.statuses.iter().enumerate() {
            if usize::from(status.status_ordinal) != index
                || !stable_id(&status.status_id)
                || status.remaining_ticks > 108_000
                || status.stack_count == 0
                || status.stack_count > 255
                || previous.is_some_and(|value| value.as_bytes() >= status.status_id.as_bytes())
            {
                return Err(corrupt());
            }
            previous = Some(&status.status_id);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveDamageTraceTickCommandV1 {
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub trace_tick_id: [u8; ID_BYTES],
    pub expected_character_version: u64,
    pub event_tick: u64,
    pub danger: LiveDamageTraceDangerAuthorityV1,
    pub content: LiveDamageTraceContentAuthorityV1,
    pub entries: Vec<LiveDamageTraceEntryV1>,
    pub issued_at_unix_ms: u64,
}

impl LiveDamageTraceTickCommandV1 {
    fn validate(&self) -> Result<(), PersistenceError> {
        if all_zero(&self.account_id)
            || all_zero(&self.character_id)
            || all_zero(&self.trace_tick_id)
            || self.expected_character_version == 0
            || self.event_tick == 0
            || self.event_tick < self.danger.checkpoint_tick
            || self.entries.is_empty()
            || self.entries.len() > MAX_LIVE_DAMAGE_TRACE_ENTRIES_V1
            || self.issued_at_unix_ms == 0
            || i64::try_from(self.expected_character_version).is_err()
            || i64::try_from(self.event_tick).is_err()
            || i64::try_from(self.issued_at_unix_ms).is_err()
            || self
                .entries
                .iter()
                .map(|entry| entry.statuses.len())
                .sum::<usize>()
                > MAX_LIVE_DAMAGE_TRACE_ENTRIES_V1
        {
            return Err(corrupt());
        }
        self.danger.validate()?;
        self.content.validate()?;
        let mut lethal = 0;
        let mut previous_ordinal = None;
        for (index, entry) in self.entries.iter().enumerate() {
            entry.validate()?;
            if previous_ordinal.is_some_and(|previous| previous >= entry.event_ordinal) {
                return Err(corrupt());
            }
            previous_ordinal = Some(entry.event_ordinal);
            if entry.lethal {
                lethal += 1;
            }
            if lethal > 0 && index + 1 != self.entries.len() {
                return Err(corrupt());
            }
        }
        if lethal > 1 {
            return Err(corrupt());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveDamageTraceTickRequestV1 {
    pub command: LiveDamageTraceTickCommandV1,
    pub request_hash: [u8; HASH_BYTES],
}

impl LiveDamageTraceTickRequestV1 {
    pub fn seal(command: LiveDamageTraceTickCommandV1) -> Result<Self, PersistenceError> {
        command.validate()?;
        let request_hash = request_hash(&command)?;
        Ok(Self {
            command,
            request_hash,
        })
    }
    fn validate(&self) -> Result<(), PersistenceError> {
        self.command.validate()?;
        if all_zero(&self.request_hash) || self.request_hash != request_hash(&self.command)? {
            return Err(corrupt());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredLiveDamageTraceTickV1 {
    pub contract_version: u16,
    pub command: LiveDamageTraceTickCommandV1,
    pub request_hash: [u8; HASH_BYTES],
    pub tick_digest: [u8; HASH_BYTES],
    pub result_digest: [u8; HASH_BYTES],
    pub committed_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveDamageTraceTickTransactionV1 {
    Committed(StoredLiveDamageTraceTickV1),
    Replayed(StoredLiveDamageTraceTickV1),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredLiveDamageTraceSnapshotEntryV1 {
    pub event_tick: u64,
    pub entry: LiveDamageTraceEntryV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredLiveDamageTraceSnapshotV1 {
    pub danger: LiveDamageTraceDangerAuthorityV1,
    pub content: LiveDamageTraceContentAuthorityV1,
    pub through_tick: u64,
    pub entries: Vec<StoredLiveDamageTraceSnapshotEntryV1>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Receipt {
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
    trace_tick_id: [u8; ID_BYTES],
    expected_character_version: u64,
    danger: LiveDamageTraceDangerAuthorityV1,
    event_tick: u64,
    entry_count: usize,
    status_count: usize,
    lethal_count: usize,
    content: LiveDamageTraceContentAuthorityV1,
    request_hash: [u8; HASH_BYTES],
    tick_digest: [u8; HASH_BYTES],
    result_digest: [u8; HASH_BYTES],
    committed_at_unix_ms: u64,
}

impl PostgresPersistence {
    pub async fn transact_live_damage_trace_tick_v1(
        &self,
        request: &LiveDamageTraceTickRequestV1,
    ) -> Result<LiveDamageTraceTickTransactionV1, PersistenceError> {
        request.validate()?;
        if request.command.entries.iter().any(|entry| entry.lethal) {
            return Err(PersistenceError::LiveDamageTraceTerminalStagingRequired);
        }
        for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self.transact_live_damage_trace_tick_v1_once(request).await {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS
                        && is_retryable_transaction_failure(&error) => {}
                result => return result,
            }
        }
        unreachable!("bounded trace transaction loop always returns")
    }

    #[allow(
        clippy::too_many_lines,
        reason = "account-first replay, authority locks, complete-tick pruning, and commit stay auditable"
    )]
    async fn transact_live_damage_trace_tick_v1_once(
        &self,
        request: &LiveDamageTraceTickRequestV1,
    ) -> Result<LiveDamageTraceTickTransactionV1, PersistenceError> {
        let command = &request.command;
        let mut tx = self.begin_transaction().await?;
        let selected = lock_account(tx.connection(), command.account_id).await?;
        if let Some(receipt) =
            load_receipt(tx.connection(), command.account_id, command.trace_tick_id).await?
        {
            if receipt.request_hash != request.request_hash {
                insert_conflict(tx.connection(), request, &receipt).await?;
                tx.commit().await?;
                return Err(PersistenceError::LiveDamageTraceIdempotencyConflict);
            }
            validate_receipt_matches(&receipt, command)?;
            let stored = stored_from_receipt(&receipt, command.clone(), request.request_hash)?;
            tx.rollback().await?;
            return Ok(LiveDamageTraceTickTransactionV1::Replayed(stored));
        }
        if selected != Some(command.character_id) {
            return Err(PersistenceError::LiveDamageTraceBindingMismatch);
        }
        let version = lock_character(tx.connection(), command).await?;
        if version != command.expected_character_version {
            return Err(PersistenceError::LiveDamageTraceCharacterVersionMismatch {
                expected: command.expected_character_version,
                actual: version,
            });
        }
        validate_active_danger(
            tx.connection(),
            command.account_id,
            command.character_id,
            &command.danger,
            &command.content,
        )
        .await?;
        let latest = load_latest_receipt(
            tx.connection(),
            command.account_id,
            command.character_id,
            &command.danger,
        )
        .await?;
        if let Some(latest) = &latest {
            if !receipt_matches_danger_root(latest, &command.danger) {
                return Err(PersistenceError::LiveDamageTraceBindingMismatch);
            }
            if latest.lethal_count != 0 {
                return Err(PersistenceError::LiveDamageTraceTerminal);
            }
            if command.event_tick <= latest.event_tick {
                return Err(PersistenceError::LiveDamageTraceTickOrder {
                    previous: latest.event_tick,
                    attempted: command.event_tick,
                });
            }
        }
        let committed_at_unix_ms = transaction_timestamp_ms(tx.connection()).await?;
        if command.issued_at_unix_ms > committed_at_unix_ms {
            return Err(corrupt());
        }
        let tick_digest = tick_digest(command)?;
        let receipt = receipt_from_command(
            command,
            request.request_hash,
            tick_digest,
            committed_at_unix_ms,
        )?;
        let cutoff = command
            .event_tick
            .saturating_sub(LIVE_DAMAGE_TRACE_WINDOW_TICKS_V1);
        sqlx::query("DELETE FROM character_live_damage_trace_ticks_v1 WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND lineage_id=$4 AND restore_point_id=$5 AND event_tick<$6")
            .bind(WIPEABLE_CORE_NAMESPACE).bind(command.account_id.as_slice()).bind(command.character_id.as_slice())
            .bind(command.danger.lineage_id.as_slice()).bind(command.danger.restore_point_id.as_slice()).bind(i64_value(cutoff)?).execute(tx.connection()).await?;
        let retained_entries: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM character_live_damage_trace_entries_v1 \
             WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 \
               AND lineage_id=$4 AND restore_point_id=$5",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(command.account_id.as_slice())
        .bind(command.character_id.as_slice())
        .bind(command.danger.lineage_id.as_slice())
        .bind(command.danger.restore_point_id.as_slice())
        .fetch_one(tx.connection())
        .await?;
        if usize::try_from(retained_entries)
            .ok()
            .and_then(|count| count.checked_add(command.entries.len()))
            .is_none_or(|count| count > MAX_LIVE_DAMAGE_TRACE_ENTRIES_V1)
        {
            return Err(PersistenceError::LiveDamageTraceCapacityExceeded);
        }
        insert_payload(tx.connection(), command, request.request_hash, tick_digest).await?;
        insert_receipt(tx.connection(), &receipt, command.issued_at_unix_ms).await?;
        sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
            .execute(tx.connection())
            .await?;
        tx.commit().await?;
        Ok(LiveDamageTraceTickTransactionV1::Committed(
            StoredLiveDamageTraceTickV1 {
                contract_version: CONTRACT_VERSION,
                command: command.clone(),
                request_hash: request.request_hash,
                tick_digest,
                result_digest: receipt.result_digest,
                committed_at_unix_ms,
            },
        ))
    }

    pub async fn load_live_damage_trace_snapshot_v1(
        &self,
        account_id: [u8; ID_BYTES],
        character_id: [u8; ID_BYTES],
    ) -> Result<StoredLiveDamageTraceSnapshotV1, PersistenceError> {
        if all_zero(&account_id) || all_zero(&character_id) {
            return Err(corrupt());
        }
        let mut tx = self.begin_transaction().await?;
        if lock_account(tx.connection(), account_id).await? != Some(character_id) {
            return Err(PersistenceError::LiveDamageTraceBindingMismatch);
        }
        let _ = lock_character_by_id(tx.connection(), account_id, character_id).await?;
        let authority = load_current_danger(tx.connection(), account_id, character_id).await?;
        let latest = load_latest_receipt(tx.connection(), account_id, character_id, &authority)
            .await?
            .ok_or(PersistenceError::LiveDamageTraceNotFound)?;
        if latest.danger.lineage_id != authority.lineage_id
            || latest.danger.restore_point_id != authority.restore_point_id
            || latest.content != LiveDamageTraceContentAuthorityV1::core()
        {
            return Err(corrupt());
        }
        validate_active_danger(
            tx.connection(),
            account_id,
            character_id,
            &authority,
            &latest.content,
        )
        .await?;
        let entries = load_snapshot_entries(
            tx.connection(),
            account_id,
            character_id,
            &authority,
            latest.event_tick,
        )
        .await?;
        let receipts = load_window_receipts(
            tx.connection(),
            account_id,
            character_id,
            &authority,
            latest.event_tick,
        )
        .await?;
        validate_snapshot_graph(&receipts, &entries, &latest)?;
        tx.rollback().await?;
        Ok(StoredLiveDamageTraceSnapshotV1 {
            danger: authority,
            content: latest.content,
            through_tick: latest.event_tick,
            entries,
        })
    }
}

async fn lock_account(
    connection: &mut PgConnection,
    account: [u8; ID_BYTES],
) -> Result<Option<[u8; ID_BYTES]>, PersistenceError> {
    let row = sqlx::query("SELECT selected_character_id FROM accounts WHERE namespace_id=$1 AND account_id=$2 FOR UPDATE").bind(WIPEABLE_CORE_NAMESPACE).bind(account.as_slice()).fetch_optional(connection).await?.ok_or(PersistenceError::LiveDamageTraceOwnerNotFound)?;
    row.try_get::<Option<Vec<u8>>, _>("selected_character_id")?
        .map(exact_id)
        .transpose()
}

async fn lock_character(
    connection: &mut PgConnection,
    command: &LiveDamageTraceTickCommandV1,
) -> Result<u64, PersistenceError> {
    lock_character_by_id(connection, command.account_id, command.character_id).await
}
async fn lock_character_by_id(
    connection: &mut PgConnection,
    account: [u8; ID_BYTES],
    character: [u8; ID_BYTES],
) -> Result<u64, PersistenceError> {
    let row = sqlx::query("SELECT roster_ordinal,life_state,security_state,character_state_version FROM characters WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE")
        .bind(WIPEABLE_CORE_NAMESPACE).bind(account.as_slice()).bind(character.as_slice()).fetch_optional(connection).await?.ok_or(PersistenceError::LiveDamageTraceOwnerNotFound)?;
    if row.try_get::<Option<i16>, _>("roster_ordinal")?.is_none()
        || row.try_get::<i16, _>("life_state")? != 0
        || row.try_get::<i16, _>("security_state")? != 0
    {
        return Err(PersistenceError::LiveDamageTraceBindingMismatch);
    }
    positive_u64(row.try_get("character_state_version")?)
}

async fn validate_active_danger(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
    danger: &LiveDamageTraceDangerAuthorityV1,
    content: &LiveDamageTraceContentAuthorityV1,
) -> Result<(), PersistenceError> {
    let world = sqlx::query("SELECT location_kind,instance_lineage_id,entry_restore_point_id FROM character_world_locations WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE")
        .bind(WIPEABLE_CORE_NAMESPACE).bind(account_id.as_slice()).bind(character_id.as_slice()).fetch_optional(&mut *connection).await?.ok_or(PersistenceError::LiveDamageTraceBindingMismatch)?;
    if world.try_get::<i16, _>("location_kind")? != 2
        || optional_id(world.try_get("instance_lineage_id")?)? != Some(danger.lineage_id)
        || optional_id(world.try_get("entry_restore_point_id")?)? != Some(danger.restore_point_id)
    {
        return Err(PersistenceError::LiveDamageTraceBindingMismatch);
    }
    let root = sqlx::query("SELECT restore_state,records_blake3,assets_blake3,localization_blake3 FROM character_entry_restore_points WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND lineage_id=$4 AND restore_point_id=$5 FOR UPDATE")
        .bind(WIPEABLE_CORE_NAMESPACE).bind(account_id.as_slice()).bind(character_id.as_slice()).bind(danger.lineage_id.as_slice()).bind(danger.restore_point_id.as_slice()).fetch_optional(&mut *connection).await?.ok_or(PersistenceError::LiveDamageTraceBindingMismatch)?;
    let lineage = sqlx::query("SELECT lineage_state,records_blake3,assets_blake3,localization_blake3 FROM character_instance_lineages WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND lineage_id=$4 FOR UPDATE")
        .bind(WIPEABLE_CORE_NAMESPACE).bind(account_id.as_slice()).bind(character_id.as_slice()).bind(danger.lineage_id.as_slice()).fetch_optional(&mut *connection).await?.ok_or(PersistenceError::LiveDamageTraceBindingMismatch)?;
    let checkpoint: Option<i64> = sqlx::query_scalar("SELECT checkpoint_tick FROM character_danger_checkpoints WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND lineage_id=$4 AND records_blake3=$5 AND assets_blake3=$6 AND localization_blake3=$7 FOR UPDATE")
        .bind(WIPEABLE_CORE_NAMESPACE).bind(account_id.as_slice()).bind(character_id.as_slice()).bind(danger.lineage_id.as_slice()).bind(&content.records_blake3).bind(&content.assets_blake3).bind(&content.localization_blake3).fetch_optional(&mut *connection).await?;
    let exact_content = |row: &sqlx::postgres::PgRow| -> Result<bool, PersistenceError> {
        Ok(
            row.try_get::<String, _>("records_blake3")? == content.records_blake3
                && row.try_get::<String, _>("assets_blake3")? == content.assets_blake3
                && row.try_get::<String, _>("localization_blake3")? == content.localization_blake3,
        )
    };
    if root.try_get::<i16, _>("restore_state")? != 0
        || lineage.try_get::<i16, _>("lineage_state")? != 0
        || !exact_content(&root)?
        || !exact_content(&lineage)?
        || checkpoint.and_then(|v| u64::try_from(v).ok()) != Some(danger.checkpoint_tick)
    {
        return Err(PersistenceError::LiveDamageTraceBindingMismatch);
    }
    Ok(())
}

async fn load_current_danger(
    connection: &mut PgConnection,
    account: [u8; ID_BYTES],
    character: [u8; ID_BYTES],
) -> Result<LiveDamageTraceDangerAuthorityV1, PersistenceError> {
    let row = sqlx::query("SELECT location_kind,instance_lineage_id,entry_restore_point_id FROM character_world_locations WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE")
        .bind(WIPEABLE_CORE_NAMESPACE).bind(account.as_slice()).bind(character.as_slice()).fetch_optional(&mut *connection).await?.ok_or(PersistenceError::LiveDamageTraceBindingMismatch)?;
    if row.try_get::<i16, _>("location_kind")? != 2 {
        return Err(PersistenceError::LiveDamageTraceBindingMismatch);
    }
    let lineage = optional_id(row.try_get("instance_lineage_id")?)?
        .ok_or(PersistenceError::LiveDamageTraceBindingMismatch)?;
    let restore = optional_id(row.try_get("entry_restore_point_id")?)?
        .ok_or(PersistenceError::LiveDamageTraceBindingMismatch)?;
    let checkpoint: i64 = sqlx::query_scalar("SELECT checkpoint_tick FROM character_danger_checkpoints WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND lineage_id=$4 AND records_blake3=$5 AND assets_blake3=$6 AND localization_blake3=$7 FOR UPDATE")
        .bind(WIPEABLE_CORE_NAMESPACE).bind(account.as_slice()).bind(character.as_slice()).bind(lineage.as_slice()).bind(CORE_WORLD_RECORDS_BLAKE3).bind(CORE_WORLD_ASSETS_BLAKE3).bind(CORE_WORLD_LOCALIZATION_BLAKE3).fetch_optional(connection).await?.ok_or(PersistenceError::LiveDamageTraceBindingMismatch)?;
    Ok(LiveDamageTraceDangerAuthorityV1 {
        lineage_id: lineage,
        restore_point_id: restore,
        checkpoint_tick: nonnegative_u64(checkpoint)?,
    })
}

async fn insert_payload(
    connection: &mut PgConnection,
    command: &LiveDamageTraceTickCommandV1,
    request_hash: [u8; HASH_BYTES],
    digest: [u8; HASH_BYTES],
) -> Result<(), PersistenceError> {
    sqlx::query("INSERT INTO character_live_damage_trace_ticks_v1(namespace_id,account_id,character_id,lineage_id,restore_point_id,trace_tick_id,event_tick,entry_count,records_blake3,assets_blake3,localization_blake3,request_hash,tick_digest) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)")
        .bind(WIPEABLE_CORE_NAMESPACE).bind(command.account_id.as_slice()).bind(command.character_id.as_slice()).bind(command.danger.lineage_id.as_slice()).bind(command.danger.restore_point_id.as_slice()).bind(command.trace_tick_id.as_slice()).bind(i64_value(command.event_tick)?).bind(i16::try_from(command.entries.len()).map_err(corrupt_conversion)?).bind(&command.content.records_blake3).bind(&command.content.assets_blake3).bind(&command.content.localization_blake3).bind(request_hash.as_slice()).bind(digest.as_slice()).execute(&mut *connection).await?;
    for entry in &command.entries {
        let entry_digest = entry_digest(command.event_tick, entry)?;
        sqlx::query("INSERT INTO character_live_damage_trace_entries_v1(namespace_id,account_id,character_id,lineage_id,restore_point_id,trace_tick_id,event_tick,event_ordinal,cause_kind,source_content_id,source_entity_id,source_sim_entity_id,pattern_id,attack_id,raw_damage,final_damage,damage_type,pre_health,post_health,source_x_milli_tiles,source_y_milli_tiles,status_count,network_state,recall_state,lethal,entry_digest) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26)")
            .bind(WIPEABLE_CORE_NAMESPACE).bind(command.account_id.as_slice()).bind(command.character_id.as_slice()).bind(command.danger.lineage_id.as_slice()).bind(command.danger.restore_point_id.as_slice()).bind(command.trace_tick_id.as_slice()).bind(i64_value(command.event_tick)?).bind(i32::try_from(entry.event_ordinal).map_err(corrupt_conversion)?).bind(entry.cause.code()).bind(&entry.source_content_id).bind(entry.source_entity_id.as_ref().map(<[u8; ID_BYTES]>::as_slice)).bind(entry.source_sim_entity_id.map(u64::to_le_bytes).as_ref().map(<[u8; 8]>::as_slice)).bind(&entry.pattern_id).bind(&entry.attack_id).bind(i32::try_from(entry.raw_damage).map_err(corrupt_conversion)?).bind(i32::try_from(entry.final_damage).map_err(corrupt_conversion)?).bind(entry.damage_type.code()).bind(i32::try_from(entry.pre_health).map_err(corrupt_conversion)?).bind(i32::try_from(entry.post_health).map_err(corrupt_conversion)?).bind(entry.source_x_milli_tiles).bind(entry.source_y_milli_tiles).bind(i16::try_from(entry.statuses.len()).map_err(corrupt_conversion)?).bind(entry.network_state.code()).bind(entry.recall_state.code()).bind(entry.lethal).bind(entry_digest.as_slice()).execute(&mut *connection).await?;
        for status in &entry.statuses {
            sqlx::query("INSERT INTO character_live_damage_trace_statuses_v1(namespace_id,account_id,character_id,lineage_id,restore_point_id,trace_tick_id,event_tick,event_ordinal,status_ordinal,status_id,remaining_ticks,stack_count) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
                .bind(WIPEABLE_CORE_NAMESPACE).bind(command.account_id.as_slice()).bind(command.character_id.as_slice()).bind(command.danger.lineage_id.as_slice()).bind(command.danger.restore_point_id.as_slice()).bind(command.trace_tick_id.as_slice()).bind(i64_value(command.event_tick)?).bind(i32::try_from(entry.event_ordinal).map_err(corrupt_conversion)?).bind(i16::from(status.status_ordinal)).bind(&status.status_id).bind(i32::try_from(status.remaining_ticks).map_err(corrupt_conversion)?).bind(i16::try_from(status.stack_count).map_err(corrupt_conversion)?).execute(&mut *connection).await?;
        }
    }
    Ok(())
}

async fn insert_receipt(
    connection: &mut PgConnection,
    receipt: &Receipt,
    issued_at_unix_ms: u64,
) -> Result<(), PersistenceError> {
    sqlx::query("INSERT INTO character_live_damage_trace_ingest_receipts_v1(namespace_id,account_id,character_id,trace_tick_id,contract_version,expected_character_version,lineage_id,restore_point_id,checkpoint_tick,event_tick,entry_count,status_count,lethal_count,records_blake3,assets_blake3,localization_blake3,request_hash,tick_digest,result_digest,issued_at,committed_at) VALUES($1,$2,$3,$4,1,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,to_timestamp($19::double precision/1000.0),to_timestamp($20::double precision/1000.0))")
        .bind(WIPEABLE_CORE_NAMESPACE).bind(receipt.account_id.as_slice()).bind(receipt.character_id.as_slice()).bind(receipt.trace_tick_id.as_slice()).bind(i64_value(receipt.expected_character_version)?).bind(receipt.danger.lineage_id.as_slice()).bind(receipt.danger.restore_point_id.as_slice()).bind(i64_value(receipt.danger.checkpoint_tick)?).bind(i64_value(receipt.event_tick)?).bind(i16::try_from(receipt.entry_count).map_err(corrupt_conversion)?).bind(i16::try_from(receipt.status_count).map_err(corrupt_conversion)?).bind(i16::try_from(receipt.lethal_count).map_err(corrupt_conversion)?).bind(&receipt.content.records_blake3).bind(&receipt.content.assets_blake3).bind(&receipt.content.localization_blake3).bind(receipt.request_hash.as_slice()).bind(receipt.tick_digest.as_slice()).bind(receipt.result_digest.as_slice()).bind(i64_value(issued_at_unix_ms)?).bind(i64_value(receipt.committed_at_unix_ms)?).execute(connection).await?;
    Ok(())
}

async fn load_receipt(
    connection: &mut PgConnection,
    account: [u8; ID_BYTES],
    tick_id: [u8; ID_BYTES],
) -> Result<Option<Receipt>, PersistenceError> {
    let row = sqlx::query("SELECT account_id,character_id,trace_tick_id,expected_character_version,lineage_id,restore_point_id,checkpoint_tick,event_tick,entry_count,status_count,lethal_count,records_blake3,assets_blake3,localization_blake3,request_hash,tick_digest,result_digest,CAST(EXTRACT(EPOCH FROM committed_at)*1000 AS BIGINT) committed_at_ms FROM character_live_damage_trace_ingest_receipts_v1 WHERE namespace_id=$1 AND account_id=$2 AND trace_tick_id=$3 FOR UPDATE")
        .bind(WIPEABLE_CORE_NAMESPACE).bind(account.as_slice()).bind(tick_id.as_slice()).fetch_optional(connection).await?;
    row.map(|row| decode_receipt(&row)).transpose()
}

async fn load_latest_receipt(
    connection: &mut PgConnection,
    account: [u8; ID_BYTES],
    character: [u8; ID_BYTES],
    danger: &LiveDamageTraceDangerAuthorityV1,
) -> Result<Option<Receipt>, PersistenceError> {
    let row = sqlx::query("SELECT account_id,character_id,trace_tick_id,expected_character_version,lineage_id,restore_point_id,checkpoint_tick,event_tick,entry_count,status_count,lethal_count,records_blake3,assets_blake3,localization_blake3,request_hash,tick_digest,result_digest,CAST(EXTRACT(EPOCH FROM committed_at)*1000 AS BIGINT) committed_at_ms FROM character_live_damage_trace_ingest_receipts_v1 WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND lineage_id=$4 AND restore_point_id=$5 ORDER BY event_tick DESC LIMIT 1 FOR UPDATE")
        .bind(WIPEABLE_CORE_NAMESPACE).bind(account.as_slice()).bind(character.as_slice()).bind(danger.lineage_id.as_slice()).bind(danger.restore_point_id.as_slice()).fetch_optional(connection).await?;
    row.map(|row| decode_receipt(&row)).transpose()
}

async fn load_window_receipts(
    connection: &mut PgConnection,
    account: [u8; ID_BYTES],
    character: [u8; ID_BYTES],
    danger: &LiveDamageTraceDangerAuthorityV1,
    through_tick: u64,
) -> Result<Vec<Receipt>, PersistenceError> {
    let cutoff = through_tick.saturating_sub(LIVE_DAMAGE_TRACE_WINDOW_TICKS_V1);
    let rows = sqlx::query("SELECT account_id,character_id,trace_tick_id,expected_character_version,lineage_id,restore_point_id,checkpoint_tick,event_tick,entry_count,status_count,lethal_count,records_blake3,assets_blake3,localization_blake3,request_hash,tick_digest,result_digest,CAST(EXTRACT(EPOCH FROM committed_at)*1000 AS BIGINT) committed_at_ms FROM character_live_damage_trace_ingest_receipts_v1 WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND lineage_id=$4 AND restore_point_id=$5 AND event_tick>=$6 AND event_tick<=$7 ORDER BY event_tick FOR UPDATE")
        .bind(WIPEABLE_CORE_NAMESPACE).bind(account.as_slice()).bind(character.as_slice()).bind(danger.lineage_id.as_slice()).bind(danger.restore_point_id.as_slice()).bind(i64_value(cutoff)?).bind(i64_value(through_tick)?).fetch_all(connection).await?;
    rows.iter().map(decode_receipt).collect()
}

fn validate_snapshot_graph(
    receipts: &[Receipt],
    entries: &[StoredLiveDamageTraceSnapshotEntryV1],
    latest: &Receipt,
) -> Result<(), PersistenceError> {
    if receipts.last() != Some(latest) {
        return Err(corrupt());
    }
    let mut entry_index = 0;
    for receipt in receipts {
        let start = entry_index;
        while entries
            .get(entry_index)
            .is_some_and(|entry| entry.event_tick == receipt.event_tick)
        {
            entry_index += 1;
        }
        let tick_entries = entries[start..entry_index]
            .iter()
            .map(|entry| entry.entry.clone())
            .collect::<Vec<_>>();
        let statuses: usize = tick_entries.iter().map(|entry| entry.statuses.len()).sum();
        let lethal = tick_entries.iter().filter(|entry| entry.lethal).count();
        if tick_entries.len() != receipt.entry_count
            || statuses != receipt.status_count
            || lethal != receipt.lethal_count
            || tick_digest_entries(receipt.event_tick, &tick_entries)? != receipt.tick_digest
        {
            return Err(corrupt());
        }
    }
    if entry_index != entries.len() {
        return Err(corrupt());
    }
    Ok(())
}

fn decode_receipt(row: &sqlx::postgres::PgRow) -> Result<Receipt, PersistenceError> {
    let receipt = Receipt {
        account_id: exact_id(row.try_get("account_id")?)?,
        character_id: exact_id(row.try_get("character_id")?)?,
        trace_tick_id: exact_id(row.try_get("trace_tick_id")?)?,
        expected_character_version: positive_u64(row.try_get("expected_character_version")?)?,
        danger: LiveDamageTraceDangerAuthorityV1 {
            lineage_id: exact_id(row.try_get("lineage_id")?)?,
            restore_point_id: exact_id(row.try_get("restore_point_id")?)?,
            checkpoint_tick: nonnegative_u64(row.try_get("checkpoint_tick")?)?,
        },
        event_tick: positive_u64(row.try_get("event_tick")?)?,
        entry_count: positive_usize(row.try_get("entry_count")?)?,
        status_count: nonnegative_usize(row.try_get("status_count")?)?,
        lethal_count: nonnegative_usize(row.try_get("lethal_count")?)?,
        content: LiveDamageTraceContentAuthorityV1 {
            records_blake3: row.try_get("records_blake3")?,
            assets_blake3: row.try_get("assets_blake3")?,
            localization_blake3: row.try_get("localization_blake3")?,
        },
        request_hash: exact_hash(row.try_get("request_hash")?)?,
        tick_digest: exact_hash(row.try_get("tick_digest")?)?,
        result_digest: exact_hash(row.try_get("result_digest")?)?,
        committed_at_unix_ms: positive_u64(row.try_get("committed_at_ms")?)?,
    };
    validate_receipt(&receipt)?;
    Ok(receipt)
}

fn receipt_from_command(
    command: &LiveDamageTraceTickCommandV1,
    request_hash: [u8; HASH_BYTES],
    tick_digest: [u8; HASH_BYTES],
    committed_at_unix_ms: u64,
) -> Result<Receipt, PersistenceError> {
    let mut receipt = Receipt {
        account_id: command.account_id,
        character_id: command.character_id,
        trace_tick_id: command.trace_tick_id,
        expected_character_version: command.expected_character_version,
        danger: command.danger.clone(),
        event_tick: command.event_tick,
        entry_count: command.entries.len(),
        status_count: command
            .entries
            .iter()
            .map(|entry| entry.statuses.len())
            .sum(),
        lethal_count: command.entries.iter().filter(|entry| entry.lethal).count(),
        content: command.content.clone(),
        request_hash,
        tick_digest,
        result_digest: [0; HASH_BYTES],
        committed_at_unix_ms,
    };
    receipt.result_digest = receipt_result_digest(&receipt)?;
    validate_receipt(&receipt)?;
    Ok(receipt)
}

fn validate_receipt(receipt: &Receipt) -> Result<(), PersistenceError> {
    receipt.danger.validate()?;
    receipt.content.validate()?;
    if all_zero(&receipt.account_id)
        || all_zero(&receipt.character_id)
        || all_zero(&receipt.trace_tick_id)
        || receipt.expected_character_version == 0
        || receipt.event_tick == 0
        || receipt.event_tick < receipt.danger.checkpoint_tick
        || !(1..=MAX_LIVE_DAMAGE_TRACE_ENTRIES_V1).contains(&receipt.entry_count)
        || receipt.status_count > MAX_LIVE_DAMAGE_TRACE_ENTRIES_V1
        || receipt.lethal_count > 1
        || all_zero(&receipt.request_hash)
        || all_zero(&receipt.tick_digest)
        || all_zero(&receipt.result_digest)
        || receipt.committed_at_unix_ms == 0
        || receipt.result_digest != receipt_result_digest(receipt)?
    {
        return Err(corrupt());
    }
    Ok(())
}

fn receipt_result_digest(receipt: &Receipt) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let mut h = blake3::Hasher::new();
    field(&mut h, RESULT_CONTEXT.as_bytes())?;
    field(&mut h, &CONTRACT_VERSION.to_le_bytes())?;
    field(&mut h, &receipt.account_id)?;
    field(&mut h, &receipt.character_id)?;
    field(&mut h, &receipt.trace_tick_id)?;
    field(&mut h, &receipt.expected_character_version.to_le_bytes())?;
    field(&mut h, &receipt.danger.lineage_id)?;
    field(&mut h, &receipt.danger.restore_point_id)?;
    field(&mut h, &receipt.danger.checkpoint_tick.to_le_bytes())?;
    field(&mut h, &receipt.event_tick.to_le_bytes())?;
    field(
        &mut h,
        &u64::try_from(receipt.entry_count)
            .map_err(corrupt_conversion)?
            .to_le_bytes(),
    )?;
    field(
        &mut h,
        &u64::try_from(receipt.status_count)
            .map_err(corrupt_conversion)?
            .to_le_bytes(),
    )?;
    field(
        &mut h,
        &u64::try_from(receipt.lethal_count)
            .map_err(corrupt_conversion)?
            .to_le_bytes(),
    )?;
    field(&mut h, receipt.content.records_blake3.as_bytes())?;
    field(&mut h, receipt.content.assets_blake3.as_bytes())?;
    field(&mut h, receipt.content.localization_blake3.as_bytes())?;
    field(&mut h, &receipt.request_hash)?;
    field(&mut h, &receipt.tick_digest)?;
    field(&mut h, &receipt.committed_at_unix_ms.to_le_bytes())?;
    Ok(*h.finalize().as_bytes())
}

fn validate_receipt_matches(
    receipt: &Receipt,
    command: &LiveDamageTraceTickCommandV1,
) -> Result<(), PersistenceError> {
    let statuses: usize = command
        .entries
        .iter()
        .map(|entry| entry.statuses.len())
        .sum();
    let lethal = command.entries.iter().filter(|entry| entry.lethal).count();
    if receipt.character_id != command.character_id
        || receipt.account_id != command.account_id
        || receipt.trace_tick_id != command.trace_tick_id
        || receipt.expected_character_version != command.expected_character_version
        || receipt.danger != command.danger
        || receipt.event_tick != command.event_tick
        || receipt.entry_count != command.entries.len()
        || receipt.status_count != statuses
        || receipt.lethal_count != lethal
        || receipt.content != command.content
        || receipt.tick_digest != tick_digest(command)?
    {
        return Err(corrupt());
    }
    Ok(())
}

fn receipt_matches_danger_root(
    receipt: &Receipt,
    danger: &LiveDamageTraceDangerAuthorityV1,
) -> bool {
    receipt.danger.lineage_id == danger.lineage_id
        && receipt.danger.restore_point_id == danger.restore_point_id
}

fn stored_from_receipt(
    receipt: &Receipt,
    command: LiveDamageTraceTickCommandV1,
    request_hash: [u8; HASH_BYTES],
) -> Result<StoredLiveDamageTraceTickV1, PersistenceError> {
    validate_receipt_matches(receipt, &command)?;
    Ok(StoredLiveDamageTraceTickV1 {
        contract_version: CONTRACT_VERSION,
        command,
        request_hash,
        tick_digest: receipt.tick_digest,
        result_digest: receipt.result_digest,
        committed_at_unix_ms: receipt.committed_at_unix_ms,
    })
}

async fn insert_conflict(
    connection: &mut PgConnection,
    request: &LiveDamageTraceTickRequestV1,
    stored: &Receipt,
) -> Result<(), PersistenceError> {
    let audit = conflict_id(request)?;
    sqlx::query("INSERT INTO character_live_damage_trace_conflict_audits_v1(namespace_id,account_id,character_id,trace_tick_id,attempted_character_id,audit_id,conflict_code,stored_request_hash,attempted_request_hash,observed_character_version,attempted_issued_at) SELECT $1,$2,$3,$4,$5,$6,0,$7,$8,character_state_version,to_timestamp($9::double precision/1000.0) FROM characters WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 ON CONFLICT(namespace_id,account_id,trace_tick_id,attempted_request_hash) DO NOTHING")
        .bind(WIPEABLE_CORE_NAMESPACE).bind(request.command.account_id.as_slice()).bind(stored.character_id.as_slice()).bind(request.command.trace_tick_id.as_slice()).bind(request.command.character_id.as_slice()).bind(audit.as_slice()).bind(stored.request_hash.as_slice()).bind(request.request_hash.as_slice()).bind(i64_value(request.command.issued_at_unix_ms)?).execute(connection).await?;
    Ok(())
}

async fn load_snapshot_entries(
    connection: &mut PgConnection,
    account: [u8; ID_BYTES],
    character: [u8; ID_BYTES],
    danger: &LiveDamageTraceDangerAuthorityV1,
    through: u64,
) -> Result<Vec<StoredLiveDamageTraceSnapshotEntryV1>, PersistenceError> {
    let rows = sqlx::query("SELECT entry.trace_tick_id,entry.event_tick,entry.event_ordinal,entry.cause_kind,entry.source_content_id,entry.source_entity_id,entry.source_sim_entity_id,entry.pattern_id,entry.attack_id,entry.raw_damage,entry.final_damage,entry.damage_type,entry.pre_health,entry.post_health,entry.source_x_milli_tiles,entry.source_y_milli_tiles,entry.status_count,entry.network_state,entry.recall_state,entry.lethal,entry.entry_digest FROM character_live_damage_trace_entries_v1 entry WHERE entry.namespace_id=$1 AND entry.account_id=$2 AND entry.character_id=$3 AND entry.lineage_id=$4 AND entry.restore_point_id=$5 ORDER BY entry.event_tick,entry.event_ordinal")
        .bind(WIPEABLE_CORE_NAMESPACE).bind(account.as_slice()).bind(character.as_slice()).bind(danger.lineage_id.as_slice()).bind(danger.restore_point_id.as_slice()).fetch_all(&mut *connection).await?;
    let mut result = Vec::with_capacity(rows.len());
    let mut previous_order = None;
    for row in rows {
        let event_tick = positive_u64(row.try_get("event_tick")?)?;
        if through.saturating_sub(event_tick) > LIVE_DAMAGE_TRACE_WINDOW_TICKS_V1
            || event_tick > through
        {
            return Err(corrupt());
        }
        let tick_id: Vec<u8> = row.try_get("trace_tick_id")?;
        let event_ordinal = nonnegative_u32(row.try_get("event_ordinal")?)?;
        if previous_order.is_some_and(|previous| previous >= (event_tick, event_ordinal)) {
            return Err(corrupt());
        }
        previous_order = Some((event_tick, event_ordinal));
        let statuses = sqlx::query("SELECT status_ordinal,status_id,remaining_ticks,stack_count FROM character_live_damage_trace_statuses_v1 WHERE namespace_id=$1 AND account_id=$2 AND trace_tick_id=$3 AND event_ordinal=$4 ORDER BY status_ordinal")
            .bind(WIPEABLE_CORE_NAMESPACE).bind(account.as_slice()).bind(&tick_id).bind(i32::try_from(event_ordinal).map_err(corrupt_conversion)?).fetch_all(&mut *connection).await?.into_iter().map(|status| Ok(LiveDamageTraceStatusV1 { status_ordinal: u8::try_from(status.try_get::<i16,_>("status_ordinal")?).map_err(corrupt_conversion)?, status_id: status.try_get("status_id")?, remaining_ticks: nonnegative_u32(status.try_get("remaining_ticks")?)?, stack_count: u16::try_from(status.try_get::<i16,_>("stack_count")?).map_err(corrupt_conversion)? })).collect::<Result<Vec<_>, PersistenceError>>()?;
        if statuses.len() != nonnegative_usize(row.try_get("status_count")?)? {
            return Err(corrupt());
        }
        let entry = LiveDamageTraceEntryV1 {
            event_ordinal,
            cause: LiveDamageTraceCauseV1::from_code(row.try_get("cause_kind")?)?,
            source_content_id: row.try_get("source_content_id")?,
            source_entity_id: row
                .try_get::<Option<Vec<u8>>, _>("source_entity_id")?
                .map(exact_id)
                .transpose()?,
            source_sim_entity_id: row
                .try_get::<Option<Vec<u8>>, _>("source_sim_entity_id")?
                .map(exact_sim_id)
                .transpose()?,
            pattern_id: row.try_get("pattern_id")?,
            attack_id: row.try_get("attack_id")?,
            raw_damage: nonnegative_u32(row.try_get("raw_damage")?)?,
            final_damage: nonnegative_u32(row.try_get("final_damage")?)?,
            damage_type: LiveDamageTraceDamageTypeV1::from_code(row.try_get("damage_type")?)?,
            pre_health: positive_u32(row.try_get("pre_health")?)?,
            post_health: nonnegative_u32(row.try_get("post_health")?)?,
            source_x_milli_tiles: row.try_get("source_x_milli_tiles")?,
            source_y_milli_tiles: row.try_get("source_y_milli_tiles")?,
            network_state: LiveDamageTraceNetworkStateV1::from_code(row.try_get("network_state")?)?,
            recall_state: LiveDamageTraceRecallStateV1::from_code(row.try_get("recall_state")?)?,
            lethal: row.try_get("lethal")?,
            statuses,
        };
        entry.validate()?;
        if exact_hash(row.try_get("entry_digest")?)? != entry_digest(event_tick, &entry)? {
            return Err(corrupt());
        }
        result.push(StoredLiveDamageTraceSnapshotEntryV1 { event_tick, entry });
    }
    if result.len() > MAX_LIVE_DAMAGE_TRACE_ENTRIES_V1 {
        return Err(corrupt());
    }
    Ok(result)
}

fn request_hash(
    command: &LiveDamageTraceTickCommandV1,
) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let mut h = blake3::Hasher::new();
    field(&mut h, REQUEST_CONTEXT.as_bytes())?;
    field(&mut h, &command.account_id)?;
    field(&mut h, &command.character_id)?;
    field(&mut h, &command.trace_tick_id)?;
    field(&mut h, &command.expected_character_version.to_le_bytes())?;
    field(&mut h, &command.event_tick.to_le_bytes())?;
    field(&mut h, &command.danger.lineage_id)?;
    field(&mut h, &command.danger.restore_point_id)?;
    field(&mut h, &command.danger.checkpoint_tick.to_le_bytes())?;
    field(&mut h, command.content.records_blake3.as_bytes())?;
    field(&mut h, command.content.assets_blake3.as_bytes())?;
    field(&mut h, command.content.localization_blake3.as_bytes())?;
    field(&mut h, &tick_digest(command)?)?;
    field(&mut h, &command.issued_at_unix_ms.to_le_bytes())?;
    Ok(*h.finalize().as_bytes())
}
fn tick_digest(
    command: &LiveDamageTraceTickCommandV1,
) -> Result<[u8; HASH_BYTES], PersistenceError> {
    tick_digest_entries(command.event_tick, &command.entries)
}
fn tick_digest_entries(
    event_tick: u64,
    entries: &[LiveDamageTraceEntryV1],
) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let mut h = blake3::Hasher::new();
    field(&mut h, TICK_CONTEXT.as_bytes())?;
    field(&mut h, &event_tick.to_le_bytes())?;
    for entry in entries {
        field(&mut h, &entry_digest(event_tick, entry)?)?;
    }
    Ok(*h.finalize().as_bytes())
}
fn entry_digest(
    tick: u64,
    entry: &LiveDamageTraceEntryV1,
) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let mut h = blake3::Hasher::new();
    field(&mut h, &tick.to_le_bytes())?;
    field(&mut h, &entry.event_ordinal.to_le_bytes())?;
    field(&mut h, &entry.cause.code().to_le_bytes())?;
    field(&mut h, entry.source_content_id.as_bytes())?;
    optional_field(
        &mut h,
        entry
            .source_entity_id
            .as_ref()
            .map(<[u8; ID_BYTES]>::as_slice),
    )?;
    optional_field(
        &mut h,
        entry
            .source_sim_entity_id
            .as_ref()
            .map(|value| value.to_le_bytes())
            .as_ref()
            .map(<[u8; 8]>::as_slice),
    )?;
    optional_field(&mut h, entry.pattern_id.as_deref().map(str::as_bytes))?;
    field(&mut h, entry.attack_id.as_bytes())?;
    field(&mut h, &entry.raw_damage.to_le_bytes())?;
    field(&mut h, &entry.final_damage.to_le_bytes())?;
    field(&mut h, &entry.damage_type.code().to_le_bytes())?;
    field(&mut h, &entry.pre_health.to_le_bytes())?;
    field(&mut h, &entry.post_health.to_le_bytes())?;
    field(&mut h, &entry.source_x_milli_tiles.to_le_bytes())?;
    field(&mut h, &entry.source_y_milli_tiles.to_le_bytes())?;
    field(&mut h, &entry.network_state.code().to_le_bytes())?;
    field(&mut h, &entry.recall_state.code().to_le_bytes())?;
    field(&mut h, &[u8::from(entry.lethal)])?;
    for status in &entry.statuses {
        field(&mut h, &[status.status_ordinal])?;
        field(&mut h, status.status_id.as_bytes())?;
        field(&mut h, &status.remaining_ticks.to_le_bytes())?;
        field(&mut h, &status.stack_count.to_le_bytes())?;
    }
    Ok(*h.finalize().as_bytes())
}
fn conflict_id(request: &LiveDamageTraceTickRequestV1) -> Result<[u8; ID_BYTES], PersistenceError> {
    let mut h = blake3::Hasher::new();
    field(&mut h, CONFLICT_CONTEXT.as_bytes())?;
    field(&mut h, &request.command.account_id)?;
    field(&mut h, &request.command.trace_tick_id)?;
    field(&mut h, &request.request_hash)?;
    let mut id = [0; ID_BYTES];
    id.copy_from_slice(&h.finalize().as_bytes()[..ID_BYTES]);
    if all_zero(&id) {
        Err(corrupt())
    } else {
        Ok(id)
    }
}
fn field(h: &mut blake3::Hasher, bytes: &[u8]) -> Result<(), PersistenceError> {
    h.update(
        &u64::try_from(bytes.len())
            .map_err(corrupt_conversion)?
            .to_le_bytes(),
    );
    h.update(bytes);
    Ok(())
}
fn optional_field(h: &mut blake3::Hasher, value: Option<&[u8]>) -> Result<(), PersistenceError> {
    match value {
        None => field(h, &[0]),
        Some(value) => {
            field(h, &[1])?;
            field(h, value)
        }
    }
}
fn stable_id(value: &str) -> bool {
    (3..=96).contains(&value.len())
        && value.is_ascii()
        && !value
            .bytes()
            .any(|b| b.is_ascii_whitespace() || b.is_ascii_uppercase())
}
fn exact_id(value: Vec<u8>) -> Result<[u8; ID_BYTES], PersistenceError> {
    let id = value.try_into().map_err(corrupt_conversion)?;
    if all_zero(&id) {
        Err(corrupt())
    } else {
        Ok(id)
    }
}
fn exact_sim_id(value: Vec<u8>) -> Result<u64, PersistenceError> {
    let bytes: [u8; 8] = value.try_into().map_err(corrupt_conversion)?;
    let value = u64::from_le_bytes(bytes);
    if value == 0 {
        Err(corrupt())
    } else {
        Ok(value)
    }
}
fn optional_id(value: Option<Vec<u8>>) -> Result<Option<[u8; ID_BYTES]>, PersistenceError> {
    value.map(exact_id).transpose()
}
fn exact_hash(value: Vec<u8>) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let hash = value.try_into().map_err(corrupt_conversion)?;
    if all_zero(&hash) {
        Err(corrupt())
    } else {
        Ok(hash)
    }
}
fn positive_u64(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .ok()
        .filter(|v| *v > 0)
        .ok_or_else(corrupt)
}
fn nonnegative_u64(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value).map_err(corrupt_conversion)
}
fn positive_u32(value: i32) -> Result<u32, PersistenceError> {
    u32::try_from(value)
        .ok()
        .filter(|v| *v > 0)
        .ok_or_else(corrupt)
}
fn nonnegative_u32(value: i32) -> Result<u32, PersistenceError> {
    u32::try_from(value).map_err(corrupt_conversion)
}
fn positive_usize(value: i16) -> Result<usize, PersistenceError> {
    usize::try_from(value)
        .ok()
        .filter(|v| *v > 0)
        .ok_or_else(corrupt)
}
fn nonnegative_usize(value: i16) -> Result<usize, PersistenceError> {
    usize::try_from(value).map_err(corrupt_conversion)
}
fn i64_value(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(corrupt_conversion)
}
async fn transaction_timestamp_ms(connection: &mut PgConnection) -> Result<u64, PersistenceError> {
    positive_u64(
        sqlx::query_scalar(
            "SELECT CAST(EXTRACT(EPOCH FROM transaction_timestamp())*1000 AS BIGINT)",
        )
        .fetch_one(connection)
        .await?,
    )
}
fn all_zero<const N: usize>(value: &[u8; N]) -> bool {
    value.iter().all(|byte| *byte == 0)
}
fn corrupt() -> PersistenceError {
    PersistenceError::CorruptStoredLiveDamageTrace
}
fn corrupt_conversion<T>(_: T) -> PersistenceError {
    corrupt()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(ordinal: u32) -> LiveDamageTraceEntryV1 {
        LiveDamageTraceEntryV1 {
            event_ordinal: ordinal,
            cause: LiveDamageTraceCauseV1::DirectHit,
            source_content_id: "enemy.bell_reed".into(),
            source_entity_id: Some([8; 16]),
            source_sim_entity_id: Some(42),
            pattern_id: Some("pattern.bell_reed.ring".into()),
            attack_id: "attack.bell_reed.ring".into(),
            raw_damage: 12,
            final_damage: 10,
            damage_type: LiveDamageTraceDamageTypeV1::Veil,
            pre_health: 100,
            post_health: 90,
            source_x_milli_tiles: 1000,
            source_y_milli_tiles: -2000,
            network_state: LiveDamageTraceNetworkStateV1::Connected,
            recall_state: LiveDamageTraceRecallStateV1::Inactive,
            lethal: false,
            statuses: vec![LiveDamageTraceStatusV1 {
                status_ordinal: 0,
                status_id: "status.bleed".into(),
                remaining_ticks: 30,
                stack_count: 1,
            }],
        }
    }
    fn command() -> LiveDamageTraceTickCommandV1 {
        LiveDamageTraceTickCommandV1 {
            account_id: [1; 16],
            character_id: [2; 16],
            trace_tick_id: [3; 16],
            expected_character_version: 4,
            event_tick: 120,
            danger: LiveDamageTraceDangerAuthorityV1 {
                lineage_id: [5; 16],
                restore_point_id: [6; 16],
                checkpoint_tick: 0,
            },
            content: LiveDamageTraceContentAuthorityV1::core(),
            entries: vec![entry(0)],
            issued_at_unix_ms: 7,
        }
    }
    #[test]
    fn canonical_hash_binds_entries_statuses_and_authority() {
        let base = LiveDamageTraceTickRequestV1::seal(command()).unwrap();
        let mut variants = Vec::new();
        let mut c = command();
        c.event_tick += 1;
        variants.push(c);
        let mut c = command();
        c.danger.checkpoint_tick += 1;
        variants.push(c);
        let mut c = command();
        c.entries[0].final_damage += 1;
        c.entries[0].post_health -= 1;
        variants.push(c);
        let mut c = command();
        c.entries[0].statuses[0].remaining_ticks += 1;
        variants.push(c);
        let mut c = command();
        c.entries[0].source_sim_entity_id = Some(43);
        variants.push(c);
        for c in variants {
            assert_ne!(
                LiveDamageTraceTickRequestV1::seal(c).unwrap().request_hash,
                base.request_hash
            );
        }
    }
    #[test]
    fn ordering_health_status_and_lethal_shape_fail_closed() {
        let mut c = command();
        c.entries = vec![entry(3), entry(7)];
        LiveDamageTraceTickRequestV1::seal(c).unwrap();
        let mut c = command();
        c.entries = vec![entry(7), entry(3)];
        assert!(LiveDamageTraceTickRequestV1::seal(c).is_err());
        let mut c = command();
        c.entries = vec![entry(3), entry(3)];
        assert!(LiveDamageTraceTickRequestV1::seal(c).is_err());
        let mut c = command();
        c.entries[0].event_ordinal = u32::MAX;
        assert!(LiveDamageTraceTickRequestV1::seal(c).is_err());
        let mut c = command();
        c.entries[0].post_health = 89;
        assert!(LiveDamageTraceTickRequestV1::seal(c).is_err());
        let mut c = command();
        c.entries[0].statuses[0].status_ordinal = 1;
        assert!(LiveDamageTraceTickRequestV1::seal(c).is_err());
        let mut c = command();
        c.entries[0].source_sim_entity_id = None;
        assert!(LiveDamageTraceTickRequestV1::seal(c).is_err());
        let mut c = command();
        c.entries.push(entry(1));
        c.entries[0].lethal = true;
        c.entries[0].post_health = 0;
        c.entries[0].final_damage = 100;
        assert!(LiveDamageTraceTickRequestV1::seal(c).is_err());
    }
    #[test]
    fn exact_window_and_bounded_payload_are_pinned() {
        assert_eq!(LIVE_DAMAGE_TRACE_WINDOW_TICKS_V1, 300);
        assert_eq!(MAX_LIVE_DAMAGE_TRACE_ENTRIES_V1, 4096);
        assert_eq!(MAX_LIVE_DAMAGE_TRACE_STATUSES_PER_ENTRY_V1, 32);
    }

    #[test]
    fn retained_result_digest_binds_commit_identity_and_counts() {
        let command = command();
        let request = LiveDamageTraceTickRequestV1::seal(command.clone()).unwrap();
        let receipt = receipt_from_command(
            &command,
            request.request_hash,
            tick_digest(&command).unwrap(),
            99,
        )
        .unwrap();
        for mutate in [
            |receipt: &mut Receipt| receipt.committed_at_unix_ms += 1,
            |receipt: &mut Receipt| receipt.entry_count += 1,
            |receipt: &mut Receipt| receipt.expected_character_version += 1,
        ] {
            let mut changed = receipt.clone();
            mutate(&mut changed);
            assert_ne!(
                receipt_result_digest(&changed).unwrap(),
                receipt.result_digest
            );
        }
    }

    #[test]
    fn retained_prior_root_is_not_current_root_sequence_authority() {
        let old_command = command();
        let request = LiveDamageTraceTickRequestV1::seal(old_command.clone()).unwrap();
        let old = receipt_from_command(
            &old_command,
            request.request_hash,
            tick_digest(&old_command).unwrap(),
            99,
        )
        .unwrap();
        let new_root = LiveDamageTraceDangerAuthorityV1 {
            lineage_id: [9; ID_BYTES],
            restore_point_id: [10; ID_BYTES],
            checkpoint_tick: 0,
        };
        assert!(!receipt_matches_danger_root(&old, &new_root));
        assert!(receipt_matches_danger_root(&old, &old_command.danger));
    }
}
