use std::collections::BTreeSet;

use sqlx::Row;

use crate::{PersistenceError, PostgresPersistence, WIPEABLE_CORE_NAMESPACE};

pub const MAX_GROUND_EXPIRY_BATCH: u16 = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredGroundExpiryCandidate {
    pub account_id: [u8; 16],
    pub character_id: [u8; 16],
    pub item_uid: [u8; 16],
    pub pickup_id: [u8; 16],
    pub expires_at_tick: i64,
    pub item_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredGroundExpiry {
    pub ledger_event_id: [u8; 16],
    pub candidate: StoredGroundExpiryCandidate,
}

impl PostgresPersistence {
    pub async fn expire_personal_ground(
        &self,
        instance_id: [u8; 16],
        current_tick: i64,
        limit: u16,
        identify: impl FnOnce(&[StoredGroundExpiryCandidate]) -> Result<Vec<[u8; 16]>, PersistenceError>,
    ) -> Result<Vec<StoredGroundExpiry>, PersistenceError> {
        if instance_id == [0; 16]
            || current_tick <= 0
            || limit == 0
            || limit > MAX_GROUND_EXPIRY_BATCH
        {
            return Err(PersistenceError::CorruptStoredItems);
        }
        let mut transaction = self.begin_transaction().await?;
        let probes =
            load_due_probes(transaction.connection(), instance_id, current_tick, limit).await?;
        if probes.is_empty() {
            transaction.rollback().await?;
            return Ok(Vec::new());
        }
        lock_owners(transaction.connection(), &probes).await?;
        let candidates =
            lock_still_due(transaction.connection(), instance_id, current_tick, &probes).await?;
        let event_ids = identify(&candidates)?;
        validate_event_ids(&candidates, &event_ids)?;
        let mut expired = Vec::with_capacity(candidates.len());
        for (candidate, ledger_event_id) in candidates.into_iter().zip(event_ids) {
            expire_one(
                transaction.connection(),
                instance_id,
                current_tick,
                &candidate,
                ledger_event_id,
            )
            .await?;
            expired.push(StoredGroundExpiry {
                ledger_event_id,
                candidate,
            });
        }
        advance_affected_inventories(transaction.connection(), &expired).await?;
        transaction.commit().await?;
        Ok(expired)
    }
}

async fn load_due_probes(
    connection: &mut sqlx::PgConnection,
    instance_id: [u8; 16],
    current_tick: i64,
    limit: u16,
) -> Result<Vec<StoredGroundExpiryCandidate>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT account_id, character_id, item_uid, pickup_id, expires_at_tick, item_version \
         FROM item_instances WHERE namespace_id = $1 AND location_kind = 3 \
         AND instance_id = $2 AND expires_at_tick <= $3 \
         ORDER BY expires_at_tick, pickup_id, item_uid LIMIT $4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(instance_id.as_slice())
    .bind(current_tick)
    .bind(i64::from(limit))
    .fetch_all(connection)
    .await?;
    rows.iter().map(candidate_from_row).collect()
}

