//! Serializable query and replay-first writer for M03 `ResolutionHold` recovery.

use std::collections::BTreeMap;

use sqlx::{PgConnection, Row};

use crate::{
    CORE_ITEM_CONTENT_REVISION, MAX_RESOLUTION_HOLD_STACKS_V1, PersistenceError,
    PostgresPersistence, RESOLUTION_HOLD_ACCEPTED_AUDIT_ID_CONTEXT_V1,
    RESOLUTION_HOLD_CONTRACT_VERSION_V1, RESOLUTION_HOLD_HASH_BYTES,
    RESOLUTION_HOLD_ITEM_LEDGER_ID_CONTEXT_V1, RESOLUTION_HOLD_OUTBOX_ID_CONTEXT_V1,
    RESOLUTION_HOLD_OVERFLOW_LIFETIME_MILLIS, ResolutionHoldMutationRequestV1,
    ResolutionHoldMutationTransactionV1, ResolutionHoldStorageSnapshotV1,
    ResolutionHoldStorageStackV1, StoredResolutionHoldActionV1, StoredResolutionHoldDestinationV1,
    StoredResolutionHoldDispositionV1, StoredResolutionHoldItemKindV1,
    StoredResolutionHoldItemTransitionV1, StoredResolutionHoldItemV1,
    StoredResolutionHoldMutationResultV1, StoredResolutionHoldSnapshotV1,
    StoredResolutionHoldStackV1, StoredResolutionHoldVersionAdvanceV1,
    StoredResolutionHoldVersionVectorV1, StoredResolutionHoldVersionsV1, WIPEABLE_CORE_NAMESPACE,
    canonical_resolution_hold_conflict_digest_v1, canonical_resolution_hold_stack_digest_v1,
    derive_resolution_hold_id_v1, is_retryable_transaction_failure,
    plan_resolution_hold_destination_v1,
};

const ID_BYTES: usize = 16;
const MAX_TRANSACTION_ATTEMPTS: u8 = 3;
const LIFE_LIVING: i16 = 0;
const SECURITY_NORMAL: i16 = 0;
const SECURITY_STORAGE_RESOLUTION_REQUIRED: i16 = 1;
const ITEM_SECURITY_SAFE: i16 = 0;
const LOCATION_CHARACTER_SAFE: i16 = 5;
const LOCATION_VAULT: i16 = 6;
const LOCATION_OVERFLOW: i16 = 8;
const LOCATION_RESOLUTION_HOLD: i16 = 9;
const LOCATION_DESTROYED: i16 = 4;
const LOCATION_HALL: i16 = 1;
const LANTERN_HALLS_CONTENT_ID: &str = "hub.lantern_halls_01";
const ITEM_SECURITY_DESTROYED: i16 = 3;
const RESOLUTION_HOLD_LEDGER_SOURCE_KIND: i16 = 7;
const RESOLUTION_HOLD_DESTROY_REASON: &str = "resolution_hold_destroyed";

#[derive(Debug, Clone, Copy)]
struct LockedHoldAccount {
    state_version: u64,
    selected_character_id: Option<[u8; ID_BYTES]>,
}

#[derive(Debug, Clone, Copy)]
struct LockedHoldAuthority {
    account_version: u64,
    character_version: u64,
    world_version: u64,
    inventory_version: u64,
    security_state: i16,
}

#[derive(Debug, Clone)]
struct LockedHoldItemRow {
    item_uid: [u8; ID_BYTES],
    account_id: [u8; ID_BYTES],
    character_id: Option<[u8; ID_BYTES]>,
    template_id: String,
    content_revision: String,
    item_kind: StoredResolutionHoldItemKindV1,
    item_version: u64,
    security_state: i16,
    location_kind: i16,
    slot_index: u16,
    destruction_reason: Option<String>,
    terminal_extraction_id: Option<[u8; ID_BYTES]>,
    extracted_at_unix_millis: Option<u64>,
    overflow_deadline_unix_millis: Option<u64>,
    placement_account_id: Option<[u8; ID_BYTES]>,
    placement_character_id: Option<[u8; ID_BYTES]>,
    placement_template_id: Option<String>,
    placement_item_kind: Option<i16>,
    placement_destination_kind: Option<i16>,
    placement_destination_slot_index: Option<u16>,
    placement_post_item_version: Option<u64>,
    placement_post_security_state: Option<i16>,
    extraction_account_id: Option<[u8; ID_BYTES]>,
    extraction_character_id: Option<[u8; ID_BYTES]>,
    extraction_committed_at_unix_millis: Option<u64>,
}

#[derive(Debug)]
struct LogicalStackBuilder {
    template_id: String,
    content_revision: String,
    item_kind: StoredResolutionHoldItemKindV1,
    extracted_at_unix_millis: u64,
    items: Vec<StoredResolutionHoldItemV1>,
}

#[derive(Debug)]
struct StorageStackBuilder {
    template_id: String,
    content_revision: String,
    item_kind: StoredResolutionHoldItemKindV1,
    items: Vec<StoredResolutionHoldItemV1>,
}

type HoldGroups = BTreeMap<([u8; ID_BYTES], u8), LogicalStackBuilder>;
type StorageGroups = BTreeMap<(i16, u16), StorageStackBuilder>;

impl PostgresPersistence {
    /// Loads one bounded server-authoritative Hold projection from a serializable locked snapshot.
    ///
    /// The read never reconstructs provenance from mutable item state alone. Every held UID must
    /// match its immutable extraction placement/result before the stack is published.
    pub async fn load_resolution_hold_snapshot_v1(
        &self,
        account_id: [u8; ID_BYTES],
        character_id: [u8; ID_BYTES],
    ) -> Result<StoredResolutionHoldSnapshotV1, PersistenceError> {
        if account_id == [0; ID_BYTES]
            || character_id == [0; ID_BYTES]
            || account_id == character_id
        {
            return Err(PersistenceError::CorruptStoredResolutionHold);
        }
        for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self
                .load_resolution_hold_snapshot_once_v1(account_id, character_id)
                .await
            {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS
                        && is_retryable_transaction_failure(&error) => {}
                outcome => return outcome,
            }
        }
        Err(PersistenceError::ResolutionHoldUnresolvedMutation)
    }

    async fn load_resolution_hold_snapshot_once_v1(
        &self,
        account_id: [u8; ID_BYTES],
        character_id: [u8; ID_BYTES],
    ) -> Result<StoredResolutionHoldSnapshotV1, PersistenceError> {
        let mut transaction = self.begin_transaction().await?;
        let authority =
            lock_hold_authority(transaction.connection(), account_id, character_id).await?;
        let rows =
            lock_hold_and_storage_items(transaction.connection(), account_id, character_id).await?;
        let authoritative_time_unix_millis =
            transaction_timestamp_millis(transaction.connection()).await?;
        let snapshot = assemble_resolution_hold_snapshot(
            account_id,
            character_id,
            authority,
            rows,
            authoritative_time_unix_millis,
        )?;
        transaction.rollback().await?;
        Ok(snapshot)
    }
}

impl PostgresPersistence {
    /// Commits one whole-stack Move or final confirmed destruction with replay checked before
    /// current aggregate validation.
    pub async fn commit_resolution_hold_mutation_v1(
        &self,
        request: &ResolutionHoldMutationRequestV1,
    ) -> Result<ResolutionHoldMutationTransactionV1, PersistenceError> {
        request.validate()?;
        for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self.commit_resolution_hold_mutation_once_v1(request).await {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS
                        && is_retryable_transaction_failure(&error) => {}
                outcome => return outcome,
            }
        }
        Err(PersistenceError::ResolutionHoldUnresolvedMutation)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the one-writer transaction keeps replay, mutation, projection, and publication order auditable"
    )]
    async fn commit_resolution_hold_mutation_once_v1(
        &self,
        request: &ResolutionHoldMutationRequestV1,
    ) -> Result<ResolutionHoldMutationTransactionV1, PersistenceError> {
        let request_hash = request.canonical_hash()?;
        let mut transaction = self.begin_transaction().await?;
        let account = lock_hold_account(transaction.connection(), request.account_id).await?;
        if let Some(stored) = load_existing_hold_result(
            transaction.connection(),
            request.account_id,
            request.mutation_id,
        )
        .await?
        {
            if exact_hold_replay(&stored, request, request_hash) {
                transaction.rollback().await?;
                return Ok(ResolutionHoldMutationTransactionV1::Replayed(stored));
            }
            if stored.canonical_request_hash == request_hash {
                transaction.rollback().await?;
                return Err(PersistenceError::CorruptStoredResolutionHold);
            }
            insert_hold_conflict_audit(transaction.connection(), &stored, request_hash).await?;
            transaction.commit().await?;
            return Ok(ResolutionHoldMutationTransactionV1::Conflict {
                mutation_id: stored.mutation_id,
                character_id: stored.character_id,
            });
        }

        let authority = lock_hold_authority_after_account(
            transaction.connection(),
            request.account_id,
            request.character_id,
            account,
        )
        .await?;
        validate_expected_hold_versions(authority, request)?;
        let rows = lock_hold_and_storage_items(
            transaction.connection(),
            request.account_id,
            request.character_id,
        )
        .await?;
        let committed_at_unix_millis =
            transaction_timestamp_millis(transaction.connection()).await?;
        if request.issued_at_unix_millis > committed_at_unix_millis {
            transaction.rollback().await?;
            return Err(PersistenceError::ResolutionHoldIssuedAtInvalid);
        }
        let snapshot = assemble_resolution_hold_snapshot(
            request.account_id,
            request.character_id,
            authority,
            rows,
            committed_at_unix_millis,
        )?;
        let result =
            build_hold_mutation_result(request, request_hash, &snapshot, committed_at_unix_millis)?;
        let result_payload = result.encode()?;

        insert_hold_result_root(transaction.connection(), request, &result, &result_payload)
            .await?;
        apply_hold_item_transitions(transaction.connection(), request, &result).await?;
        apply_hold_aggregate_heads(transaction.connection(), request, &result).await?;
        insert_hold_audit_and_outbox(transaction.connection(), request, &result, &result_payload)
            .await?;
        force_hold_deferred_constraints(transaction.connection()).await?;
        transaction.commit().await?;
        Ok(ResolutionHoldMutationTransactionV1::Fresh(result))
    }
}

