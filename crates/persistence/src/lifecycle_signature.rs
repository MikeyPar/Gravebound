use sqlx::{PgConnection, Row};

use crate::{
    PersistenceError, PostgresPersistence, WIPEABLE_CORE_NAMESPACE,
    items::CORE_ITEM_CONTENT_REVISION,
};

pub const CORE_ITEM_LIFECYCLE_SIGNATURE_CONTEXT: &str = "gravebound.m03-04g.lifecycle-signature.v1";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredCoreItemLifecycleSignatureV1 {
    pub contract_version: u16,
    pub namespace_id: String,
    pub item_content_revision: String,
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
    pub starter_receipts: Vec<StoredLifecycleStarterReceiptV1>,
    pub xp_receipts: Vec<StoredLifecycleXpReceiptV1>,
    pub boss_first_clears: Vec<StoredLifecycleBossFirstClearV1>,
    pub reward_receipts: Vec<StoredLifecycleRewardReceiptV1>,
    pub equipment_receipts: Vec<StoredLifecycleEquipmentReceiptV1>,
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
pub struct StoredLifecycleStarterReceiptV1 {
    pub initializer_revision: String,
    pub request_hash: [u8; 32],
    pub result_hash: [u8; 32],
    pub pre_inventory_version: u64,
    pub post_inventory_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredLifecycleXpReceiptV1 {
    pub reward_event_id: [u8; 16],
    pub payload_hash: [u8; 32],
    pub source_content_id: String,
    pub xp_profile_id: Option<String>,
    pub progression_content_revision: String,
    pub entry_restore_point_id: Option<[u8; 16]>,
    pub revoked_by_restore_point_id: Option<[u8; 16]>,
    pub revocation_progression_version: Option<u64>,
    pub eligible: bool,
    pub first_clear_awarded: bool,
    pub applied_xp: u32,
    pub discarded_xp: u32,
    pub pre_total_xp: u32,
    pub post_total_xp: u32,
    pub pre_level: u16,
    pub post_level: u16,
    pub pre_progression_version: u64,
    pub post_progression_version: u64,
    pub result_code: u16,
    pub result_payload_hash: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredLifecycleBossFirstClearV1 {
    pub boss_id: String,
    pub reward_event_id: [u8; 16],
    pub character_id: [u8; 16],
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredLifecycleRewardEntryV1 {
    pub roll_index: u16,
    pub template_id: String,
    pub item_kind: u16,
    pub quantity: u16,
    pub item_level: Option<u16>,
    pub rarity: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredLifecycleRewardReceiptV1 {
    pub reward_request_id: [u8; 16],
    pub source_instance_id: [u8; 16],
    pub reward_table_id: String,
    pub content_revision: String,
    pub epoch_id: String,
    pub canonical_request_hash: [u8; 32],
    pub request_state: u16,
    pub plan_hash: Option<[u8; 32]>,
    pub result_hash: Option<[u8; 32]>,
    pub audit_digest: Option<[u8; 32]>,
    pub pre_inventory_version: u64,
    pub post_inventory_version: Option<u64>,
    pub reward_item_count: Option<u16>,
    pub entries: Vec<StoredLifecycleRewardEntryV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredLifecycleEquipmentReceiptV1 {
    pub command_id: [u8; 16],
    pub canonical_request_hash: [u8; 32],
    pub preview_hash: [u8; 32],
    pub result_hash: [u8; 32],
    pub content_revision: String,
    pub pre_inventory_version: u64,
    pub post_inventory_version: u64,
    pub incoming_item_uid: [u8; 16],
    pub replaced_item_uid: Option<[u8; 16]>,
    pub source_kind: u16,
    pub source_slot_index: Option<u16>,
    pub source_instance_id: Option<[u8; 16]>,
    pub source_pickup_id: Option<[u8; 16]>,
    pub replacement_slot_index: Option<u16>,
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
    pub result_code: u16,
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

async fn load_starter_receipts(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<Vec<StoredLifecycleStarterReceiptV1>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT initializer_revision,request_hash,result_hash,pre_inventory_version, \
         post_inventory_version FROM starter_initializer_results WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3 ORDER BY initializer_revision",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(StoredLifecycleStarterReceiptV1 {
                initializer_revision: row.try_get("initializer_revision")?,
                request_hash: required_hash(row.try_get("request_hash")?)?,
                result_hash: required_hash(row.try_get("result_hash")?)?,
                pre_inventory_version: unsigned(row.try_get::<i64, _>("pre_inventory_version")?)?,
                post_inventory_version: unsigned(row.try_get::<i64, _>("post_inventory_version")?)?,
            })
        })
        .collect()
}

async fn load_xp_receipts(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<Vec<StoredLifecycleXpReceiptV1>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT reward_event_id,payload_hash,source_content_id,xp_profile_id, \
         progression_content_revision,entry_restore_point_id,revoked_by_restore_point_id, \
         revocation_progression_version,eligible,first_clear_awarded,applied_xp,discarded_xp, \
         pre_total_xp,post_total_xp,pre_level,post_level,pre_progression_version, \
         post_progression_version,result_code,result_payload FROM character_xp_award_results \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 ORDER BY reward_event_id",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            let result_payload: Vec<u8> = row.try_get("result_payload")?;
            Ok(StoredLifecycleXpReceiptV1 {
                reward_event_id: required_id(row.try_get("reward_event_id")?)?,
                payload_hash: required_hash(row.try_get("payload_hash")?)?,
                source_content_id: row.try_get("source_content_id")?,
                xp_profile_id: row.try_get("xp_profile_id")?,
                progression_content_revision: row.try_get("progression_content_revision")?,
                entry_restore_point_id: optional_id(row.try_get("entry_restore_point_id")?)?,
                revoked_by_restore_point_id: optional_id(
                    row.try_get("revoked_by_restore_point_id")?,
                )?,
                revocation_progression_version: optional_unsigned(
                    row.try_get::<Option<i64>, _>("revocation_progression_version")?,
                )?,
                eligible: row.try_get("eligible")?,
                first_clear_awarded: row.try_get("first_clear_awarded")?,
                applied_xp: unsigned(row.try_get::<i32, _>("applied_xp")?)?,
                discarded_xp: unsigned(row.try_get::<i32, _>("discarded_xp")?)?,
                pre_total_xp: unsigned(row.try_get::<i32, _>("pre_total_xp")?)?,
                post_total_xp: unsigned(row.try_get::<i32, _>("post_total_xp")?)?,
                pre_level: unsigned(row.try_get::<i16, _>("pre_level")?)?,
                post_level: unsigned(row.try_get::<i16, _>("post_level")?)?,
                pre_progression_version: unsigned(
                    row.try_get::<i64, _>("pre_progression_version")?,
                )?,
                post_progression_version: unsigned(
                    row.try_get::<i64, _>("post_progression_version")?,
                )?,
                result_code: unsigned(row.try_get::<i16, _>("result_code")?)?,
                result_payload_hash: *blake3::hash(&result_payload).as_bytes(),
            })
        })
        .collect()
}

async fn load_boss_first_clears(
    connection: &mut PgConnection,
    account_id: [u8; 16],
) -> Result<Vec<StoredLifecycleBossFirstClearV1>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT boss_id,reward_event_id,character_id FROM account_boss_first_clears \
         WHERE namespace_id=$1 AND account_id=$2 ORDER BY boss_id",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(StoredLifecycleBossFirstClearV1 {
                boss_id: row.try_get("boss_id")?,
                reward_event_id: required_id(row.try_get("reward_event_id")?)?,
                character_id: required_id(row.try_get("character_id")?)?,
            })
        })
        .collect()
}

