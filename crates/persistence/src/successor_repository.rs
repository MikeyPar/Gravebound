//! Replay-first serializable writer for GB-M03 successor creation.
//!
//! The lock and write order follows the canonical Production GDD (`DTH-020`/`021` and
//! `TECH-021`-`023`), Content Production Spec (`CONT-CATALOG-003`), Development Roadmap
//! (`GB-M03-07`), and accepted `SPEC-CONFLICT-031`. The repository never reconstructs the
//! death-time preset and never commits a character separately from its exact starter grant.

use sqlx::{PgConnection, Row};

use crate::{
    CORE_ITEM_CONTENT_REVISION, CORE_SUCCESSOR_BASE_SILHOUETTE_ID, CORE_SUCCESSOR_CLASS_ID,
    DurableSuccessorPresetV1, PersistenceError, PostgresPersistence, STARTER_INITIALIZER_REVISION,
    SUCCESSOR_APPEARANCE_KIND_CORE_BASE_SILHOUETTE, StoredSuccessorResultV1,
    SuccessorCreateRequestV1, SuccessorCreateTransactionV1, WIPEABLE_CORE_NAMESPACE,
    is_retryable_transaction_failure, items::initialize_starter_items_in_transaction,
};

const MAX_TRANSACTION_ATTEMPTS: u8 = 3;
const ID_BYTES: usize = 16;
const HASH_BYTES: usize = 32;
const RESERVATION_ACTIVE: i16 = 0;
const RESERVATION_CONSUMED: i16 = 1;
const RESERVATION_SUPERSEDED: i16 = 2;
const ORDINARY_DEATH_PROVENANCE: i16 = 0;
const SUCCESSOR_CREATED_EVENT_TYPE: i16 = 1;
const SUCCESSOR_RESULT_CODE: i16 = 1;
const SUCCESSOR_AUDIT_ID_CONTEXT_V1: &str = "gravebound.successor-created-audit.v1";
const SUCCESSOR_OUTBOX_ID_CONTEXT_V1: &str = "gravebound.successor-created-outbox.v1";
const SUCCESSOR_CONFLICT_DIGEST_CONTEXT_V1: &str = "gravebound.successor-conflict.v1";

#[derive(Debug)]
struct LockedAccount {
    pre_version: u64,
}

#[derive(Debug)]
struct LockedSuccessorAuthority {
    preset: DurableSuccessorPresetV1,
    reservation_state: i16,
}

