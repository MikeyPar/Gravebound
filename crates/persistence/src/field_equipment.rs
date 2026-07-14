use sqlx::Row;

use crate::{PersistenceError, PostgresPersistence, WIPEABLE_CORE_NAMESPACE};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFieldEquipmentItem {
    pub item_uid: [u8; 16],
    pub template_id: String,
    pub content_revision: String,
    pub item_level: u8,
    pub rarity: i16,
    pub item_version: u64,
    pub security_state: i16,
    pub location_kind: i16,
    pub slot_index: Option<u8>,
    pub instance_id: Option<[u8; 16]>,
    pub pickup_id: Option<[u8; 16]>,
    pub expires_at_tick: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFieldEquipmentSnapshot {
    pub inventory_version: u64,
    pub equipment: Vec<StoredFieldEquipmentItem>,
    pub occupied_backpack_slots: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredFieldEquipmentSource {
    RunBackpack {
        slot_index: u8,
    },
    PersonalGround {
        instance_id: [u8; 16],
        pickup_id: [u8; 16],
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFieldEquipmentCommand {
    pub command_id: [u8; 16],
    pub canonical_request_hash: [u8; 32],
    pub preview_hash: [u8; 32],
    pub result_hash: [u8; 32],
    pub content_revision: String,
    pub expected_inventory_version: u64,
    pub incoming_item_uid: [u8; 16],
    pub incoming_item_version: u64,
    pub target_slot_index: u8,
    pub replaced_item_uid: Option<[u8; 16]>,
    pub replaced_item_version: Option<u64>,
    pub source: StoredFieldEquipmentSource,
    pub replacement_slot_index: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFieldEquipmentResult {
    pub replayed: bool,
    pub command_id: [u8; 16],
    pub result_hash: [u8; 32],
    pub pre_inventory_version: u64,
    pub post_inventory_version: u64,
    pub incoming_item_uid: [u8; 16],
    pub replaced_item_uid: Option<[u8; 16]>,
    pub replacement_slot_index: Option<u8>,
}

impl PostgresPersistence {
    pub async fn load_field_equipment_replay(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
        command_id: [u8; 16],
        canonical_request_hash: [u8; 32],
    ) -> Result<Option<StoredFieldEquipmentResult>, PersistenceError> {
        if command_id == [0; 16] || canonical_request_hash == [0; 32] {
            return Err(PersistenceError::CorruptStoredItems);
        }
        let mut transaction = self.begin_transaction().await?;
        let row = load_result_row(
            transaction.connection(),
            account_id,
            character_id,
            command_id,
        )
        .await?;
        transaction.rollback().await?;
        row.map(|row| decode_result_row(&row, command_id, canonical_request_hash, true))
            .transpose()
    }

    pub async fn load_field_equipment_snapshot(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> Result<StoredFieldEquipmentSnapshot, PersistenceError> {
        let mut transaction = self.begin_transaction().await?;
        let inventory_version = super::items::lock_or_create_inventory(
            transaction.connection(),
            account_id,
            character_id,
        )
        .await?;
        let rows = sqlx::query(
            "SELECT item_uid, template_id, content_revision, item_level, rarity, item_version, \
             security_state, location_kind, slot_index, instance_id, pickup_id, expires_at_tick, \
             item_kind FROM item_instances WHERE namespace_id = $1 AND account_id = $2 \
             AND character_id = $3 AND location_kind IN (0, 2, 3) \
             ORDER BY location_kind, slot_index NULLS LAST, item_uid FOR SHARE",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .fetch_all(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?;
        let snapshot = decode_snapshot(inventory_version, rows)?;
        transaction.rollback().await?;
        Ok(snapshot)
    }

    pub async fn commit_field_equipment(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
        command: &StoredFieldEquipmentCommand,
    ) -> Result<StoredFieldEquipmentResult, PersistenceError> {
        const MAX_TRANSACTION_ATTEMPTS: u8 = 3;
        validate_command(command)?;
        for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self
                .commit_field_equipment_once(account_id, character_id, command)
                .await
            {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS
                        && crate::is_retryable_transaction_failure(&error) => {}
                result => return result,
            }
        }
        unreachable!("bounded field equipment transaction always returns")
    }

    async fn commit_field_equipment_once(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
        command: &StoredFieldEquipmentCommand,
    ) -> Result<StoredFieldEquipmentResult, PersistenceError> {
        let mut transaction = self.begin_transaction().await?;
        let inventory_version = super::items::lock_or_create_inventory(
            transaction.connection(),
            account_id,
            character_id,
        )
        .await?;
        if let Some(replay) =
            load_result(transaction.connection(), account_id, character_id, command).await?
        {
            transaction.rollback().await?;
            return Ok(replay);
        }
        if u64::try_from(inventory_version).ok() != Some(command.expected_inventory_version) {
            transaction.rollback().await?;
            return Err(PersistenceError::FieldEquipmentVersionMismatch);
        }

        let incoming = lock_item(
            transaction.connection(),
            account_id,
            character_id,
            command.incoming_item_uid,
        )
        .await?;
        validate_incoming(&incoming, command)?;
        let replaced = lock_equipped_slot(
            transaction.connection(),
            account_id,
            character_id,
            command.target_slot_index,
        )
        .await?;
        validate_replaced(replaced.as_ref(), command)?;
        validate_destination(transaction.connection(), account_id, character_id, command).await?;

        if let Some(item) = &replaced {
            transition_item(
                transaction.connection(),
                account_id,
                character_id,
                item,
                command.command_id,
                2,
                command
                    .replacement_slot_index
                    .ok_or(PersistenceError::FieldEquipmentBindingMismatch)?,
            )
            .await?;
        }
        transition_item(
            transaction.connection(),
            account_id,
            character_id,
            &incoming,
            command.command_id,
            0,
            command.target_slot_index,
        )
        .await?;
        let post_inventory_version = inventory_version
            .checked_add(1)
            .ok_or(PersistenceError::CorruptStoredItems)?;
        sqlx::query(
            "UPDATE character_inventories SET inventory_version = $1, updated_at = \
             transaction_timestamp() WHERE namespace_id = $2 AND account_id = $3 \
             AND character_id = $4",
        )
        .bind(post_inventory_version)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .execute(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?;
        insert_result(
            transaction.connection(),
            account_id,
            character_id,
            command,
            inventory_version,
            post_inventory_version,
        )
        .await?;
        transaction.commit().await?;
        result_from_command(command, false)
    }
}

#[derive(Debug, Clone)]
struct LockedEquipmentItem {
    item_uid: [u8; 16],
    content_revision: String,
    item_version: u64,
    security_state: i16,
    location_kind: i16,
    slot_index: Option<u8>,
    instance_id: Option<[u8; 16]>,
    pickup_id: Option<[u8; 16]>,
}

async fn lock_item(
    connection: &mut sqlx::PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    item_uid: [u8; 16],
) -> Result<LockedEquipmentItem, PersistenceError> {
    let row = sqlx::query(
        "SELECT item_uid, content_revision, item_version, security_state, location_kind, \
         slot_index, instance_id, pickup_id FROM item_instances WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3 AND item_uid = $4 AND item_kind = 0 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(item_uid.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?
    .ok_or(PersistenceError::FieldEquipmentBindingMismatch)?;
    decode_locked_item(&row)
}

async fn lock_equipped_slot(
    connection: &mut sqlx::PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    slot_index: u8,
) -> Result<Option<LockedEquipmentItem>, PersistenceError> {
    let row = sqlx::query(
        "SELECT item_uid, content_revision, item_version, security_state, location_kind, \
         slot_index, instance_id, pickup_id FROM item_instances WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3 AND location_kind = 0 \
         AND slot_index = $4 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(i16::from(slot_index))
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?;
    row.as_ref().map(decode_locked_item).transpose()
}

fn decode_locked_item(
    row: &sqlx::postgres::PgRow,
) -> Result<LockedEquipmentItem, PersistenceError> {
    Ok(LockedEquipmentItem {
        item_uid: fixed_bytes(row.try_get("item_uid")?)?,
        content_revision: row.try_get("content_revision")?,
        item_version: u64::try_from(row.try_get::<i64, _>("item_version")?)
            .map_err(|_| PersistenceError::CorruptStoredItems)?,
        security_state: row.try_get("security_state")?,
        location_kind: row.try_get("location_kind")?,
        slot_index: optional_index(row.try_get("slot_index")?)?,
        instance_id: optional_bytes(row.try_get("instance_id")?)?,
        pickup_id: optional_bytes(row.try_get("pickup_id")?)?,
    })
}

fn validate_command(command: &StoredFieldEquipmentCommand) -> Result<(), PersistenceError> {
    let replacement_shape = command.replaced_item_uid.is_some()
        && command.replaced_item_version.is_some()
        && command.replacement_slot_index.is_some_and(|slot| slot < 8)
        || command.replaced_item_uid.is_none()
            && command.replaced_item_version.is_none()
            && command.replacement_slot_index.is_none();
    let source_valid = match command.source {
        StoredFieldEquipmentSource::RunBackpack { slot_index } => slot_index < 8,
        StoredFieldEquipmentSource::PersonalGround {
            instance_id,
            pickup_id,
        } => instance_id != [0; 16] && pickup_id != [0; 16],
    };
    if command.command_id == [0; 16]
        || command.canonical_request_hash == [0; 32]
        || command.preview_hash == [0; 32]
        || command.result_hash == [0; 32]
        || !command.content_revision.starts_with("core-dev.blake3.")
        || command.content_revision.len() != "core-dev.blake3.".len() + 64
        || command.expected_inventory_version == 0
        || command.incoming_item_uid == [0; 16]
        || command.incoming_item_version == 0
        || command.target_slot_index >= 4
        || command.replaced_item_uid == Some(command.incoming_item_uid)
        || !replacement_shape
        || !source_valid
    {
        return Err(PersistenceError::CorruptStoredItems);
    }
    Ok(())
}

fn validate_incoming(
    item: &LockedEquipmentItem,
    command: &StoredFieldEquipmentCommand,
) -> Result<(), PersistenceError> {
    let source_matches = match command.source {
        StoredFieldEquipmentSource::RunBackpack { slot_index } => {
            item.location_kind == 2
                && item.slot_index == Some(slot_index)
                && item.instance_id.is_none()
                && item.pickup_id.is_none()
        }
        StoredFieldEquipmentSource::PersonalGround {
            instance_id,
            pickup_id,
        } => {
            item.location_kind == 3
                && item.slot_index.is_none()
                && item.instance_id == Some(instance_id)
                && item.pickup_id == Some(pickup_id)
        }
    };
    if item.item_uid != command.incoming_item_uid
        || item.item_version != command.incoming_item_version
        || item.content_revision != command.content_revision
        || item.security_state != 2
        || !source_matches
    {
        return Err(PersistenceError::FieldEquipmentBindingMismatch);
    }
    Ok(())
}

fn validate_replaced(
    item: Option<&LockedEquipmentItem>,
    command: &StoredFieldEquipmentCommand,
) -> Result<(), PersistenceError> {
    match (
        item,
        command.replaced_item_uid,
        command.replaced_item_version,
    ) {
        (None, None, None) => Ok(()),
        (Some(item), Some(uid), Some(version))
            if item.item_uid == uid
                && item.item_version == version
                && item.location_kind == 0
                && item.slot_index == Some(command.target_slot_index)
                && matches!(item.security_state, 0 | 1) =>
        {
            Ok(())
        }
        _ => Err(PersistenceError::FieldEquipmentBindingMismatch),
    }
}

async fn validate_destination(
    connection: &mut sqlx::PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    command: &StoredFieldEquipmentCommand,
) -> Result<(), PersistenceError> {
    let Some(destination) = command.replacement_slot_index else {
        return Ok(());
    };
    match command.source {
        StoredFieldEquipmentSource::RunBackpack { slot_index } if slot_index == destination => {
            Ok(())
        }
        StoredFieldEquipmentSource::PersonalGround { .. } => {
            let occupied = sqlx::query_scalar::<_, i32>(
                "SELECT 1 FROM item_instances WHERE namespace_id = $1 AND account_id = $2 \
                 AND character_id = $3 AND location_kind = 2 AND slot_index = $4 \
                 LIMIT 1 FOR UPDATE",
            )
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(account_id.as_slice())
            .bind(character_id.as_slice())
            .bind(i16::from(destination))
            .fetch_optional(connection)
            .await
            .map_err(PersistenceError::Database)?;
            if occupied.is_some() {
                Err(PersistenceError::FieldEquipmentBindingMismatch)
            } else {
                Ok(())
            }
        }
        StoredFieldEquipmentSource::RunBackpack { .. } => {
            Err(PersistenceError::FieldEquipmentBindingMismatch)
        }
    }
}

async fn transition_item(
    connection: &mut sqlx::PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    item: &LockedEquipmentItem,
    command_id: [u8; 16],
    location_kind: i16,
    slot_index: u8,
) -> Result<(), PersistenceError> {
    let post_version = item
        .item_version
        .checked_add(1)
        .ok_or(PersistenceError::CorruptStoredItems)?;
    let post_security = if location_kind == 0 { 1_i16 } else { 2_i16 };
    let changed = sqlx::query(
        "UPDATE item_instances SET item_version = $1, security_state = $2, location_kind = $3, \
         slot_index = $4, instance_id = NULL, pickup_id = NULL, expires_at_tick = NULL, \
         updated_at = transaction_timestamp() WHERE namespace_id = $5 AND account_id = $6 \
         AND character_id = $7 AND item_uid = $8 AND item_version = $9",
    )
    .bind(i64::try_from(post_version).map_err(|_| PersistenceError::CorruptStoredItems)?)
    .bind(post_security)
    .bind(location_kind)
    .bind(i16::from(slot_index))
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(item.item_uid.as_slice())
    .bind(i64::try_from(item.item_version).map_err(|_| PersistenceError::CorruptStoredItems)?)
    .execute(&mut *connection)
    .await
    .map_err(PersistenceError::Database)?
    .rows_affected();
    if changed != 1 {
        return Err(PersistenceError::FieldEquipmentVersionMismatch);
    }
    let ledger_event_id = transition_event_id(command_id, item.item_uid);
    sqlx::query(
        "INSERT INTO item_ledger_events (namespace_id, ledger_event_id, item_uid, account_id, \
         character_id, mutation_id, event_kind, source_kind, pre_item_version, post_item_version, \
         pre_security_state, post_security_state, pre_location_kind, post_location_kind) \
         VALUES ($1, $2, $3, $4, $5, $6, 1, 2, $7, $8, $9, $10, $11, $12)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ledger_event_id.as_slice())
    .bind(item.item_uid.as_slice())
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(command_id.as_slice())
    .bind(i64::try_from(item.item_version).map_err(|_| PersistenceError::CorruptStoredItems)?)
    .bind(i64::try_from(post_version).map_err(|_| PersistenceError::CorruptStoredItems)?)
    .bind(item.security_state)
    .bind(post_security)
    .bind(item.location_kind)
    .bind(location_kind)
    .execute(connection)
    .await
    .map_err(PersistenceError::Database)?;
    Ok(())
}

fn transition_event_id(command_id: [u8; 16], item_uid: [u8; 16]) -> [u8; 16] {
    let mut material = [0_u8; 32];
    material[..16].copy_from_slice(&command_id);
    material[16..].copy_from_slice(&item_uid);
    let hash = blake3::derive_key("gravebound.field-equipment-ledger.v1", &material);
    let mut event = [0; 16];
    event.copy_from_slice(&hash[..16]);
    event
}

async fn load_result(
    connection: &mut sqlx::PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    command: &StoredFieldEquipmentCommand,
) -> Result<Option<StoredFieldEquipmentResult>, PersistenceError> {
    let row = load_result_row(connection, account_id, character_id, command.command_id).await?;
    let Some(row) = row else { return Ok(None) };
    decode_result_row(
        &row,
        command.command_id,
        command.canonical_request_hash,
        true,
    )
    .map(Some)
}

async fn load_result_row(
    connection: &mut sqlx::PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    command_id: [u8; 16],
) -> Result<Option<sqlx::postgres::PgRow>, PersistenceError> {
    sqlx::query(
        "SELECT canonical_request_hash, result_hash, pre_inventory_version, \
         post_inventory_version, incoming_item_uid, replaced_item_uid, replacement_slot_index \
         FROM field_equipment_mutations WHERE namespace_id = $1 AND account_id = $2 \
         AND character_id = $3 AND command_id = $4",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(command_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)
}

fn decode_result_row(
    row: &sqlx::postgres::PgRow,
    command_id: [u8; 16],
    canonical_request_hash: [u8; 32],
    replayed: bool,
) -> Result<StoredFieldEquipmentResult, PersistenceError> {
    let request_hash: [u8; 32] = fixed_bytes(row.try_get("canonical_request_hash")?)?;
    if request_hash != canonical_request_hash {
        return Err(PersistenceError::ItemIdempotencyConflict);
    }
    Ok(StoredFieldEquipmentResult {
        replayed,
        command_id,
        result_hash: fixed_bytes(row.try_get("result_hash")?)?,
        pre_inventory_version: positive_u64(row.try_get("pre_inventory_version")?)?,
        post_inventory_version: positive_u64(row.try_get("post_inventory_version")?)?,
        incoming_item_uid: fixed_bytes(row.try_get("incoming_item_uid")?)?,
        replaced_item_uid: optional_bytes(row.try_get("replaced_item_uid")?)?,
        replacement_slot_index: optional_index(row.try_get("replacement_slot_index")?)?,
    })
}

async fn insert_result(
    connection: &mut sqlx::PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    command: &StoredFieldEquipmentCommand,
    pre_inventory_version: i64,
    post_inventory_version: i64,
) -> Result<(), PersistenceError> {
    let (source_kind, source_slot, source_instance, source_pickup) = match command.source {
        StoredFieldEquipmentSource::RunBackpack { slot_index } => {
            (0_i16, Some(i16::from(slot_index)), None, None)
        }
        StoredFieldEquipmentSource::PersonalGround {
            instance_id,
            pickup_id,
        } => (1_i16, None, Some(instance_id), Some(pickup_id)),
    };
    sqlx::query(
        "INSERT INTO field_equipment_mutations (namespace_id, account_id, character_id, \
         command_id, canonical_request_hash, preview_hash, result_hash, content_revision, \
         pre_inventory_version, post_inventory_version, incoming_item_uid, replaced_item_uid, \
         source_kind, source_slot_index, source_instance_id, source_pickup_id, \
         replacement_slot_index) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(command.command_id.as_slice())
    .bind(command.canonical_request_hash.as_slice())
    .bind(command.preview_hash.as_slice())
    .bind(command.result_hash.as_slice())
    .bind(&command.content_revision)
    .bind(pre_inventory_version)
    .bind(post_inventory_version)
    .bind(command.incoming_item_uid.as_slice())
    .bind(command.replaced_item_uid.as_ref().map(<[u8; 16]>::as_slice))
    .bind(source_kind)
    .bind(source_slot)
    .bind(source_instance.as_ref().map(<[u8; 16]>::as_slice))
    .bind(source_pickup.as_ref().map(<[u8; 16]>::as_slice))
    .bind(command.replacement_slot_index.map(i16::from))
    .execute(connection)
    .await
    .map_err(PersistenceError::Database)?;
    Ok(())
}

fn result_from_command(
    command: &StoredFieldEquipmentCommand,
    replayed: bool,
) -> Result<StoredFieldEquipmentResult, PersistenceError> {
    Ok(StoredFieldEquipmentResult {
        replayed,
        command_id: command.command_id,
        result_hash: command.result_hash,
        pre_inventory_version: command.expected_inventory_version,
        post_inventory_version: command
            .expected_inventory_version
            .checked_add(1)
            .ok_or(PersistenceError::CorruptStoredItems)?,
        incoming_item_uid: command.incoming_item_uid,
        replaced_item_uid: command.replaced_item_uid,
        replacement_slot_index: command.replacement_slot_index,
    })
}

fn positive_u64(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(PersistenceError::CorruptStoredItems)
}

fn decode_snapshot(
    inventory_version: i64,
    rows: Vec<sqlx::postgres::PgRow>,
) -> Result<StoredFieldEquipmentSnapshot, PersistenceError> {
    let inventory_version = u64::try_from(inventory_version)
        .ok()
        .filter(|version| *version > 0)
        .ok_or(PersistenceError::CorruptStoredItems)?;
    let mut equipment = Vec::new();
    let mut occupied_backpack_slots = Vec::new();
    for row in rows {
        let location_kind: i16 = row.try_get("location_kind")?;
        let slot_index = optional_index(row.try_get("slot_index")?)?;
        if location_kind == 2 {
            let index = slot_index.ok_or(PersistenceError::CorruptStoredItems)?;
            if !occupied_backpack_slots.contains(&index) {
                occupied_backpack_slots.push(index);
            }
        }
        let item_kind: i16 = row.try_get("item_kind")?;
        if item_kind == 1 {
            continue;
        }
        let item_level: i16 = row.try_get("item_level")?;
        let item_version: i64 = row.try_get("item_version")?;
        let expires_at_tick: Option<i64> = row.try_get("expires_at_tick")?;
        let stored = StoredFieldEquipmentItem {
            item_uid: fixed_bytes(row.try_get("item_uid")?)?,
            template_id: row.try_get("template_id")?,
            content_revision: row.try_get("content_revision")?,
            item_level: u8::try_from(item_level)
                .map_err(|_| PersistenceError::CorruptStoredItems)?,
            rarity: row.try_get("rarity")?,
            item_version: u64::try_from(item_version)
                .map_err(|_| PersistenceError::CorruptStoredItems)?,
            security_state: row.try_get("security_state")?,
            location_kind,
            slot_index,
            instance_id: optional_bytes(row.try_get("instance_id")?)?,
            pickup_id: optional_bytes(row.try_get("pickup_id")?)?,
            expires_at_tick: expires_at_tick
                .map(u64::try_from)
                .transpose()
                .map_err(|_| PersistenceError::CorruptStoredItems)?,
        };
        validate_item(&stored)?;
        equipment.push(stored);
    }
    occupied_backpack_slots.sort_unstable();
    Ok(StoredFieldEquipmentSnapshot {
        inventory_version,
        equipment,
        occupied_backpack_slots,
    })
}

fn validate_item(item: &StoredFieldEquipmentItem) -> Result<(), PersistenceError> {
    let location_valid = match item.location_kind {
        0 => {
            item.slot_index.is_some_and(|slot| slot < 4)
                && item.instance_id.is_none()
                && item.pickup_id.is_none()
                && item.expires_at_tick.is_none()
                && matches!(item.security_state, 0 | 1)
        }
        2 => {
            item.slot_index.is_some_and(|slot| slot < 8)
                && item.instance_id.is_none()
                && item.pickup_id.is_none()
                && item.expires_at_tick.is_none()
                && item.security_state == 2
        }
        3 => {
            item.slot_index.is_none()
                && item.instance_id.is_some()
                && item.pickup_id.is_some()
                && item.expires_at_tick.is_some()
                && item.security_state == 2
        }
        _ => false,
    };
    if item.item_uid == [0; 16]
        || item.template_id.is_empty()
        || !item.content_revision.starts_with("core-dev.blake3.")
        || !(1..=10).contains(&item.item_level)
        || !(0..=4).contains(&item.rarity)
        || item.item_version == 0
        || !location_valid
    {
        return Err(PersistenceError::CorruptStoredItems);
    }
    Ok(())
}

fn optional_index(value: Option<i16>) -> Result<Option<u8>, PersistenceError> {
    value
        .map(u8::try_from)
        .transpose()
        .map_err(|_| PersistenceError::CorruptStoredItems)
}

fn optional_bytes(value: Option<Vec<u8>>) -> Result<Option<[u8; 16]>, PersistenceError> {
    value.map(fixed_bytes).transpose()
}

fn fixed_bytes<const N: usize>(bytes: Vec<u8>) -> Result<[u8; N], PersistenceError> {
    bytes
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredItems)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_item_validation_rejects_cross_axis_corruption() {
        let valid = StoredFieldEquipmentItem {
            item_uid: [1; 16],
            template_id: "item.weapon.crossbow.pine_crossbow".to_owned(),
            content_revision: format!("core-dev.blake3.{}", "a".repeat(64)),
            item_level: 1,
            rarity: 0,
            item_version: 1,
            security_state: 2,
            location_kind: 2,
            slot_index: Some(0),
            instance_id: None,
            pickup_id: None,
            expires_at_tick: None,
        };
        assert!(validate_item(&valid).is_ok());
        let mut corrupt = valid.clone();
        corrupt.location_kind = 3;
        assert!(validate_item(&corrupt).is_err());
        corrupt = valid;
        corrupt.item_version = 0;
        assert!(validate_item(&corrupt).is_err());
    }

    fn command() -> StoredFieldEquipmentCommand {
        StoredFieldEquipmentCommand {
            command_id: [1; 16],
            canonical_request_hash: [2; 32],
            preview_hash: [3; 32],
            result_hash: [4; 32],
            content_revision: format!("core-dev.blake3.{}", "a".repeat(64)),
            expected_inventory_version: 7,
            incoming_item_uid: [5; 16],
            incoming_item_version: 2,
            target_slot_index: 0,
            replaced_item_uid: Some([6; 16]),
            replaced_item_version: Some(3),
            source: StoredFieldEquipmentSource::RunBackpack { slot_index: 4 },
            replacement_slot_index: Some(4),
        }
    }

    #[test]
    fn command_shape_and_ledger_identity_are_exact() {
        let valid = command();
        assert!(validate_command(&valid).is_ok());
        assert_eq!(
            transition_event_id(valid.command_id, valid.incoming_item_uid),
            transition_event_id(valid.command_id, valid.incoming_item_uid)
        );
        assert_ne!(
            transition_event_id(valid.command_id, valid.incoming_item_uid),
            transition_event_id(valid.command_id, valid.replaced_item_uid.unwrap())
        );
        let mut corrupt = valid.clone();
        corrupt.replacement_slot_index = Some(3);
        // Cross-field equality is checked under the transactional source binding.
        assert!(validate_command(&corrupt).is_ok());
        corrupt.replaced_item_version = None;
        assert!(validate_command(&corrupt).is_err());
        corrupt = valid;
        corrupt.command_id = [0; 16];
        assert!(validate_command(&corrupt).is_err());
    }
}
