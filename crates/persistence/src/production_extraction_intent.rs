//! Durable acceptance and conflict audit for one production extraction intent.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` `DTH-011`, `LOOT-002`,
//! `LOOT-060`, and `TECH-015`/`021`-`023`; `Gravebound_Content_Production_Spec_v1.md`
//! `CONT-HUB-001`/`002`, the Core Bell Sepulcher/Caldus route, and `CONT-VALID-001`;
//! `Gravebound_Development_Roadmap_v1.md` `GB-M03-03`/`08`; and accepted
//! `SPEC-CONFLICT-029-m03-extraction-recall-terminal-authority.md`.
//!
//! The live route accepts a client frame exactly once before terminal planning. This boundary
//! persists the complete server-bound attempt, including the broad route revision and narrower
//! world-flow revision, so a process restart cannot forget an altered replay. Reliable transport
//! sequence is intentionally absent: it is delivery metadata and may change on an exact retry.

use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, Row};

use crate::{
    PersistenceError, PostgresPersistence, ProductionExtractionCommitRequestV1,
    StoredWorldFlowRevisionV1, WIPEABLE_CORE_NAMESPACE, is_retryable_transaction_failure,
};

pub const PRODUCTION_EXTRACTION_INTENT_CONTRACT_VERSION_V1: u16 = 1;
pub const PRODUCTION_EXTRACTION_INTENT_FRAME_SCHEMA_VERSION_V1: u16 = 1;

const ID_BYTES: usize = 16;
const HASH_BYTES: usize = 32;
const MAX_ATTEMPT_PAYLOAD_BYTES: usize = 65_536;
const MAX_TRANSACTION_ATTEMPTS: u8 = 8;
const INTENT_HASH_CONTEXT: &str = "gravebound.production-extraction-intent-attempt.v1";
const CONFLICT_ID_CONTEXT: &str = "gravebound.production-extraction-intent-conflict.v1";

/// Broad content identity for the complete ordinary Core route.
///
/// This is intentionally a different type from [`StoredWorldFlowRevisionV1`]. The broad revision
/// includes Hall, micro-realm, Bell Sepulcher rooms/encounters, and Sir Caldus in addition to the
/// world-flow inputs represented by the narrower revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionExtractionCoreRouteRevisionV1 {
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
}

/// Canonical server-bound material for the first accepted extraction frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionExtractionIntentAttemptV1 {
    pub contract_version: u16,
    pub namespace_id: String,
    pub authenticated_account_id: [u8; ID_BYTES],
    pub attempted_character_id: [u8; ID_BYTES],
    pub attempted_mutation_id: [u8; ID_BYTES],
    pub attempted_frame_schema_version: u16,
    pub attempted_frame_payload_hash: [u8; HASH_BYTES],
    pub extraction_request_id: [u8; ID_BYTES],
    pub extraction_receipt_id: [u8; ID_BYTES],
    pub terminal_id: [u8; ID_BYTES],
    pub actor_generation: u64,
    pub accepted_pre_route_state_version: u64,
    pub accepted_post_route_state_version: u64,
    pub core_route_revision: ProductionExtractionCoreRouteRevisionV1,
    pub world_flow_revision: StoredWorldFlowRevisionV1,
    pub commit_request: ProductionExtractionCommitRequestV1,
    pub issued_at_unix_ms: u64,
    pub observed_tick: u64,
}