impl PostgresPersistence {
    /// Creates one fully initialized successor or returns the immutable stored result.
    pub async fn create_successor_v1(
        &self,
        request: &SuccessorCreateRequestV1,
    ) -> Result<SuccessorCreateTransactionV1, PersistenceError> {
        request.validate()?;
        for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self.create_successor_once_v1(request).await {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS
                        && is_retryable_transaction_failure(&error) => {}
                result => return result,
            }
        }
        unreachable!("bounded successor transaction loop always returns")
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the complete successor graph is deliberately one auditable transaction"
    )]
    async fn create_successor_once_v1(
        &self,
        request: &SuccessorCreateRequestV1,
    ) -> Result<SuccessorCreateTransactionV1, PersistenceError> {
        let mut transaction = self.begin_transaction().await?;
        let account = lock_account(transaction.connection(), request.account_id).await?;

        if let Some(stored) = load_result_by_mutation(
            transaction.connection(),
            request.account_id,
            request.mutation_id,
        )
        .await?
        {
            if exact_replay(&stored, request) {
                transaction.rollback().await?;
                return Ok(SuccessorCreateTransactionV1::Replayed(stored));
            }
            if stored.canonical_request_hash == request.canonical_request_hash {
                return Err(PersistenceError::CorruptStoredSuccessor);
            }
            insert_conflict_audit(transaction.connection(), &stored, request).await?;
            transaction.commit().await?;
            return Ok(SuccessorCreateTransactionV1::Conflict {
                stored_mutation_id: stored.mutation_id,
                stored_death_id: stored.death_id,
            });
        }

        if result_exists_for_death(
            transaction.connection(),
            request.account_id,
            request.death_id,
        )
        .await?
        {
            transaction.rollback().await?;
            return Err(PersistenceError::SuccessorAlreadyConsumed);
        }

        reject_cross_domain_mutation_reuse(transaction.connection(), request).await?;
        let authority = lock_successor_authority(transaction.connection(), request).await?;
        match authority.reservation_state {
            RESERVATION_ACTIVE => {}
            RESERVATION_CONSUMED => {
                transaction.rollback().await?;
                return Err(PersistenceError::SuccessorAlreadyConsumed);
            }
            RESERVATION_SUPERSEDED => {
                transaction.rollback().await?;
                return Err(PersistenceError::SuccessorDeathSuperseded);
            }
            _ => return Err(PersistenceError::CorruptStoredSuccessor),
        }
        lock_reserved_slot(transaction.connection(), request, &authority.preset).await?;
        reject_successor_identity_collision(transaction.connection(), request).await?;

        insert_successor_aggregate(transaction.connection(), request, &authority.preset).await?;
        let starter = initialize_starter_items_in_transaction(
            transaction.connection(),
            request.account_id,
            request.successor_id,
            request.starter_request_hash,
            request.starter_result_hash,
            &request.starter_items,
        )
        .await?;
        if starter.replayed
            || starter.pre_inventory_version != 1
            || starter.post_inventory_version != 2
            || starter.result_hash != request.starter_result_hash
            || starter.items != request.starter_items
        {
            return Err(PersistenceError::CorruptStoredSuccessor);
        }

        let post_account_version = account
            .pre_version
            .checked_add(1)
            .ok_or(PersistenceError::CorruptStoredSuccessor)?;
        select_successor(
            transaction.connection(),
            request,
            account.pre_version,
            post_account_version,
        )
        .await?;
        let result = StoredSuccessorResultV1::from_request(
            request,
            &authority.preset,
            post_account_version,
        )?;
        let result_payload = result.encode()?;
        insert_result_root(
            transaction.connection(),
            request,
            &authority.preset,
            account.pre_version,
            &result,
            &result_payload,
        )
        .await?;
        insert_creation_receipt(transaction.connection(), request, &result).await?;
        insert_audit_and_outbox(transaction.connection(), request, &result, &result_payload)
            .await?;
        consume_reservation(transaction.connection(), request).await?;
        force_deferred_constraints(transaction.connection()).await?;
        transaction.commit().await?;
        Ok(SuccessorCreateTransactionV1::Fresh(result))
    }
}

fn exact_replay(stored: &StoredSuccessorResultV1, request: &SuccessorCreateRequestV1) -> bool {
    stored.account_id == request.account_id
        && stored.mutation_id == request.mutation_id
        && stored.death_id == request.death_id
        && stored.successor_id == request.successor_id
        && stored.receipt_id == request.receipt_id
        && stored.canonical_request_hash == request.canonical_request_hash
        && stored.content_revision == request.content_revision
        && stored
            .starter_items
            .ordered_uids()
            .into_iter()
            .eq(request.starter_items.iter().map(|item| item.item_uid))
}

