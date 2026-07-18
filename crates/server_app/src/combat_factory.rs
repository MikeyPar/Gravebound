//! Fail-closed construction of authoritative Core combat from one persisted loadout snapshot.

use std::path::Path;

use persistence::{PostgresPersistence, StoredCombatBeltStack, StoredCoreCombatLoadout};
use protocol::GRAVE_ARBALIST_CLASS_ID;
use sim_core::{
    BeltSlot, CoreBargainLoadout, EnemyLabPlayer, EntityId, EquipmentRarity, HostileTargetState,
    PlayerCombatState, PlayerVitals, RedTonicSimulation, ResolvedCoreBargainModifiers,
    SimulationVector, TonicBelt, TonicBeltPolicy,
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
    pub armor: u32,
    pub resistance_basis_points: i32,
    pub movement_milli_tiles_per_second: u32,
    pub healing_received_multiplier_basis_points: u32,
    pub negative_status_reduction_basis_points: u32,
    pub direct_hit_barrier_health: Option<u32>,
    pub rested_primary_bonus_basis_points: u32,
    pub rested_primary_idle_millis: u32,
    pub relic_resonance_basis_points: u32,
    pub equipment: sim_content::ResolvedCoreEquipmentLoadout,
    pub bargains: CoreBargainLoadout,
    pub bargain_modifiers: ResolvedCoreBargainModifiers,
    pub maximum_health_multiplier_basis_points: u32,
    pub state: PlayerCombatState,
    pub consumables: RedTonicSimulation,
}

/// Immutable/versioned half of one live Core combat aggregate. Mutable combat, health, Belt,
/// Bell Debt, cooldown, and projectile authority move into exactly one `EnemyLabPlayer`; they are
/// never cloned into a scene owner. The envelope rejoins that player at a safe/terminal handoff.
#[derive(Debug, Clone)]
pub struct CoreCharacterCombatEnvelope {
    character_id: [u8; 16],
    character_state_version: u64,
    progression_version: u64,
    inventory_version: u64,
    oath_bargain_version: u64,
    level: u16,
    maximum_health: u32,
    armor: u32,
    resistance_basis_points: i32,
    movement_milli_tiles_per_second: u32,
    healing_received_multiplier_basis_points: u32,
    negative_status_reduction_basis_points: u32,
    direct_hit_barrier_health: Option<u32>,
    rested_primary_bonus_basis_points: u32,
    rested_primary_idle_millis: u32,
    relic_resonance_basis_points: u32,
    equipment: sim_content::ResolvedCoreEquipmentLoadout,
    bargains: CoreBargainLoadout,
    bargain_modifiers: ResolvedCoreBargainModifiers,
    maximum_health_multiplier_basis_points: u32,
    player_entity_id: EntityId,
}

impl CoreCharacterCombat {
    /// Moves the one mutable combat aggregate into a live scene participant.
    pub fn into_live_player(
        self,
        player_entity_id: EntityId,
        position: SimulationVector,
    ) -> Result<(CoreCharacterCombatEnvelope, EnemyLabPlayer), CoreCombatFactoryError> {
        if !position.is_finite() || self.character_id == [0; 16] {
            return Err(CoreCombatFactoryError::InvalidLiveHandoff);
        }
        let envelope = CoreCharacterCombatEnvelope {
            character_id: self.character_id,
            character_state_version: self.character_state_version,
            progression_version: self.progression_version,
            inventory_version: self.inventory_version,
            oath_bargain_version: self.oath_bargain_version,
            level: self.level,
            maximum_health: self.maximum_health,
            armor: self.armor,
            resistance_basis_points: self.resistance_basis_points,
            movement_milli_tiles_per_second: self.movement_milli_tiles_per_second,
            healing_received_multiplier_basis_points: self.healing_received_multiplier_basis_points,
            negative_status_reduction_basis_points: self.negative_status_reduction_basis_points,
            direct_hit_barrier_health: self.direct_hit_barrier_health,
            rested_primary_bonus_basis_points: self.rested_primary_bonus_basis_points,
            rested_primary_idle_millis: self.rested_primary_idle_millis,
            relic_resonance_basis_points: self.relic_resonance_basis_points,
            equipment: self.equipment,
            bargains: self.bargains,
            bargain_modifiers: self.bargain_modifiers,
            maximum_health_multiplier_basis_points: self.maximum_health_multiplier_basis_points,
            player_entity_id,
        };
        let player = EnemyLabPlayer {
            target: HostileTargetState {
                entity_id: player_entity_id,
                position,
                target_is_immune: false,
                resistance_basis_points: envelope.resistance_basis_points,
                additional_direct_damage_reductions_basis_points: Vec::new(),
                armor: envelope.armor,
                current_barrier: 0,
                health_damage_cap_basis_points: None,
            },
            consumables: self.consumables,
            combat: self.state,
        };
        Ok((envelope, player))
    }
}