async fn lock_owners(
    connection: &mut sqlx::PgConnection,
    probes: &[StoredGroundExpiryCandidate],
) -> Result<(), PersistenceError> {
    let owners = probes
        .iter()
        .map(|candidate| (candidate.account_id, candidate.character_id))
        .collect::<BTreeSet<_>>();
    for (account_id, character_id) in owners {
        let locked: Option<i64> = sqlx::query_scalar(
            "SELECT inventory_version FROM character_inventories WHERE namespace_id = $1 \
             AND account_id = $2 AND character_id = $3 FOR UPDATE",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .fetch_optional(&mut *connection)
        .await?;
        if locked.is_none() {
            return Err(PersistenceError::CorruptStoredItems);
        }
    }
    Ok(())
}

async fn lock_still_due(
    connection: &mut sqlx::PgConnection,
    instance_id: [u8; 16],
    current_tick: i64,
    probes: &[StoredGroundExpiryCandidate],
) -> Result<Vec<StoredGroundExpiryCandidate>, PersistenceError> {
    let mut candidates = Vec::with_capacity(probes.len());
    for probe in probes {
        let row = sqlx::query(
            "SELECT account_id, character_id, item_uid, pickup_id, expires_at_tick, item_version \
             FROM item_instances WHERE namespace_id = $1 AND item_uid = $2 \
             AND location_kind = 3 AND instance_id = $3 AND expires_at_tick <= $4 FOR UPDATE",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(probe.item_uid.as_slice())
        .bind(instance_id.as_slice())
        .bind(current_tick)
        .fetch_optional(&mut *connection)
        .await?;
        if let Some(row) = row {
            let candidate = candidate_from_row(&row)?;
            if candidate.account_id != probe.account_id
                || candidate.character_id != probe.character_id
            {
                return Err(PersistenceError::CorruptStoredItems);
            }
            candidates.push(candidate);
        }
    }
    Ok(candidates)
}

async fn expire_one(
    connection: &mut sqlx::PgConnection,
    instance_id: [u8; 16],
    current_tick: i64,
    candidate: &StoredGroundExpiryCandidate,
    ledger_event_id: [u8; 16],
) -> Result<(), PersistenceError> {
    let post_version = candidate
        .item_version
        .checked_add(1)
        .ok_or(PersistenceError::CorruptStoredItems)?;
    let changed = sqlx::query(
        "UPDATE item_instances SET item_version = $1, security_state = 3, location_kind = 4, \
         instance_id = NULL, pickup_id = NULL, expires_at_tick = NULL, \
         destruction_reason = 'ground_expired', updated_at = transaction_timestamp() \
         WHERE namespace_id = $2 AND item_uid = $3 AND item_version = $4 \
         AND location_kind = 3 AND instance_id = $5 AND expires_at_tick <= $6",
    )
    .bind(post_version)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(candidate.item_uid.as_slice())
    .bind(candidate.item_version)
    .bind(instance_id.as_slice())
    .bind(current_tick)
    .execute(&mut *connection)
    .await?
    .rows_affected();
    if changed != 1 {
        return Err(PersistenceError::CorruptStoredItems);
    }
    sqlx::query(
        "INSERT INTO item_ledger_events \
         (namespace_id, ledger_event_id, item_uid, account_id, character_id, mutation_id, \
          event_kind, source_kind, pre_item_version, post_item_version, pre_security_state, \
          post_security_state, pre_location_kind, post_location_kind, reason) \
         VALUES ($1, $2, $3, $4, $5, $6, 2, 2, $7, $8, 2, 3, 3, 4, 'ground_expired')",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ledger_event_id.as_slice())
    .bind(candidate.item_uid.as_slice())
    .bind(candidate.account_id.as_slice())
    .bind(candidate.character_id.as_slice())
    .bind(candidate.pickup_id.as_slice())
    .bind(candidate.item_version)
    .bind(post_version)
    .execute(connection)
    .await?;
    Ok(())
}

async fn advance_affected_inventories(
    connection: &mut sqlx::PgConnection,
    expired: &[StoredGroundExpiry],
) -> Result<(), PersistenceError> {
    let owners = expired
        .iter()
        .map(|item| (item.candidate.account_id, item.candidate.character_id))
        .collect::<BTreeSet<_>>();
    for (account_id, character_id) in owners {
        let changed = sqlx::query(
            "UPDATE character_inventories SET inventory_version = inventory_version + 1, \
             updated_at = transaction_timestamp() WHERE namespace_id = $1 \
             AND account_id = $2 AND character_id = $3",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .execute(&mut *connection)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(PersistenceError::CorruptStoredItems);
        }
    }
    Ok(())
}

fn candidate_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<StoredGroundExpiryCandidate, PersistenceError> {
    Ok(StoredGroundExpiryCandidate {
        account_id: fixed_bytes(row.try_get("account_id")?)?,
        character_id: fixed_bytes(row.try_get("character_id")?)?,
        item_uid: fixed_bytes(row.try_get("item_uid")?)?,
        pickup_id: fixed_bytes(row.try_get("pickup_id")?)?,
        expires_at_tick: row.try_get("expires_at_tick")?,
        item_version: row.try_get("item_version")?,
    })
}

fn validate_event_ids(
    candidates: &[StoredGroundExpiryCandidate],
    event_ids: &[[u8; 16]],
) -> Result<(), PersistenceError> {
    if candidates.len() != event_ids.len()
        || event_ids.contains(&[0; 16])
        || event_ids.iter().collect::<BTreeSet<_>>().len() != event_ids.len()
    {
        return Err(PersistenceError::CorruptStoredItems);
    }
    Ok(())
}

fn fixed_bytes<const N: usize>(bytes: Vec<u8>) -> Result<[u8; N], PersistenceError> {
    bytes
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredItems)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_event_identity_set_is_exact_nonzero_and_unique() {
        let candidate = StoredGroundExpiryCandidate {
            account_id: [1; 16],
            character_id: [2; 16],
            item_uid: [3; 16],
            pickup_id: [4; 16],
            expires_at_tick: 1_800,
            item_version: 1,
        };
        assert!(validate_event_ids(std::slice::from_ref(&candidate), &[[5; 16]]).is_ok());
        assert!(validate_event_ids(std::slice::from_ref(&candidate), &[]).is_err());
        assert!(validate_event_ids(std::slice::from_ref(&candidate), &[[0; 16]]).is_err());
        assert!(validate_event_ids(&[candidate.clone(), candidate], &[[5; 16], [5; 16]]).is_err());
    }
}