fn exact_hold_replay(
    stored: &StoredResolutionHoldMutationResultV1,
    request: &ResolutionHoldMutationRequestV1,
    request_hash: [u8; RESOLUTION_HOLD_HASH_BYTES],
) -> bool {
    stored.canonical_request_hash == request_hash
        && stored.account_id == request.account_id
        && stored.character_id == request.character_id
        && stored.mutation_id == request.mutation_id
        && stored.extraction_id == request.extraction_id
        && stored.stack_index == request.stack_index
        && stored.action == request.action
        && stored.expected_stack_digest == request.expected_stack_digest
        && stored.issued_at_unix_millis == request.issued_at_unix_millis
        && stored.versions.account.pre == request.expected_versions.account
        && stored.versions.character.pre == request.expected_versions.character
        && stored.versions.world.pre == request.expected_versions.world
        && stored.versions.inventory.pre == request.expected_versions.inventory
}

fn validate_expected_hold_versions(
    authority: LockedHoldAuthority,
    request: &ResolutionHoldMutationRequestV1,
) -> Result<(), PersistenceError> {
    if authority.account_version != request.expected_versions.account
        || authority.character_version != request.expected_versions.character
        || authority.world_version != request.expected_versions.world
        || authority.inventory_version != request.expected_versions.inventory
    {
        return Err(PersistenceError::ResolutionHoldVersionMismatch {
            account: authority.account_version,
            character: authority.character_version,
            world: authority.world_version,
            inventory: authority.inventory_version,
        });
    }
    Ok(())
}

fn build_hold_mutation_result(
    request: &ResolutionHoldMutationRequestV1,
    request_hash: [u8; RESOLUTION_HOLD_HASH_BYTES],
    snapshot: &StoredResolutionHoldSnapshotV1,
    committed_at_unix_millis: u64,
) -> Result<StoredResolutionHoldMutationResultV1, PersistenceError> {
    let selected = snapshot
        .stacks
        .iter()
        .find(|stack| {
            stack.extraction_id == request.extraction_id && stack.stack_index == request.stack_index
        })
        .ok_or(PersistenceError::ResolutionHoldStackNotFound)?;
    if selected.content_revision != request.content_revision {
        return Err(PersistenceError::ResolutionHoldContentMismatch);
    }
    if selected.stack_digest != request.expected_stack_digest {
        return Err(PersistenceError::ResolutionHoldStackDigestMismatch);
    }
    let destination = match request.action {
        StoredResolutionHoldActionV1::Move => Some(
            selected
                .planned_destination
                .ok_or(PersistenceError::ResolutionHoldStorageFull)?,
        ),
        StoredResolutionHoldActionV1::DestroyConfirmed => None,
    };
    let remaining_hold_stack_count = snapshot
        .stacks
        .len()
        .checked_sub(1)
        .and_then(|count| u8::try_from(count).ok())
        .ok_or(PersistenceError::CorruptStoredResolutionHold)?;
    let final_clear = remaining_hold_stack_count == 0;
    let account_post = if destination.is_some_and(StoredResolutionHoldDestinationV1::account_owned)
    {
        advance_version(snapshot.versions.account)?
    } else {
        snapshot.versions.account
    };
    let character_post = if final_clear {
        advance_version(snapshot.versions.character)?
    } else {
        snapshot.versions.character
    };
    let world_post = if final_clear {
        advance_version(snapshot.versions.world)?
    } else {
        snapshot.versions.world
    };
    let disposition = destination.map_or(
        StoredResolutionHoldDispositionV1::Destroyed,
        StoredResolutionHoldDispositionV1::Moved,
    );
    let transitions = build_hold_item_transitions(request, selected, disposition)?;
    StoredResolutionHoldMutationResultV1 {
        contract_version: RESOLUTION_HOLD_CONTRACT_VERSION_V1,
        namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
        account_id: request.account_id,
        character_id: request.character_id,
        mutation_id: request.mutation_id,
        extraction_id: request.extraction_id,
        stack_index: request.stack_index,
        action: request.action,
        canonical_request_hash: request_hash,
        expected_stack_digest: request.expected_stack_digest,
        result_hash: [0; RESOLUTION_HOLD_HASH_BYTES],
        issued_at_unix_millis: request.issued_at_unix_millis,
        committed_at_unix_millis,
        versions: StoredResolutionHoldVersionVectorV1 {
            account: StoredResolutionHoldVersionAdvanceV1 {
                pre: snapshot.versions.account,
                post: account_post,
            },
            character: StoredResolutionHoldVersionAdvanceV1 {
                pre: snapshot.versions.character,
                post: character_post,
            },
            world: StoredResolutionHoldVersionAdvanceV1 {
                pre: snapshot.versions.world,
                post: world_post,
            },
            inventory: StoredResolutionHoldVersionAdvanceV1 {
                pre: snapshot.versions.inventory,
                post: advance_version(snapshot.versions.inventory)?,
            },
        },
        destination,
        transitions,
        remaining_hold_stack_count,
        storage_resolution_required: !final_clear,
    }
    .seal()
}

fn build_hold_item_transitions(
    request: &ResolutionHoldMutationRequestV1,
    selected: &StoredResolutionHoldStackV1,
    disposition: StoredResolutionHoldDispositionV1,
) -> Result<Vec<StoredResolutionHoldItemTransitionV1>, PersistenceError> {
    let transitions = selected
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let ordinal =
                u8::try_from(index).map_err(|_| PersistenceError::CorruptStoredResolutionHold)?;
            let ledger_event_id = derive_resolution_hold_id_v1(
                RESOLUTION_HOLD_ITEM_LEDGER_ID_CONTEXT_V1,
                &[
                    request.account_id.as_slice(),
                    request.character_id.as_slice(),
                    request.mutation_id.as_slice(),
                    request.extraction_id.as_slice(),
                    item.item_uid.as_slice(),
                ],
            );
            Ok(StoredResolutionHoldItemTransitionV1 {
                ordinal,
                item_uid: item.item_uid,
                template_id: selected.template_id.clone(),
                content_revision: selected.content_revision.clone(),
                item_kind: selected.item_kind,
                disposition,
                pre_item_version: item.item_version,
                post_item_version: advance_version(item.item_version)?,
                ledger_event_id,
            })
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    Ok(transitions)
}

fn advance_version(value: u64) -> Result<u64, PersistenceError> {
    value
        .checked_add(1)
        .ok_or(PersistenceError::CorruptStoredResolutionHold)
}