async fn lock_account(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
) -> Result<LockedAccount, PersistenceError> {
    let row = sqlx::query(
        "SELECT state_version, slot_capacity FROM accounts \
         WHERE namespace_id=$1 AND account_id=$2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_optional(connection)
    .await?;
    let Some(row) = row else {
        return Err(PersistenceError::SuccessorDeathNotFound);
    };
    if row.try_get::<i16, _>("slot_capacity")? != 2 {
        return Err(PersistenceError::CorruptStoredSuccessor);
    }
    Ok(LockedAccount {
        pre_version: positive_u64(row.try_get("state_version")?)?,
    })
}

async fn load_result_by_mutation(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    mutation_id: [u8; ID_BYTES],
) -> Result<Option<StoredSuccessorResultV1>, PersistenceError> {
    let row = sqlx::query(
        "SELECT result.*, receipt.initializer_revision, receipt.initializer_request_hash, \
                receipt.initializer_result_hash, receipt.weapon_uid, receipt.relic_uid, \
                receipt.tonic_uid_0, receipt.tonic_uid_1, receipt.item_count, \
                receipt.item_content_revision \
         FROM successor_mutation_results_v1 AS result \
         JOIN successor_creation_receipts_v1 AS receipt \
           ON receipt.namespace_id=result.namespace_id \
          AND receipt.account_id=result.account_id \
          AND receipt.mutation_id=result.mutation_id \
         WHERE result.namespace_id=$1 AND result.account_id=$2 AND result.mutation_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(mutation_id.as_slice())
    .fetch_optional(connection)
    .await?;
    row.as_ref().map(decode_stored_result).transpose()
}

#[allow(
    clippy::too_many_lines,
    reason = "every normalized stored-result column is compared to its canonical payload"
)]
fn decode_stored_result(
    row: &sqlx::postgres::PgRow,
) -> Result<StoredSuccessorResultV1, PersistenceError> {
    let payload: Vec<u8> = row.try_get("result_payload")?;
    let result = StoredSuccessorResultV1::decode(&payload)?;
    let result_hash = exact_hash(row.try_get("result_hash")?)?;
    let appearance_kind = nonnegative_u16(row.try_get("appearance_kind")?)?;
    let former_roster_ordinal = positive_u8(row.try_get("former_roster_ordinal")?)?;
    let pre_account_version = positive_u64(row.try_get("pre_account_version")?)?;
    let post_account_version = positive_u64(row.try_get("post_account_version")?)?;
    let receipt_uids = [
        exact_id(row.try_get("weapon_uid")?)?,
        exact_id(row.try_get("relic_uid")?)?,
        exact_id(row.try_get("tonic_uid_0")?)?,
        exact_id(row.try_get("tonic_uid_1")?)?,
    ];
    if nonnegative_u16(row.try_get("contract_version")?)? != result.contract_version
        || nonnegative_u16(row.try_get("protocol_major")?)? != result.protocol_major
        || nonnegative_u16(row.try_get("protocol_minor")?)? != result.protocol_minor
        || exact_id(row.try_get("account_id")?)? != result.account_id
        || exact_id(row.try_get("mutation_id")?)? != result.mutation_id
        || exact_id(row.try_get("death_id")?)? != result.death_id
        || exact_id(row.try_get("successor_id")?)? != result.successor_id
        || exact_id(row.try_get("selected_character_id")?)? != result.selected_character_id
        || exact_id(row.try_get("receipt_id")?)? != result.receipt_id
        || exact_hash(row.try_get("canonical_request_hash")?)? != result.canonical_request_hash
        || former_roster_ordinal != result.former_roster_ordinal
        || row.try_get::<String, _>("class_id")? != result.class_id
        || appearance_kind != result.appearance.durable_kind()
        || row.try_get::<String, _>("base_silhouette_id")? != result.base_silhouette_id
        || exact_hash(row.try_get("preset_hash")?)? != result.preset_hash
        || row.try_get::<String, _>("content_revision")? != result.content_revision
        || row.try_get::<i16, _>("result_code")? != SUCCESSOR_RESULT_CODE
        || result_hash != result.result_hash
        || pre_account_version.checked_add(1) != Some(post_account_version)
        || post_account_version != result.versions.account
        || positive_u64(row.try_get("post_character_version")?)? != result.versions.character
        || positive_u64(row.try_get("post_progression_version")?)? != result.versions.progression
        || positive_u64(row.try_get("post_world_version")?)? != result.versions.world
        || positive_u64(row.try_get("post_inventory_version")?)? != result.versions.inventory
        || positive_u64(row.try_get("post_life_metrics_version")?)? != result.versions.life_metrics
        || positive_u64(row.try_get("post_oath_bargain_version")?)? != result.versions.oath_bargain
        || row.try_get::<String, _>("initializer_revision")? != STARTER_INITIALIZER_REVISION
        || exact_hash(row.try_get("initializer_request_hash")?)? == [0; HASH_BYTES]
        || exact_hash(row.try_get("initializer_result_hash")?)? == [0; HASH_BYTES]
        || receipt_uids != result.starter_items.ordered_uids()
        || row.try_get::<i16, _>("item_count")? != 4
        || row.try_get::<String, _>("item_content_revision")? != result.content_revision
        || payload != result.encode()?
    {
        return Err(PersistenceError::CorruptStoredSuccessor);
    }
    Ok(result)
}

async fn result_exists_for_death(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    death_id: [u8; ID_BYTES],
) -> Result<bool, PersistenceError> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM successor_mutation_results_v1 \
         WHERE namespace_id=$1 AND account_id=$2 AND death_id=$3)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(death_id.as_slice())
    .fetch_one(connection)
    .await?;
    Ok(exists)
}

