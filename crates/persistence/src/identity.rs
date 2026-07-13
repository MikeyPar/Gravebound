use sqlx::Row;

use crate::{
    PersistenceError, PersistenceTransaction, PostgresPersistence, WIPEABLE_CORE_NAMESPACE,
};

const ID_BYTES: usize = 16;
const HASH_BYTES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCharacter {
    pub character_id: [u8; ID_BYTES],
    pub roster_ordinal: i16,
    pub class_id: String,
    pub level: i32,
    pub oath_id: Option<String>,
    pub life_state: i16,
    pub security_state: i16,
    pub character_state_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMutation {
    pub mutation_id: [u8; ID_BYTES],
    pub payload_hash: [u8; HASH_BYTES],
    pub result_payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredIdentityAggregate {
    pub state_version: i64,
    pub slot_capacity: i16,
    pub selected_character_id: Option<[u8; ID_BYTES]>,
    pub characters: Vec<StoredCharacter>,
    pub mutations: Vec<StoredMutation>,
}

impl PostgresPersistence {
    pub async fn transact_identity<T, F>(
        &self,
        account_id: [u8; ID_BYTES],
        initial_state_version: i64,
        slot_capacity: i16,
        mut operation: F,
    ) -> Result<T, PersistenceError>
    where
        T: Send,
        F: FnMut(&mut StoredIdentityAggregate) -> Result<T, PersistenceError> + Send,
    {
        const MAX_SERIALIZATION_ATTEMPTS: u8 = 3;

        for attempt in 1..=MAX_SERIALIZATION_ATTEMPTS {
            match self
                .transact_identity_once(
                    account_id,
                    initial_state_version,
                    slot_capacity,
                    &mut operation,
                )
                .await
            {
                Err(error)
                    if attempt < MAX_SERIALIZATION_ATTEMPTS
                        && crate::is_serialization_failure(&error) => {}
                result => return result,
            }
        }
        unreachable!("bounded identity transaction loop always returns")
    }

    async fn transact_identity_once<T, F>(
        &self,
        account_id: [u8; ID_BYTES],
        initial_state_version: i64,
        slot_capacity: i16,
        operation: &mut F,
    ) -> Result<T, PersistenceError>
    where
        T: Send,
        F: FnMut(&mut StoredIdentityAggregate) -> Result<T, PersistenceError> + Send,
    {
        let mut transaction = self.begin_transaction().await?;
        ensure_account(
            &mut transaction,
            &account_id,
            initial_state_version,
            slot_capacity,
        )
        .await?;
        let mut aggregate = load_aggregate(&mut transaction, &account_id).await?;
        let result = operation(&mut aggregate)?;
        persist_aggregate(&mut transaction, &account_id, &aggregate).await?;
        transaction.commit().await?;
        Ok(result)
    }

    pub async fn identity_character_owner(
        &self,
        character_id: [u8; ID_BYTES],
    ) -> Result<Option<[u8; ID_BYTES]>, PersistenceError> {
        let mut transaction = self.begin_transaction().await?;
        let owner: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT account_id FROM characters \
             WHERE namespace_id = $1 AND character_id = $2",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(character_id.as_slice())
        .fetch_optional(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?;
        transaction.rollback().await?;
        owner.map(fixed_bytes).transpose()
    }
}

async fn ensure_account(
    transaction: &mut PersistenceTransaction<'_>,
    account_id: &[u8; ID_BYTES],
    initial_state_version: i64,
    slot_capacity: i16,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO accounts \
         (namespace_id, account_id, state_version, slot_capacity) VALUES ($1, $2, $3, $4) \
         ON CONFLICT (namespace_id, account_id) DO NOTHING",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(initial_state_version)
    .bind(slot_capacity)
    .execute(transaction.connection())
    .await
    .map_err(PersistenceError::Database)?;
    Ok(())
}

async fn load_aggregate(
    transaction: &mut PersistenceTransaction<'_>,
    account_id: &[u8; ID_BYTES],
) -> Result<StoredIdentityAggregate, PersistenceError> {
    let account = sqlx::query(
        "SELECT state_version, slot_capacity, selected_character_id FROM accounts \
         WHERE namespace_id = $1 AND account_id = $2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .map_err(PersistenceError::Database)?;
    let selected = account
        .try_get::<Option<Vec<u8>>, _>("selected_character_id")
        .map_err(PersistenceError::Database)?
        .map(fixed_bytes)
        .transpose()?;

    Ok(StoredIdentityAggregate {
        state_version: account
            .try_get("state_version")
            .map_err(PersistenceError::Database)?,
        slot_capacity: account
            .try_get("slot_capacity")
            .map_err(PersistenceError::Database)?,
        selected_character_id: selected,
        characters: load_characters(transaction, account_id).await?,
        mutations: load_mutations(transaction, account_id).await?,
    })
}

async fn load_characters(
    transaction: &mut PersistenceTransaction<'_>,
    account_id: &[u8; ID_BYTES],
) -> Result<Vec<StoredCharacter>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT characters.character_id, roster_ordinal, class_id, \
                characters.level AS identity_level, character_progression.level, oath_id, \
                life_state, security_state, character_state_version \
         FROM characters JOIN character_progression USING (namespace_id, account_id, character_id) \
         WHERE namespace_id = $1 AND account_id = $2 ORDER BY roster_ordinal",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_all(transaction.connection())
    .await
    .map_err(PersistenceError::Database)?;
    rows.into_iter()
        .map(|row| {
            let progression_level: i16 =
                row.try_get("level").map_err(PersistenceError::Database)?;
            let identity_level: i32 = row
                .try_get("identity_level")
                .map_err(PersistenceError::Database)?;
            if identity_level != i32::from(progression_level) {
                return Err(PersistenceError::CorruptStoredIdentity);
            }
            Ok(StoredCharacter {
                character_id: fixed_bytes(
                    row.try_get("character_id")
                        .map_err(PersistenceError::Database)?,
                )?,
                roster_ordinal: row
                    .try_get("roster_ordinal")
                    .map_err(PersistenceError::Database)?,
                class_id: row
                    .try_get("class_id")
                    .map_err(PersistenceError::Database)?,
                level: identity_level,
                oath_id: row.try_get("oath_id").map_err(PersistenceError::Database)?,
                life_state: row
                    .try_get("life_state")
                    .map_err(PersistenceError::Database)?,
                security_state: row
                    .try_get("security_state")
                    .map_err(PersistenceError::Database)?,
                character_state_version: row
                    .try_get("character_state_version")
                    .map_err(PersistenceError::Database)?,
            })
        })
        .collect()
}

async fn load_mutations(
    transaction: &mut PersistenceTransaction<'_>,
    account_id: &[u8; ID_BYTES],
) -> Result<Vec<StoredMutation>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT mutation_id, payload_hash, result_payload FROM account_mutation_results \
         WHERE namespace_id = $1 AND account_id = $2 ORDER BY created_at, mutation_id",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_all(transaction.connection())
    .await
    .map_err(PersistenceError::Database)?;
    rows.into_iter()
        .map(|row| {
            Ok(StoredMutation {
                mutation_id: fixed_bytes(
                    row.try_get("mutation_id")
                        .map_err(PersistenceError::Database)?,
                )?,
                payload_hash: fixed_bytes(
                    row.try_get("payload_hash")
                        .map_err(PersistenceError::Database)?,
                )?,
                result_payload: row
                    .try_get("result_payload")
                    .map_err(PersistenceError::Database)?,
            })
        })
        .collect()
}

async fn persist_aggregate(
    transaction: &mut PersistenceTransaction<'_>,
    account_id: &[u8; ID_BYTES],
    aggregate: &StoredIdentityAggregate,
) -> Result<(), PersistenceError> {
    for character in &aggregate.characters {
        sqlx::query(
            "INSERT INTO characters \
             (namespace_id, account_id, character_id, roster_ordinal, class_id, level, \
              oath_id, life_state, security_state, character_state_version) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
             ON CONFLICT (namespace_id, character_id) DO NOTHING",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character.character_id.as_slice())
        .bind(character.roster_ordinal)
        .bind(&character.class_id)
        .bind(character.level)
        .bind(&character.oath_id)
        .bind(character.life_state)
        .bind(character.security_state)
        .bind(character.character_state_version)
        .execute(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?;
        sqlx::query(
            "INSERT INTO character_progression \
             (namespace_id, account_id, character_id, total_xp, level, current_health, \
              progression_version) VALUES ($1, $2, $3, 0, 1, 120, 1) \
             ON CONFLICT (namespace_id, account_id, character_id) DO NOTHING",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character.character_id.as_slice())
        .execute(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?;
        sqlx::query(
            "INSERT INTO character_world_locations \
             (namespace_id, account_id, character_id, character_version, location_kind, \
              safe_arrival_kind) \
             VALUES ($1, $2, $3, $4, 0, 0) \
             ON CONFLICT (namespace_id, account_id, character_id) DO NOTHING",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character.character_id.as_slice())
        .bind(character.character_state_version)
        .execute(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?;
        sqlx::query(
            "INSERT INTO character_oath_bargain_state \
             (namespace_id, account_id, character_id, earned_bargain_slots, \
              oath_bargain_version) VALUES ($1, $2, $3, 0, 1) \
             ON CONFLICT (namespace_id, account_id, character_id) DO NOTHING",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character.character_id.as_slice())
        .execute(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?;
    }
    for mutation in &aggregate.mutations {
        sqlx::query(
            "INSERT INTO account_mutation_results \
             (namespace_id, account_id, mutation_id, payload_hash, result_payload) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (namespace_id, account_id, mutation_id) DO NOTHING",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(mutation.mutation_id.as_slice())
        .bind(mutation.payload_hash.as_slice())
        .bind(&mutation.result_payload)
        .execute(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?;
    }
    sqlx::query(
        "UPDATE accounts SET state_version = $1, slot_capacity = $2, \
         selected_character_id = $3, updated_at = transaction_timestamp() \
         WHERE namespace_id = $4 AND account_id = $5",
    )
    .bind(aggregate.state_version)
    .bind(aggregate.slot_capacity)
    .bind(
        aggregate
            .selected_character_id
            .as_ref()
            .map(<[u8; ID_BYTES]>::as_slice),
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .execute(transaction.connection())
    .await
    .map_err(PersistenceError::Database)?;
    Ok(())
}

fn fixed_bytes<const N: usize>(bytes: Vec<u8>) -> Result<[u8; N], PersistenceError> {
    bytes
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredIdentity)
}