#[allow(
    clippy::too_many_lines,
    reason = "replay validation compares every normalized root and transition field before returning stored authority"
)]
async fn load_existing_hold_result(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    mutation_id: [u8; ID_BYTES],
) -> Result<Option<StoredResolutionHoldMutationResultV1>, PersistenceError> {
    let Some(row) = sqlx::query(
        "SELECT account_id,character_id,mutation_id,extraction_id,stack_index,
                contract_version,action_kind,canonical_request_hash,expected_stack_digest,
                result_hash,result_payload,content_revision,
                floor(extract(epoch FROM issued_at) * 1000)::bigint AS issued_at_unix_millis,
                floor(extract(epoch FROM committed_at) * 1000)::bigint
                    AS committed_at_unix_millis,
                pre_account_version,post_account_version,pre_character_version,
                post_character_version,pre_world_version,post_world_version,
                pre_inventory_version,post_inventory_version,destination_kind,
                destination_slot_index,transition_count,remaining_hold_stack_count,
                storage_resolution_required
         FROM resolution_hold_mutation_results_v1
         WHERE namespace_id=$1 AND account_id=$2 AND mutation_id=$3
         FOR SHARE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(mutation_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(None);
    };
    let payload: Vec<u8> = row.try_get("result_payload")?;
    let stored = StoredResolutionHoldMutationResultV1::decode(&payload)?;
    let durable_action =
        StoredResolutionHoldActionV1::try_from_durable_kind(row.try_get("action_kind")?)?;
    let destination_kind: Option<i16> = row.try_get("destination_kind")?;
    let destination_slot: Option<u16> = optional_u16(row.try_get("destination_slot_index")?)?;
    let durable_destination = match (destination_kind, destination_slot) {
        (Some(kind), Some(slot)) => Some(StoredResolutionHoldDestinationV1::try_from_durable(
            kind, slot,
        )?),
        (None, None) => None,
        _ => return Err(PersistenceError::CorruptStoredResolutionHold),
    };
    let row_matches = exact_id(row.try_get("account_id")?)? == stored.account_id
        && exact_id(row.try_get("character_id")?)? == stored.character_id
        && exact_id(row.try_get("mutation_id")?)? == stored.mutation_id
        && exact_id(row.try_get("extraction_id")?)? == stored.extraction_id
        && u8_value(row.try_get("stack_index")?)? == stored.stack_index
        && u16_value(row.try_get("contract_version")?)? == stored.contract_version
        && durable_action == stored.action
        && exact_hash(row.try_get("canonical_request_hash")?)? == stored.canonical_request_hash
        && exact_hash(row.try_get("expected_stack_digest")?)? == stored.expected_stack_digest
        && exact_hash(row.try_get("result_hash")?)? == stored.result_hash
        && row.try_get::<String, _>("content_revision")? == CORE_ITEM_CONTENT_REVISION
        && positive(row.try_get("issued_at_unix_millis")?)? == stored.issued_at_unix_millis
        && positive(row.try_get("committed_at_unix_millis")?)? == stored.committed_at_unix_millis
        && positive(row.try_get("pre_account_version")?)? == stored.versions.account.pre
        && positive(row.try_get("post_account_version")?)? == stored.versions.account.post
        && positive(row.try_get("pre_character_version")?)? == stored.versions.character.pre
        && positive(row.try_get("post_character_version")?)? == stored.versions.character.post
        && positive(row.try_get("pre_world_version")?)? == stored.versions.world.pre
        && positive(row.try_get("post_world_version")?)? == stored.versions.world.post
        && positive(row.try_get("pre_inventory_version")?)? == stored.versions.inventory.pre
        && positive(row.try_get("post_inventory_version")?)? == stored.versions.inventory.post
        && durable_destination == stored.destination
        && usize::from(u16_value(row.try_get("transition_count")?)?) == stored.transitions.len()
        && u8_value(row.try_get("remaining_hold_stack_count")?)?
            == stored.remaining_hold_stack_count
        && row.try_get::<bool, _>("storage_resolution_required")?
            == stored.storage_resolution_required;
    if !row_matches || stored.account_id != account_id || stored.mutation_id != mutation_id {
        return Err(PersistenceError::CorruptStoredResolutionHold);
    }

    let transition_rows = sqlx::query(
        "SELECT transition_ordinal,item_uid,template_id,content_revision,item_kind,
                disposition_kind,source_kind,source_slot_index,destination_kind,
                destination_slot_index,pre_item_version,post_item_version,
                pre_security_state,post_security_state,destruction_reason,
                ledger_event_id,ledger_event_kind,ledger_source_kind
         FROM resolution_hold_item_transitions_v1
         WHERE namespace_id=$1 AND account_id=$2 AND mutation_id=$3
         ORDER BY transition_ordinal FOR SHARE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(mutation_id.as_slice())
    .fetch_all(connection)
    .await?;
    if transition_rows.len() != stored.transitions.len() {
        return Err(PersistenceError::CorruptStoredResolutionHold);
    }
    for (row, transition) in transition_rows.iter().zip(&stored.transitions) {
        let (destination_kind, destination_slot, post_security, destruction_reason, ledger_kind) =
            match transition.disposition {
                StoredResolutionHoldDispositionV1::Moved(destination) => (
                    destination.durable_kind(),
                    Some(destination.slot_index()),
                    ITEM_SECURITY_SAFE,
                    None,
                    1_i16,
                ),
                StoredResolutionHoldDispositionV1::Destroyed => (
                    LOCATION_DESTROYED,
                    None,
                    ITEM_SECURITY_DESTROYED,
                    Some(RESOLUTION_HOLD_DESTROY_REASON),
                    2_i16,
                ),
            };
        if u8_value(row.try_get("transition_ordinal")?)? != transition.ordinal
            || exact_id(row.try_get("item_uid")?)? != transition.item_uid
            || row.try_get::<String, _>("template_id")? != transition.template_id
            || row.try_get::<String, _>("content_revision")? != transition.content_revision
            || row.try_get::<i16, _>("item_kind")? != transition.item_kind.durable_kind()
            || row.try_get::<i16, _>("disposition_kind")? != stored.action.durable_kind()
            || row.try_get::<i16, _>("source_kind")? != LOCATION_RESOLUTION_HOLD
            || u8_value(row.try_get("source_slot_index")?)? != stored.stack_index
            || row.try_get::<i16, _>("destination_kind")? != destination_kind
            || optional_u16(row.try_get("destination_slot_index")?)? != destination_slot
            || positive(row.try_get("pre_item_version")?)? != transition.pre_item_version
            || positive(row.try_get("post_item_version")?)? != transition.post_item_version
            || row.try_get::<i16, _>("pre_security_state")? != ITEM_SECURITY_SAFE
            || row.try_get::<i16, _>("post_security_state")? != post_security
            || row
                .try_get::<Option<String>, _>("destruction_reason")?
                .as_deref()
                != destruction_reason
            || exact_id(row.try_get("ledger_event_id")?)? != transition.ledger_event_id
            || row.try_get::<i16, _>("ledger_event_kind")? != ledger_kind
            || row.try_get::<i16, _>("ledger_source_kind")? != RESOLUTION_HOLD_LEDGER_SOURCE_KIND
        {
            return Err(PersistenceError::CorruptStoredResolutionHold);
        }
    }
    Ok(Some(stored))
}

async fn insert_hold_conflict_audit(
    connection: &mut PgConnection,
    stored: &StoredResolutionHoldMutationResultV1,
    incoming_request_hash: [u8; RESOLUTION_HOLD_HASH_BYTES],
) -> Result<(), PersistenceError> {
    let conflict_digest = canonical_resolution_hold_conflict_digest_v1(
        stored.account_id,
        stored.mutation_id,
        stored.canonical_request_hash,
        incoming_request_hash,
    )?;
    sqlx::query(
        "INSERT INTO resolution_hold_mutation_conflict_audits_v1
         (namespace_id,account_id,mutation_id,stored_request_hash,
          incoming_request_hash,conflict_digest)
         VALUES ($1,$2,$3,$4,$5,$6)
         ON CONFLICT DO NOTHING",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(stored.account_id.as_slice())
    .bind(stored.mutation_id.as_slice())
    .bind(stored.canonical_request_hash.as_slice())
    .bind(incoming_request_hash.as_slice())
    .bind(conflict_digest.as_slice())
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_hold_result_root(
    connection: &mut PgConnection,
    request: &ResolutionHoldMutationRequestV1,
    result: &StoredResolutionHoldMutationResultV1,
    result_payload: &[u8],
) -> Result<(), PersistenceError> {
    let destination_kind = result
        .destination
        .map(StoredResolutionHoldDestinationV1::durable_kind);
    let destination_slot_index = result
        .destination
        .map(StoredResolutionHoldDestinationV1::slot_index)
        .map(i16_value)
        .transpose()?;
    let affected = sqlx::query(
        "INSERT INTO resolution_hold_mutation_results_v1
         (namespace_id,account_id,character_id,mutation_id,extraction_id,stack_index,
          contract_version,action_kind,canonical_request_hash,expected_stack_digest,
          result_hash,result_payload,content_revision,issued_at,
          pre_account_version,post_account_version,pre_character_version,
          post_character_version,pre_world_version,post_world_version,
          pre_inventory_version,post_inventory_version,destination_kind,
          destination_slot_index,transition_count,remaining_hold_stack_count,
          storage_resolution_required)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,
                 to_timestamp($14::double precision/1000.0),
                 $15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(request.extraction_id.as_slice())
    .bind(i16::from(request.stack_index))
    .bind(i16_value(result.contract_version)?)
    .bind(result.action.durable_kind())
    .bind(result.canonical_request_hash.as_slice())
    .bind(result.expected_stack_digest.as_slice())
    .bind(result.result_hash.as_slice())
    .bind(result_payload)
    .bind(&request.content_revision)
    .bind(i64_value(result.issued_at_unix_millis)?)
    .bind(i64_value(result.versions.account.pre)?)
    .bind(i64_value(result.versions.account.post)?)
    .bind(i64_value(result.versions.character.pre)?)
    .bind(i64_value(result.versions.character.post)?)
    .bind(i64_value(result.versions.world.pre)?)
    .bind(i64_value(result.versions.world.post)?)
    .bind(i64_value(result.versions.inventory.pre)?)
    .bind(i64_value(result.versions.inventory.post)?)
    .bind(destination_kind)
    .bind(destination_slot_index)
    .bind(
        i16::try_from(result.transitions.len())
            .map_err(|_| PersistenceError::CorruptStoredResolutionHold)?,
    )
    .bind(i16::from(result.remaining_hold_stack_count))
    .bind(result.storage_resolution_required)
    .execute(connection)
    .await?
    .rows_affected();
    expect_one_hold(affected)
}

async fn apply_hold_item_transitions(
    connection: &mut PgConnection,
    request: &ResolutionHoldMutationRequestV1,
    result: &StoredResolutionHoldMutationResultV1,
) -> Result<(), PersistenceError> {
    for transition in &result.transitions {
        let (destination_kind, destination_slot, post_security, reason, ledger_kind) =
            match transition.disposition {
                StoredResolutionHoldDispositionV1::Moved(destination) => {
                    update_moved_hold_item(connection, request, transition, destination).await?;
                    (
                        destination.durable_kind(),
                        Some(destination.slot_index()),
                        ITEM_SECURITY_SAFE,
                        None,
                        1_i16,
                    )
                }
                StoredResolutionHoldDispositionV1::Destroyed => {
                    update_destroyed_hold_item(connection, request, transition).await?;
                    (
                        LOCATION_DESTROYED,
                        None,
                        ITEM_SECURITY_DESTROYED,
                        Some(RESOLUTION_HOLD_DESTROY_REASON),
                        2_i16,
                    )
                }
            };
        insert_hold_item_ledger(
            connection,
            request,
            transition,
            destination_kind,
            post_security,
            reason,
            ledger_kind,
        )
        .await?;
        insert_hold_item_transition(
            connection,
            request,
            transition,
            destination_kind,
            destination_slot,
            post_security,
            reason,
            ledger_kind,
        )
        .await?;
    }
    Ok(())
}

async fn update_moved_hold_item(
    connection: &mut PgConnection,
    request: &ResolutionHoldMutationRequestV1,
    transition: &StoredResolutionHoldItemTransitionV1,
    destination: StoredResolutionHoldDestinationV1,
) -> Result<(), PersistenceError> {
    let character_id = if matches!(
        destination,
        StoredResolutionHoldDestinationV1::CharacterSafe(_)
    ) {
        Some(request.character_id.as_slice())
    } else {
        None
    };
    let affected = sqlx::query(
        "UPDATE item_instances SET character_id=$1,item_version=$2,security_state=0,
                location_kind=$3,slot_index=$4,instance_id=NULL,pickup_id=NULL,
                expires_at_tick=NULL,destruction_reason=NULL,
                overflow_expires_at=CASE WHEN $3=8
                    THEN extracted_at+INTERVAL '72 hours' ELSE NULL END,
                updated_at=transaction_timestamp()
         WHERE namespace_id=$5 AND account_id=$6 AND character_id=$7 AND item_uid=$8
           AND template_id=$9 AND content_revision=$10 AND item_kind=$11
           AND item_version=$12 AND security_state=0 AND location_kind=9
           AND slot_index=$13 AND terminal_extraction_id=$14
           AND extracted_at IS NOT NULL AND overflow_expires_at IS NULL
           AND destruction_reason IS NULL AND terminal_recall_id IS NULL",
    )
    .bind(character_id)
    .bind(i64_value(transition.post_item_version)?)
    .bind(destination.durable_kind())
    .bind(i16_value(destination.slot_index())?)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(transition.item_uid.as_slice())
    .bind(&transition.template_id)
    .bind(&transition.content_revision)
    .bind(transition.item_kind.durable_kind())
    .bind(i64_value(transition.pre_item_version)?)
    .bind(i16::from(request.stack_index))
    .bind(request.extraction_id.as_slice())
    .execute(connection)
    .await?
    .rows_affected();
    expect_one_hold(affected)
}

async fn update_destroyed_hold_item(
    connection: &mut PgConnection,
    request: &ResolutionHoldMutationRequestV1,
    transition: &StoredResolutionHoldItemTransitionV1,
) -> Result<(), PersistenceError> {
    let affected = sqlx::query(
        "UPDATE item_instances SET item_version=$1,security_state=3,location_kind=4,
                slot_index=NULL,instance_id=NULL,pickup_id=NULL,expires_at_tick=NULL,
                destruction_reason='resolution_hold_destroyed',overflow_expires_at=NULL,
                updated_at=transaction_timestamp()
         WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4 AND item_uid=$5
           AND template_id=$6 AND content_revision=$7 AND item_kind=$8
           AND item_version=$9 AND security_state=0 AND location_kind=9
           AND slot_index=$10 AND terminal_extraction_id=$11
           AND extracted_at IS NOT NULL AND overflow_expires_at IS NULL
           AND destruction_reason IS NULL AND terminal_recall_id IS NULL",
    )
    .bind(i64_value(transition.post_item_version)?)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(transition.item_uid.as_slice())
    .bind(&transition.template_id)
    .bind(&transition.content_revision)
    .bind(transition.item_kind.durable_kind())
    .bind(i64_value(transition.pre_item_version)?)
    .bind(i16::from(request.stack_index))
    .bind(request.extraction_id.as_slice())
    .execute(connection)
    .await?
    .rows_affected();
    expect_one_hold(affected)
}

#[allow(
    clippy::too_many_arguments,
    reason = "the normalized ledger mirrors every transition axis explicitly"
)]
async fn insert_hold_item_ledger(
    connection: &mut PgConnection,
    request: &ResolutionHoldMutationRequestV1,
    transition: &StoredResolutionHoldItemTransitionV1,
    destination_kind: i16,
    post_security: i16,
    reason: Option<&str>,
    ledger_kind: i16,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO item_ledger_events
         (namespace_id,ledger_event_id,item_uid,account_id,character_id,mutation_id,
          event_kind,source_kind,pre_item_version,post_item_version,pre_security_state,
          post_security_state,pre_location_kind,post_location_kind,reason,
          terminal_extraction_id)
         VALUES ($1,$2,$3,$4,$5,$6,$7,7,$8,$9,0,$10,9,$11,$12,$13)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(transition.ledger_event_id.as_slice())
    .bind(transition.item_uid.as_slice())
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(ledger_kind)
    .bind(i64_value(transition.pre_item_version)?)
    .bind(i64_value(transition.post_item_version)?)
    .bind(post_security)
    .bind(destination_kind)
    .bind(reason)
    .bind(request.extraction_id.as_slice())
    .execute(connection)
    .await?;
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "the normalized projection mirrors every transition axis explicitly"
)]
async fn insert_hold_item_transition(
    connection: &mut PgConnection,
    request: &ResolutionHoldMutationRequestV1,
    transition: &StoredResolutionHoldItemTransitionV1,
    destination_kind: i16,
    destination_slot: Option<u16>,
    post_security: i16,
    reason: Option<&str>,
    ledger_kind: i16,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO resolution_hold_item_transitions_v1
         (namespace_id,account_id,character_id,mutation_id,extraction_id,stack_index,
          transition_ordinal,item_uid,template_id,content_revision,item_kind,
          disposition_kind,source_kind,source_slot_index,destination_kind,
          destination_slot_index,pre_item_version,post_item_version,pre_security_state,
          post_security_state,destruction_reason,ledger_event_id,ledger_event_kind,
          ledger_source_kind)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,9,$6,$13,$14,
                 $15,$16,0,$17,$18,$19,$20,7)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(request.extraction_id.as_slice())
    .bind(i16::from(request.stack_index))
    .bind(i16::from(transition.ordinal))
    .bind(transition.item_uid.as_slice())
    .bind(&transition.template_id)
    .bind(&transition.content_revision)
    .bind(transition.item_kind.durable_kind())
    .bind(request.action.durable_kind())
    .bind(destination_kind)
    .bind(destination_slot.map(i16_value).transpose()?)
    .bind(i64_value(transition.pre_item_version)?)
    .bind(i64_value(transition.post_item_version)?)
    .bind(post_security)
    .bind(reason)
    .bind(transition.ledger_event_id.as_slice())
    .bind(ledger_kind)
    .execute(connection)
    .await?;
    Ok(())
}

async fn apply_hold_aggregate_heads(
    connection: &mut PgConnection,
    request: &ResolutionHoldMutationRequestV1,
    result: &StoredResolutionHoldMutationResultV1,
) -> Result<(), PersistenceError> {
    if result.versions.account.advanced() {
        let affected = sqlx::query(
            "UPDATE accounts SET state_version=$1,updated_at=transaction_timestamp()
             WHERE namespace_id=$2 AND account_id=$3 AND state_version=$4
               AND selected_character_id=$5",
        )
        .bind(i64_value(result.versions.account.post)?)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(i64_value(result.versions.account.pre)?)
        .bind(request.character_id.as_slice())
        .execute(&mut *connection)
        .await?
        .rows_affected();
        expect_one_hold(affected)?;
    }
    let inventory_affected = sqlx::query(
        "UPDATE character_inventories SET inventory_version=$1,
                updated_at=transaction_timestamp()
         WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4
           AND inventory_version=$5",
    )
    .bind(i64_value(result.versions.inventory.post)?)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(i64_value(result.versions.inventory.pre)?)
    .execute(&mut *connection)
    .await?
    .rows_affected();
    expect_one_hold(inventory_affected)?;

    if !result.storage_resolution_required {
        let character_affected = sqlx::query(
            "UPDATE characters SET character_state_version=$1,security_state=0,
                    updated_at=transaction_timestamp()
             WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4
               AND character_state_version=$5 AND life_state=0 AND security_state=1",
        )
        .bind(i64_value(result.versions.character.post)?)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(request.character_id.as_slice())
        .bind(i64_value(result.versions.character.pre)?)
        .execute(&mut *connection)
        .await?
        .rows_affected();
        expect_one_hold(character_affected)?;
        let world_affected = sqlx::query(
            "UPDATE character_world_locations SET character_version=$1,
                    updated_at=transaction_timestamp()
             WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4
               AND character_version=$5 AND location_kind=1
               AND location_content_id='hub.lantern_halls_01'
               AND instance_lineage_id IS NULL AND entry_restore_point_id IS NULL",
        )
        .bind(i64_value(result.versions.world.post)?)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(request.account_id.as_slice())
        .bind(request.character_id.as_slice())
        .bind(i64_value(result.versions.world.pre)?)
        .execute(connection)
        .await?
        .rows_affected();
        expect_one_hold(world_affected)?;
    }
    Ok(())
}

async fn insert_hold_audit_and_outbox(
    connection: &mut PgConnection,
    request: &ResolutionHoldMutationRequestV1,
    result: &StoredResolutionHoldMutationResultV1,
    result_payload: &[u8],
) -> Result<(), PersistenceError> {
    let id_parts = [
        request.account_id.as_slice(),
        request.character_id.as_slice(),
        request.mutation_id.as_slice(),
    ];
    let audit_id =
        derive_resolution_hold_id_v1(RESOLUTION_HOLD_ACCEPTED_AUDIT_ID_CONTEXT_V1, &id_parts);
    let outbox_id = derive_resolution_hold_id_v1(RESOLUTION_HOLD_OUTBOX_ID_CONTEXT_V1, &id_parts);
    sqlx::query(
        "INSERT INTO resolution_hold_mutation_audit_events_v1
         (namespace_id,account_id,character_id,mutation_id,event_id,event_type,event_digest)
         VALUES ($1,$2,$3,$4,$5,1,$6)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(audit_id.as_slice())
    .bind(result.result_hash.as_slice())
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "INSERT INTO resolution_hold_mutation_outbox_events_v1
         (namespace_id,account_id,character_id,mutation_id,event_id,event_type,event_payload)
         VALUES ($1,$2,$3,$4,$5,1,$6)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(request.account_id.as_slice())
    .bind(request.character_id.as_slice())
    .bind(request.mutation_id.as_slice())
    .bind(outbox_id.as_slice())
    .bind(result_payload)
    .execute(connection)
    .await?;
    Ok(())
}

async fn force_hold_deferred_constraints(
    connection: &mut PgConnection,
) -> Result<(), PersistenceError> {
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(connection)
        .await?;
    Ok(())
}

fn expect_one_hold(rows_affected: u64) -> Result<(), PersistenceError> {
    if rows_affected == 1 {
        Ok(())
    } else {
        Err(PersistenceError::ResolutionHoldUnresolvedMutation)
    }
}

async fn lock_hold_authority(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
) -> Result<LockedHoldAuthority, PersistenceError> {
    let account = lock_hold_account(connection, account_id).await?;
    lock_hold_authority_after_account(connection, account_id, character_id, account).await
}

async fn lock_hold_account(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
) -> Result<LockedHoldAccount, PersistenceError> {
    let row = sqlx::query(
        "SELECT state_version,selected_character_id FROM accounts
         WHERE namespace_id=$1 AND account_id=$2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ResolutionHoldOwnerNotFound)?;
    Ok(LockedHoldAccount {
        state_version: positive(row.try_get("state_version")?)?,
        selected_character_id: optional_exact_id(row.try_get("selected_character_id")?)?,
    })
}

async fn lock_hold_authority_after_account(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
    account: LockedHoldAccount,
) -> Result<LockedHoldAuthority, PersistenceError> {
    let character = sqlx::query(
        "SELECT life_state,security_state,character_state_version FROM characters
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ResolutionHoldOwnerNotFound)?;
    let world = sqlx::query(
        "SELECT character_version,location_kind,location_content_id,
                instance_lineage_id,entry_restore_point_id
         FROM character_world_locations
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ResolutionHoldHallBindingMismatch)?;
    let inventory = sqlx::query(
        "SELECT inventory_version FROM character_inventories
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(connection)
    .await?
    .ok_or(PersistenceError::ResolutionHoldOwnerNotFound)?;

    let life_state: i16 = character.try_get("life_state")?;
    let security_state: i16 = character.try_get("security_state")?;
    let location_kind: i16 = world.try_get("location_kind")?;
    let location_content_id: String = world.try_get("location_content_id")?;
    let instance_lineage_id = optional_exact_id(world.try_get("instance_lineage_id")?)?;
    let entry_restore_point_id = optional_exact_id(world.try_get("entry_restore_point_id")?)?;
    if account.selected_character_id != Some(character_id) {
        return Err(PersistenceError::ResolutionHoldOwnerNotFound);
    }
    if life_state != LIFE_LIVING
        || !matches!(
            security_state,
            SECURITY_NORMAL | SECURITY_STORAGE_RESOLUTION_REQUIRED
        )
        || location_kind != LOCATION_HALL
        || location_content_id != LANTERN_HALLS_CONTENT_ID
        || instance_lineage_id.is_some()
        || entry_restore_point_id.is_some()
    {
        return Err(PersistenceError::ResolutionHoldHallBindingMismatch);
    }
    let authority = LockedHoldAuthority {
        account_version: account.state_version,
        character_version: positive(character.try_get("character_state_version")?)?,
        world_version: positive(world.try_get("character_version")?)?,
        inventory_version: positive(inventory.try_get("inventory_version")?)?,
        security_state,
    };
    if authority.character_version != authority.world_version {
        return Err(PersistenceError::CorruptStoredResolutionHold);
    }
    Ok(authority)
}

#[allow(
    clippy::too_many_lines,
    reason = "every selected SQL column has an explicit bounded decoder"
)]
async fn lock_hold_and_storage_items(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
) -> Result<Vec<LockedHoldItemRow>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT item.item_uid,item.account_id,item.character_id,item.template_id,
                item.content_revision,item.item_kind,item.item_version,item.security_state,
                item.location_kind,item.slot_index,item.destruction_reason,
                item.terminal_extraction_id,
                floor(extract(epoch FROM item.extracted_at) * 1000)::bigint
                    AS extracted_at_unix_millis,
                floor(extract(epoch FROM item.overflow_expires_at) * 1000)::bigint
                    AS overflow_deadline_unix_millis,
                placement.account_id AS placement_account_id,
                placement.character_id AS placement_character_id,
                placement.template_id AS placement_template_id,
                placement.item_kind AS placement_item_kind,
                placement.destination_kind AS placement_destination_kind,
                placement.destination_slot_index AS placement_destination_slot_index,
                placement.post_item_version AS placement_post_item_version,
                placement.post_security_state AS placement_post_security_state,
                extraction.account_id AS extraction_account_id,
                extraction.character_id AS extraction_character_id,
                floor(extract(epoch FROM extraction.committed_at) * 1000)::bigint
                    AS extraction_committed_at_unix_millis
         FROM item_instances AS item
         LEFT JOIN extraction_terminal_item_placements_v1 AS placement
           ON item.location_kind=9
          AND placement.namespace_id=item.namespace_id
          AND placement.terminal_id=item.terminal_extraction_id
          AND placement.item_uid=item.item_uid
         LEFT JOIN character_extraction_terminal_results_v1 AS extraction
           ON item.location_kind=9
          AND extraction.namespace_id=item.namespace_id
          AND extraction.terminal_id=item.terminal_extraction_id
          AND extraction.account_id=item.account_id
         WHERE item.namespace_id=$1 AND item.account_id=$2
           AND ((item.location_kind=5 AND item.character_id=$3)
             OR (item.location_kind IN (6,8) AND item.character_id IS NULL)
             OR (item.location_kind=9 AND item.character_id=$3))
         ORDER BY item.item_uid
         FOR UPDATE OF item",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(LockedHoldItemRow {
                item_uid: exact_id(row.try_get("item_uid")?)?,
                account_id: exact_id(row.try_get("account_id")?)?,
                character_id: optional_exact_id(row.try_get("character_id")?)?,
                template_id: row.try_get("template_id")?,
                content_revision: row.try_get("content_revision")?,
                item_kind: StoredResolutionHoldItemKindV1::try_from_durable_kind(
                    row.try_get("item_kind")?,
                )?,
                item_version: positive(row.try_get("item_version")?)?,
                security_state: row.try_get("security_state")?,
                location_kind: row.try_get("location_kind")?,
                slot_index: u16_value(row.try_get("slot_index")?)?,
                destruction_reason: row.try_get("destruction_reason")?,
                terminal_extraction_id: optional_exact_id(row.try_get("terminal_extraction_id")?)?,
                extracted_at_unix_millis: optional_positive(
                    row.try_get("extracted_at_unix_millis")?,
                )?,
                overflow_deadline_unix_millis: optional_positive(
                    row.try_get("overflow_deadline_unix_millis")?,
                )?,
                placement_account_id: optional_exact_id(row.try_get("placement_account_id")?)?,
                placement_character_id: optional_exact_id(row.try_get("placement_character_id")?)?,
                placement_template_id: row.try_get("placement_template_id")?,
                placement_item_kind: row.try_get("placement_item_kind")?,
                placement_destination_kind: row.try_get("placement_destination_kind")?,
                placement_destination_slot_index: optional_u16(
                    row.try_get("placement_destination_slot_index")?,
                )?,
                placement_post_item_version: optional_positive(
                    row.try_get("placement_post_item_version")?,
                )?,
                placement_post_security_state: row.try_get("placement_post_security_state")?,
                extraction_account_id: optional_exact_id(row.try_get("extraction_account_id")?)?,
                extraction_character_id: optional_exact_id(
                    row.try_get("extraction_character_id")?,
                )?,
                extraction_committed_at_unix_millis: optional_positive(
                    row.try_get("extraction_committed_at_unix_millis")?,
                )?,
            })
        })
        .collect()
}

