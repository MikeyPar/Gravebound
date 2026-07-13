//! Durable replay-first Veil Bargain offer decisions for `GB-M03-05D`.

use std::collections::BTreeSet;

use sqlx::Row;

use crate::{
    PersistenceError, PostgresPersistence, StoredCharacterLifeEvent, WIPEABLE_CORE_NAMESPACE,
};

const ID_BYTES: usize = 16;
const HASH_BYTES: usize = 32;
const MAX_RESULT_PAYLOAD_BYTES: usize = 65_536;
const MAX_ACTIVE_BARGAINS: usize = 3;
const SELECT_DECISION_KIND: i16 = 0;
const REFUSE_DECISION_KIND: i16 = 1;
const SELECTED_RESULT_CODE: i16 = 0;
const REFUSED_RESULT_CODE: i16 = 1;
const OPEN_OFFER_STATE: i16 = 0;
const SELECTED_OFFER_STATE: i16 = 1;
const REFUSED_OFFER_STATE: i16 = 2;
const UNAVAILABLE_OFFER_STATE: i16 = 3;
const CORE_SOURCE_ID: &str = "miniboss.sepulcher_knight";
const CORE_LAYOUT_ID: &str = "layout.core_private_life_01";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredActiveBargain {
    pub bargain_id: String,
    pub acquisition_ordinal: i16,
    pub acquired_by_offer_id: [u8; ID_BYTES],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBargainLife {
    pub selected_character_id: Option<[u8; ID_BYTES]>,
    pub level: i16,
    pub life_state: i16,
    pub security_state: i16,
    pub character_state_version: i64,
    pub location_character_version: i64,
    pub location_kind: i16,
    pub location_content_id: Option<String>,
    pub instance_lineage_id: Option<[u8; ID_BYTES]>,
    pub entry_restore_point_id: Option<[u8; ID_BYTES]>,
    pub earned_bargain_slots: i16,
    pub oath_bargain_version: i64,
    pub active_bargains: Vec<StoredActiveBargain>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBargainCandidate {
    pub candidate_ordinal: i16,
    pub bargain_id: String,
    pub score: [u8; HASH_BYTES],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBargainOffer {
    pub offer_id: [u8; ID_BYTES],
    pub source_reward_event_id: [u8; ID_BYTES],
    pub source_content_id: String,
    pub source_layout_id: String,
    pub instance_lineage_id: [u8; ID_BYTES],
    pub entry_restore_point_id: [u8; ID_BYTES],
    pub content_version: String,
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
    pub offer_state: i16,
    pub selected_bargain_id: Option<String>,
    pub created_oath_bargain_version: i64,
    pub resolved_oath_bargain_version: Option<i64>,
    pub candidates: Vec<StoredBargainCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBargainDecisionResult {
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub mutation_id: [u8; ID_BYTES],
    pub offer_id: [u8; ID_BYTES],
    pub payload_hash: [u8; HASH_BYTES],
    pub decision_kind: i16,
    pub bargain_id: Option<String>,
    pub pre_oath_bargain_version: i64,
    pub post_oath_bargain_version: i64,
    pub result_code: i16,
    pub result_payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BargainDecisionTransactionState {
    pub life: StoredBargainLife,
    pub offer: StoredBargainOffer,
    pub new_result: Option<StoredBargainDecisionResult>,
    pub new_event: Option<StoredCharacterLifeEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BargainDecisionTransaction<T> {
    Replayed(Box<StoredBargainDecisionResult>),
    Committed(T),
}

impl PostgresPersistence {
    /// Applies one offer selection/refusal or returns the exact stored result before inspecting
    /// current character, location, offer, or active-Bargain state.
    pub async fn transact_bargain_decision<T, F>(
        &self,
        account_id: [u8; ID_BYTES],
        character_id: [u8; ID_BYTES],
        offer_id: [u8; ID_BYTES],
        mutation_id: [u8; ID_BYTES],
        operation: F,
    ) -> Result<BargainDecisionTransaction<T>, PersistenceError>
    where
        T: Send,
        F: FnOnce(&mut BargainDecisionTransactionState) -> Result<T, PersistenceError> + Send,
    {
        if [account_id, character_id, offer_id, mutation_id]
            .iter()
            .any(all_zero)
        {
            return Err(PersistenceError::CorruptStoredBargain);
        }
        let mut transaction = self.begin_transaction().await?;
        let selected_character_id = lock_account(transaction.connection(), &account_id).await?;
        if let Some(result) =
            load_replay(transaction.connection(), &account_id, &mutation_id).await?
        {
            transaction.rollback().await?;
            return Ok(BargainDecisionTransaction::Replayed(Box::new(result)));
        }
        let life = lock_life(
            transaction.connection(),
            &account_id,
            &character_id,
            selected_character_id,
        )
        .await?;
        let offer = lock_offer(
            transaction.connection(),
            &account_id,
            &character_id,
            &offer_id,
        )
        .await?;
        validate_life(&life)?;
        validate_offer(&offer)?;
        let original_life = life.clone();
        let original_offer = offer.clone();
        let mut state = BargainDecisionTransactionState {
            life,
            offer,
            new_result: None,
            new_event: None,
        };
        let output = operation(&mut state)?;
        validate_transition(
            &account_id,
            &character_id,
            &offer_id,
            &mutation_id,
            &original_life,
            &original_offer,
            &state,
        )?;
        persist_transition(transaction.connection(), &state).await?;
        insert_result(
            transaction.connection(),
            state
                .new_result
                .as_ref()
                .ok_or(PersistenceError::BargainDecisionResultRequired)?,
        )
        .await?;
        transaction.commit().await?;
        Ok(BargainDecisionTransaction::Committed(output))
    }
}

async fn lock_account(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
) -> Result<Option<[u8; ID_BYTES]>, PersistenceError> {
    let selected: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT selected_character_id FROM accounts WHERE namespace_id = $1 \
         AND account_id = $2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?
    .ok_or(PersistenceError::BargainCharacterNotFound)?;
    selected.map(fixed_bytes).transpose()
}

async fn load_replay(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    mutation_id: &[u8; ID_BYTES],
) -> Result<Option<StoredBargainDecisionResult>, PersistenceError> {
    let row = sqlx::query(
        "SELECT account_id, character_id, mutation_id, offer_id, payload_hash, decision_kind, \
                bargain_id, pre_oath_bargain_version, post_oath_bargain_version, result_code, \
                result_payload FROM bargain_decision_results WHERE namespace_id = $1 \
         AND account_id = $2 AND mutation_id = $3 FOR UPDATE",
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

async fn lock_life(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    selected_character_id: Option<[u8; ID_BYTES]>,
) -> Result<StoredBargainLife, PersistenceError> {
    let row = sqlx::query(
        "SELECT p.level, c.life_state, c.security_state, c.character_state_version, \
                l.character_version AS location_character_version, l.location_kind, \
                l.location_content_id, l.instance_lineage_id, l.entry_restore_point_id, \
                ob.earned_bargain_slots, ob.oath_bargain_version \
         FROM characters c JOIN character_progression p \
              USING (namespace_id, account_id, character_id) \
         JOIN character_world_locations l USING (namespace_id, account_id, character_id) \
         JOIN character_oath_bargain_state ob USING (namespace_id, account_id, character_id) \
         WHERE c.namespace_id = $1 AND c.account_id = $2 AND c.character_id = $3 \
         FOR UPDATE OF c, p, l, ob",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await
    .map_err(PersistenceError::Database)?
    .ok_or(PersistenceError::BargainCharacterNotFound)?;
    let mut life = decode_life(&row, selected_character_id)?;
    life.active_bargains = load_active(connection, account_id, character_id).await?;
    Ok(life)
}

async fn load_active(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
) -> Result<Vec<StoredActiveBargain>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT bargain_id, acquisition_ordinal, acquired_by_offer_id \
         FROM character_active_bargains WHERE namespace_id = $1 AND account_id = $2 \
         AND character_id = $3 ORDER BY acquisition_ordinal FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_all(connection)
    .await
    .map_err(PersistenceError::Database)?;
    rows.into_iter()
        .map(|row| {
            Ok(StoredActiveBargain {
                bargain_id: row.try_get("bargain_id")?,
                acquisition_ordinal: row.try_get("acquisition_ordinal")?,
                acquired_by_offer_id: fixed_bytes(row.try_get("acquired_by_offer_id")?)?,
            })
        })
        .collect()
}

async fn lock_offer(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    offer_id: &[u8; ID_BYTES],
) -> Result<StoredBargainOffer, PersistenceError> {
    let row = sqlx::query(
        "SELECT offer_id, source_reward_event_id, source_content_id, source_layout_id, \
                instance_lineage_id, entry_restore_point_id, content_version, records_blake3, \
                assets_blake3, localization_blake3, offer_state, selected_bargain_id, \
                created_oath_bargain_version, resolved_oath_bargain_version \
         FROM bargain_offers WHERE namespace_id = $1 AND account_id = $2 \
         AND character_id = $3 AND offer_id = $4 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(offer_id.as_slice())
    .fetch_optional(&mut *connection)
    .await
    .map_err(PersistenceError::Database)?
    .ok_or(PersistenceError::BargainOfferNotFound)?;
    let mut offer = decode_offer(&row)?;
    offer.candidates = load_candidates(connection, account_id, offer_id).await?;
    Ok(offer)
}

async fn load_candidates(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    offer_id: &[u8; ID_BYTES],
) -> Result<Vec<StoredBargainCandidate>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT candidate_ordinal, bargain_id, score FROM bargain_offer_candidates \
         WHERE namespace_id = $1 AND account_id = $2 AND offer_id = $3 \
         ORDER BY candidate_ordinal FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(offer_id.as_slice())
    .fetch_all(connection)
    .await
    .map_err(PersistenceError::Database)?;
    rows.into_iter()
        .map(|row| {
            Ok(StoredBargainCandidate {
                candidate_ordinal: row.try_get("candidate_ordinal")?,
                bargain_id: row.try_get("bargain_id")?,
                score: fixed_bytes(row.try_get("score")?)?,
            })
        })
        .collect()
}

fn decode_life(
    row: &sqlx::postgres::PgRow,
    selected_character_id: Option<[u8; ID_BYTES]>,
) -> Result<StoredBargainLife, PersistenceError> {
    Ok(StoredBargainLife {
        selected_character_id,
        level: row.try_get("level")?,
        life_state: row.try_get("life_state")?,
        security_state: row.try_get("security_state")?,
        character_state_version: row.try_get("character_state_version")?,
        location_character_version: row.try_get("location_character_version")?,
        location_kind: row.try_get("location_kind")?,
        location_content_id: row.try_get("location_content_id")?,
        instance_lineage_id: optional_fixed(row.try_get("instance_lineage_id")?)?,
        entry_restore_point_id: optional_fixed(row.try_get("entry_restore_point_id")?)?,
        earned_bargain_slots: row.try_get("earned_bargain_slots")?,
        oath_bargain_version: row.try_get("oath_bargain_version")?,
        active_bargains: Vec::new(),
    })
}

fn decode_offer(row: &sqlx::postgres::PgRow) -> Result<StoredBargainOffer, PersistenceError> {
    Ok(StoredBargainOffer {
        offer_id: fixed_bytes(row.try_get("offer_id")?)?,
        source_reward_event_id: fixed_bytes(row.try_get("source_reward_event_id")?)?,
        source_content_id: row.try_get("source_content_id")?,
        source_layout_id: row.try_get("source_layout_id")?,
        instance_lineage_id: fixed_bytes(row.try_get("instance_lineage_id")?)?,
        entry_restore_point_id: fixed_bytes(row.try_get("entry_restore_point_id")?)?,
        content_version: row.try_get("content_version")?,
        records_blake3: row.try_get("records_blake3")?,
        assets_blake3: row.try_get("assets_blake3")?,
        localization_blake3: row.try_get("localization_blake3")?,
        offer_state: row.try_get("offer_state")?,
        selected_bargain_id: row.try_get("selected_bargain_id")?,
        created_oath_bargain_version: row.try_get("created_oath_bargain_version")?,
        resolved_oath_bargain_version: row.try_get("resolved_oath_bargain_version")?,
        candidates: Vec::new(),
    })
}

fn decode_result(
    row: &sqlx::postgres::PgRow,
) -> Result<StoredBargainDecisionResult, PersistenceError> {
    Ok(StoredBargainDecisionResult {
        account_id: fixed_bytes(row.try_get("account_id")?)?,
        character_id: fixed_bytes(row.try_get("character_id")?)?,
        mutation_id: fixed_bytes(row.try_get("mutation_id")?)?,
        offer_id: fixed_bytes(row.try_get("offer_id")?)?,
        payload_hash: fixed_bytes(row.try_get("payload_hash")?)?,
        decision_kind: row.try_get("decision_kind")?,
        bargain_id: row.try_get("bargain_id")?,
        pre_oath_bargain_version: row.try_get("pre_oath_bargain_version")?,
        post_oath_bargain_version: row.try_get("post_oath_bargain_version")?,
        result_code: row.try_get("result_code")?,
        result_payload: row.try_get("result_payload")?,
    })
}

async fn persist_transition(
    connection: &mut sqlx::PgConnection,
    state: &BargainDecisionTransactionState,
) -> Result<(), PersistenceError> {
    let result = state
        .new_result
        .as_ref()
        .ok_or(PersistenceError::BargainDecisionResultRequired)?;
    match result.result_code {
        SELECTED_RESULT_CODE => persist_selection(connection, state, result).await?,
        REFUSED_RESULT_CODE => persist_refusal(connection, state, result).await?,
        _ => {}
    }
    Ok(())
}

async fn persist_selection(
    connection: &mut sqlx::PgConnection,
    state: &BargainDecisionTransactionState,
    result: &StoredBargainDecisionResult,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "UPDATE character_oath_bargain_state SET oath_bargain_version = $1, \
                updated_at = transaction_timestamp() WHERE namespace_id = $2 \
         AND account_id = $3 AND character_id = $4",
    )
    .bind(state.life.oath_bargain_version)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(result.account_id.as_slice())
    .bind(result.character_id.as_slice())
    .execute(&mut *connection)
    .await?;
    let active = state
        .life
        .active_bargains
        .last()
        .ok_or(PersistenceError::CorruptStoredBargain)?;
    sqlx::query(
        "INSERT INTO character_active_bargains (namespace_id, account_id, character_id, \
         bargain_id, acquisition_ordinal, acquired_by_offer_id) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(result.account_id.as_slice())
    .bind(result.character_id.as_slice())
    .bind(&active.bargain_id)
    .bind(active.acquisition_ordinal)
    .bind(active.acquired_by_offer_id.as_slice())
    .execute(&mut *connection)
    .await?;
    update_offer(connection, &result.account_id, &state.offer).await?;
    let event = state
        .new_event
        .as_ref()
        .ok_or(PersistenceError::BargainSelectionEventRequired)?;
    sqlx::query(
        "INSERT INTO character_life_outbox (namespace_id, account_id, character_id, event_id, \
         event_type, aggregate_version, event_payload) \
         VALUES ($1, $2, $3, $4, 'bargain_selected', $5, $6)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(result.account_id.as_slice())
    .bind(result.character_id.as_slice())
    .bind(event.event_id.as_slice())
    .bind(event.aggregate_version)
    .bind(&event.event_payload)
    .execute(connection)
    .await?;
    Ok(())
}

async fn persist_refusal(
    connection: &mut sqlx::PgConnection,
    state: &BargainDecisionTransactionState,
    result: &StoredBargainDecisionResult,
) -> Result<(), PersistenceError> {
    update_offer(connection, &result.account_id, &state.offer).await
}

async fn update_offer(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    offer: &StoredBargainOffer,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "UPDATE bargain_offers SET offer_state = $1, selected_bargain_id = $2, \
         resolved_oath_bargain_version = $3, resolved_at = transaction_timestamp() \
         WHERE namespace_id = $4 AND account_id = $5 AND offer_id = $6",
    )
    .bind(offer.offer_state)
    .bind(&offer.selected_bargain_id)
    .bind(offer.resolved_oath_bargain_version)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(offer.offer_id.as_slice())
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_result(
    connection: &mut sqlx::PgConnection,
    result: &StoredBargainDecisionResult,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO bargain_decision_results (namespace_id, account_id, character_id, \
         mutation_id, offer_id, payload_hash, decision_kind, bargain_id, \
         pre_oath_bargain_version, post_oath_bargain_version, result_code, result_payload) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(result.account_id.as_slice())
    .bind(result.character_id.as_slice())
    .bind(result.mutation_id.as_slice())
    .bind(result.offer_id.as_slice())
    .bind(result.payload_hash.as_slice())
    .bind(result.decision_kind)
    .bind(&result.bargain_id)
    .bind(result.pre_oath_bargain_version)
    .bind(result.post_oath_bargain_version)
    .bind(result.result_code)
    .bind(&result.result_payload)
    .execute(connection)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_transition(
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    offer_id: &[u8; ID_BYTES],
    mutation_id: &[u8; ID_BYTES],
    original_life: &StoredBargainLife,
    original_offer: &StoredBargainOffer,
    state: &BargainDecisionTransactionState,
) -> Result<(), PersistenceError> {
    validate_life(&state.life)?;
    validate_offer(&state.offer)?;
    let result = state
        .new_result
        .as_ref()
        .ok_or(PersistenceError::BargainDecisionResultRequired)?;
    validate_result(result)?;
    if &result.account_id != account_id
        || &result.character_id != character_id
        || &result.offer_id != offer_id
        || &result.mutation_id != mutation_id
        || result.pre_oath_bargain_version != original_life.oath_bargain_version
    {
        return Err(PersistenceError::CorruptStoredBargain);
    }
    match result.result_code {
        SELECTED_RESULT_CODE => {
            validate_selected_transition(original_life, original_offer, state, result)
        }
        REFUSED_RESULT_CODE => {
            validate_refused_transition(original_life, original_offer, state, result)
        }
        _ if &state.life == original_life
            && &state.offer == original_offer
            && state.new_event.is_none() =>
        {
            Ok(())
        }
        _ => Err(PersistenceError::CorruptStoredBargain),
    }
}

fn validate_selected_transition(
    original_life: &StoredBargainLife,
    original_offer: &StoredBargainOffer,
    state: &BargainDecisionTransactionState,
    result: &StoredBargainDecisionResult,
) -> Result<(), PersistenceError> {
    let event = state
        .new_event
        .as_ref()
        .ok_or(PersistenceError::BargainSelectionEventRequired)?;
    let selected = result
        .bargain_id
        .as_deref()
        .ok_or(PersistenceError::CorruptStoredBargain)?;
    let new_active = state
        .life
        .active_bargains
        .last()
        .ok_or(PersistenceError::CorruptStoredBargain)?;
    let original_active_count = original_life.active_bargains.len();
    let common_life_unchanged = state.life.selected_character_id
        == original_life.selected_character_id
        && state.life.level == original_life.level
        && state.life.life_state == original_life.life_state
        && state.life.security_state == original_life.security_state
        && state.life.character_state_version == original_life.character_state_version
        && state.life.location_character_version == original_life.location_character_version
        && state.life.location_kind == original_life.location_kind
        && state.life.location_content_id == original_life.location_content_id
        && state.life.instance_lineage_id == original_life.instance_lineage_id
        && state.life.entry_restore_point_id == original_life.entry_restore_point_id
        && state.life.earned_bargain_slots == original_life.earned_bargain_slots;
    if original_offer.offer_state != OPEN_OFFER_STATE
        || original_life.selected_character_id != Some(result.character_id)
        || original_life.life_state != 0
        || original_life.security_state != 0
        || original_life.character_state_version != original_life.location_character_version
        || original_life.location_kind != 2
        || original_life.instance_lineage_id != Some(original_offer.instance_lineage_id)
        || original_life.entry_restore_point_id != Some(original_offer.entry_restore_point_id)
        || original_active_count >= usize::try_from(original_life.earned_bargain_slots).unwrap_or(0)
        || original_life
            .active_bargains
            .iter()
            .any(|value| value.bargain_id == selected)
        || !original_offer
            .candidates
            .iter()
            .any(|value| value.bargain_id == selected)
        || !common_life_unchanged
        || state.life.oath_bargain_version != original_life.oath_bargain_version + 1
        || state.life.active_bargains[..original_active_count] != original_life.active_bargains
        || state.life.active_bargains.len() != original_active_count + 1
        || new_active.bargain_id != selected
        || new_active.acquisition_ordinal != i16::try_from(original_active_count + 1).unwrap_or(0)
        || new_active.acquired_by_offer_id != original_offer.offer_id
        || state.offer.offer_state != SELECTED_OFFER_STATE
        || state.offer.selected_bargain_id.as_deref() != Some(selected)
        || state.offer.resolved_oath_bargain_version != Some(state.life.oath_bargain_version)
        || offer_immutable_fields_changed(original_offer, &state.offer)
        || result.post_oath_bargain_version != state.life.oath_bargain_version
        || event.event_id != result.mutation_id
        || event.aggregate_version != state.life.oath_bargain_version
        || event.event_payload.is_empty()
        || event.event_payload.len() > MAX_RESULT_PAYLOAD_BYTES
    {
        return Err(PersistenceError::CorruptStoredBargain);
    }
    Ok(())
}

fn validate_refused_transition(
    original_life: &StoredBargainLife,
    original_offer: &StoredBargainOffer,
    state: &BargainDecisionTransactionState,
    result: &StoredBargainDecisionResult,
) -> Result<(), PersistenceError> {
    if original_offer.offer_state != OPEN_OFFER_STATE
        || &state.life != original_life
        || state.offer.offer_state != REFUSED_OFFER_STATE
        || state.offer.selected_bargain_id.is_some()
        || state.offer.resolved_oath_bargain_version != Some(original_life.oath_bargain_version)
        || offer_immutable_fields_changed(original_offer, &state.offer)
        || result.post_oath_bargain_version != original_life.oath_bargain_version
        || state.new_event.is_some()
    {
        return Err(PersistenceError::CorruptStoredBargain);
    }
    Ok(())
}

fn offer_immutable_fields_changed(
    original: &StoredBargainOffer,
    current: &StoredBargainOffer,
) -> bool {
    let mut normalized = current.clone();
    normalized.offer_state = original.offer_state;
    normalized
        .selected_bargain_id
        .clone_from(&original.selected_bargain_id);
    normalized.resolved_oath_bargain_version = original.resolved_oath_bargain_version;
    &normalized != original
}

fn validate_life(life: &StoredBargainLife) -> Result<(), PersistenceError> {
    if !(1..=20).contains(&life.level)
        || !matches!(life.life_state, 0..=1)
        || !matches!(life.security_state, 0..=1)
        || life.character_state_version < 1
        || life.location_character_version < 1
        || !matches!(life.location_kind, 0..=2)
        || !(0..=3).contains(&life.earned_bargain_slots)
        || life.oath_bargain_version < 1
        || life.active_bargains.len() > usize::try_from(life.earned_bargain_slots).unwrap_or(0)
    {
        return Err(PersistenceError::CorruptStoredBargain);
    }
    let mut ids = BTreeSet::new();
    for (index, active) in life.active_bargains.iter().enumerate() {
        if !legal_bargain_id(&active.bargain_id)
            || active.acquisition_ordinal != i16::try_from(index + 1).unwrap_or(0)
            || all_zero(&active.acquired_by_offer_id)
            || !ids.insert(active.bargain_id.as_str())
        {
            return Err(PersistenceError::CorruptStoredBargain);
        }
    }
    Ok(())
}

fn validate_offer(offer: &StoredBargainOffer) -> Result<(), PersistenceError> {
    let revision_valid = [
        &offer.records_blake3,
        &offer.assets_blake3,
        &offer.localization_blake3,
    ]
    .into_iter()
    .all(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()));
    if all_zero(&offer.offer_id)
        || offer.offer_id != offer.source_reward_event_id
        || offer.source_content_id != CORE_SOURCE_ID
        || offer.source_layout_id != CORE_LAYOUT_ID
        || all_zero(&offer.instance_lineage_id)
        || all_zero(&offer.entry_restore_point_id)
        || offer.content_version.is_empty()
        || offer.content_version.len() > 96
        || !revision_valid
        || !matches!(offer.offer_state, 0..=3)
        || offer.created_oath_bargain_version < 1
        || offer.candidates.len() > MAX_ACTIVE_BARGAINS
    {
        return Err(PersistenceError::CorruptStoredBargain);
    }
    let mut candidate_ids = BTreeSet::new();
    for (index, candidate) in offer.candidates.iter().enumerate() {
        if candidate.candidate_ordinal != i16::try_from(index).unwrap_or(-1)
            || !legal_bargain_id(&candidate.bargain_id)
            || all_zero(&candidate.score)
            || !candidate_ids.insert(candidate.bargain_id.as_str())
        {
            return Err(PersistenceError::CorruptStoredBargain);
        }
    }
    let shape_valid = match offer.offer_state {
        OPEN_OFFER_STATE => {
            !offer.candidates.is_empty()
                && offer.selected_bargain_id.is_none()
                && offer.resolved_oath_bargain_version.is_none()
        }
        SELECTED_OFFER_STATE => offer
            .selected_bargain_id
            .as_deref()
            .is_some_and(|selected| {
                candidate_ids.contains(selected)
                    && offer
                        .resolved_oath_bargain_version
                        .is_some_and(|version| version > offer.created_oath_bargain_version)
            }),
        REFUSED_OFFER_STATE => {
            offer.selected_bargain_id.is_none()
                && offer
                    .resolved_oath_bargain_version
                    .is_some_and(|version| version >= offer.created_oath_bargain_version)
        }
        UNAVAILABLE_OFFER_STATE => {
            offer.candidates.is_empty()
                && offer.selected_bargain_id.is_none()
                && offer.resolved_oath_bargain_version == Some(offer.created_oath_bargain_version)
        }
        _ => false,
    };
    if !shape_valid {
        return Err(PersistenceError::CorruptStoredBargain);
    }
    Ok(())
}

fn validate_result(result: &StoredBargainDecisionResult) -> Result<(), PersistenceError> {
    let decision_valid = match result.decision_kind {
        SELECT_DECISION_KIND => result.bargain_id.as_deref().is_some_and(legal_bargain_id),
        REFUSE_DECISION_KIND => result.bargain_id.is_none(),
        _ => false,
    };
    let version_valid = if result.result_code == SELECTED_RESULT_CODE {
        result.decision_kind == SELECT_DECISION_KIND
            && result.post_oath_bargain_version == result.pre_oath_bargain_version + 1
    } else {
        result.post_oath_bargain_version == result.pre_oath_bargain_version
            && (result.result_code != REFUSED_RESULT_CODE
                || result.decision_kind == REFUSE_DECISION_KIND)
    };
    if [
        result.account_id,
        result.character_id,
        result.mutation_id,
        result.offer_id,
    ]
    .iter()
    .any(all_zero)
        || all_zero(&result.payload_hash)
        || !decision_valid
        || !(0..=15).contains(&result.result_code)
        || result.pre_oath_bargain_version < 1
        || !version_valid
        || result.result_payload.is_empty()
        || result.result_payload.len() > MAX_RESULT_PAYLOAD_BYTES
    {
        return Err(PersistenceError::CorruptStoredBargain);
    }
    Ok(())
}

fn legal_bargain_id(value: &str) -> bool {
    matches!(
        value,
        "bargain.bell_debt" | "bargain.cinder_hunger" | "bargain.lantern_ash"
    )
}

fn fixed_bytes<const N: usize>(bytes: Vec<u8>) -> Result<[u8; N], PersistenceError> {
    bytes
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredBargain)
}

fn optional_fixed<const N: usize>(
    bytes: Option<Vec<u8>>,
) -> Result<Option<[u8; N]>, PersistenceError> {
    bytes.map(fixed_bytes).transpose()
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

    fn life() -> StoredBargainLife {
        StoredBargainLife {
            selected_character_id: Some([2; 16]),
            level: 5,
            life_state: 0,
            security_state: 0,
            character_state_version: 3,
            location_character_version: 3,
            location_kind: 2,
            location_content_id: Some("world.core_microrealm_01".into()),
            instance_lineage_id: Some([7; 16]),
            entry_restore_point_id: Some([8; 16]),
            earned_bargain_slots: 1,
            oath_bargain_version: 2,
            active_bargains: Vec::new(),
        }
    }

    fn offer() -> StoredBargainOffer {
        StoredBargainOffer {
            offer_id: [5; 16],
            source_reward_event_id: [5; 16],
            source_content_id: CORE_SOURCE_ID.into(),
            source_layout_id: CORE_LAYOUT_ID.into(),
            instance_lineage_id: [7; 16],
            entry_restore_point_id: [8; 16],
            content_version: format!("core-dev.blake3.{}", "1".repeat(64)),
            records_blake3: "2".repeat(64),
            assets_blake3: "3".repeat(64),
            localization_blake3: "4".repeat(64),
            offer_state: OPEN_OFFER_STATE,
            selected_bargain_id: None,
            created_oath_bargain_version: 2,
            resolved_oath_bargain_version: None,
            candidates: vec![StoredBargainCandidate {
                candidate_ordinal: 0,
                bargain_id: "bargain.cinder_hunger".into(),
                score: [9; 32],
            }],
        }
    }

    fn result(code: i16, bargain_id: Option<&str>) -> StoredBargainDecisionResult {
        StoredBargainDecisionResult {
            account_id: [1; 16],
            character_id: [2; 16],
            mutation_id: [4; 16],
            offer_id: [5; 16],
            payload_hash: [6; 32],
            decision_kind: i16::from(bargain_id.is_none()),
            bargain_id: bargain_id.map(str::to_owned),
            pre_oath_bargain_version: 2,
            post_oath_bargain_version: if code == 0 { 3 } else { 2 },
            result_code: code,
            result_payload: vec![1],
        }
    }

    #[test]
    fn selection_requires_exact_candidate_append_version_and_event() {
        let original_life = life();
        let original_offer = offer();
        let mut selected_life = original_life.clone();
        selected_life.oath_bargain_version = 3;
        selected_life.active_bargains.push(StoredActiveBargain {
            bargain_id: "bargain.cinder_hunger".into(),
            acquisition_ordinal: 1,
            acquired_by_offer_id: [5; 16],
        });
        let mut selected_offer = original_offer.clone();
        selected_offer.offer_state = SELECTED_OFFER_STATE;
        selected_offer.selected_bargain_id = Some("bargain.cinder_hunger".into());
        selected_offer.resolved_oath_bargain_version = Some(3);
        let state = BargainDecisionTransactionState {
            life: selected_life,
            offer: selected_offer,
            new_result: Some(result(0, Some("bargain.cinder_hunger"))),
            new_event: Some(StoredCharacterLifeEvent {
                event_id: [4; 16],
                aggregate_version: 3,
                event_payload: vec![1],
            }),
        };
        assert!(
            validate_transition(
                &[1; 16],
                &[2; 16],
                &[5; 16],
                &[4; 16],
                &original_life,
                &original_offer,
                &state,
            )
            .is_ok()
        );
    }

    #[test]
    fn refusal_is_terminal_without_consuming_slot_or_advancing_life() {
        let original_life = life();
        let original_offer = offer();
        let mut refused_offer = original_offer.clone();
        refused_offer.offer_state = REFUSED_OFFER_STATE;
        refused_offer.resolved_oath_bargain_version = Some(2);
        let state = BargainDecisionTransactionState {
            life: original_life.clone(),
            offer: refused_offer,
            new_result: Some(result(1, None)),
            new_event: None,
        };
        assert!(
            validate_transition(
                &[1; 16],
                &[2; 16],
                &[5; 16],
                &[4; 16],
                &original_life,
                &original_offer,
                &state,
            )
            .is_ok()
        );
    }

    #[test]
    fn malformed_offer_life_and_result_fail_closed() {
        let mut bad_offer = offer();
        bad_offer.candidates[0].score = [0; 32];
        assert!(validate_offer(&bad_offer).is_err());
        let mut bad_life = life();
        bad_life.active_bargains = vec![
            StoredActiveBargain {
                bargain_id: "bargain.bell_debt".into(),
                acquisition_ordinal: 1,
                acquired_by_offer_id: [1; 16],
            },
            StoredActiveBargain {
                bargain_id: "bargain.bell_debt".into(),
                acquisition_ordinal: 2,
                acquired_by_offer_id: [2; 16],
            },
        ];
        bad_life.earned_bargain_slots = 2;
        assert!(validate_life(&bad_life).is_err());
        let mut bad_result = result(0, Some("bargain.cinder_hunger"));
        bad_result.post_oath_bargain_version = 2;
        assert!(validate_result(&bad_result).is_err());
    }
}
