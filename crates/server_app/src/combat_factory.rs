//! Fail-closed construction of authoritative Core combat from one persisted loadout snapshot.

use std::path::Path;

use persistence::{PostgresPersistence, StoredCombatBeltStack, StoredCoreCombatLoadout};
use protocol::GRAVE_ARBALIST_CLASS_ID;
use sim_core::{
    BeltSlot, CoreBargainLoadout, EquipmentRarity, PlayerCombatState, PlayerVitals,
    RedTonicSimulation, ResolvedCoreBargainModifiers, TonicBelt, TonicBeltPolicy,
};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct CoreCharacterCombat {
    pub character_id: [u8; 16],
    pub character_state_version: u64,
    pub progression_version: u64,
    pub inventory_version: u64,
    pub oath_bargain_version: u64,
    pub level: u16,
    pub maximum_health: u32,
    pub bargains: CoreBargainLoadout,
    pub bargain_modifiers: ResolvedCoreBargainModifiers,
    pub maximum_health_multiplier_basis_points: u32,
    pub state: PlayerCombatState,
    pub consumables: RedTonicSimulation,
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
    progression: sim_content::CoreDevelopmentProgression,
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
        let progression = sim_content::load_core_development_progression(content_root)
            .map_err(|_| CoreCombatFactoryError::InvalidContent)?;
        Ok(Self {
            class_package,
            items,
            oaths,
            progression,
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
        let (character_state_version, progression_version, inventory_version, oath_bargain_version) =
            compile_aggregate_versions(snapshot)?;
        let weapon = snapshot
            .equipped_weapon
            .as_ref()
            .ok_or(CoreCombatFactoryError::WeaponUnavailable)?;
        if weapon.content_revision != self.items.revision_label() {
            return Err(CoreCombatFactoryError::ContentMismatch);
        }
        if snapshot
            .active_bargains
            .iter()
            .any(|bargain| bargain.acquiring_offer_content_version != self.oaths.revision_label())
        {
            return Err(CoreCombatFactoryError::ContentMismatch);
        }
        // Durable affix identities/resolved values are owned by 04F. Until that projection exists,
        // only the exact Worn starter weapon is safe to construct; rolled rewards fail closed.
        if weapon.rarity != 0 {
            return Err(CoreCombatFactoryError::RolledWeaponStageDisabled);
        }
        let bargain_ids = snapshot
            .active_bargains
            .iter()
            .map(|bargain| bargain.bargain_id.as_str())
            .collect::<Vec<_>>();
        let definitions = sim_content::compile_core_combat_definitions_for_item(
            &self.class_package,
            &self.items,
            &self.oaths,
            snapshot.oath_id.as_deref(),
            &bargain_ids,
            &weapon.template_id,
            u8::try_from(weapon.item_level).map_err(|_| CoreCombatFactoryError::InvalidContent)?,
            EquipmentRarity::Worn,
            0,
        )
        .map_err(|_| CoreCombatFactoryError::InvalidContent)?;
        let level =
            u16::try_from(snapshot.level).map_err(|_| CoreCombatFactoryError::InvalidContent)?;
        let level_stats = sim_core::grave_arbalist_level_stats(self.progression.arbalist(), level)
            .map_err(|_| CoreCombatFactoryError::InvalidContent)?;
        let outgoing_direct_damage_basis_points =
            sim_core::compose_outgoing_direct_damage_multiplier(
                level_stats.damage_multiplier_basis_points,
                definitions
                    .bargain_modifiers
                    .outgoing_direct_damage_basis_points,
            )
            .map_err(|_| CoreCombatFactoryError::InvalidContent)?;
        let maximum_health = sim_core::resolve_oath_maximum_health(
            level_stats.maximum_health,
            definitions.maximum_health_multiplier_basis_points,
        )
        .map_err(|_| CoreCombatFactoryError::InvalidContent)?;
        let consumables = compile_consumables(
            snapshot,
            self.items.revision_label(),
            &self.class_package,
            maximum_health,
            definitions.bargains.lantern_ash(),
        )?;
        let state = PlayerCombatState::with_core_choices(
            definitions.weapon,
            definitions.grave_mark,
            definitions.slipstep,
            definitions.stillness,
            definitions.oath,
            outgoing_direct_damage_basis_points,
            definitions.bargains.bell_debt(),
        )
        .map_err(|_| CoreCombatFactoryError::InvalidContent)?;
        Ok(CoreCharacterCombat {
            character_id: snapshot.character_id,
            character_state_version,
            progression_version,
            inventory_version,
            oath_bargain_version,
            level,
            maximum_health,
            bargains: definitions.bargains,
            bargain_modifiers: definitions.bargain_modifiers,
            maximum_health_multiplier_basis_points: definitions
                .maximum_health_multiplier_basis_points,
            state,
            consumables,
        })
    }
}

fn compile_aggregate_versions(
    snapshot: &StoredCoreCombatLoadout,
) -> Result<(u64, u64, u64, u64), CoreCombatFactoryError> {
    let convert = |value| u64::try_from(value).map_err(|_| CoreCombatFactoryError::InvalidContent);
    Ok((
        convert(snapshot.character_state_version)?,
        convert(snapshot.progression_version)?,
        snapshot
            .inventory_version
            .and_then(|value| u64::try_from(value).ok())
            .ok_or(CoreCombatFactoryError::InventoryUnavailable)?,
        convert(snapshot.oath_bargain_version)?,
    ))
}

fn compile_consumables(
    snapshot: &StoredCoreCombatLoadout,
    required_item_revision: &str,
    class_package: &sim_content::ContentPackage,
    maximum_health: u32,
    lantern_ash: Option<sim_core::LanternAshDefinition>,
) -> Result<RedTonicSimulation, CoreCombatFactoryError> {
    let stored_current_health = u32::try_from(snapshot.current_health)
        .map_err(|_| CoreCombatFactoryError::InvalidContent)?;
    // Only absolute current health is durable. Any historical maximum at least this large gives
    // the same approved rebuild result, so use the smallest valid value instead of inventing a
    // percentage or healing on load.
    let current_health = sim_core::rebuild_current_health(
        stored_current_health,
        stored_current_health.max(maximum_health),
        maximum_health,
    )
    .map_err(|_| CoreCombatFactoryError::InvalidContent)?;
    let belt = TonicBelt::from_slots([
        compile_belt_slot(snapshot.belt_slots[0].as_ref(), required_item_revision)?,
        compile_belt_slot(snapshot.belt_slots[1].as_ref(), required_item_revision)?,
    ])
    .map_err(|_| CoreCombatFactoryError::InvalidContent)?;
    let belt_policy = lantern_ash
        .map_or_else(
            || Ok(TonicBeltPolicy::normal()),
            TonicBeltPolicy::lantern_ash,
        )
        .map_err(|_| CoreCombatFactoryError::InvalidContent)?;
    RedTonicSimulation::with_policy(
        sim_content::first_playable_red_tonic(class_package)
            .map_err(|_| CoreCombatFactoryError::InvalidContent)?,
        PlayerVitals::new(current_health, maximum_health)
            .map_err(|_| CoreCombatFactoryError::InvalidContent)?,
        belt,
        belt_policy,
    )
    .map_err(|_| CoreCombatFactoryError::InvalidContent)
}

fn compile_belt_slot(
    stored: Option<&StoredCombatBeltStack>,
    required_revision: &str,
) -> Result<BeltSlot, CoreCombatFactoryError> {
    let Some(stored) = stored else {
        return Ok(BeltSlot::Empty);
    };
    if stored.template_id != sim_core::RED_TONIC_CONTENT_ID
        || stored.content_revision != required_revision
    {
        return Err(CoreCombatFactoryError::ContentMismatch);
    }
    Ok(BeltSlot::RedTonic(
        u8::try_from(stored.quantity).map_err(|_| CoreCombatFactoryError::InvalidContent)?,
    ))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use persistence::{StoredCombatBargain, StoredCombatBeltStack, StoredEquippedWeapon};
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
            current_health: 120,
            oath_id: Some(oath_id.into()),
            oath_bargain_version: 1,
            active_bargains: Vec::new(),
            life_state: 0,
            security_state: 0,
            character_state_version: 8,
            progression_version: 3,
            inventory_version: Some(4),
            equipped_weapon: Some(StoredEquippedWeapon {
                item_uid: [3; 16],
                template_id: "item.weapon.crossbow.pine_crossbow".into(),
                content_revision: compiler.items.revision_label().into(),
                item_level: 1,
                rarity: 0,
            }),
            equipped_armor: None,
            equipped_relic: None,
            equipped_charm: None,
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
        value.level = 5;
        let no_oath = compiler.build_from_snapshot(&value).unwrap();
        assert_eq!(no_oath.state.oath(), None);
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

    #[test]
    fn level_five_cinder_loadout_uses_the_acquiring_content_revision_without_an_oath() {
        let compiler = compiler();
        let mut value = snapshot(&compiler, LONG_VIGIL_ID);
        value.level = 5;
        value.oath_id = None;
        value.oath_bargain_version = 2;
        value.active_bargains.push(StoredCombatBargain {
            bargain_id: "bargain.cinder_hunger".into(),
            acquisition_ordinal: 1,
            acquired_by_offer_id: [4; 16],
            acquiring_offer_content_version: compiler.oaths.revision_label().into(),
        });
        let combat = compiler.build_from_snapshot(&value).unwrap();
        assert_eq!(combat.state.oath(), None);
        assert_eq!(combat.oath_bargain_version, 2);
        assert_eq!(combat.bargains.definitions().len(), 1);
        assert_eq!(combat.maximum_health_multiplier_basis_points, 8_800);
        assert_eq!(combat.maximum_health, 120);
        assert_eq!(combat.state.outgoing_direct_damage_basis_points(), 12_508);
        value.active_bargains[0]
            .acquiring_offer_content_version
            .push_str(".drift");
        assert_eq!(
            compiler.build_from_snapshot(&value).unwrap_err(),
            CoreCombatFactoryError::ContentMismatch
        );
    }

    #[test]
    fn combat_rebuild_preserves_absolute_health_and_clamps_only_on_decrease() {
        let compiler = compiler();
        let mut value = snapshot(&compiler, LONG_VIGIL_ID);
        value.current_health = 120;
        let unchanged = compiler.build_from_snapshot(&value).unwrap();
        assert_eq!(unchanged.maximum_health, 140);
        assert_eq!(unchanged.consumables.vitals().current_health(), 120);

        value.current_health = 150;
        let clamped = compiler.build_from_snapshot(&value).unwrap();
        assert_eq!(clamped.maximum_health, 140);
        assert_eq!(clamped.consumables.vitals().current_health(), 140);
    }

    #[test]
    fn lantern_loadout_preserves_both_stacks_and_locks_only_second_input() {
        let compiler = compiler();
        let mut value = snapshot(&compiler, LONG_VIGIL_ID);
        value.level = 5;
        value.current_health = 60;
        value.oath_id = None;
        value.oath_bargain_version = 2;
        value.active_bargains.push(StoredCombatBargain {
            bargain_id: "bargain.lantern_ash".into(),
            acquisition_ordinal: 1,
            acquired_by_offer_id: [5; 16],
            acquiring_offer_content_version: compiler.oaths.revision_label().into(),
        });
        let stack = |quantity| StoredCombatBeltStack {
            template_id: sim_core::RED_TONIC_CONTENT_ID.into(),
            content_revision: compiler.items.revision_label().into(),
            quantity,
        };
        value.belt_slots = [Some(stack(2)), Some(stack(3))];
        let mut combat = compiler.build_from_snapshot(&value).unwrap();
        assert_eq!(
            combat.consumables.belt().slot(1),
            Some(BeltSlot::RedTonic(3))
        );
        assert!(!combat.consumables.belt_policy().is_active(1));
        assert_eq!(
            combat
                .consumables
                .belt_policy()
                .potion_healing_multiplier_basis_points(),
            14_000
        );
        let result = combat
            .consumables
            .step(sim_core::ConsumableAction {
                use_second_slot_press_sequence: 1,
                ..sim_core::ConsumableAction::default()
            })
            .unwrap();
        assert!(
            result
                .events
                .contains(&sim_core::ConsumableEvent::UseRejected {
                    tick: sim_core::Tick(1),
                    press_sequence: 1,
                    reason: sim_core::TonicUseRejection::InactiveBeltSlot { index: 1 },
                })
        );
        assert_eq!(
            combat.consumables.belt().slot(1),
            Some(BeltSlot::RedTonic(3))
        );
    }
}
