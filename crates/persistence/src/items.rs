use sqlx::Row;

use crate::{PersistenceError, PostgresPersistence, WIPEABLE_CORE_NAMESPACE};

pub const STARTER_INITIALIZER_REVISION: &str = "starter.core-dev.v1";
pub const STARTER_ITEM_COUNT: usize = 4;
pub const CORE_ITEM_CONTENT_REVISION: &str =
    "core-dev.blake3.27818db710b7553520a162f6f8337dcd0419c459d20c6513a7e12c78fed24ebb";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredStarterItem {
    pub item_uid: [u8; 16],
    pub ledger_event_id: [u8; 16],
    pub template_id: String,
    pub item_kind: i16,
    pub item_level: Option<i16>,
    pub rarity: Option<i16>,
    pub roll_index: i32,
    pub unit_ordinal: i32,
    pub location_kind: i16,
    pub slot_index: i16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredStarterInitialization {
    pub replayed: bool,
    pub pre_inventory_version: i64,
    pub post_inventory_version: i64,
    pub result_hash: [u8; 32],
    pub items: Vec<StoredStarterItem>,
}

impl PostgresPersistence {
    pub async fn initialize_starter_items(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
        request_hash: [u8; 32],
        result_hash: [u8; 32],
        items: &[StoredStarterItem],
    ) -> Result<StoredStarterInitialization, PersistenceError> {
        validate_initializer_input(request_hash, result_hash, items)?;
        let mut transaction = self.begin_transaction().await?;
        let inventory_version =
            lock_or_create_inventory(transaction.connection(), account_id, character_id).await?;

        if let Some(row) = sqlx::query(
            "SELECT request_hash, result_hash, pre_inventory_version, post_inventory_version \
             FROM starter_initializer_results WHERE namespace_id = $1 AND account_id = $2 \
             AND character_id = $3 AND initializer_revision = $4",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .bind(STARTER_INITIALIZER_REVISION)
        .fetch_optional(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?
        {
            let stored_request = fixed_bytes::<32>(row.try_get("request_hash")?)?;
            if stored_request != request_hash {
                transaction.rollback().await?;
                return Err(PersistenceError::ItemIdempotencyConflict);
            }
            let stored_result = fixed_bytes::<32>(row.try_get("result_hash")?)?;
            let pre_inventory_version = row.try_get("pre_inventory_version")?;
            let post_inventory_version = row.try_get("post_inventory_version")?;
            let stored_items =
                load_starter_items(transaction.connection(), account_id, character_id).await?;
            validate_initializer_input(stored_request, stored_result, &stored_items)?;
            transaction.rollback().await?;
            return Ok(StoredStarterInitialization {
                replayed: true,
                pre_inventory_version,
                post_inventory_version,
                result_hash: stored_result,
                items: stored_items,
            });
        }

        for item in items {
            insert_starter_item(transaction.connection(), account_id, character_id, item).await?;
        }
        let post_inventory_version = inventory_version
            .checked_add(1)
            .ok_or(PersistenceError::CorruptStoredItems)?;
        sqlx::query(
            "UPDATE character_inventories SET inventory_version = $1, \
             updated_at = transaction_timestamp() WHERE namespace_id = $2 \
             AND account_id = $3 AND character_id = $4",
        )
        .bind(post_inventory_version)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .execute(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?;
        sqlx::query(
            "INSERT INTO starter_initializer_results \
             (namespace_id, account_id, character_id, initializer_revision, request_hash, \
              result_hash, pre_inventory_version, post_inventory_version) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .bind(STARTER_INITIALIZER_REVISION)
        .bind(request_hash.as_slice())
        .bind(result_hash.as_slice())
        .bind(inventory_version)
        .bind(post_inventory_version)
        .execute(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?;
        transaction.commit().await?;
        Ok(StoredStarterInitialization {
            replayed: false,
            pre_inventory_version: inventory_version,
            post_inventory_version,
            result_hash,
            items: items.to_vec(),
        })
    }
}

pub(crate) async fn lock_or_create_inventory(
    connection: &mut sqlx::PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<i64, PersistenceError> {
    let character = sqlx::query_scalar::<_, i32>(
        "SELECT 1 FROM characters WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?;
    if character.is_none() {
        return Err(PersistenceError::ItemCharacterNotFound);
    }
    sqlx::query(
        "INSERT INTO character_inventories \
         (namespace_id, account_id, character_id, inventory_version) \
         VALUES ($1, $2, $3, 1) ON CONFLICT DO NOTHING",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(&mut *connection)
    .await?;
    sqlx::query_scalar(
        "SELECT inventory_version FROM character_inventories WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_one(connection)
    .await
    .map_err(PersistenceError::Database)
}

async fn insert_starter_item(
    connection: &mut sqlx::PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    item: &StoredStarterItem,
) -> Result<(), PersistenceError> {
    let content_revision = starter_content_revision(item)?;
    sqlx::query(
        "INSERT INTO item_instances \
         (namespace_id, item_uid, account_id, character_id, template_id, content_revision, \
          item_kind, item_level, rarity, creation_kind, creation_request_id, roll_index, \
          unit_ordinal, item_version, security_state, location_kind, slot_index, provenance_kind, \
          salvage_band, salvage_value) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 0, $4, $10, $11, 1, 0, $12, $13, \
          $14, 0, 0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(item.item_uid.as_slice())
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(&item.template_id)
    .bind(content_revision)
    .bind(item.item_kind)
    .bind(item.item_level)
    .bind(item.rarity)
    .bind(item.roll_index)
    .bind(item.unit_ordinal)
    .bind(item.location_kind)
    .bind(item.slot_index)
    .bind(if item.item_kind == 0 { 0_i16 } else { 4_i16 })
    .execute(&mut *connection)
    .await
    .map_err(PersistenceError::Database)?;
    sqlx::query(
        "INSERT INTO item_ledger_events \
         (namespace_id, ledger_event_id, item_uid, account_id, character_id, mutation_id, \
          event_kind, source_kind, pre_item_version, post_item_version, post_security_state, \
          post_location_kind) VALUES ($1, $2, $3, $4, $5, $5, 0, 0, 0, 1, 0, $6)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(item.ledger_event_id.as_slice())
    .bind(item.item_uid.as_slice())
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(item.location_kind)
    .execute(connection)
    .await
    .map_err(PersistenceError::Database)?;
    Ok(())
}

async fn load_starter_items(
    connection: &mut sqlx::PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<Vec<StoredStarterItem>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT i.item_uid, l.ledger_event_id, i.template_id, i.item_kind, i.item_level, \
         i.rarity, i.roll_index, i.unit_ordinal, i.location_kind, i.slot_index \
         FROM item_instances i JOIN item_ledger_events l ON l.namespace_id = i.namespace_id \
         AND l.item_uid = i.item_uid AND l.post_item_version = 1 \
         WHERE i.namespace_id = $1 AND i.account_id = $2 AND i.character_id = $3 \
         AND i.creation_kind = 0 AND i.creation_request_id = $3 \
         ORDER BY i.roll_index, i.unit_ordinal, i.item_uid",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_all(connection)
    .await
    .map_err(PersistenceError::Database)?;
    rows.into_iter()
        .map(|row| {
            Ok(StoredStarterItem {
                item_uid: fixed_bytes(row.try_get("item_uid")?)?,
                ledger_event_id: fixed_bytes(row.try_get("ledger_event_id")?)?,
                template_id: row.try_get("template_id")?,
                item_kind: row.try_get("item_kind")?,
                item_level: row.try_get("item_level")?,
                rarity: row.try_get("rarity")?,
                roll_index: row.try_get("roll_index")?,
                unit_ordinal: row.try_get("unit_ordinal")?,
                location_kind: row.try_get("location_kind")?,
                slot_index: row.try_get("slot_index")?,
            })
        })
        .collect()
}

fn starter_content_revision(item: &StoredStarterItem) -> Result<&'static str, PersistenceError> {
    // The exact hash is the approved 04C manifest revision; no mutable development label is legal.
    validate_starter_item(item)?;
    Ok(CORE_ITEM_CONTENT_REVISION)
}

fn validate_initializer_input(
    request_hash: [u8; 32],
    result_hash: [u8; 32],
    items: &[StoredStarterItem],
) -> Result<(), PersistenceError> {
    if request_hash == [0; 32] || result_hash == [0; 32] || items.len() != STARTER_ITEM_COUNT {
        return Err(PersistenceError::CorruptStoredItems);
    }
    for item in items {
        validate_starter_item(item)?;
    }
    let expected = [
        ("item.weapon.crossbow.pine_crossbow", 0, 0, 0, 0, 0),
        ("item.relic.arbalist.cracked_mark_lens", 0, 0, 1, 1, 0),
        ("consumable.red_tonic", 1, 1, 0, 2, 0),
        ("consumable.red_tonic", 1, 1, 0, 2, 1),
    ];
    for (item, (template, kind, location, slot, roll, unit)) in items.iter().zip(expected) {
        if item.template_id != template
            || item.item_kind != kind
            || item.location_kind != location
            || item.slot_index != slot
            || item.roll_index != roll
            || item.unit_ordinal != unit
        {
            return Err(PersistenceError::CorruptStoredItems);
        }
    }
    for (index, item) in items.iter().enumerate() {
        if items[..index].iter().any(|other| {
            other.item_uid == item.item_uid || other.ledger_event_id == item.ledger_event_id
        }) {
            return Err(PersistenceError::CorruptStoredItems);
        }
    }
    if items[2].unit_ordinal != 0 || items[3].unit_ordinal != 1 {
        return Err(PersistenceError::CorruptStoredItems);
    }
    Ok(())
}

fn validate_starter_item(item: &StoredStarterItem) -> Result<(), PersistenceError> {
    if item.item_uid == [0; 16]
        || item.ledger_event_id == [0; 16]
        || !(0..=u16::MAX.into()).contains(&item.roll_index)
        || !(0..=u16::MAX.into()).contains(&item.unit_ordinal)
        || (item.item_kind == 0 && (item.item_level != Some(1) || item.rarity != Some(0)))
        || (item.item_kind == 1 && (item.item_level.is_some() || item.rarity.is_some()))
    {
        return Err(PersistenceError::CorruptStoredItems);
    }
    Ok(())
}

fn fixed_bytes<const N: usize>(bytes: Vec<u8>) -> Result<[u8; N], PersistenceError> {
    bytes
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredItems)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn starter_items() -> Vec<StoredStarterItem> {
        vec![
            StoredStarterItem {
                item_uid: [1; 16],
                ledger_event_id: [11; 16],
                template_id: "item.weapon.crossbow.pine_crossbow".to_owned(),
                item_kind: 0,
                item_level: Some(1),
                rarity: Some(0),
                roll_index: 0,
                unit_ordinal: 0,
                location_kind: 0,
                slot_index: 0,
            },
            StoredStarterItem {
                item_uid: [2; 16],
                ledger_event_id: [12; 16],
                template_id: "item.relic.arbalist.cracked_mark_lens".to_owned(),
                item_kind: 0,
                item_level: Some(1),
                rarity: Some(0),
                roll_index: 1,
                unit_ordinal: 0,
                location_kind: 0,
                slot_index: 1,
            },
            StoredStarterItem {
                item_uid: [3; 16],
                ledger_event_id: [13; 16],
                template_id: "consumable.red_tonic".to_owned(),
                item_kind: 1,
                item_level: None,
                rarity: None,
                roll_index: 2,
                unit_ordinal: 0,
                location_kind: 1,
                slot_index: 0,
            },
            StoredStarterItem {
                item_uid: [4; 16],
                ledger_event_id: [14; 16],
                template_id: "consumable.red_tonic".to_owned(),
                item_kind: 1,
                item_level: None,
                rarity: None,
                roll_index: 2,
                unit_ordinal: 1,
                location_kind: 1,
                slot_index: 0,
            },
        ]
    }

    #[test]
    fn starter_shape_is_exact_and_unit_normalized() {
        let mut items = starter_items();
        assert!(validate_initializer_input([1; 32], [2; 32], &items).is_ok());
        items[3].unit_ordinal = 0;
        assert!(validate_initializer_input([1; 32], [2; 32], &items).is_err());
        items = starter_items();
        items[2].slot_index = 1;
        assert!(validate_initializer_input([1; 32], [2; 32], &items).is_err());
    }
}