fn assemble_resolution_hold_snapshot(
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
    authority: LockedHoldAuthority,
    rows: Vec<LockedHoldItemRow>,
    authoritative_time_unix_millis: u64,
) -> Result<StoredResolutionHoldSnapshotV1, PersistenceError> {
    if authoritative_time_unix_millis == 0 {
        return Err(PersistenceError::CorruptStoredResolutionHold);
    }
    let (hold_groups, storage_groups) = group_hold_rows(account_id, character_id, rows)?;
    if hold_groups.len() > MAX_RESOLUTION_HOLD_STACKS_V1
        || (authority.security_state == SECURITY_STORAGE_RESOLUTION_REQUIRED)
            == hold_groups.is_empty()
    {
        return Err(PersistenceError::CorruptStoredResolutionHold);
    }
    let storage = build_storage_snapshot(storage_groups)?;
    let mut stacks = Vec::with_capacity(hold_groups.len());
    for ((extraction_id, stack_index), mut group) in hold_groups {
        group.items.sort_by_key(|item| item.item_uid);
        let overflow_deadline_unix_millis = group
            .extracted_at_unix_millis
            .checked_add(RESOLUTION_HOLD_OVERFLOW_LIFETIME_MILLIS)
            .ok_or(PersistenceError::CorruptStoredResolutionHold)?;
        let mut stack = StoredResolutionHoldStackV1 {
            extraction_id,
            stack_index,
            template_id: group.template_id,
            content_revision: group.content_revision,
            item_kind: group.item_kind,
            items: group.items,
            stack_digest: [0; 32],
            extracted_at_unix_millis: group.extracted_at_unix_millis,
            overflow_deadline_unix_millis,
            planned_destination: None,
        };
        stack.stack_digest = canonical_resolution_hold_stack_digest_v1(&stack)?;
        stack.validate()?;
        stack.planned_destination = match plan_resolution_hold_destination_v1(
            &stack,
            &storage,
            authoritative_time_unix_millis,
        ) {
            Ok(destination) => Some(destination),
            Err(PersistenceError::ResolutionHoldStorageFull) => None,
            Err(error) => return Err(error),
        };
        stacks.push(stack);
    }
    let snapshot = StoredResolutionHoldSnapshotV1 {
        account_id,
        character_id,
        versions: StoredResolutionHoldVersionsV1 {
            account: authority.account_version,
            character: authority.character_version,
            world: authority.world_version,
            inventory: authority.inventory_version,
        },
        storage_resolution_required: !stacks.is_empty(),
        stacks,
    };
    snapshot.validate()?;
    Ok(snapshot)
}