impl CoreCharacterCombatEnvelope {
    #[must_use]
    pub const fn character_id(&self) -> [u8; 16] {
        self.character_id
    }

    #[must_use]
    pub const fn movement_milli_tiles_per_second(&self) -> u32 {
        self.movement_milli_tiles_per_second
    }

    #[must_use]
    pub(crate) const fn character_state_version(&self) -> u64 {
        self.character_state_version
    }

    /// Rebases only the character aggregate version after an exact committed scene transfer.
    /// Mutable combat remains in the moved player allocation; skipped, repeated, or stale
    /// versions fail closed.
    pub(crate) fn rebase_character_state_version(
        &mut self,
        source: u64,
        destination: u64,
    ) -> Result<(), CoreCombatFactoryError> {
        if self.character_state_version != source || source.checked_add(1) != Some(destination) {
            return Err(CoreCombatFactoryError::InvalidLiveHandoff);
        }
        self.character_state_version = destination;
        Ok(())
    }

    /// Rejoins the exact player allocation after a scene handoff. Foreign entity identity or
    /// immutable combat-axis drift fails closed instead of silently rebuilding mutable state.
    pub fn rejoin(
        self,
        player: EnemyLabPlayer,
    ) -> Result<CoreCharacterCombat, CoreCombatFactoryError> {
        if player.target.entity_id != self.player_entity_id
            || player.target.armor != self.armor
            || player.target.resistance_basis_points != self.resistance_basis_points
            || player.target.target_is_immune
            || player.consumables.vitals().maximum_health() != self.maximum_health
        {
            return Err(CoreCombatFactoryError::InvalidLiveHandoff);
        }
        Ok(CoreCharacterCombat {
            character_id: self.character_id,
            character_state_version: self.character_state_version,
            progression_version: self.progression_version,
            inventory_version: self.inventory_version,
            oath_bargain_version: self.oath_bargain_version,
            level: self.level,
            maximum_health: self.maximum_health,
            armor: self.armor,
            resistance_basis_points: self.resistance_basis_points,
            movement_milli_tiles_per_second: self.movement_milli_tiles_per_second,
            healing_received_multiplier_basis_points: self.healing_received_multiplier_basis_points,
            negative_status_reduction_basis_points: self.negative_status_reduction_basis_points,
            direct_hit_barrier_health: self.direct_hit_barrier_health,
            rested_primary_bonus_basis_points: self.rested_primary_bonus_basis_points,
            rested_primary_idle_millis: self.rested_primary_idle_millis,
            relic_resonance_basis_points: self.relic_resonance_basis_points,
            equipment: self.equipment,
            bargains: self.bargains,
            bargain_modifiers: self.bargain_modifiers,
            maximum_health_multiplier_basis_points: self.maximum_health_multiplier_basis_points,
            state: player.combat,
            consumables: player.consumables,
        })
    }
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
    #[error("equipment rarity is unavailable in the Core stage")]
    EquipmentRarityUnavailable,
    #[error("compiled combat content is invalid")]
    InvalidContent,
    #[error("live Core combat handoff is invalid or foreign")]
    InvalidLiveHandoff,
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

