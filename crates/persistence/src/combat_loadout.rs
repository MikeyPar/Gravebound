//! Atomic read model for authoritative character combat construction.

use sqlx::Row;

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
pub struct StoredCoreCombatLoadout {
    pub character_id: [u8; 16],
    pub selected_character_id: Option<[u8; 16]>,
    pub class_id: String,
    pub level: i16,
    pub oath_id: Option<String>,
    pub life_state: i16,
    pub security_state: i16,
    pub character_state_version: i64,
    pub inventory_version: Option<i64>,
    pub equipped_weapon: Option<StoredEquippedWeapon>,
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
            "SELECT a.selected_character_id, c.class_id, p.level, c.oath_id, c.life_state, \
                    c.security_state, c.character_state_version, i.inventory_version, \
                    w.item_uid AS weapon_uid, w.template_id AS weapon_template_id, \
                    w.content_revision AS weapon_content_revision, \
                    w.item_level AS weapon_item_level, w.rarity AS weapon_rarity \
             FROM accounts a \
             JOIN characters c USING (namespace_id, account_id) \
             JOIN character_progression p USING (namespace_id, account_id, character_id) \
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
    let equipped_weapon = weapon_uid.map(|item_uid| StoredEquippedWeapon {
        item_uid,
        template_id: weapon_template_id.expect("validated weapon shape"),
        content_revision: weapon_content_revision.expect("validated weapon shape"),
        item_level: weapon_item_level.expect("validated weapon shape"),
        rarity: weapon_rarity.expect("validated weapon shape"),
    });
    let loadout = StoredCoreCombatLoadout {
        character_id,
        selected_character_id,
        class_id: row.try_get("class_id")?,
        level: row.try_get("level")?,
        oath_id: row.try_get("oath_id")?,
        life_state: row.try_get("life_state")?,
        security_state: row.try_get("security_state")?,
        character_state_version: row.try_get("character_state_version")?,
        inventory_version: row.try_get("inventory_version")?,
        equipped_weapon,
    };
    validate_loadout_shape(&loadout)?;
    Ok(loadout)
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
        || loadout.character_state_version <= 0
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
            oath_id: Some("oath.arbalist.long_vigil".into()),
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
}
