//! Atomic Core Bargain milestone participation in progression awards.

use std::collections::BTreeSet;

use sqlx::Row;

use crate::{
    ASH_WALLET_CAP, AshMutationCode, AshMutationKind, AshMutationRequest, PersistenceError,
    StoredAshWallet, StoredBargainOffer, WIPEABLE_CORE_NAMESPACE,
    ash_wallet::apply_ash_mutation_on_connection, bargain::validate_offer,
    bargain_events::encode_bargain_offered,
};

const ID_BYTES: usize = 16;
const HASH_BYTES: usize = 32;
const MAX_RESULT_PAYLOAD_BYTES: usize = 65_536;
const MAX_BARGAINS: i16 = 3;
const OFFER_CREATED: i16 = 0;
const OFFER_UNAVAILABLE_WITH_ASH: i16 = 1;
const NO_SLOT_ASH: i16 = 2;
const OPEN_OFFER_STATE: i16 = 0;
const UNAVAILABLE_OFFER_STATE: i16 = 3;
const FALLBACK_ASH: i32 = 10;
pub const CORE_BARGAIN_MILESTONE_ID: &str = "milestone.core.sepulcher_knight_first_clear";
pub const CORE_BARGAIN_SOURCE_ID: &str = "miniboss.sepulcher_knight";
pub const CORE_BARGAIN_LAYOUT_ID: &str = "layout.core_private_life_01";
const FALLBACK_REASON: &str = "bargain_milestone_fallback";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBargainMilestoneLife {
    pub earned_bargain_slots: i16,
    pub oath_bargain_version: i64,
    pub active_bargain_ids: Vec<String>,
    pub core_milestone_awarded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBargainMilestoneResult {
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub source_reward_event_id: [u8; ID_BYTES],
    pub payload_hash: [u8; HASH_BYTES],
    pub result_code: i16,
    pub pre_oath_bargain_version: i64,
    pub post_oath_bargain_version: i64,
    pub pre_earned_bargain_slots: i16,
    pub post_earned_bargain_slots: i16,
    pub offer_id: Option<[u8; ID_BYTES]>,
    pub ash_mutation_id: Option<[u8; ID_BYTES]>,
    pub milestone_id: String,
    pub source_content_id: String,
    pub source_layout_id: String,
    pub instance_lineage_id: [u8; ID_BYTES],
    pub entry_restore_point_id: [u8; ID_BYTES],
    pub result_payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedBargainMilestone {
    pub life: StoredBargainMilestoneLife,
    pub result: StoredBargainMilestoneResult,
    pub offer: Option<StoredBargainOffer>,
    pub ash_request: Option<AshMutationRequest>,
}

pub(crate) async fn lock_bargain_milestone_life(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
) -> Result<StoredBargainMilestoneLife, PersistenceError> {
    let row = sqlx::query(
        "SELECT earned_bargain_slots, oath_bargain_version FROM character_oath_bargain_state \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::BargainCharacterNotFound)?;
    let active_rows = sqlx::query(
        "SELECT bargain_id FROM character_active_bargains WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3 ORDER BY acquisition_ordinal FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_all(&mut *connection)
    .await?;
    let core_milestone_awarded = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM bargain_milestone_results WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3 AND milestone_id = $4 FOR UPDATE)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(CORE_BARGAIN_MILESTONE_ID)
    .fetch_one(connection)
    .await?;
    let life = StoredBargainMilestoneLife {
        earned_bargain_slots: row.try_get("earned_bargain_slots")?,
        oath_bargain_version: row.try_get("oath_bargain_version")?,
        active_bargain_ids: active_rows
            .iter()
            .map(|value| value.try_get("bargain_id"))
            .collect::<Result<_, _>>()?,
        core_milestone_awarded,
    };
    validate_life(&life)?;
    Ok(life)
}

pub(crate) struct BargainMilestoneBinding<'a> {
    pub account_id: &'a [u8; ID_BYTES],
    pub character_id: &'a [u8; ID_BYTES],
    pub reward_event_id: &'a [u8; ID_BYTES],
    pub reward_payload_hash: &'a [u8; HASH_BYTES],
    pub layout_id: Option<&'a str>,
    pub instance_lineage_id: Option<&'a [u8; ID_BYTES]>,
    pub entry_restore_point_id: Option<&'a [u8; ID_BYTES]>,
    pub initial_life: &'a StoredBargainMilestoneLife,
    pub locked_wallet: StoredAshWallet,
}

pub(crate) async fn persist_bargain_milestone(
    connection: &mut sqlx::PgConnection,
    staged: &StagedBargainMilestone,
    binding: BargainMilestoneBinding<'_>,
) -> Result<(), PersistenceError> {
    validate_staged(staged, &binding)?;
    if staged.life != *binding.initial_life {
        sqlx::query(
            "UPDATE character_oath_bargain_state SET earned_bargain_slots = $1, \
             oath_bargain_version = $2, updated_at = transaction_timestamp() \
             WHERE namespace_id = $3 AND account_id = $4 AND character_id = $5",
        )
        .bind(staged.life.earned_bargain_slots)
        .bind(staged.life.oath_bargain_version)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(binding.account_id.as_slice())
        .bind(binding.character_id.as_slice())
        .execute(&mut *connection)
        .await?;
    }
    if let Some(offer) = &staged.offer {
        insert_offer(connection, binding.account_id, binding.character_id, offer).await?;
        if offer.offer_state == OPEN_OFFER_STATE {
            insert_offered_event(connection, binding.account_id, binding.character_id, offer)
                .await?;
        }
    }
    if let Some(request) = &staged.ash_request {
        let outcome = apply_ash_mutation_on_connection(connection, request).await?;
        if outcome.result().code != AshMutationCode::Accepted {
            return Err(PersistenceError::CorruptStoredBargain);
        }
    }
    insert_milestone_result(connection, &staged.result).await
}

async fn insert_offered_event(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    offer: &StoredBargainOffer,
) -> Result<(), PersistenceError> {
    let payload = encode_bargain_offered(offer)?;
    sqlx::query(
        "INSERT INTO character_life_outbox (namespace_id, account_id, character_id, event_id, \
         event_type, aggregate_version, event_payload) \
         VALUES ($1, $2, $3, $4, 'bargain_offered', $5, $6)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(offer.offer_id.as_slice())
    .bind(offer.created_oath_bargain_version)
    .bind(payload)
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_offer(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    offer: &StoredBargainOffer,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO bargain_offers (namespace_id, account_id, character_id, offer_id, \
         source_reward_event_id, source_content_id, source_layout_id, instance_lineage_id, \
         entry_restore_point_id, content_version, records_blake3, assets_blake3, \
         localization_blake3, offer_state, selected_bargain_id, created_oath_bargain_version, \
         resolved_oath_bargain_version, resolved_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, \
         $9, $10, $11, $12, $13, $14, $15, $16, $17, \
         CASE WHEN $17 IS NULL THEN NULL ELSE transaction_timestamp() END)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(offer.offer_id.as_slice())
    .bind(offer.source_reward_event_id.as_slice())
    .bind(&offer.source_content_id)
    .bind(&offer.source_layout_id)
    .bind(offer.instance_lineage_id.as_slice())
    .bind(offer.entry_restore_point_id.as_slice())
    .bind(&offer.content_version)
    .bind(&offer.records_blake3)
    .bind(&offer.assets_blake3)
    .bind(&offer.localization_blake3)
    .bind(offer.offer_state)
    .bind(&offer.selected_bargain_id)
    .bind(offer.created_oath_bargain_version)
    .bind(offer.resolved_oath_bargain_version)
    .execute(&mut *connection)
    .await?;
    for candidate in &offer.candidates {
        sqlx::query(
            "INSERT INTO bargain_offer_candidates (namespace_id, account_id, offer_id, \
             candidate_ordinal, bargain_id, score) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(offer.offer_id.as_slice())
        .bind(candidate.candidate_ordinal)
        .bind(&candidate.bargain_id)
        .bind(candidate.score.as_slice())
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

async fn insert_milestone_result(
    connection: &mut sqlx::PgConnection,
    result: &StoredBargainMilestoneResult,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO bargain_milestone_results (namespace_id, account_id, character_id, \
         source_reward_event_id, payload_hash, result_code, pre_oath_bargain_version, \
         post_oath_bargain_version, offer_id, ash_mutation_id, result_payload, milestone_id, \
         source_content_id, source_layout_id, instance_lineage_id, entry_restore_point_id, \
         pre_earned_bargain_slots, post_earned_bargain_slots) VALUES ($1, $2, $3, $4, $5, $6, \
         $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(result.account_id.as_slice())
    .bind(result.character_id.as_slice())
    .bind(result.source_reward_event_id.as_slice())
    .bind(result.payload_hash.as_slice())
    .bind(result.result_code)
    .bind(result.pre_oath_bargain_version)
    .bind(result.post_oath_bargain_version)
    .bind(result.offer_id.as_ref().map(<[u8; ID_BYTES]>::as_slice))
    .bind(
        result
            .ash_mutation_id
            .as_ref()
            .map(<[u8; ID_BYTES]>::as_slice),
    )
    .bind(&result.result_payload)
    .bind(&result.milestone_id)
    .bind(&result.source_content_id)
    .bind(&result.source_layout_id)
    .bind(result.instance_lineage_id.as_slice())
    .bind(result.entry_restore_point_id.as_slice())
    .bind(result.pre_earned_bargain_slots)
    .bind(result.post_earned_bargain_slots)
    .execute(connection)
    .await?;
    Ok(())
}

fn validate_staged(
    staged: &StagedBargainMilestone,
    binding: &BargainMilestoneBinding<'_>,
) -> Result<(), PersistenceError> {
    validate_life(&staged.life)?;
    validate_result(&staged.result)?;
    let result = &staged.result;
    if binding.initial_life.core_milestone_awarded
        || result.account_id != *binding.account_id
        || result.character_id != *binding.character_id
        || result.source_reward_event_id != *binding.reward_event_id
        || result.payload_hash != *binding.reward_payload_hash
        || result.pre_oath_bargain_version != binding.initial_life.oath_bargain_version
        || result.pre_earned_bargain_slots != binding.initial_life.earned_bargain_slots
        || result.source_layout_id.as_str() != binding.layout_id.unwrap_or_default()
        || Some(&result.instance_lineage_id) != binding.instance_lineage_id
        || Some(&result.entry_restore_point_id) != binding.entry_restore_point_id
        || staged.life.active_bargain_ids != binding.initial_life.active_bargain_ids
        || staged.life.core_milestone_awarded
        || staged.life.oath_bargain_version != result.post_oath_bargain_version
        || staged.life.earned_bargain_slots != result.post_earned_bargain_slots
    {
        return Err(PersistenceError::CorruptStoredBargain);
    }
    let offer_valid = match (result.result_code, staged.offer.as_ref()) {
        (OFFER_CREATED, Some(offer)) => {
            validate_offer(offer).is_ok()
                && offer.offer_state == OPEN_OFFER_STATE
                && !offer.candidates.is_empty()
        }
        (OFFER_UNAVAILABLE_WITH_ASH, Some(offer)) => {
            validate_offer(offer).is_ok()
                && offer.offer_state == UNAVAILABLE_OFFER_STATE
                && offer.candidates.is_empty()
        }
        (NO_SLOT_ASH, None) => true,
        _ => false,
    };
    if !offer_valid
        || staged.offer.as_ref().is_some_and(|offer| {
            offer.offer_id != result.source_reward_event_id
                || offer.instance_lineage_id != result.instance_lineage_id
                || offer.entry_restore_point_id != result.entry_restore_point_id
                || offer.source_layout_id != result.source_layout_id
                || offer.created_oath_bargain_version != result.post_oath_bargain_version
        })
        || !ash_shape_valid(staged, binding.locked_wallet)
    {
        return Err(PersistenceError::CorruptStoredBargain);
    }
    Ok(())
}

fn ash_shape_valid(staged: &StagedBargainMilestone, wallet: StoredAshWallet) -> bool {
    let needs_ash = matches!(
        staged.result.result_code,
        OFFER_UNAVAILABLE_WITH_ASH | NO_SLOT_ASH
    );
    match &staged.ash_request {
        None => !needs_ash && staged.result.ash_mutation_id.is_none(),
        Some(request) => {
            needs_ash
                && request.account_id == staged.result.account_id
                && request.mutation_id == staged.result.source_reward_event_id
                && request.payload_hash == staged.result.payload_hash
                && request.expected_wallet_version == wallet.wallet_version
                && request.kind == AshMutationKind::Earn
                && request.amount == FALLBACK_ASH
                && request.reason_code == FALLBACK_REASON
                && request.source_id == CORE_BARGAIN_MILESTONE_ID
                && request.content_version.len() <= 128
                && staged.result.ash_mutation_id == Some(request.mutation_id)
                && wallet.balance <= ASH_WALLET_CAP - FALLBACK_ASH
        }
    }
}

fn validate_life(life: &StoredBargainMilestoneLife) -> Result<(), PersistenceError> {
    let unique = life.active_bargain_ids.iter().collect::<BTreeSet<_>>();
    if !(0..=MAX_BARGAINS).contains(&life.earned_bargain_slots)
        || life.oath_bargain_version < 1
        || life.active_bargain_ids.len()
            > usize::try_from(life.earned_bargain_slots).unwrap_or_default()
        || unique.len() != life.active_bargain_ids.len()
        || life.active_bargain_ids.iter().any(|id| {
            !matches!(
                id.as_str(),
                "bargain.bell_debt" | "bargain.cinder_hunger" | "bargain.lantern_ash"
            )
        })
    {
        return Err(PersistenceError::CorruptStoredBargain);
    }
    Ok(())
}

fn validate_result(result: &StoredBargainMilestoneResult) -> Result<(), PersistenceError> {
    let slot_granted = matches!(
        result.result_code,
        OFFER_CREATED | OFFER_UNAVAILABLE_WITH_ASH
    );
    let shape = match result.result_code {
        OFFER_CREATED => {
            result.offer_id == Some(result.source_reward_event_id)
                && result.ash_mutation_id.is_none()
        }
        OFFER_UNAVAILABLE_WITH_ASH => {
            result.offer_id == Some(result.source_reward_event_id)
                && result.ash_mutation_id == Some(result.source_reward_event_id)
        }
        NO_SLOT_ASH => {
            result.offer_id.is_none()
                && result.ash_mutation_id == Some(result.source_reward_event_id)
        }
        _ => false,
    };
    if result.account_id == [0; ID_BYTES]
        || result.character_id == [0; ID_BYTES]
        || result.source_reward_event_id == [0; ID_BYTES]
        || result.payload_hash == [0; HASH_BYTES]
        || result.milestone_id != CORE_BARGAIN_MILESTONE_ID
        || result.source_content_id != CORE_BARGAIN_SOURCE_ID
        || result.source_layout_id != CORE_BARGAIN_LAYOUT_ID
        || result.instance_lineage_id == [0; ID_BYTES]
        || result.entry_restore_point_id == [0; ID_BYTES]
        || result.pre_oath_bargain_version < 1
        || result.post_oath_bargain_version
            != result.pre_oath_bargain_version + i64::from(slot_granted)
        || result.post_earned_bargain_slots
            != result.pre_earned_bargain_slots + i16::from(slot_granted)
        || !(0..=MAX_BARGAINS).contains(&result.pre_earned_bargain_slots)
        || !(0..=MAX_BARGAINS).contains(&result.post_earned_bargain_slots)
        || result.result_payload.is_empty()
        || result.result_payload.len() > MAX_RESULT_PAYLOAD_BYTES
        || !shape
    {
        return Err(PersistenceError::CorruptStoredBargain);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn milestone_result_pins_slot_version_and_cross_domain_shapes() {
        let base = StoredBargainMilestoneResult {
            account_id: [1; 16],
            character_id: [2; 16],
            source_reward_event_id: [3; 16],
            payload_hash: [4; 32],
            result_code: OFFER_CREATED,
            pre_oath_bargain_version: 1,
            post_oath_bargain_version: 2,
            pre_earned_bargain_slots: 0,
            post_earned_bargain_slots: 1,
            offer_id: Some([3; 16]),
            ash_mutation_id: None,
            milestone_id: CORE_BARGAIN_MILESTONE_ID.into(),
            source_content_id: CORE_BARGAIN_SOURCE_ID.into(),
            source_layout_id: CORE_BARGAIN_LAYOUT_ID.into(),
            instance_lineage_id: [5; 16],
            entry_restore_point_id: [6; 16],
            result_payload: vec![1],
        };
        assert!(validate_result(&base).is_ok());
        let mut unavailable = base.clone();
        unavailable.result_code = OFFER_UNAVAILABLE_WITH_ASH;
        unavailable.ash_mutation_id = Some([3; 16]);
        assert!(validate_result(&unavailable).is_ok());
        let mut corrupt = base;
        corrupt.post_earned_bargain_slots = 2;
        assert!(validate_result(&corrupt).is_err());
    }
}