fn group_hold_rows(
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
    rows: Vec<LockedHoldItemRow>,
) -> Result<(HoldGroups, StorageGroups), PersistenceError> {
    let mut hold_groups = HoldGroups::new();
    let mut storage_groups = StorageGroups::new();
    for row in rows {
        validate_common_item(&row, account_id)?;
        let item = StoredResolutionHoldItemV1 {
            item_uid: row.item_uid,
            item_version: row.item_version,
        };
        match row.location_kind {
            LOCATION_CHARACTER_SAFE | LOCATION_VAULT | LOCATION_OVERFLOW => {
                validate_storage_item(&row, character_id)?;
                let group = storage_groups
                    .entry((row.location_kind, row.slot_index))
                    .or_insert_with(|| StorageStackBuilder {
                        template_id: row.template_id.clone(),
                        content_revision: row.content_revision.clone(),
                        item_kind: row.item_kind,
                        items: Vec::new(),
                    });
                if group.template_id != row.template_id
                    || group.content_revision != row.content_revision
                    || group.item_kind != row.item_kind
                {
                    return Err(PersistenceError::CorruptStoredResolutionHold);
                }
                group.items.push(item);
            }
            LOCATION_RESOLUTION_HOLD => {
                validate_hold_item(&row, account_id, character_id)?;
                let extraction_id = row
                    .terminal_extraction_id
                    .ok_or(PersistenceError::CorruptStoredResolutionHold)?;
                let stack_index = u8::try_from(row.slot_index)
                    .map_err(|_| PersistenceError::CorruptStoredResolutionHold)?;
                let extracted_at_unix_millis = row
                    .extracted_at_unix_millis
                    .ok_or(PersistenceError::CorruptStoredResolutionHold)?;
                let group = hold_groups
                    .entry((extraction_id, stack_index))
                    .or_insert_with(|| LogicalStackBuilder {
                        template_id: row.template_id.clone(),
                        content_revision: row.content_revision.clone(),
                        item_kind: row.item_kind,
                        extracted_at_unix_millis,
                        items: Vec::new(),
                    });
                if group.template_id != row.template_id
                    || group.content_revision != row.content_revision
                    || group.item_kind != row.item_kind
                    || group.extracted_at_unix_millis != extracted_at_unix_millis
                {
                    return Err(PersistenceError::CorruptStoredResolutionHold);
                }
                group.items.push(item);
            }
            _ => return Err(PersistenceError::CorruptStoredResolutionHold),
        }
    }
    Ok((hold_groups, storage_groups))
}

