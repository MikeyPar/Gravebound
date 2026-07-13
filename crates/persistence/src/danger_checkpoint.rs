//! Version- and lineage-bound danger checkpoint storage for `GB-M03-05F`.

use sqlx::Row;

use crate::{
    PersistenceError, PostgresPersistence, StoredWorldFlowRevisionV1, WIPEABLE_CORE_NAMESPACE,
};

const ID_BYTES: usize = 16;
const HASH_BYTES: usize = 32;
const COMPONENT_MASK_WITH_BELL_DEBT: i16 = 15;
const BELL_CHECKPOINT_SCHEMA_VERSION: i16 = 1;
const MAX_CHECKPOINT_PAYLOAD_BYTES: usize = 4_096;
const MAX_SERIALIZATION_ATTEMPTS: u8 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDangerCheckpoint {
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub lineage_id: [u8; ID_BYTES],
    pub checkpoint_tick: i64,
    pub content_revision: StoredWorldFlowRevisionV1,
    pub composite_digest: [u8; HASH_BYTES],
    pub character_version: i64,
    pub progression_version: i64,
    pub inventory_version: i64,
    pub oath_bargain_version: i64,
    pub checkpoint_schema_version: i16,
    pub checkpoint_payload: Vec<u8>,
    pub checkpoint_payload_digest: [u8; HASH_BYTES],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DangerCheckpointWrite {
    Created,
    Advanced,
    Replayed,
}

impl PostgresPersistence {
    pub async fn danger_checkpoint(
        &self,
        account_id: [u8; ID_BYTES],
        character_id: [u8; ID_BYTES],
    ) -> Result<Option<StoredDangerCheckpoint>, PersistenceError> {
        if all_zero(&account_id) || all_zero(&character_id) {
            return Err(PersistenceError::CorruptStoredDangerCheckpoint);
        }
        let mut transaction = self.begin_transaction().await?;
        let stored =
            load_checkpoint(transaction.connection(), &account_id, &character_id, false).await?;
        if let Some(checkpoint) = &stored {
            validate_checkpoint(checkpoint)?;
            validate_current_binding(transaction.connection(), checkpoint).await?;
        }
        transaction.rollback().await?;
        Ok(stored)
    }

    pub async fn write_danger_checkpoint(
        &self,
        checkpoint: &StoredDangerCheckpoint,
    ) -> Result<DangerCheckpointWrite, PersistenceError> {
        validate_checkpoint(checkpoint)?;
        for attempt in 1..=MAX_SERIALIZATION_ATTEMPTS {
            match self.write_danger_checkpoint_once(checkpoint).await {
                Err(error)
                    if attempt < MAX_SERIALIZATION_ATTEMPTS
                        && crate::is_retryable_transaction_failure(&error) => {}
                result => return result,
            }
        }
        unreachable!("bounded danger-checkpoint transaction loop always returns")
    }

    async fn write_danger_checkpoint_once(
        &self,
        checkpoint: &StoredDangerCheckpoint,
    ) -> Result<DangerCheckpointWrite, PersistenceError> {
        let mut transaction = self.begin_transaction().await?;
        validate_current_binding(transaction.connection(), checkpoint).await?;
        let existing = load_checkpoint(
            transaction.connection(),
            &checkpoint.account_id,
            &checkpoint.character_id,
            true,
        )
        .await?;
        let outcome = match existing {
            Some(existing) if existing.checkpoint_tick > checkpoint.checkpoint_tick => {
                return Err(PersistenceError::StaleDangerCheckpoint);
            }
            Some(existing) if existing.checkpoint_tick == checkpoint.checkpoint_tick => {
                validate_checkpoint(&existing)?;
                if existing != *checkpoint {
                    return Err(PersistenceError::DangerCheckpointReplayConflict);
                }
                transaction.rollback().await?;
                return Ok(DangerCheckpointWrite::Replayed);
            }
            Some(_) => DangerCheckpointWrite::Advanced,
            None => DangerCheckpointWrite::Created,
        };
        sqlx::query(
            "INSERT INTO character_danger_checkpoints \
             (namespace_id, account_id, character_id, lineage_id, checkpoint_tick, component_mask, \
              composite_digest, character_version, progression_version, inventory_version, \
              oath_bargain_version, records_blake3, assets_blake3, localization_blake3, \
              checkpoint_schema_version, checkpoint_payload, checkpoint_payload_digest) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17) \
             ON CONFLICT (namespace_id, account_id, character_id) DO UPDATE SET \
              lineage_id = EXCLUDED.lineage_id, checkpoint_tick = EXCLUDED.checkpoint_tick, \
              component_mask = EXCLUDED.component_mask, composite_digest = EXCLUDED.composite_digest, \
              character_version = EXCLUDED.character_version, progression_version = EXCLUDED.progression_version, \
              inventory_version = EXCLUDED.inventory_version, oath_bargain_version = EXCLUDED.oath_bargain_version, \
              records_blake3 = EXCLUDED.records_blake3, assets_blake3 = EXCLUDED.assets_blake3, \
              localization_blake3 = EXCLUDED.localization_blake3, \
              checkpoint_schema_version = EXCLUDED.checkpoint_schema_version, \
              checkpoint_payload = EXCLUDED.checkpoint_payload, \
              checkpoint_payload_digest = EXCLUDED.checkpoint_payload_digest, \
              updated_at = transaction_timestamp()",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(checkpoint.account_id.as_slice())
        .bind(checkpoint.character_id.as_slice())
        .bind(checkpoint.lineage_id.as_slice())
        .bind(checkpoint.checkpoint_tick)
        .bind(COMPONENT_MASK_WITH_BELL_DEBT)
        .bind(checkpoint.composite_digest.as_slice())
        .bind(checkpoint.character_version)
        .bind(checkpoint.progression_version)
        .bind(checkpoint.inventory_version)
        .bind(checkpoint.oath_bargain_version)
        .bind(&checkpoint.content_revision.records_blake3)
        .bind(&checkpoint.content_revision.assets_blake3)
        .bind(&checkpoint.content_revision.localization_blake3)
        .bind(checkpoint.checkpoint_schema_version)
        .bind(&checkpoint.checkpoint_payload)
        .bind(checkpoint.checkpoint_payload_digest.as_slice())
        .execute(transaction.connection())
        .await?;
        transaction.commit().await?;
        Ok(outcome)
    }
}

async fn validate_current_binding(
    connection: &mut sqlx::PgConnection,
    checkpoint: &StoredDangerCheckpoint,
) -> Result<(), PersistenceError> {
    let row = sqlx::query(
        "SELECT c.character_state_version, p.progression_version, i.inventory_version, \
                ob.oath_bargain_version, w.location_kind, w.instance_lineage_id, \
                l.lineage_state, l.records_blake3, l.assets_blake3, l.localization_blake3 \
         FROM characters c \
         JOIN character_progression p USING (namespace_id, account_id, character_id) \
         JOIN character_inventories i USING (namespace_id, account_id, character_id) \
         JOIN character_oath_bargain_state ob USING (namespace_id, account_id, character_id) \
         JOIN character_world_locations w USING (namespace_id, account_id, character_id) \
         JOIN character_instance_lineages l ON l.namespace_id = c.namespace_id \
              AND l.account_id = c.account_id AND l.character_id = c.character_id \
              AND l.lineage_id = w.instance_lineage_id \
         WHERE c.namespace_id = $1 AND c.account_id = $2 AND c.character_id = $3 \
         FOR UPDATE OF c, p, i, ob, w, l",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(checkpoint.account_id.as_slice())
    .bind(checkpoint.character_id.as_slice())
    .fetch_optional(connection)
    .await?
    .ok_or(PersistenceError::DangerCheckpointCharacterNotFound)?;

    let lineage_id = fixed_bytes(row.try_get("instance_lineage_id")?)?;
    let bound = row.try_get::<i64, _>("character_state_version")? == checkpoint.character_version
        && row.try_get::<i64, _>("progression_version")? == checkpoint.progression_version
        && row.try_get::<i64, _>("inventory_version")? == checkpoint.inventory_version
        && row.try_get::<i64, _>("oath_bargain_version")? == checkpoint.oath_bargain_version
        && row.try_get::<i16, _>("location_kind")? == 2
        && lineage_id == checkpoint.lineage_id
        && matches!(row.try_get::<i16, _>("lineage_state")?, 0 | 1)
        && row.try_get::<String, _>("records_blake3")?
            == checkpoint.content_revision.records_blake3
        && row.try_get::<String, _>("assets_blake3")? == checkpoint.content_revision.assets_blake3
        && row.try_get::<String, _>("localization_blake3")?
            == checkpoint.content_revision.localization_blake3;
    if !bound {
        return Err(PersistenceError::StaleDangerCheckpoint);
    }
    Ok(())
}

async fn load_checkpoint(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    lock: bool,
) -> Result<Option<StoredDangerCheckpoint>, PersistenceError> {
    let sql = if lock {
        "SELECT lineage_id, checkpoint_tick, component_mask, composite_digest, character_version, \
                progression_version, inventory_version, oath_bargain_version, records_blake3, \
                assets_blake3, localization_blake3, checkpoint_schema_version, checkpoint_payload, \
                checkpoint_payload_digest FROM character_danger_checkpoints \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3 FOR UPDATE"
    } else {
        "SELECT lineage_id, checkpoint_tick, component_mask, composite_digest, character_version, \
                progression_version, inventory_version, oath_bargain_version, records_blake3, \
                assets_blake3, localization_blake3, checkpoint_schema_version, checkpoint_payload, \
                checkpoint_payload_digest FROM character_danger_checkpoints \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3"
    };
    let row = sqlx::query(sql)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .fetch_optional(connection)
        .await?;
    row.map(|row| {
        if row.try_get::<i16, _>("component_mask")? != COMPONENT_MASK_WITH_BELL_DEBT {
            return Err(PersistenceError::CorruptStoredDangerCheckpoint);
        }
        Ok(StoredDangerCheckpoint {
            account_id: *account_id,
            character_id: *character_id,
            lineage_id: fixed_bytes(row.try_get("lineage_id")?)?,
            checkpoint_tick: row.try_get("checkpoint_tick")?,
            content_revision: StoredWorldFlowRevisionV1 {
                records_blake3: row.try_get("records_blake3")?,
                assets_blake3: row.try_get("assets_blake3")?,
                localization_blake3: row.try_get("localization_blake3")?,
            },
            composite_digest: fixed_bytes(row.try_get("composite_digest")?)?,
            character_version: row.try_get("character_version")?,
            progression_version: row.try_get("progression_version")?,
            inventory_version: row.try_get("inventory_version")?,
            oath_bargain_version: row.try_get("oath_bargain_version")?,
            checkpoint_schema_version: row.try_get("checkpoint_schema_version")?,
            checkpoint_payload: row.try_get("checkpoint_payload")?,
            checkpoint_payload_digest: fixed_bytes(row.try_get("checkpoint_payload_digest")?)?,
        })
    })
    .transpose()
}

fn validate_checkpoint(checkpoint: &StoredDangerCheckpoint) -> Result<(), PersistenceError> {
    if all_zero(&checkpoint.account_id)
        || all_zero(&checkpoint.character_id)
        || all_zero(&checkpoint.lineage_id)
        || checkpoint.checkpoint_tick < 0
        || checkpoint.character_version <= 0
        || checkpoint.progression_version <= 0
        || checkpoint.inventory_version <= 0
        || checkpoint.oath_bargain_version <= 0
        || checkpoint.checkpoint_schema_version != BELL_CHECKPOINT_SCHEMA_VERSION
        || checkpoint.checkpoint_payload.is_empty()
        || checkpoint.checkpoint_payload.len() > MAX_CHECKPOINT_PAYLOAD_BYTES
        || all_zero(&checkpoint.composite_digest)
        || all_zero(&checkpoint.checkpoint_payload_digest)
        || *blake3::hash(&checkpoint.checkpoint_payload).as_bytes()
            != checkpoint.checkpoint_payload_digest
        || !valid_revision(&checkpoint.content_revision)
    {
        return Err(PersistenceError::CorruptStoredDangerCheckpoint);
    }
    Ok(())
}

fn valid_revision(revision: &StoredWorldFlowRevisionV1) -> bool {
    [
        &revision.records_blake3,
        &revision.assets_blake3,
        &revision.localization_blake3,
    ]
    .iter()
    .all(|value| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn all_zero<const N: usize>(value: &[u8; N]) -> bool {
    value.iter().all(|byte| *byte == 0)
}

fn fixed_bytes<const N: usize>(bytes: Vec<u8>) -> Result<[u8; N], PersistenceError> {
    bytes
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredDangerCheckpoint)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checkpoint() -> StoredDangerCheckpoint {
        let payload = vec![1, 2, 3];
        StoredDangerCheckpoint {
            account_id: [1; ID_BYTES],
            character_id: [2; ID_BYTES],
            lineage_id: [3; ID_BYTES],
            checkpoint_tick: 900,
            content_revision: StoredWorldFlowRevisionV1 {
                records_blake3: "1".repeat(64),
                assets_blake3: "2".repeat(64),
                localization_blake3: "3".repeat(64),
            },
            composite_digest: [4; HASH_BYTES],
            character_version: 2,
            progression_version: 3,
            inventory_version: 4,
            oath_bargain_version: 5,
            checkpoint_schema_version: BELL_CHECKPOINT_SCHEMA_VERSION,
            checkpoint_payload_digest: *blake3::hash(&payload).as_bytes(),
            checkpoint_payload: payload,
        }
    }

    #[test]
    fn checkpoint_envelope_rejects_corruption_and_unbounded_payloads() {
        let valid = checkpoint();
        validate_checkpoint(&valid).unwrap();

        let mut corrupt = valid.clone();
        corrupt.checkpoint_payload.push(4);
        assert!(matches!(
            validate_checkpoint(&corrupt),
            Err(PersistenceError::CorruptStoredDangerCheckpoint)
        ));

        let mut oversized = valid;
        oversized.checkpoint_payload = vec![1; MAX_CHECKPOINT_PAYLOAD_BYTES + 1];
        oversized.checkpoint_payload_digest =
            *blake3::hash(&oversized.checkpoint_payload).as_bytes();
        assert!(matches!(
            validate_checkpoint(&oversized),
            Err(PersistenceError::CorruptStoredDangerCheckpoint)
        ));
    }
}
