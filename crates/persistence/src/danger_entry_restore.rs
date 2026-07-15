//! Transaction-bound durable components for the `TECH-023` danger-entry restore point.

use std::collections::BTreeSet;

use serde::Serialize;
use sqlx::Row;

use crate::{
    PersistenceError, PersistenceTransaction, WIPEABLE_CORE_NAMESPACE,
    items::CORE_ITEM_CONTENT_REVISION,
};

const MAX_BASELINE_ITEMS: usize = 64;
const MAX_ACTIVE_BARGAINS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDangerEntryInventoryItemV3 {
    pub item_uid: [u8; 16],
    pub template_id: String,
    pub content_revision: String,
    pub item_kind: i16,
    pub creation_kind: i16,
    pub creation_request_id: [u8; 16],
    pub roll_index: i32,
    pub unit_ordinal: i32,
    pub provenance_kind: i16,
    pub location_kind: i16,
    pub slot_index: i16,
    pub entry_item_version: u64,
    pub entry_security_state: i16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDangerEntryInventoryV3 {
    pub pre_inventory_version: u64,
    pub post_inventory_version: u64,
    pub safe_placement_count: u16,
    pub items: Vec<StoredDangerEntryInventoryItemV3>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDangerEntryActiveBargainV3 {
    pub acquisition_ordinal: u8,
    pub bargain_id: String,
    pub acquired_by_offer_id: [u8; 16],
    pub source_reward_event_id: [u8; 16],
    pub content_version: String,
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDangerEntryOathBargainV3 {
    pub oath_id: Option<String>,
    pub active_bargains: Vec<StoredDangerEntryActiveBargainV3>,
    pub earned_bargain_slots: u8,
    pub oath_bargain_version: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredDangerEntryLifeMetricsV3 {
    pub captured_lifetime_ticks: u64,
    pub rollback_permadeath_combat_ticks: u64,
    pub life_metrics_version: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredDangerEntryAshWalletV3 {
    pub ash_wallet_version: u64,
}

/// Converts every equipped/Belt item from Safe to `AtRiskEquipped` and persists those identities
/// together with every pre-entry `RunBackpack` unit. When `CharacterSafe` preflight already moved
/// items, that preflight's single inventory version advance owns the combined Realm Gate mutation.
#[allow(
    clippy::too_many_lines,
    reason = "the lock, validate, transition, version, ledger, and component writes remain contiguous for atomicity audit"
)]
pub async fn stage_danger_entry_inventory_restore_v3(
    transaction: &mut PersistenceTransaction<'_>,
    account_id: [u8; 16],
    character_id: [u8; 16],
    restore_point_id: [u8; 16],
    mutation_id: [u8; 16],
    safe_placement_count: u16,
) -> Result<StoredDangerEntryInventoryV3, PersistenceError> {
    validate_context(account_id, character_id, restore_point_id)?;
    if mutation_id == [0; 16] || safe_placement_count > 48 {
        return Err(PersistenceError::CorruptStoredDangerEntryRestore);
    }
    let current_inventory_version = positive_u64(
        sqlx::query_scalar::<_, i64>(
            "SELECT inventory_version FROM character_inventories WHERE namespace_id = $1 \
             AND account_id = $2 AND character_id = $3 FOR UPDATE",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .fetch_optional(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?
        .ok_or(PersistenceError::CorruptStoredDangerEntryRestore)?,
    )?;
    let rows = sqlx::query(
        "SELECT item_uid, template_id, content_revision, item_kind, creation_kind, \
                creation_request_id, roll_index, unit_ordinal, provenance_kind, item_version, \
                security_state, location_kind, slot_index FROM item_instances \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3 \
           AND location_kind IN (0, 1, 2) \
         ORDER BY location_kind, slot_index, item_uid FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_all(transaction.connection())
    .await
    .map_err(PersistenceError::Database)?;
    if rows.len() > MAX_BASELINE_ITEMS {
        return Err(PersistenceError::CorruptStoredDangerEntryRestore);
    }

    let mut items = Vec::with_capacity(rows.len());
    let mut identities = BTreeSet::new();
    let mut equipment_slots = BTreeSet::new();
    let mut belt_slot_templates = [None::<String>, None::<String>];
    let mut belt_slot_counts = [0_usize; 2];
    let mut backpack_slot_templates = std::array::from_fn::<_, 8, _>(|_| None::<String>);
    let mut backpack_slot_kinds = [None::<i16>; 8];
    let mut backpack_slot_counts = [0_usize; 8];
    for row in rows {
        let item_uid = fixed_bytes(row.try_get("item_uid")?)?;
        let template_id: String = row.try_get("template_id")?;
        let content_revision: String = row.try_get("content_revision")?;
        let item_kind: i16 = row.try_get("item_kind")?;
        let creation_kind: i16 = row.try_get("creation_kind")?;
        let creation_request_id = fixed_bytes(row.try_get("creation_request_id")?)?;
        let roll_index: i32 = row.try_get("roll_index")?;
        let unit_ordinal: i32 = row.try_get("unit_ordinal")?;
        let provenance_kind: i16 = row.try_get("provenance_kind")?;
        let current_item_version = positive_u64(row.try_get("item_version")?)?;
        let security_state: i16 = row.try_get("security_state")?;
        let location_kind: i16 = row.try_get("location_kind")?;
        let slot_index: i16 = row.try_get("slot_index")?;
        if item_uid == [0; 16]
            || !identities.insert(item_uid)
            || !(3..=96).contains(&template_id.len())
            || content_revision != CORE_ITEM_CONTENT_REVISION
            || creation_request_id == [0; 16]
            || !(0..=3).contains(&creation_kind)
            || !(0..=65_535).contains(&roll_index)
            || !(0..=65_535).contains(&unit_ordinal)
            || !(0..=7).contains(&provenance_kind)
        {
            return Err(PersistenceError::CorruptStoredDangerEntryRestore);
        }
        match location_kind {
            0 if security_state == 0 && item_kind == 0 && (0..=3).contains(&slot_index) => {
                if !equipment_slots.insert(slot_index) {
                    return Err(PersistenceError::CorruptStoredDangerEntryRestore);
                }
            }
            1 if security_state == 0 && item_kind == 1 && (0..=1).contains(&slot_index) => {
                let slot = usize::try_from(slot_index)
                    .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?;
                belt_slot_counts[slot] += 1;
                if belt_slot_counts[slot] > 6
                    || belt_slot_templates[slot]
                        .as_ref()
                        .is_some_and(|existing| existing != &template_id)
                {
                    return Err(PersistenceError::CorruptStoredDangerEntryRestore);
                }
                belt_slot_templates[slot].get_or_insert_with(|| template_id.clone());
            }
            2 if security_state == 2
                && matches!(item_kind, 0 | 1)
                && (0..=7).contains(&slot_index) =>
            {
                let slot = usize::try_from(slot_index)
                    .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?;
                backpack_slot_counts[slot] += 1;
                if backpack_slot_kinds[slot].is_some_and(|existing| existing != item_kind)
                    || backpack_slot_templates[slot]
                        .as_ref()
                        .is_some_and(|existing| existing != &template_id)
                    || item_kind == 0 && backpack_slot_counts[slot] > 1
                    || item_kind == 1 && backpack_slot_counts[slot] > 6
                {
                    return Err(PersistenceError::CorruptStoredDangerEntryRestore);
                }
                backpack_slot_kinds[slot].get_or_insert(item_kind);
                backpack_slot_templates[slot].get_or_insert_with(|| template_id.clone());
            }
            _ => return Err(PersistenceError::CorruptStoredDangerEntryRestore),
        }
        let (entry_item_version, entry_security_state) = if location_kind < 2 {
            (
                current_item_version
                    .checked_add(1)
                    .ok_or(PersistenceError::CorruptStoredDangerEntryRestore)?,
                1,
            )
        } else {
            (current_item_version, 2)
        };
        items.push(StoredDangerEntryInventoryItemV3 {
            item_uid,
            template_id,
            content_revision,
            item_kind,
            creation_kind,
            creation_request_id,
            roll_index,
            unit_ordinal,
            provenance_kind,
            location_kind,
            slot_index,
            entry_item_version,
            entry_security_state,
        });
    }

    let (pre_inventory_version, post_inventory_version) = if safe_placement_count > 0 {
        (
            current_inventory_version
                .checked_sub(1)
                .ok_or(PersistenceError::CorruptStoredDangerEntryRestore)?,
            current_inventory_version,
        )
    } else if items.iter().all(|item| item.location_kind == 2) {
        (current_inventory_version, current_inventory_version)
    } else {
        (
            current_inventory_version,
            current_inventory_version
                .checked_add(1)
                .ok_or(PersistenceError::CorruptStoredDangerEntryRestore)?,
        )
    };

    for item in items.iter().filter(|item| item.location_kind < 2) {
        transition_risk_item(transaction, account_id, character_id, mutation_id, item).await?;
    }
    if post_inventory_version != current_inventory_version {
        let changed = sqlx::query(
            "UPDATE character_inventories SET inventory_version = $1, \
                    updated_at = transaction_timestamp() \
             WHERE namespace_id = $2 AND account_id = $3 AND character_id = $4 \
               AND inventory_version = $5",
        )
        .bind(
            i64::try_from(post_inventory_version)
                .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .bind(
            i64::try_from(current_inventory_version)
                .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
        )
        .execute(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?
        .rows_affected();
        if changed != 1 {
            return Err(PersistenceError::CorruptStoredDangerEntryRestore);
        }
    }

    let component_digest = inventory_digest(
        pre_inventory_version,
        post_inventory_version,
        safe_placement_count,
        &items,
    );
    sqlx::query(
        "INSERT INTO entry_restore_inventory_v3 \
         (namespace_id, account_id, character_id, restore_point_id, pre_inventory_version, \
          post_inventory_version, baseline_item_count, safe_placement_count, component_digest) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(restore_point_id.as_slice())
    .bind(
        i64::try_from(pre_inventory_version)
            .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
    )
    .bind(
        i64::try_from(post_inventory_version)
            .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
    )
    .bind(
        i16::try_from(items.len())
            .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
    )
    .bind(
        i16::try_from(safe_placement_count)
            .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
    )
    .bind(component_digest.as_slice())
    .execute(transaction.connection())
    .await
    .map_err(PersistenceError::Database)?;
    for (ordinal, item) in items.iter().enumerate() {
        sqlx::query(
            "INSERT INTO entry_restore_inventory_items_v3 \
             (namespace_id, account_id, character_id, restore_point_id, item_ordinal, \
              item_uid, template_id, content_revision, item_kind, creation_kind, \
              creation_request_id, roll_index, unit_ordinal, \
              provenance_kind, location_kind, slot_index, entry_item_version, \
              entry_security_state) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .bind(restore_point_id.as_slice())
        .bind(
            i16::try_from(ordinal)
                .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
        )
        .bind(item.item_uid.as_slice())
        .bind(&item.template_id)
        .bind(&item.content_revision)
        .bind(item.item_kind)
        .bind(item.creation_kind)
        .bind(item.creation_request_id.as_slice())
        .bind(item.roll_index)
        .bind(item.unit_ordinal)
        .bind(item.provenance_kind)
        .bind(item.location_kind)
        .bind(item.slot_index)
        .bind(
            i64::try_from(item.entry_item_version)
                .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
        )
        .bind(item.entry_security_state)
        .execute(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?;
    }
    Ok(StoredDangerEntryInventoryV3 {
        pre_inventory_version,
        post_inventory_version,
        safe_placement_count,
        items,
    })
}

#[allow(
    clippy::too_many_lines,
    reason = "the authority lock, provenance validation, digest, parent, and ordered child writes remain contiguous for auditability"
)]
pub async fn stage_danger_entry_oath_bargain_restore_v3(
    transaction: &mut PersistenceTransaction<'_>,
    account_id: [u8; 16],
    character_id: [u8; 16],
    restore_point_id: [u8; 16],
) -> Result<StoredDangerEntryOathBargainV3, PersistenceError> {
    validate_context(account_id, character_id, restore_point_id)?;
    let row = sqlx::query(
        "SELECT c.oath_id, ob.earned_bargain_slots, ob.oath_bargain_version \
         FROM characters c JOIN character_oath_bargain_state ob \
         USING (namespace_id, account_id, character_id) WHERE c.namespace_id = $1 \
         AND c.account_id = $2 AND c.character_id = $3 FOR UPDATE OF c, ob",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(transaction.connection())
    .await
    .map_err(PersistenceError::Database)?
    .ok_or(PersistenceError::CorruptStoredDangerEntryRestore)?;
    let oath_id: Option<String> = row.try_get("oath_id")?;
    let earned_bargain_slots: i16 = row.try_get("earned_bargain_slots")?;
    let oath_bargain_version = positive_u64(row.try_get("oath_bargain_version")?)?;
    let active_rows = sqlx::query(
        "SELECT ab.bargain_id, ab.acquisition_ordinal, ab.acquired_by_offer_id, \
                bo.source_reward_event_id, bo.content_version, bo.records_blake3, \
                bo.assets_blake3, bo.localization_blake3, bo.offer_state, \
                bo.selected_bargain_id \
         FROM character_active_bargains ab JOIN bargain_offers bo \
           ON bo.namespace_id = ab.namespace_id AND bo.account_id = ab.account_id \
          AND bo.character_id = ab.character_id AND bo.offer_id = ab.acquired_by_offer_id \
         WHERE ab.namespace_id = $1 AND ab.account_id = $2 AND ab.character_id = $3 \
         ORDER BY ab.acquisition_ordinal FOR UPDATE OF ab, bo",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_all(transaction.connection())
    .await
    .map_err(PersistenceError::Database)?;
    let active_bargains = active_rows
        .iter()
        .map(decode_active_bargain)
        .collect::<Result<Vec<_>, _>>()?;
    let earned_bargain_slots = u8::try_from(earned_bargain_slots)
        .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?;
    if usize::from(earned_bargain_slots) > MAX_ACTIVE_BARGAINS
        || active_bargains.len() > usize::from(earned_bargain_slots)
        || oath_id
            .as_ref()
            .is_some_and(|value| !(3..=96).contains(&value.len()))
        || active_bargains
            .iter()
            .map(|value| &value.bargain_id)
            .collect::<BTreeSet<_>>()
            .len()
            != active_bargains.len()
        || active_bargains
            .iter()
            .map(|value| value.acquired_by_offer_id)
            .collect::<BTreeSet<_>>()
            .len()
            != active_bargains.len()
        || active_bargains
            .iter()
            .enumerate()
            .any(|(index, bargain)| usize::from(bargain.acquisition_ordinal) != index + 1)
    {
        return Err(PersistenceError::CorruptStoredDangerEntryRestore);
    }
    let component_digest = oath_digest(
        oath_id.as_deref(),
        earned_bargain_slots,
        oath_bargain_version,
        &active_bargains,
    );
    sqlx::query(
        "INSERT INTO entry_restore_oath_bargain_v3 \
         (namespace_id, account_id, character_id, restore_point_id, oath_id, \
          earned_bargain_slots, active_bargain_count, oath_bargain_version, component_digest) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(restore_point_id.as_slice())
    .bind(oath_id.as_deref())
    .bind(i16::from(earned_bargain_slots))
    .bind(
        i16::try_from(active_bargains.len())
            .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
    )
    .bind(
        i64::try_from(oath_bargain_version)
            .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
    )
    .bind(component_digest.as_slice())
    .execute(transaction.connection())
    .await
    .map_err(PersistenceError::Database)?;
    for bargain in &active_bargains {
        sqlx::query(
            "INSERT INTO entry_restore_active_bargains_v3 \
             (namespace_id, restore_point_id, acquisition_ordinal, bargain_id, \
              acquired_by_offer_id, source_reward_event_id, content_version, records_blake3, \
              assets_blake3, localization_blake3) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(restore_point_id.as_slice())
        .bind(i16::from(bargain.acquisition_ordinal))
        .bind(&bargain.bargain_id)
        .bind(bargain.acquired_by_offer_id.as_slice())
        .bind(bargain.source_reward_event_id.as_slice())
        .bind(&bargain.content_version)
        .bind(&bargain.records_blake3)
        .bind(&bargain.assets_blake3)
        .bind(&bargain.localization_blake3)
        .execute(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?;
    }
    Ok(StoredDangerEntryOathBargainV3 {
        oath_id,
        active_bargains,
        earned_bargain_slots,
        oath_bargain_version,
    })
}

pub async fn stage_danger_entry_life_metrics_restore_v3(
    transaction: &mut PersistenceTransaction<'_>,
    account_id: [u8; 16],
    character_id: [u8; 16],
    restore_point_id: [u8; 16],
) -> Result<StoredDangerEntryLifeMetricsV3, PersistenceError> {
    validate_context(account_id, character_id, restore_point_id)?;
    let row = sqlx::query(
        "SELECT lifetime_ticks, permadeath_combat_ticks, life_metrics_version \
         FROM character_life_metrics WHERE namespace_id = $1 AND account_id = $2 \
         AND character_id = $3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(transaction.connection())
    .await
    .map_err(PersistenceError::Database)?
    .ok_or(PersistenceError::CorruptStoredDangerEntryRestore)?;
    let captured_lifetime_ticks = nonnegative_u64(row.try_get("lifetime_ticks")?)?;
    let rollback_permadeath_combat_ticks =
        nonnegative_u64(row.try_get("permadeath_combat_ticks")?)?;
    let life_metrics_version = positive_u64(row.try_get("life_metrics_version")?)?;
    let component_digest = life_digest(
        captured_lifetime_ticks,
        rollback_permadeath_combat_ticks,
        life_metrics_version,
    );
    sqlx::query(
        "INSERT INTO entry_restore_life_metrics_v3 \
         (namespace_id, account_id, character_id, restore_point_id, captured_lifetime_ticks, \
          rollback_permadeath_combat_ticks, life_metrics_version, component_digest) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(restore_point_id.as_slice())
    .bind(
        i64::try_from(captured_lifetime_ticks)
            .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
    )
    .bind(
        i64::try_from(rollback_permadeath_combat_ticks)
            .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
    )
    .bind(
        i64::try_from(life_metrics_version)
            .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
    )
    .bind(component_digest.as_slice())
    .execute(transaction.connection())
    .await
    .map_err(PersistenceError::Database)?;
    Ok(StoredDangerEntryLifeMetricsV3 {
        captured_lifetime_ticks,
        rollback_permadeath_combat_ticks,
        life_metrics_version,
    })
}

pub async fn stage_danger_entry_ash_wallet_restore_v3(
    transaction: &mut PersistenceTransaction<'_>,
    account_id: [u8; 16],
    character_id: [u8; 16],
    restore_point_id: [u8; 16],
) -> Result<StoredDangerEntryAshWalletV3, PersistenceError> {
    validate_context(account_id, character_id, restore_point_id)?;
    let ash_wallet_version = positive_u64(
        sqlx::query_scalar::<_, i64>(
            "SELECT wallet_version FROM ash_wallets WHERE namespace_id = $1 \
             AND account_id = $2 FOR UPDATE",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .fetch_optional(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?
        .ok_or(PersistenceError::CorruptStoredDangerEntryRestore)?,
    )?;
    let component_digest = ash_digest(ash_wallet_version);
    sqlx::query(
        "INSERT INTO entry_restore_ash_wallet_v3 \
         (namespace_id, account_id, character_id, restore_point_id, ash_wallet_version, \
          component_digest) VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(restore_point_id.as_slice())
    .bind(
        i64::try_from(ash_wallet_version)
            .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
    )
    .bind(component_digest.as_slice())
    .execute(transaction.connection())
    .await
    .map_err(PersistenceError::Database)?;
    Ok(StoredDangerEntryAshWalletV3 { ash_wallet_version })
}

async fn transition_risk_item(
    transaction: &mut PersistenceTransaction<'_>,
    account_id: [u8; 16],
    character_id: [u8; 16],
    mutation_id: [u8; 16],
    item: &StoredDangerEntryInventoryItemV3,
) -> Result<(), PersistenceError> {
    let pre_item_version = item
        .entry_item_version
        .checked_sub(1)
        .ok_or(PersistenceError::CorruptStoredDangerEntryRestore)?;
    let changed = sqlx::query(
        "UPDATE item_instances SET item_version = $1, security_state = 1, \
                updated_at = transaction_timestamp() \
         WHERE namespace_id = $2 AND account_id = $3 AND character_id = $4 AND item_uid = $5 \
           AND item_version = $6 AND security_state = 0 AND location_kind = $7 AND slot_index = $8",
    )
    .bind(
        i64::try_from(item.entry_item_version)
            .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(item.item_uid.as_slice())
    .bind(
        i64::try_from(pre_item_version)
            .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
    )
    .bind(item.location_kind)
    .bind(item.slot_index)
    .execute(transaction.connection())
    .await
    .map_err(PersistenceError::Database)?
    .rows_affected();
    if changed != 1 {
        return Err(PersistenceError::CorruptStoredDangerEntryRestore);
    }
    let ledger_event_id = risk_transition_event_id(mutation_id, item.item_uid);
    sqlx::query(
        "INSERT INTO item_ledger_events \
         (namespace_id, ledger_event_id, item_uid, account_id, character_id, mutation_id, \
          event_kind, source_kind, pre_item_version, post_item_version, pre_security_state, \
          post_security_state, pre_location_kind, post_location_kind) \
         VALUES ($1,$2,$3,$4,$5,$6,1,2,$7,$8,0,1,$9,$9)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ledger_event_id.as_slice())
    .bind(item.item_uid.as_slice())
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(mutation_id.as_slice())
    .bind(
        i64::try_from(pre_item_version)
            .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
    )
    .bind(
        i64::try_from(item.entry_item_version)
            .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
    )
    .bind(item.location_kind)
    .execute(transaction.connection())
    .await
    .map_err(PersistenceError::Database)?;
    Ok(())
}

fn risk_transition_event_id(mutation_id: [u8; 16], item_uid: [u8; 16]) -> [u8; 16] {
    let mut material = [0_u8; 32];
    material[..16].copy_from_slice(&mutation_id);
    material[16..].copy_from_slice(&item_uid);
    let hash = blake3::derive_key("gravebound.danger-entry-risk-ledger.v3", &material);
    let mut value = [0; 16];
    value.copy_from_slice(&hash[..16]);
    value
}

pub(crate) fn inventory_digest(
    pre_version: u64,
    post_version: u64,
    safe_placement_count: u16,
    items: &[StoredDangerEntryInventoryItemV3],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_derive_key("gravebound.entry-inventory.v3");
    hasher.update(&pre_version.to_le_bytes());
    hasher.update(&post_version.to_le_bytes());
    hasher.update(&safe_placement_count.to_le_bytes());
    for item in items {
        hasher.update(&item.item_uid);
        hasher.update(item.template_id.as_bytes());
        hasher.update(item.content_revision.as_bytes());
        hasher.update(&item.item_kind.to_le_bytes());
        hasher.update(&item.creation_kind.to_le_bytes());
        hasher.update(&item.creation_request_id);
        hasher.update(&item.roll_index.to_le_bytes());
        hasher.update(&item.unit_ordinal.to_le_bytes());
        hasher.update(&item.provenance_kind.to_le_bytes());
        hasher.update(&item.location_kind.to_le_bytes());
        hasher.update(&item.slot_index.to_le_bytes());
        hasher.update(&item.entry_item_version.to_le_bytes());
        hasher.update(&item.entry_security_state.to_le_bytes());
    }
    *hasher.finalize().as_bytes()
}

pub(crate) fn oath_digest(
    oath_id: Option<&str>,
    earned_slots: u8,
    version: u64,
    active: &[StoredDangerEntryActiveBargainV3],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_derive_key("gravebound.entry-oath-bargain.v3");
    if let Some(oath_id) = oath_id {
        hasher.update(&[1]);
        hasher.update(oath_id.as_bytes());
    } else {
        hasher.update(&[0]);
    }
    hasher.update(&[earned_slots]);
    hasher.update(&version.to_le_bytes());
    for bargain in active {
        hasher.update(&[bargain.acquisition_ordinal]);
        hasher.update(bargain.bargain_id.as_bytes());
        hasher.update(&bargain.acquired_by_offer_id);
        hasher.update(&bargain.source_reward_event_id);
        hasher.update(bargain.content_version.as_bytes());
        hasher.update(bargain.records_blake3.as_bytes());
        hasher.update(bargain.assets_blake3.as_bytes());
        hasher.update(bargain.localization_blake3.as_bytes());
    }
    *hasher.finalize().as_bytes()
}

pub(crate) fn life_digest(lifetime_ticks: u64, combat_ticks: u64, version: u64) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_derive_key("gravebound.entry-life-metrics.v3");
    hasher.update(&lifetime_ticks.to_le_bytes());
    hasher.update(&combat_ticks.to_le_bytes());
    hasher.update(&version.to_le_bytes());
    *hasher.finalize().as_bytes()
}

pub(crate) fn ash_digest(version: u64) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_derive_key("gravebound.entry-ash-wallet.v3");
    hasher.update(&version.to_le_bytes());
    *hasher.finalize().as_bytes()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DangerEntrySnapshotDigestV3 {
    pub character_id: [u8; 16],
    pub content_revision: DangerEntryContentRevisionDigestV3,
    pub progression: DangerEntryProgressionDigestV3,
    pub inventory: DangerEntryInventoryDigestV3,
    pub oath_bargains: DangerEntryOathDigestV3,
    pub life_metrics: DangerEntryLifeDigestV3,
    pub ash_wallet: DangerEntryAshDigestV3,
    pub versions: DangerEntryVersionsDigestV3,
}

impl DangerEntrySnapshotDigestV3 {
    pub(crate) fn composite_digest(&self) -> Result<[u8; 32], PersistenceError> {
        let bytes = postcard::to_stdvec(self)
            .map_err(|_| PersistenceError::CorruptStoredDangerCrashRestore)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"gravebound.danger-entry-restore.v3\0");
        hasher.update(&bytes);
        Ok(*hasher.finalize().as_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[allow(
    clippy::struct_field_names,
    reason = "field names mirror the server V3 content-revision postcard contract"
)]
pub(crate) struct DangerEntryContentRevisionDigestV3 {
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct DangerEntryProgressionDigestV3 {
    pub level: u16,
    pub xp: u32,
    pub current_health: u32,
    pub progression_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DangerEntryInventoryDigestV3 {
    pub baseline_items: Vec<DangerEntryInventoryItemDigestV3>,
    pub pre_inventory_version: u64,
    pub inventory_version: u64,
    pub safe_placement_count: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DangerEntryInventoryItemDigestV3 {
    pub item_uid: [u8; 16],
    pub template_id: String,
    pub content_revision: String,
    pub creation_kind: u8,
    pub creation_request_id: [u8; 16],
    pub roll_index: u16,
    pub unit_ordinal: u16,
    pub provenance_kind: u8,
    pub location: DangerEntryInventoryLocationDigestV3,
    pub slot_index: u8,
    pub item_version: u64,
    pub security: DangerEntryInventorySecurityDigestV3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum DangerEntryInventoryLocationDigestV3 {
    Equipment,
    Belt,
    RunBackpack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum DangerEntryInventorySecurityDigestV3 {
    AtRiskEquipped,
    AtRiskPending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DangerEntryOathDigestV3 {
    pub oath_id: Option<String>,
    pub active_bargains: Vec<DangerEntryActiveBargainDigestV3>,
    pub earned_bargain_slots: u8,
    pub oath_bargain_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DangerEntryActiveBargainDigestV3 {
    pub acquisition_ordinal: u8,
    pub bargain_id: String,
    pub acquired_by_offer_id: [u8; 16],
    pub source_reward_event_id: [u8; 16],
    pub content_version: String,
    pub content_revision: DangerEntryContentRevisionDigestV3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct DangerEntryLifeDigestV3 {
    pub lifetime_ticks: u64,
    pub permadeath_combat_ticks: u64,
    pub life_metrics_version: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct DangerEntryAshDigestV3 {
    pub ash_wallet_version: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[allow(
    clippy::struct_field_names,
    reason = "field names mirror the server V3 aggregate-version postcard contract"
)]
pub(crate) struct DangerEntryVersionsDigestV3 {
    pub account_version: u64,
    pub character_version: u64,
    pub progression_version: u64,
    pub inventory_version: u64,
    pub oath_bargain_version: u64,
    pub life_metrics_version: u64,
    pub ash_wallet_version: u64,
}

fn decode_active_bargain(
    row: &sqlx::postgres::PgRow,
) -> Result<StoredDangerEntryActiveBargainV3, PersistenceError> {
    let acquisition_ordinal = u8::try_from(row.try_get::<i16, _>("acquisition_ordinal")?)
        .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?;
    let bargain_id: String = row.try_get("bargain_id")?;
    let selected_bargain_id: Option<String> = row.try_get("selected_bargain_id")?;
    let acquired_by_offer_id = fixed_bytes(row.try_get("acquired_by_offer_id")?)?;
    let source_reward_event_id = fixed_bytes(row.try_get("source_reward_event_id")?)?;
    let content_version: String = row.try_get("content_version")?;
    let records_blake3: String = row.try_get("records_blake3")?;
    let assets_blake3: String = row.try_get("assets_blake3")?;
    let localization_blake3: String = row.try_get("localization_blake3")?;
    if !(1..=3).contains(&acquisition_ordinal)
        || !(3..=96).contains(&bargain_id.len())
        || acquired_by_offer_id == [0; 16]
        || source_reward_event_id == [0; 16]
        || !(1..=96).contains(&content_version.len())
        || row.try_get::<i16, _>("offer_state")? != 1
        || selected_bargain_id.as_deref() != Some(bargain_id.as_str())
        || !is_lower_hex_hash(&records_blake3)
        || !is_lower_hex_hash(&assets_blake3)
        || !is_lower_hex_hash(&localization_blake3)
    {
        return Err(PersistenceError::CorruptStoredDangerEntryRestore);
    }
    Ok(StoredDangerEntryActiveBargainV3 {
        acquisition_ordinal,
        bargain_id,
        acquired_by_offer_id,
        source_reward_event_id,
        content_version,
        records_blake3,
        assets_blake3,
        localization_blake3,
    })
}

fn is_lower_hex_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

fn validate_context(
    account_id: [u8; 16],
    character_id: [u8; 16],
    restore_point_id: [u8; 16],
) -> Result<(), PersistenceError> {
    if [account_id, character_id, restore_point_id].contains(&[0; 16]) {
        Err(PersistenceError::CorruptStoredDangerEntryRestore)
    } else {
        Ok(())
    }
}

fn positive_u64(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(PersistenceError::CorruptStoredDangerEntryRestore)
}

fn nonnegative_u64(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value).map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)
}

fn fixed_bytes(bytes: Vec<u8>) -> Result<[u8; 16], PersistenceError> {
    bytes
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "the server V3 parity fixture is intentionally exact"
    )]
    fn persistence_v3_root_digest_matches_server_fixture() {
        let revision = DangerEntryContentRevisionDigestV3 {
            records_blake3: "a".repeat(64),
            assets_blake3: "b".repeat(64),
            localization_blake3: "c".repeat(64),
        };
        let item_revision = format!("core-dev.blake3.{}", "d".repeat(64));
        let snapshot = DangerEntrySnapshotDigestV3 {
            character_id: [1; 16],
            content_revision: revision.clone(),
            progression: DangerEntryProgressionDigestV3 {
                level: 10,
                xp: 4_200,
                current_health: 120,
                progression_version: 5,
            },
            inventory: DangerEntryInventoryDigestV3 {
                baseline_items: vec![
                    DangerEntryInventoryItemDigestV3 {
                        item_uid: [2; 16],
                        template_id: "weapon.iron_arbalest".into(),
                        content_revision: item_revision.clone(),
                        creation_kind: 0,
                        creation_request_id: [10; 16],
                        roll_index: 0,
                        unit_ordinal: 0,
                        provenance_kind: 0,
                        location: DangerEntryInventoryLocationDigestV3::Equipment,
                        slot_index: 0,
                        item_version: 2,
                        security: DangerEntryInventorySecurityDigestV3::AtRiskEquipped,
                    },
                    DangerEntryInventoryItemDigestV3 {
                        item_uid: [3; 16],
                        template_id: "consumable.red_tonic".into(),
                        content_revision: item_revision.clone(),
                        creation_kind: 1,
                        creation_request_id: [11; 16],
                        roll_index: 2,
                        unit_ordinal: 1,
                        provenance_kind: 1,
                        location: DangerEntryInventoryLocationDigestV3::Belt,
                        slot_index: 0,
                        item_version: 4,
                        security: DangerEntryInventorySecurityDigestV3::AtRiskEquipped,
                    },
                    DangerEntryInventoryItemDigestV3 {
                        item_uid: [4; 16],
                        template_id: "relic.ember_glass".into(),
                        content_revision: item_revision,
                        creation_kind: 2,
                        creation_request_id: [12; 16],
                        roll_index: 3,
                        unit_ordinal: 0,
                        provenance_kind: 2,
                        location: DangerEntryInventoryLocationDigestV3::RunBackpack,
                        slot_index: 5,
                        item_version: 7,
                        security: DangerEntryInventorySecurityDigestV3::AtRiskPending,
                    },
                ],
                pre_inventory_version: 6,
                inventory_version: 7,
                safe_placement_count: 1,
            },
            oath_bargains: DangerEntryOathDigestV3 {
                oath_id: Some("oath.arbalist.long_vigil".into()),
                active_bargains: vec![DangerEntryActiveBargainDigestV3 {
                    acquisition_ordinal: 1,
                    bargain_id: "bargain.cinder_hunger".into(),
                    acquired_by_offer_id: [20; 16],
                    source_reward_event_id: [21; 16],
                    content_version: "core-dev".into(),
                    content_revision: revision,
                }],
                earned_bargain_slots: 1,
                oath_bargain_version: 9,
            },
            life_metrics: DangerEntryLifeDigestV3 {
                lifetime_ticks: 36_000,
                permadeath_combat_ticks: 900,
                life_metrics_version: 3,
            },
            ash_wallet: DangerEntryAshDigestV3 {
                ash_wallet_version: 8,
            },
            versions: DangerEntryVersionsDigestV3 {
                account_version: 2,
                character_version: 11,
                progression_version: 5,
                inventory_version: 7,
                oath_bargain_version: 9,
                life_metrics_version: 3,
                ash_wallet_version: 8,
            },
        };
        assert_eq!(
            snapshot.composite_digest().unwrap(),
            [
                62, 92, 98, 12, 238, 99, 91, 222, 86, 244, 12, 154, 10, 49, 205, 162, 82, 102, 107,
                128, 16, 210, 21, 158, 193, 103, 238, 190, 128, 111, 24, 66,
            ]
        );
    }

    #[test]
    fn risk_ledger_identity_is_domain_separated_and_deterministic() {
        let first = risk_transition_event_id([1; 16], [2; 16]);
        assert_eq!(first, risk_transition_event_id([1; 16], [2; 16]));
        assert_ne!(first, risk_transition_event_id([1; 16], [3; 16]));
        assert_ne!(first, [0; 16]);
    }

    #[test]
    fn component_digests_cover_versions_order_and_clock_values() {
        let item = StoredDangerEntryInventoryItemV3 {
            item_uid: [4; 16],
            template_id: "weapon.rustbound_repeater".to_owned(),
            content_revision: CORE_ITEM_CONTENT_REVISION.to_owned(),
            item_kind: 0,
            creation_kind: 0,
            creation_request_id: [5; 16],
            roll_index: 0,
            unit_ordinal: 0,
            provenance_kind: 0,
            location_kind: 0,
            slot_index: 0,
            entry_item_version: 2,
            entry_security_state: 1,
        };
        assert_ne!(
            inventory_digest(1, 2, 0, std::slice::from_ref(&item)),
            inventory_digest(1, 2, 1, std::slice::from_ref(&item))
        );
        assert_ne!(
            oath_digest(None, 0, 1, &[]),
            oath_digest(Some("oath.black_bell"), 0, 1, &[])
        );
        assert_ne!(life_digest(10, 4, 1), life_digest(10, 5, 1));
        assert_ne!(ash_digest(1), ash_digest(2));
    }

    #[test]
    fn v3_inventory_digest_covers_provenance_and_backpack_baseline() {
        let baseline = StoredDangerEntryInventoryItemV3 {
            item_uid: [4; 16],
            template_id: "consumable.red_tonic".to_owned(),
            content_revision: CORE_ITEM_CONTENT_REVISION.to_owned(),
            item_kind: 1,
            creation_kind: 1,
            creation_request_id: [5; 16],
            roll_index: 3,
            unit_ordinal: 1,
            provenance_kind: 1,
            location_kind: 2,
            slot_index: 7,
            entry_item_version: 9,
            entry_security_state: 2,
        };
        let mut altered = baseline.clone();
        altered.creation_request_id = [6; 16];
        assert_ne!(
            inventory_digest(3, 3, 0, std::slice::from_ref(&baseline)),
            inventory_digest(3, 3, 0, std::slice::from_ref(&altered))
        );
        altered = baseline.clone();
        altered.location_kind = 1;
        assert_ne!(
            inventory_digest(3, 3, 0, std::slice::from_ref(&baseline)),
            inventory_digest(3, 3, 0, std::slice::from_ref(&altered))
        );
    }

    #[test]
    fn v3_oath_digest_covers_offer_source_and_revision_authority() {
        let bargain = StoredDangerEntryActiveBargainV3 {
            acquisition_ordinal: 1,
            bargain_id: "bargain.bell_debt".to_owned(),
            acquired_by_offer_id: [1; 16],
            source_reward_event_id: [2; 16],
            content_version: "core.1.0.0".to_owned(),
            records_blake3: "a".repeat(64),
            assets_blake3: "b".repeat(64),
            localization_blake3: "c".repeat(64),
        };
        let mut altered = bargain.clone();
        altered.source_reward_event_id = [3; 16];
        assert_ne!(
            oath_digest(
                Some("oath.black_bell"),
                1,
                8,
                std::slice::from_ref(&bargain)
            ),
            oath_digest(
                Some("oath.black_bell"),
                1,
                8,
                std::slice::from_ref(&altered)
            )
        );
        altered = bargain.clone();
        altered.records_blake3 = "d".repeat(64);
        assert_ne!(
            oath_digest(
                Some("oath.black_bell"),
                1,
                8,
                std::slice::from_ref(&bargain)
            ),
            oath_digest(
                Some("oath.black_bell"),
                1,
                8,
                std::slice::from_ref(&altered)
            )
        );
    }

    #[test]
    fn content_hash_validation_rejects_wrong_length_or_case() {
        assert!(is_lower_hex_hash(&"a".repeat(64)));
        assert!(!is_lower_hex_hash(&"A".repeat(64)));
        assert!(!is_lower_hex_hash(&"a".repeat(63)));
    }
}
