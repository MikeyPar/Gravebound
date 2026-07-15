//! Serializable authoritative life-clock persistence for `GB-M03-06B`.
//!
//! The contract is derived jointly from:
//! - `Gravebound_Production_GDD_v1_Canonical.md` `DTH-001` and `TECH-020..023`;
//! - `Gravebound_Content_Production_Spec_v1.md` Core content/Echo authority;
//! - `Gravebound_Development_Roadmap_v1.md` `GB-M03-06`/`13` restart gates.
//!
//! The server owns interval boundaries and state selection. Callers cannot author pre/post clock
//! values, link-loss accumulation, versions, or result digests. Account-first serialization keeps
//! clocks ordered with reward deeds, crash restoration, and terminal death.

use sqlx::{PgConnection, Row};

use crate::{
    CORE_WORLD_ASSETS_BLAKE3, CORE_WORLD_LOCALIZATION_BLAKE3, CORE_WORLD_RECORDS_BLAKE3,
    PersistenceError, PostgresPersistence, WIPEABLE_CORE_NAMESPACE,
    is_retryable_transaction_failure,
};

const ID_BYTES: usize = 16;
const HASH_BYTES: usize = 32;
const MAX_TRANSACTION_ATTEMPTS: u8 = 3;
const MAX_INTERVAL_TICKS: u32 = 1_800;
const LINK_LOST_WINDOW_TICKS: u32 = 90;
const CONTRACT_VERSION: u16 = 1;
const HALL_CONTENT_ID: &str = "hub.lantern_halls_01";
const REQUEST_HASH_CONTEXT: &str = "gravebound.life-clock-checkpoint.request.v1";
const RESULT_DIGEST_CONTEXT: &str = "gravebound.life-clock-checkpoint.result.v1";
const CONFLICT_AUDIT_CONTEXT: &str = "gravebound.life-clock-conflict-audit.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifeClockContentAuthorityV1 {
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
}