impl ProductionExtractionIntentAttemptV1 {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        let corrupt = || PersistenceError::CorruptStoredProductionExtractionIntent;
        self.commit_request.validate().map_err(|_| corrupt())?;
        if self.contract_version != PRODUCTION_EXTRACTION_INTENT_CONTRACT_VERSION_V1
            || self.namespace_id != WIPEABLE_CORE_NAMESPACE
            || self.authenticated_account_id == [0; ID_BYTES]
            || self.attempted_character_id == [0; ID_BYTES]
            || self.attempted_mutation_id == [0; ID_BYTES]
            || self.attempted_frame_schema_version
                != PRODUCTION_EXTRACTION_INTENT_FRAME_SCHEMA_VERSION_V1
            || self.attempted_frame_payload_hash == [0; HASH_BYTES]
            || self.extraction_request_id == [0; ID_BYTES]
            || self.extraction_receipt_id == [0; ID_BYTES]
            || self.terminal_id == [0; ID_BYTES]
            || self.actor_generation == 0
            || self.actor_generation > i64::MAX as u64
            || self.accepted_pre_route_state_version == 0
            || self.accepted_pre_route_state_version > i64::MAX as u64
            || self.accepted_post_route_state_version
                != self
                    .accepted_pre_route_state_version
                    .checked_add(1)
                    .ok_or_else(corrupt)?
            || self.accepted_post_route_state_version > i64::MAX as u64
            || self.issued_at_unix_ms == 0
            || self.issued_at_unix_ms > i64::MAX as u64
            || self.observed_tick == 0
            || self.observed_tick > i64::MAX as u64
            || !valid_revision(
                &self.core_route_revision.records_blake3,
                &self.core_route_revision.assets_blake3,
                &self.core_route_revision.localization_blake3,
            )
            || !valid_revision(
                &self.world_flow_revision.records_blake3,
                &self.world_flow_revision.assets_blake3,
                &self.world_flow_revision.localization_blake3,
            )
            || self.authenticated_account_id != self.commit_request.account_id
            || self.attempted_character_id != self.commit_request.character_id
            || self.attempted_mutation_id != self.commit_request.mutation_id
            || self.extraction_request_id != self.commit_request.extraction_request_id
            || self.extraction_receipt_id != self.commit_request.extraction_receipt_id
            || self.terminal_id != self.commit_request.terminal_id
            || self.world_flow_revision != self.commit_request.content_revision
            || self.issued_at_unix_ms != self.commit_request.issued_at_unix_ms
            || self.observed_tick != self.commit_request.observed_tick
            || self.attempted_frame_payload_hash
                != canonical_production_extraction_frame_payload_hash_v1(&self.commit_request)?
        {
            return Err(corrupt());
        }
        Ok(())
    }

    pub fn canonical_hash(&self) -> Result<[u8; HASH_BYTES], PersistenceError> {
        self.validate()?;
        canonical_hash(INTENT_HASH_CONTEXT, self)
    }

    fn encode(&self) -> Result<Vec<u8>, PersistenceError> {
        self.validate()?;
        let payload = postcard::to_stdvec(self)
            .map_err(|_| PersistenceError::CorruptStoredProductionExtractionIntent)?;
        if payload.is_empty() || payload.len() > MAX_ATTEMPT_PAYLOAD_BYTES {
            return Err(PersistenceError::CorruptStoredProductionExtractionIntent);
        }
        Ok(payload)
    }

    fn decode(payload: &[u8]) -> Result<Self, PersistenceError> {
        if payload.is_empty() || payload.len() > MAX_ATTEMPT_PAYLOAD_BYTES {
            return Err(PersistenceError::CorruptStoredProductionExtractionIntent);
        }
        let attempt: Self = postcard::from_bytes(payload)
            .map_err(|_| PersistenceError::CorruptStoredProductionExtractionIntent)?;
        attempt.validate()?;
        Ok(attempt)
    }
}

/// Immutable first acceptance returned on fresh commit and exact replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProductionExtractionIntentAcceptanceV1 {
    pub attempt: ProductionExtractionIntentAttemptV1,
    pub canonical_attempt_hash: [u8; HASH_BYTES],
    pub commit_request_hash: [u8; HASH_BYTES],
    pub accepted_at_unix_ms: u64,
}

