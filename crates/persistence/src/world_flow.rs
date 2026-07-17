use sqlx::Row;

use crate::{
    PersistenceError, PersistenceTransaction, PostgresPersistence, WIPEABLE_CORE_NAMESPACE,
};

const ID_BYTES: usize = 16;
const HASH_BYTES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredSafeArrival {
    HallDefault,
    SpawnAnchor(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredWorldLocation {
    CharacterSelect {
        character_version: i64,
        next_hall_arrival: StoredSafeArrival,
    },
    Safe {
        character_version: i64,
        location_content_id: String,
        arrival: StoredSafeArrival,
    },
    Danger {
        character_version: i64,
        location_content_id: String,
        instance_lineage_id: [u8; ID_BYTES],
        entry_restore_point_id: [u8; ID_BYTES],
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StoredWorldFlowRevisionV1 {
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
}

impl StoredWorldLocation {
    #[must_use]
    pub const fn character_version(&self) -> i64 {
        match self {
            Self::CharacterSelect {
                character_version, ..
            }
            | Self::Safe {
                character_version, ..
            }
            | Self::Danger {
                character_version, ..
            } => *character_version,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredWorldTransferReceipt {
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub mutation_id: [u8; ID_BYTES],
    pub payload_hash: [u8; HASH_BYTES],
    pub content_revision: StoredWorldFlowRevisionV1,
    pub expected_character_version: i64,
    pub issued_at_unix_millis: i64,
    pub command_kind: i16,
    pub transfer_id: Option<[u8; ID_BYTES]>,
    pub pre_character_version: i64,
    pub post_character_version: i64,
    pub result_code: i16,
    pub result_payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredWorldFlowCharacter {
    pub life_state: i16,
    pub security_state: i16,
    pub character_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDangerEntryRootV3 {
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub lineage_id: [u8; ID_BYTES],
    pub restore_point_id: [u8; ID_BYTES],
    pub source_location_id: String,
    pub danger_location_id: String,
    pub layout_id: String,
    pub content_revision: StoredWorldFlowRevisionV1,
    pub account_version: i64,
    pub character_version: i64,
    pub progression_version: i64,
    pub inventory_version: i64,
    pub oath_bargain_version: i64,
    pub life_metrics_version: i64,
    pub ash_wallet_version: i64,
    pub composite_digest: [u8; HASH_BYTES],
}

/// Mutable state exposed only inside one serializable account/character transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldFlowTransactionState {
    pub account_version: i64,
    pub selected_character_id: Option<[u8; ID_BYTES]>,
    pub character: StoredWorldFlowCharacter,
    pub location: StoredWorldLocation,
    pub new_receipt: Option<StoredWorldTransferReceipt>,
    pub location_changed: bool,
}

/// Replay is read-only and returns before character/location validation or mutation staging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldFlowTransaction<T> {
    Replayed(Box<StoredWorldTransferReceipt>),
    Committed(T),
}

pub enum WorldFlowBegin<'pool> {
    Replayed(Box<StoredWorldTransferReceipt>),
    Fresh(Box<WorldFlowWrite<'pool>>),
}

/// Owned serializable world-flow write. Only `commit` can publish staged state.
pub struct WorldFlowWrite<'pool> {
    transaction: PersistenceTransaction<'pool>,
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
    mutation_id: [u8; ID_BYTES],
    initial_location: StoredWorldLocation,
    state: WorldFlowTransactionState,
}

impl<'pool> WorldFlowWrite<'pool> {
    #[must_use]
    pub const fn state(&self) -> &WorldFlowTransactionState {
        &self.state
    }

    pub const fn state_mut(&mut self) -> &mut WorldFlowTransactionState {
        &mut self.state
    }

    pub const fn transaction_mut(&mut self) -> &mut PersistenceTransaction<'pool> {
        &mut self.transaction
    }

    /// Explicitly releases the serializable aggregate locks without publishing staged state.
    /// Two-phase live-authority preparation uses this boundary before awaiting an actor.
    pub async fn rollback(self) -> Result<(), PersistenceError> {
        self.transaction.rollback().await
    }

    pub async fn commit<T>(
        mut self,
        result: T,
    ) -> Result<WorldFlowTransaction<T>, PersistenceError> {
        if self.state.location_changed {
            if self.state.location == self.initial_location
                || self.state.location.character_version()
                    != self
                        .initial_location
                        .character_version()
                        .checked_add(1)
                        .ok_or(PersistenceError::CorruptStoredWorldFlow)?
            {
                return Err(PersistenceError::CorruptStoredWorldFlow);
            }
            persist_location(
                self.transaction.connection(),
                &self.account_id,
                &self.character_id,
                &self.state.location,
            )
            .await?;
        } else if self.state.location != self.initial_location {
            return Err(PersistenceError::CorruptStoredWorldFlow);
        }
        let receipt = self
            .state
            .new_receipt
            .as_ref()
            .ok_or(PersistenceError::WorldFlowResultRequired)?;
        if receipt.pre_character_version != self.initial_location.character_version()
            || receipt.post_character_version != self.state.location.character_version()
            || self.state.location_changed != (receipt.result_code == 0)
        {
            return Err(PersistenceError::CorruptStoredWorldFlow);
        }
        validate_receipt_binding(
            receipt,
            &self.account_id,
            &self.character_id,
            &self.mutation_id,
        )?;
        insert_receipt(self.transaction.connection(), receipt).await?;
        self.transaction.commit().await?;
        Ok(WorldFlowTransaction::Committed(result))
    }
}

impl PostgresPersistence {
    pub async fn world_flow_selected_character(
        &self,
        account_id: [u8; ID_BYTES],
    ) -> Result<Option<[u8; ID_BYTES]>, PersistenceError> {
        let mut transaction = self.begin_transaction().await?;
        let selected: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT selected_character_id FROM accounts \
             WHERE namespace_id = $1 AND account_id = $2",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .fetch_optional(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?
        .flatten();
        transaction.rollback().await?;
        selected.map(fixed_bytes).transpose()
    }

    pub async fn world_location(
        &self,
        account_id: [u8; ID_BYTES],
        character_id: [u8; ID_BYTES],
    ) -> Result<Option<StoredWorldLocation>, PersistenceError> {
        let mut transaction = self.begin_transaction().await?;
        let row =
            load_location(transaction.connection(), &account_id, &character_id, false).await?;
        transaction.rollback().await?;
        row.map(|row| decode_location(&row)).transpose()
    }

    pub async fn begin_world_flow(
        &self,
        account_id: [u8; ID_BYTES],
        character_id: [u8; ID_BYTES],
        mutation_id: [u8; ID_BYTES],
    ) -> Result<WorldFlowBegin<'_>, PersistenceError> {
        let mut transaction = self.begin_transaction().await?;
        let account = lock_account(transaction.connection(), &account_id).await?;
        if let Some(receipt) =
            load_receipt(transaction.connection(), &account_id, &mutation_id).await?
        {
            transaction.rollback().await?;
            return Ok(WorldFlowBegin::Replayed(Box::new(receipt)));
        }
        let character = lock_character(transaction.connection(), &account_id, &character_id)
            .await?
            .ok_or(PersistenceError::WorldFlowCharacterNotFound)?;
        if character.life_state == 1 {
            return Err(PersistenceError::WorldFlowCharacterDead);
        }
        if character.life_state != 0 {
            return Err(PersistenceError::CorruptStoredWorldFlow);
        }
        let location_row =
            load_location(transaction.connection(), &account_id, &character_id, true)
                .await?
                .ok_or(PersistenceError::WorldFlowCharacterNotFound)?;
        let location = decode_location(&location_row)?;
        if character.character_version != location.character_version() {
            return Err(PersistenceError::CorruptStoredWorldFlow);
        }
        let initial_location = location.clone();
        let state = WorldFlowTransactionState {
            account_version: account.state_version,
            selected_character_id: account.selected_character_id,
            character,
            location,
            new_receipt: None,
            location_changed: false,
        };
        Ok(WorldFlowBegin::Fresh(Box::new(WorldFlowWrite {
            transaction,
            account_id,
            character_id,
            mutation_id,
            initial_location,
            state,
        })))
    }
}

/// Stages the immutable lineage and component-complete v3 restore root inside a caller-owned
/// transaction.
/// Provider-owned component rows may be inserted before this call because their root foreign keys
/// are deferred until commit.
pub async fn stage_world_flow_danger_entry(
    transaction: &mut PersistenceTransaction<'_>,
    root: &StoredDangerEntryRootV3,
) -> Result<(), PersistenceError> {
    validate_danger_entry_root(root)?;
    sqlx::query(
        "INSERT INTO character_instance_lineages \
         (namespace_id, account_id, character_id, lineage_id, content_id, layout_id, \
          lineage_state, records_blake3, assets_blake3, localization_blake3) \
         VALUES ($1, $2, $3, $4, $5, $6, 0, $7, $8, $9)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(root.account_id.as_slice())
    .bind(root.character_id.as_slice())
    .bind(root.lineage_id.as_slice())
    .bind(&root.danger_location_id)
    .bind(&root.layout_id)
    .bind(&root.content_revision.records_blake3)
    .bind(&root.content_revision.assets_blake3)
    .bind(&root.content_revision.localization_blake3)
    .execute(transaction.connection())
    .await
    .map_err(PersistenceError::Database)?;
    sqlx::query(
        "INSERT INTO character_entry_restore_points \
         (namespace_id, account_id, character_id, restore_point_id, lineage_id, \
          source_location_id, restore_location_id, snapshot_contract_version, account_version, \
          character_version, progression_version, inventory_version, oath_bargain_version, \
          life_metrics_version, ash_wallet_version, component_mask, composite_digest, restore_state, records_blake3, assets_blake3, \
          localization_blake3) \
         VALUES ($1, $2, $3, $4, $5, $6, 'hub.lantern_halls_01', 3, $7, $8, $9, $10, \
                 $11, $12, $13, 31, $14, 0, $15, $16, $17)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(root.account_id.as_slice())
    .bind(root.character_id.as_slice())
    .bind(root.restore_point_id.as_slice())
    .bind(root.lineage_id.as_slice())
    .bind(&root.source_location_id)
    .bind(root.account_version)
    .bind(root.character_version)
    .bind(root.progression_version)
    .bind(root.inventory_version)
    .bind(root.oath_bargain_version)
    .bind(root.life_metrics_version)
    .bind(root.ash_wallet_version)
    .bind(root.composite_digest.as_slice())
    .bind(&root.content_revision.records_blake3)
    .bind(&root.content_revision.assets_blake3)
    .bind(&root.content_revision.localization_blake3)
    .execute(transaction.connection())
    .await
    .map_err(PersistenceError::Database)?;
    Ok(())
}

fn validate_danger_entry_root(root: &StoredDangerEntryRootV3) -> Result<(), PersistenceError> {
    let bounded_content_id = |value: &str| (3..=96).contains(&value.len());
    if [
        &root.account_id,
        &root.character_id,
        &root.lineage_id,
        &root.restore_point_id,
    ]
    .into_iter()
    .any(|value| value.iter().all(|byte| *byte == 0))
        || root.lineage_id == root.restore_point_id
        || !bounded_content_id(&root.source_location_id)
        || !bounded_content_id(&root.danger_location_id)
        || !bounded_content_id(&root.layout_id)
        || !valid_revision(&root.content_revision)
        || [
            root.account_version,
            root.character_version,
            root.progression_version,
            root.inventory_version,
            root.oath_bargain_version,
            root.life_metrics_version,
            root.ash_wallet_version,
        ]
        .into_iter()
        .any(|version| version <= 0)
        || root.composite_digest.iter().all(|byte| *byte == 0)
    {
        return Err(PersistenceError::CorruptStoredWorldFlow);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LockedWorldFlowAccount {
    state_version: i64,
    selected_character_id: Option<[u8; ID_BYTES]>,
}

async fn lock_account(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
) -> Result<LockedWorldFlowAccount, PersistenceError> {
    let row = sqlx::query(
        "SELECT state_version, selected_character_id FROM accounts \
         WHERE namespace_id = $1 AND account_id = $2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?
    .ok_or(PersistenceError::WorldFlowCharacterNotFound)?;
    let state_version = row
        .try_get("state_version")
        .map_err(PersistenceError::Database)?;
    let selected_character_id = row
        .try_get::<Option<Vec<u8>>, _>("selected_character_id")
        .map_err(PersistenceError::Database)?
        .map(fixed_bytes)
        .transpose()?;
    if state_version <= 0 {
        return Err(PersistenceError::CorruptStoredWorldFlow);
    }
    Ok(LockedWorldFlowAccount {
        state_version,
        selected_character_id,
    })
}

async fn lock_character(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
) -> Result<Option<StoredWorldFlowCharacter>, PersistenceError> {
    let row = sqlx::query(
        "SELECT life_state, security_state, character_state_version FROM characters \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?;
    row.map(|row| {
        let character = StoredWorldFlowCharacter {
            life_state: row
                .try_get("life_state")
                .map_err(PersistenceError::Database)?,
            security_state: row
                .try_get("security_state")
                .map_err(PersistenceError::Database)?,
            character_version: row
                .try_get("character_state_version")
                .map_err(PersistenceError::Database)?,
        };
        if character.character_version <= 0 {
            return Err(PersistenceError::CorruptStoredWorldFlow);
        }
        Ok(character)
    })
    .transpose()
}

async fn load_location(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    lock: bool,
) -> Result<Option<sqlx::postgres::PgRow>, PersistenceError> {
    let row = if lock {
        sqlx::query(
            "SELECT character_version, location_kind, location_content_id, safe_arrival_kind, \
                    safe_spawn_id, instance_lineage_id, entry_restore_point_id \
             FROM character_world_locations \
             WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3 FOR UPDATE",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .fetch_optional(&mut *connection)
        .await
    } else {
        sqlx::query(
            "SELECT character_version, location_kind, location_content_id, safe_arrival_kind, \
                    safe_spawn_id, instance_lineage_id, entry_restore_point_id \
             FROM character_world_locations \
             WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .fetch_optional(connection)
        .await
    };
    row.map_err(PersistenceError::Database)
}

fn decode_location(row: &sqlx::postgres::PgRow) -> Result<StoredWorldLocation, PersistenceError> {
    let version: i64 = row
        .try_get("character_version")
        .map_err(PersistenceError::Database)?;
    let kind: i16 = row
        .try_get("location_kind")
        .map_err(PersistenceError::Database)?;
    let content_id: Option<String> = row
        .try_get("location_content_id")
        .map_err(PersistenceError::Database)?;
    let arrival_kind: Option<i16> = row
        .try_get("safe_arrival_kind")
        .map_err(PersistenceError::Database)?;
    let spawn_id: Option<String> = row
        .try_get("safe_spawn_id")
        .map_err(PersistenceError::Database)?;
    let lineage: Option<Vec<u8>> = row
        .try_get("instance_lineage_id")
        .map_err(PersistenceError::Database)?;
    let restore: Option<Vec<u8>> = row
        .try_get("entry_restore_point_id")
        .map_err(PersistenceError::Database)?;
    if version <= 0 {
        return Err(PersistenceError::CorruptStoredWorldFlow);
    }
    match (kind, content_id, arrival_kind, spawn_id, lineage, restore) {
        (0, None, Some(0), None, None, None) => Ok(StoredWorldLocation::CharacterSelect {
            character_version: version,
            next_hall_arrival: StoredSafeArrival::HallDefault,
        }),
        (0, None, Some(1), Some(spawn), None, None) => Ok(StoredWorldLocation::CharacterSelect {
            character_version: version,
            next_hall_arrival: StoredSafeArrival::SpawnAnchor(spawn),
        }),
        (1, Some(location_content_id), Some(0), None, None, None) => {
            Ok(StoredWorldLocation::Safe {
                character_version: version,
                location_content_id,
                arrival: StoredSafeArrival::HallDefault,
            })
        }
        (1, Some(location_content_id), Some(1), Some(spawn), None, None) => {
            Ok(StoredWorldLocation::Safe {
                character_version: version,
                location_content_id,
                arrival: StoredSafeArrival::SpawnAnchor(spawn),
            })
        }
        (2, Some(location_content_id), None, None, Some(lineage), Some(restore)) => {
            Ok(StoredWorldLocation::Danger {
                character_version: version,
                location_content_id,
                instance_lineage_id: fixed_bytes(lineage)?,
                entry_restore_point_id: fixed_bytes(restore)?,
            })
        }
        _ => Err(PersistenceError::CorruptStoredWorldFlow),
    }
}

async fn load_receipt(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    mutation_id: &[u8; ID_BYTES],
) -> Result<Option<StoredWorldTransferReceipt>, PersistenceError> {
    let row = sqlx::query(
        "SELECT character_id, payload_hash, records_blake3, assets_blake3, \
                localization_blake3, expected_character_version, \
                floor(extract(epoch FROM issued_at) * 1000)::bigint AS issued_at_unix_millis, \
                command_kind, transfer_id, pre_character_version, post_character_version, \
                result_code, result_payload \
         FROM character_world_transfer_results \
         WHERE namespace_id = $1 AND account_id = $2 AND mutation_id = $3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(mutation_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?;
    row.map(|row| {
        Ok(StoredWorldTransferReceipt {
            account_id: *account_id,
            character_id: fixed_bytes(
                row.try_get("character_id")
                    .map_err(PersistenceError::Database)?,
            )?,
            mutation_id: *mutation_id,
            payload_hash: fixed_bytes(
                row.try_get("payload_hash")
                    .map_err(PersistenceError::Database)?,
            )?,
            content_revision: StoredWorldFlowRevisionV1 {
                records_blake3: row
                    .try_get("records_blake3")
                    .map_err(PersistenceError::Database)?,
                assets_blake3: row
                    .try_get("assets_blake3")
                    .map_err(PersistenceError::Database)?,
                localization_blake3: row
                    .try_get("localization_blake3")
                    .map_err(PersistenceError::Database)?,
            },
            expected_character_version: row
                .try_get("expected_character_version")
                .map_err(PersistenceError::Database)?,
            issued_at_unix_millis: row
                .try_get("issued_at_unix_millis")
                .map_err(PersistenceError::Database)?,
            command_kind: row
                .try_get("command_kind")
                .map_err(PersistenceError::Database)?,
            transfer_id: row
                .try_get::<Option<Vec<u8>>, _>("transfer_id")
                .map_err(PersistenceError::Database)?
                .map(fixed_bytes)
                .transpose()?,
            pre_character_version: row
                .try_get("pre_character_version")
                .map_err(PersistenceError::Database)?,
            post_character_version: row
                .try_get("post_character_version")
                .map_err(PersistenceError::Database)?,
            result_code: row
                .try_get("result_code")
                .map_err(PersistenceError::Database)?,
            result_payload: row
                .try_get("result_payload")
                .map_err(PersistenceError::Database)?,
        })
    })
    .transpose()
}

type StoredLocationColumns<'a> = (
    i64,
    i16,
    Option<&'a str>,
    Option<i16>,
    Option<&'a str>,
    Option<&'a [u8]>,
    Option<&'a [u8]>,
);

fn location_columns(location: &StoredWorldLocation) -> StoredLocationColumns<'_> {
    match location {
        StoredWorldLocation::CharacterSelect {
            character_version,
            next_hall_arrival,
        } => match next_hall_arrival {
            StoredSafeArrival::HallDefault => (
                *character_version,
                0_i16,
                None,
                Some(0_i16),
                None,
                None,
                None,
            ),
            StoredSafeArrival::SpawnAnchor(spawn) => (
                *character_version,
                0_i16,
                None,
                Some(1_i16),
                Some(spawn.as_str()),
                None,
                None,
            ),
        },
        StoredWorldLocation::Safe {
            character_version,
            location_content_id,
            arrival,
        } => match arrival {
            StoredSafeArrival::HallDefault => (
                *character_version,
                1,
                Some(location_content_id.as_str()),
                Some(0_i16),
                None,
                None,
                None,
            ),
            StoredSafeArrival::SpawnAnchor(spawn) => (
                *character_version,
                1,
                Some(location_content_id.as_str()),
                Some(1_i16),
                Some(spawn.as_str()),
                None,
                None,
            ),
        },
        StoredWorldLocation::Danger {
            character_version,
            location_content_id,
            instance_lineage_id,
            entry_restore_point_id,
        } => (
            *character_version,
            2,
            Some(location_content_id.as_str()),
            None,
            None,
            Some(instance_lineage_id.as_slice()),
            Some(entry_restore_point_id.as_slice()),
        ),
    }
}

async fn persist_location(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    location: &StoredWorldLocation,
) -> Result<(), PersistenceError> {
    let (version, kind, content, arrival, spawn, lineage, restore) = location_columns(location);
    if version <= 0 {
        return Err(PersistenceError::CorruptStoredWorldFlow);
    }
    sqlx::query(
        "UPDATE character_world_locations SET character_version = $1, location_kind = $2, \
                location_content_id = $3, safe_arrival_kind = $4, safe_spawn_id = $5, \
                instance_lineage_id = $6, entry_restore_point_id = $7, \
                updated_at = transaction_timestamp() \
         WHERE namespace_id = $8 AND account_id = $9 AND character_id = $10",
    )
    .bind(version)
    .bind(kind)
    .bind(content)
    .bind(arrival)
    .bind(spawn)
    .bind(lineage)
    .bind(restore)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(&mut *connection)
    .await
    .map_err(PersistenceError::Database)?;
    sqlx::query(
        "UPDATE characters SET character_state_version = $1, \
                updated_at = transaction_timestamp() \
         WHERE namespace_id = $2 AND account_id = $3 AND character_id = $4",
    )
    .bind(version)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(connection)
    .await
    .map_err(PersistenceError::Database)?;
    Ok(())
}

async fn insert_receipt(
    connection: &mut sqlx::PgConnection,
    receipt: &StoredWorldTransferReceipt,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO character_world_transfer_results \
         (namespace_id, account_id, character_id, mutation_id, payload_hash, \
          records_blake3, assets_blake3, localization_blake3, \
          expected_character_version, issued_at, command_kind, transfer_id, \
          pre_character_version, post_character_version, result_code, result_payload) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, \
                 to_timestamp($10::double precision / 1000.0), \
                 $11, $12, $13, $14, $15, $16)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(receipt.account_id.as_slice())
    .bind(receipt.character_id.as_slice())
    .bind(receipt.mutation_id.as_slice())
    .bind(receipt.payload_hash.as_slice())
    .bind(&receipt.content_revision.records_blake3)
    .bind(&receipt.content_revision.assets_blake3)
    .bind(&receipt.content_revision.localization_blake3)
    .bind(receipt.expected_character_version)
    .bind(receipt.issued_at_unix_millis)
    .bind(receipt.command_kind)
    .bind(receipt.transfer_id.as_ref().map(<[u8; ID_BYTES]>::as_slice))
    .bind(receipt.pre_character_version)
    .bind(receipt.post_character_version)
    .bind(receipt.result_code)
    .bind(&receipt.result_payload)
    .execute(connection)
    .await
    .map_err(PersistenceError::Database)?;
    Ok(())
}

fn validate_receipt_binding(
    receipt: &StoredWorldTransferReceipt,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    mutation_id: &[u8; ID_BYTES],
) -> Result<(), PersistenceError> {
    if &receipt.account_id != account_id
        || &receipt.character_id != character_id
        || &receipt.mutation_id != mutation_id
        || receipt.payload_hash.iter().all(|byte| *byte == 0)
        || !valid_revision(&receipt.content_revision)
        || receipt.expected_character_version <= 0
        || receipt.issued_at_unix_millis <= 0
        || !(0..=3).contains(&receipt.command_kind)
        || receipt.pre_character_version <= 0
        || receipt.post_character_version <= 0
        || !(0..=20).contains(&receipt.result_code)
        || (receipt.result_code == 0)
            != (receipt.transfer_id.is_some()
                && receipt.post_character_version
                    == receipt
                        .pre_character_version
                        .checked_add(1)
                        .unwrap_or(i64::MIN))
        || (receipt.result_code != 0
            && (receipt.transfer_id.is_some()
                || receipt.post_character_version != receipt.pre_character_version))
        || receipt.result_payload.is_empty()
        || receipt.result_payload.len() > 65_536
    {
        return Err(PersistenceError::CorruptStoredWorldFlow);
    }
    Ok(())
}

fn valid_revision(revision: &StoredWorldFlowRevisionV1) -> bool {
    [
        &revision.records_blake3,
        &revision.assets_blake3,
        &revision.localization_blake3,
    ]
    .into_iter()
    .all(|value| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn fixed_bytes<const N: usize>(bytes: Vec<u8>) -> Result<[u8; N], PersistenceError> {
    bytes
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredWorldFlow)
}
