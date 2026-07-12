use sqlx::Row;

use crate::{PersistenceError, PostgresPersistence, WIPEABLE_CORE_NAMESPACE};

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

impl StoredWorldLocation {
    #[must_use]
    pub const fn character_version(&self) -> i64 {
        match self {
            Self::CharacterSelect { character_version }
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
    pub expected_character_version: i64,
    pub issued_at_unix_millis: i64,
    pub command_kind: i16,
    pub transfer_id: Option<[u8; ID_BYTES]>,
    pub pre_character_version: i64,
    pub post_character_version: i64,
    pub result_code: i16,
    pub result_payload: Vec<u8>,
}

/// Mutable state exposed only inside one serializable account/character transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldFlowTransactionState {
    pub location: StoredWorldLocation,
    pub existing_receipt: Option<StoredWorldTransferReceipt>,
    pub new_receipt: Option<StoredWorldTransferReceipt>,
}

impl PostgresPersistence {
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

    pub async fn transact_world_flow<T, F>(
        &self,
        account_id: [u8; ID_BYTES],
        character_id: [u8; ID_BYTES],
        mutation_id: [u8; ID_BYTES],
        operation: F,
    ) -> Result<T, PersistenceError>
    where
        T: Send,
        F: FnOnce(&mut WorldFlowTransactionState) -> Result<T, PersistenceError> + Send,
    {
        let mut transaction = self.begin_transaction().await?;
        lock_account(transaction.connection(), &account_id).await?;
        let location_row =
            load_location(transaction.connection(), &account_id, &character_id, true)
                .await?
                .ok_or(PersistenceError::WorldFlowCharacterNotFound)?;
        let mut state = WorldFlowTransactionState {
            location: decode_location(&location_row)?,
            existing_receipt: load_receipt(transaction.connection(), &account_id, &mutation_id)
                .await?,
            new_receipt: None,
        };
        let result = operation(&mut state)?;
        persist_location(
            transaction.connection(),
            &account_id,
            &character_id,
            &state.location,
        )
        .await?;
        if let Some(receipt) = &state.new_receipt {
            validate_receipt_binding(receipt, &account_id, &character_id, &mutation_id)?;
            insert_receipt(transaction.connection(), receipt).await?;
        }
        transaction.commit().await?;
        Ok(result)
    }
}

async fn lock_account(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
) -> Result<(), PersistenceError> {
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT state_version FROM accounts \
         WHERE namespace_id = $1 AND account_id = $2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?;
    if exists.is_none() {
        return Err(PersistenceError::WorldFlowCharacterNotFound);
    }
    Ok(())
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
        (0, None, None, None, None, None) => Ok(StoredWorldLocation::CharacterSelect {
            character_version: version,
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
        "SELECT character_id, payload_hash, expected_character_version, \
                floor(extract(epoch FROM issued_at) * 1000)::bigint AS issued_at_unix_millis, \
                command_kind, transfer_id, pre_character_version, post_character_version, \
                result_code, result_payload \
         FROM character_world_transfer_results \
         WHERE namespace_id = $1 AND account_id = $2 AND mutation_id = $3",
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

async fn persist_location(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    location: &StoredWorldLocation,
) -> Result<(), PersistenceError> {
    let (version, kind, content, arrival, spawn, lineage, restore) = match location {
        StoredWorldLocation::CharacterSelect { character_version } => {
            (*character_version, 0_i16, None, None, None, None, None)
        }
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
    };
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
          expected_character_version, issued_at, command_kind, transfer_id, \
          pre_character_version, post_character_version, result_code, result_payload) \
         VALUES ($1, $2, $3, $4, $5, $6, to_timestamp($7::double precision / 1000.0), \
                 $8, $9, $10, $11, $12, $13)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(receipt.account_id.as_slice())
    .bind(receipt.character_id.as_slice())
    .bind(receipt.mutation_id.as_slice())
    .bind(receipt.payload_hash.as_slice())
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
        || receipt.expected_character_version <= 0
        || receipt.issued_at_unix_millis <= 0
        || receipt.pre_character_version <= 0
        || receipt.post_character_version <= 0
        || receipt.result_payload.is_empty()
        || receipt.result_payload.len() > 65_536
    {
        return Err(PersistenceError::CorruptStoredWorldFlow);
    }
    Ok(())
}

fn fixed_bytes<const N: usize>(bytes: Vec<u8>) -> Result<[u8; N], PersistenceError> {
    bytes
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredWorldFlow)
}