async fn reject_cross_domain_mutation_reuse(
    connection: &mut PgConnection,
    request: &SuccessorCreateRequestV1,
) -> Result<(), PersistenceError> {
    let reused: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM account_mutation_results \
         WHERE namespace_id=$1 AND account_id=$2 AND mutation_id=$3)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .fetch_one(connection)
    .await?;
    if reused {
        return Err(PersistenceError::SuccessorIdempotencyConflict);
    }
    Ok(())
}

async fn lock_successor_authority(
    connection: &mut PgConnection,
    request: &SuccessorCreateRequestV1,
) -> Result<LockedSuccessorAuthority, PersistenceError> {
    let row = sqlx::query(
        "SELECT preset.former_character_id, preset.former_roster_ordinal, preset.class_id, \
                preset.appearance_kind, preset.base_silhouette_id, preset.content_revision, \
                preset.preset_hash, \
                (EXTRACT(EPOCH FROM preset.created_at) * 1000)::BIGINT AS created_at_unix_ms, \
                reservation.reservation_state, death.death_provenance, \
                death.former_roster_ordinal AS death_roster_ordinal, \
                death.content_revision AS death_content_revision \
         FROM death_successor_presets_v1 AS preset \
         JOIN successor_roster_reservations_v1 AS reservation \
           ON reservation.namespace_id=preset.namespace_id \
          AND reservation.account_id=preset.account_id \
          AND reservation.death_id=preset.death_id \
         JOIN death_events AS death \
           ON death.namespace_id=preset.namespace_id \
          AND death.account_id=preset.account_id \
          AND death.death_id=preset.death_id \
         WHERE preset.namespace_id=$1 AND preset.account_id=$2 AND preset.death_id=$3 \
         FOR UPDATE OF preset, reservation, death",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.death_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return classify_missing_death(connection, request).await;
    };
    let former_roster_ordinal = positive_u8(row.try_get("former_roster_ordinal")?)?;
    let appearance_kind = nonnegative_u16(row.try_get("appearance_kind")?)?;
    let preset = DurableSuccessorPresetV1 {
        namespace_id: WIPEABLE_CORE_NAMESPACE.to_owned(),
        account_id: request.account_id,
        former_character_id: exact_id(row.try_get("former_character_id")?)?,
        death_id: request.death_id,
        former_roster_ordinal,
        class_id: row.try_get("class_id")?,
        appearance_kind,
        base_silhouette_id: row.try_get("base_silhouette_id")?,
        content_revision: row.try_get("content_revision")?,
        created_at_unix_ms: positive_u64(row.try_get("created_at_unix_ms")?)?,
        preset_hash: exact_hash(row.try_get("preset_hash")?)?,
    };
    if row.try_get::<i16, _>("death_provenance")? != ORDINARY_DEATH_PROVENANCE
        || positive_u8(row.try_get("death_roster_ordinal")?)? != former_roster_ordinal
        || row.try_get::<String, _>("death_content_revision")? != preset.content_revision
        || preset.former_roster_ordinal > 2
        || preset.class_id != CORE_SUCCESSOR_CLASS_ID
        || preset.appearance_kind != SUCCESSOR_APPEARANCE_KIND_CORE_BASE_SILHOUETTE
        || preset.base_silhouette_id != CORE_SUCCESSOR_BASE_SILHOUETTE_ID
        || preset.content_revision != request.content_revision
        || preset.content_revision != CORE_ITEM_CONTENT_REVISION
        || preset.preset_hash != preset.expected_hash()?
    {
        if preset.content_revision != request.content_revision {
            return Err(PersistenceError::SuccessorContentMismatch);
        }
        return Err(PersistenceError::CorruptStoredSuccessor);
    }
    Ok(LockedSuccessorAuthority {
        preset,
        reservation_state: row.try_get("reservation_state")?,
    })
}

async fn classify_missing_death(
    connection: &mut PgConnection,
    request: &SuccessorCreateRequestV1,
) -> Result<LockedSuccessorAuthority, PersistenceError> {
    let owner: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT account_id FROM death_events WHERE namespace_id=$1 AND death_id=$2",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.death_id.as_slice())
    .fetch_optional(connection)
    .await?;
    match owner {
        Some(owner) => {
            if exact_id(owner)? == request.account_id {
                Err(PersistenceError::SuccessorDeathNotTerminal)
            } else {
                Err(PersistenceError::SuccessorForeignAuthority)
            }
        }
        None => Err(PersistenceError::SuccessorDeathNotFound),
    }
}

