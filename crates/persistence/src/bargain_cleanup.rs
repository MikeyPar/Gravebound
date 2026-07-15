//! Transaction participant for future death and retirement aggregate owners.

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{
    PersistenceError, PersistenceTransaction, StoredActiveBargain, WIPEABLE_CORE_NAMESPACE,
};

const ID_BYTES: usize = 16;
const MAX_EVENT_PAYLOAD_BYTES: usize = 65_536;
pub const BARGAIN_LIFE_CLEANUP_EVENT_SCHEMA_VERSION: u16 = 1;

/// Controls which terminal aggregate owns danger-checkpoint finalization.
///
/// The canonical GDD `DTH-001`/`TECH-023`, Content Production Spec danger-entry authority, and
/// Development Roadmap `GB-M03-06` atomic-death gate require durable death to retain the normalized
/// live trace until it has been promoted into the immutable death graph. Other life-end owners keep
/// the established eager checkpoint cleanup behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DangerCheckpointCleanupPolicy {
    Remove,
    PreserveForDurableDeath,
}

impl DangerCheckpointCleanupPolicy {
    const fn removes_checkpoint(self) -> bool {
        matches!(self, Self::Remove)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BargainLifeEndReason {
    Death,
    Retirement,
}

impl BargainLifeEndReason {
    const fn event_type(self) -> &'static str {
        match self {
            Self::Death => "bargains_cleared_death",
            Self::Retirement => "bargains_cleared_retirement",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BargainLifeCleanupCommand {
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub event_id: [u8; ID_BYTES],
    pub reason: BargainLifeEndReason,
    pub expected_oath_bargain_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BargainLifeCleanupResult {
    pub active_bargains: Vec<StoredActiveBargain>,
    pub pre_oath_bargain_version: i64,
    pub post_oath_bargain_version: i64,
    pub removed_danger_checkpoint: bool,
    pub event_payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BargainLifeCleanupEventV1 {
    pub schema_version: u16,
    pub reason: BargainLifeEndReason,
    pub pre_oath_bargain_version: i64,
    pub post_oath_bargain_version: i64,
    pub active_bargains: Vec<BargainLifeCleanupEventBargainV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BargainLifeCleanupEventBargainV1 {
    pub bargain_id: String,
    pub acquisition_ordinal: i16,
    pub acquired_by_offer_id: [u8; ID_BYTES],
}

impl BargainLifeCleanupEventV1 {
    pub fn decode(bytes: &[u8]) -> Result<Self, PersistenceError> {
        if bytes.is_empty() || bytes.len() > MAX_EVENT_PAYLOAD_BYTES {
            return Err(PersistenceError::CorruptBargainCleanup);
        }
        let event: Self =
            postcard::from_bytes(bytes).map_err(|_| PersistenceError::CorruptBargainCleanup)?;
        if event.schema_version != BARGAIN_LIFE_CLEANUP_EVENT_SCHEMA_VERSION
            || postcard::to_stdvec(&event).map_err(|_| PersistenceError::CorruptBargainCleanup)?
                != bytes
        {
            return Err(PersistenceError::CorruptBargainCleanup);
        }
        Ok(event)
    }
}

pub async fn cleanup_bargains_for_life_end(
    transaction: &mut PersistenceTransaction<'_>,
    command: &BargainLifeCleanupCommand,
) -> Result<BargainLifeCleanupResult, PersistenceError> {
    cleanup_bargains_for_life_end_with_checkpoint_policy(
        transaction,
        command,
        DangerCheckpointCleanupPolicy::Remove,
    )
    .await
}

pub(crate) async fn cleanup_bargains_for_durable_death(
    transaction: &mut PersistenceTransaction<'_>,
    command: &BargainLifeCleanupCommand,
) -> Result<BargainLifeCleanupResult, PersistenceError> {
    let checkpoint_policy = durable_death_checkpoint_policy(command.reason)?;
    cleanup_bargains_for_life_end_with_checkpoint_policy(transaction, command, checkpoint_policy)
        .await
}

fn durable_death_checkpoint_policy(
    reason: BargainLifeEndReason,
) -> Result<DangerCheckpointCleanupPolicy, PersistenceError> {
    if reason != BargainLifeEndReason::Death {
        return Err(PersistenceError::CorruptBargainCleanup);
    }
    Ok(DangerCheckpointCleanupPolicy::PreserveForDurableDeath)
}

async fn cleanup_bargains_for_life_end_with_checkpoint_policy(
    transaction: &mut PersistenceTransaction<'_>,
    command: &BargainLifeCleanupCommand,
    checkpoint_policy: DangerCheckpointCleanupPolicy,
) -> Result<BargainLifeCleanupResult, PersistenceError> {
    validate_command(command)?;
    let version: i64 = sqlx::query_scalar(
        "SELECT oath_bargain_version FROM character_oath_bargain_state WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.account_id.as_slice())
    .bind(command.character_id.as_slice())
    .fetch_optional(transaction.connection())
    .await?
    .ok_or(PersistenceError::BargainCharacterNotFound)?;
    if version != command.expected_oath_bargain_version {
        return Err(PersistenceError::BargainCleanupVersionMismatch);
    }
    let rows = sqlx::query(
        "SELECT bargain_id, acquisition_ordinal, acquired_by_offer_id \
         FROM character_active_bargains WHERE namespace_id = $1 AND account_id = $2 \
         AND character_id = $3 ORDER BY acquisition_ordinal FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.account_id.as_slice())
    .bind(command.character_id.as_slice())
    .fetch_all(transaction.connection())
    .await?;
    let active_bargains = rows
        .into_iter()
        .map(|row| {
            Ok(StoredActiveBargain {
                bargain_id: row.try_get("bargain_id")?,
                acquisition_ordinal: row.try_get("acquisition_ordinal")?,
                acquired_by_offer_id: fixed_id(row.try_get("acquired_by_offer_id")?)?,
            })
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    validate_ordered_bargains(&active_bargains)?;
    let post_version = version
        .checked_add(1)
        .ok_or(PersistenceError::CorruptBargainCleanup)?;
    let event_payload =
        encode_cleanup_event(command.reason, version, post_version, &active_bargains)?;
    sqlx::query(
        "DELETE FROM character_active_bargains WHERE namespace_id = $1 AND account_id = $2 \
         AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.account_id.as_slice())
    .bind(command.character_id.as_slice())
    .execute(transaction.connection())
    .await?;
    let removed_danger_checkpoint =
        apply_checkpoint_policy(transaction, command, checkpoint_policy).await?;
    let updated = sqlx::query(
        "UPDATE character_oath_bargain_state SET oath_bargain_version = $1, \
         updated_at = transaction_timestamp() WHERE namespace_id = $2 AND account_id = $3 \
         AND character_id = $4 AND oath_bargain_version = $5",
    )
    .bind(post_version)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.account_id.as_slice())
    .bind(command.character_id.as_slice())
    .bind(version)
    .execute(transaction.connection())
    .await?
    .rows_affected();
    if updated != 1 {
        return Err(PersistenceError::CorruptBargainCleanup);
    }
    sqlx::query(
        "INSERT INTO character_life_outbox (namespace_id, account_id, character_id, event_id, \
         event_type, aggregate_version, event_payload) VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.account_id.as_slice())
    .bind(command.character_id.as_slice())
    .bind(command.event_id.as_slice())
    .bind(command.reason.event_type())
    .bind(post_version)
    .bind(&event_payload)
    .execute(transaction.connection())
    .await?;
    Ok(BargainLifeCleanupResult {
        active_bargains,
        pre_oath_bargain_version: version,
        post_oath_bargain_version: post_version,
        removed_danger_checkpoint,
        event_payload,
    })
}

async fn apply_checkpoint_policy(
    transaction: &mut PersistenceTransaction<'_>,
    command: &BargainLifeCleanupCommand,
    checkpoint_policy: DangerCheckpointCleanupPolicy,
) -> Result<bool, PersistenceError> {
    if !checkpoint_policy.removes_checkpoint() {
        return Ok(false);
    }
    let removed = sqlx::query(
        "DELETE FROM character_danger_checkpoints WHERE namespace_id = $1 AND account_id = $2 \
         AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.account_id.as_slice())
    .bind(command.character_id.as_slice())
    .execute(transaction.connection())
    .await?
    .rows_affected();
    Ok(removed == 1)
}

fn encode_cleanup_event(
    reason: BargainLifeEndReason,
    pre_oath_bargain_version: i64,
    post_oath_bargain_version: i64,
    active_bargains: &[StoredActiveBargain],
) -> Result<Vec<u8>, PersistenceError> {
    postcard::to_stdvec(&BargainLifeCleanupEventV1 {
        schema_version: BARGAIN_LIFE_CLEANUP_EVENT_SCHEMA_VERSION,
        reason,
        pre_oath_bargain_version,
        post_oath_bargain_version,
        active_bargains: active_bargains
            .iter()
            .map(|bargain| BargainLifeCleanupEventBargainV1 {
                bargain_id: bargain.bargain_id.clone(),
                acquisition_ordinal: bargain.acquisition_ordinal,
                acquired_by_offer_id: bargain.acquired_by_offer_id,
            })
            .collect(),
    })
    .map_err(|_| PersistenceError::CorruptBargainCleanup)
}

fn validate_command(command: &BargainLifeCleanupCommand) -> Result<(), PersistenceError> {
    if [command.account_id, command.character_id, command.event_id]
        .iter()
        .any(|value| value.iter().all(|byte| *byte == 0))
        || command.expected_oath_bargain_version <= 0
    {
        return Err(PersistenceError::CorruptBargainCleanup);
    }
    Ok(())
}

fn validate_ordered_bargains(values: &[StoredActiveBargain]) -> Result<(), PersistenceError> {
    if values.len() > 3
        || values.iter().enumerate().any(|(index, bargain)| {
            bargain.acquisition_ordinal != i16::try_from(index + 1).unwrap_or(i16::MAX)
                || !matches!(
                    bargain.bargain_id.as_str(),
                    "bargain.bell_debt" | "bargain.cinder_hunger" | "bargain.lantern_ash"
                )
                || bargain.acquired_by_offer_id == [0; ID_BYTES]
        })
    {
        return Err(PersistenceError::CorruptBargainCleanup);
    }
    Ok(())
}

fn fixed_id(value: Vec<u8>) -> Result<[u8; ID_BYTES], PersistenceError> {
    value
        .try_into()
        .map_err(|_| PersistenceError::CorruptBargainCleanup)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_and_ordered_snapshot_validation_fail_closed() {
        let mut command = BargainLifeCleanupCommand {
            account_id: [1; ID_BYTES],
            character_id: [2; ID_BYTES],
            event_id: [3; ID_BYTES],
            reason: BargainLifeEndReason::Death,
            expected_oath_bargain_version: 4,
        };
        validate_command(&command).unwrap();
        command.event_id = [0; ID_BYTES];
        assert!(matches!(
            validate_command(&command),
            Err(PersistenceError::CorruptBargainCleanup)
        ));

        let event = BargainLifeCleanupEventV1 {
            schema_version: BARGAIN_LIFE_CLEANUP_EVENT_SCHEMA_VERSION,
            reason: BargainLifeEndReason::Retirement,
            pre_oath_bargain_version: 4,
            post_oath_bargain_version: 5,
            active_bargains: vec![BargainLifeCleanupEventBargainV1 {
                bargain_id: "bargain.bell_debt".into(),
                acquisition_ordinal: 1,
                acquired_by_offer_id: [4; ID_BYTES],
            }],
        };
        let encoded = postcard::to_stdvec(&event).unwrap();
        assert_eq!(BargainLifeCleanupEventV1::decode(&encoded).unwrap(), event);
        assert!(matches!(
            validate_ordered_bargains(&[StoredActiveBargain {
                bargain_id: "bargain.bell_debt".into(),
                acquisition_ordinal: 2,
                acquired_by_offer_id: [4; ID_BYTES],
            }]),
            Err(PersistenceError::CorruptBargainCleanup)
        ));
    }

    #[test]
    fn checkpoint_cleanup_policy_keeps_terminal_ownership_explicit() {
        assert!(DangerCheckpointCleanupPolicy::Remove.removes_checkpoint());
        assert!(!DangerCheckpointCleanupPolicy::PreserveForDurableDeath.removes_checkpoint());
        assert_eq!(
            durable_death_checkpoint_policy(BargainLifeEndReason::Death).unwrap(),
            DangerCheckpointCleanupPolicy::PreserveForDurableDeath
        );
        assert!(matches!(
            durable_death_checkpoint_policy(BargainLifeEndReason::Retirement),
            Err(PersistenceError::CorruptBargainCleanup)
        ));
    }
}
