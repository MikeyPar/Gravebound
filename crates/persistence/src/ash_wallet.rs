//! Durable replay-first Ash Shard wallet and immutable currency ledger.

use sqlx::Row;

use crate::{PersistenceError, PostgresPersistence, WIPEABLE_CORE_NAMESPACE};

pub const ASH_WALLET_CAP: i32 = 99_999;
pub const ASH_CURRENCY_ID: &str = "currency.ash_shards";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AshMutationKind {
    Earn,
    Spend,
}

impl AshMutationKind {
    const fn code(self) -> i16 {
        match self {
            Self::Earn => 0,
            Self::Spend => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AshMutationCode {
    Accepted,
    InsufficientBalance,
    CapExceeded,
    StateVersionMismatch,
}

impl AshMutationCode {
    const fn code(self) -> i16 {
        match self {
            Self::Accepted => 0,
            Self::InsufficientBalance => 1,
            Self::CapExceeded => 2,
            Self::StateVersionMismatch => 3,
        }
    }

    fn from_code(value: i16) -> Result<Self, PersistenceError> {
        match value {
            0 => Ok(Self::Accepted),
            1 => Ok(Self::InsufficientBalance),
            2 => Ok(Self::CapExceeded),
            3 => Ok(Self::StateVersionMismatch),
            _ => Err(PersistenceError::CorruptStoredAsh),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AshMutationRequest {
    pub account_id: [u8; 16],
    pub mutation_id: [u8; 16],
    pub payload_hash: [u8; 32],
    pub expected_wallet_version: i64,
    pub kind: AshMutationKind,
    pub amount: i32,
    pub reason_code: String,
    pub source_id: String,
    pub content_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAshMutationResult {
    pub mutation_id: [u8; 16],
    pub payload_hash: [u8; 32],
    pub expected_wallet_version: i64,
    pub kind: AshMutationKind,
    pub amount: i32,
    pub code: AshMutationCode,
    pub before_balance: i32,
    pub after_balance: i32,
    pub pre_wallet_version: i64,
    pub post_wallet_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AshWalletTransaction {
    Committed(StoredAshMutationResult),
    Replayed(StoredAshMutationResult),
}

impl AshWalletTransaction {
    #[must_use]
    pub const fn result(&self) -> &StoredAshMutationResult {
        match self {
            Self::Committed(result) | Self::Replayed(result) => result,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredAshWallet {
    pub balance: i32,
    pub wallet_version: i64,
}

impl PostgresPersistence {
    pub async fn ash_wallet_snapshot(
        &self,
        account_id: [u8; 16],
    ) -> Result<Option<StoredAshWallet>, PersistenceError> {
        if account_id == [0; 16] {
            return Err(PersistenceError::CorruptStoredAsh);
        }
        let row = sqlx::query(
            "SELECT balance, wallet_version FROM ash_wallets \
             WHERE namespace_id = $1 AND account_id = $2",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| decode_wallet(&row)).transpose()
    }

    pub async fn transact_ash_mutation(
        &self,
        request: &AshMutationRequest,
    ) -> Result<AshWalletTransaction, PersistenceError> {
        validate_request(request)?;
        let mut transaction = self.begin_transaction().await?;
        let account_exists = sqlx::query_scalar::<_, i32>(
            "SELECT 1 FROM accounts WHERE namespace_id = $1 AND account_id = $2 FOR UPDATE",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .fetch_optional(transaction.connection())
        .await?
        .is_some();
        if !account_exists {
            transaction.rollback().await?;
            return Err(PersistenceError::AshAccountNotFound);
        }
        if let Some(result) = load_result(transaction.connection(), request).await? {
            transaction.rollback().await?;
            if result.payload_hash != request.payload_hash {
                return Err(PersistenceError::AshIdempotencyConflict);
            }
            return Ok(AshWalletTransaction::Replayed(result));
        }
        sqlx::query(
            "INSERT INTO ash_wallets (namespace_id, account_id) VALUES ($1, $2) \
             ON CONFLICT DO NOTHING",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .execute(transaction.connection())
        .await?;
        let row = sqlx::query(
            "SELECT balance, wallet_version FROM ash_wallets \
             WHERE namespace_id = $1 AND account_id = $2 FOR UPDATE",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .fetch_one(transaction.connection())
        .await?;
        let wallet = decode_wallet(&row)?;
        let result = resolve(request, wallet)?;
        insert_result(transaction.connection(), request, &result).await?;
        if result.code == AshMutationCode::Accepted {
            sqlx::query(
                "UPDATE ash_wallets SET balance = $1, wallet_version = $2, \
                 updated_at = transaction_timestamp() WHERE namespace_id = $3 AND account_id = $4",
            )
            .bind(result.after_balance)
            .bind(result.post_wallet_version)
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(request.account_id.as_slice())
            .execute(transaction.connection())
            .await?;
            insert_ledger_event(transaction.connection(), request, &result).await?;
        }
        transaction.commit().await?;
        Ok(AshWalletTransaction::Committed(result))
    }
}

fn validate_request(request: &AshMutationRequest) -> Result<(), PersistenceError> {
    if request.account_id == [0; 16]
        || request.mutation_id == [0; 16]
        || request.payload_hash == [0; 32]
        || request.expected_wallet_version <= 0
        || !(1..=ASH_WALLET_CAP).contains(&request.amount)
        || !bounded(&request.reason_code, 64)
        || !bounded(&request.source_id, 128)
        || !bounded(&request.content_version, 128)
    {
        return Err(PersistenceError::CorruptStoredAsh);
    }
    Ok(())
}

fn bounded(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.chars().count() <= maximum
}

fn decode_wallet(row: &sqlx::postgres::PgRow) -> Result<StoredAshWallet, PersistenceError> {
    let wallet = StoredAshWallet {
        balance: row.try_get("balance")?,
        wallet_version: row.try_get("wallet_version")?,
    };
    if !(0..=ASH_WALLET_CAP).contains(&wallet.balance) || wallet.wallet_version <= 0 {
        return Err(PersistenceError::CorruptStoredAsh);
    }
    Ok(wallet)
}

fn resolve(
    request: &AshMutationRequest,
    wallet: StoredAshWallet,
) -> Result<StoredAshMutationResult, PersistenceError> {
    let candidate = match request.kind {
        AshMutationKind::Earn => wallet.balance.checked_add(request.amount),
        AshMutationKind::Spend => wallet.balance.checked_sub(request.amount),
    };
    let (code, after_balance) = if request.expected_wallet_version == wallet.wallet_version {
        match (request.kind, candidate) {
            (AshMutationKind::Earn, Some(value)) if value <= ASH_WALLET_CAP => {
                (AshMutationCode::Accepted, value)
            }
            (AshMutationKind::Earn, _) => (AshMutationCode::CapExceeded, wallet.balance),
            (AshMutationKind::Spend, Some(value)) if value >= 0 => {
                (AshMutationCode::Accepted, value)
            }
            (AshMutationKind::Spend, _) => (AshMutationCode::InsufficientBalance, wallet.balance),
        }
    } else {
        (AshMutationCode::StateVersionMismatch, wallet.balance)
    };
    let post_wallet_version = if code == AshMutationCode::Accepted {
        wallet
            .wallet_version
            .checked_add(1)
            .ok_or(PersistenceError::CorruptStoredAsh)?
    } else {
        wallet.wallet_version
    };
    Ok(StoredAshMutationResult {
        mutation_id: request.mutation_id,
        payload_hash: request.payload_hash,
        expected_wallet_version: request.expected_wallet_version,
        kind: request.kind,
        amount: request.amount,
        code,
        before_balance: wallet.balance,
        after_balance,
        pre_wallet_version: wallet.wallet_version,
        post_wallet_version,
    })
}

async fn load_result(
    connection: &mut sqlx::PgConnection,
    request: &AshMutationRequest,
) -> Result<Option<StoredAshMutationResult>, PersistenceError> {
    let row = sqlx::query(
        "SELECT mutation_id, payload_hash, expected_wallet_version, mutation_kind, \
                requested_amount, result_code, \
                before_balance, after_balance, pre_wallet_version, post_wallet_version \
         FROM ash_mutation_results WHERE namespace_id = $1 AND account_id = $2 \
         AND mutation_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .fetch_optional(connection)
    .await?;
    row.map(|row| decode_result(&row)).transpose()
}

fn decode_result(row: &sqlx::postgres::PgRow) -> Result<StoredAshMutationResult, PersistenceError> {
    let kind = match row.try_get::<i16, _>("mutation_kind")? {
        0 => AshMutationKind::Earn,
        1 => AshMutationKind::Spend,
        _ => return Err(PersistenceError::CorruptStoredAsh),
    };
    let result = StoredAshMutationResult {
        mutation_id: fixed_bytes(row.try_get("mutation_id")?)?,
        payload_hash: fixed_bytes(row.try_get("payload_hash")?)?,
        expected_wallet_version: row.try_get("expected_wallet_version")?,
        kind,
        amount: row.try_get("requested_amount")?,
        code: AshMutationCode::from_code(row.try_get("result_code")?)?,
        before_balance: row.try_get("before_balance")?,
        after_balance: row.try_get("after_balance")?,
        pre_wallet_version: row.try_get("pre_wallet_version")?,
        post_wallet_version: row.try_get("post_wallet_version")?,
    };
    validate_stored_result(&result)?;
    Ok(result)
}

fn validate_stored_result(result: &StoredAshMutationResult) -> Result<(), PersistenceError> {
    let accepted = result.code == AshMutationCode::Accepted;
    let expected_after = match result.kind {
        AshMutationKind::Earn => result.before_balance.checked_add(result.amount),
        AshMutationKind::Spend => result.before_balance.checked_sub(result.amount),
    };
    if result.mutation_id == [0; 16]
        || result.payload_hash == [0; 32]
        || result.expected_wallet_version <= 0
        || !(1..=ASH_WALLET_CAP).contains(&result.amount)
        || !(0..=ASH_WALLET_CAP).contains(&result.before_balance)
        || !(0..=ASH_WALLET_CAP).contains(&result.after_balance)
        || result.pre_wallet_version <= 0
        || (accepted && expected_after != Some(result.after_balance))
        || (!accepted && result.after_balance != result.before_balance)
        || (result.code == AshMutationCode::InsufficientBalance
            && result.kind != AshMutationKind::Spend)
        || (result.code == AshMutationCode::CapExceeded && result.kind != AshMutationKind::Earn)
        || (result.code == AshMutationCode::StateVersionMismatch
            && result.expected_wallet_version == result.pre_wallet_version)
        || (result.code != AshMutationCode::StateVersionMismatch
            && result.expected_wallet_version != result.pre_wallet_version)
        || result.post_wallet_version
            != if accepted {
                result.pre_wallet_version.checked_add(1).unwrap_or(0)
            } else {
                result.pre_wallet_version
            }
    {
        return Err(PersistenceError::CorruptStoredAsh);
    }
    Ok(())
}

async fn insert_result(
    connection: &mut sqlx::PgConnection,
    request: &AshMutationRequest,
    result: &StoredAshMutationResult,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO ash_mutation_results \
         (namespace_id, account_id, mutation_id, payload_hash, expected_wallet_version, \
          mutation_kind, reason_code, \
          source_id, content_version, requested_amount, result_code, before_balance, \
          after_balance, pre_wallet_version, post_wallet_version) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(request.payload_hash.as_slice())
    .bind(request.expected_wallet_version)
    .bind(request.kind.code())
    .bind(&request.reason_code)
    .bind(&request.source_id)
    .bind(&request.content_version)
    .bind(request.amount)
    .bind(result.code.code())
    .bind(result.before_balance)
    .bind(result.after_balance)
    .bind(result.pre_wallet_version)
    .bind(result.post_wallet_version)
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_ledger_event(
    connection: &mut sqlx::PgConnection,
    request: &AshMutationRequest,
    result: &StoredAshMutationResult,
) -> Result<(), PersistenceError> {
    let delta = match request.kind {
        AshMutationKind::Earn => request.amount,
        AshMutationKind::Spend => -request.amount,
    };
    sqlx::query(
        "INSERT INTO currency_ledger_events \
         (namespace_id, account_id, event_id, mutation_id, currency_id, reason_code, source_id, \
          content_version, before_balance, delta, after_balance, wallet_version) \
         VALUES ($1, $2, $3, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(ASH_CURRENCY_ID)
    .bind(&request.reason_code)
    .bind(&request.source_id)
    .bind(&request.content_version)
    .bind(result.before_balance)
    .bind(delta)
    .bind(result.after_balance)
    .bind(result.post_wallet_version)
    .execute(connection)
    .await?;
    Ok(())
}

fn fixed_bytes<const N: usize>(bytes: Vec<u8>) -> Result<[u8; N], PersistenceError> {
    <[u8; N]>::try_from(bytes).map_err(|_| PersistenceError::CorruptStoredAsh)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(
        kind: AshMutationKind,
        amount: i32,
        expected_wallet_version: i64,
    ) -> AshMutationRequest {
        AshMutationRequest {
            account_id: [1; 16],
            mutation_id: [2; 16],
            payload_hash: [3; 32],
            expected_wallet_version,
            kind,
            amount,
            reason_code: "test_reason".into(),
            source_id: "test.source".into(),
            content_version: "core-dev.test".into(),
        }
    }

    #[test]
    fn earn_spend_cap_and_insufficient_results_are_exact() {
        let earned = resolve(
            &request(AshMutationKind::Earn, 40, 2),
            StoredAshWallet {
                balance: 10,
                wallet_version: 2,
            },
        )
        .unwrap();
        assert_eq!(earned.code, AshMutationCode::Accepted);
        assert_eq!((earned.before_balance, earned.after_balance), (10, 50));
        assert_eq!(
            (earned.pre_wallet_version, earned.post_wallet_version),
            (2, 3)
        );
        let spent = resolve(
            &request(AshMutationKind::Spend, 40, 3),
            StoredAshWallet {
                balance: 50,
                wallet_version: 3,
            },
        )
        .unwrap();
        assert_eq!(spent.code, AshMutationCode::Accepted);
        assert_eq!(spent.after_balance, 10);
        let capped = resolve(
            &request(AshMutationKind::Earn, 1, 4),
            StoredAshWallet {
                balance: ASH_WALLET_CAP,
                wallet_version: 4,
            },
        )
        .unwrap();
        assert_eq!(capped.code, AshMutationCode::CapExceeded);
        assert_eq!(capped.post_wallet_version, 4);
        let insufficient = resolve(
            &request(AshMutationKind::Spend, 11, 4),
            StoredAshWallet {
                balance: 10,
                wallet_version: 4,
            },
        )
        .unwrap();
        assert_eq!(insufficient.code, AshMutationCode::InsufficientBalance);
        assert_eq!(insufficient.after_balance, 10);
        let stale = resolve(
            &request(AshMutationKind::Earn, 10, 3),
            StoredAshWallet {
                balance: 10,
                wallet_version: 4,
            },
        )
        .unwrap();
        assert_eq!(stale.code, AshMutationCode::StateVersionMismatch);
        assert_eq!(stale.after_balance, 10);
        assert_eq!(stale.post_wallet_version, 4);
    }

    #[test]
    fn malformed_requests_and_stored_results_fail_closed() {
        let mut malformed = request(AshMutationKind::Earn, 1, 1);
        malformed.payload_hash = [0; 32];
        assert!(matches!(
            validate_request(&malformed),
            Err(PersistenceError::CorruptStoredAsh)
        ));
        let mut result = resolve(
            &request(AshMutationKind::Earn, 5, 1),
            StoredAshWallet {
                balance: 1,
                wallet_version: 1,
            },
        )
        .unwrap();
        result.after_balance = 7;
        assert!(matches!(
            validate_stored_result(&result),
            Err(PersistenceError::CorruptStoredAsh)
        ));
    }
}
