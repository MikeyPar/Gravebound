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

impl PostgresPersistence {
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
}