fn build_storage_snapshot(
    groups: StorageGroups,
) -> Result<ResolutionHoldStorageSnapshotV1, PersistenceError> {
    let mut storage = ResolutionHoldStorageSnapshotV1::empty();
    for ((location_kind, slot_index), mut group) in groups {
        group.items.sort_by_key(|item| item.item_uid);
        let stack = ResolutionHoldStorageStackV1 {
            template_id: group.template_id,
            content_revision: group.content_revision,
            item_kind: group.item_kind,
            items: group.items,
        };
        let destination = match location_kind {
            LOCATION_CHARACTER_SAFE => storage.character_safe.get_mut(usize::from(slot_index)),
            LOCATION_VAULT => storage.vault.get_mut(usize::from(slot_index)),
            LOCATION_OVERFLOW => storage.overflow.get_mut(usize::from(slot_index)),
            _ => None,
        }
        .ok_or(PersistenceError::CorruptStoredResolutionHold)?;
        if destination.replace(stack).is_some() {
            return Err(PersistenceError::CorruptStoredResolutionHold);
        }
    }
    Ok(storage)
}

fn validate_common_item(
    row: &LockedHoldItemRow,
    account_id: [u8; ID_BYTES],
) -> Result<(), PersistenceError> {
    if row.account_id != account_id
        || row.content_revision != CORE_ITEM_CONTENT_REVISION
        || row.security_state != ITEM_SECURITY_SAFE
        || row.destruction_reason.is_some()
    {
        return Err(PersistenceError::CorruptStoredResolutionHold);
    }
    Ok(())
}