async fn load_reward_receipts(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<Vec<StoredLifecycleRewardReceiptV1>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT reward_request_id,source_instance_id,reward_table_id,content_revision,epoch_id, \
         canonical_request_hash,request_state,plan_hash,result_hash,audit_digest, \
         pre_inventory_version,post_inventory_version,reward_item_count FROM reward_requests \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 ORDER BY reward_request_id",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_all(&mut *connection)
    .await?;
    let mut receipts = Vec::with_capacity(rows.len());
    for row in rows {
        let reward_request_id = required_id(row.try_get("reward_request_id")?)?;
        let entry_rows = sqlx::query(
            "SELECT roll_index,template_id,item_kind,quantity,item_level,rarity \
             FROM reward_result_entries WHERE namespace_id=$1 AND reward_request_id=$2 \
             ORDER BY roll_index",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(reward_request_id.as_slice())
        .fetch_all(&mut *connection)
        .await?;
        let mut entries = Vec::with_capacity(entry_rows.len());
        for entry in entry_rows {
            entries.push(StoredLifecycleRewardEntryV1 {
                roll_index: unsigned(entry.try_get::<i32, _>("roll_index")?)?,
                template_id: entry.try_get("template_id")?,
                item_kind: unsigned(entry.try_get::<i16, _>("item_kind")?)?,
                quantity: unsigned(entry.try_get::<i16, _>("quantity")?)?,
                item_level: optional_unsigned(entry.try_get::<Option<i16>, _>("item_level")?)?,
                rarity: optional_unsigned(entry.try_get::<Option<i16>, _>("rarity")?)?,
            });
        }
        receipts.push(StoredLifecycleRewardReceiptV1 {
            reward_request_id,
            source_instance_id: required_id(row.try_get("source_instance_id")?)?,
            reward_table_id: row.try_get("reward_table_id")?,
            content_revision: row.try_get("content_revision")?,
            epoch_id: row.try_get("epoch_id")?,
            canonical_request_hash: required_hash(row.try_get("canonical_request_hash")?)?,
            request_state: unsigned(row.try_get::<i16, _>("request_state")?)?,
            plan_hash: optional_hash(row.try_get("plan_hash")?)?,
            result_hash: optional_hash(row.try_get("result_hash")?)?,
            audit_digest: optional_hash(row.try_get("audit_digest")?)?,
            pre_inventory_version: unsigned(row.try_get::<i64, _>("pre_inventory_version")?)?,
            post_inventory_version: optional_unsigned(
                row.try_get::<Option<i64>, _>("post_inventory_version")?,
            )?,
            reward_item_count: optional_unsigned(
                row.try_get::<Option<i16>, _>("reward_item_count")?,
            )?,
            entries,
        });
    }
    Ok(receipts)
}

async fn load_equipment_receipts(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<Vec<StoredLifecycleEquipmentReceiptV1>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT command_id,canonical_request_hash,preview_hash,result_hash,content_revision, \
         pre_inventory_version,post_inventory_version,incoming_item_uid,replaced_item_uid, \
         source_kind,source_slot_index,source_instance_id,source_pickup_id,replacement_slot_index \
         FROM field_equipment_mutations WHERE namespace_id=$1 AND account_id=$2 \
         AND character_id=$3 ORDER BY command_id",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(StoredLifecycleEquipmentReceiptV1 {
                command_id: required_id(row.try_get("command_id")?)?,
                canonical_request_hash: required_hash(row.try_get("canonical_request_hash")?)?,
                preview_hash: required_hash(row.try_get("preview_hash")?)?,
                result_hash: required_hash(row.try_get("result_hash")?)?,
                content_revision: row.try_get("content_revision")?,
                pre_inventory_version: unsigned(row.try_get::<i64, _>("pre_inventory_version")?)?,
                post_inventory_version: unsigned(row.try_get::<i64, _>("post_inventory_version")?)?,
                incoming_item_uid: required_id(row.try_get("incoming_item_uid")?)?,
                replaced_item_uid: optional_id(row.try_get("replaced_item_uid")?)?,
                source_kind: unsigned(row.try_get::<i16, _>("source_kind")?)?,
                source_slot_index: optional_unsigned(
                    row.try_get::<Option<i16>, _>("source_slot_index")?,
                )?,
                source_instance_id: optional_id(row.try_get("source_instance_id")?)?,
                source_pickup_id: optional_id(row.try_get("source_pickup_id")?)?,
                replacement_slot_index: optional_unsigned(
                    row.try_get::<Option<i16>, _>("replacement_slot_index")?,
                )?,
            })
        })
        .collect()
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
            "SELECT mutation_id,command_kind,result_code,source_slot_index,canonical_request_hash, \
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
                result_code: unsigned(row.try_get::<i16, _>("result_code")?)?,
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
        let starter_receipts =
            load_starter_receipts(transaction.connection(), account_id, character_id).await?;
        let xp_receipts =
            load_xp_receipts(transaction.connection(), account_id, character_id).await?;
        let boss_first_clears =
            load_boss_first_clears(transaction.connection(), account_id).await?;
        let reward_receipts =
            load_reward_receipts(transaction.connection(), account_id, character_id).await?;
        let equipment_receipts =
            load_equipment_receipts(transaction.connection(), account_id, character_id).await?;
        transaction.rollback().await?;

        let signature = StoredCoreItemLifecycleSignatureV1 {
            contract_version: 1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.to_owned(),
            item_content_revision: CORE_ITEM_CONTENT_REVISION.to_owned(),
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
            starter_receipts,
            xp_receipts,
            boss_first_clears,
            reward_receipts,
            equipment_receipts,
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
        || signature.item_content_revision != CORE_ITEM_CONTENT_REVISION
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
        || signature
            .items
            .iter()
            .any(|item| item.content_revision != signature.item_content_revision)
        || !signature
            .starter_receipts
            .windows(2)
            .all(|pair| pair[0].initializer_revision < pair[1].initializer_revision)
        || signature.starter_receipts.iter().any(|receipt| {
            receipt.request_hash == [0; 32]
                || receipt.result_hash == [0; 32]
                || receipt.pre_inventory_version == 0
                || receipt.post_inventory_version != receipt.pre_inventory_version.saturating_add(1)
        })
        || !signature
            .xp_receipts
            .windows(2)
            .all(|pair| pair[0].reward_event_id < pair[1].reward_event_id)
        || signature.xp_receipts.iter().any(invalid_xp_receipt)
        || !signature
            .boss_first_clears
            .windows(2)
            .all(|pair| pair[0].boss_id < pair[1].boss_id)
        || signature.boss_first_clears.iter().any(|clear| {
            clear.boss_id.is_empty()
                || clear.reward_event_id == [0; 16]
                || clear.character_id == [0; 16]
        })
        || !signature
            .reward_receipts
            .windows(2)
            .all(|pair| pair[0].reward_request_id < pair[1].reward_request_id)
        || signature.reward_receipts.iter().any(invalid_reward_receipt)
        || !signature
            .equipment_receipts
            .windows(2)
            .all(|pair| pair[0].command_id < pair[1].command_id)
        || signature
            .equipment_receipts
            .iter()
            .any(invalid_equipment_receipt)
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
        || signature
            .safe_inventory_receipts
            .iter()
            .any(invalid_safe_inventory_receipt)
    {
        return Err(PersistenceError::CorruptStoredLifecycleSignature);
    }
    Ok(())
}