async fn lock_reserved_slot(
    connection: &mut PgConnection,
    request: &SuccessorCreateRequestV1,
    preset: &DurableSuccessorPresetV1,
) -> Result<(), PersistenceError> {
    let occupying: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT character_id FROM characters \
         WHERE namespace_id=$1 AND account_id=$2 AND roster_ordinal=$3 AND life_state=0 \
         FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(i16::from(preset.former_roster_ordinal))
    .fetch_optional(connection)
    .await?;
    if occupying.is_some() {
        return Err(PersistenceError::SuccessorSlotConflict);
    }
    Ok(())
}

async fn reject_successor_identity_collision(
    connection: &mut PgConnection,
    request: &SuccessorCreateRequestV1,
) -> Result<(), PersistenceError> {
    let collision: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT account_id FROM characters WHERE namespace_id=$1 AND character_id=$2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.successor_id.as_slice())
    .fetch_optional(connection)
    .await?;
    if collision.is_some() {
        return Err(PersistenceError::SuccessorSlotConflict);
    }
    Ok(())
}

async fn insert_successor_aggregate(
    connection: &mut PgConnection,
    request: &SuccessorCreateRequestV1,
    preset: &DurableSuccessorPresetV1,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO characters \
         (namespace_id, account_id, character_id, roster_ordinal, class_id, level, oath_id, \
          life_state, security_state, character_state_version) \
         VALUES ($1,$2,$3,$4,$5,1,NULL,0,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.successor_id.as_slice())
    .bind(i16::from(preset.former_roster_ordinal))
    .bind(&preset.class_id)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "INSERT INTO character_progression \
         (namespace_id, account_id, character_id, total_xp, level, current_health, \
          progression_version) VALUES ($1,$2,$3,0,1,120,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.successor_id.as_slice())
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "INSERT INTO character_world_locations \
         (namespace_id, account_id, character_id, character_version, location_kind, \
          safe_arrival_kind) VALUES ($1,$2,$3,1,0,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.successor_id.as_slice())
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "INSERT INTO character_life_metrics \
         (namespace_id, account_id, character_id, lifetime_ticks, permadeath_combat_ticks, \
          life_metrics_version) VALUES ($1,$2,$3,0,0,1) \
          ON CONFLICT (namespace_id, account_id, character_id) DO NOTHING",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.successor_id.as_slice())
    .execute(&mut *connection)
    .await?;
    let life_metrics: (i64, i64, i64) = sqlx::query_as(
        "SELECT lifetime_ticks,permadeath_combat_ticks,life_metrics_version \
         FROM character_life_metrics WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.successor_id.as_slice())
    .fetch_one(&mut *connection)
    .await?;
    if life_metrics != (0, 0, 1) {
        return Err(PersistenceError::CorruptStoredSuccessor);
    }
    sqlx::query(
        "INSERT INTO character_oath_bargain_state \
         (namespace_id, account_id, character_id, earned_bargain_slots, oath_bargain_version) \
         VALUES ($1,$2,$3,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.successor_id.as_slice())
    .execute(connection)
    .await?;
    Ok(())
}

async fn select_successor(
    connection: &mut PgConnection,
    request: &SuccessorCreateRequestV1,
    pre_account_version: u64,
    post_account_version: u64,
) -> Result<(), PersistenceError> {
    let updated = sqlx::query(
        "UPDATE accounts SET state_version=$1, selected_character_id=$2, \
         updated_at=transaction_timestamp() \
         WHERE namespace_id=$3 AND account_id=$4 AND state_version=$5",
    )
    .bind(to_i64(post_account_version)?)
    .bind(request.successor_id.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(to_i64(pre_account_version)?)
    .execute(connection)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(PersistenceError::CorruptStoredSuccessor);
    }
    Ok(())
}

async fn insert_result_root(
    connection: &mut PgConnection,
    request: &SuccessorCreateRequestV1,
    preset: &DurableSuccessorPresetV1,
    pre_account_version: u64,
    result: &StoredSuccessorResultV1,
    result_payload: &[u8],
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO successor_mutation_results_v1 \
         (namespace_id, account_id, mutation_id, death_id, successor_id, \
          selected_character_id, receipt_id, contract_version, protocol_major, protocol_minor, \
          canonical_request_hash, former_roster_ordinal, class_id, appearance_kind, \
          base_silhouette_id, preset_hash, content_revision, result_code, result_payload, \
          result_hash, pre_account_version, post_account_version, post_character_version, \
          post_progression_version, post_world_version, post_inventory_version, \
          post_life_metrics_version, post_oath_bargain_version) \
         VALUES ($1,$2,$3,$4,$5,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,1,$17,$18, \
                 $19,$20,1,1,1,2,1,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(request.death_id.as_slice())
    .bind(request.successor_id.as_slice())
    .bind(request.receipt_id.as_slice())
    .bind(i16::try_from(result.contract_version).map_err(|_| corrupt())?)
    .bind(i16::try_from(result.protocol_major).map_err(|_| corrupt())?)
    .bind(i16::try_from(result.protocol_minor).map_err(|_| corrupt())?)
    .bind(result.canonical_request_hash.as_slice())
    .bind(i16::from(preset.former_roster_ordinal))
    .bind(&preset.class_id)
    .bind(i16::try_from(preset.appearance_kind).map_err(|_| corrupt())?)
    .bind(&preset.base_silhouette_id)
    .bind(preset.preset_hash.as_slice())
    .bind(&preset.content_revision)
    .bind(result_payload)
    .bind(result.result_hash.as_slice())
    .bind(to_i64(pre_account_version)?)
    .bind(to_i64(result.versions.account)?)
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_creation_receipt(
    connection: &mut PgConnection,
    request: &SuccessorCreateRequestV1,
    result: &StoredSuccessorResultV1,
) -> Result<(), PersistenceError> {
    let [weapon_uid, relic_uid, tonic_uid_0, tonic_uid_1] = result.starter_items.ordered_uids();
    sqlx::query(
        "INSERT INTO successor_creation_receipts_v1 \
         (namespace_id, account_id, receipt_id, mutation_id, death_id, successor_id, \
          initializer_revision, initializer_request_hash, initializer_result_hash, weapon_uid, \
          relic_uid, tonic_uid_0, tonic_uid_1, item_count, item_content_revision) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,4,$14)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.receipt_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(request.death_id.as_slice())
    .bind(request.successor_id.as_slice())
    .bind(STARTER_INITIALIZER_REVISION)
    .bind(request.starter_request_hash.as_slice())
    .bind(request.starter_result_hash.as_slice())
    .bind(weapon_uid.as_slice())
    .bind(relic_uid.as_slice())
    .bind(tonic_uid_0.as_slice())
    .bind(tonic_uid_1.as_slice())
    .bind(CORE_ITEM_CONTENT_REVISION)
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_audit_and_outbox(
    connection: &mut PgConnection,
    request: &SuccessorCreateRequestV1,
    result: &StoredSuccessorResultV1,
    result_payload: &[u8],
) -> Result<(), PersistenceError> {
    let parts = [
        request.account_id.as_slice(),
        request.death_id.as_slice(),
        request.mutation_id.as_slice(),
        request.successor_id.as_slice(),
    ];
    let audit_id = derive_id(SUCCESSOR_AUDIT_ID_CONTEXT_V1, &parts);
    let outbox_id = derive_id(SUCCESSOR_OUTBOX_ID_CONTEXT_V1, &parts);
    sqlx::query(
        "INSERT INTO successor_mutation_audit_events_v1 \
         (namespace_id, account_id, death_id, mutation_id, successor_id, event_id, event_type, \
          event_digest) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.death_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(request.successor_id.as_slice())
    .bind(audit_id.as_slice())
    .bind(SUCCESSOR_CREATED_EVENT_TYPE)
    .bind(result.result_hash.as_slice())
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "INSERT INTO successor_mutation_outbox_events_v1 \
         (namespace_id, account_id, death_id, mutation_id, successor_id, receipt_id, event_id, \
          event_type, event_payload) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.death_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(request.successor_id.as_slice())
    .bind(request.receipt_id.as_slice())
    .bind(outbox_id.as_slice())
    .bind(SUCCESSOR_CREATED_EVENT_TYPE)
    .bind(result_payload)
    .execute(connection)
    .await?;
    Ok(())
}

async fn consume_reservation(
    connection: &mut PgConnection,
    request: &SuccessorCreateRequestV1,
) -> Result<(), PersistenceError> {
    let updated = sqlx::query(
        "UPDATE successor_roster_reservations_v1 \
         SET reservation_state=1, consumed_mutation_id=$1, consumed_successor_id=$2, \
             consumed_receipt_id=$3, consumed_at=transaction_timestamp() \
         WHERE namespace_id=$4 AND account_id=$5 AND death_id=$6 AND reservation_state=0",
    )
    .bind(request.mutation_id.as_slice())
    .bind(request.successor_id.as_slice())
    .bind(request.receipt_id.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.death_id.as_slice())
    .execute(connection)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(PersistenceError::SuccessorAlreadyConsumed);
    }
    Ok(())
}

async fn insert_conflict_audit(
    connection: &mut PgConnection,
    stored: &StoredSuccessorResultV1,
    request: &SuccessorCreateRequestV1,
) -> Result<(), PersistenceError> {
    let digest = conflict_digest(stored, request);
    sqlx::query(
        "INSERT INTO successor_mutation_conflict_audits_v1 \
         (namespace_id, account_id, mutation_id, incoming_death_id, stored_request_hash, \
          incoming_request_hash, conflict_digest) VALUES ($1,$2,$3,$4,$5,$6,$7) \
         ON CONFLICT DO NOTHING",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(request.death_id.as_slice())
    .bind(stored.canonical_request_hash.as_slice())
    .bind(request.canonical_request_hash.as_slice())
    .bind(digest.as_slice())
    .execute(connection)
    .await?;
    Ok(())
}

fn conflict_digest(
    stored: &StoredSuccessorResultV1,
    request: &SuccessorCreateRequestV1,
) -> [u8; HASH_BYTES] {
    blake3::derive_key(
        SUCCESSOR_CONFLICT_DIGEST_CONTEXT_V1,
        &[
            stored.canonical_request_hash.as_slice(),
            request.canonical_request_hash.as_slice(),
            request.death_id.as_slice(),
        ]
        .concat(),
    )
}

async fn force_deferred_constraints(connection: &mut PgConnection) -> Result<(), PersistenceError> {
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(connection)
        .await?;
    Ok(())
}

fn derive_id(context: &str, parts: &[&[u8]]) -> [u8; ID_BYTES] {
    let mut material = Vec::new();
    for part in parts {
        let length = u32::try_from(part.len()).expect("successor child identity input is bounded");
        material.extend_from_slice(&length.to_le_bytes());
        material.extend_from_slice(part);
    }
    let digest = blake3::derive_key(context, &material);
    let mut id = [0_u8; ID_BYTES];
    id.copy_from_slice(&digest[..ID_BYTES]);
    id
}

fn exact_id(value: Vec<u8>) -> Result<[u8; ID_BYTES], PersistenceError> {
    value
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredSuccessor)
}

fn exact_hash(value: Vec<u8>) -> Result<[u8; HASH_BYTES], PersistenceError> {
    value
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredSuccessor)
}

fn positive_u64(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(PersistenceError::CorruptStoredSuccessor)
}

fn positive_u8(value: i16) -> Result<u8, PersistenceError> {
    u8::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(PersistenceError::CorruptStoredSuccessor)
}

fn nonnegative_u16(value: i16) -> Result<u16, PersistenceError> {
    u16::try_from(value).map_err(|_| PersistenceError::CorruptStoredSuccessor)
}

fn to_i64(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| PersistenceError::CorruptStoredSuccessor)
}

fn corrupt() -> PersistenceError {
    PersistenceError::CorruptStoredSuccessor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_ids_are_domain_separated_and_stable() {
        let parts = [&[1_u8; ID_BYTES][..], &[2_u8; ID_BYTES][..]];
        let audit = derive_id(SUCCESSOR_AUDIT_ID_CONTEXT_V1, &parts);
        let outbox = derive_id(SUCCESSOR_OUTBOX_ID_CONTEXT_V1, &parts);
        assert_ne!(audit, [0; ID_BYTES]);
        assert_ne!(audit, outbox);
        assert_eq!(audit, derive_id(SUCCESSOR_AUDIT_ID_CONTEXT_V1, &parts));
    }
}