    #[allow(clippy::too_many_lines)] // Linear construction keeps every authoritative source/order visible.
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
        let equipment = sim_content::compose_core_equipment_loadout([
            Some(self.resolve_stored_equipment(weapon, sim_core::EquipmentSlot::Weapon)?),
            snapshot
                .equipped_relic
                .as_ref()
                .map(|item| self.resolve_stored_equipment(item, sim_core::EquipmentSlot::Relic))
                .transpose()?,
            snapshot
                .equipped_armor
                .as_ref()
                .map(|item| self.resolve_stored_equipment(item, sim_core::EquipmentSlot::Armor))
                .transpose()?,
            snapshot
                .equipped_charm
                .as_ref()
                .map(|item| self.resolve_stored_equipment(item, sim_core::EquipmentSlot::Charm))
                .transpose()?,
        ])
        .map_err(|_| CoreCombatFactoryError::InvalidContent)?;
        let bargain_ids = snapshot
            .active_bargains
            .iter()
            .map(|bargain| bargain.bargain_id.as_str())
            .collect::<Vec<_>>();
        let definitions = sim_content::compile_core_combat_definitions_for_loadout(
            &self.class_package,
            &self.items,
            &self.oaths,
            snapshot.oath_id.as_deref(),
            &bargain_ids,
            &equipment,
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
        let pre_multiplier_maximum_health = level_stats
            .maximum_health
            .checked_add(equipment.maximum_health_flat)
            .ok_or(CoreCombatFactoryError::InvalidContent)?;
        let maximum_health = sim_core::resolve_oath_maximum_health(
            pre_multiplier_maximum_health,
            definitions.maximum_health_multiplier_basis_points,
        )
        .map_err(|_| CoreCombatFactoryError::InvalidContent)?;
        let consumables = compile_consumables(
            snapshot,
            self.items.revision_label(),
            &self.class_package,
            maximum_health,
            definitions.bargains.lantern_ash(),
            equipment.potion_healing_output_multiplier_basis_points,
        )?;
        let armor = u32::from(level_stats.armor)
            .checked_add(equipment.armor_flat)
            .ok_or(CoreCombatFactoryError::InvalidContent)?;
        let movement_milli_tiles_per_second = apply_basis_points(
            level_stats.movement_milli_tiles_per_second,
            equipment.movement_multiplier_basis_points,
        )?
        .clamp(4_500, 5_600);
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
            armor,
            resistance_basis_points: equipment.resistance_basis_points.clamp(-2_500, 2_500),
            movement_milli_tiles_per_second,
            healing_received_multiplier_basis_points: equipment
                .healing_received_multiplier_basis_points,
            negative_status_reduction_basis_points: equipment
                .negative_status_reduction_basis_points,
            direct_hit_barrier_health: equipment.direct_hit_barrier_health,
            rested_primary_bonus_basis_points: equipment.rested_primary_bonus_basis_points,
            rested_primary_idle_millis: equipment.rested_primary_idle_millis,
            relic_resonance_basis_points: equipment.relic_resonance_basis_points,
            equipment,
            bargains: definitions.bargains,
            bargain_modifiers: definitions.bargain_modifiers,
            maximum_health_multiplier_basis_points: definitions
                .maximum_health_multiplier_basis_points,
            state,
            consumables,
        })
    }

    fn resolve_stored_equipment(
        &self,
        item: &persistence::StoredEquippedWeapon,
        required_slot: sim_core::EquipmentSlot,
    ) -> Result<sim_content::CoreEquipmentPresentation, CoreCombatFactoryError> {
        if item.content_revision != self.items.revision_label() {
            return Err(CoreCombatFactoryError::ContentMismatch);
        }
        let rarity = match item.rarity {
            0 => EquipmentRarity::Worn,
            1 => EquipmentRarity::Forged,
            _ => return Err(CoreCombatFactoryError::EquipmentRarityUnavailable),
        };
        let presentation = sim_content::resolve_core_equipment_presentation(
            &self.items,
            &item.template_id,
            u8::try_from(item.item_level).map_err(|_| CoreCombatFactoryError::InvalidContent)?,
            rarity,
        )
        .map_err(|_| CoreCombatFactoryError::InvalidContent)?;
        if presentation.slot != required_slot {
            return Err(CoreCombatFactoryError::InvalidContent);
        }
        Ok(presentation)
    }
}

