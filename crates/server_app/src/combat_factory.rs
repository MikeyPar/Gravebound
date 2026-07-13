//! Fail-closed construction of authoritative Core combat from one persisted loadout snapshot.

use std::path::Path;

use persistence::{PostgresPersistence, StoredCoreCombatLoadout};
use protocol::GRAVE_ARBALIST_CLASS_ID;
use sim_core::{EquipmentRarity, PlayerCombatState};
use thiserror::Error;

const UNMODIFIED_ATTACK_RATE_BASIS_POINTS: u32 = 10_000;

#[derive(Debug, Clone)]
pub struct CoreCharacterCombat {
    pub character_id: [u8; 16],
    pub character_state_version: u64,
    pub inventory_version: u64,
    pub level: u16,
    pub maximum_health_multiplier_basis_points: u32,
    pub state: PlayerCombatState,
}

#[derive(Debug, Clone)]
pub struct CoreCharacterCombatFactory {
    persistence: PostgresPersistence,
    compiler: CoreCharacterCombatCompiler,
}

#[derive(Debug, Clone)]
pub struct CoreCharacterCombatCompiler {
    class_package: sim_content::ContentPackage,
    items: sim_content::CompiledProductionItemCatalog,
    oaths: sim_content::CompiledOathBargainCatalog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CoreCombatFactoryError {
    #[error("authoritative combat loadout is unavailable")]
    Unavailable,
    #[error("character is not the account's selected character")]
    CharacterNotSelected,
    #[error("character class is unavailable in Core")]
    ClassUnavailable,
    #[error("character life is not eligible for combat construction")]
    LifeUnavailable,
    #[error("character mutation is unresolved")]
    UnresolvedMutation,
    #[error("character has no selected Oath")]
    OathRequired,
    #[error("durable inventory is unavailable")]
    InventoryUnavailable,
    #[error("equipped weapon is unavailable")]
    WeaponUnavailable,
    #[error("equipped weapon content does not match the active Core revision")]
    ContentMismatch,
    #[error("rolled weapon combat projection is not active in this Core stage")]
    RolledWeaponStageDisabled,
    #[error("compiled combat content is invalid")]
    InvalidContent,
}

impl CoreCharacterCombatFactory {
    pub fn load(
        persistence: PostgresPersistence,
        content_root: &Path,
    ) -> Result<Self, CoreCombatFactoryError> {
        Ok(Self {
            persistence,
            compiler: CoreCharacterCombatCompiler::load(content_root)?,
        })
    }

    pub async fn build(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> Result<CoreCharacterCombat, CoreCombatFactoryError> {
        let snapshot = self
            .persistence
            .core_combat_loadout_snapshot(account_id, character_id)
            .await
            .map_err(|_| CoreCombatFactoryError::Unavailable)?
            .ok_or(CoreCombatFactoryError::Unavailable)?;
        self.compiler.build_from_snapshot(&snapshot)
    }
}

impl CoreCharacterCombatCompiler {
    pub fn load(content_root: &Path) -> Result<Self, CoreCombatFactoryError> {
        let (class_package, _) = sim_content::load_and_validate(content_root)
            .map_err(|_| CoreCombatFactoryError::InvalidContent)?;
        let items = sim_content::load_core_development_items(content_root)
            .map_err(|_| CoreCombatFactoryError::InvalidContent)?;
        let oaths = sim_content::load_core_development_oaths_bargains(content_root)
            .map_err(|_| CoreCombatFactoryError::InvalidContent)?;
        Ok(Self {
            class_package,
            items,
            oaths,
        })
    }

