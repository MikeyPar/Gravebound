//! Typed `PostgreSQL` transaction boundary for initial Oath selection.
//!
//! This repository owns lock order, durable replay, the character-life version update, and the
//! transactional outbox. Gameplay eligibility remains server-owned; this boundary also locks the
//! durable inventory aggregate and exposes a conservative safety projection to that authority.

use sqlx::Row;

use crate::{PersistenceError, PostgresPersistence, WIPEABLE_CORE_NAMESPACE};

const ID_BYTES: usize = 16;
const HASH_BYTES: usize = 32;
const MAX_RESULT_PAYLOAD_BYTES: usize = 65_536;
const LONG_VIGIL_ID: &str = "oath.arbalist.long_vigil";
const NAILKEEPER_ID: &str = "oath.arbalist.nailkeeper";
const ACCEPTED_RESULT_CODE: i16 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredOathCharacter {
    pub selected_character_id: Option<[u8; ID_BYTES]>,
    pub level: i16,
    pub life_state: i16,
    pub security_state: i16,
    pub character_state_version: i64,
    pub oath_bargain_version: i64,
    pub oath_id: Option<String>,
    pub location_character_version: i64,
    pub location_kind: i16,
    pub location_content_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredOathMutationResult {
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub mutation_id: [u8; ID_BYTES],
    pub payload_hash: [u8; HASH_BYTES],
    pub oath_id: String,
    pub pre_character_state_version: i64,
    pub post_character_state_version: i64,
    pub result_code: i16,
    pub result_payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCharacterLifeEvent {
    pub event_id: [u8; ID_BYTES],
    pub aggregate_version: i64,
    pub event_payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredOathInventory {
    /// Missing until the starter-item initializer creates the aggregate.
    pub inventory_version: Option<i64>,
    /// True only when every live item is in a safe equipped or belt location.
    pub is_safe: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OathSelectionTransactionState {
    pub character: StoredOathCharacter,
    pub inventory: StoredOathInventory,
    pub new_result: Option<StoredOathMutationResult>,
    pub new_event: Option<StoredCharacterLifeEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OathSelectionTransaction<T> {
    Replayed(Box<StoredOathMutationResult>),
    Committed(T),
}

impl PostgresPersistence {
    /// Reads one owned character's Oath eligibility projection without mutation locks.
    pub async fn oath_selection_snapshot(
        &self,
        account_id: [u8; ID_BYTES],
        character_id: [u8; ID_BYTES],
    ) -> Result<Option<StoredOathCharacter>, PersistenceError> {
        if all_zero(&account_id) || all_zero(&character_id) {
            return Err(PersistenceError::CorruptStoredOath);
        }
        let mut transaction = self.begin_transaction().await?;
        let selected = sqlx::query_scalar::<_, Option<Vec<u8>>>(
            "SELECT selected_character_id FROM accounts \
             WHERE namespace_id = $1 AND account_id = $2",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .fetch_optional(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?
        .flatten()
        .map(fixed_bytes)
        .transpose()?;
        let row = sqlx::query(
            "SELECT p.level, c.life_state, c.security_state, c.character_state_version, \
                    ob.oath_bargain_version, \
                    c.oath_id, l.character_version AS location_character_version, \
                    l.location_kind, l.location_content_id \
             FROM characters c \
             JOIN character_progression p USING (namespace_id, account_id, character_id) \
             JOIN character_world_locations l USING (namespace_id, account_id, character_id) \
             JOIN character_oath_bargain_state ob \
                  USING (namespace_id, account_id, character_id) \
             WHERE c.namespace_id = $1 AND c.account_id = $2 AND c.character_id = $3",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .fetch_optional(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?;
        transaction.rollback().await?;
        row.map(|value| {
            let character = decode_character(&value, selected)?;
            validate_character(&character)?;
            Ok(character)
        })
        .transpose()
    }

    /// Applies one initial-Oath mutation or returns its exact prior result.
    ///
    /// Lock order is account -> replay receipt -> character/progression/location -> inventory ->
    /// item UID. A replay never invokes `operation`, so current aggregate state cannot change its
    /// response. Inventory writers use the same character -> inventory order.
    pub async fn transact_initial_oath_selection<T, F>(
        &self,
        account_id: [u8; ID_BYTES],
        character_id: [u8; ID_BYTES],
        mutation_id: [u8; ID_BYTES],
        operation: F,
    ) -> Result<OathSelectionTransaction<T>, PersistenceError>
    where
        T: Send,
        F: FnOnce(&mut OathSelectionTransactionState) -> Result<T, PersistenceError> + Send,
    {
        if all_zero(&account_id) || all_zero(&character_id) || all_zero(&mutation_id) {
            return Err(PersistenceError::CorruptStoredOath);
        }
        let mut transaction = self.begin_transaction().await?;
        let selected_character_id = lock_account(transaction.connection(), &account_id).await?;
        if let Some(result) =
            load_replay(transaction.connection(), &account_id, &mutation_id).await?
        {
            transaction.rollback().await?;
            return Ok(OathSelectionTransaction::Replayed(Box::new(result)));
        }
        let character = lock_character(
            transaction.connection(),
            &account_id,
            &character_id,
            selected_character_id,
        )
        .await?;
        validate_character(&character)?;
        let inventory =
            lock_inventory_safety(transaction.connection(), &account_id, &character_id).await?;
        validate_inventory(&inventory)?;
        let original = character.clone();
        let original_inventory = inventory.clone();
        let mut state = OathSelectionTransactionState {
            character,
            inventory,
            new_result: None,
            new_event: None,
        };
        let output = operation(&mut state)?;
        let result = state
            .new_result
            .as_ref()
            .ok_or(PersistenceError::OathSelectionResultRequired)?;
        validate_result(result)?;
        validate_transition(
            &account_id,
            &character_id,
            &mutation_id,
            &original,
            &original_inventory,
            &state,
        )?;

        if result.result_code == ACCEPTED_RESULT_CODE {
            persist_accepted_oath_state(
                transaction.connection(),
                &account_id,
                &character_id,
                &state.character,
            )
            .await?;
        }
        insert_result(transaction.connection(), result).await?;
        if let Some(event) = &state.new_event {
            sqlx::query(
                "INSERT INTO character_life_outbox \
                 (namespace_id, account_id, character_id, event_id, event_type, \
                  aggregate_version, event_payload) \
                 VALUES ($1, $2, $3, $4, 'oath_selected', $5, $6)",
            )
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(account_id.as_slice())
            .bind(character_id.as_slice())
            .bind(event.event_id.as_slice())
            .bind(event.aggregate_version)
            .bind(&event.event_payload)
            .execute(transaction.connection())
            .await
            .map_err(PersistenceError::Database)?;
        }
        transaction.commit().await?;
        Ok(OathSelectionTransaction::Committed(output))
    }
}

async fn persist_accepted_oath_state(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    character: &StoredOathCharacter,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "UPDATE characters SET oath_id = $1, character_state_version = $2, \
                updated_at = transaction_timestamp() \
         WHERE namespace_id = $3 AND account_id = $4 AND character_id = $5",
    )
    .bind(&character.oath_id)
    .bind(character.character_state_version)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(&mut *connection)
    .await
    .map_err(PersistenceError::Database)?;
    sqlx::query(
        "UPDATE character_world_locations SET character_version = $1, \
                updated_at = transaction_timestamp() \
         WHERE namespace_id = $2 AND account_id = $3 AND character_id = $4",
    )
    .bind(character.location_character_version)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(&mut *connection)
    .await
    .map_err(PersistenceError::Database)?;
    sqlx::query(
        "UPDATE character_oath_bargain_state SET oath_bargain_version = $1, \
                updated_at = transaction_timestamp() \
         WHERE namespace_id = $2 AND account_id = $3 AND character_id = $4",
    )
    .bind(character.oath_bargain_version)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(connection)
    .await
    .map_err(PersistenceError::Database)?;
    Ok(())
}

async fn lock_inventory_safety(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
) -> Result<StoredOathInventory, PersistenceError> {
    let inventory_version = sqlx::query_scalar::<_, i64>(
        "SELECT inventory_version FROM character_inventories WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await
    .map_err(PersistenceError::Database)?;
    let Some(inventory_version) = inventory_version else {
        return Ok(StoredOathInventory {
            inventory_version: None,
            is_safe: false,
        });
    };
    let rows = sqlx::query(
        "SELECT security_state, location_kind FROM item_instances WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3 ORDER BY item_uid FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_all(connection)
    .await
    .map_err(PersistenceError::Database)?;
    let mut is_safe = true;
    for row in rows {
        let security_state = row
            .try_get::<i16, _>("security_state")
            .map_err(PersistenceError::Database)?;
        let location_kind = row
            .try_get::<i16, _>("location_kind")
            .map_err(PersistenceError::Database)?;
        is_safe &= matches!((security_state, location_kind), (0, 0 | 1) | (3, 4));
    }
    Ok(StoredOathInventory {
        inventory_version: Some(inventory_version),
        is_safe,
    })
}

async fn lock_account(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
) -> Result<Option<[u8; ID_BYTES]>, PersistenceError> {
    let row = sqlx::query(
        "SELECT selected_character_id FROM accounts \
         WHERE namespace_id = $1 AND account_id = $2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?
    .ok_or(PersistenceError::OathCharacterNotFound)?;
    row.try_get::<Option<Vec<u8>>, _>("selected_character_id")
        .map_err(PersistenceError::Database)?
        .map(fixed_bytes)
        .transpose()
}

async fn load_replay(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    mutation_id: &[u8; ID_BYTES],
) -> Result<Option<StoredOathMutationResult>, PersistenceError> {
    let row = sqlx::query(
        "SELECT account_id, character_id, mutation_id, payload_hash, oath_id, \
                pre_character_state_version, post_character_state_version, result_code, \
                result_payload \
         FROM character_oath_mutation_results \
         WHERE namespace_id = $1 AND account_id = $2 AND mutation_id = $3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(mutation_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?;
    row.map(|value| {
        let result = decode_result(&value)?;
        validate_result(&result)?;
        Ok(result)
    })
    .transpose()
}

async fn lock_character(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    selected_character_id: Option<[u8; ID_BYTES]>,
) -> Result<StoredOathCharacter, PersistenceError> {
    let row = sqlx::query(
        "SELECT p.level, c.life_state, c.security_state, c.character_state_version, \
                ob.oath_bargain_version, \
                c.oath_id, l.character_version AS location_character_version, \
                l.location_kind, l.location_content_id \
         FROM characters c \
         JOIN character_progression p USING (namespace_id, account_id, character_id) \
         JOIN character_world_locations l USING (namespace_id, account_id, character_id) \
         JOIN character_oath_bargain_state ob USING (namespace_id, account_id, character_id) \
         WHERE c.namespace_id = $1 AND c.account_id = $2 AND c.character_id = $3 \
         FOR UPDATE OF c, p, l, ob",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?
    .ok_or(PersistenceError::OathCharacterNotFound)?;
    decode_character(&row, selected_character_id)
}

fn decode_character(
    row: &sqlx::postgres::PgRow,
    selected_character_id: Option<[u8; ID_BYTES]>,
) -> Result<StoredOathCharacter, PersistenceError> {
    let level: i16 = row.try_get("level").map_err(PersistenceError::Database)?;
    Ok(StoredOathCharacter {
        selected_character_id,
        level,
        life_state: row
            .try_get("life_state")
            .map_err(PersistenceError::Database)?,
        security_state: row
            .try_get("security_state")
            .map_err(PersistenceError::Database)?,
        character_state_version: row
            .try_get("character_state_version")
            .map_err(PersistenceError::Database)?,
        oath_bargain_version: row
            .try_get("oath_bargain_version")
            .map_err(PersistenceError::Database)?,
        oath_id: row.try_get("oath_id").map_err(PersistenceError::Database)?,
        location_character_version: row
            .try_get("location_character_version")
            .map_err(PersistenceError::Database)?,
        location_kind: row
            .try_get("location_kind")
            .map_err(PersistenceError::Database)?,
        location_content_id: row
            .try_get("location_content_id")
            .map_err(PersistenceError::Database)?,
    })
}

fn decode_result(
    row: &sqlx::postgres::PgRow,
) -> Result<StoredOathMutationResult, PersistenceError> {
    Ok(StoredOathMutationResult {
        account_id: fixed_bytes(
            row.try_get("account_id")
                .map_err(PersistenceError::Database)?,
        )?,
        character_id: fixed_bytes(
            row.try_get("character_id")
                .map_err(PersistenceError::Database)?,
        )?,
        mutation_id: fixed_bytes(
            row.try_get("mutation_id")
                .map_err(PersistenceError::Database)?,
        )?,
        payload_hash: fixed_bytes(
            row.try_get("payload_hash")
                .map_err(PersistenceError::Database)?,
        )?,
        oath_id: row.try_get("oath_id").map_err(PersistenceError::Database)?,
        pre_character_state_version: row
            .try_get("pre_character_state_version")
            .map_err(PersistenceError::Database)?,
        post_character_state_version: row
            .try_get("post_character_state_version")
            .map_err(PersistenceError::Database)?,
        result_code: row
            .try_get("result_code")
            .map_err(PersistenceError::Database)?,
        result_payload: row
            .try_get("result_payload")
            .map_err(PersistenceError::Database)?,
    })
}

async fn insert_result(
    connection: &mut sqlx::PgConnection,
    result: &StoredOathMutationResult,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO character_oath_mutation_results \
         (namespace_id, account_id, character_id, mutation_id, payload_hash, oath_id, \
          pre_character_state_version, post_character_state_version, result_code, result_payload) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(result.account_id.as_slice())
    .bind(result.character_id.as_slice())
    .bind(result.mutation_id.as_slice())
    .bind(result.payload_hash.as_slice())
    .bind(&result.oath_id)
    .bind(result.pre_character_state_version)
    .bind(result.post_character_state_version)
    .bind(result.result_code)
    .bind(&result.result_payload)
    .execute(connection)
    .await
    .map_err(PersistenceError::Database)?;
    Ok(())
}

fn validate_transition(
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    mutation_id: &[u8; ID_BYTES],
    original: &StoredOathCharacter,
    original_inventory: &StoredOathInventory,
    state: &OathSelectionTransactionState,
) -> Result<(), PersistenceError> {
    validate_character(&state.character)?;
    validate_inventory(&state.inventory)?;
    let result = state
        .new_result
        .as_ref()
        .ok_or(PersistenceError::OathSelectionResultRequired)?;
    if &result.account_id != account_id
        || &result.character_id != character_id
        || &result.mutation_id != mutation_id
        || result.pre_character_state_version != original.character_state_version
    {
        return Err(PersistenceError::CorruptStoredOath);
    }
    let accepted = result.result_code == ACCEPTED_RESULT_CODE;
    if accepted {
        let event = state
            .new_event
            .as_ref()
            .ok_or(PersistenceError::OathSelectionEventRequired)?;
        if original.oath_id.is_some()
            || !original_inventory.is_safe
            || original.selected_character_id != Some(*character_id)
            || !(10..=20).contains(&original.level)
            || original.life_state != 0
            || original.security_state != 0
            || original.location_kind != 1
            || original.location_content_id.as_deref() != Some("hub.lantern_halls_01")
            || original.location_character_version != original.character_state_version
            || state.character.selected_character_id != original.selected_character_id
            || state.character.level != original.level
            || state.character.life_state != original.life_state
            || state.character.security_state != original.security_state
            || state.character.location_kind != original.location_kind
            || state.character.location_content_id != original.location_content_id
            || &state.inventory != original_inventory
            || state.character.oath_id.as_deref() != Some(result.oath_id.as_str())
            || state.character.character_state_version != original.character_state_version + 1
            || state.character.oath_bargain_version != original.oath_bargain_version + 1
            || state.character.location_character_version != state.character.character_state_version
            || result.post_character_state_version != state.character.character_state_version
            || event.event_id != *mutation_id
            || event.aggregate_version != state.character.oath_bargain_version
            || event.event_payload.is_empty()
            || event.event_payload.len() > MAX_RESULT_PAYLOAD_BYTES
        {
            return Err(PersistenceError::CorruptStoredOath);
        }
    } else if &state.character != original
        || &state.inventory != original_inventory
        || state.new_event.is_some()
        || result.post_character_state_version != original.character_state_version
    {
        return Err(PersistenceError::CorruptStoredOath);
    }
    Ok(())
}

fn validate_inventory(inventory: &StoredOathInventory) -> Result<(), PersistenceError> {
    if inventory
        .inventory_version
        .is_some_and(|version| version < 1)
        || (inventory.is_safe && inventory.inventory_version.is_none())
    {
        return Err(PersistenceError::CorruptStoredOath);
    }
    Ok(())
}

fn validate_character(character: &StoredOathCharacter) -> Result<(), PersistenceError> {
    if !(1..=20).contains(&character.level)
        || !matches!(character.life_state, 0..=1)
        || !matches!(character.security_state, 0..=1)
        || character.character_state_version < 1
        || character.oath_bargain_version < 1
        || character.location_character_version < 1
        || !matches!(character.location_kind, 0..=2)
        || character
            .oath_id
            .as_deref()
            .is_some_and(|value| !legal_oath_id(value))
    {
        return Err(PersistenceError::CorruptStoredOath);
    }
    Ok(())
}

fn validate_result(result: &StoredOathMutationResult) -> Result<(), PersistenceError> {
    let accepted = result.result_code == ACCEPTED_RESULT_CODE;
    if all_zero(&result.account_id)
        || all_zero(&result.character_id)
        || all_zero(&result.mutation_id)
        || all_zero(&result.payload_hash)
        || !legal_oath_id(&result.oath_id)
        || result.pre_character_state_version < 1
        || !(0..=18).contains(&result.result_code)
        || result.result_payload.is_empty()
        || result.result_payload.len() > MAX_RESULT_PAYLOAD_BYTES
        || (accepted
            && result.post_character_state_version != result.pre_character_state_version + 1)
        || (!accepted && result.post_character_state_version != result.pre_character_state_version)
    {
        return Err(PersistenceError::CorruptStoredOath);
    }
    Ok(())
}

fn legal_oath_id(value: &str) -> bool {
    matches!(value, LONG_VIGIL_ID | NAILKEEPER_ID)
}

fn fixed_bytes<const N: usize>(bytes: Vec<u8>) -> Result<[u8; N], PersistenceError> {
    bytes
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredOath)
}

const fn all_zero<const N: usize>(bytes: &[u8; N]) -> bool {
    let mut index = 0;
    while index < N {
        if bytes[index] != 0 {
            return false;
        }
        index += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn character() -> StoredOathCharacter {
        StoredOathCharacter {
            selected_character_id: Some([2; 16]),
            level: 10,
            life_state: 0,
            security_state: 0,
            character_state_version: 7,
            oath_bargain_version: 4,
            oath_id: None,
            location_character_version: 7,
            location_kind: 1,
            location_content_id: Some("hub.lantern_halls_01".into()),
        }
    }

    fn result(code: i16) -> StoredOathMutationResult {
        StoredOathMutationResult {
            account_id: [1; 16],
            character_id: [2; 16],
            mutation_id: [3; 16],
            payload_hash: [4; 32],
            oath_id: LONG_VIGIL_ID.into(),
            pre_character_state_version: 7,
            post_character_state_version: if code == ACCEPTED_RESULT_CODE { 8 } else { 7 },
            result_code: code,
            result_payload: vec![1],
        }
    }

    #[test]
    fn accepted_transition_requires_one_version_step_and_matching_outbox() {
        let original = character();
        let mut selected = original.clone();
        selected.oath_id = Some(LONG_VIGIL_ID.into());
        selected.character_state_version = 8;
        selected.oath_bargain_version = 5;
        selected.location_character_version = 8;
        let state = OathSelectionTransactionState {
            character: selected,
            inventory: StoredOathInventory {
                inventory_version: Some(4),
                is_safe: true,
            },
            new_result: Some(result(ACCEPTED_RESULT_CODE)),
            new_event: Some(StoredCharacterLifeEvent {
                event_id: [3; 16],
                aggregate_version: 5,
                event_payload: vec![1],
            }),
        };
        let inventory = state.inventory.clone();
        assert!(
            validate_transition(&[1; 16], &[2; 16], &[3; 16], &original, &inventory, &state)
                .is_ok()
        );
        let mut missing_event = state;
        missing_event.new_event = None;
        assert!(
            validate_transition(
                &[1; 16],
                &[2; 16],
                &[3; 16],
                &original,
                &inventory,
                &missing_event
            )
            .is_err()
        );
    }

    #[test]
    fn rejected_transition_cannot_mutate_character_or_emit_event() {
        let original = character();
        let clean = OathSelectionTransactionState {
            character: original.clone(),
            inventory: StoredOathInventory {
                inventory_version: Some(4),
                is_safe: true,
            },
            new_result: Some(result(10)),
            new_event: None,
        };
        let inventory = clean.inventory.clone();
        assert!(
            validate_transition(&[1; 16], &[2; 16], &[3; 16], &original, &inventory, &clean)
                .is_ok()
        );
        let mut changed = clean;
        changed.character.oath_id = Some(NAILKEEPER_ID.into());
        assert!(
            validate_transition(
                &[1; 16], &[2; 16], &[3; 16], &original, &inventory, &changed
            )
            .is_err()
        );
    }

    #[test]
    fn safe_inventory_requires_an_initialized_positive_version() {
        assert!(
            validate_inventory(&StoredOathInventory {
                inventory_version: Some(1),
                is_safe: true,
            })
            .is_ok()
        );
        assert!(
            validate_inventory(&StoredOathInventory {
                inventory_version: None,
                is_safe: true,
            })
            .is_err()
        );
    }
}
