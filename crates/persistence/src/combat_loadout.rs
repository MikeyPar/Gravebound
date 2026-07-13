//! Atomic read model for authoritative character combat construction.

use sqlx::Row;
use std::collections::BTreeSet;

use crate::{PersistenceError, PostgresPersistence, WIPEABLE_CORE_NAMESPACE};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEquippedWeapon {
    pub item_uid: [u8; 16],
    pub template_id: String,
    pub content_revision: String,
    pub item_level: i16,
    pub rarity: i16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCombatBargain {
    pub bargain_id: String,
    pub acquisition_ordinal: i16,
    pub acquired_by_offer_id: [u8; 16],
    pub acquiring_offer_content_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCombatBeltStack {
    pub template_id: String,
    pub content_revision: String,
    pub quantity: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCoreCombatLoadout {
    pub character_id: [u8; 16],
    pub selected_character_id: Option<[u8; 16]>,
    pub class_id: String,
    pub level: i16,
    pub current_health: i32,
    pub oath_id: Option<String>,
    pub oath_bargain_version: i64,
    pub active_bargains: Vec<StoredCombatBargain>,
    pub life_state: i16,
    pub security_state: i16,
    pub character_state_version: i64,
    pub inventory_version: Option<i64>,
    pub equipped_weapon: Option<StoredEquippedWeapon>,
    pub belt_slots: [Option<StoredCombatBeltStack>; 2],
}

impl PostgresPersistence {
    /// Reads identity, progression, Oath, inventory, and equipped weapon in one `PostgreSQL`
    /// statement snapshot. Callers never assemble combat authority from independently timed reads.
    pub async fn core_combat_loadout_snapshot(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> Result<Option<StoredCoreCombatLoadout>, PersistenceError> {
        if account_id == [0; 16] || character_id == [0; 16] {
            return Err(PersistenceError::CorruptStoredItems);
        }
        let row = sqlx::query(
            "SELECT a.selected_character_id, c.class_id, p.level, p.current_health, c.oath_id, \
                    ob.oath_bargain_version, \
                    ARRAY(SELECT ab.bargain_id FROM character_active_bargains ab \
                          WHERE ab.namespace_id = c.namespace_id AND ab.account_id = c.account_id \
                          AND ab.character_id = c.character_id ORDER BY ab.acquisition_ordinal) \
                        AS active_bargain_ids, \
                    ARRAY(SELECT ab.acquisition_ordinal FROM character_active_bargains ab \
                          WHERE ab.namespace_id = c.namespace_id AND ab.account_id = c.account_id \
                          AND ab.character_id = c.character_id ORDER BY ab.acquisition_ordinal) \
                        AS active_bargain_ordinals, \
                    ARRAY(SELECT ab.acquired_by_offer_id FROM character_active_bargains ab \
                          WHERE ab.namespace_id = c.namespace_id AND ab.account_id = c.account_id \
                          AND ab.character_id = c.character_id ORDER BY ab.acquisition_ordinal) \
                        AS active_bargain_offer_ids, \
                    ARRAY(SELECT bo.content_version FROM character_active_bargains ab \
                          JOIN bargain_offers bo ON bo.namespace_id = ab.namespace_id \
                          AND bo.account_id = ab.account_id AND bo.character_id = ab.character_id \
                          AND bo.offer_id = ab.acquired_by_offer_id \
                          WHERE ab.namespace_id = c.namespace_id AND ab.account_id = c.account_id \
                          AND ab.character_id = c.character_id ORDER BY ab.acquisition_ordinal) \
                        AS active_bargain_content_versions, \
                    c.life_state, \
                    c.security_state, c.character_state_version, i.inventory_version, \
                    w.item_uid AS weapon_uid, w.template_id AS weapon_template_id, \
                    w.content_revision AS weapon_content_revision, \
                    w.item_level AS weapon_item_level, w.rarity AS weapon_rarity, \
                    ARRAY(SELECT belt.slot_index FROM item_instances belt \
                          WHERE belt.namespace_id = c.namespace_id \
                          AND belt.account_id = c.account_id AND belt.character_id = c.character_id \
                          AND belt.item_kind = 1 AND belt.security_state = 0 \
                          AND belt.location_kind = 1 GROUP BY belt.slot_index, belt.template_id, \
                          belt.content_revision ORDER BY belt.slot_index, belt.template_id, \
                          belt.content_revision) AS belt_slot_indices, \
                    ARRAY(SELECT belt.template_id FROM item_instances belt \
                          WHERE belt.namespace_id = c.namespace_id \
                          AND belt.account_id = c.account_id AND belt.character_id = c.character_id \
                          AND belt.item_kind = 1 AND belt.security_state = 0 \
                          AND belt.location_kind = 1 GROUP BY belt.slot_index, belt.template_id, \
                          belt.content_revision ORDER BY belt.slot_index, belt.template_id, \
                          belt.content_revision) AS belt_template_ids, \
                    ARRAY(SELECT belt.content_revision FROM item_instances belt \
                          WHERE belt.namespace_id = c.namespace_id \
                          AND belt.account_id = c.account_id AND belt.character_id = c.character_id \
                          AND belt.item_kind = 1 AND belt.security_state = 0 \
                          AND belt.location_kind = 1 GROUP BY belt.slot_index, belt.template_id, \
                          belt.content_revision ORDER BY belt.slot_index, belt.template_id, \
                          belt.content_revision) AS belt_content_revisions, \
                    ARRAY(SELECT COUNT(*) FROM item_instances belt \
                          WHERE belt.namespace_id = c.namespace_id \
                          AND belt.account_id = c.account_id AND belt.character_id = c.character_id \
                          AND belt.item_kind = 1 AND belt.security_state = 0 \
                          AND belt.location_kind = 1 GROUP BY belt.slot_index, belt.template_id, \
                          belt.content_revision ORDER BY belt.slot_index, belt.template_id, \
                          belt.content_revision) AS belt_quantities \
             FROM accounts a \
             JOIN characters c USING (namespace_id, account_id) \
             JOIN character_progression p USING (namespace_id, account_id, character_id) \
             JOIN character_oath_bargain_state ob \
                  USING (namespace_id, account_id, character_id) \
             LEFT JOIN character_inventories i USING (namespace_id, account_id, character_id) \
             LEFT JOIN item_instances w ON w.namespace_id = c.namespace_id \
                  AND w.account_id = c.account_id AND w.character_id = c.character_id \
                  AND w.item_kind = 0 AND w.security_state = 0 \
                  AND w.location_kind = 0 AND w.slot_index = 0 \
             WHERE c.namespace_id = $1 AND c.account_id = $2 AND c.character_id = $3",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .fetch_optional(&self.pool)
        .await
        .map_err(PersistenceError::Database)?;
        row.map(|row| decode_loadout(&row, character_id))
            .transpose()
    }
}

fn decode_loadout(
    row: &sqlx::postgres::PgRow,
    character_id: [u8; 16],
) -> Result<StoredCoreCombatLoadout, PersistenceError> {
    let selected_character_id = row
        .try_get::<Option<Vec<u8>>, _>("selected_character_id")?
        .map(fixed_id)
        .transpose()?;
    let equipped_weapon = decode_weapon(row)?;
    let active_bargains = decode_active_bargains(row)?;
    let belt_slots = decode_belt_slots(row)?;
    let loadout = StoredCoreCombatLoadout {
        character_id,
        selected_character_id,
        class_id: row.try_get("class_id")?,
        level: row.try_get("level")?,
        current_health: row.try_get("current_health")?,
        oath_id: row.try_get("oath_id")?,
        oath_bargain_version: row.try_get("oath_bargain_version")?,
        active_bargains,
        life_state: row.try_get("life_state")?,
        security_state: row.try_get("security_state")?,
        character_state_version: row.try_get("character_state_version")?,
        inventory_version: row.try_get("inventory_version")?,
        equipped_weapon,
        belt_slots,
    };
    validate_loadout_shape(&loadout)?;
    Ok(loadout)
}

fn decode_weapon(
    row: &sqlx::postgres::PgRow,
) -> Result<Option<StoredEquippedWeapon>, PersistenceError> {
    let weapon_uid = row
        .try_get::<Option<Vec<u8>>, _>("weapon_uid")?
        .map(fixed_id)
        .transpose()?;
    let weapon_template_id = row.try_get::<Option<String>, _>("weapon_template_id")?;
    let weapon_content_revision = row.try_get::<Option<String>, _>("weapon_content_revision")?;
    let weapon_item_level = row.try_get::<Option<i16>, _>("weapon_item_level")?;
    let weapon_rarity = row.try_get::<Option<i16>, _>("weapon_rarity")?;
    let weapon_shape = [
        weapon_uid.is_some(),
        weapon_template_id.is_some(),
        weapon_content_revision.is_some(),
        weapon_item_level.is_some(),
        weapon_rarity.is_some(),
    ];
    if weapon_shape.iter().any(|value| *value) && !weapon_shape.iter().all(|value| *value) {
        return Err(PersistenceError::CorruptStoredItems);
    }
    Ok(weapon_uid.map(|item_uid| StoredEquippedWeapon {
        item_uid,
        template_id: weapon_template_id.expect("validated weapon shape"),
        content_revision: weapon_content_revision.expect("validated weapon shape"),
        item_level: weapon_item_level.expect("validated weapon shape"),
        rarity: weapon_rarity.expect("validated weapon shape"),
    }))
}

fn decode_active_bargains(
    row: &sqlx::postgres::PgRow,
) -> Result<Vec<StoredCombatBargain>, PersistenceError> {
    let bargain_ids = row.try_get::<Vec<String>, _>("active_bargain_ids")?;
    let bargain_ordinals = row.try_get::<Vec<i16>, _>("active_bargain_ordinals")?;
    let bargain_offer_ids = row.try_get::<Vec<Vec<u8>>, _>("active_bargain_offer_ids")?;
    let bargain_content_versions =
        row.try_get::<Vec<String>, _>("active_bargain_content_versions")?;
    let bargain_lengths = [
        bargain_ids.len(),
        bargain_ordinals.len(),
        bargain_offer_ids.len(),
        bargain_content_versions.len(),
    ];
    if bargain_lengths
        .iter()
        .any(|length| *length != bargain_lengths[0])
    {
        return Err(PersistenceError::CorruptStoredBargain);
    }
    bargain_ids
        .into_iter()
        .zip(bargain_ordinals)
        .zip(bargain_offer_ids)
        .zip(bargain_content_versions)
        .map(
            |(((bargain_id, acquisition_ordinal), acquired_by_offer_id), content_version)| {
                Ok(StoredCombatBargain {
                    bargain_id,
                    acquisition_ordinal,
                    acquired_by_offer_id: fixed_id(acquired_by_offer_id)?,
                    acquiring_offer_content_version: content_version,
                })
            },
        )
        .collect()
}

fn decode_belt_slots(
    row: &sqlx::postgres::PgRow,
) -> Result<[Option<StoredCombatBeltStack>; 2], PersistenceError> {
    let belt_indices = row.try_get::<Vec<i16>, _>("belt_slot_indices")?;
    let belt_template_ids = row.try_get::<Vec<String>, _>("belt_template_ids")?;
    let belt_content_revisions = row.try_get::<Vec<String>, _>("belt_content_revisions")?;
    let belt_quantities = row.try_get::<Vec<i64>, _>("belt_quantities")?;
    let belt_lengths = [
        belt_indices.len(),
        belt_template_ids.len(),
        belt_content_revisions.len(),
        belt_quantities.len(),
    ];
    if belt_lengths.iter().any(|length| *length != belt_lengths[0]) {
        return Err(PersistenceError::CorruptStoredItems);
    }
    let mut belt_slots = [None, None];
    for (((slot_index, template_id), content_revision), quantity) in belt_indices
        .into_iter()
        .zip(belt_template_ids)
        .zip(belt_content_revisions)
        .zip(belt_quantities)
    {
        let index =
            usize::try_from(slot_index).map_err(|_| PersistenceError::CorruptStoredItems)?;
        let destination = belt_slots
            .get_mut(index)
            .ok_or(PersistenceError::CorruptStoredItems)?;
        if destination.is_some() {
            return Err(PersistenceError::CorruptStoredItems);
        }
        *destination = Some(StoredCombatBeltStack {
            template_id,
            content_revision,
            quantity,
        });
    }
    Ok(belt_slots)
}

fn fixed_id(bytes: Vec<u8>) -> Result<[u8; 16], PersistenceError> {
    let value = <[u8; 16]>::try_from(bytes).map_err(|_| PersistenceError::CorruptStoredItems)?;
    if value == [0; 16] {
        return Err(PersistenceError::CorruptStoredItems);
    }
    Ok(value)
}

fn validate_loadout_shape(loadout: &StoredCoreCombatLoadout) -> Result<(), PersistenceError> {
    if loadout.character_id == [0; 16]
        || loadout.class_id.is_empty()
        || !(1..=10).contains(&loadout.level)
        || loadout.current_health <= 0
        || loadout.character_state_version <= 0
        || loadout.oath_bargain_version <= 0
        || loadout.inventory_version.is_some_and(|value| value <= 0)
        || loadout.equipped_weapon.as_ref().is_some_and(|weapon| {
            weapon.template_id.is_empty()
                || weapon.content_revision.is_empty()
                || !(1..=10).contains(&weapon.item_level)
                || !(0..=4).contains(&weapon.rarity)
        })
    {
        return Err(PersistenceError::CorruptStoredItems);
    }
    if loadout.active_bargains.len() > 3 {
        return Err(PersistenceError::CorruptStoredBargain);
    }
    let mut bargain_ids = BTreeSet::new();
    for (index, bargain) in loadout.active_bargains.iter().enumerate() {
        if bargain.bargain_id.is_empty()
            || bargain.acquisition_ordinal != i16::try_from(index + 1).unwrap_or(i16::MAX)
            || bargain.acquired_by_offer_id == [0; 16]
            || bargain.acquiring_offer_content_version.is_empty()
            || !bargain_ids.insert(&bargain.bargain_id)
        {
            return Err(PersistenceError::CorruptStoredBargain);
        }
    }
    if loadout.belt_slots.iter().flatten().any(|stack| {
        stack.template_id.is_empty()
            || stack.content_revision.is_empty()
            || !(1..=6).contains(&stack.quantity)
    }) {
        return Err(PersistenceError::CorruptStoredItems);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loadout() -> StoredCoreCombatLoadout {
        StoredCoreCombatLoadout {
            character_id: [1; 16],
            selected_character_id: Some([1; 16]),
            class_id: "class.grave_arbalist".into(),
            level: 10,
            current_health: 120,
            oath_id: Some("oath.arbalist.long_vigil".into()),
            oath_bargain_version: 2,
            active_bargains: vec![StoredCombatBargain {
                bargain_id: "bargain.cinder_hunger".into(),
                acquisition_ordinal: 1,
                acquired_by_offer_id: [3; 16],
                acquiring_offer_content_version: "core-dev.blake3.bargains".into(),
            }],
            life_state: 0,
            security_state: 0,
            character_state_version: 4,
            inventory_version: Some(2),
            equipped_weapon: Some(StoredEquippedWeapon {
                item_uid: [2; 16],
                template_id: "item.weapon.crossbow.pine_crossbow".into(),
                content_revision: "core-dev.blake3.test".into(),
                item_level: 10,
                rarity: 0,
            }),
            belt_slots: [
                Some(StoredCombatBeltStack {
                    template_id: "consumable.red_tonic".into(),
                    content_revision: "core-dev.blake3.items".into(),
                    quantity: 2,
                }),
                None,
            ],
        }
    }

    #[test]
    fn combat_loadout_shape_is_bounded_but_readiness_remains_server_owned() {
        let mut value = loadout();
        assert!(validate_loadout_shape(&value).is_ok());
        value.selected_character_id = None;
        value.oath_id = None;
        value.inventory_version = None;
        value.equipped_weapon = None;
        value.active_bargains.clear();
        value.belt_slots = [None, None];
        assert!(validate_loadout_shape(&value).is_ok());
    }

    #[test]
    fn malformed_weapon_or_versions_fail_closed() {
        let mut value = loadout();
        value.equipped_weapon.as_mut().unwrap().rarity = 5;
        assert!(matches!(
            validate_loadout_shape(&value),
            Err(PersistenceError::CorruptStoredItems)
        ));
        value = loadout();
        value.inventory_version = Some(0);
        assert!(matches!(
            validate_loadout_shape(&value),
            Err(PersistenceError::CorruptStoredItems)
        ));
    }

    #[test]
    fn bargain_order_offer_revision_and_belt_shape_fail_closed() {
        let mut value = loadout();
        value.active_bargains[0].acquisition_ordinal = 2;
        assert!(matches!(
            validate_loadout_shape(&value),
            Err(PersistenceError::CorruptStoredBargain)
        ));
        value = loadout();
        value.active_bargains[0]
            .acquiring_offer_content_version
            .clear();
        assert!(matches!(
            validate_loadout_shape(&value),
            Err(PersistenceError::CorruptStoredBargain)
        ));
        value = loadout();
        value.belt_slots[0].as_mut().unwrap().quantity = 7;
        assert!(matches!(
            validate_loadout_shape(&value),
            Err(PersistenceError::CorruptStoredItems)
        ));
    }
}