fn validate_storage_item(
    row: &LockedHoldItemRow,
    character_id: [u8; ID_BYTES],
) -> Result<(), PersistenceError> {
    let owner_valid = match row.location_kind {
        LOCATION_CHARACTER_SAFE => row.character_id == Some(character_id),
        LOCATION_VAULT | LOCATION_OVERFLOW => row.character_id.is_none(),
        _ => false,
    };
    let slot_valid = match row.location_kind {
        LOCATION_CHARACTER_SAFE => row.slot_index < 8,
        LOCATION_VAULT => row.slot_index < 160,
        LOCATION_OVERFLOW => row.slot_index < 20,
        _ => false,
    };
    let overflow_valid = if row.location_kind == LOCATION_OVERFLOW {
        row.terminal_extraction_id.is_some()
            && row.extracted_at_unix_millis.is_some()
            && row.overflow_deadline_unix_millis
                == row
                    .extracted_at_unix_millis
                    .and_then(|value| value.checked_add(RESOLUTION_HOLD_OVERFLOW_LIFETIME_MILLIS))
    } else {
        row.overflow_deadline_unix_millis.is_none()
    };
    if !owner_valid || !slot_valid || !overflow_valid {
        return Err(PersistenceError::CorruptStoredResolutionHold);
    }
    Ok(())
}

fn validate_hold_item(
    row: &LockedHoldItemRow,
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
) -> Result<(), PersistenceError> {
    if row.character_id != Some(character_id)
        || row.slot_index >= 8
        || row.overflow_deadline_unix_millis.is_some()
        || row.terminal_extraction_id.is_none()
        || row.extracted_at_unix_millis.is_none()
        || row.placement_account_id != Some(account_id)
        || row.placement_character_id != Some(character_id)
        || row.placement_template_id.as_deref() != Some(row.template_id.as_str())
        || row.placement_item_kind != Some(row.item_kind.durable_kind())
        || row.placement_destination_kind != Some(LOCATION_RESOLUTION_HOLD)
        || row.placement_destination_slot_index != Some(row.slot_index)
        || row.placement_post_item_version != Some(row.item_version)
        || row.placement_post_security_state != Some(ITEM_SECURITY_SAFE)
        || row.extraction_account_id != Some(account_id)
        || row.extraction_character_id != Some(character_id)
        || row.extraction_committed_at_unix_millis != row.extracted_at_unix_millis
    {
        return Err(PersistenceError::CorruptStoredResolutionHold);
    }
    Ok(())
}

async fn transaction_timestamp_millis(
    connection: &mut PgConnection,
) -> Result<u64, PersistenceError> {
    let value: i64 = sqlx::query_scalar(
        "SELECT floor(extract(epoch FROM transaction_timestamp()) * 1000)::bigint",
    )
    .fetch_one(connection)
    .await?;
    positive(value)
}

fn exact_id(value: Vec<u8>) -> Result<[u8; ID_BYTES], PersistenceError> {
    let id: [u8; ID_BYTES] = value
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredResolutionHold)?;
    if id == [0; ID_BYTES] {
        return Err(PersistenceError::CorruptStoredResolutionHold);
    }
    Ok(id)
}

fn optional_exact_id(value: Option<Vec<u8>>) -> Result<Option<[u8; ID_BYTES]>, PersistenceError> {
    value.map(exact_id).transpose()
}

fn exact_hash(value: Vec<u8>) -> Result<[u8; RESOLUTION_HOLD_HASH_BYTES], PersistenceError> {
    let hash: [u8; RESOLUTION_HOLD_HASH_BYTES] = value
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredResolutionHold)?;
    if hash == [0; RESOLUTION_HOLD_HASH_BYTES] {
        return Err(PersistenceError::CorruptStoredResolutionHold);
    }
    Ok(hash)
}

fn positive(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(PersistenceError::CorruptStoredResolutionHold)
}

fn optional_positive(value: Option<i64>) -> Result<Option<u64>, PersistenceError> {
    value.map(positive).transpose()
}

fn i64_value(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| PersistenceError::CorruptStoredResolutionHold)
}

fn i16_value(value: u16) -> Result<i16, PersistenceError> {
    i16::try_from(value).map_err(|_| PersistenceError::CorruptStoredResolutionHold)
}

fn u8_value(value: i16) -> Result<u8, PersistenceError> {
    u8::try_from(value).map_err(|_| PersistenceError::CorruptStoredResolutionHold)
}

fn u16_value(value: i16) -> Result<u16, PersistenceError> {
    u16::try_from(value).map_err(|_| PersistenceError::CorruptStoredResolutionHold)
}

