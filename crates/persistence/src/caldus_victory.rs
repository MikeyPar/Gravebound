use sqlx::Row;

use crate::{
    PersistenceError, PostgresPersistence, StoredActiveDangerAuthorityV1, WIPEABLE_CORE_NAMESPACE,
    active_danger_authority::{
        lock_active_danger_account, validate_active_danger_after_account_lock,
    },
};

const ID_BYTES: usize = 16;
const HASH_BYTES: usize = 32;
const MAX_OWNERS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCaldusVictoryOwner {
    pub party_slot: u8,
    pub participant_entity_id: u64,
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub reward_request_id: [u8; ID_BYTES],
    pub reward_result_hash: [u8; HASH_BYTES],
    pub progression_payload_hash: [u8; HASH_BYTES],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaldusVictoryExitCommit {
    pub encounter_id: [u8; ID_BYTES],
    pub instance_lineage_id: [u8; ID_BYTES],
    pub attempt_ordinal: u32,
    pub exit_instance_id: [u8; ID_BYTES],
    pub owners: Vec<StoredCaldusVictoryOwner>,
    pub danger_authorities: Vec<StoredActiveDangerAuthorityV1>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCaldusVictoryExit {
    pub replayed: bool,
    pub encounter_id: [u8; ID_BYTES],
    pub instance_lineage_id: [u8; ID_BYTES],
    pub attempt_ordinal: u32,
    pub exit_instance_id: [u8; ID_BYTES],
    pub canonical_request_hash: [u8; HASH_BYTES],
    pub owners: Vec<StoredCaldusVictoryOwner>,
}

impl PostgresPersistence {
    pub async fn commit_caldus_victory_exit(
        &self,
        commit: &CaldusVictoryExitCommit,
    ) -> Result<StoredCaldusVictoryExit, PersistenceError> {
        validate_commit(commit)?;
        let canonical_request_hash = canonical_hash(commit)?;
        let mut transaction = self.begin_transaction().await?;
        let mut danger_authorities = commit.danger_authorities.clone();
        danger_authorities.sort_unstable_by_key(|authority| authority.account_id);
        for authority in &danger_authorities {
            lock_active_danger_account(transaction.connection(), *authority).await?;
        }
        let lock_key = i64::from_le_bytes(
            commit.encounter_id[..8]
                .try_into()
                .map_err(|_| PersistenceError::CorruptCaldusVictory)?,
        );
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(lock_key)
            .execute(transaction.connection())
            .await?;
        if let Some(existing) = load_existing(
            transaction.connection(),
            commit.encounter_id,
            canonical_request_hash,
        )
        .await?
        {
            transaction.rollback().await?;
            return Ok(existing);
        }
        for authority in &commit.danger_authorities {
            validate_active_danger_after_account_lock(transaction.connection(), *authority).await?;
        }
        for owner in &commit.owners {
            verify_terminal_owner(transaction.connection(), owner).await?;
        }
        sqlx::query(
            "INSERT INTO caldus_victory_exits
             (namespace_id, encounter_id, instance_lineage_id, attempt_ordinal,
              exit_instance_id, canonical_request_hash, eligible_owner_count)
             VALUES ($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(commit.encounter_id.as_slice())
        .bind(commit.instance_lineage_id.as_slice())
        .bind(
            i32::try_from(commit.attempt_ordinal)
                .map_err(|_| PersistenceError::CorruptCaldusVictory)?,
        )
        .bind(commit.exit_instance_id.as_slice())
        .bind(canonical_request_hash.as_slice())
        .bind(
            i16::try_from(commit.owners.len())
                .map_err(|_| PersistenceError::CorruptCaldusVictory)?,
        )
        .execute(transaction.connection())
        .await?;
        for owner in &commit.owners {
            sqlx::query(
                "INSERT INTO caldus_victory_exit_owners
                 (namespace_id, encounter_id, party_slot, participant_entity_id, account_id,
                  character_id, reward_request_id, reward_result_hash, progression_payload_hash)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
            )
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(commit.encounter_id.as_slice())
            .bind(i16::from(owner.party_slot))
            .bind(owner.participant_entity_id.to_le_bytes().as_slice())
            .bind(owner.account_id.as_slice())
            .bind(owner.character_id.as_slice())
            .bind(owner.reward_request_id.as_slice())
            .bind(owner.reward_result_hash.as_slice())
            .bind(owner.progression_payload_hash.as_slice())
            .execute(transaction.connection())
            .await?;
        }
        transaction.commit().await?;
        Ok(StoredCaldusVictoryExit {
            replayed: false,
            encounter_id: commit.encounter_id,
            instance_lineage_id: commit.instance_lineage_id,
            attempt_ordinal: commit.attempt_ordinal,
            exit_instance_id: commit.exit_instance_id,
            canonical_request_hash,
            owners: commit.owners.clone(),
        })
    }
}

async fn load_existing(
    connection: &mut sqlx::PgConnection,
    encounter_id: [u8; ID_BYTES],
    expected_hash: [u8; HASH_BYTES],
) -> Result<Option<StoredCaldusVictoryExit>, PersistenceError> {
    let row = sqlx::query(
        "SELECT instance_lineage_id, attempt_ordinal, exit_instance_id,
                canonical_request_hash, eligible_owner_count
         FROM caldus_victory_exits
         WHERE namespace_id=$1 AND encounter_id=$2 FOR SHARE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(encounter_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let canonical_request_hash = fixed_bytes(row.try_get("canonical_request_hash")?)?;
    if canonical_request_hash != expected_hash {
        return Err(PersistenceError::CaldusVictoryIdempotencyConflict);
    }
    let owner_rows = sqlx::query(
        "SELECT party_slot, participant_entity_id, account_id, character_id,
                reward_request_id, reward_result_hash, progression_payload_hash
         FROM caldus_victory_exit_owners
         WHERE namespace_id=$1 AND encounter_id=$2 ORDER BY party_slot",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(encounter_id.as_slice())
    .fetch_all(&mut *connection)
    .await?;
    let expected_count: i16 = row.try_get("eligible_owner_count")?;
    if owner_rows.len()
        != usize::try_from(expected_count).map_err(|_| PersistenceError::CorruptCaldusVictory)?
    {
        return Err(PersistenceError::CorruptCaldusVictory);
    }
    let owners = owner_rows
        .into_iter()
        .map(|owner| {
            let entity = fixed_bytes::<8>(owner.try_get("participant_entity_id")?)?;
            Ok(StoredCaldusVictoryOwner {
                party_slot: u8::try_from(owner.try_get::<i16, _>("party_slot")?)
                    .map_err(|_| PersistenceError::CorruptCaldusVictory)?,
                participant_entity_id: u64::from_le_bytes(entity),
                account_id: fixed_bytes(owner.try_get("account_id")?)?,
                character_id: fixed_bytes(owner.try_get("character_id")?)?,
                reward_request_id: fixed_bytes(owner.try_get("reward_request_id")?)?,
                reward_result_hash: fixed_bytes(owner.try_get("reward_result_hash")?)?,
                progression_payload_hash: fixed_bytes(owner.try_get("progression_payload_hash")?)?,
            })
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    let attempt = row.try_get::<i32, _>("attempt_ordinal")?;
    Ok(Some(StoredCaldusVictoryExit {
        replayed: true,
        encounter_id,
        instance_lineage_id: fixed_bytes(row.try_get("instance_lineage_id")?)?,
        attempt_ordinal: u32::try_from(attempt)
            .map_err(|_| PersistenceError::CorruptCaldusVictory)?,
        exit_instance_id: fixed_bytes(row.try_get("exit_instance_id")?)?,
        canonical_request_hash,
        owners,
    }))
}

async fn verify_terminal_owner(
    connection: &mut sqlx::PgConnection,
    owner: &StoredCaldusVictoryOwner,
) -> Result<(), PersistenceError> {
    let row = sqlx::query(
        "SELECT r.account_id AS reward_account_id, r.character_id AS reward_character_id,
                r.reward_table_id, r.result_hash,
                x.character_id AS xp_character_id, x.payload_hash, x.source_content_id,
                x.eligible, x.base_xp, x.result_code, x.revoked_by_restore_point_id
         FROM reward_requests r
         JOIN character_xp_award_results x
           ON x.namespace_id=r.namespace_id AND x.account_id=r.account_id
          AND x.reward_event_id=r.reward_request_id
         WHERE r.namespace_id=$1 AND r.reward_request_id=$2
         FOR SHARE OF r,x",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(owner.reward_request_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Err(PersistenceError::CaldusRewardNotTerminal);
    };
    let revoked: Option<Vec<u8>> = row.try_get("revoked_by_restore_point_id")?;
    if fixed_bytes::<ID_BYTES>(row.try_get("reward_account_id")?)? != owner.account_id
        || fixed_bytes::<ID_BYTES>(row.try_get("reward_character_id")?)? != owner.character_id
        || fixed_bytes::<ID_BYTES>(row.try_get("xp_character_id")?)? != owner.character_id
        || row.try_get::<String, _>("reward_table_id")? != "reward.boss_caldus"
        || fixed_bytes::<HASH_BYTES>(row.try_get("result_hash")?)? != owner.reward_result_hash
        || fixed_bytes::<HASH_BYTES>(row.try_get("payload_hash")?)?
            != owner.progression_payload_hash
        || row.try_get::<String, _>("source_content_id")? != "boss.sir_caldus"
        || !row.try_get::<bool, _>("eligible")?
        || row.try_get::<i32, _>("base_xp")? != 450
        || row.try_get::<i16, _>("result_code")? != 0
        || revoked.is_some()
    {
        return Err(PersistenceError::CaldusRewardTerminalMismatch);
    }
    Ok(())
}

fn validate_commit(commit: &CaldusVictoryExitCommit) -> Result<(), PersistenceError> {
    if all_zero(&commit.encounter_id)
        || all_zero(&commit.instance_lineage_id)
        || all_zero(&commit.exit_instance_id)
        || commit.attempt_ordinal == 0
        || commit.owners.is_empty()
        || commit.owners.len() > MAX_OWNERS
        || commit.danger_authorities.len() != commit.owners.len()
    {
        return Err(PersistenceError::CorruptCaldusVictory);
    }
    let mut previous = None;
    for (owner, authority) in commit.owners.iter().zip(&commit.danger_authorities) {
        if owner.party_slot >= 8
            || owner.participant_entity_id == 0
            || all_zero(&owner.account_id)
            || all_zero(&owner.character_id)
            || all_zero(&owner.reward_request_id)
            || all_zero(&owner.reward_result_hash)
            || all_zero(&owner.progression_payload_hash)
            || authority.account_id != owner.account_id
            || authority.character_id != owner.character_id
            || authority.instance_lineage_id != commit.instance_lineage_id
            || authority.validate().is_err()
            || previous.is_some_and(|slot| owner.party_slot <= slot)
        {
            return Err(PersistenceError::CorruptCaldusVictory);
        }
        previous = Some(owner.party_slot);
    }
    Ok(())
}

fn canonical_hash(commit: &CaldusVictoryExitCommit) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let mut hasher = blake3::Hasher::new();
    update_field(&mut hasher, b"gravebound.caldus.victory-exit-commit.v1")?;
    update_field(&mut hasher, &commit.encounter_id)?;
    update_field(&mut hasher, &commit.instance_lineage_id)?;
    update_field(&mut hasher, &commit.attempt_ordinal.to_le_bytes())?;
    update_field(&mut hasher, &commit.exit_instance_id)?;
    for owner in &commit.owners {
        update_field(&mut hasher, &[owner.party_slot])?;
        update_field(&mut hasher, &owner.participant_entity_id.to_le_bytes())?;
        update_field(&mut hasher, &owner.account_id)?;
        update_field(&mut hasher, &owner.character_id)?;
        update_field(&mut hasher, &owner.reward_request_id)?;
        update_field(&mut hasher, &owner.reward_result_hash)?;
        update_field(&mut hasher, &owner.progression_payload_hash)?;
    }
    for authority in &commit.danger_authorities {
        update_field(&mut hasher, &authority.entry_restore_point_id)?;
    }
    Ok(*hasher.finalize().as_bytes())
}

fn update_field(hasher: &mut blake3::Hasher, value: &[u8]) -> Result<(), PersistenceError> {
    let length = u32::try_from(value.len()).map_err(|_| PersistenceError::CorruptCaldusVictory)?;
    hasher.update(&length.to_le_bytes());
    hasher.update(value);
    Ok(())
}

fn fixed_bytes<const N: usize>(value: Vec<u8>) -> Result<[u8; N], PersistenceError> {
    value
        .try_into()
        .map_err(|_| PersistenceError::CorruptCaldusVictory)
}

fn all_zero(value: &[u8]) -> bool {
    value.iter().all(|byte| *byte == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(slot: u8) -> StoredCaldusVictoryOwner {
        StoredCaldusVictoryOwner {
            party_slot: slot,
            participant_entity_id: u64::from(slot) + 1,
            account_id: [slot + 1; 16],
            character_id: [slot + 11; 16],
            reward_request_id: [slot + 21; 16],
            reward_result_hash: [slot + 31; 32],
            progression_payload_hash: [slot + 41; 32],
        }
    }

    fn commit() -> CaldusVictoryExitCommit {
        CaldusVictoryExitCommit {
            encounter_id: [1; 16],
            instance_lineage_id: [2; 16],
            attempt_ordinal: 1,
            exit_instance_id: [3; 16],
            owners: vec![owner(0), owner(1)],
            danger_authorities: vec![
                StoredActiveDangerAuthorityV1 {
                    account_id: [1; 16],
                    character_id: [11; 16],
                    instance_lineage_id: [2; 16],
                    entry_restore_point_id: [41; 16],
                },
                StoredActiveDangerAuthorityV1 {
                    account_id: [2; 16],
                    character_id: [12; 16],
                    instance_lineage_id: [2; 16],
                    entry_restore_point_id: [42; 16],
                },
            ],
        }
    }

    #[test]
    fn canonical_material_is_stable_and_owner_sensitive() {
        let original = commit();
        assert_eq!(
            canonical_hash(&original).unwrap(),
            canonical_hash(&original).unwrap()
        );
        let mut changed = original.clone();
        changed.owners[1].reward_result_hash[0] ^= 1;
        assert_ne!(
            canonical_hash(&original).unwrap(),
            canonical_hash(&changed).unwrap()
        );
        let mut changed = original.clone();
        changed.danger_authorities[1].entry_restore_point_id[0] ^= 1;
        assert_ne!(
            canonical_hash(&original).unwrap(),
            canonical_hash(&changed).unwrap()
        );
    }

    #[test]
    fn invalid_empty_duplicate_or_zero_owner_material_fails_closed() {
        let mut invalid = commit();
        invalid.owners.clear();
        assert!(matches!(
            validate_commit(&invalid),
            Err(PersistenceError::CorruptCaldusVictory)
        ));
        let mut invalid = commit();
        invalid.danger_authorities[0].character_id[0] ^= 1;
        assert!(matches!(
            validate_commit(&invalid),
            Err(PersistenceError::CorruptCaldusVictory)
        ));
        let mut invalid = commit();
        invalid.owners[1].party_slot = 0;
        assert!(matches!(
            validate_commit(&invalid),
            Err(PersistenceError::CorruptCaldusVictory)
        ));
        let mut invalid = commit();
        invalid.owners[0].reward_request_id = [0; 16];
        assert!(matches!(
            validate_commit(&invalid),
            Err(PersistenceError::CorruptCaldusVictory)
        ));
    }
}
