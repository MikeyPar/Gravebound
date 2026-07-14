use sqlx::Row;

use crate::{PersistenceError, PostgresPersistence, WIPEABLE_CORE_NAMESPACE};

pub const CORE_ITEM_LIFECYCLE_SIGNATURE_CONTEXT: &str = "gravebound.m03-04g.lifecycle-signature.v1";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredCoreItemLifecycleSignatureV1 {
    pub contract_version: u16,
    pub namespace_id: String,
    pub account_id: [u8; 16],
    pub character_id: [u8; 16],
    pub account_version: u64,
    pub selected_character_id: [u8; 16],
    pub slot_capacity: u16,
    pub character: StoredLifecycleCharacterV1,
    pub world: StoredLifecycleWorldV1,
    pub progression: Option<StoredLifecycleProgressionV1>,
    pub inventory_version: u64,
    pub capacities: StoredLifecycleCapacitiesV1,
    pub items: Vec<StoredLifecycleItemV1>,
    pub safe_inventory_receipts: Vec<StoredLifecycleSafeInventoryReceiptV1>,
    pub ledger: Vec<StoredLifecycleLedgerEntryV1>,
}

impl StoredCoreItemLifecycleSignatureV1 {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, PersistenceError> {
        validate_signature(self)?;
        postcard::to_stdvec(self).map_err(|_| PersistenceError::CorruptStoredLifecycleSignature)
    }