fn apply_basis_points(
    value: u32,
    multiplier_basis_points: u32,
) -> Result<u32, CoreCombatFactoryError> {
    let numerator = u64::from(value)
        .checked_mul(u64::from(multiplier_basis_points))
        .and_then(|resolved| resolved.checked_add(5_000))
        .ok_or(CoreCombatFactoryError::InvalidContent)?;
    u32::try_from(numerator / 10_000).map_err(|_| CoreCombatFactoryError::InvalidContent)
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
    equipment_potion_output_multiplier_basis_points: u32,
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
        .map_err(|_| CoreCombatFactoryError::InvalidContent)?
        .with_potion_output_multiplier(equipment_potion_output_multiplier_basis_points)
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
pub(crate) fn core_character_combat_test_fixture(character_id: [u8; 16]) -> CoreCharacterCombat {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content");
    let compiler = CoreCharacterCombatCompiler::load(&root).expect("Core combat compiler");
    compiler
        .build_from_snapshot(&StoredCoreCombatLoadout {
            character_id,
            selected_character_id: Some(character_id),
            class_id: GRAVE_ARBALIST_CLASS_ID.into(),
            level: 1,
            current_health: 120,
            oath_id: None,
            oath_bargain_version: 1,
            active_bargains: Vec::new(),
            life_state: 0,
            security_state: 0,
            character_state_version: 2,
            progression_version: 1,
            inventory_version: Some(1),
            equipped_weapon: Some(persistence::StoredEquippedWeapon {
                item_uid: [0xA5; 16],
                template_id: "item.weapon.crossbow.pine_crossbow".into(),
                content_revision: compiler.items.revision_label().into(),
                item_level: 1,
                rarity: 0,
            }),
            equipped_armor: None,
            equipped_relic: None,
            equipped_charm: None,
            belt_slots: [None, None],
        })
        .expect("Core combat fixture")
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
        value.equipped_weapon.as_mut().unwrap().rarity = 2;
        assert_eq!(
            compiler.build_from_snapshot(&value).unwrap_err(),
            CoreCombatFactoryError::EquipmentRarityUnavailable
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
    fn forged_four_slot_loadout_builds_one_authoritative_combat_projection() {
        let compiler = compiler();
        let mut value = snapshot(&compiler, LONG_VIGIL_ID);
        let item = |uid: u8, template_id: &str, item_level: i16| StoredEquippedWeapon {
            item_uid: [uid; 16],
            template_id: template_id.into(),
            content_revision: compiler.items.revision_label().into(),
            item_level,
            rarity: 1,
        };
        value.equipped_weapon = Some(item(10, "item.weapon.crossbow.grave_repeater", 10));
        value.equipped_relic = Some(item(11, "item.relic.arbalist.long_lens", 10));
        value.equipped_armor = Some(item(12, "item.armor.saltglass.t1", 10));
        value.equipped_charm = Some(item(13, "item.charm.bell_locket.t1", 10));
        value.current_health = 150;
        let combat = compiler.build_from_snapshot(&value).unwrap();
        assert_eq!(combat.maximum_health, 153);
        assert_eq!(combat.consumables.vitals().current_health(), 150);
        assert_eq!(combat.armor, 4);
        assert_eq!(combat.resistance_basis_points, 700);
        assert_eq!(combat.healing_received_multiplier_basis_points, 9_200);
        assert_eq!(
            combat
                .consumables
                .belt_policy()
                .potion_healing_multiplier_basis_points(),
            11_000
        );
        assert_eq!(combat.relic_resonance_basis_points, 10_360);
        assert_eq!(
            combat.state.grave_mark_definition().range_milli_tiles(),
            15_000
        );
        assert_eq!(
            combat
                .state
                .grave_mark_definition()
                .projectile_speed_milli_tiles_per_second(),
            15_000
        );
        assert_eq!(
            combat
                .state
                .grave_mark_definition()
                .weapon_damage_multiplier_basis_points(),
            15_500
        );
        assert_eq!(
            combat
                .state
                .grave_mark_definition()
                .marked_primary_bonus_basis_points(),
            2_000
        );
        assert_eq!(
            combat.equipment.weapon().template_id,
            "item.weapon.crossbow.grave_repeater"
        );
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
        value.equipped_charm = Some(StoredEquippedWeapon {
            item_uid: [9; 16],
            template_id: "item.charm.bell_locket.t1".into(),
            content_revision: compiler.items.revision_label().into(),
            item_level: 5,
            rarity: 1,
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
            15_000
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

    #[test]
    fn live_player_handoff_moves_and_rejoins_the_single_mutable_combat_owner() {
        let compiler = compiler();
        let combat = compiler
            .build_from_snapshot(&snapshot(&compiler, LONG_VIGIL_ID))
            .expect("compiled combat");
        let player_id = EntityId::new(10_001).expect("player ID");
        let (envelope, player) = combat
            .into_live_player(player_id, SimulationVector::new(8.5, 40.5))
            .expect("live handoff");

        assert_eq!(envelope.character_id(), [2; 16]);
        assert_eq!(player.target.entity_id, player_id);
        assert_eq!(player.target.position, SimulationVector::new(8.5, 40.5));
        assert_eq!(player.combat.tick(), sim_core::Tick(0));
        assert_eq!(player.consumables.vitals().current_health(), 120);

        let rejoined = envelope.rejoin(player).expect("safe handoff");
        assert_eq!(rejoined.character_id, [2; 16]);
        assert_eq!(rejoined.state.tick(), sim_core::Tick(0));
        assert_eq!(rejoined.consumables.vitals().current_health(), 120);
    }

    #[test]
    fn live_player_rejoin_rejects_foreign_entity_or_immutable_axis_drift() {
        let compiler = compiler();
        let combat = compiler
            .build_from_snapshot(&snapshot(&compiler, LONG_VIGIL_ID))
            .expect("compiled combat");
        let (envelope, mut player) = combat
            .into_live_player(
                EntityId::new(10_001).expect("player ID"),
                SimulationVector::new(8.5, 40.5),
            )
            .expect("live handoff");
        player.target.entity_id = EntityId::new(10_002).expect("foreign player ID");
        assert_eq!(
            envelope.rejoin(player).unwrap_err(),
            CoreCombatFactoryError::InvalidLiveHandoff
        );
    }
}
