//! Serializable query and replay-first writer for M03 `ResolutionHold` recovery.

use std::collections::BTreeMap;

use sqlx::{PgConnection, Row};

use crate::{
    CORE_ITEM_CONTENT_REVISION, MAX_RESOLUTION_HOLD_STACKS_V1, PersistenceError,
    PostgresPersistence, RESOLUTION_HOLD_OVERFLOW_LIFETIME_MILLIS, ResolutionHoldStorageSnapshotV1,
    ResolutionHoldStorageStackV1, StoredResolutionHoldItemKindV1, StoredResolutionHoldItemV1,
    StoredResolutionHoldSnapshotV1, StoredResolutionHoldStackV1, StoredResolutionHoldVersionsV1,
    WIPEABLE_CORE_NAMESPACE, canonical_resolution_hold_stack_digest_v1,
    is_retryable_transaction_failure, plan_resolution_hold_destination_v1,
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
const LOCATION_HALL: i16 = 1;
const LANTERN_HALLS_CONTENT_ID: &str = "hub.lantern_halls_01";

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

async fn lock_hold_authority(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
) -> Result<LockedHoldAuthority, PersistenceError> {
    let account = sqlx::query(
        "SELECT state_version,selected_character_id FROM accounts
         WHERE namespace_id=$1 AND account_id=$2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::ResolutionHoldOwnerNotFound)?;
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

    let selected_character_id = optional_exact_id(account.try_get("selected_character_id")?)?;
    let life_state: i16 = character.try_get("life_state")?;
    let security_state: i16 = character.try_get("security_state")?;
    let location_kind: i16 = world.try_get("location_kind")?;
    let location_content_id: String = world.try_get("location_content_id")?;
    let instance_lineage_id = optional_exact_id(world.try_get("instance_lineage_id")?)?;
    let entry_restore_point_id = optional_exact_id(world.try_get("entry_restore_point_id")?)?;
    if selected_character_id != Some(character_id)
        || life_state != LIFE_LIVING
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
        account_version: positive(account.try_get("state_version")?)?,
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

fn positive(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(PersistenceError::CorruptStoredResolutionHold)
}

fn optional_positive(value: Option<i64>) -> Result<Option<u64>, PersistenceError> {
    value.map(positive).transpose()
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
}