fn optional_u16(value: Option<i16>) -> Result<Option<u16>, PersistenceError> {
    value.map(u16_value).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCOUNT: [u8; 16] = [1; 16];
    const CHARACTER: [u8; 16] = [2; 16];
    const EXTRACTION: [u8; 16] = [3; 16];
    const MUTATION: [u8; 16] = [4; 16];

    fn authority(security_state: i16) -> LockedHoldAuthority {
        LockedHoldAuthority {
            account_version: 4,
            character_version: 5,
            world_version: 5,
            inventory_version: 6,
            security_state,
        }
    }

    fn hold_item(uid: u8, item_version: u64) -> LockedHoldItemRow {
        LockedHoldItemRow {
            item_uid: [uid; 16],
            account_id: ACCOUNT,
            character_id: Some(CHARACTER),
            template_id: "consumable.red_tonic".into(),
            content_revision: CORE_ITEM_CONTENT_REVISION.into(),
            item_kind: StoredResolutionHoldItemKindV1::Consumable,
            item_version,
            security_state: ITEM_SECURITY_SAFE,
            location_kind: LOCATION_RESOLUTION_HOLD,
            slot_index: 0,
            destruction_reason: None,
            terminal_extraction_id: Some(EXTRACTION),
            extracted_at_unix_millis: Some(1_000),
            overflow_deadline_unix_millis: None,
            placement_account_id: Some(ACCOUNT),
            placement_character_id: Some(CHARACTER),
            placement_template_id: Some("consumable.red_tonic".into()),
            placement_item_kind: Some(1),
            placement_destination_kind: Some(LOCATION_RESOLUTION_HOLD),
            placement_destination_slot_index: Some(0),
            placement_post_item_version: Some(item_version),
            placement_post_security_state: Some(ITEM_SECURITY_SAFE),
            extraction_account_id: Some(ACCOUNT),
            extraction_character_id: Some(CHARACTER),
            extraction_committed_at_unix_millis: Some(1_000),
        }
    }

    fn storage_item(uid: u8, location_kind: i16, slot_index: u16) -> LockedHoldItemRow {
        LockedHoldItemRow {
            item_uid: [uid; 16],
            account_id: ACCOUNT,
            character_id: (location_kind == LOCATION_CHARACTER_SAFE).then_some(CHARACTER),
            template_id: format!("equipment.test_{uid}"),
            content_revision: CORE_ITEM_CONTENT_REVISION.into(),
            item_kind: StoredResolutionHoldItemKindV1::Equipment,
            item_version: 1,
            security_state: ITEM_SECURITY_SAFE,
            location_kind,
            slot_index,
            destruction_reason: None,
            terminal_extraction_id: None,
            extracted_at_unix_millis: None,
            overflow_deadline_unix_millis: None,
            placement_account_id: None,
            placement_character_id: None,
            placement_template_id: None,
            placement_item_kind: None,
            placement_destination_kind: None,
            placement_destination_slot_index: None,
            placement_post_item_version: None,
            placement_post_security_state: None,
            extraction_account_id: None,
            extraction_character_id: None,
            extraction_committed_at_unix_millis: None,
        }
    }

    fn mutation_request(
        snapshot: &StoredResolutionHoldSnapshotV1,
        action: StoredResolutionHoldActionV1,
    ) -> ResolutionHoldMutationRequestV1 {
        ResolutionHoldMutationRequestV1 {
            contract_version: RESOLUTION_HOLD_CONTRACT_VERSION_V1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            account_id: ACCOUNT,
            character_id: CHARACTER,
            mutation_id: MUTATION,
            extraction_id: snapshot.stacks[0].extraction_id,
            stack_index: snapshot.stacks[0].stack_index,
            action,
            expected_versions: snapshot.versions,
            content_revision: CORE_ITEM_CONTENT_REVISION.into(),
            expected_stack_digest: snapshot.stacks[0].stack_digest,
            issued_at_unix_millis: 1_500,
        }
    }

    #[test]
    fn snapshot_groups_unsigned_uids_and_publishes_server_preview() {
        let snapshot = assemble_resolution_hold_snapshot(
            ACCOUNT,
            CHARACTER,
            authority(SECURITY_STORAGE_RESOLUTION_REQUIRED),
            vec![hold_item(11, 2), hold_item(10, 1)],
            2_000,
        )
        .unwrap();
        assert!(snapshot.storage_resolution_required);
        assert_eq!(snapshot.stacks.len(), 1);
        assert_eq!(snapshot.stacks[0].items[0].item_uid, [10; 16]);
        assert_eq!(snapshot.stacks[0].items[1].item_uid, [11; 16]);
        assert_eq!(
            snapshot.stacks[0].planned_destination,
            Some(crate::StoredResolutionHoldDestinationV1::CharacterSafe(0))
        );
        snapshot.validate().unwrap();
    }

    #[test]
    fn empty_normal_hall_snapshot_is_valid_and_bounded() {
        let snapshot = assemble_resolution_hold_snapshot(
            ACCOUNT,
            CHARACTER,
            authority(SECURITY_NORMAL),
            Vec::new(),
            2_000,
        )
        .unwrap();
        assert!(!snapshot.storage_resolution_required);
        assert!(snapshot.stacks.is_empty());
    }

    #[test]
    fn storage_capacity_changes_preview_without_changing_stack_digest() {
        let mut rows = vec![hold_item(10, 1)];
        for index in 0..8 {
            rows.push(storage_item(
                20 + index,
                LOCATION_CHARACTER_SAFE,
                u16::from(index),
            ));
        }
        let snapshot = assemble_resolution_hold_snapshot(
            ACCOUNT,
            CHARACTER,
            authority(SECURITY_STORAGE_RESOLUTION_REQUIRED),
            rows,
            2_000,
        )
        .unwrap();
        assert_eq!(
            snapshot.stacks[0].planned_destination,
            Some(crate::StoredResolutionHoldDestinationV1::Vault(0))
        );
        assert_eq!(
            snapshot.stacks[0].stack_digest,
            canonical_resolution_hold_stack_digest_v1(&snapshot.stacks[0]).unwrap()
        );
    }

    #[test]
    fn missing_or_changed_extraction_provenance_fails_closed() {
        let mut row = hold_item(10, 1);
        row.placement_post_item_version = Some(2);
        assert!(matches!(
            assemble_resolution_hold_snapshot(
                ACCOUNT,
                CHARACTER,
                authority(SECURITY_STORAGE_RESOLUTION_REQUIRED),
                vec![row],
                2_000,
            ),
            Err(PersistenceError::CorruptStoredResolutionHold)
        ));

        let mut row = hold_item(10, 1);
        row.extraction_committed_at_unix_millis = Some(999);
        assert!(matches!(
            assemble_resolution_hold_snapshot(
                ACCOUNT,
                CHARACTER,
                authority(SECURITY_STORAGE_RESOLUTION_REQUIRED),
                vec![row],
                2_000,
            ),
            Err(PersistenceError::CorruptStoredResolutionHold)
        ));
    }

    #[test]
    fn security_and_content_corruption_are_never_projected() {
        assert!(matches!(
            assemble_resolution_hold_snapshot(
                ACCOUNT,
                CHARACTER,
                authority(SECURITY_NORMAL),
                vec![hold_item(10, 1)],
                2_000,
            ),
            Err(PersistenceError::CorruptStoredResolutionHold)
        ));
        let mut row = hold_item(10, 1);
        row.content_revision = "core.invalid".into();
        assert!(matches!(
            assemble_resolution_hold_snapshot(
                ACCOUNT,
                CHARACTER,
                authority(SECURITY_STORAGE_RESOLUTION_REQUIRED),
                vec![row],
                2_000,
            ),
            Err(PersistenceError::CorruptStoredResolutionHold)
        ));
    }

    #[test]
    fn fresh_move_result_advances_only_the_owned_aggregate_heads() {
        let snapshot = assemble_resolution_hold_snapshot(
            ACCOUNT,
            CHARACTER,
            authority(SECURITY_STORAGE_RESOLUTION_REQUIRED),
            vec![hold_item(11, 2), hold_item(10, 1)],
            2_000,
        )
        .unwrap();
        let request = mutation_request(&snapshot, StoredResolutionHoldActionV1::Move);
        let request_hash = request.canonical_hash().unwrap();
        let result = build_hold_mutation_result(&request, request_hash, &snapshot, 2_000).unwrap();

        assert_eq!(
            result.destination,
            Some(StoredResolutionHoldDestinationV1::CharacterSafe(0))
        );
        assert_eq!(
            result.versions.account,
            StoredResolutionHoldVersionAdvanceV1 { pre: 4, post: 4 }
        );
        assert_eq!(result.versions.character.post, 6);
        assert_eq!(result.versions.world.post, 6);
        assert_eq!(result.versions.inventory.post, 7);
        assert!(!result.storage_resolution_required);
        assert_eq!(result.remaining_hold_stack_count, 0);
        assert_eq!(result.transitions[0].item_uid, [10; 16]);
        assert_eq!(result.transitions[0].pre_item_version, 1);
        assert_eq!(result.transitions[0].post_item_version, 2);
        assert!(matches!(
            result.transitions[0].disposition,
            StoredResolutionHoldDispositionV1::Moved(
                StoredResolutionHoldDestinationV1::CharacterSafe(0)
            )
        ));
        assert!(exact_hold_replay(&result, &request, request_hash));
    }

    #[test]
    fn account_storage_and_partial_clear_version_rules_are_exact() {
        let mut rows = vec![hold_item(10, 1)];
        let mut second_stack = hold_item(11, 3);
        second_stack.slot_index = 1;
        second_stack.placement_destination_slot_index = Some(1);
        rows.push(second_stack);
        for index in 0_u8..8 {
            rows.push(storage_item(
                20 + index,
                LOCATION_CHARACTER_SAFE,
                u16::from(index),
            ));
        }
        let snapshot = assemble_resolution_hold_snapshot(
            ACCOUNT,
            CHARACTER,
            authority(SECURITY_STORAGE_RESOLUTION_REQUIRED),
            rows,
            2_000,
        )
        .unwrap();
        let request = mutation_request(&snapshot, StoredResolutionHoldActionV1::Move);
        let result = build_hold_mutation_result(
            &request,
            request.canonical_hash().unwrap(),
            &snapshot,
            2_000,
        )
        .unwrap();

        assert_eq!(
            result.destination,
            Some(StoredResolutionHoldDestinationV1::Vault(0))
        );
        assert_eq!(result.versions.account.post, 5);
        assert_eq!(result.versions.character.post, 5);
        assert_eq!(result.versions.world.post, 5);
        assert_eq!(result.versions.inventory.post, 7);
        assert!(result.storage_resolution_required);
        assert_eq!(result.remaining_hold_stack_count, 1);
    }

    #[test]
    fn confirmed_destruction_has_no_destination_or_account_reward() {
        let snapshot = assemble_resolution_hold_snapshot(
            ACCOUNT,
            CHARACTER,
            authority(SECURITY_STORAGE_RESOLUTION_REQUIRED),
            vec![hold_item(10, 7)],
            2_000,
        )
        .unwrap();
        let request = mutation_request(&snapshot, StoredResolutionHoldActionV1::DestroyConfirmed);
        let result = build_hold_mutation_result(
            &request,
            request.canonical_hash().unwrap(),
            &snapshot,
            2_000,
        )
        .unwrap();

        assert_eq!(result.destination, None);
        assert_eq!(result.versions.account.post, 4);
        assert_eq!(result.versions.character.post, 6);
        assert_eq!(result.versions.world.post, 6);
        assert_eq!(result.versions.inventory.post, 7);
        assert_eq!(result.transitions[0].post_item_version, 8);
        assert_eq!(
            result.transitions[0].disposition,
            StoredResolutionHoldDispositionV1::Destroyed
        );
    }

    #[test]
    fn stale_digest_and_changed_exact_replay_fail_closed() {
        let snapshot = assemble_resolution_hold_snapshot(
            ACCOUNT,
            CHARACTER,
            authority(SECURITY_STORAGE_RESOLUTION_REQUIRED),
            vec![hold_item(10, 1)],
            2_000,
        )
        .unwrap();
        let request = mutation_request(&snapshot, StoredResolutionHoldActionV1::Move);
        let request_hash = request.canonical_hash().unwrap();
        let result = build_hold_mutation_result(&request, request_hash, &snapshot, 2_000).unwrap();
        let mut altered = request.clone();
        altered.issued_at_unix_millis += 1;
        assert!(!exact_hold_replay(
            &result,
            &altered,
            altered.canonical_hash().unwrap()
        ));

        let mut stale = request;
        stale.expected_stack_digest[0] ^= 1;
        assert!(matches!(
            build_hold_mutation_result(&stale, stale.canonical_hash().unwrap(), &snapshot, 2_000,),
            Err(PersistenceError::ResolutionHoldStackDigestMismatch)
        ));
    }
}