impl StoredProductionExtractionIntentAcceptanceV1 {
    fn validate(&self) -> Result<(), PersistenceError> {
        self.attempt.validate()?;
        if self.canonical_attempt_hash == [0; HASH_BYTES]
            || self.commit_request_hash == [0; HASH_BYTES]
            || self.canonical_attempt_hash != self.attempt.canonical_hash()?
            || self.commit_request_hash != self.attempt.commit_request.canonical_hash()?
            || self.accepted_at_unix_ms < self.attempt.issued_at_unix_ms
        {
            return Err(PersistenceError::CorruptStoredProductionExtractionIntent);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProductionExtractionIntentAcceptanceTransactionV1 {
    Fresh(StoredProductionExtractionIntentAcceptanceV1),
    Replayed(StoredProductionExtractionIntentAcceptanceV1),
    Conflict {
        extraction_request_id: [u8; ID_BYTES],
        conflict_audit_id: [u8; ID_BYTES],
        stored_attempt_hash: [u8; HASH_BYTES],
        attempted_attempt_hash: [u8; HASH_BYTES],
    },
}

impl ProductionExtractionIntentAcceptanceTransactionV1 {
    #[must_use]
    pub const fn acceptance(&self) -> Option<&StoredProductionExtractionIntentAcceptanceV1> {
        match self {
            Self::Fresh(acceptance) | Self::Replayed(acceptance) => Some(acceptance),
            Self::Conflict { .. } => None,
        }
    }

    #[must_use]
    pub const fn is_replay(&self) -> bool {
        matches!(self, Self::Replayed(_))
    }
}

#[derive(Serialize)]
struct CanonicalFrameExpectedVersionsV1 {
    account: u64,
    character: u64,
    world: u64,
    inventory: u64,
    life_clock: u64,
}

#[derive(Serialize)]
struct CanonicalExtractionFramePayloadV1<'a> {
    extraction_request_id: [u8; ID_BYTES],
    expected_versions: CanonicalFrameExpectedVersionsV1,
    content_revision: &'a StoredWorldFlowRevisionV1,
}

/// Reconstructs the protocol-v1 extraction payload hash without creating a persistence-to-wire
/// dependency. Field order and primitive encodings are the append-only protocol contract.
pub fn canonical_production_extraction_frame_payload_hash_v1(
    request: &ProductionExtractionCommitRequestV1,
) -> Result<[u8; HASH_BYTES], PersistenceError> {
    request
        .validate()
        .map_err(|_| PersistenceError::CorruptStoredProductionExtractionIntent)?;
    let payload = CanonicalExtractionFramePayloadV1 {
        extraction_request_id: request.extraction_request_id,
        expected_versions: CanonicalFrameExpectedVersionsV1 {
            account: request.expected_versions.account,
            character: request.expected_versions.character,
            world: request.expected_versions.world,
            inventory: request.expected_versions.inventory,
            life_clock: request.expected_versions.life_metrics,
        },
        content_revision: &request.content_revision,
    };
    let encoded = postcard::to_stdvec(&payload)
        .map_err(|_| PersistenceError::CorruptStoredProductionExtractionIntent)?;
    if encoded.is_empty() || encoded.len() > MAX_ATTEMPT_PAYLOAD_BYTES {
        return Err(PersistenceError::CorruptStoredProductionExtractionIntent);
    }
    Ok(*blake3::hash(&encoded).as_bytes())
}

impl PostgresPersistence {
    /// Accepts the first canonical frame material for one extraction request.
    ///
    /// Existing acceptance is inspected before current actor authority: an exact retry survives
    /// process replacement, while changed material durably commits its conflict audit before this
    /// method returns [`ProductionExtractionIntentAcceptanceTransactionV1::Conflict`].
    pub async fn accept_production_extraction_intent_v1(
        &self,
        attempt: &ProductionExtractionIntentAttemptV1,
    ) -> Result<ProductionExtractionIntentAcceptanceTransactionV1, PersistenceError> {
        attempt.validate()?;
        for transaction_attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self
                .accept_production_extraction_intent_once_v1(attempt)
                .await
            {
                Err(error)
                    if transaction_attempt < MAX_TRANSACTION_ATTEMPTS
                        && is_retryable_transaction_failure(&error) => {}
                result => return result,
            }
        }
        unreachable!("bounded extraction-intent acceptance always returns")
    }

    async fn accept_production_extraction_intent_once_v1(
        &self,
        attempt: &ProductionExtractionIntentAttemptV1,
    ) -> Result<ProductionExtractionIntentAcceptanceTransactionV1, PersistenceError> {
        let attempted_hash = attempt.canonical_hash()?;
        let attempted_payload = attempt.encode()?;
        let attempted_commit_hash = attempt.commit_request.canonical_hash()?;
        let mut transaction = self.begin_transaction().await?;
        lock_extraction_request(transaction.connection(), attempt.extraction_request_id).await?;

        if let Some(stored) = load_acceptance(
            transaction.connection(),
            &attempt.namespace_id,
            attempt.extraction_request_id,
        )
        .await?
        {
            if stored.canonical_attempt_hash == attempted_hash {
                if stored.attempt != *attempt {
                    transaction.rollback().await?;
                    return Err(PersistenceError::CorruptStoredProductionExtractionIntent);
                }
                transaction.rollback().await?;
                return Ok(ProductionExtractionIntentAcceptanceTransactionV1::Replayed(
                    stored,
                ));
            }
            let conflict_audit_id = derive_conflict_audit_id(
                attempt.extraction_request_id,
                stored.canonical_attempt_hash,
                attempted_hash,
            );
            insert_conflict_audit(
                transaction.connection(),
                &stored,
                attempt,
                attempted_hash,
                attempted_commit_hash,
                &attempted_payload,
                conflict_audit_id,
            )
            .await?;
            transaction.commit().await?;
            return Ok(
                ProductionExtractionIntentAcceptanceTransactionV1::Conflict {
                    extraction_request_id: attempt.extraction_request_id,
                    conflict_audit_id,
                    stored_attempt_hash: stored.canonical_attempt_hash,
                    attempted_attempt_hash: attempted_hash,
                },
            );
        }

        lock_current_actor_authority(transaction.connection(), attempt).await?;
        let accepted_at_unix_ms = transaction_time_unix_ms(transaction.connection()).await?;
        if accepted_at_unix_ms < attempt.issued_at_unix_ms {
            transaction.rollback().await?;
            return Err(PersistenceError::CorruptStoredProductionExtractionIntent);
        }
        insert_acceptance(
            transaction.connection(),
            attempt,
            attempted_hash,
            attempted_commit_hash,
            &attempted_payload,
        )
        .await?;
        let stored = StoredProductionExtractionIntentAcceptanceV1 {
            attempt: attempt.clone(),
            canonical_attempt_hash: attempted_hash,
            commit_request_hash: attempted_commit_hash,
            accepted_at_unix_ms,
        };
        stored.validate()?;
        transaction.commit().await?;
        Ok(ProductionExtractionIntentAcceptanceTransactionV1::Fresh(
            stored,
        ))
    }

    /// Read-only recovery for ambiguous transport outcomes and support diagnostics.
    pub async fn load_production_extraction_intent_acceptance_v1(
        &self,
        extraction_request_id: [u8; ID_BYTES],
    ) -> Result<Option<StoredProductionExtractionIntentAcceptanceV1>, PersistenceError> {
        if extraction_request_id == [0; ID_BYTES] {
            return Err(PersistenceError::CorruptStoredProductionExtractionIntent);
        }
        let mut transaction = self.begin_transaction().await?;
        let stored = load_acceptance(
            transaction.connection(),
            WIPEABLE_CORE_NAMESPACE,
            extraction_request_id,
        )
        .await?;
        transaction.rollback().await?;
        Ok(stored)
    }
}

async fn lock_extraction_request(
    connection: &mut PgConnection,
    extraction_request_id: [u8; ID_BYTES],
) -> Result<(), PersistenceError> {
    let mut hasher =
        blake3::Hasher::new_derive_key("gravebound.production-extraction-intent-advisory-lock.v1");
    hasher.update(WIPEABLE_CORE_NAMESPACE.as_bytes());
    hasher.update(&extraction_request_id);
    let digest = hasher.finalize();
    let lock_key = i64::from_be_bytes(
        digest.as_bytes()[..8]
            .try_into()
            .expect("BLAKE3 digest contains eight lock bytes"),
    );
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(lock_key)
        .execute(connection)
        .await
        .map_err(PersistenceError::Database)?;
    Ok(())
}

async fn lock_current_actor_authority(
    connection: &mut PgConnection,
    attempt: &ProductionExtractionIntentAttemptV1,
) -> Result<(), PersistenceError> {
    let row = sqlx::query(
        "SELECT account.selected_character_id, character.life_state,
                character.security_state, generation.last_generation
         FROM accounts AS account
         JOIN characters AS character
           ON character.namespace_id=account.namespace_id
          AND character.account_id=account.account_id
          AND character.character_id=$3
         JOIN character_private_route_generation_heads_v1 AS generation
           ON generation.namespace_id=character.namespace_id
          AND generation.account_id=character.account_id
          AND generation.character_id=character.character_id
         WHERE account.namespace_id=$1 AND account.account_id=$2
         FOR UPDATE OF account, character, generation",
    )
    .bind(&attempt.namespace_id)
    .bind(attempt.authenticated_account_id.as_slice())
    .bind(attempt.attempted_character_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?
    .ok_or(PersistenceError::ProductionExtractionIntentAuthorityMismatch)?;
    let selected_character_id: Option<Vec<u8>> = row.try_get("selected_character_id")?;
    let life_state: i16 = row.try_get("life_state")?;
    let security_state: i16 = row.try_get("security_state")?;
    let generation: i64 = row.try_get("last_generation")?;
    if selected_character_id.as_deref() != Some(attempt.attempted_character_id.as_slice())
        || life_state != 0
        || security_state != 0
        || generation != u64_to_i64(attempt.actor_generation)?
    {
        return Err(PersistenceError::ProductionExtractionIntentAuthorityMismatch);
    }
    Ok(())
}

async fn load_acceptance(
    connection: &mut PgConnection,
    namespace_id: &str,
    extraction_request_id: [u8; ID_BYTES],
) -> Result<Option<StoredProductionExtractionIntentAcceptanceV1>, PersistenceError> {
    let row = sqlx::query(
        "SELECT authenticated_account_id, attempted_character_id, attempted_mutation_id,
                contract_version, frame_schema_version, frame_payload_hash,
                extraction_receipt_id, terminal_id, actor_generation,
                accepted_pre_route_state_version, accepted_post_route_state_version,
                route_records_blake3, route_assets_blake3, route_localization_blake3,
                world_records_blake3, world_assets_blake3, world_localization_blake3,
                canonical_attempt_hash, commit_request_hash, attempt_payload,
                floor(extract(epoch FROM issued_at)*1000)::bigint AS issued_at_unix_ms,
                observed_tick,
                floor(extract(epoch FROM accepted_at)*1000)::bigint AS accepted_at_unix_ms
         FROM production_extraction_intent_acceptances_v1
         WHERE namespace_id=$1 AND extraction_request_id=$2
         FOR SHARE",
    )
    .bind(namespace_id)
    .bind(extraction_request_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let attempt_payload: Vec<u8> = row.try_get("attempt_payload")?;
    let attempt = ProductionExtractionIntentAttemptV1::decode(&attempt_payload)?;
    let stored = StoredProductionExtractionIntentAcceptanceV1 {
        attempt,
        canonical_attempt_hash: exact_hash(row.try_get("canonical_attempt_hash")?)?,
        commit_request_hash: exact_hash(row.try_get("commit_request_hash")?)?,
        accepted_at_unix_ms: i64_to_u64(row.try_get("accepted_at_unix_ms")?)?,
    };
    stored.validate()?;
    if stored.attempt.namespace_id != namespace_id
        || stored.attempt.extraction_request_id != extraction_request_id
        || stored.attempt.authenticated_account_id
            != exact_id(row.try_get("authenticated_account_id")?)?
        || stored.attempt.attempted_character_id
            != exact_id(row.try_get("attempted_character_id")?)?
        || stored.attempt.attempted_mutation_id != exact_id(row.try_get("attempted_mutation_id")?)?
        || stored.attempt.contract_version
            != u16::try_from(row.try_get::<i16, _>("contract_version")?)
                .map_err(|_| PersistenceError::CorruptStoredProductionExtractionIntent)?
        || stored.attempt.attempted_frame_schema_version
            != u16::try_from(row.try_get::<i16, _>("frame_schema_version")?)
                .map_err(|_| PersistenceError::CorruptStoredProductionExtractionIntent)?
        || stored.attempt.attempted_frame_payload_hash
            != exact_hash(row.try_get("frame_payload_hash")?)?
        || stored.attempt.extraction_receipt_id != exact_id(row.try_get("extraction_receipt_id")?)?
        || stored.attempt.terminal_id != exact_id(row.try_get("terminal_id")?)?
        || stored.attempt.actor_generation != i64_to_u64(row.try_get("actor_generation")?)?
        || stored.attempt.accepted_pre_route_state_version
            != i64_to_u64(row.try_get("accepted_pre_route_state_version")?)?
        || stored.attempt.accepted_post_route_state_version
            != i64_to_u64(row.try_get("accepted_post_route_state_version")?)?
        || stored.attempt.core_route_revision.records_blake3
            != row.try_get::<String, _>("route_records_blake3")?
        || stored.attempt.core_route_revision.assets_blake3
            != row.try_get::<String, _>("route_assets_blake3")?
        || stored.attempt.core_route_revision.localization_blake3
            != row.try_get::<String, _>("route_localization_blake3")?
        || stored.attempt.world_flow_revision.records_blake3
            != row.try_get::<String, _>("world_records_blake3")?
        || stored.attempt.world_flow_revision.assets_blake3
            != row.try_get::<String, _>("world_assets_blake3")?
        || stored.attempt.world_flow_revision.localization_blake3
            != row.try_get::<String, _>("world_localization_blake3")?
        || stored.attempt.issued_at_unix_ms != i64_to_u64(row.try_get("issued_at_unix_ms")?)?
        || stored.attempt.observed_tick != i64_to_u64(row.try_get("observed_tick")?)?
    {
        return Err(PersistenceError::CorruptStoredProductionExtractionIntent);
    }
    Ok(Some(stored))
}

async fn insert_acceptance(
    connection: &mut PgConnection,
    attempt: &ProductionExtractionIntentAttemptV1,
    canonical_attempt_hash: [u8; HASH_BYTES],
    commit_request_hash: [u8; HASH_BYTES],
    attempt_payload: &[u8],
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO production_extraction_intent_acceptances_v1
         (namespace_id,extraction_request_id,authenticated_account_id,
          attempted_character_id,attempted_mutation_id,contract_version,frame_schema_version,
          frame_payload_hash,extraction_receipt_id,terminal_id,actor_generation,
          accepted_pre_route_state_version,accepted_post_route_state_version,
          route_records_blake3,route_assets_blake3,route_localization_blake3,
          world_records_blake3,world_assets_blake3,world_localization_blake3,
          canonical_attempt_hash,commit_request_hash,attempt_payload,issued_at,observed_tick)
         VALUES
         ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,
          $20,$21,$22,to_timestamp($23::double precision/1000.0),$24)",
    )
    .bind(&attempt.namespace_id)
    .bind(attempt.extraction_request_id.as_slice())
    .bind(attempt.authenticated_account_id.as_slice())
    .bind(attempt.attempted_character_id.as_slice())
    .bind(attempt.attempted_mutation_id.as_slice())
    .bind(
        i16::try_from(attempt.contract_version)
            .map_err(|_| PersistenceError::CorruptStoredProductionExtractionIntent)?,
    )
    .bind(
        i16::try_from(attempt.attempted_frame_schema_version)
            .map_err(|_| PersistenceError::CorruptStoredProductionExtractionIntent)?,
    )
    .bind(attempt.attempted_frame_payload_hash.as_slice())
    .bind(attempt.extraction_receipt_id.as_slice())
    .bind(attempt.terminal_id.as_slice())
    .bind(u64_to_i64(attempt.actor_generation)?)
    .bind(u64_to_i64(attempt.accepted_pre_route_state_version)?)
    .bind(u64_to_i64(attempt.accepted_post_route_state_version)?)
    .bind(&attempt.core_route_revision.records_blake3)
    .bind(&attempt.core_route_revision.assets_blake3)
    .bind(&attempt.core_route_revision.localization_blake3)
    .bind(&attempt.world_flow_revision.records_blake3)
    .bind(&attempt.world_flow_revision.assets_blake3)
    .bind(&attempt.world_flow_revision.localization_blake3)
    .bind(canonical_attempt_hash.as_slice())
    .bind(commit_request_hash.as_slice())
    .bind(attempt_payload)
    .bind(u64_to_i64(attempt.issued_at_unix_ms)?)
    .bind(u64_to_i64(attempt.observed_tick)?)
    .execute(connection)
    .await
    .map_err(PersistenceError::Database)?;
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "the append-only conflict record keeps every canonical audit correlation explicit"
)]
async fn insert_conflict_audit(
    connection: &mut PgConnection,
    stored: &StoredProductionExtractionIntentAcceptanceV1,
    attempted: &ProductionExtractionIntentAttemptV1,
    attempted_hash: [u8; HASH_BYTES],
    attempted_commit_hash: [u8; HASH_BYTES],
    attempted_payload: &[u8],
    conflict_audit_id: [u8; ID_BYTES],
) -> Result<(), PersistenceError> {
    if attempted_hash == stored.canonical_attempt_hash {
        return Err(PersistenceError::CorruptStoredProductionExtractionIntent);
    }
    sqlx::query(
        "INSERT INTO production_extraction_intent_conflict_audits_v1
         (namespace_id,extraction_request_id,conflict_audit_id,attempted_account_id,
          attempted_character_id,attempted_mutation_id,attempted_actor_generation,
          attempted_pre_route_state_version,attempted_post_route_state_version,
          attempted_commit_request_hash,stored_attempt_hash,attempted_attempt_hash,
          attempted_payload,attempted_issued_at,attempted_observed_tick)
         VALUES
         ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,
          to_timestamp($14::double precision/1000.0),$15)
         ON CONFLICT (namespace_id,extraction_request_id,attempted_attempt_hash) DO NOTHING",
    )
    .bind(&attempted.namespace_id)
    .bind(attempted.extraction_request_id.as_slice())
    .bind(conflict_audit_id.as_slice())
    .bind(attempted.authenticated_account_id.as_slice())
    .bind(attempted.attempted_character_id.as_slice())
    .bind(attempted.attempted_mutation_id.as_slice())
    .bind(u64_to_i64(attempted.actor_generation)?)
    .bind(u64_to_i64(attempted.accepted_pre_route_state_version)?)
    .bind(u64_to_i64(attempted.accepted_post_route_state_version)?)
    .bind(attempted_commit_hash.as_slice())
    .bind(stored.canonical_attempt_hash.as_slice())
    .bind(attempted_hash.as_slice())
    .bind(attempted_payload)
    .bind(u64_to_i64(attempted.issued_at_unix_ms)?)
    .bind(u64_to_i64(attempted.observed_tick)?)
    .execute(connection)
    .await
    .map_err(PersistenceError::Database)?;
    Ok(())
}

