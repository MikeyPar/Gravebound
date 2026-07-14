use sqlx::Row;

use crate::{
    PersistenceError, PostgresPersistence, StoredWorldFlowRevisionV1, WIPEABLE_CORE_NAMESPACE,
};

const ID_BYTES: usize = 16;
const HASH_BYTES: usize = 32;
const CALDUS_EXIT_ID: &str = "portal.exit.dungeon.bell_sepulcher";
const HALL_ID: &str = "hub.lantern_halls_01";
const RESTORE_EXTRACTION_COMMITTED: i16 = 1;
const LINEAGE_CLOSED_SUCCESS: i16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredExtractionState {
    Requested,
    Committed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredExtractionAuthority {
    WipeableTestEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaldusExtractionRequest {
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub extraction_request_id: [u8; ID_BYTES],
    pub encounter_id: [u8; ID_BYTES],
    pub instance_lineage_id: [u8; ID_BYTES],
    pub entry_restore_point_id: [u8; ID_BYTES],
    pub exit_instance_id: [u8; ID_BYTES],
    pub attempt_ordinal: u32,
    pub party_slot: u8,
    pub participant_entity_id: u64,
    pub expected_character_version: u64,
    pub content_revision: StoredWorldFlowRevisionV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaldusExtractionCommit {
    pub extraction_request_id: [u8; ID_BYTES],
    pub extraction_receipt_id: [u8; ID_BYTES],
    pub authority: StoredExtractionAuthority,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredExtractionResult {
    pub replayed: bool,
    pub request: CaldusExtractionRequest,
    pub request_payload_hash: [u8; HASH_BYTES],
    pub state: StoredExtractionState,
    pub extraction_receipt_id: Option<[u8; ID_BYTES]>,
    pub receipt_payload_hash: Option<[u8; HASH_BYTES]>,
    pub authority: Option<StoredExtractionAuthority>,
    pub transfer_mutation_id: Option<[u8; ID_BYTES]>,
    pub post_character_version: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaldusExtractionTransaction {
    Fresh(StoredExtractionResult),
    Replay(StoredExtractionResult),
}

impl PostgresPersistence {
    pub async fn request_caldus_extraction(
        &self,
        request: &CaldusExtractionRequest,
    ) -> Result<CaldusExtractionTransaction, PersistenceError> {
        validate_request(request)?;
        let request_payload_hash = request_hash(request)?;
        let mut transaction = self.begin_transaction().await?;
        advisory_lock(transaction.connection(), request.extraction_request_id).await?;
        if let Some(existing) = load_by_request(
            transaction.connection(),
            request.extraction_request_id,
            false,
        )
        .await?
        {
            transaction.rollback().await?;
            if existing.request_payload_hash != request_payload_hash || existing.request != *request
            {
                return Err(PersistenceError::ExtractionIdempotencyConflict);
            }
            return Ok(CaldusExtractionTransaction::Replay(
                StoredExtractionResult {
                    replayed: true,
                    ..existing
                },
            ));
        }
        verify_active_binding(transaction.connection(), request).await?;
        sqlx::query(
            "INSERT INTO character_extraction_results
             (namespace_id,account_id,character_id,extraction_request_id,request_payload_hash,
              encounter_id,instance_lineage_id,entry_restore_point_id,exit_instance_id,
              exit_content_id,attempt_ordinal,party_slot,participant_entity_id,
              expected_character_version,records_blake3,assets_blake3,localization_blake3,
              extraction_state)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,0)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(request.character_id.as_slice())
        .bind(request.extraction_request_id.as_slice())
        .bind(request_payload_hash.as_slice())
        .bind(request.encounter_id.as_slice())
        .bind(request.instance_lineage_id.as_slice())
        .bind(request.entry_restore_point_id.as_slice())
        .bind(request.exit_instance_id.as_slice())
        .bind(CALDUS_EXIT_ID)
        .bind(
            i32::try_from(request.attempt_ordinal)
                .map_err(|_| PersistenceError::CorruptStoredExtraction)?,
        )
        .bind(i16::from(request.party_slot))
        .bind(request.participant_entity_id.to_le_bytes().as_slice())
        .bind(
            i64::try_from(request.expected_character_version)
                .map_err(|_| PersistenceError::CorruptStoredExtraction)?,
        )
        .bind(&request.content_revision.records_blake3)
        .bind(&request.content_revision.assets_blake3)
        .bind(&request.content_revision.localization_blake3)
        .execute(transaction.connection())
        .await?;
        transaction.commit().await?;
        Ok(CaldusExtractionTransaction::Fresh(StoredExtractionResult {
            replayed: false,
            request: request.clone(),
            request_payload_hash,
            state: StoredExtractionState::Requested,
            extraction_receipt_id: None,
            receipt_payload_hash: None,
            authority: None,
            transfer_mutation_id: None,
            post_character_version: None,
        }))
    }

    pub async fn commit_caldus_extraction(
        &self,
        commit: CaldusExtractionCommit,
    ) -> Result<CaldusExtractionTransaction, PersistenceError> {
        if commit.extraction_request_id == [0; ID_BYTES]
            || commit.extraction_receipt_id == [0; ID_BYTES]
            || commit.extraction_request_id == commit.extraction_receipt_id
        {
            return Err(PersistenceError::CorruptStoredExtraction);
        }
        let mut transaction = self.begin_transaction().await?;
        advisory_lock(transaction.connection(), commit.extraction_request_id).await?;
        let existing =
            load_by_request(transaction.connection(), commit.extraction_request_id, true)
                .await?
                .ok_or(PersistenceError::ExtractionBindingMismatch)?;
        let receipt_payload_hash = receipt_hash(&existing.request, commit)?;
        if existing.state == StoredExtractionState::Committed {
            transaction.rollback().await?;
            if existing.extraction_receipt_id != Some(commit.extraction_receipt_id)
                || existing.receipt_payload_hash != Some(receipt_payload_hash)
                || existing.authority != Some(commit.authority)
            {
                return Err(PersistenceError::ExtractionIdempotencyConflict);
            }
            return Ok(CaldusExtractionTransaction::Replay(
                StoredExtractionResult {
                    replayed: true,
                    ..existing
                },
            ));
        }
        verify_active_binding(transaction.connection(), &existing.request).await?;
        reject_completed_restore(transaction.connection(), &existing.request).await?;
        sqlx::query(
            "UPDATE character_entry_restore_points SET restore_state=$1,
                    consumed_at=transaction_timestamp()
             WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4
               AND restore_point_id=$5 AND restore_state=0",
        )
        .bind(RESTORE_EXTRACTION_COMMITTED)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(existing.request.account_id.as_slice())
        .bind(existing.request.character_id.as_slice())
        .bind(existing.request.entry_restore_point_id.as_slice())
        .execute(transaction.connection())
        .await?
        .rows_affected()
        .eq(&1)
        .then_some(())
        .ok_or(PersistenceError::ExtractionSuperseded)?;
        sqlx::query(
            "UPDATE character_instance_lineages SET lineage_state=$1,
                    closed_at=transaction_timestamp()
             WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4
               AND lineage_id=$5 AND lineage_state IN (0,1)",
        )
        .bind(LINEAGE_CLOSED_SUCCESS)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(existing.request.account_id.as_slice())
        .bind(existing.request.character_id.as_slice())
        .bind(existing.request.instance_lineage_id.as_slice())
        .execute(transaction.connection())
        .await?
        .rows_affected()
        .eq(&1)
        .then_some(())
        .ok_or(PersistenceError::ExtractionSuperseded)?;
        let updated = sqlx::query(
            "UPDATE character_extraction_results SET extraction_receipt_id=$1,
                    receipt_payload_hash=$2, extraction_state=1, authority_kind=0,
                    destination_content_id=$3, safe_arrival_kind=0,
                    committed_at=transaction_timestamp()
             WHERE namespace_id=$4 AND extraction_request_id=$5 AND extraction_state=0",
        )
        .bind(commit.extraction_receipt_id.as_slice())
        .bind(receipt_payload_hash.as_slice())
        .bind(HALL_ID)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(commit.extraction_request_id.as_slice())
        .execute(transaction.connection())
        .await?
        .rows_affected();
        if updated != 1 {
            return Err(PersistenceError::ExtractionSuperseded);
        }
        transaction.commit().await?;
        Ok(CaldusExtractionTransaction::Fresh(StoredExtractionResult {
            replayed: false,
            state: StoredExtractionState::Committed,
            extraction_receipt_id: Some(commit.extraction_receipt_id),
            receipt_payload_hash: Some(receipt_payload_hash),
            authority: Some(commit.authority),
            ..existing
        }))
    }
}

async fn advisory_lock(
    connection: &mut sqlx::PgConnection,
    request_id: [u8; ID_BYTES],
) -> Result<(), PersistenceError> {
    let key = i64::from_le_bytes(
        request_id[..8]
            .try_into()
            .map_err(|_| PersistenceError::CorruptStoredExtraction)?,
    );
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(key)
        .execute(connection)
        .await?;
    Ok(())
}

async fn verify_active_binding(
    connection: &mut sqlx::PgConnection,
    request: &CaldusExtractionRequest,
) -> Result<(), PersistenceError> {
    let account_exists: Option<i32> = sqlx::query_scalar(
        "SELECT 1 FROM accounts WHERE namespace_id=$1 AND account_id=$2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?;
    if account_exists.is_none() {
        return Err(PersistenceError::ExtractionBindingMismatch);
    }
    let character = sqlx::query(
        "SELECT life_state,character_state_version FROM characters
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ExtractionBindingMismatch)?;
    let restore = sqlx::query(
        "SELECT lineage_id,restore_state FROM character_entry_restore_points
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
           AND restore_point_id=$4 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.entry_restore_point_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ExtractionBindingMismatch)?;
    let location = sqlx::query(
        "SELECT location_kind,instance_lineage_id,entry_restore_point_id
         FROM character_world_locations WHERE namespace_id=$1 AND account_id=$2
           AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ExtractionBindingMismatch)?;
    let lineage_state: Option<i16> = sqlx::query_scalar(
        "SELECT lineage_state FROM character_instance_lineages
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND lineage_id=$4
         FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.instance_lineage_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?;
    let exit = sqlx::query(
        "SELECT instance_lineage_id,attempt_ordinal,exit_instance_id
         FROM caldus_victory_exits WHERE namespace_id=$1 AND encounter_id=$2 FOR SHARE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.encounter_id.as_slice())
    .fetch_optional(connection)
    .await?
    .ok_or(PersistenceError::ExtractionBindingMismatch)?;
    let expected_version = i64::try_from(request.expected_character_version)
        .map_err(|_| PersistenceError::CorruptStoredExtraction)?;
    let attempt = i32::try_from(request.attempt_ordinal)
        .map_err(|_| PersistenceError::CorruptStoredExtraction)?;
    let bound = character.try_get::<i16, _>("life_state")? == 0
        && character.try_get::<i64, _>("character_state_version")? == expected_version
        && fixed_bytes::<ID_BYTES>(restore.try_get("lineage_id")?)? == request.instance_lineage_id
        && restore.try_get::<i16, _>("restore_state")? == 0
        && location.try_get::<i16, _>("location_kind")? == 2
        && fixed_bytes::<ID_BYTES>(location.try_get("instance_lineage_id")?)?
            == request.instance_lineage_id
        && fixed_bytes::<ID_BYTES>(location.try_get("entry_restore_point_id")?)?
            == request.entry_restore_point_id
        && lineage_state.is_some_and(|state| matches!(state, 0 | 1))
        && fixed_bytes::<ID_BYTES>(exit.try_get("instance_lineage_id")?)?
            == request.instance_lineage_id
        && exit.try_get::<i32, _>("attempt_ordinal")? == attempt
        && fixed_bytes::<ID_BYTES>(exit.try_get("exit_instance_id")?)? == request.exit_instance_id;
    if !bound {
        return Err(PersistenceError::ExtractionSuperseded);
    }
    Ok(())
}

async fn reject_completed_restore(
    connection: &mut sqlx::PgConnection,
    request: &CaldusExtractionRequest,
) -> Result<(), PersistenceError> {
    let restored: Option<i64> = sqlx::query_scalar(
        "SELECT restored_progression_version FROM entry_restore_progression_v1
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND restore_point_id=$4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.entry_restore_point_id.as_slice())
    .fetch_optional(connection)
    .await?
    .flatten();
    if restored.is_some() {
        return Err(PersistenceError::ExtractionSuperseded);
    }
    Ok(())
}

async fn load_by_request(
    connection: &mut sqlx::PgConnection,
    request_id: [u8; ID_BYTES],
    lock: bool,
) -> Result<Option<StoredExtractionResult>, PersistenceError> {
    const SELECT_RESULT: &str =
        "SELECT account_id,character_id,extraction_receipt_id,request_payload_hash,
                receipt_payload_hash,encounter_id,instance_lineage_id,entry_restore_point_id,
                exit_instance_id,attempt_ordinal,party_slot,participant_entity_id,
                expected_character_version,records_blake3,assets_blake3,localization_blake3,
                extraction_state,authority_kind,transfer_mutation_id,post_character_version
         FROM character_extraction_results WHERE namespace_id=$1 AND extraction_request_id=$2";
    const SELECT_RESULT_FOR_UPDATE: &str =
        "SELECT account_id,character_id,extraction_receipt_id,request_payload_hash,
                receipt_payload_hash,encounter_id,instance_lineage_id,entry_restore_point_id,
                exit_instance_id,attempt_ordinal,party_slot,participant_entity_id,
                expected_character_version,records_blake3,assets_blake3,localization_blake3,
                extraction_state,authority_kind,transfer_mutation_id,post_character_version
         FROM character_extraction_results WHERE namespace_id=$1 AND extraction_request_id=$2
         FOR UPDATE";
    let row = if lock {
        sqlx::query(SELECT_RESULT_FOR_UPDATE)
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(request_id.as_slice())
            .fetch_optional(&mut *connection)
            .await?
    } else {
        sqlx::query(SELECT_RESULT)
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(request_id.as_slice())
            .fetch_optional(connection)
            .await?
    };
    row.as_ref()
        .map(|row| decode_result(row, request_id))
        .transpose()
}

fn decode_result(
    row: &sqlx::postgres::PgRow,
    request_id: [u8; ID_BYTES],
) -> Result<StoredExtractionResult, PersistenceError> {
    let state = match row.try_get::<i16, _>("extraction_state")? {
        0 => StoredExtractionState::Requested,
        1 => StoredExtractionState::Committed,
        _ => return Err(PersistenceError::CorruptStoredExtraction),
    };
    let authority = match row.try_get::<Option<i16>, _>("authority_kind")? {
        None => None,
        Some(0) => Some(StoredExtractionAuthority::WipeableTestEvidence),
        Some(_) => return Err(PersistenceError::CorruptStoredExtraction),
    };
    let expected_character_version =
        u64::try_from(row.try_get::<i64, _>("expected_character_version")?)
            .map_err(|_| PersistenceError::CorruptStoredExtraction)?;
    Ok(StoredExtractionResult {
        replayed: false,
        request: CaldusExtractionRequest {
            account_id: fixed_bytes(row.try_get("account_id")?)?,
            character_id: fixed_bytes(row.try_get("character_id")?)?,
            extraction_request_id: request_id,
            encounter_id: fixed_bytes(row.try_get("encounter_id")?)?,
            instance_lineage_id: fixed_bytes(row.try_get("instance_lineage_id")?)?,
            entry_restore_point_id: fixed_bytes(row.try_get("entry_restore_point_id")?)?,
            exit_instance_id: fixed_bytes(row.try_get("exit_instance_id")?)?,
            attempt_ordinal: u32::try_from(row.try_get::<i32, _>("attempt_ordinal")?)
                .map_err(|_| PersistenceError::CorruptStoredExtraction)?,
            party_slot: u8::try_from(row.try_get::<i16, _>("party_slot")?)
                .map_err(|_| PersistenceError::CorruptStoredExtraction)?,
            participant_entity_id: u64::from_le_bytes(fixed_bytes(
                row.try_get("participant_entity_id")?,
            )?),
            expected_character_version,
            content_revision: StoredWorldFlowRevisionV1 {
                records_blake3: row.try_get("records_blake3")?,
                assets_blake3: row.try_get("assets_blake3")?,
                localization_blake3: row.try_get("localization_blake3")?,
            },
        },
        request_payload_hash: fixed_bytes(row.try_get("request_payload_hash")?)?,
        state,
        extraction_receipt_id: row
            .try_get::<Option<Vec<u8>>, _>("extraction_receipt_id")?
            .map(fixed_bytes)
            .transpose()?,
        receipt_payload_hash: row
            .try_get::<Option<Vec<u8>>, _>("receipt_payload_hash")?
            .map(fixed_bytes)
            .transpose()?,
        authority,
        transfer_mutation_id: row
            .try_get::<Option<Vec<u8>>, _>("transfer_mutation_id")?
            .map(fixed_bytes)
            .transpose()?,
        post_character_version: row
            .try_get::<Option<i64>, _>("post_character_version")?
            .map(u64::try_from)
            .transpose()
            .map_err(|_| PersistenceError::CorruptStoredExtraction)?,
    })
}

fn validate_request(request: &CaldusExtractionRequest) -> Result<(), PersistenceError> {
    if [
        request.account_id,
        request.character_id,
        request.extraction_request_id,
        request.encounter_id,
        request.instance_lineage_id,
        request.entry_restore_point_id,
        request.exit_instance_id,
    ]
    .contains(&[0; ID_BYTES])
        || request.attempt_ordinal == 0
        || request.party_slot >= 8
        || request.participant_entity_id == 0
        || request.expected_character_version == 0
        || !valid_revision(&request.content_revision)
    {
        return Err(PersistenceError::CorruptStoredExtraction);
    }
    Ok(())
}

fn request_hash(request: &CaldusExtractionRequest) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let attempt = request.attempt_ordinal.to_le_bytes();
    let slot = [request.party_slot];
    let entity = request.participant_entity_id.to_le_bytes();
    let version = request.expected_character_version.to_le_bytes();
    canonical_hash(
        b"gravebound.caldus.extraction-request-payload.v1",
        &[
            &request.account_id,
            &request.character_id,
            &request.extraction_request_id,
            &request.encounter_id,
            &request.instance_lineage_id,
            &request.entry_restore_point_id,
            &request.exit_instance_id,
            &attempt,
            &slot,
            &entity,
            &version,
            CALDUS_EXIT_ID.as_bytes(),
            request.content_revision.records_blake3.as_bytes(),
            request.content_revision.assets_blake3.as_bytes(),
            request.content_revision.localization_blake3.as_bytes(),
        ],
    )
}

fn receipt_hash(
    request: &CaldusExtractionRequest,
    commit: CaldusExtractionCommit,
) -> Result<[u8; HASH_BYTES], PersistenceError> {
    canonical_hash(
        b"gravebound.caldus.extraction-receipt-payload.v1",
        &[
            &request.extraction_request_id,
            &commit.extraction_receipt_id,
            &request.account_id,
            &request.character_id,
            &request.instance_lineage_id,
            &request.entry_restore_point_id,
            &request.exit_instance_id,
            &[match commit.authority {
                StoredExtractionAuthority::WipeableTestEvidence => 0,
            }],
            HALL_ID.as_bytes(),
            &[0],
        ],
    )
}

fn canonical_hash(domain: &[u8], fields: &[&[u8]]) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let mut hasher = blake3::Hasher::new();
    for field in std::iter::once(domain).chain(fields.iter().copied()) {
        let length =
            u32::try_from(field.len()).map_err(|_| PersistenceError::CorruptStoredExtraction)?;
        hasher.update(&length.to_le_bytes());
        hasher.update(field);
    }
    Ok(*hasher.finalize().as_bytes())
}

fn fixed_bytes<const N: usize>(value: Vec<u8>) -> Result<[u8; N], PersistenceError> {
    value
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredExtraction)
}

fn valid_revision(revision: &StoredWorldFlowRevisionV1) -> bool {
    [
        &revision.records_blake3,
        &revision.assets_blake3,
        &revision.localization_blake3,
    ]
    .into_iter()
    .all(|value| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> CaldusExtractionRequest {
        CaldusExtractionRequest {
            account_id: [1; 16],
            character_id: [2; 16],
            extraction_request_id: [3; 16],
            encounter_id: [4; 16],
            instance_lineage_id: [5; 16],
            entry_restore_point_id: [6; 16],
            exit_instance_id: [7; 16],
            attempt_ordinal: 1,
            party_slot: 0,
            participant_entity_id: 8,
            expected_character_version: 9,
            content_revision: StoredWorldFlowRevisionV1 {
                records_blake3: "1".repeat(64),
                assets_blake3: "2".repeat(64),
                localization_blake3: "3".repeat(64),
            },
        }
    }

    #[test]
    fn request_and_receipt_hashes_are_stable_and_domain_separated() {
        let original = request();
        let commit = CaldusExtractionCommit {
            extraction_request_id: original.extraction_request_id,
            extraction_receipt_id: [9; 16],
            authority: StoredExtractionAuthority::WipeableTestEvidence,
        };
        assert_eq!(
            request_hash(&original).unwrap(),
            request_hash(&original).unwrap()
        );
        assert_ne!(
            request_hash(&original).unwrap(),
            receipt_hash(&original, commit).unwrap()
        );
        let mut changed = original;
        changed.exit_instance_id[0] ^= 1;
        assert_ne!(
            request_hash(&changed).unwrap(),
            request_hash(&request()).unwrap()
        );
    }

    #[test]
    fn schema_26_is_a_receipt_seam_without_m03_08_inventory_ownership() {
        let migration = include_str!("../../../migrations/0026_caldus_extraction_receipt.sql");
        let normalized = migration.to_ascii_lowercase();
        for forbidden in [
            "item_instances",
            "character_inventories",
            "overflow",
            "resolutionhold",
            "resolution_hold",
            "ash_wallet",
            "run_material",
        ] {
            assert!(
                !normalized.contains(forbidden),
                "schema 26 must not own {forbidden}"
            );
        }
        assert!(normalized.contains("extraction_state = 0"));
        assert!(normalized.contains("extraction_state = 1"));
        assert!(normalized.contains("hub.lantern_halls_01"));
        assert!(normalized.contains("safe_arrival_kind = 0"));
    }
}
