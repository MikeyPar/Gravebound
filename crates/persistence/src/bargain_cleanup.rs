//! Transaction participant for future death and retirement aggregate owners.

use sqlx::Row;

use crate::{
    PersistenceError, PersistenceTransaction, StoredActiveBargain, WIPEABLE_CORE_NAMESPACE,
};

const ID_BYTES: usize = 16;
const MAX_EVENT_PAYLOAD_BYTES: usize = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// Canonical owner-produced payload containing the ordered Bargain snapshot and final result.
    pub event_payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BargainLifeCleanupResult {
    pub active_bargains: Vec<StoredActiveBargain>,
    pub pre_oath_bargain_version: i64,
    pub post_oath_bargain_version: i64,
    pub removed_danger_checkpoint: bool,
}

pub async fn cleanup_bargains_for_life_end(
    transaction: &mut PersistenceTransaction<'_>,
    command: &BargainLifeCleanupCommand,
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
    sqlx::query(
        "DELETE FROM character_active_bargains WHERE namespace_id = $1 AND account_id = $2 \
         AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.account_id.as_slice())
    .bind(command.character_id.as_slice())
    .execute(transaction.connection())
    .await?;
    let removed_danger_checkpoint = sqlx::query(
        "DELETE FROM character_danger_checkpoints WHERE namespace_id = $1 AND account_id = $2 \
         AND character_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.account_id.as_slice())
    .bind(command.character_id.as_slice())
    .execute(transaction.connection())
    .await?
    .rows_affected()
        == 1;
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
    .bind(&command.event_payload)
    .execute(transaction.connection())
    .await?;
    Ok(BargainLifeCleanupResult {
        active_bargains,
        pre_oath_bargain_version: version,
        post_oath_bargain_version: post_version,
        removed_danger_checkpoint,
    })
}

fn validate_command(command: &BargainLifeCleanupCommand) -> Result<(), PersistenceError> {
    if [command.account_id, command.character_id, command.event_id]
        .iter()
        .any(|value| value.iter().all(|byte| *byte == 0))
        || command.expected_oath_bargain_version <= 0
        || command.event_payload.is_empty()
        || command.event_payload.len() > MAX_EVENT_PAYLOAD_BYTES
    {
        return Err(PersistenceError::CorruptBargainCleanup);
    }
    Ok(())
}

fn validate_ordered_bargains(values: &[StoredActiveBargain]) -> Result<(), PersistenceError> {
    if values.len() > 3
        || values.iter().enumerate().any(|(index, bargain)| {
            bargain.acquisition_ordinal != i16::try_from(index + 1).unwrap_or(i16::MAX)
                || bargain.bargain_id.is_empty()
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
            event_payload: vec![1],
        };
        validate_command(&command).unwrap();
        command.event_payload.clear();
        assert!(matches!(
            validate_command(&command),
            Err(PersistenceError::CorruptBargainCleanup)
        ));
        assert!(matches!(
            validate_ordered_bargains(&[StoredActiveBargain {
                bargain_id: "bargain.bell_debt".into(),
                acquisition_ordinal: 2,
                acquired_by_offer_id: [4; ID_BYTES],
            }]),
            Err(PersistenceError::CorruptBargainCleanup)
        ));
    }
}