async fn transaction_time_unix_ms(connection: &mut PgConnection) -> Result<u64, PersistenceError> {
    let value: i64 = sqlx::query_scalar(
        "SELECT floor(extract(epoch FROM transaction_timestamp())*1000)::bigint",
    )
    .fetch_one(connection)
    .await
    .map_err(PersistenceError::Database)?;
    i64_to_u64(value)
}

fn canonical_hash<T: Serialize>(
    context: &str,
    value: &T,
) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let payload = postcard::to_stdvec(value)
        .map_err(|_| PersistenceError::CorruptStoredProductionExtractionIntent)?;
    if payload.is_empty() || payload.len() > MAX_ATTEMPT_PAYLOAD_BYTES {
        return Err(PersistenceError::CorruptStoredProductionExtractionIntent);
    }
    let mut hasher = blake3::Hasher::new_derive_key(context);
    hasher.update(&(payload.len() as u64).to_be_bytes());
    hasher.update(&payload);
    Ok(*hasher.finalize().as_bytes())
}

fn derive_conflict_audit_id(
    extraction_request_id: [u8; ID_BYTES],
    stored_hash: [u8; HASH_BYTES],
    attempted_hash: [u8; HASH_BYTES],
) -> [u8; ID_BYTES] {
    let mut hasher = blake3::Hasher::new_derive_key(CONFLICT_ID_CONTEXT);
    hasher.update(&extraction_request_id);
    hasher.update(&stored_hash);
    hasher.update(&attempted_hash);
    let mut id = [0; ID_BYTES];
    id.copy_from_slice(&hasher.finalize().as_bytes()[..ID_BYTES]);
    id
}

