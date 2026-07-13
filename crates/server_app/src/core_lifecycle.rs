//! Dormant Core character-life authority for `GB-M03-05F` lifecycle verification.
//!
//! This directory is intentionally not attached to the normal player route. It proves that one
//! account/character/lineage identity owns one mutable combat aggregate across transport and room
//! changes without rebuilding Bargain state from persistence.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::CoreCharacterCombat;

const ID_BYTES: usize = 16;
const MIN_ROOM_ID_BYTES: usize = 3;
const MAX_ROOM_ID_BYTES: usize = 96;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CoreLifeKey {
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub lineage_id: [u8; ID_BYTES],
}

impl CoreLifeKey {
    pub fn new(
        account_id: [u8; ID_BYTES],
        character_id: [u8; ID_BYTES],
        lineage_id: [u8; ID_BYTES],
    ) -> Result<Self, CoreLiveError> {
        if [account_id, character_id, lineage_id]
            .iter()
            .any(|value| value.iter().all(|byte| *byte == 0))
        {
            return Err(CoreLiveError::InvalidIdentity);
        }
        Ok(Self {
            account_id,
            character_id,
            lineage_id,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CoreLiveBindingId(u64);

impl CoreLiveBindingId {
    #[must_use]
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct CoreLiveCharacter {
    key: CoreLifeKey,
    binding_id: CoreLiveBindingId,
    room_id: String,
    combat: CoreCharacterCombat,
}

impl CoreLiveCharacter {
    #[must_use]
    pub const fn key(&self) -> CoreLifeKey {
        self.key
    }

    #[must_use]
    pub const fn binding_id(&self) -> CoreLiveBindingId {
        self.binding_id
    }

    #[must_use]
    pub fn room_id(&self) -> &str {
        &self.room_id
    }

    #[must_use]
    pub const fn combat(&self) -> &CoreCharacterCombat {
        &self.combat
    }

    pub const fn combat_mut(&mut self) -> &mut CoreCharacterCombat {
        &mut self.combat
    }
}

#[derive(Debug, Default)]
pub struct CoreLiveDirectory {
    lives: BTreeMap<CoreLifeKey, CoreLiveCharacter>,
}

impl CoreLiveDirectory {
    pub fn insert(
        &mut self,
        key: CoreLifeKey,
        binding_id: CoreLiveBindingId,
        room_id: impl Into<String>,
        combat: CoreCharacterCombat,
    ) -> Result<(), CoreLiveError> {
        let room_id = room_id.into();
        validate_room_id(&room_id)?;
        if combat.character_id != key.character_id {
            return Err(CoreLiveError::CombatIdentityMismatch);
        }
        if self.lives.contains_key(&key)
            || self
                .lives
                .keys()
                .any(|existing| existing.character_id == key.character_id)
        {
            return Err(CoreLiveError::AlreadyExists);
        }
        self.lives.insert(
            key,
            CoreLiveCharacter {
                key,
                binding_id,
                room_id,
                combat,
            },
        );
        Ok(())
    }

    pub fn reattach(
        &mut self,
        key: CoreLifeKey,
        new_binding_id: CoreLiveBindingId,
    ) -> Result<CoreLiveBindingId, CoreLiveError> {
        let live = self.lives.get_mut(&key).ok_or(CoreLiveError::NotFound)?;
        let replaced = live.binding_id;
        live.binding_id = new_binding_id;
        Ok(replaced)
    }

    pub fn handoff_danger_room(
        &mut self,
        key: CoreLifeKey,
        expected_room_id: &str,
        destination_room_id: impl Into<String>,
    ) -> Result<(), CoreLiveError> {
        let destination_room_id = destination_room_id.into();
        validate_room_id(&destination_room_id)?;
        let live = self.lives.get_mut(&key).ok_or(CoreLiveError::NotFound)?;
        if live.room_id != expected_room_id {
            return Err(CoreLiveError::StaleRoom);
        }
        live.room_id = destination_room_id;
        Ok(())
    }

    #[must_use]
    pub fn get(&self, key: CoreLifeKey) -> Option<&CoreLiveCharacter> {
        self.lives.get(&key)
    }

    pub fn get_mut(&mut self, key: CoreLifeKey) -> Option<&mut CoreLiveCharacter> {
        self.lives.get_mut(&key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CoreLiveError {
    #[error("Core live aggregate identity must be nonzero")]
    InvalidIdentity,
    #[error("Core live aggregate room ID is invalid")]
    InvalidRoom,
    #[error("compiled combat belongs to a different character")]
    CombatIdentityMismatch,
    #[error("a Core live aggregate already owns this character life")]
    AlreadyExists,
    #[error("Core live aggregate was not found for the exact life identity")]
    NotFound,
    #[error("dangerous-room handoff used a stale source room")]
    StaleRoom,
}

fn validate_room_id(value: &str) -> Result<(), CoreLiveError> {
    if !(MIN_ROOM_ID_BYTES..=MAX_ROOM_ID_BYTES).contains(&value.len())
        || value.starts_with('.')
        || value.ends_with('.')
        || value.split('.').any(|segment| {
            segment.is_empty()
                || !segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        })
    {
        return Err(CoreLiveError::InvalidRoom);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use persistence::{StoredCombatBargain, StoredCoreCombatLoadout, StoredEquippedWeapon};
    use protocol::{BELL_DEBT_ID, GRAVE_ARBALIST_CLASS_ID};
    use sim_core::{
        ArenaGeometry, CombatAction, ProjectileCollisionWorld, SimulationVector, TilePoint,
    };

    use super::*;
    use crate::CoreCharacterCombatCompiler;

    fn combat() -> CoreCharacterCombat {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let compiler = CoreCharacterCombatCompiler::load(&root).unwrap();
        let revision = sim_content::load_core_development_oaths_bargains(&root)
            .unwrap()
            .revision_label()
            .to_owned();
        let item_revision = sim_content::load_core_development_items(&root)
            .unwrap()
            .revision_label()
            .to_owned();
        compiler
            .build_from_snapshot(&StoredCoreCombatLoadout {
                character_id: [2; 16],
                selected_character_id: Some([2; 16]),
                class_id: GRAVE_ARBALIST_CLASS_ID.into(),
                level: 5,
                current_health: 120,
                oath_id: None,
                oath_bargain_version: 2,
                active_bargains: vec![StoredCombatBargain {
                    bargain_id: BELL_DEBT_ID.into(),
                    acquisition_ordinal: 1,
                    acquired_by_offer_id: [4; 16],
                    acquiring_offer_content_version: revision,
                }],
                life_state: 0,
                security_state: 0,
                character_state_version: 8,
                progression_version: 3,
                inventory_version: Some(4),
                equipped_weapon: Some(StoredEquippedWeapon {
                    item_uid: [3; 16],
                    template_id: "item.weapon.crossbow.pine_crossbow".into(),
                    content_revision: item_revision,
                    item_level: 1,
                    rarity: 0,
                }),
                belt_slots: [None, None],
            })
            .unwrap()
    }

    fn empty_world() -> ProjectileCollisionWorld {
        let arena = ArenaGeometry {
            id: "arena.core_lifecycle_test".to_owned(),
            width_milli_tiles: 100_000,
            height_milli_tiles: 100_000,
            shell_thickness_milli_tiles: 1_000,
            player_spawn: TilePoint::new(4_000, 12_000),
            boss_spawn: TilePoint::new(80_000, 80_000),
            pillars: vec![],
            anchors: vec![],
        }
        .validated()
        .unwrap();
        ProjectileCollisionWorld::new(&arena, vec![]).unwrap()
    }

    #[test]
    fn reconnect_and_room_handoff_preserve_one_exact_bell_aggregate() {
        let key = CoreLifeKey::new([1; 16], [2; 16], [3; 16]).unwrap();
        let mut directory = CoreLiveDirectory::default();
        directory
            .insert(
                key,
                CoreLiveBindingId::new(10).unwrap(),
                "room.bell_approach",
                combat(),
            )
            .unwrap();
        let world = empty_world();
        for _ in 0..65 {
            directory
                .get_mut(key)
                .unwrap()
                .combat_mut()
                .state
                .step(
                    CombatAction {
                        primary_held: true,
                        primary_press_sequence: 1,
                        ..CombatAction::default()
                    },
                    SimulationVector::new(4.0, 7.0),
                    &world,
                )
                .unwrap();
        }
        let before = directory
            .get(key)
            .unwrap()
            .combat()
            .state
            .export_bell_debt_checkpoint()
            .unwrap();
        assert!(before.has_pending_repeat());

        assert_eq!(
            directory
                .reattach(key, CoreLiveBindingId::new(11).unwrap())
                .unwrap()
                .get(),
            10
        );
        directory
            .handoff_danger_room(key, "room.bell_approach", "room.sepulcher")
            .unwrap();
        let live = directory.get(key).unwrap();
        assert_eq!(live.binding_id().get(), 11);
        assert_eq!(live.room_id(), "room.sepulcher");
        assert_eq!(
            live.combat().state.export_bell_debt_checkpoint().unwrap(),
            before
        );
    }

    #[test]
    fn exact_life_identity_and_source_room_fail_closed() {
        let key = CoreLifeKey::new([1; 16], [2; 16], [3; 16]).unwrap();
        let mut directory = CoreLiveDirectory::default();
        directory
            .insert(
                key,
                CoreLiveBindingId::new(10).unwrap(),
                "room.bell_approach",
                combat(),
            )
            .unwrap();
        for wrong in [
            CoreLifeKey::new([9; 16], [2; 16], [3; 16]).unwrap(),
            CoreLifeKey::new([1; 16], [9; 16], [3; 16]).unwrap(),
            CoreLifeKey::new([1; 16], [2; 16], [9; 16]).unwrap(),
        ] {
            assert_eq!(
                directory
                    .reattach(wrong, CoreLiveBindingId::new(11).unwrap())
                    .unwrap_err(),
                CoreLiveError::NotFound
            );
        }
        assert_eq!(
            directory
                .handoff_danger_room(key, "room.stale", "room.sepulcher")
                .unwrap_err(),
            CoreLiveError::StaleRoom
        );
        assert_eq!(directory.get(key).unwrap().room_id(), "room.bell_approach");
    }
}
