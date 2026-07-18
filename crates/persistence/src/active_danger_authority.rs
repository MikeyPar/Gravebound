//! Shared account-first validation for fresh danger-bound writes and terminal winners.
//!
//! Every terminal writer locks the account row first. Caldus item, progression, and exit writers
//! join that same order, check exact durable replay, and only then revalidate the active root.

use sqlx::{PgConnection, Row};

use crate::{PersistenceError, WIPEABLE_CORE_NAMESPACE};

const ID_BYTES: usize = 16;
const WORLD_LOCATION_DANGER: i16 = 2;
const LIFE_STATE_LIVING: i16 = 0;
const SECURITY_STATE_NORMAL: i16 = 0;
const RESTORE_STATE_ACTIVE: i16 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredActiveDangerAuthorityV1 {
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub instance_lineage_id: [u8; ID_BYTES],
    pub entry_restore_point_id: [u8; ID_BYTES],
}

impl StoredActiveDangerAuthorityV1 {
    pub fn validate(self) -> Result<(), PersistenceError> {
        if [
            self.account_id,
            self.character_id,
            self.instance_lineage_id,
            self.entry_restore_point_id,
        ]
        .contains(&[0; ID_BYTES])
        {
            return Err(PersistenceError::InvalidActiveDangerAuthority);
        }
        Ok(())
    }
}

pub(crate) async fn lock_active_danger_account(
    connection: &mut PgConnection,
    authority: StoredActiveDangerAuthorityV1,
) -> Result<(), PersistenceError> {
    authority.validate()?;
    let found: Option<i32> = sqlx::query_scalar(
        "SELECT 1 FROM accounts WHERE namespace_id=$1 AND account_id=$2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(authority.account_id.as_slice())
    .fetch_optional(connection)
    .await?;
    if found.is_none() {
        return Err(PersistenceError::ActiveDangerAuthorityBindingMismatch);
    }
    Ok(())
}

pub(crate) async fn validate_active_danger_after_account_lock(
    connection: &mut PgConnection,
    authority: StoredActiveDangerAuthorityV1,
) -> Result<(), PersistenceError> {
    authority.validate()?;
    let account = sqlx::query(
        "SELECT selected_character_id FROM accounts
         WHERE namespace_id=$1 AND account_id=$2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(authority.account_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ActiveDangerAuthorityBindingMismatch)?;
    let character = sqlx::query(
        "SELECT life_state,security_state FROM characters
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(authority.account_id.as_slice())
    .bind(authority.character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ActiveDangerAuthorityBindingMismatch)?;
    let root = sqlx::query(
        "SELECT restore_state,lineage_id FROM character_entry_restore_points
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
           AND restore_point_id=$4 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(authority.account_id.as_slice())
    .bind(authority.character_id.as_slice())
    .bind(authority.entry_restore_point_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ActiveDangerAuthorityBindingMismatch)?;
    let world_lineage = sqlx::query(
        "SELECT world.location_kind,world.instance_lineage_id,world.entry_restore_point_id,
                lineage.lineage_state
         FROM character_world_locations AS world
         JOIN character_instance_lineages AS lineage
           ON lineage.namespace_id=world.namespace_id AND lineage.account_id=world.account_id
          AND lineage.character_id=world.character_id
          AND lineage.lineage_id=world.instance_lineage_id
         WHERE world.namespace_id=$1 AND world.account_id=$2 AND world.character_id=$3
         FOR UPDATE OF world,lineage",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(authority.account_id.as_slice())
    .bind(authority.character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ActiveDangerAuthorityBindingMismatch)?;

    if optional_id(account.try_get("selected_character_id")?)? != Some(authority.character_id)
        || character.try_get::<i16, _>("life_state")? != LIFE_STATE_LIVING
        || character.try_get::<i16, _>("security_state")? != SECURITY_STATE_NORMAL
        || exact_id(root.try_get("lineage_id")?)? != authority.instance_lineage_id
    {
        return Err(PersistenceError::ActiveDangerAuthorityBindingMismatch);
    }
    if root.try_get::<i16, _>("restore_state")? != RESTORE_STATE_ACTIVE
        || !matches!(world_lineage.try_get::<i16, _>("lineage_state")?, 0 | 1)
    {
        return Err(PersistenceError::ActiveDangerAuthoritySuperseded);
    }
    if world_lineage.try_get::<i16, _>("location_kind")? != WORLD_LOCATION_DANGER
        || optional_id(world_lineage.try_get("instance_lineage_id")?)?
            != Some(authority.instance_lineage_id)
        || optional_id(world_lineage.try_get("entry_restore_point_id")?)?
            != Some(authority.entry_restore_point_id)
    {
        return Err(PersistenceError::ActiveDangerAuthorityBindingMismatch);
    }
    Ok(())
}

fn exact_id(value: Vec<u8>) -> Result<[u8; ID_BYTES], PersistenceError> {
    value
        .try_into()
        .map_err(|_| PersistenceError::ActiveDangerAuthorityBindingMismatch)
}

fn optional_id(value: Option<Vec<u8>>) -> Result<Option<[u8; ID_BYTES]>, PersistenceError> {
    value.map(exact_id).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authority() -> StoredActiveDangerAuthorityV1 {
        StoredActiveDangerAuthorityV1 {
            account_id: [1; 16],
            character_id: [2; 16],
            instance_lineage_id: [3; 16],
            entry_restore_point_id: [4; 16],
        }
    }

    #[test]
    fn authority_rejects_every_zero_identity() {
        for axis in 0..4 {
            let mut invalid = authority();
            match axis {
                0 => invalid.account_id = [0; 16],
                1 => invalid.character_id = [0; 16],
                2 => invalid.instance_lineage_id = [0; 16],
                _ => invalid.entry_restore_point_id = [0; 16],
            }
            assert!(matches!(
                invalid.validate(),
                Err(PersistenceError::InvalidActiveDangerAuthority)
            ));
        }
    }
}