fn valid_revision(records: &str, assets: &str, localization: &str) -> bool {
    [records, assets, localization].into_iter().all(|hash| {
        hash.len() == 64
            && !hash.bytes().all(|byte| byte == b'0')
            && hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn exact_id(value: Vec<u8>) -> Result<[u8; ID_BYTES], PersistenceError> {
    value
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredProductionExtractionIntent)
}

fn exact_hash(value: Vec<u8>) -> Result<[u8; HASH_BYTES], PersistenceError> {
    value
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredProductionExtractionIntent)
}

fn u64_to_i64(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| PersistenceError::CorruptStoredProductionExtractionIntent)
}

fn i64_to_u64(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value).map_err(|_| PersistenceError::CorruptStoredProductionExtractionIntent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        PRODUCTION_EXTRACTION_CONTRACT_VERSION_V1, ProductionExtractionExpectedVersionsV1,
    };

    fn request() -> ProductionExtractionCommitRequestV1 {
        ProductionExtractionCommitRequestV1 {
            contract_version: PRODUCTION_EXTRACTION_CONTRACT_VERSION_V1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            account_id: [1; 16],
            character_id: [2; 16],
            mutation_id: [3; 16],
            terminal_id: [4; 16],
            extraction_request_id: [5; 16],
            extraction_receipt_id: [6; 16],
            encounter_id: [7; 16],
            instance_lineage_id: [8; 16],
            entry_restore_point_id: [9; 16],
            exit_instance_id: [10; 16],
            expected_versions: ProductionExtractionExpectedVersionsV1 {
                account: 1,
                character: 2,
                world: 2,
                inventory: 3,
                life_metrics: 4,
            },
            content_revision: StoredWorldFlowRevisionV1 {
                records_blake3: "a".repeat(64),
                assets_blake3: "b".repeat(64),
                localization_blake3: "c".repeat(64),
            },
            issued_at_unix_ms: 1,
            observed_tick: 30,
        }
    }

    fn attempt() -> ProductionExtractionIntentAttemptV1 {
        let request = request();
        ProductionExtractionIntentAttemptV1 {
            contract_version: PRODUCTION_EXTRACTION_INTENT_CONTRACT_VERSION_V1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            authenticated_account_id: request.account_id,
            attempted_character_id: request.character_id,
            attempted_mutation_id: request.mutation_id,
            attempted_frame_schema_version: PRODUCTION_EXTRACTION_INTENT_FRAME_SCHEMA_VERSION_V1,
            attempted_frame_payload_hash: canonical_production_extraction_frame_payload_hash_v1(
                &request,
            )
            .unwrap(),
            extraction_request_id: request.extraction_request_id,
            extraction_receipt_id: request.extraction_receipt_id,
            terminal_id: request.terminal_id,
            actor_generation: 1,
            accepted_pre_route_state_version: 40,
            accepted_post_route_state_version: 41,
            core_route_revision: ProductionExtractionCoreRouteRevisionV1 {
                records_blake3: "d".repeat(64),
                assets_blake3: "e".repeat(64),
                localization_blake3: "f".repeat(64),
            },
            world_flow_revision: request.content_revision.clone(),
            issued_at_unix_ms: request.issued_at_unix_ms,
            observed_tick: request.observed_tick,
            commit_request: request,
        }
    }

    #[test]
    fn attempt_binds_frame_route_and_complete_commit_material() {
        let attempt = attempt();
        attempt.validate().unwrap();
        assert_ne!(attempt.canonical_hash().unwrap(), [0; 32]);

        let mut changed = attempt.clone();
        changed.commit_request.expected_versions.inventory += 1;
        assert!(matches!(
            changed.validate(),
            Err(PersistenceError::CorruptStoredProductionExtractionIntent)
        ));

        let mut changed = attempt;
        changed.core_route_revision.records_blake3 = "0".repeat(64);
        assert!(matches!(
            changed.validate(),
            Err(PersistenceError::CorruptStoredProductionExtractionIntent)
        ));
    }
}
