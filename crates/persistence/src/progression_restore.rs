//! TECH-023 progression capture and crash restoration inside a caller-owned transaction.

use sqlx::Row;

use crate::{
    PersistenceError, PersistenceTransaction, StoredProgression, StoredProgressionContract,
    WIPEABLE_CORE_NAMESPACE,
};

const ID_BYTES: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProgressionCrashRestore {
    pub replayed: bool,
    pub restored_progression: StoredProgression,
    pub revoked_award_count: u64,
    pub revoked_first_clear_count: u64,
}

pub async fn capture_progression_restore(
    transaction: &mut PersistenceTransaction<'_>,
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
    restore_point_id: [u8; ID_BYTES],
    contract: &StoredProgressionContract,
) -> Result<StoredProgression, PersistenceError> {
    validate_ids(&account_id, &character_id, &restore_point_id)?;
    let progression = lock_progression(
        transaction.connection(),
        &account_id,
        &character_id,
        contract,
    )
    .await?;
    sqlx::query(
        "INSERT INTO entry_restore_progression_v1 \
         (namespace_id, account_id, character_id, restore_point_id, level, total_xp, \
          current_health, progression_version) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(restore_point_id.as_slice())
    .bind(progression.level)
    .bind(progression.total_xp)
    .bind(progression.current_health)
    .bind(progression.progression_version)
    .execute(transaction.connection())
    .await
    .map_err(PersistenceError::Database)?;
    Ok(progression)
}

pub async fn restore_progression_after_crash(
    transaction: &mut PersistenceTransaction<'_>,
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
    restore_point_id: [u8; ID_BYTES],
    contract: &StoredProgressionContract,
) -> Result<StoredProgressionCrashRestore, PersistenceError> {
    validate_ids(&account_id, &character_id, &restore_point_id)?;
    lock_account(transaction.connection(), &account_id).await?;
    let character_version =
        lock_living_character(transaction.connection(), &account_id, &character_id).await?;
    let snapshot = lock_restore_snapshot(
        transaction.connection(),
        &account_id,
        &character_id,
        &restore_point_id,
        contract,
    )
    .await?;
    if let Some(restored_version) = snapshot.restored_progression_version {
        let mut restored_progression = snapshot.progression;
        restored_progression.progression_version = restored_version;
        return Ok(StoredProgressionCrashRestore {
            replayed: true,
            restored_progression,
            revoked_award_count: 0,
            revoked_first_clear_count: 0,
        });
    }
    if snapshot.restore_state != 0 {
        return Err(PersistenceError::ProgressionRestoreSuperseded);
    }
    lock_matching_danger_location(
        transaction.connection(),
        &account_id,
        &character_id,
        &restore_point_id,
        character_version,
    )
    .await?;
    let current = lock_progression(
        transaction.connection(),
        &account_id,
        &character_id,
        contract,
    )
    .await?;
    let restored_version = current
        .progression_version
        .checked_add(1)
        .ok_or(PersistenceError::CorruptStoredProgression)?;
    let restored_progression = StoredProgression {
        progression_version: restored_version,
        ..snapshot.progression
    };
    validate_progression(&restored_progression, contract)?;
    persist_restored_progression(
        transaction.connection(),
        &account_id,
        &character_id,
        &restored_progression,
    )
    .await?;
    let revoked_award_count = revoke_bound_awards(
        transaction.connection(),
        &account_id,
        &character_id,
        &restore_point_id,
        restored_version,
    )
    .await?;
    let revoked_first_clear_count = delete_exact_revoked_first_clears(
        transaction.connection(),
        &account_id,
        &character_id,
        &restore_point_id,
    )
    .await?;
    mark_snapshot_restored(
        transaction.connection(),
        &account_id,
        &character_id,
        &restore_point_id,
        restored_version,
    )
    .await?;
    Ok(StoredProgressionCrashRestore {
        replayed: false,
        restored_progression,
        revoked_award_count,
        revoked_first_clear_count,
    })
}

#[derive(Debug)]
struct LockedRestoreSnapshot {
    progression: StoredProgression,
    restore_state: i16,
    restored_progression_version: Option<i64>,
}

async fn lock_account(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
) -> Result<(), PersistenceError> {
    let exists = sqlx::query_scalar::<_, i32>(
        "SELECT 1 FROM accounts WHERE namespace_id = $1 AND account_id = $2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?;
    exists
        .is_some()
        .then_some(())
        .ok_or(PersistenceError::ProgressionCharacterNotFound)
}

async fn lock_living_character(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
) -> Result<i64, PersistenceError> {
    let row = sqlx::query(
        "SELECT life_state, character_state_version FROM characters WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?
    .ok_or(PersistenceError::ProgressionCharacterNotFound)?;
    let life_state: i16 = row
        .try_get("life_state")
        .map_err(PersistenceError::Database)?;
    let version: i64 = row
        .try_get("character_state_version")
        .map_err(PersistenceError::Database)?;
    if life_state != 0 {
        return Err(PersistenceError::ProgressionRestoreSuperseded);
    }
    if version < 1 {
        return Err(PersistenceError::CorruptStoredProgression);
    }
    Ok(version)
}

async fn lock_restore_snapshot(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    restore_point_id: &[u8; ID_BYTES],
    contract: &StoredProgressionContract,
) -> Result<LockedRestoreSnapshot, PersistenceError> {
    let row = sqlx::query(
        "SELECT s.level, s.total_xp, s.current_health, s.progression_version, \
                s.restored_progression_version, r.restore_state \
         FROM entry_restore_progression_v1 s JOIN character_entry_restore_points r \
           ON r.namespace_id = s.namespace_id AND r.account_id = s.account_id \
          AND r.character_id = s.character_id AND r.restore_point_id = s.restore_point_id \
         WHERE s.namespace_id = $1 AND s.account_id = $2 AND s.character_id = $3 \
           AND s.restore_point_id = $4 FOR UPDATE OF r, s",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(restore_point_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?
    .ok_or(PersistenceError::ProgressionRestorePointNotFound)?;
    let progression = StoredProgression {
        level: row.try_get("level").map_err(PersistenceError::Database)?,
        total_xp: row
            .try_get("total_xp")
            .map_err(PersistenceError::Database)?,
        current_health: row
            .try_get("current_health")
            .map_err(PersistenceError::Database)?,
        progression_version: row
            .try_get("progression_version")
            .map_err(PersistenceError::Database)?,
    };
    validate_progression(&progression, contract)?;
    let restored_progression_version: Option<i64> = row
        .try_get("restored_progression_version")
        .map_err(PersistenceError::Database)?;
    if restored_progression_version
        .is_some_and(|version| version <= progression.progression_version)
    {
        return Err(PersistenceError::CorruptStoredProgression);
    }
    Ok(LockedRestoreSnapshot {
        progression,
        restore_state: row
            .try_get("restore_state")
            .map_err(PersistenceError::Database)?,
        restored_progression_version,
    })
}

async fn lock_matching_danger_location(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    restore_point_id: &[u8; ID_BYTES],
    character_version: i64,
) -> Result<(), PersistenceError> {
    let row = sqlx::query(
        "SELECT character_version, location_kind, entry_restore_point_id \
         FROM character_world_locations WHERE namespace_id = $1 AND account_id = $2 \
         AND character_id = $3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?
    .ok_or(PersistenceError::ProgressionCharacterNotFound)?;
    let location_version: i64 = row
        .try_get("character_version")
        .map_err(PersistenceError::Database)?;
    let location_kind: i16 = row
        .try_get("location_kind")
        .map_err(PersistenceError::Database)?;
    let active_restore = row
        .try_get::<Option<Vec<u8>>, _>("entry_restore_point_id")
        .map_err(PersistenceError::Database)?
        .map(fixed_bytes)
        .transpose()?;
    if location_version != character_version
        || location_kind != 2
        || active_restore.as_ref() != Some(restore_point_id)
    {
        return Err(PersistenceError::ProgressionRestoreSuperseded);
    }
    Ok(())
}

async fn lock_progression(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    contract: &StoredProgressionContract,
) -> Result<StoredProgression, PersistenceError> {
    let row = sqlx::query(
        "SELECT total_xp, level, current_health, progression_version \
         FROM character_progression WHERE namespace_id = $1 AND account_id = $2 \
         AND character_id = $3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?
    .ok_or(PersistenceError::ProgressionCharacterNotFound)?;
    let progression = StoredProgression {
        total_xp: row
            .try_get("total_xp")
            .map_err(PersistenceError::Database)?,
        level: row.try_get("level").map_err(PersistenceError::Database)?,
        current_health: row
            .try_get("current_health")
            .map_err(PersistenceError::Database)?,
        progression_version: row
            .try_get("progression_version")
            .map_err(PersistenceError::Database)?,
    };
    validate_progression(&progression, contract)?;
    Ok(progression)
}

async fn persist_restored_progression(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    progression: &StoredProgression,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "UPDATE character_progression SET total_xp = $1, level = $2, current_health = $3, \
         progression_version = $4, updated_at = transaction_timestamp() \
         WHERE namespace_id = $5 AND account_id = $6 AND character_id = $7",
    )
    .bind(progression.total_xp)
    .bind(progression.level)
    .bind(progression.current_health)
    .bind(progression.progression_version)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(&mut *connection)
    .await
    .map_err(PersistenceError::Database)?;
    sqlx::query(
        "UPDATE characters SET level = $1, updated_at = transaction_timestamp() \
         WHERE namespace_id = $2 AND account_id = $3 AND character_id = $4",
    )
    .bind(progression.level)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(connection)
    .await
    .map_err(PersistenceError::Database)?;
    Ok(())
}

async fn revoke_bound_awards(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    restore_point_id: &[u8; ID_BYTES],
    restored_version: i64,
) -> Result<u64, PersistenceError> {
    let existing = sqlx::query(
        "SELECT reward_event_id, revoked_by_restore_point_id \
         FROM character_xp_award_results WHERE namespace_id = $1 AND account_id = $2 \
         AND character_id = $3 AND entry_restore_point_id = $4 \
         ORDER BY reward_event_id FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(restore_point_id.as_slice())
    .fetch_all(&mut *connection)
    .await
    .map_err(PersistenceError::Database)?;
    for row in &existing {
        let revoked: Option<Vec<u8>> = row
            .try_get("revoked_by_restore_point_id")
            .map_err(PersistenceError::Database)?;
        if revoked.is_some() {
            return Err(PersistenceError::CorruptStoredProgression);
        }
    }
    let result = sqlx::query(
        "UPDATE character_xp_award_results SET revoked_by_restore_point_id = $1, \
         revoked_at = transaction_timestamp(), revocation_progression_version = $2 \
         WHERE namespace_id = $3 AND account_id = $4 AND character_id = $5 \
         AND entry_restore_point_id = $1 AND revoked_by_restore_point_id IS NULL",
    )
    .bind(restore_point_id.as_slice())
    .bind(restored_version)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(connection)
    .await
    .map_err(PersistenceError::Database)?;
    if result.rows_affected() != existing.len() as u64 {
        return Err(PersistenceError::CorruptStoredProgression);
    }
    Ok(result.rows_affected())
}

async fn delete_exact_revoked_first_clears(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    restore_point_id: &[u8; ID_BYTES],
) -> Result<u64, PersistenceError> {
    let result = sqlx::query(
        "DELETE FROM account_boss_first_clears f USING character_xp_award_results a \
         WHERE f.namespace_id = $1 AND f.account_id = $2 \
           AND a.namespace_id = f.namespace_id AND a.account_id = f.account_id \
           AND a.reward_event_id = f.reward_event_id AND a.character_id = $3 \
           AND a.entry_restore_point_id = $4 AND a.revoked_by_restore_point_id = $4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(restore_point_id.as_slice())
    .execute(connection)
    .await
    .map_err(PersistenceError::Database)?;
    Ok(result.rows_affected())
}

async fn mark_snapshot_restored(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    restore_point_id: &[u8; ID_BYTES],
    restored_version: i64,
) -> Result<(), PersistenceError> {
    let result = sqlx::query(
        "UPDATE entry_restore_progression_v1 SET restored_progression_version = $1, \
         restored_at = transaction_timestamp() WHERE namespace_id = $2 AND account_id = $3 \
         AND character_id = $4 AND restore_point_id = $5 \
         AND restored_progression_version IS NULL",
    )
    .bind(restored_version)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(restore_point_id.as_slice())
    .execute(connection)
    .await
    .map_err(PersistenceError::Database)?;
    if result.rows_affected() != 1 {
        return Err(PersistenceError::CorruptStoredProgression);
    }
    Ok(())
}

fn validate_progression(
    progression: &StoredProgression,
    contract: &StoredProgressionContract,
) -> Result<(), PersistenceError> {
    let level = contract
        .cumulative_xp
        .iter()
        .rposition(|threshold| progression.total_xp >= *threshold)
        .and_then(|index| i16::try_from(index + 1).ok());
    if progression.total_xp < 0
        || progression.total_xp > contract.cumulative_xp[9]
        || progression.current_health < 1
        || progression.progression_version < 1
        || level != Some(progression.level)
    {
        return Err(PersistenceError::CorruptStoredProgression);
    }
    Ok(())
}

fn validate_ids(
    ids: &[u8; ID_BYTES],
    second: &[u8; ID_BYTES],
    third: &[u8; ID_BYTES],
) -> Result<(), PersistenceError> {
    if [ids, second, third]
        .into_iter()
        .any(|id| id.iter().all(|byte| *byte == 0))
    {
        return Err(PersistenceError::CorruptStoredProgression);
    }
    Ok(())
}

fn fixed_bytes<const N: usize>(bytes: Vec<u8>) -> Result<[u8; N], PersistenceError> {
    bytes
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredProgression)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restored_progression_keeps_exact_values_with_a_new_version() {
        let contract = StoredProgressionContract {
            cumulative_xp: [0, 100, 250, 450, 700, 1_000, 1_350, 1_750, 2_200, 2_700],
        };
        let restored = StoredProgression {
            total_xp: 700,
            level: 5,
            current_health: 91,
            progression_version: 14,
        };
        assert!(validate_progression(&restored, &contract).is_ok());
    }
}