    pub fn digest(&self) -> Result<[u8; 32], PersistenceError> {
        Ok(blake3::derive_key(
            CORE_ITEM_LIFECYCLE_SIGNATURE_CONTEXT,
            &self.canonical_bytes()?,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredLifecycleCharacterV1 {
    pub roster_ordinal: u16,
    pub class_id: String,
    pub cached_level: u16,
    pub oath_id: Option<String>,
    pub life_state: u16,
    pub security_state: u16,
    pub character_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredLifecycleWorldV1 {
    pub character_version: u64,
    pub location_kind: u16,
    pub location_content_id: Option<String>,
    pub safe_arrival_kind: Option<u16>,
    pub safe_spawn_id: Option<String>,
    pub instance_lineage_id: Option<[u8; 16]>,
    pub entry_restore_point_id: Option<[u8; 16]>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredLifecycleProgressionV1 {
    pub total_xp: u32,
    pub level: u16,
    pub current_health: u32,
    pub progression_version: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct StoredLifecycleCapacitiesV1 {
    pub equipment: u16,
    pub belt: u16,
    pub run_backpack: u16,
    pub character_safe: u16,
    pub vault: u16,
}

impl Default for StoredLifecycleCapacitiesV1 {
    fn default() -> Self {
        Self {
            equipment: 4,
            belt: 2,
            run_backpack: 8,
            character_safe: 8,
            vault: 160,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredLifecycleItemV1 {
    pub item_uid: [u8; 16],
    pub character_id: Option<[u8; 16]>,
    pub template_id: String,
    pub content_revision: String,
    pub item_kind: u16,
    pub item_level: Option<u16>,
    pub rarity: Option<u16>,
    pub creation_kind: u16,
    pub creation_request_id: [u8; 16],
    pub roll_index: u16,
    pub unit_ordinal: u16,
    pub item_version: u64,
    pub security_state: u16,
    pub location_kind: u16,
    pub slot_index: Option<u16>,
    pub provenance_kind: u16,
    pub salvage_band: u16,
    pub salvage_value: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredLifecycleSafeInventoryPlacementV1 {
    pub ordinal: u16,
    pub item_uid: [u8; 16],
    pub destination_kind: u16,
    pub destination_slot_index: u16,
    pub pre_item_version: u64,
    pub post_item_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredLifecycleSafeInventoryReceiptV1 {
    pub mutation_id: [u8; 16],
    pub command_kind: u16,
    pub source_slot_index: u16,
    pub canonical_request_hash: [u8; 32],
    pub pre_account_version: u64,
    pub post_account_version: u64,
    pub pre_inventory_version: u64,
    pub post_inventory_version: u64,
    pub result_hash: [u8; 32],
    pub placements: Vec<StoredLifecycleSafeInventoryPlacementV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredLifecycleLedgerEntryV1 {
    pub ledger_event_id: [u8; 16],
    pub item_uid: [u8; 16],
    pub acting_character_id: [u8; 16],
    pub mutation_id: [u8; 16],
    pub event_kind: u16,
    pub source_kind: u16,
    pub pre_item_version: u64,
    pub post_item_version: u64,
    pub pre_security_state: Option<u16>,
    pub post_security_state: u16,
    pub pre_location_kind: Option<u16>,
    pub post_location_kind: u16,
    pub reason: Option<String>,
}

impl PostgresPersistence {
    /// Reads all signature rows from one SERIALIZABLE snapshot and excludes volatile timestamps,
    /// connection identity, session identity, and transport sequencing by construction.
    // Keeping the complete row projection together makes the one-snapshot audit boundary explicit;
    // splitting it across domain readers would silently open multiple transactions.
    #[allow(clippy::too_many_lines)]
    pub async fn core_item_lifecycle_signature_v1(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> Result<StoredCoreItemLifecycleSignatureV1, PersistenceError> {
        let mut transaction = self.begin_transaction().await?;
        let account = sqlx::query(
            "SELECT state_version,selected_character_id,slot_capacity FROM accounts \
             WHERE namespace_id=$1 AND account_id=$2",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .fetch_one(transaction.connection())
        .await?;
        let selected_character_id = required_id(account.try_get("selected_character_id")?)?;
        if selected_character_id != character_id {
            return Err(PersistenceError::CorruptStoredLifecycleSignature);
        }
        let character_row = sqlx::query(
            "SELECT roster_ordinal,class_id,level,oath_id,life_state,security_state, \
             character_state_version FROM characters WHERE namespace_id=$1 AND account_id=$2 \
             AND character_id=$3",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .fetch_one(transaction.connection())
        .await?;
        let world_row = sqlx::query(
            "SELECT character_version,location_kind,location_content_id,safe_arrival_kind, \
             safe_spawn_id,instance_lineage_id,entry_restore_point_id FROM character_world_locations \
             WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .fetch_one(transaction.connection())
        .await?;
        let progression = sqlx::query(
            "SELECT total_xp,level,current_health,progression_version FROM character_progression \
             WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .fetch_optional(transaction.connection())
        .await?
        .map(|row| {
            Ok::<StoredLifecycleProgressionV1, PersistenceError>(StoredLifecycleProgressionV1 {
                total_xp: unsigned(row.try_get::<i32, _>("total_xp")?)?,
                level: unsigned(row.try_get::<i16, _>("level")?)?,
                current_health: unsigned(row.try_get::<i32, _>("current_health")?)?,
                progression_version: unsigned(row.try_get::<i64, _>("progression_version")?)?,
            })
        })
        .transpose()?;
        let inventory_version: i64 = sqlx::query_scalar(
            "SELECT inventory_version FROM character_inventories WHERE namespace_id=$1 \
             AND account_id=$2 AND character_id=$3",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .fetch_one(transaction.connection())
        .await?;

        let item_rows = sqlx::query(
            "SELECT item_uid,character_id,template_id,content_revision,item_kind,item_level,rarity, \
             creation_kind,creation_request_id,roll_index,unit_ordinal,item_version,security_state, \
             location_kind,slot_index,provenance_kind,salvage_band,salvage_value FROM item_instances \
             WHERE namespace_id=$1 AND account_id=$2 ORDER BY item_uid",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .fetch_all(transaction.connection())
        .await?;
        let mut items = Vec::with_capacity(item_rows.len());
        for row in item_rows {
            items.push(StoredLifecycleItemV1 {
                item_uid: required_id(row.try_get("item_uid")?)?,
                character_id: optional_id(row.try_get("character_id")?)?,
                template_id: row.try_get("template_id")?,
                content_revision: row.try_get("content_revision")?,
                item_kind: unsigned(row.try_get::<i16, _>("item_kind")?)?,
                item_level: optional_unsigned(row.try_get::<Option<i16>, _>("item_level")?)?,
                rarity: optional_unsigned(row.try_get::<Option<i16>, _>("rarity")?)?,
                creation_kind: unsigned(row.try_get::<i16, _>("creation_kind")?)?,
                creation_request_id: required_id(row.try_get("creation_request_id")?)?,
                roll_index: unsigned(row.try_get::<i32, _>("roll_index")?)?,
                unit_ordinal: unsigned(row.try_get::<i32, _>("unit_ordinal")?)?,
                item_version: unsigned(row.try_get::<i64, _>("item_version")?)?,
                security_state: unsigned(row.try_get::<i16, _>("security_state")?)?,
                location_kind: unsigned(row.try_get::<i16, _>("location_kind")?)?,
                slot_index: optional_unsigned(row.try_get::<Option<i16>, _>("slot_index")?)?,
                provenance_kind: unsigned(row.try_get::<i16, _>("provenance_kind")?)?,
                salvage_band: unsigned(row.try_get::<i16, _>("salvage_band")?)?,
                salvage_value: unsigned(row.try_get::<i32, _>("salvage_value")?)?,
            });
        }

        let receipt_rows = sqlx::query(
            "SELECT mutation_id,command_kind,source_slot_index,canonical_request_hash, \
             pre_account_version,post_account_version,pre_inventory_version,post_inventory_version, \
             result_hash FROM safe_inventory_mutations WHERE namespace_id=$1 AND account_id=$2 \
             AND character_id=$3 ORDER BY mutation_id",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .fetch_all(transaction.connection())
        .await?;
        let mut safe_inventory_receipts = Vec::with_capacity(receipt_rows.len());
        for row in receipt_rows {
            let mutation_id = required_id(row.try_get("mutation_id")?)?;
            let placement_rows = sqlx::query(
                "SELECT placement_ordinal,item_uid,destination_kind,destination_slot_index, \
                 pre_item_version,post_item_version FROM safe_inventory_placements \
                 WHERE namespace_id=$1 AND account_id=$2 AND mutation_id=$3 \
                 ORDER BY placement_ordinal",
            )
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(account_id.as_slice())
            .bind(mutation_id.as_slice())
            .fetch_all(transaction.connection())
            .await?;
            let mut placements = Vec::with_capacity(placement_rows.len());
            for placement in placement_rows {
                placements.push(StoredLifecycleSafeInventoryPlacementV1 {
                    ordinal: unsigned(placement.try_get::<i16, _>("placement_ordinal")?)?,
                    item_uid: required_id(placement.try_get("item_uid")?)?,
                    destination_kind: unsigned(placement.try_get::<i16, _>("destination_kind")?)?,
                    destination_slot_index: unsigned(
                        placement.try_get::<i16, _>("destination_slot_index")?,
                    )?,
                    pre_item_version: unsigned(placement.try_get::<i64, _>("pre_item_version")?)?,
                    post_item_version: unsigned(placement.try_get::<i64, _>("post_item_version")?)?,
                });
            }
            safe_inventory_receipts.push(StoredLifecycleSafeInventoryReceiptV1 {
                mutation_id,
                command_kind: unsigned(row.try_get::<i16, _>("command_kind")?)?,
                source_slot_index: unsigned(row.try_get::<i16, _>("source_slot_index")?)?,
                canonical_request_hash: required_hash(row.try_get("canonical_request_hash")?)?,
                pre_account_version: unsigned(row.try_get::<i64, _>("pre_account_version")?)?,
                post_account_version: unsigned(row.try_get::<i64, _>("post_account_version")?)?,
                pre_inventory_version: unsigned(row.try_get::<i64, _>("pre_inventory_version")?)?,
                post_inventory_version: unsigned(row.try_get::<i64, _>("post_inventory_version")?)?,
                result_hash: required_hash(row.try_get("result_hash")?)?,
                placements,
            });
        }

        let ledger_rows = sqlx::query(
            "SELECT ledger_event_id,item_uid,character_id,mutation_id,event_kind,source_kind, \
             pre_item_version,post_item_version,pre_security_state,post_security_state, \
             pre_location_kind,post_location_kind,reason FROM item_ledger_events \
             WHERE namespace_id=$1 AND account_id=$2 ORDER BY item_uid,post_item_version,ledger_event_id",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .fetch_all(transaction.connection())
        .await?;
        let mut ledger = Vec::with_capacity(ledger_rows.len());
        for row in ledger_rows {
            ledger.push(StoredLifecycleLedgerEntryV1 {
                ledger_event_id: required_id(row.try_get("ledger_event_id")?)?,
                item_uid: required_id(row.try_get("item_uid")?)?,
                acting_character_id: required_id(row.try_get("character_id")?)?,
                mutation_id: required_id(row.try_get("mutation_id")?)?,
                event_kind: unsigned(row.try_get::<i16, _>("event_kind")?)?,
                source_kind: unsigned(row.try_get::<i16, _>("source_kind")?)?,
                pre_item_version: unsigned(row.try_get::<i64, _>("pre_item_version")?)?,
                post_item_version: unsigned(row.try_get::<i64, _>("post_item_version")?)?,
                pre_security_state: optional_unsigned(
                    row.try_get::<Option<i16>, _>("pre_security_state")?,
                )?,
                post_security_state: unsigned(row.try_get::<i16, _>("post_security_state")?)?,
                pre_location_kind: optional_unsigned(
                    row.try_get::<Option<i16>, _>("pre_location_kind")?,
                )?,
                post_location_kind: unsigned(row.try_get::<i16, _>("post_location_kind")?)?,
                reason: row.try_get("reason")?,
            });
        }
        transaction.rollback().await?;

        let signature = StoredCoreItemLifecycleSignatureV1 {
            contract_version: 1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.to_owned(),
            account_id,
            character_id,
            account_version: unsigned(account.try_get::<i64, _>("state_version")?)?,
            selected_character_id,
            slot_capacity: unsigned(account.try_get::<i16, _>("slot_capacity")?)?,
            character: StoredLifecycleCharacterV1 {
                roster_ordinal: unsigned(character_row.try_get::<i16, _>("roster_ordinal")?)?,
                class_id: character_row.try_get("class_id")?,
                cached_level: unsigned(character_row.try_get::<i32, _>("level")?)?,
                oath_id: character_row.try_get("oath_id")?,
                life_state: unsigned(character_row.try_get::<i16, _>("life_state")?)?,
                security_state: unsigned(character_row.try_get::<i16, _>("security_state")?)?,
                character_version: unsigned(
                    character_row.try_get::<i64, _>("character_state_version")?,
                )?,
            },
            world: StoredLifecycleWorldV1 {
                character_version: unsigned(world_row.try_get::<i64, _>("character_version")?)?,
                location_kind: unsigned(world_row.try_get::<i16, _>("location_kind")?)?,
                location_content_id: world_row.try_get("location_content_id")?,
                safe_arrival_kind: optional_unsigned(
                    world_row.try_get::<Option<i16>, _>("safe_arrival_kind")?,
                )?,
                safe_spawn_id: world_row.try_get("safe_spawn_id")?,
                instance_lineage_id: optional_id(world_row.try_get("instance_lineage_id")?)?,
                entry_restore_point_id: optional_id(world_row.try_get("entry_restore_point_id")?)?,
            },
            progression,
            inventory_version: unsigned(inventory_version)?,
            capacities: StoredLifecycleCapacitiesV1::default(),
            items,
            safe_inventory_receipts,
            ledger,
        };
        validate_signature(&signature)?;
        Ok(signature)
    }
}

fn validate_signature(
    signature: &StoredCoreItemLifecycleSignatureV1,
) -> Result<(), PersistenceError> {
    if signature.contract_version != 1
        || signature.namespace_id != WIPEABLE_CORE_NAMESPACE
        || signature.account_id == [0; 16]
        || signature.character_id == [0; 16]
        || signature.selected_character_id != signature.character_id
        || signature.account_version == 0
        || signature.inventory_version == 0
        || signature.slot_capacity != 2
        || signature.character.character_version == 0
        || signature.world.character_version != signature.character.character_version
        || signature.capacities != StoredLifecycleCapacitiesV1::default()
        || !signature
            .items
            .windows(2)
            .all(|pair| pair[0].item_uid < pair[1].item_uid)
        || !signature
            .safe_inventory_receipts
            .windows(2)
            .all(|pair| pair[0].mutation_id < pair[1].mutation_id)
        || !signature.ledger.windows(2).all(|pair| {
            (
                pair[0].item_uid,
                pair[0].post_item_version,
                pair[0].ledger_event_id,
            ) < (
                pair[1].item_uid,
                pair[1].post_item_version,
                pair[1].ledger_event_id,
            )
        })
        || signature.safe_inventory_receipts.iter().any(|receipt| {
            receipt.mutation_id == [0; 16]
                || receipt.canonical_request_hash == [0; 32]
                || receipt.result_hash == [0; 32]
                || receipt.placements.is_empty()
                || !receipt
                    .placements
                    .iter()
                    .enumerate()
                    .all(|(index, placement)| usize::from(placement.ordinal) == index)
        })
    {
        return Err(PersistenceError::CorruptStoredLifecycleSignature);
    }
    Ok(())
}

fn required_id(bytes: Vec<u8>) -> Result<[u8; 16], PersistenceError> {
    let value = bytes
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredLifecycleSignature)?;
    if value == [0; 16] {
        return Err(PersistenceError::CorruptStoredLifecycleSignature);
    }
    Ok(value)
}

fn optional_id(bytes: Option<Vec<u8>>) -> Result<Option<[u8; 16]>, PersistenceError> {
    bytes.map(required_id).transpose()
}

fn required_hash(bytes: Vec<u8>) -> Result<[u8; 32], PersistenceError> {
    let value = bytes
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredLifecycleSignature)?;
    if value == [0; 32] {
        return Err(PersistenceError::CorruptStoredLifecycleSignature);
    }
    Ok(value)
}

fn unsigned<T, U>(value: T) -> Result<U, PersistenceError>
where
    U: TryFrom<T>,
{
    U::try_from(value).map_err(|_| PersistenceError::CorruptStoredLifecycleSignature)
}

fn optional_unsigned<T, U>(value: Option<T>) -> Result<Option<U>, PersistenceError>
where
    U: TryFrom<T>,
{
    value.map(unsigned).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signature() -> StoredCoreItemLifecycleSignatureV1 {
        StoredCoreItemLifecycleSignatureV1 {
            contract_version: 1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.to_owned(),
            account_id: [1; 16],
            character_id: [2; 16],
            account_version: 2,
            selected_character_id: [2; 16],
            slot_capacity: 2,
            character: StoredLifecycleCharacterV1 {
                roster_ordinal: 1,
                class_id: "class.grave_arbalist".to_owned(),
                cached_level: 1,
                oath_id: None,
                life_state: 0,
                security_state: 0,
                character_version: 1,
            },
            world: StoredLifecycleWorldV1 {
                character_version: 1,
                location_kind: 1,
                location_content_id: Some("hub.lantern_halls_01".to_owned()),
                safe_arrival_kind: Some(0),
                safe_spawn_id: None,
                instance_lineage_id: None,
                entry_restore_point_id: None,
            },
            progression: Some(StoredLifecycleProgressionV1 {
                total_xp: 0,
                level: 1,
                current_health: 120,
                progression_version: 1,
            }),
            inventory_version: 2,
            capacities: StoredLifecycleCapacitiesV1::default(),
            items: vec![],
            safe_inventory_receipts: vec![],
            ledger: vec![],
        }
    }

    #[test]
    fn digest_changes_with_authoritative_state() {
        let original = signature();
        let mut changed = original.clone();
        changed.inventory_version += 1;
        assert_ne!(original.digest().unwrap(), changed.digest().unwrap());
    }

    #[test]
    fn invalid_binding_fails_closed() {
        let mut value = signature();
        value.selected_character_id = [3; 16];
        assert!(matches!(
            value.canonical_bytes(),
            Err(PersistenceError::CorruptStoredLifecycleSignature)
        ));
    }
}