fn invalid_xp_receipt(receipt: &StoredLifecycleXpReceiptV1) -> bool {
    receipt.reward_event_id == [0; 16]
        || receipt.payload_hash == [0; 32]
        || receipt.result_payload_hash == [0; 32]
        || receipt.source_content_id.is_empty()
        || receipt.progression_content_revision.len() != 64
        || receipt.post_total_xp != receipt.pre_total_xp.saturating_add(receipt.applied_xp)
        || receipt.post_level < receipt.pre_level
        || receipt.pre_progression_version == 0
        || receipt.post_progression_version
            != receipt
                .pre_progression_version
                .saturating_add(u64::from(receipt.applied_xp > 0))
        || receipt.revoked_by_restore_point_id.is_some()
            != receipt.revocation_progression_version.is_some()
}

fn invalid_reward_receipt(receipt: &StoredLifecycleRewardReceiptV1) -> bool {
    let entries_valid = receipt
        .entries
        .windows(2)
        .all(|pair| pair[0].roll_index < pair[1].roll_index)
        && receipt.entries.iter().all(|entry| {
            !entry.template_id.is_empty()
                && entry.quantity > 0
                && matches!(
                    (entry.item_kind, entry.item_level, entry.rarity),
                    (0, Some(1..=10), Some(0..=4)) | (1, None, None)
                )
        });
    let state_valid = match receipt.request_state {
        0 => {
            receipt.plan_hash.is_none()
                && receipt.result_hash.is_none()
                && receipt.audit_digest.is_none()
                && receipt.post_inventory_version.is_none()
                && receipt.reward_item_count.is_none()
                && receipt.entries.is_empty()
        }
        1 => {
            let count = receipt.reward_item_count.map(usize::from);
            let expected_post = receipt
                .pre_inventory_version
                .saturating_add(u64::from(count.is_some_and(|item_count| item_count > 0)));
            receipt.plan_hash.is_some()
                && receipt.result_hash.is_some()
                && receipt.audit_digest.is_some()
                && receipt.post_inventory_version == Some(expected_post)
                && count
                    == Some(
                        receipt
                            .entries
                            .iter()
                            .map(|entry| usize::from(entry.quantity))
                            .sum(),
                    )
        }
        _ => false,
    };
    receipt.reward_request_id == [0; 16]
        || receipt.source_instance_id == [0; 16]
        || receipt.canonical_request_hash == [0; 32]
        || receipt.reward_table_id.is_empty()
        || receipt.content_revision.is_empty()
        || receipt.epoch_id.is_empty()
        || receipt.pre_inventory_version == 0
        || !entries_valid
        || !state_valid
}

