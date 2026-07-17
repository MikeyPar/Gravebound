//! Restart-stable actor-generation allocation for the ordinary Core private-life route.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` `TECH-010`-`023`;
//! `Gravebound_Content_Production_Spec_v1.md` `CONT-WORLD-001`, `CONT-ROOM-007`, and
//! `CONT-HUB-001`-`002`; `Gravebound_Development_Roadmap_v1.md` `GB-M03-03`; and accepted
//! `ADR-037`.
//!
//! The selected living character is locked before one strictly increasing generation head and
//! immutable allocation audit are committed. A lost response may create a gap; retry allocates a
//! newer generation and can never recreate stale client authority.

use sqlx::{PgConnection, Row};

use crate::{
    PersistenceError, PostgresPersistence, WIPEABLE_CORE_NAMESPACE,
    is_retryable_transaction_failure,
};

const MAX_TRANSACTION_ATTEMPTS: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredPrivateRouteGenerationV1 {
    pub account_id: [u8; 16],
    pub character_id: [u8; 16],
    pub actor_generation: u64,
}

impl PostgresPersistence {
    /// Allocates one generation for an authenticated selected living character. This operation is
    /// intentionally not idempotent: ambiguity is resolved by allocating a newer value, leaving a
    /// harmless audit gap instead of risking ABA reuse.
    pub async fn allocate_private_route_generation_v1(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> Result<StoredPrivateRouteGenerationV1, PersistenceError> {
        if account_id.iter().all(|byte| *byte == 0) || character_id.iter().all(|byte| *byte == 0) {
            return Err(PersistenceError::CorruptStoredPrivateRouteGeneration);
        }
        for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self
                .allocate_private_route_generation_once_v1(account_id, character_id)
                .await
            {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS
                        && is_retryable_transaction_failure(&error) => {}
                result => return result,
            }
        }
        unreachable!("bounded private-route generation transaction always returns")
    }

    async fn allocate_private_route_generation_once_v1(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> Result<StoredPrivateRouteGenerationV1, PersistenceError> {
        let mut transaction = self.begin_transaction().await?;
        lock_selected_living_character(transaction.connection(), account_id, character_id).await?;
        let generation: Option<i64> = sqlx::query_scalar(
            "INSERT INTO character_private_route_generation_heads_v1 \
             (namespace_id, account_id, character_id, last_generation) \
             VALUES ($1, $2, $3, 1) \
             ON CONFLICT (namespace_id, account_id, character_id) DO UPDATE \
             SET last_generation = \
                 character_private_route_generation_heads_v1.last_generation + 1, \
                 updated_at = transaction_timestamp() \
             WHERE character_private_route_generation_heads_v1.last_generation \
                 < 9223372036854775807 \
             RETURNING last_generation",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .fetch_optional(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?;
        let generation = generation.ok_or(PersistenceError::PrivateRouteGenerationExhausted)?;
        sqlx::query(
            "INSERT INTO private_route_generation_allocations_v1 \
             (namespace_id, account_id, character_id, actor_generation) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .bind(generation)
        .execute(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?;
        transaction.commit().await?;
        let actor_generation = u64::try_from(generation)
            .map_err(|_| PersistenceError::CorruptStoredPrivateRouteGeneration)?;
        Ok(StoredPrivateRouteGenerationV1 {
            account_id,
            character_id,
            actor_generation,
        })
    }
}

async fn lock_selected_living_character(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<(), PersistenceError> {
    let row = sqlx::query(
        "SELECT account.selected_character_id, character.life_state, character.security_state \
         FROM accounts AS account \
         JOIN characters AS character \
           ON character.namespace_id = account.namespace_id \
          AND character.account_id = account.account_id \
          AND character.character_id = $3 \
         WHERE account.namespace_id = $1 AND account.account_id = $2 \
         FOR UPDATE OF account, character",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?
    .ok_or(PersistenceError::PrivateRouteCharacterUnavailable)?;
    let selected: Option<Vec<u8>> = row
        .try_get("selected_character_id")
        .map_err(PersistenceError::Database)?;
    let life_state: i16 = row
        .try_get("life_state")
        .map_err(PersistenceError::Database)?;
    let security_state: i16 = row
        .try_get("security_state")
        .map_err(PersistenceError::Database)?;
    if selected.as_deref() != Some(character_id.as_slice()) {
        return Err(PersistenceError::PrivateRouteCharacterUnavailable);
    }
    if life_state != 0 {
        return Err(PersistenceError::PrivateRouteCharacterDead);
    }
    if security_state != 0 {
        return Err(PersistenceError::CorruptStoredPrivateRouteGeneration);
    }
    Ok(())
}