    pub fn build_from_snapshot(
        &self,
        snapshot: &StoredCoreCombatLoadout,
    ) -> Result<CoreCharacterCombat, CoreCombatFactoryError> {
        if snapshot.selected_character_id != Some(snapshot.character_id) {
            return Err(CoreCombatFactoryError::CharacterNotSelected);
        }
        if snapshot.class_id != GRAVE_ARBALIST_CLASS_ID {
            return Err(CoreCombatFactoryError::ClassUnavailable);
        }
        if snapshot.life_state != 0 {
            return Err(CoreCombatFactoryError::LifeUnavailable);
        }
        if snapshot.security_state != 0 {
            return Err(CoreCombatFactoryError::UnresolvedMutation);
        }
        let oath_id = snapshot
            .oath_id
            .as_deref()
            .ok_or(CoreCombatFactoryError::OathRequired)?;
        let inventory_version = snapshot
            .inventory_version
            .and_then(|value| u64::try_from(value).ok())
            .ok_or(CoreCombatFactoryError::InventoryUnavailable)?;
        let weapon = snapshot
            .equipped_weapon
            .as_ref()
            .ok_or(CoreCombatFactoryError::WeaponUnavailable)?;
        if weapon.content_revision != self.items.revision_label() {
            return Err(CoreCombatFactoryError::ContentMismatch);
        }
        // Durable affix identities/resolved values are owned by 04F. Until that projection exists,
        // only the exact Worn starter weapon is safe to construct; rolled rewards fail closed.
        if weapon.rarity != 0 {
            return Err(CoreCombatFactoryError::RolledWeaponStageDisabled);
        }
        let definitions = sim_content::compile_core_oathed_combat_definitions_for_item(
            &self.class_package,
            &self.items,
            &self.oaths,
            oath_id,
            &weapon.template_id,
            u8::try_from(weapon.item_level).map_err(|_| CoreCombatFactoryError::InvalidContent)?,
            EquipmentRarity::Worn,
            0,
            UNMODIFIED_ATTACK_RATE_BASIS_POINTS,
        )
        .map_err(|_| CoreCombatFactoryError::InvalidContent)?;
        let state = PlayerCombatState::with_oath(
            definitions.weapon,
            definitions.grave_mark,
            definitions.slipstep,
            definitions.stillness,
            definitions.oath,
        )
        .map_err(|_| CoreCombatFactoryError::InvalidContent)?;
        Ok(CoreCharacterCombat {
            character_id: snapshot.character_id,
            character_state_version: u64::try_from(snapshot.character_state_version)
                .map_err(|_| CoreCombatFactoryError::InvalidContent)?,
            inventory_version,
            level: u16::try_from(snapshot.level)
                .map_err(|_| CoreCombatFactoryError::InvalidContent)?,
            maximum_health_multiplier_basis_points: definitions
                .maximum_health_multiplier_basis_points,
            state,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use persistence::StoredEquippedWeapon;
    use protocol::{LONG_VIGIL_ID, NAILKEEPER_ID};
    use sim_core::GraveArbalistOath;

    use super::*;

    fn compiler() -> CoreCharacterCombatCompiler {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content");
        CoreCharacterCombatCompiler::load(&root).unwrap()
    }

    fn snapshot(compiler: &CoreCharacterCombatCompiler, oath_id: &str) -> StoredCoreCombatLoadout {
        StoredCoreCombatLoadout {
            character_id: [2; 16],
            selected_character_id: Some([2; 16]),
            class_id: GRAVE_ARBALIST_CLASS_ID.into(),
            level: 10,
            oath_id: Some(oath_id.into()),
            oath_bargain_version: 1,
            active_bargains: Vec::new(),
            life_state: 0,
            security_state: 0,
            character_state_version: 8,
            inventory_version: Some(4),
            equipped_weapon: Some(StoredEquippedWeapon {
                item_uid: [3; 16],
                template_id: "item.weapon.crossbow.pine_crossbow".into(),
                content_revision: compiler.items.revision_label().into(),
                item_level: 1,
                rarity: 0,
            }),
            belt_slots: [None, None],
        }
    }

    #[test]
    fn persisted_oath_selects_the_exact_authoritative_combat_definition() {
        let compiler = compiler();
        let vigil = compiler
            .build_from_snapshot(&snapshot(&compiler, LONG_VIGIL_ID))
            .unwrap();
        assert_eq!(vigil.state.oath(), Some(GraveArbalistOath::LongVigil));
        assert_eq!(vigil.maximum_health_multiplier_basis_points, 9_000);
        let nailkeeper = compiler
            .build_from_snapshot(&snapshot(&compiler, NAILKEEPER_ID))
            .unwrap();
        assert_eq!(nailkeeper.state.oath(), Some(GraveArbalistOath::Nailkeeper));
        assert_eq!(nailkeeper.maximum_health_multiplier_basis_points, 10_000);
        assert!(
            nailkeeper.state.weapon().attack_interval_ticks()
                > vigil.state.weapon().attack_interval_ticks()
        );
    }

    #[test]
    fn absent_unknown_or_unprojected_state_fails_closed() {
        let compiler = compiler();
        let mut value = snapshot(&compiler, LONG_VIGIL_ID);
        value.oath_id = None;
        assert_eq!(
            compiler.build_from_snapshot(&value).unwrap_err(),
            CoreCombatFactoryError::OathRequired
        );
        value = snapshot(&compiler, "oath.arbalist.unknown");
        assert_eq!(
            compiler.build_from_snapshot(&value).unwrap_err(),
            CoreCombatFactoryError::InvalidContent
        );
        value = snapshot(&compiler, LONG_VIGIL_ID);
        value.equipped_weapon.as_mut().unwrap().rarity = 1;
        assert_eq!(
            compiler.build_from_snapshot(&value).unwrap_err(),
            CoreCombatFactoryError::RolledWeaponStageDisabled
        );
    }
}