fn invalid_equipment_receipt(receipt: &StoredLifecycleEquipmentReceiptV1) -> bool {
    let source_valid = match receipt.source_kind {
        0 => {
            matches!(receipt.source_slot_index, Some(0..=7))
                && receipt.source_instance_id.is_none()
                && receipt.source_pickup_id.is_none()
        }
        1 => {
            receipt.source_slot_index.is_none()
                && receipt.source_instance_id.is_some()
                && receipt.source_pickup_id.is_some()
        }
        _ => false,
    };
    receipt.command_id == [0; 16]
        || receipt.canonical_request_hash == [0; 32]
        || receipt.preview_hash == [0; 32]
        || receipt.result_hash == [0; 32]
        || receipt.content_revision.is_empty()
        || receipt.pre_inventory_version == 0
        || receipt.post_inventory_version != receipt.pre_inventory_version.saturating_add(1)
        || receipt.incoming_item_uid == [0; 16]
        || receipt.replaced_item_uid == Some(receipt.incoming_item_uid)
        || !source_valid
        || !matches!(receipt.replacement_slot_index, None | Some(0..=7))
}

fn invalid_safe_inventory_receipt(receipt: &StoredLifecycleSafeInventoryReceiptV1) -> bool {
    let account_version_valid = match receipt.command_kind {
        0 | 1 => receipt.post_account_version == receipt.pre_account_version.saturating_add(1),
        2 => receipt.post_account_version == receipt.pre_account_version,
        _ => false,
    };
    receipt.mutation_id == [0; 16]
        || receipt.result_code != 1
        || receipt.canonical_request_hash == [0; 32]
        || receipt.result_hash == [0; 32]
        || receipt.pre_account_version == 0
        || !account_version_valid
        || receipt.pre_inventory_version == 0
        || receipt.post_inventory_version != receipt.pre_inventory_version.saturating_add(1)
        || receipt.placements.is_empty()
        || receipt.placements.len() > 6
        || !receipt
            .placements
            .iter()
            .enumerate()
            .all(|(index, placement)| {
                usize::from(placement.ordinal) == index
                    && placement.item_uid != [0; 16]
                    && placement.pre_item_version > 0
                    && placement.post_item_version == placement.pre_item_version.saturating_add(1)
                    && matches!(
                        (placement.destination_kind, placement.destination_slot_index),
                        (2 | 5, 0..=7) | (6, 0..=159)
                    )
            })
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

fn optional_hash(bytes: Option<Vec<u8>>) -> Result<Option<[u8; 32]>, PersistenceError> {
    bytes.map(required_hash).transpose()
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
            item_content_revision: CORE_ITEM_CONTENT_REVISION.to_owned(),
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
            starter_receipts: vec![],
            xp_receipts: vec![],
            boss_first_clears: vec![],
            reward_receipts: vec![],
            equipment_receipts: vec![],
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

    #[test]
    fn safe_receipt_result_code_and_version_shape_are_canonical() {
        let mut value = signature();
        value
            .safe_inventory_receipts
            .push(StoredLifecycleSafeInventoryReceiptV1 {
                mutation_id: [3; 16],
                command_kind: 0,
                result_code: 1,
                source_slot_index: 0,
                canonical_request_hash: [4; 32],
                pre_account_version: 1,
                post_account_version: 2,
                pre_inventory_version: 1,
                post_inventory_version: 2,
                result_hash: [5; 32],
                placements: vec![StoredLifecycleSafeInventoryPlacementV1 {
                    ordinal: 0,
                    item_uid: [6; 16],
                    destination_kind: 6,
                    destination_slot_index: 0,
                    pre_item_version: 1,
                    post_item_version: 2,
                }],
            });
        let canonical = value.canonical_bytes().unwrap();
        value.safe_inventory_receipts[0].result_hash[0] ^= 1;
        assert_ne!(canonical, value.canonical_bytes().unwrap());
        value.safe_inventory_receipts[0].result_code = 0;
        assert!(matches!(
            value.canonical_bytes(),
            Err(PersistenceError::CorruptStoredLifecycleSignature)
        ));
    }
}
