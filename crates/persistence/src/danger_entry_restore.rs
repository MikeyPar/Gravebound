//! Transaction-bound durable components for the `TECH-023` danger-entry restore point.

use std::collections::BTreeSet;

use sqlx::Row;

use crate::{
    PersistenceError, PersistenceTransaction, WIPEABLE_CORE_NAMESPACE,
    items::CORE_ITEM_CONTENT_REVISION,
};

const MAX_RISK_ITEMS: usize = 16;
const MAX_ACTIVE_BARGAINS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDangerEntryInventoryItemV2 {
    pub item_uid: [u8; 16],
    pub template_id: String,
    pub location_kind: i16,
    pub slot_index: i16,
    pub pre_item_version: u64,
    pub post_item_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDangerEntryInventoryV2 {
    pub pre_inventory_version: u64,
    pub post_inventory_version: u64,
    pub safe_placement_count: u16,
    pub items: Vec<StoredDangerEntryInventoryItemV2>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDangerEntryOathBargainV2 {
    pub oath_id: Option<String>,
    pub active_bargain_ids: Vec<String>,
    pub earned_bargain_slots: u8,
    pub oath_bargain_version: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredDangerEntryLifeMetricsV2 {
    pub captured_lifetime_ticks: u64,
    pub rollback_permadeath_combat_ticks: u64,
    pub life_metrics_version: u64,
}

/// Converts every equipped/Belt item from Safe to `AtRiskEquipped` and persists the exact restore
/// component. When `CharacterSafe` preflight already moved items, that preflight's single inventory
/// version advance owns the combined Realm Gate mutation.
#[allow(
    clippy::too_many_lines,
    reason = "the lock, validate, transition, version, ledger, and component writes remain contiguous for atomicity audit"
)]
pub async fn stage_danger_entry_inventory_restore_v2(
    transaction: &mut PersistenceTransaction<'_>,
    account_id: [u8; 16],
    character_id: [u8; 16],
    restore_point_id: [u8; 16],
    mutation_id: [u8; 16],
    safe_placement_count: u16,
) -> Result<StoredDangerEntryInventoryV2, PersistenceError> {
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
        "SELECT item_uid, template_id, content_revision, item_kind, item_version, \
                security_state, location_kind, slot_index FROM item_instances \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3 \
           AND location_kind IN (0, 1) \
         ORDER BY location_kind, slot_index, item_uid FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_all(transaction.connection())
    .await
    .map_err(PersistenceError::Database)?;
    if rows.len() > MAX_RISK_ITEMS {
        return Err(PersistenceError::CorruptStoredDangerEntryRestore);
    }

    let mut items = Vec::with_capacity(rows.len());
    let mut identities = BTreeSet::new();
    let mut equipment_slots = BTreeSet::new();
    let mut belt_slot_templates = [None::<String>, None::<String>];
    let mut belt_slot_counts = [0_usize; 2];
    for row in rows {
        let item_uid = fixed_bytes(row.try_get("item_uid")?)?;
        let template_id: String = row.try_get("template_id")?;
        let content_revision: String = row.try_get("content_revision")?;
        let item_kind: i16 = row.try_get("item_kind")?;
        let pre_item_version = positive_u64(row.try_get("item_version")?)?;
        let security_state: i16 = row.try_get("security_state")?;
        let location_kind: i16 = row.try_get("location_kind")?;
        let slot_index: i16 = row.try_get("slot_index")?;
        if item_uid == [0; 16]
            || !identities.insert(item_uid)
            || template_id.len() < 3
            || content_revision != CORE_ITEM_CONTENT_REVISION
            || security_state != 0
        {
            return Err(PersistenceError::CorruptStoredDangerEntryRestore);
        }
        match location_kind {
            0 if item_kind == 0 && (0..=3).contains(&slot_index) => {
                if !equipment_slots.insert(slot_index) {
                    return Err(PersistenceError::CorruptStoredDangerEntryRestore);
                }
            }
            1 if item_kind == 1 && (0..=1).contains(&slot_index) => {
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
            _ => return Err(PersistenceError::CorruptStoredDangerEntryRestore),
        }
        items.push(StoredDangerEntryInventoryItemV2 {
            item_uid,
            template_id,
            location_kind,
            slot_index,
            pre_item_version,
            post_item_version: pre_item_version
                .checked_add(1)
                .ok_or(PersistenceError::CorruptStoredDangerEntryRestore)?,
        });
    }

    let (pre_inventory_version, post_inventory_version) = if safe_placement_count > 0 {
        (
            current_inventory_version
                .checked_sub(1)
                .ok_or(PersistenceError::CorruptStoredDangerEntryRestore)?,
            current_inventory_version,
        )
    } else if items.is_empty() {
        (current_inventory_version, current_inventory_version)
    } else {
        (
            current_inventory_version,
            current_inventory_version
                .checked_add(1)
                .ok_or(PersistenceError::CorruptStoredDangerEntryRestore)?,
        )
    };

    for item in &items {
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
        "INSERT INTO entry_restore_inventory_v1 \
         (namespace_id, account_id, character_id, restore_point_id, pre_inventory_version, \
          post_inventory_version, risk_item_count, safe_placement_count, component_digest) \
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
            "INSERT INTO entry_restore_inventory_items_v1 \
             (namespace_id, restore_point_id, item_ordinal, item_uid, location_kind, slot_index, \
              pre_item_version, post_item_version, pre_security_state, post_security_state) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,0,1)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(restore_point_id.as_slice())
        .bind(
            i16::try_from(ordinal)
                .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
        )
        .bind(item.item_uid.as_slice())
        .bind(item.location_kind)
        .bind(item.slot_index)
        .bind(
            i64::try_from(item.pre_item_version)
                .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
        )
        .bind(
            i64::try_from(item.post_item_version)
                .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
        )
        .execute(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?;
    }
    Ok(StoredDangerEntryInventoryV2 {
        pre_inventory_version,
        post_inventory_version,
        safe_placement_count,
        items,
    })
}

pub async fn stage_danger_entry_oath_bargain_restore_v2(
    transaction: &mut PersistenceTransaction<'_>,
    account_id: [u8; 16],
    character_id: [u8; 16],
    restore_point_id: [u8; 16],
) -> Result<StoredDangerEntryOathBargainV2, PersistenceError> {
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
    let active_bargain_ids = sqlx::query_scalar::<_, String>(
        "SELECT bargain_id FROM character_active_bargains WHERE namespace_id = $1 \
         AND account_id = $2 AND character_id = $3 ORDER BY acquisition_ordinal FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_all(transaction.connection())
    .await
    .map_err(PersistenceError::Database)?;
    let earned_bargain_slots = u8::try_from(earned_bargain_slots)
        .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?;
    if usize::from(earned_bargain_slots) > MAX_ACTIVE_BARGAINS
        || active_bargain_ids.len() > usize::from(earned_bargain_slots)
        || oath_id
            .as_ref()
            .is_some_and(|value| !(3..=96).contains(&value.len()))
        || active_bargain_ids
            .iter()
            .any(|value| !(3..=96).contains(&value.len()))
        || active_bargain_ids.iter().collect::<BTreeSet<_>>().len() != active_bargain_ids.len()
    {
        return Err(PersistenceError::CorruptStoredDangerEntryRestore);
    }
    let component_digest = oath_digest(
        oath_id.as_deref(),
        earned_bargain_slots,
        oath_bargain_version,
        &active_bargain_ids,
    );
    sqlx::query(
        "INSERT INTO entry_restore_oath_bargain_v2 \
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
        i16::try_from(active_bargain_ids.len())
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
    for (index, bargain_id) in active_bargain_ids.iter().enumerate() {
        sqlx::query(
            "INSERT INTO entry_restore_active_bargains_v2 \
             (namespace_id, restore_point_id, acquisition_ordinal, bargain_id) \
             VALUES ($1,$2,$3,$4)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(restore_point_id.as_slice())
        .bind(
            i16::try_from(index + 1)
                .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
        )
        .bind(bargain_id)
        .execute(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?;
    }
    Ok(StoredDangerEntryOathBargainV2 {
        oath_id,
        active_bargain_ids,
        earned_bargain_slots,
        oath_bargain_version,
    })
}

pub async fn stage_danger_entry_life_metrics_restore_v2(
    transaction: &mut PersistenceTransaction<'_>,
    account_id: [u8; 16],
    character_id: [u8; 16],
    restore_point_id: [u8; 16],
) -> Result<StoredDangerEntryLifeMetricsV2, PersistenceError> {
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
        "INSERT INTO entry_restore_life_metrics_v2 \
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
    Ok(StoredDangerEntryLifeMetricsV2 {
        captured_lifetime_ticks,
        rollback_permadeath_combat_ticks,
        life_metrics_version,
    })
}

async fn transition_risk_item(
    transaction: &mut PersistenceTransaction<'_>,
    account_id: [u8; 16],
    character_id: [u8; 16],
    mutation_id: [u8; 16],
    item: &StoredDangerEntryInventoryItemV2,
) -> Result<(), PersistenceError> {
    let changed = sqlx::query(
        "UPDATE item_instances SET item_version = $1, security_state = 1, \
                updated_at = transaction_timestamp() \
         WHERE namespace_id = $2 AND account_id = $3 AND character_id = $4 AND item_uid = $5 \
           AND item_version = $6 AND security_state = 0 AND location_kind = $7 AND slot_index = $8",
    )
    .bind(
        i64::try_from(item.post_item_version)
            .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(item.item_uid.as_slice())
    .bind(
        i64::try_from(item.pre_item_version)
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
        i64::try_from(item.pre_item_version)
            .map_err(|_| PersistenceError::CorruptStoredDangerEntryRestore)?,
    )
    .bind(
        i64::try_from(item.post_item_version)
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
    let hash = blake3::derive_key("gravebound.danger-entry-risk-ledger.v2", &material);
    let mut value = [0; 16];
    value.copy_from_slice(&hash[..16]);
    value
}

fn inventory_digest(
    pre_version: u64,
    post_version: u64,
    safe_placement_count: u16,
    items: &[StoredDangerEntryInventoryItemV2],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_derive_key("gravebound.entry-inventory.v2");
    hasher.update(&pre_version.to_le_bytes());
    hasher.update(&post_version.to_le_bytes());
    hasher.update(&safe_placement_count.to_le_bytes());
    for item in items {
        hasher.update(&item.item_uid);
        hasher.update(item.template_id.as_bytes());
        hasher.update(&item.location_kind.to_le_bytes());
        hasher.update(&item.slot_index.to_le_bytes());
        hasher.update(&item.pre_item_version.to_le_bytes());
        hasher.update(&item.post_item_version.to_le_bytes());
    }
    *hasher.finalize().as_bytes()
}

fn oath_digest(
    oath_id: Option<&str>,
    earned_slots: u8,
    version: u64,
    active: &[String],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_derive_key("gravebound.entry-oath-bargain.v2");
    if let Some(oath_id) = oath_id {
        hasher.update(&[1]);
        hasher.update(oath_id.as_bytes());
    } else {
        hasher.update(&[0]);
    }
    hasher.update(&[earned_slots]);
    hasher.update(&version.to_le_bytes());
    for bargain in active {
        hasher.update(bargain.as_bytes());
    }
    *hasher.finalize().as_bytes()
}

fn life_digest(lifetime_ticks: u64, combat_ticks: u64, version: u64) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_derive_key("gravebound.entry-life-metrics.v2");
    hasher.update(&lifetime_ticks.to_le_bytes());
    hasher.update(&combat_ticks.to_le_bytes());
    hasher.update(&version.to_le_bytes());
    *hasher.finalize().as_bytes()
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
    fn risk_ledger_identity_is_domain_separated_and_deterministic() {
        let first = risk_transition_event_id([1; 16], [2; 16]);
        assert_eq!(first, risk_transition_event_id([1; 16], [2; 16]));
        assert_ne!(first, risk_transition_event_id([1; 16], [3; 16]));
        assert_ne!(first, [0; 16]);
    }

    #[test]
    fn component_digests_cover_versions_order_and_clock_values() {
        let item = StoredDangerEntryInventoryItemV2 {
            item_uid: [4; 16],
            template_id: "weapon.rustbound_repeater".to_owned(),
            location_kind: 0,
            slot_index: 0,
            pre_item_version: 1,
            post_item_version: 2,
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
    }
}