impl LifeClockContentAuthorityV1 {
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
            return Err(PersistenceError::LifeClockContentMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifeClockStateV1 {
    CharacterSelect,
    Loading,
    Offline,
    HallControllable,
    DangerLoading,
    DangerStaging,
    DangerControllable,
    DangerLinkLost,
}

impl LifeClockStateV1 {
    const fn code(self) -> i16 {
        match self {
            Self::CharacterSelect => 0,
            Self::Loading => 1,
            Self::Offline => 2,
            Self::HallControllable => 3,
            Self::DangerLoading => 4,
            Self::DangerStaging => 5,
            Self::DangerControllable => 6,
            Self::DangerLinkLost => 7,
        }
    }

    fn from_code(code: i16) -> Result<Self, PersistenceError> {
        match code {
            0 => Ok(Self::CharacterSelect),
            1 => Ok(Self::Loading),
            2 => Ok(Self::Offline),
            3 => Ok(Self::HallControllable),
            4 => Ok(Self::DangerLoading),
            5 => Ok(Self::DangerStaging),
            6 => Ok(Self::DangerControllable),
            7 => Ok(Self::DangerLinkLost),
            _ => Err(PersistenceError::CorruptStoredLifeClock),
        }
    }

    const fn counts_lifetime(self) -> bool {
        matches!(
            self,
            Self::HallControllable | Self::DangerControllable | Self::DangerLinkLost
        )
    }

    const fn counts_combat(self) -> bool {
        self.code() >= Self::DangerLoading.code()
    }

    const fn is_danger(self) -> bool {
        self.counts_combat()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifeClockDangerAuthorityV1 {
    pub lineage_id: [u8; ID_BYTES],
    pub restore_point_id: [u8; ID_BYTES],
    pub entry_life_metrics_version: u64,
    pub entry_permadeath_combat_ticks: u64,
}

impl LifeClockDangerAuthorityV1 {
    fn validate(&self) -> Result<(), PersistenceError> {
        if all_zero(&self.lineage_id)
            || all_zero(&self.restore_point_id)
            || self.entry_life_metrics_version == 0
            || i64::try_from(self.entry_life_metrics_version).is_err()
            || i64::try_from(self.entry_permadeath_combat_ticks).is_err()
        {
            return Err(PersistenceError::CorruptStoredLifeClock);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifeClockCheckpointCommandV1 {
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub checkpoint_id: [u8; ID_BYTES],
    pub expected_character_version: u64,
    pub expected_life_metrics_version: u64,
    /// End tick of this contiguous server-owned interval.
    pub authoritative_tick: u64,
    pub state: LifeClockStateV1,
    pub advanced_ticks: u32,
    pub danger: Option<LifeClockDangerAuthorityV1>,
    pub content: LifeClockContentAuthorityV1,
    pub issued_at_unix_ms: u64,
}

impl LifeClockCheckpointCommandV1 {
    fn validate(&self) -> Result<(), PersistenceError> {
        if all_zero(&self.account_id)
            || all_zero(&self.character_id)
            || all_zero(&self.checkpoint_id)
            || self.expected_character_version == 0
            || self.expected_life_metrics_version == 0
            || self.authoritative_tick == 0
            || self.advanced_ticks == 0
            || self.advanced_ticks > MAX_INTERVAL_TICKS
            || self.authoritative_tick < u64::from(self.advanced_ticks)
            || self.issued_at_unix_ms == 0
            || i64::try_from(self.expected_character_version).is_err()
            || i64::try_from(self.expected_life_metrics_version).is_err()
            || i64::try_from(self.authoritative_tick).is_err()
            || i64::try_from(self.issued_at_unix_ms).is_err()
            || self.state.is_danger() != self.danger.is_some()
        {
            return Err(PersistenceError::CorruptStoredLifeClock);
        }
        if let Some(danger) = &self.danger {
            danger.validate()?;
            if danger.entry_life_metrics_version > self.expected_life_metrics_version {
                return Err(PersistenceError::CorruptStoredLifeClock);
            }
        }
        self.content.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifeClockCheckpointRequestV1 {
    pub command: LifeClockCheckpointCommandV1,
    pub request_hash: [u8; HASH_BYTES],
}

impl LifeClockCheckpointRequestV1 {
    pub fn seal(command: LifeClockCheckpointCommandV1) -> Result<Self, PersistenceError> {
        command.validate()?;
        let request_hash = canonical_request_hash(&command)?;
        Ok(Self {
            command,
            request_hash,
        })
    }

    fn validate(&self) -> Result<(), PersistenceError> {
        self.command.validate()?;
        if all_zero(&self.request_hash)
            || self.request_hash != canonical_request_hash(&self.command)?
        {
            return Err(PersistenceError::CorruptStoredLifeClock);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredLifeClockCheckpointV1 {
    pub contract_version: u16,
    pub command: LifeClockCheckpointCommandV1,
    pub pre_lifetime_ticks: u64,
    pub post_lifetime_ticks: u64,
    pub pre_permadeath_combat_ticks: u64,
    pub post_permadeath_combat_ticks: u64,
    pub pre_link_lost_ticks: u32,
    pub post_link_lost_ticks: u32,
    pub pre_life_metrics_version: u64,
    pub post_life_metrics_version: u64,
    pub request_hash: [u8; HASH_BYTES],
    pub result_digest: [u8; HASH_BYTES],
    pub committed_at_unix_ms: u64,
}

impl StoredLifeClockCheckpointV1 {
    fn validate(&self) -> Result<(), PersistenceError> {
        self.command.validate()?;
        let lifetime_advance = if self.command.state.counts_lifetime() {
            u64::from(self.command.advanced_ticks)
        } else {
            0
        };
        let combat_advance = if self.command.state.counts_combat() {
            u64::from(self.command.advanced_ticks)
        } else {
            0
        };
        let expected_link_lost = if self.command.state == LifeClockStateV1::DangerLinkLost {
            self.pre_link_lost_ticks
                .checked_add(self.command.advanced_ticks)
                .filter(|ticks| *ticks <= LINK_LOST_WINDOW_TICKS)
                .ok_or(PersistenceError::LifeClockLinkLostWindowExpired)?
        } else {
            0
        };
        if self.contract_version != CONTRACT_VERSION
            || self.request_hash != canonical_request_hash(&self.command)?
            || self.pre_life_metrics_version != self.command.expected_life_metrics_version
            || self.post_life_metrics_version != self.pre_life_metrics_version + 1
            || self.post_lifetime_ticks
                != self
                    .pre_lifetime_ticks
                    .checked_add(lifetime_advance)
                    .ok_or(PersistenceError::CorruptStoredLifeClock)?
            || self.post_permadeath_combat_ticks
                != self
                    .pre_permadeath_combat_ticks
                    .checked_add(combat_advance)
                    .ok_or(PersistenceError::CorruptStoredLifeClock)?
            || self.pre_link_lost_ticks > LINK_LOST_WINDOW_TICKS
            || self.post_link_lost_ticks != expected_link_lost
            || self.committed_at_unix_ms < self.command.issued_at_unix_ms
            || self.result_digest != canonical_result_digest(self)?
        {
            return Err(PersistenceError::CorruptStoredLifeClock);
        }
        if let Some(danger) = &self.command.danger
            && danger.entry_permadeath_combat_ticks > self.pre_permadeath_combat_ticks
        {
            return Err(PersistenceError::CorruptStoredLifeClock);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifeClockCheckpointTransactionV1 {
    Committed(StoredLifeClockCheckpointV1),
    Replayed(StoredLifeClockCheckpointV1),
}

impl LifeClockCheckpointTransactionV1 {
    #[must_use]
    pub const fn receipt(&self) -> &StoredLifeClockCheckpointV1 {
        match self {
            Self::Committed(receipt) | Self::Replayed(receipt) => receipt,
        }
    }
}

/// Strict restart head. The latest receipt is historical evidence; current clocks may have a newer
/// version because deed commits also advance the shared life-metrics aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredLifeClockHeadV1 {
    pub character_version: u64,
    pub lifetime_ticks: u64,
    pub permadeath_combat_ticks: u64,
    pub life_metrics_version: u64,
    pub authoritative_tick: u64,
    pub link_lost_ticks: u32,
    pub danger: Option<LifeClockDangerAuthorityV1>,
    pub latest_receipt: Option<StoredLifeClockCheckpointV1>,
}

impl PostgresPersistence {
    pub async fn transact_life_clock_checkpoint_v1(
        &self,
        request: &LifeClockCheckpointRequestV1,
    ) -> Result<LifeClockCheckpointTransactionV1, PersistenceError> {
        request.validate()?;
        for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self.transact_life_clock_checkpoint_v1_once(request).await {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS
                        && is_retryable_transaction_failure(&error) => {}
                result => return result,
            }
        }
        unreachable!("bounded life-clock transaction loop always returns")
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the account-first replay, binding, arithmetic, and commit sequence is contiguous"
    )]
    async fn transact_life_clock_checkpoint_v1_once(
        &self,
        request: &LifeClockCheckpointRequestV1,
    ) -> Result<LifeClockCheckpointTransactionV1, PersistenceError> {
        let command = &request.command;
        let mut transaction = self.begin_transaction().await?;
        let selected = lock_account(transaction.connection(), command.account_id).await?;

        if let Some(stored) = load_receipt(
            transaction.connection(),
            command.account_id,
            command.checkpoint_id,
        )
        .await?
        {
            if stored.request_hash != request.request_hash {
                insert_conflict_audit(transaction.connection(), request, &stored).await?;
                transaction.commit().await?;
                return Err(PersistenceError::LifeClockIdempotencyConflict);
            }
            stored.validate()?;
            if &stored.command != command {
                transaction.rollback().await?;
                return Err(PersistenceError::CorruptStoredLifeClock);
            }
            transaction.rollback().await?;
            return Ok(LifeClockCheckpointTransactionV1::Replayed(stored));
        }

        if selected != Some(command.character_id) {
            transaction.rollback().await?;
            return Err(PersistenceError::LifeClockBindingMismatch);
        }
        let character_version = lock_character(transaction.connection(), command).await?;
        if character_version != command.expected_character_version {
            transaction.rollback().await?;
            return Err(PersistenceError::LifeClockCharacterVersionMismatch {
                expected: command.expected_character_version,
                actual: character_version,
            });
        }
        let metrics = lock_life_metrics(transaction.connection(), command).await?;
        if metrics.version != command.expected_life_metrics_version {
            transaction.rollback().await?;
            return Err(PersistenceError::LifeClockMetricsVersionMismatch {
                expected: command.expected_life_metrics_version,
                actual: metrics.version,
            });
        }
        validate_world_authority(transaction.connection(), command).await?;
        let previous = load_latest_receipt_for_update(
            transaction.connection(),
            command.account_id,
            command.character_id,
        )
        .await?;
        validate_interval_continuity(command, previous.as_ref())?;
        let pre_link_lost_ticks = prior_link_lost(command, previous.as_ref());
        let post_link_lost_ticks = if command.state == LifeClockStateV1::DangerLinkLost {
            pre_link_lost_ticks
                .checked_add(command.advanced_ticks)
                .filter(|ticks| *ticks <= LINK_LOST_WINDOW_TICKS)
                .ok_or(PersistenceError::LifeClockLinkLostWindowExpired)?
        } else {
            0
        };
        let lifetime_advance = if command.state.counts_lifetime() {
            u64::from(command.advanced_ticks)
        } else {
            0
        };
        let combat_advance = if command.state.counts_combat() {
            u64::from(command.advanced_ticks)
        } else {
            0
        };
        let post_lifetime_ticks = metrics
            .lifetime_ticks
            .checked_add(lifetime_advance)
            .ok_or(PersistenceError::CorruptStoredLifeClock)?;
        let post_permadeath_combat_ticks = metrics
            .permadeath_combat_ticks
            .checked_add(combat_advance)
            .ok_or(PersistenceError::CorruptStoredLifeClock)?;
        let post_life_metrics_version = metrics
            .version
            .checked_add(1)
            .ok_or(PersistenceError::CorruptStoredLifeClock)?;
        advance_life_metrics(
            transaction.connection(),
            command,
            &metrics,
            post_lifetime_ticks,
            post_permadeath_combat_ticks,
            post_life_metrics_version,
        )
        .await?;
        let committed_at_unix_ms = transaction_timestamp_ms(transaction.connection()).await?;
        if command.issued_at_unix_ms > committed_at_unix_ms {
            transaction.rollback().await?;
            return Err(PersistenceError::CorruptStoredLifeClock);
        }
        let mut stored = StoredLifeClockCheckpointV1 {
            contract_version: CONTRACT_VERSION,
            command: command.clone(),
            pre_lifetime_ticks: metrics.lifetime_ticks,
            post_lifetime_ticks,
            pre_permadeath_combat_ticks: metrics.permadeath_combat_ticks,
            post_permadeath_combat_ticks,
            pre_link_lost_ticks,
            post_link_lost_ticks,
            pre_life_metrics_version: metrics.version,
            post_life_metrics_version,
            request_hash: request.request_hash,
            result_digest: [0; HASH_BYTES],
            committed_at_unix_ms,
        };
        stored.result_digest = canonical_result_digest(&stored)?;
        stored.validate()?;
        insert_receipt(transaction.connection(), &stored).await?;
        sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
            .execute(transaction.connection())
            .await?;
        transaction.commit().await?;
        Ok(LifeClockCheckpointTransactionV1::Committed(stored))
    }

    pub async fn load_life_clock_head_v1(
        &self,
        account_id: [u8; ID_BYTES],
        character_id: [u8; ID_BYTES],
    ) -> Result<StoredLifeClockHeadV1, PersistenceError> {
        if all_zero(&account_id) || all_zero(&character_id) {
            return Err(PersistenceError::CorruptStoredLifeClock);
        }
        let mut transaction = self.begin_transaction().await?;
        let selected = lock_account(transaction.connection(), account_id).await?;
        if selected != Some(character_id) {
            transaction.rollback().await?;
            return Err(PersistenceError::LifeClockBindingMismatch);
        }
        let character_version =
            lock_character_by_id(transaction.connection(), account_id, character_id).await?;
        let metrics =
            load_life_metrics_by_id(transaction.connection(), account_id, character_id).await?;
        let latest_receipt =
            load_latest_receipt_for_update(transaction.connection(), account_id, character_id)
                .await?;
        let danger =
            load_current_danger_authority(transaction.connection(), account_id, character_id)
                .await?;
        let authoritative_tick = latest_receipt
            .as_ref()
            .map_or(0, |receipt| receipt.command.authoritative_tick);
        let link_lost_ticks = latest_receipt.as_ref().map_or(0, |receipt| {
            if receipt.command.state == LifeClockStateV1::DangerLinkLost
                && receipt.command.danger == danger
            {
                receipt.post_link_lost_ticks
            } else {
                0
            }
        });
        if let Some(receipt) = &latest_receipt {
            receipt.validate()?;
            validate_restart_metrics(
                transaction.connection(),
                account_id,
                character_id,
                &metrics,
                receipt,
            )
            .await?;
        }
        transaction.rollback().await?;
        Ok(StoredLifeClockHeadV1 {
            character_version,
            lifetime_ticks: metrics.lifetime_ticks,
            permadeath_combat_ticks: metrics.permadeath_combat_ticks,
            life_metrics_version: metrics.version,
            authoritative_tick,
            link_lost_ticks,
            danger,
            latest_receipt,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct LockedLifeMetrics {
    lifetime_ticks: u64,
    permadeath_combat_ticks: u64,
    version: u64,
}

async fn lock_account(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
) -> Result<Option<[u8; ID_BYTES]>, PersistenceError> {
    let row = sqlx::query(
        "SELECT selected_character_id FROM accounts \
         WHERE namespace_id=$1 AND account_id=$2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Err(PersistenceError::LifeClockOwnerNotFound);
    };
    row.try_get::<Option<Vec<u8>>, _>("selected_character_id")?
        .map(exact_id)
        .transpose()
}

async fn lock_character(
    connection: &mut PgConnection,
    command: &LifeClockCheckpointCommandV1,
) -> Result<u64, PersistenceError> {
    lock_character_by_id(connection, command.account_id, command.character_id).await
}

async fn lock_character_by_id(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
) -> Result<u64, PersistenceError> {
    let row = sqlx::query(
        "SELECT roster_ordinal,life_state,security_state,character_state_version \
         FROM characters WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(connection)
    .await?;
    let Some(row) = row else {
        return Err(PersistenceError::LifeClockOwnerNotFound);
    };
    if row.try_get::<Option<i16>, _>("roster_ordinal")?.is_none()
        || row.try_get::<i16, _>("life_state")? != 0
        || row.try_get::<i16, _>("security_state")? != 0
    {
        return Err(PersistenceError::LifeClockBindingMismatch);
    }
    positive_u64(row.try_get("character_state_version")?)
}

async fn lock_life_metrics(
    connection: &mut PgConnection,
    command: &LifeClockCheckpointCommandV1,
) -> Result<LockedLifeMetrics, PersistenceError> {
    load_life_metrics(connection, command.account_id, command.character_id, true).await
}

async fn load_life_metrics_by_id(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
) -> Result<LockedLifeMetrics, PersistenceError> {
    load_life_metrics(connection, account_id, character_id, false).await
}

async fn load_life_metrics(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
    lock: bool,
) -> Result<LockedLifeMetrics, PersistenceError> {
    let row = if lock {
        sqlx::query(
            "SELECT lifetime_ticks,permadeath_combat_ticks,life_metrics_version \
             FROM character_life_metrics \
             WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .fetch_optional(&mut *connection)
        .await?
    } else {
        sqlx::query(
            "SELECT lifetime_ticks,permadeath_combat_ticks,life_metrics_version \
             FROM character_life_metrics \
             WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .fetch_optional(&mut *connection)
        .await?
    };
    let Some(row) = row else {
        return Err(PersistenceError::LifeClockOwnerNotFound);
    };
    Ok(LockedLifeMetrics {
        lifetime_ticks: nonnegative_u64(row.try_get("lifetime_ticks")?)?,
        permadeath_combat_ticks: nonnegative_u64(row.try_get("permadeath_combat_ticks")?)?,
        version: positive_u64(row.try_get("life_metrics_version")?)?,
    })
}

async fn validate_world_authority(
    connection: &mut PgConnection,
    command: &LifeClockCheckpointCommandV1,
) -> Result<(), PersistenceError> {
    let row = sqlx::query(
        "SELECT location_kind,location_content_id,instance_lineage_id,entry_restore_point_id \
         FROM character_world_locations \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.account_id.as_slice())
    .bind(command.character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?;
    let Some(world) = row else {
        return Err(PersistenceError::LifeClockBindingMismatch);
    };
    let location_kind: i16 = world.try_get("location_kind")?;
    if let Some(danger) = &command.danger {
        if location_kind != 2
            || optional_id(world.try_get("instance_lineage_id")?)? != Some(danger.lineage_id)
            || optional_id(world.try_get("entry_restore_point_id")?)?
                != Some(danger.restore_point_id)
        {
            return Err(PersistenceError::LifeClockBindingMismatch);
        }
        validate_active_danger(connection, command, danger).await
    } else {
        if location_kind == 2
            || (command.state == LifeClockStateV1::CharacterSelect && location_kind != 0)
            || (command.state == LifeClockStateV1::HallControllable
                && (location_kind != 1
                    || world
                        .try_get::<Option<String>, _>("location_content_id")?
                        .as_deref()
                        != Some(HALL_CONTENT_ID)))
        {
            return Err(PersistenceError::LifeClockBindingMismatch);
        }
        Ok(())
    }
}

async fn validate_active_danger(
    connection: &mut PgConnection,
    command: &LifeClockCheckpointCommandV1,
    danger: &LifeClockDangerAuthorityV1,
) -> Result<(), PersistenceError> {
    let stored = load_active_danger_authority(
        connection,
        command.account_id,
        command.character_id,
        danger.lineage_id,
        danger.restore_point_id,
        &command.content,
    )
    .await?;
    if &stored != danger {
        return Err(PersistenceError::LifeClockBindingMismatch);
    }
    Ok(())
}

async fn load_active_danger_authority(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
    lineage_id: [u8; ID_BYTES],
    restore_point_id: [u8; ID_BYTES],
    content: &LifeClockContentAuthorityV1,
) -> Result<LifeClockDangerAuthorityV1, PersistenceError> {
    let root = sqlx::query(
        "SELECT lineage_id,restore_state,life_metrics_version, \
                records_blake3,assets_blake3,localization_blake3 \
         FROM character_entry_restore_points \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND restore_point_id=$4 \
         FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(restore_point_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?;
    let lineage = sqlx::query(
        "SELECT lineage_state,records_blake3,assets_blake3,localization_blake3 \
         FROM character_instance_lineages \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND lineage_id=$4 \
         FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(lineage_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?;
    let checkpoint_tick: Option<i64> = sqlx::query_scalar(
        "SELECT checkpoint_tick FROM character_danger_checkpoints \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND lineage_id=$4 \
           AND records_blake3=$5 AND assets_blake3=$6 AND localization_blake3=$7 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(lineage_id.as_slice())
    .bind(&content.records_blake3)
    .bind(&content.assets_blake3)
    .bind(&content.localization_blake3)
    .fetch_optional(&mut *connection)
    .await?;
    let (Some(root), Some(lineage)) = (root, lineage) else {
        return Err(PersistenceError::LifeClockBindingMismatch);
    };
    let entry_version = positive_u64(root.try_get("life_metrics_version")?)?;
    let entry = sqlx::query(
        "SELECT rollback_permadeath_combat_ticks FROM entry_restore_life_metrics_v3 \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND restore_point_id=$4 \
           AND life_metrics_version=$5 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(restore_point_id.as_slice())
    .bind(i64_value(entry_version)?)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(entry) = entry else {
        return Err(PersistenceError::LifeClockBindingMismatch);
    };
    if checkpoint_tick.is_none()
        || exact_id(root.try_get("lineage_id")?)? != lineage_id
        || root.try_get::<i16, _>("restore_state")? != 0
        || root.try_get::<String, _>("records_blake3")? != content.records_blake3
        || root.try_get::<String, _>("assets_blake3")? != content.assets_blake3
        || root.try_get::<String, _>("localization_blake3")? != content.localization_blake3
        || lineage.try_get::<i16, _>("lineage_state")? != 0
        || lineage.try_get::<String, _>("records_blake3")? != content.records_blake3
        || lineage.try_get::<String, _>("assets_blake3")? != content.assets_blake3
        || lineage.try_get::<String, _>("localization_blake3")? != content.localization_blake3
    {
        return Err(PersistenceError::LifeClockBindingMismatch);
    }
    Ok(LifeClockDangerAuthorityV1 {
        lineage_id,
        restore_point_id,
        entry_life_metrics_version: entry_version,
        entry_permadeath_combat_ticks: nonnegative_u64(
            entry.try_get("rollback_permadeath_combat_ticks")?,
        )?,
    })
}

async fn load_current_danger_authority(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
) -> Result<Option<LifeClockDangerAuthorityV1>, PersistenceError> {
    let row = sqlx::query(
        "SELECT location_kind,instance_lineage_id,entry_restore_point_id \
         FROM character_world_locations \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Err(PersistenceError::CorruptStoredLifeClock);
    };
    let location_kind: i16 = row.try_get("location_kind")?;
    let lineage = optional_id(row.try_get("instance_lineage_id")?)?;
    let restore = optional_id(row.try_get("entry_restore_point_id")?)?;
    if location_kind != 2 {
        return if lineage.is_none() && restore.is_none() {
            Ok(None)
        } else {
            Err(PersistenceError::CorruptStoredLifeClock)
        };
    }
    let (Some(lineage_id), Some(restore_point_id)) = (lineage, restore) else {
        return Err(PersistenceError::CorruptStoredLifeClock);
    };
    load_active_danger_authority(
        connection,
        account_id,
        character_id,
        lineage_id,
        restore_point_id,
        &LifeClockContentAuthorityV1::core(),
    )
    .await
    .map(Some)
}

async fn validate_restart_metrics(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
    metrics: &LockedLifeMetrics,
    latest: &StoredLifeClockCheckpointV1,
) -> Result<(), PersistenceError> {
    if metrics.lifetime_ticks != latest.post_lifetime_ticks
        || metrics.version < latest.post_life_metrics_version
        || metrics.permadeath_combat_ticks > latest.post_permadeath_combat_ticks
    {
        return Err(PersistenceError::CorruptStoredLifeClock);
    }
    if metrics.permadeath_combat_ticks == latest.post_permadeath_combat_ticks {
        return Ok(());
    }
    let Some(danger) = &latest.command.danger else {
        return Err(PersistenceError::CorruptStoredLifeClock);
    };
    let crash_restored: bool = sqlx::query_scalar(
        "SELECT EXISTS( \
           SELECT 1 FROM danger_crash_restore_results AS result \
           JOIN entry_restore_life_metrics_v3 AS entry \
             ON entry.namespace_id=result.namespace_id AND entry.account_id=result.account_id \
            AND entry.character_id=result.character_id \
            AND entry.restore_point_id=result.restore_point_id \
            AND entry.restored_life_metrics_version=result.post_life_metrics_version \
           WHERE result.namespace_id=$1 AND result.account_id=$2 AND result.character_id=$3 \
             AND result.restore_point_id=$4 AND result.result_code=0 \
             AND result.post_life_metrics_version>$5 \
             AND result.post_life_metrics_version<=$6 \
             AND entry.rollback_permadeath_combat_ticks=$7)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(danger.restore_point_id.as_slice())
    .bind(i64_value(latest.post_life_metrics_version)?)
    .bind(i64_value(metrics.version)?)
    .bind(i64_value(metrics.permadeath_combat_ticks)?)
    .fetch_one(connection)
    .await?;
    if !crash_restored {
        return Err(PersistenceError::CorruptStoredLifeClock);
    }
    Ok(())
}

fn validate_interval_continuity(
    command: &LifeClockCheckpointCommandV1,
    previous: Option<&StoredLifeClockCheckpointV1>,
) -> Result<(), PersistenceError> {
    if let Some(previous) = previous {
        previous.validate()?;
        if previous.command.danger == command.danger
            && command.danger.is_some()
            && previous.command.state == LifeClockStateV1::DangerLinkLost
            && previous.post_link_lost_ticks == LINK_LOST_WINDOW_TICKS
        {
            return Err(PersistenceError::LifeClockTerminalResolutionRequired);
        }
        let expected = previous
            .command
            .authoritative_tick
            .checked_add(u64::from(command.advanced_ticks))
            .ok_or(PersistenceError::CorruptStoredLifeClock)?;
        if command.authoritative_tick != expected {
            return Err(PersistenceError::LifeClockTickDiscontinuity {
                expected,
                actual: command.authoritative_tick,
            });
        }
    }
    Ok(())
}

fn prior_link_lost(
    command: &LifeClockCheckpointCommandV1,
    previous: Option<&StoredLifeClockCheckpointV1>,
) -> u32 {
    previous.map_or(0, |previous| {
        if command.state == LifeClockStateV1::DangerLinkLost
            && previous.command.state == LifeClockStateV1::DangerLinkLost
            && command.danger == previous.command.danger
        {
            previous.post_link_lost_ticks
        } else {
            0
        }
    })
}

async fn advance_life_metrics(
    connection: &mut PgConnection,
    command: &LifeClockCheckpointCommandV1,
    pre: &LockedLifeMetrics,
    post_lifetime_ticks: u64,
    post_permadeath_combat_ticks: u64,
    post_version: u64,
) -> Result<(), PersistenceError> {
    let affected = sqlx::query(
        "UPDATE character_life_metrics \
         SET lifetime_ticks=$1,permadeath_combat_ticks=$2,life_metrics_version=$3, \
             updated_at=transaction_timestamp() \
         WHERE namespace_id=$4 AND account_id=$5 AND character_id=$6 \
           AND lifetime_ticks=$7 AND permadeath_combat_ticks=$8 AND life_metrics_version=$9",
    )
    .bind(i64_value(post_lifetime_ticks)?)
    .bind(i64_value(post_permadeath_combat_ticks)?)
    .bind(i64_value(post_version)?)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.account_id.as_slice())
    .bind(command.character_id.as_slice())
    .bind(i64_value(pre.lifetime_ticks)?)
    .bind(i64_value(pre.permadeath_combat_ticks)?)
    .bind(i64_value(pre.version)?)
    .execute(connection)
    .await?
    .rows_affected();
    if affected != 1 {
        return Err(PersistenceError::CorruptStoredLifeClock);
    }
    Ok(())
}

async fn insert_receipt(
    connection: &mut PgConnection,
    stored: &StoredLifeClockCheckpointV1,
) -> Result<(), PersistenceError> {
    let danger = stored.command.danger.as_ref();
    let affected = sqlx::query(
        "INSERT INTO character_life_clock_checkpoint_receipts_v1( \
          namespace_id,account_id,character_id,checkpoint_id,contract_version, \
          expected_character_version,authoritative_tick,clock_state,advanced_ticks, \
          lineage_id,restore_point_id,danger_entry_life_metrics_version, \
          danger_entry_permadeath_combat_ticks,pre_lifetime_ticks,post_lifetime_ticks, \
          pre_permadeath_combat_ticks,post_permadeath_combat_ticks,pre_link_lost_ticks, \
          post_link_lost_ticks,pre_life_metrics_version,post_life_metrics_version, \
          records_blake3,assets_blake3,localization_blake3,request_hash,result_digest, \
          issued_at,committed_at) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19, \
                $20,$21,$22,$23,$24,$25,$26,to_timestamp($27::double precision/1000.0), \
                to_timestamp($28::double precision/1000.0))",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(stored.command.account_id.as_slice())
    .bind(stored.command.character_id.as_slice())
    .bind(stored.command.checkpoint_id.as_slice())
    .bind(i16::try_from(stored.contract_version).map_err(corrupt_conversion)?)
    .bind(i64_value(stored.command.expected_character_version)?)
    .bind(i64_value(stored.command.authoritative_tick)?)
    .bind(stored.command.state.code())
    .bind(i32::try_from(stored.command.advanced_ticks).map_err(corrupt_conversion)?)
    .bind(danger.map(|value| value.lineage_id.as_slice()))
    .bind(danger.map(|value| value.restore_point_id.as_slice()))
    .bind(
        danger
            .map(|value| i64_value(value.entry_life_metrics_version))
            .transpose()?,
    )
    .bind(
        danger
            .map(|value| i64_value(value.entry_permadeath_combat_ticks))
            .transpose()?,
    )
    .bind(i64_value(stored.pre_lifetime_ticks)?)
    .bind(i64_value(stored.post_lifetime_ticks)?)
    .bind(i64_value(stored.pre_permadeath_combat_ticks)?)
    .bind(i64_value(stored.post_permadeath_combat_ticks)?)
    .bind(i16::try_from(stored.pre_link_lost_ticks).map_err(corrupt_conversion)?)
    .bind(i16::try_from(stored.post_link_lost_ticks).map_err(corrupt_conversion)?)
    .bind(i64_value(stored.pre_life_metrics_version)?)
    .bind(i64_value(stored.post_life_metrics_version)?)
    .bind(&stored.command.content.records_blake3)
    .bind(&stored.command.content.assets_blake3)
    .bind(&stored.command.content.localization_blake3)
    .bind(stored.request_hash.as_slice())
    .bind(stored.result_digest.as_slice())
    .bind(i64_value(stored.command.issued_at_unix_ms)?)
    .bind(i64_value(stored.committed_at_unix_ms)?)
    .execute(connection)
    .await?
    .rows_affected();
    if affected != 1 {
        return Err(PersistenceError::CorruptStoredLifeClock);
    }
    Ok(())
}

async fn load_receipt(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    checkpoint_id: [u8; ID_BYTES],
) -> Result<Option<StoredLifeClockCheckpointV1>, PersistenceError> {
    let row = sqlx::query(
        "SELECT account_id,character_id,checkpoint_id,contract_version, \
                expected_character_version,authoritative_tick,clock_state,advanced_ticks, \
                lineage_id,restore_point_id,danger_entry_life_metrics_version, \
                danger_entry_permadeath_combat_ticks,pre_lifetime_ticks,post_lifetime_ticks, \
                pre_permadeath_combat_ticks,post_permadeath_combat_ticks,pre_link_lost_ticks, \
                post_link_lost_ticks,pre_life_metrics_version,post_life_metrics_version, \
                records_blake3,assets_blake3,localization_blake3,request_hash,result_digest, \
                CAST(EXTRACT(EPOCH FROM issued_at)*1000 AS BIGINT) AS issued_at_ms, \
                CAST(EXTRACT(EPOCH FROM committed_at)*1000 AS BIGINT) AS committed_at_ms \
         FROM character_life_clock_checkpoint_receipts_v1 \
         WHERE namespace_id=$1 AND account_id=$2 AND checkpoint_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(checkpoint_id.as_slice())
    .fetch_optional(connection)
    .await?;
    row.map(|row| decode_receipt(&row)).transpose()
}

async fn load_latest_receipt_for_update(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
) -> Result<Option<StoredLifeClockCheckpointV1>, PersistenceError> {
    let row = sqlx::query(
        "SELECT account_id,character_id,checkpoint_id,contract_version, \
                expected_character_version,authoritative_tick,clock_state,advanced_ticks, \
                lineage_id,restore_point_id,danger_entry_life_metrics_version, \
                danger_entry_permadeath_combat_ticks,pre_lifetime_ticks,post_lifetime_ticks, \
                pre_permadeath_combat_ticks,post_permadeath_combat_ticks,pre_link_lost_ticks, \
                post_link_lost_ticks,pre_life_metrics_version,post_life_metrics_version, \
                records_blake3,assets_blake3,localization_blake3,request_hash,result_digest, \
                CAST(EXTRACT(EPOCH FROM issued_at)*1000 AS BIGINT) AS issued_at_ms, \
                CAST(EXTRACT(EPOCH FROM committed_at)*1000 AS BIGINT) AS committed_at_ms \
         FROM character_life_clock_checkpoint_receipts_v1 \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 \
         ORDER BY authoritative_tick DESC LIMIT 1 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(connection)
    .await?;
    row.map(|row| decode_receipt(&row)).transpose()
}

fn decode_receipt(
    row: &sqlx::postgres::PgRow,
) -> Result<StoredLifeClockCheckpointV1, PersistenceError> {
    let state = LifeClockStateV1::from_code(row.try_get("clock_state")?)?;
    let lineage = row.try_get::<Option<Vec<u8>>, _>("lineage_id")?;
    let restore = row.try_get::<Option<Vec<u8>>, _>("restore_point_id")?;
    let entry_version = row.try_get::<Option<i64>, _>("danger_entry_life_metrics_version")?;
    let entry_ticks = row.try_get::<Option<i64>, _>("danger_entry_permadeath_combat_ticks")?;
    let danger = match (lineage, restore, entry_version, entry_ticks) {
        (None, None, None, None) => None,
        (Some(lineage), Some(restore), Some(version), Some(ticks)) => {
            Some(LifeClockDangerAuthorityV1 {
                lineage_id: exact_id(lineage)?,
                restore_point_id: exact_id(restore)?,
                entry_life_metrics_version: positive_u64(version)?,
                entry_permadeath_combat_ticks: nonnegative_u64(ticks)?,
            })
        }
        _ => return Err(PersistenceError::CorruptStoredLifeClock),
    };
    let stored = StoredLifeClockCheckpointV1 {
        contract_version: u16::try_from(row.try_get::<i16, _>("contract_version")?)
            .map_err(corrupt_conversion)?,
        command: LifeClockCheckpointCommandV1 {
            account_id: exact_id(row.try_get("account_id")?)?,
            character_id: exact_id(row.try_get("character_id")?)?,
            checkpoint_id: exact_id(row.try_get("checkpoint_id")?)?,
            expected_character_version: positive_u64(row.try_get("expected_character_version")?)?,
            expected_life_metrics_version: positive_u64(row.try_get("pre_life_metrics_version")?)?,
            authoritative_tick: positive_u64(row.try_get("authoritative_tick")?)?,
            state,
            advanced_ticks: u32::try_from(row.try_get::<i32, _>("advanced_ticks")?)
                .map_err(corrupt_conversion)?,
            danger,
            content: LifeClockContentAuthorityV1 {
                records_blake3: row.try_get("records_blake3")?,
                assets_blake3: row.try_get("assets_blake3")?,
                localization_blake3: row.try_get("localization_blake3")?,
            },
            issued_at_unix_ms: positive_u64(row.try_get("issued_at_ms")?)?,
        },
        pre_lifetime_ticks: nonnegative_u64(row.try_get("pre_lifetime_ticks")?)?,
        post_lifetime_ticks: nonnegative_u64(row.try_get("post_lifetime_ticks")?)?,
        pre_permadeath_combat_ticks: nonnegative_u64(row.try_get("pre_permadeath_combat_ticks")?)?,
        post_permadeath_combat_ticks: nonnegative_u64(
            row.try_get("post_permadeath_combat_ticks")?,
        )?,
        pre_link_lost_ticks: u32::try_from(row.try_get::<i16, _>("pre_link_lost_ticks")?)
            .map_err(corrupt_conversion)?,
        post_link_lost_ticks: u32::try_from(row.try_get::<i16, _>("post_link_lost_ticks")?)
            .map_err(corrupt_conversion)?,
        pre_life_metrics_version: positive_u64(row.try_get("pre_life_metrics_version")?)?,
        post_life_metrics_version: positive_u64(row.try_get("post_life_metrics_version")?)?,
        request_hash: exact_hash(row.try_get("request_hash")?)?,
        result_digest: exact_hash(row.try_get("result_digest")?)?,
        committed_at_unix_ms: positive_u64(row.try_get("committed_at_ms")?)?,
    };
    stored.validate()?;
    Ok(stored)
}

async fn insert_conflict_audit(
    connection: &mut PgConnection,
    request: &LifeClockCheckpointRequestV1,
    stored: &StoredLifeClockCheckpointV1,
) -> Result<(), PersistenceError> {
    let audit_id = conflict_audit_id(request)?;
    let affected = sqlx::query(
        "INSERT INTO character_life_clock_conflict_audits_v1( \
          namespace_id,account_id,character_id,checkpoint_id,attempted_character_id,audit_id, \
          conflict_code,stored_request_hash,attempted_request_hash,observed_character_version, \
          observed_life_metrics_version,attempted_issued_at) \
         SELECT $1,$2,$3,$4,$5,$6,0,$7,$8,character.character_state_version, \
                metrics.life_metrics_version,to_timestamp($9::double precision/1000.0) \
         FROM characters AS character JOIN character_life_metrics AS metrics \
           ON metrics.namespace_id=character.namespace_id AND metrics.account_id=character.account_id \
          AND metrics.character_id=character.character_id \
         WHERE character.namespace_id=$1 AND character.account_id=$2 AND character.character_id=$3 \
         ON CONFLICT(namespace_id,account_id,checkpoint_id,attempted_request_hash) DO NOTHING",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(stored.command.account_id.as_slice())
    .bind(stored.command.character_id.as_slice())
    .bind(stored.command.checkpoint_id.as_slice())
    .bind(request.command.character_id.as_slice())
    .bind(audit_id.as_slice())
    .bind(stored.request_hash.as_slice())
    .bind(request.request_hash.as_slice())
    .bind(i64_value(request.command.issued_at_unix_ms)?)
    .execute(connection)
    .await?
    .rows_affected();
    if affected > 1 {
        return Err(PersistenceError::CorruptStoredLifeClock);
    }
    Ok(())
}

async fn transaction_timestamp_ms(connection: &mut PgConnection) -> Result<u64, PersistenceError> {
    let value: i64 = sqlx::query_scalar(
        "SELECT CAST(EXTRACT(EPOCH FROM transaction_timestamp()) * 1000 AS BIGINT)",
    )
    .fetch_one(connection)
    .await?;
    positive_u64(value)
}

fn canonical_request_hash(
    command: &LifeClockCheckpointCommandV1,
) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let mut hasher = blake3::Hasher::new();
    hash_field(&mut hasher, REQUEST_HASH_CONTEXT.as_bytes())?;
    hash_field(&mut hasher, &command.account_id)?;
    hash_field(&mut hasher, &command.character_id)?;
    hash_field(&mut hasher, &command.checkpoint_id)?;
    hash_field(
        &mut hasher,
        &command.expected_character_version.to_le_bytes(),
    )?;
    hash_field(
        &mut hasher,
        &command.expected_life_metrics_version.to_le_bytes(),
    )?;
    hash_field(&mut hasher, &command.authoritative_tick.to_le_bytes())?;
    hash_field(&mut hasher, &command.state.code().to_le_bytes())?;
    hash_field(&mut hasher, &command.advanced_ticks.to_le_bytes())?;
    match &command.danger {
        None => hash_field(&mut hasher, &[0])?,
        Some(danger) => {
            hash_field(&mut hasher, &[1])?;
            hash_field(&mut hasher, &danger.lineage_id)?;
            hash_field(&mut hasher, &danger.restore_point_id)?;
            hash_field(
                &mut hasher,
                &danger.entry_life_metrics_version.to_le_bytes(),
            )?;
            hash_field(
                &mut hasher,
                &danger.entry_permadeath_combat_ticks.to_le_bytes(),
            )?;
        }
    }
    hash_field(&mut hasher, command.content.records_blake3.as_bytes())?;
    hash_field(&mut hasher, command.content.assets_blake3.as_bytes())?;
    hash_field(&mut hasher, command.content.localization_blake3.as_bytes())?;
    hash_field(&mut hasher, &command.issued_at_unix_ms.to_le_bytes())?;
    Ok(*hasher.finalize().as_bytes())
}

fn canonical_result_digest(
    stored: &StoredLifeClockCheckpointV1,
) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let mut hasher = blake3::Hasher::new();
    hash_field(&mut hasher, RESULT_DIGEST_CONTEXT.as_bytes())?;
    hash_field(&mut hasher, &stored.contract_version.to_le_bytes())?;
    hash_field(&mut hasher, &stored.request_hash)?;
    hash_field(&mut hasher, &stored.pre_lifetime_ticks.to_le_bytes())?;
    hash_field(&mut hasher, &stored.post_lifetime_ticks.to_le_bytes())?;
    hash_field(
        &mut hasher,
        &stored.pre_permadeath_combat_ticks.to_le_bytes(),
    )?;
    hash_field(
        &mut hasher,
        &stored.post_permadeath_combat_ticks.to_le_bytes(),
    )?;
    hash_field(&mut hasher, &stored.pre_link_lost_ticks.to_le_bytes())?;
    hash_field(&mut hasher, &stored.post_link_lost_ticks.to_le_bytes())?;
    hash_field(&mut hasher, &stored.pre_life_metrics_version.to_le_bytes())?;
    hash_field(&mut hasher, &stored.post_life_metrics_version.to_le_bytes())?;
    hash_field(&mut hasher, &stored.committed_at_unix_ms.to_le_bytes())?;
    Ok(*hasher.finalize().as_bytes())
}

fn conflict_audit_id(
    request: &LifeClockCheckpointRequestV1,
) -> Result<[u8; ID_BYTES], PersistenceError> {
    let mut hasher = blake3::Hasher::new();
    hash_field(&mut hasher, CONFLICT_AUDIT_CONTEXT.as_bytes())?;
    hash_field(&mut hasher, &request.command.account_id)?;
    hash_field(&mut hasher, &request.command.checkpoint_id)?;
    hash_field(&mut hasher, &request.request_hash)?;
    let digest = hasher.finalize();
    let mut id = [0; ID_BYTES];
    id.copy_from_slice(&digest.as_bytes()[..ID_BYTES]);
    if all_zero(&id) {
        return Err(PersistenceError::CorruptStoredLifeClock);
    }
    Ok(id)
}

fn hash_field(hasher: &mut blake3::Hasher, bytes: &[u8]) -> Result<(), PersistenceError> {
    let length = u64::try_from(bytes.len()).map_err(corrupt_conversion)?;
    hasher.update(&length.to_le_bytes());
    hasher.update(bytes);
    Ok(())
}

fn exact_id(value: Vec<u8>) -> Result<[u8; ID_BYTES], PersistenceError> {
    let fixed: [u8; ID_BYTES] = value.try_into().map_err(corrupt_conversion)?;
    if all_zero(&fixed) {
        return Err(PersistenceError::CorruptStoredLifeClock);
    }
    Ok(fixed)
}

fn optional_id(value: Option<Vec<u8>>) -> Result<Option<[u8; ID_BYTES]>, PersistenceError> {
    value.map(exact_id).transpose()
}

fn exact_hash(value: Vec<u8>) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let fixed: [u8; HASH_BYTES] = value.try_into().map_err(corrupt_conversion)?;
    if all_zero(&fixed) {
        return Err(PersistenceError::CorruptStoredLifeClock);
    }
    Ok(fixed)
}

fn positive_u64(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(PersistenceError::CorruptStoredLifeClock)
}

fn nonnegative_u64(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value).map_err(corrupt_conversion)
}

fn i64_value(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(corrupt_conversion)
}

fn corrupt_conversion<T>(_: T) -> PersistenceError {
    PersistenceError::CorruptStoredLifeClock
}

fn all_zero<const N: usize>(value: &[u8; N]) -> bool {
    value.iter().all(|byte| *byte == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn danger() -> LifeClockDangerAuthorityV1 {
        LifeClockDangerAuthorityV1 {
            lineage_id: [6; ID_BYTES],
            restore_point_id: [7; ID_BYTES],
            entry_life_metrics_version: 3,
            entry_permadeath_combat_ticks: 30,
        }
    }

    fn command(state: LifeClockStateV1) -> LifeClockCheckpointCommandV1 {
        LifeClockCheckpointCommandV1 {
            account_id: [1; ID_BYTES],
            character_id: [2; ID_BYTES],
            checkpoint_id: [3; ID_BYTES],
            expected_character_version: 4,
            expected_life_metrics_version: 5,
            authoritative_tick: 60,
            state,
            advanced_ticks: 30,
            danger: state.is_danger().then(danger),
            content: LifeClockContentAuthorityV1::core(),
            issued_at_unix_ms: 8,
        }
    }

    fn stored(state: LifeClockStateV1, pre_link_lost_ticks: u32) -> StoredLifeClockCheckpointV1 {
        let command = command(state);
        let request = LifeClockCheckpointRequestV1::seal(command.clone()).unwrap();
        let lifetime_advance = if state.counts_lifetime() { 30 } else { 0 };
        let combat_advance = if state.counts_combat() { 30 } else { 0 };
        let post_link_lost_ticks = if state == LifeClockStateV1::DangerLinkLost {
            pre_link_lost_ticks + 30
        } else {
            0
        };
        let mut stored = StoredLifeClockCheckpointV1 {
            contract_version: CONTRACT_VERSION,
            command,
            pre_lifetime_ticks: 100,
            post_lifetime_ticks: 100 + lifetime_advance,
            pre_permadeath_combat_ticks: 50,
            post_permadeath_combat_ticks: 50 + combat_advance,
            pre_link_lost_ticks,
            post_link_lost_ticks,
            pre_life_metrics_version: 5,
            post_life_metrics_version: 6,
            request_hash: request.request_hash,
            result_digest: [0; HASH_BYTES],
            committed_at_unix_ms: 9,
        };
        stored.result_digest = canonical_result_digest(&stored).unwrap();
        stored
    }

    #[test]
    fn every_clock_state_has_exact_independent_arithmetic() {
        for state in [
            LifeClockStateV1::CharacterSelect,
            LifeClockStateV1::Loading,
            LifeClockStateV1::Offline,
            LifeClockStateV1::HallControllable,
            LifeClockStateV1::DangerLoading,
            LifeClockStateV1::DangerStaging,
            LifeClockStateV1::DangerControllable,
            LifeClockStateV1::DangerLinkLost,
        ] {
            stored(state, 0).validate().unwrap();
        }
        assert_eq!(
            stored(LifeClockStateV1::HallControllable, 0).post_lifetime_ticks,
            130
        );
        assert_eq!(
            stored(LifeClockStateV1::DangerLoading, 0).post_lifetime_ticks,
            100
        );
        assert_eq!(
            stored(LifeClockStateV1::DangerLoading, 0).post_permadeath_combat_ticks,
            80
        );
    }

    #[test]
    fn request_hash_binds_every_authoritative_axis() {
        let baseline =
            LifeClockCheckpointRequestV1::seal(command(LifeClockStateV1::DangerControllable))
                .unwrap();
        let mut variants = Vec::new();
        let mut changed = baseline.command.clone();
        changed.account_id[0] += 1;
        variants.push(changed);
        let mut changed = baseline.command.clone();
        changed.character_id[0] += 1;
        variants.push(changed);
        let mut changed = baseline.command.clone();
        changed.checkpoint_id[0] += 1;
        variants.push(changed);
        let mut changed = baseline.command.clone();
        changed.expected_character_version += 1;
        variants.push(changed);
        let mut changed = baseline.command.clone();
        changed.expected_life_metrics_version += 1;
        variants.push(changed);
        let mut changed = baseline.command.clone();
        changed.authoritative_tick += 1;
        variants.push(changed);
        let mut changed = baseline.command.clone();
        changed.advanced_ticks += 1;
        variants.push(changed);
        let mut changed = baseline.command.clone();
        changed.state = LifeClockStateV1::DangerLoading;
        variants.push(changed);
        let mut changed = baseline.command.clone();
        changed.danger.as_mut().unwrap().lineage_id[0] += 1;
        variants.push(changed);
        let mut changed = baseline.command.clone();
        changed.danger.as_mut().unwrap().restore_point_id[0] += 1;
        variants.push(changed);
        let mut changed = baseline.command.clone();
        changed.danger.as_mut().unwrap().entry_life_metrics_version -= 1;
        variants.push(changed);
        let mut changed = baseline.command.clone();
        changed
            .danger
            .as_mut()
            .unwrap()
            .entry_permadeath_combat_ticks += 1;
        variants.push(changed);
        let mut changed = baseline.command.clone();
        changed.issued_at_unix_ms += 1;
        variants.push(changed);
        for variant in variants {
            assert_ne!(
                LifeClockCheckpointRequestV1::seal(variant)
                    .unwrap()
                    .request_hash,
                baseline.request_hash
            );
        }
        for mutate in [
            |content: &mut LifeClockContentAuthorityV1| {
                content.records_blake3.replace_range(0..1, "0");
            },
            |content: &mut LifeClockContentAuthorityV1| {
                content.assets_blake3.replace_range(0..1, "0");
            },
            |content: &mut LifeClockContentAuthorityV1| {
                content.localization_blake3.replace_range(0..1, "0");
            },
        ] {
            let mut changed = baseline.command.clone();
            mutate(&mut changed.content);
            assert_ne!(
                canonical_request_hash(&changed).unwrap(),
                baseline.request_hash
            );
        }
    }

    #[test]
    fn invalid_interval_identity_content_and_danger_shape_fail_closed() {
        for mutate in [
            |command: &mut LifeClockCheckpointCommandV1| command.account_id = [0; ID_BYTES],
            |command: &mut LifeClockCheckpointCommandV1| command.character_id = [0; ID_BYTES],
            |command: &mut LifeClockCheckpointCommandV1| command.checkpoint_id = [0; ID_BYTES],
            |command: &mut LifeClockCheckpointCommandV1| command.expected_character_version = 0,
            |command: &mut LifeClockCheckpointCommandV1| {
                command.expected_life_metrics_version = 0;
            },
            |command: &mut LifeClockCheckpointCommandV1| command.authoritative_tick = 0,
            |command: &mut LifeClockCheckpointCommandV1| command.advanced_ticks = 0,
            |command: &mut LifeClockCheckpointCommandV1| {
                command.advanced_ticks = MAX_INTERVAL_TICKS + 1;
            },
            |command: &mut LifeClockCheckpointCommandV1| command.issued_at_unix_ms = 0,
        ] {
            let mut invalid = command(LifeClockStateV1::HallControllable);
            mutate(&mut invalid);
            assert!(matches!(
                LifeClockCheckpointRequestV1::seal(invalid),
                Err(PersistenceError::CorruptStoredLifeClock)
            ));
        }
        let mut missing_danger = command(LifeClockStateV1::DangerControllable);
        missing_danger.danger = None;
        assert!(LifeClockCheckpointRequestV1::seal(missing_danger).is_err());
        let mut unexpected_danger = command(LifeClockStateV1::HallControllable);
        unexpected_danger.danger = Some(danger());
        assert!(LifeClockCheckpointRequestV1::seal(unexpected_danger).is_err());
        let mut wrong_content = command(LifeClockStateV1::HallControllable);
        wrong_content.content.records_blake3 = "0".repeat(64);
        assert!(matches!(
            LifeClockCheckpointRequestV1::seal(wrong_content),
            Err(PersistenceError::LifeClockContentMismatch)
        ));
    }

    #[test]
    fn link_lost_accumulates_to_exact_ninety_and_rejects_ninety_one() {
        let exact = stored(LifeClockStateV1::DangerLinkLost, 60);
        exact.validate().unwrap();
        assert_eq!(exact.post_link_lost_ticks, 90);

        let mut expired = exact;
        expired.pre_link_lost_ticks = 61;
        expired.post_link_lost_ticks = 91;
        expired.result_digest = canonical_result_digest(&expired).unwrap();
        assert!(matches!(
            expired.validate(),
            Err(PersistenceError::LifeClockLinkLostWindowExpired)
        ));
    }

    #[test]
    fn stored_digest_covers_all_derived_values() {
        let baseline = stored(LifeClockStateV1::DangerControllable, 0);
        baseline.validate().unwrap();
        for mutate in [
            |stored: &mut StoredLifeClockCheckpointV1| stored.contract_version += 1,
            |stored: &mut StoredLifeClockCheckpointV1| stored.pre_lifetime_ticks += 1,
            |stored: &mut StoredLifeClockCheckpointV1| stored.post_lifetime_ticks += 1,
            |stored: &mut StoredLifeClockCheckpointV1| {
                stored.pre_permadeath_combat_ticks += 1;
            },
            |stored: &mut StoredLifeClockCheckpointV1| {
                stored.post_permadeath_combat_ticks += 1;
            },
            |stored: &mut StoredLifeClockCheckpointV1| stored.pre_link_lost_ticks += 1,
            |stored: &mut StoredLifeClockCheckpointV1| stored.post_link_lost_ticks += 1,
            |stored: &mut StoredLifeClockCheckpointV1| stored.pre_life_metrics_version += 1,
            |stored: &mut StoredLifeClockCheckpointV1| stored.post_life_metrics_version += 1,
            |stored: &mut StoredLifeClockCheckpointV1| stored.committed_at_unix_ms += 1,
        ] {
            let mut changed = baseline.clone();
            mutate(&mut changed);
            assert_ne!(
                canonical_result_digest(&changed).unwrap(),
                baseline.result_digest
            );
        }
    }

    #[test]
    fn continuity_and_link_lost_reset_are_root_exact() {
        let previous = stored(LifeClockStateV1::DangerLinkLost, 0);
        let mut next = command(LifeClockStateV1::DangerLinkLost);
        next.authoritative_tick = 90;
        assert!(validate_interval_continuity(&next, Some(&previous)).is_ok());
        assert_eq!(prior_link_lost(&next, Some(&previous)), 30);

        next.authoritative_tick = 91;
        assert!(matches!(
            validate_interval_continuity(&next, Some(&previous)),
            Err(PersistenceError::LifeClockTickDiscontinuity {
                expected: 90,
                actual: 91
            })
        ));
        next.authoritative_tick = 90;
        next.danger.as_mut().unwrap().restore_point_id[0] += 1;
        assert_eq!(prior_link_lost(&next, Some(&previous)), 0);

        let at_eighty_nine = stored(LifeClockStateV1::DangerLinkLost, 59);
        let mut reconnected = command(LifeClockStateV1::DangerControllable);
        reconnected.authoritative_tick = 90;
        assert!(validate_interval_continuity(&reconnected, Some(&at_eighty_nine)).is_ok());
        assert_eq!(prior_link_lost(&reconnected, Some(&at_eighty_nine)), 0);

        let at_ninety = stored(LifeClockStateV1::DangerLinkLost, 60);
        assert!(matches!(
            validate_interval_continuity(&reconnected, Some(&at_ninety)),
            Err(PersistenceError::LifeClockTerminalResolutionRequired)
        ));
    }
}
