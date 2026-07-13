//! Dormant Core character-life authority for `GB-M03-05F` lifecycle verification.
//!
//! This directory is intentionally not attached to the normal player route. It proves that one
//! account/character/lineage identity owns one mutable combat aggregate across transport and room
//! changes without rebuilding Bargain state from persistence.

use std::{collections::BTreeMap, future::Future};

use persistence::{
    DangerCheckpointDelete, DangerCheckpointWrite, PersistenceError, PostgresPersistence,
    StoredDangerCheckpoint, StoredWorldFlowRevisionV1,
};
use sim_core::{BELL_DEBT_CHECKPOINT_SCHEMA_VERSION, BellDebtCheckpoint, BellDebtResetReason};
use thiserror::Error;

use crate::CoreCharacterCombat;

const ID_BYTES: usize = 16;
const MIN_ROOM_ID_BYTES: usize = 3;
const MAX_ROOM_ID_BYTES: usize = 96;
pub const DANGER_CHECKPOINT_INTERVAL_TICKS: u64 = 30 * 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCheckpointBinding {
    pub content_revision: StoredWorldFlowRevisionV1,
}

impl CoreCheckpointBinding {
    pub fn new(content_revision: StoredWorldFlowRevisionV1) -> Result<Self, CoreLiveError> {
        if !valid_revision(&content_revision) {
            return Err(CoreLiveError::InvalidCheckpointBinding);
        }
        Ok(Self { content_revision })
    }
}

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
    checkpoint_binding: CoreCheckpointBinding,
    checkpoint_dirty: bool,
    last_checkpoint_tick: u64,
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

    #[must_use]
    pub const fn checkpoint_binding(&self) -> &CoreCheckpointBinding {
        &self.checkpoint_binding
    }

    #[must_use]
    pub const fn is_checkpoint_dirty(&self) -> bool {
        self.checkpoint_dirty
    }

    pub fn with_combat_mutation<R>(
        &mut self,
        operation: impl FnOnce(&mut CoreCharacterCombat) -> R,
    ) -> Result<R, CoreLiveError> {
        let before = self
            .combat
            .state
            .export_bell_debt_checkpoint()
            .map_err(|_| CoreLiveError::InvalidCheckpoint)?;
        let result = operation(&mut self.combat);
        let after = self
            .combat
            .state
            .export_bell_debt_checkpoint()
            .map_err(|_| CoreLiveError::InvalidCheckpoint)?;
        self.checkpoint_dirty |= before != after;
        Ok(result)
    }

    pub fn scheduled_checkpoint(&self) -> Result<Option<StoredDangerCheckpoint>, CoreLiveError> {
        let current_tick = self.combat.state.tick().0;
        if !self.checkpoint_dirty
            || current_tick.saturating_sub(self.last_checkpoint_tick)
                < DANGER_CHECKPOINT_INTERVAL_TICKS
        {
            return Ok(None);
        }
        self.prepare_checkpoint().map(Some)
    }

    pub fn lifecycle_checkpoint(&self) -> Result<Option<StoredDangerCheckpoint>, CoreLiveError> {
        if !self.checkpoint_dirty {
            return Ok(None);
        }
        self.prepare_checkpoint().map(Some)
    }

    pub fn confirm_checkpoint(
        &mut self,
        checkpoint: &StoredDangerCheckpoint,
    ) -> Result<(), CoreLiveError> {
        self.validate_stored_checkpoint(checkpoint)?;
        let committed_tick = u64::try_from(checkpoint.checkpoint_tick)
            .map_err(|_| CoreLiveError::InvalidCheckpoint)?;
        self.last_checkpoint_tick = committed_tick;
        let current = self
            .combat
            .state
            .export_bell_debt_checkpoint()
            .map_err(|_| CoreLiveError::InvalidCheckpoint)?;
        self.checkpoint_dirty = current
            .canonical_digest()
            .map_err(|_| CoreLiveError::InvalidCheckpoint)?
            != checkpoint.checkpoint_payload_digest;
        Ok(())
    }

    pub fn restore_checkpoint(
        &mut self,
        checkpoint: &StoredDangerCheckpoint,
    ) -> Result<(), CoreLiveError> {
        if self.combat.state.tick().0 != 0 || self.checkpoint_dirty {
            return Err(CoreLiveError::ResumeRequiresFreshAggregate);
        }
        self.validate_stored_checkpoint(checkpoint)?;
        let decoded = BellDebtCheckpoint::decode_canonical(&checkpoint.checkpoint_payload)
            .map_err(|_| CoreLiveError::InvalidCheckpoint)?;
        self.combat
            .state
            .import_bell_debt_checkpoint(&decoded)
            .map_err(|_| CoreLiveError::InvalidCheckpoint)?;
        self.last_checkpoint_tick = 0;
        self.checkpoint_dirty = false;
        Ok(())
    }

    fn prepare_checkpoint(&self) -> Result<StoredDangerCheckpoint, CoreLiveError> {
        let bell = self
            .combat
            .state
            .export_bell_debt_checkpoint()
            .map_err(|_| CoreLiveError::InvalidCheckpoint)?;
        let checkpoint_payload = bell
            .canonical_bytes()
            .map_err(|_| CoreLiveError::InvalidCheckpoint)?;
        let checkpoint_payload_digest = *blake3::hash(&checkpoint_payload).as_bytes();
        let checkpoint_tick = i64::try_from(self.combat.state.tick().0)
            .map_err(|_| CoreLiveError::CheckpointTickExhausted)?;
        let character_version = version_i64(self.combat.character_state_version)?;
        let progression_version = version_i64(self.combat.progression_version)?;
        let inventory_version = version_i64(self.combat.inventory_version)?;
        let oath_bargain_version = version_i64(self.combat.oath_bargain_version)?;
        let checkpoint_schema_version = i16::try_from(BELL_DEBT_CHECKPOINT_SCHEMA_VERSION)
            .map_err(|_| CoreLiveError::InvalidCheckpoint)?;
        let mut checkpoint = StoredDangerCheckpoint {
            account_id: self.key.account_id,
            character_id: self.key.character_id,
            lineage_id: self.key.lineage_id,
            checkpoint_tick,
            content_revision: self.checkpoint_binding.content_revision.clone(),
            composite_digest: [0; 32],
            character_version,
            progression_version,
            inventory_version,
            oath_bargain_version,
            checkpoint_schema_version,
            checkpoint_payload,
            checkpoint_payload_digest,
        };
        checkpoint.composite_digest = composite_digest(&checkpoint);
        Ok(checkpoint)
    }

    fn complete_safe_transfer(&mut self) {
        self.combat
            .state
            .reset_bell_debt(BellDebtResetReason::SafeTransfer);
        self.checkpoint_dirty = false;
        self.last_checkpoint_tick = 0;
    }

    fn validate_stored_checkpoint(
        &self,
        checkpoint: &StoredDangerCheckpoint,
    ) -> Result<(), CoreLiveError> {
        let expected = self.prepare_binding_shape(checkpoint.checkpoint_tick)?;
        if checkpoint.account_id != expected.account_id
            || checkpoint.character_id != expected.character_id
            || checkpoint.lineage_id != expected.lineage_id
            || checkpoint.content_revision != expected.content_revision
            || checkpoint.character_version != expected.character_version
            || checkpoint.progression_version != expected.progression_version
            || checkpoint.inventory_version != expected.inventory_version
            || checkpoint.oath_bargain_version != expected.oath_bargain_version
            || checkpoint.checkpoint_schema_version != expected.checkpoint_schema_version
            || checkpoint.checkpoint_payload.is_empty()
            || *blake3::hash(&checkpoint.checkpoint_payload).as_bytes()
                != checkpoint.checkpoint_payload_digest
            || composite_digest(checkpoint) != checkpoint.composite_digest
        {
            return Err(CoreLiveError::InvalidCheckpoint);
        }
        Ok(())
    }

    fn prepare_binding_shape(
        &self,
        checkpoint_tick: i64,
    ) -> Result<StoredDangerCheckpoint, CoreLiveError> {
        if checkpoint_tick < 0 {
            return Err(CoreLiveError::InvalidCheckpoint);
        }
        Ok(StoredDangerCheckpoint {
            account_id: self.key.account_id,
            character_id: self.key.character_id,
            lineage_id: self.key.lineage_id,
            checkpoint_tick,
            content_revision: self.checkpoint_binding.content_revision.clone(),
            composite_digest: [0; 32],
            character_version: version_i64(self.combat.character_state_version)?,
            progression_version: version_i64(self.combat.progression_version)?,
            inventory_version: version_i64(self.combat.inventory_version)?,
            oath_bargain_version: version_i64(self.combat.oath_bargain_version)?,
            checkpoint_schema_version: i16::try_from(BELL_DEBT_CHECKPOINT_SCHEMA_VERSION)
                .map_err(|_| CoreLiveError::InvalidCheckpoint)?,
            checkpoint_payload: Vec::new(),
            checkpoint_payload_digest: [0; 32],
        })
    }
}

#[derive(Debug, Default)]
pub struct CoreLiveDirectory {
    lives: BTreeMap<CoreLifeKey, CoreLiveCharacter>,
}

pub trait CoreDangerCheckpointRepository: Send + Sync {
    fn write_checkpoint<'a>(
        &'a self,
        checkpoint: &'a StoredDangerCheckpoint,
    ) -> impl Future<Output = Result<DangerCheckpointWrite, PersistenceError>> + Send + 'a;

    fn load_checkpoint(
        &self,
        account_id: [u8; ID_BYTES],
        character_id: [u8; ID_BYTES],
    ) -> impl Future<Output = Result<Option<StoredDangerCheckpoint>, PersistenceError>> + Send;

    fn delete_after_safe_transfer(
        &self,
        account_id: [u8; ID_BYTES],
        character_id: [u8; ID_BYTES],
        lineage_id: [u8; ID_BYTES],
    ) -> impl Future<Output = Result<DangerCheckpointDelete, PersistenceError>> + Send;
}

impl CoreDangerCheckpointRepository for PostgresPersistence {
    async fn write_checkpoint(
        &self,
        checkpoint: &StoredDangerCheckpoint,
    ) -> Result<DangerCheckpointWrite, PersistenceError> {
        self.write_danger_checkpoint(checkpoint).await
    }

    async fn load_checkpoint(
        &self,
        account_id: [u8; ID_BYTES],
        character_id: [u8; ID_BYTES],
    ) -> Result<Option<StoredDangerCheckpoint>, PersistenceError> {
        self.danger_checkpoint(account_id, character_id).await
    }

    async fn delete_after_safe_transfer(
        &self,
        account_id: [u8; ID_BYTES],
        character_id: [u8; ID_BYTES],
        lineage_id: [u8; ID_BYTES],
    ) -> Result<DangerCheckpointDelete, PersistenceError> {
        self.delete_danger_checkpoint_after_safe_transfer(account_id, character_id, lineage_id)
            .await
    }
}

#[derive(Debug)]
pub struct CoreDangerCheckpointService<R> {
    repository: R,
}

impl<R> CoreDangerCheckpointService<R>
where
    R: CoreDangerCheckpointRepository,
{
    #[must_use]
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }

    #[must_use]
    pub const fn repository(&self) -> &R {
        &self.repository
    }

    pub async fn flush_scheduled(
        &self,
        live: &mut CoreLiveCharacter,
    ) -> Result<Option<DangerCheckpointWrite>, CoreCheckpointServiceError> {
        let Some(checkpoint) = live.scheduled_checkpoint()? else {
            return Ok(None);
        };
        self.persist_and_confirm(live, checkpoint).await.map(Some)
    }

    pub async fn flush_lifecycle_boundary(
        &self,
        live: &mut CoreLiveCharacter,
    ) -> Result<Option<DangerCheckpointWrite>, CoreCheckpointServiceError> {
        let Some(checkpoint) = live.lifecycle_checkpoint()? else {
            return Ok(None);
        };
        self.persist_and_confirm(live, checkpoint).await.map(Some)
    }

    pub async fn resume_latest(
        &self,
        live: &mut CoreLiveCharacter,
    ) -> Result<CoreResumeOutcome, CoreCheckpointServiceError> {
        let key = live.key();
        let Some(checkpoint) = self
            .repository
            .load_checkpoint(key.account_id, key.character_id)
            .await?
        else {
            return Ok(CoreResumeOutcome::NoCheckpoint);
        };
        live.restore_checkpoint(&checkpoint)?;
        Ok(CoreResumeOutcome::Restored {
            checkpoint_tick: u64::try_from(checkpoint.checkpoint_tick)
                .map_err(|_| CoreLiveError::InvalidCheckpoint)?,
        })
    }

    pub async fn finalize_safe_transfer(
        &self,
        live: &mut CoreLiveCharacter,
    ) -> Result<DangerCheckpointDelete, CoreCheckpointServiceError> {
        let key = live.key();
        let outcome = self
            .repository
            .delete_after_safe_transfer(key.account_id, key.character_id, key.lineage_id)
            .await?;
        live.complete_safe_transfer();
        Ok(outcome)
    }

    async fn persist_and_confirm(
        &self,
        live: &mut CoreLiveCharacter,
        checkpoint: StoredDangerCheckpoint,
    ) -> Result<DangerCheckpointWrite, CoreCheckpointServiceError> {
        let outcome = self.repository.write_checkpoint(&checkpoint).await?;
        live.confirm_checkpoint(&checkpoint)?;
        Ok(outcome)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreResumeOutcome {
    NoCheckpoint,
    Restored { checkpoint_tick: u64 },
}

#[derive(Debug, Error)]
pub enum CoreCheckpointServiceError {
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(transparent)]
    Live(#[from] CoreLiveError),
}

impl CoreLiveDirectory {
    pub fn insert(
        &mut self,
        key: CoreLifeKey,
        binding_id: CoreLiveBindingId,
        room_id: impl Into<String>,
        checkpoint_binding: CoreCheckpointBinding,
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
                checkpoint_binding,
                checkpoint_dirty: false,
                last_checkpoint_tick: 0,
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
    #[error("Core checkpoint content binding is invalid")]
    InvalidCheckpointBinding,
    #[error("Core danger checkpoint failed exact validation")]
    InvalidCheckpoint,
    #[error("Core checkpoint simulation tick exceeded the durable range")]
    CheckpointTickExhausted,
    #[error("process resume requires a fresh unmodified Core aggregate")]
    ResumeRequiresFreshAggregate,
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

fn valid_revision(revision: &StoredWorldFlowRevisionV1) -> bool {
    [
        &revision.records_blake3,
        &revision.assets_blake3,
        &revision.localization_blake3,
    ]
    .iter()
    .all(|value| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn version_i64(value: u64) -> Result<i64, CoreLiveError> {
    i64::try_from(value).map_err(|_| CoreLiveError::InvalidCheckpoint)
}

fn composite_digest(checkpoint: &StoredDangerCheckpoint) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"gravebound.danger_checkpoint.bell.v1\0");
    hasher.update(&checkpoint.account_id);
    hasher.update(&checkpoint.character_id);
    hasher.update(&checkpoint.lineage_id);
    hasher.update(&checkpoint.checkpoint_tick.to_le_bytes());
    for hash in [
        &checkpoint.content_revision.records_blake3,
        &checkpoint.content_revision.assets_blake3,
        &checkpoint.content_revision.localization_blake3,
    ] {
        hasher.update(&(hash.len() as u64).to_le_bytes());
        hasher.update(hash.as_bytes());
    }
    hasher.update(&checkpoint.character_version.to_le_bytes());
    hasher.update(&checkpoint.progression_version.to_le_bytes());
    hasher.update(&checkpoint.inventory_version.to_le_bytes());
    hasher.update(&checkpoint.oath_bargain_version.to_le_bytes());
    hasher.update(&checkpoint.checkpoint_schema_version.to_le_bytes());
    hasher.update(&(checkpoint.checkpoint_payload.len() as u64).to_le_bytes());
    hasher.update(&checkpoint.checkpoint_payload);
    hasher.update(&checkpoint.checkpoint_payload_digest);
    *hasher.finalize().as_bytes()
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Mutex};

    use persistence::{StoredCombatBargain, StoredCoreCombatLoadout, StoredEquippedWeapon};
    use protocol::{BELL_DEBT_ID, GRAVE_ARBALIST_CLASS_ID};
    use sim_core::{
        ArenaGeometry, CombatAction, ProjectileCollisionWorld, SimulationVector, TilePoint,
    };

    use super::*;
    use crate::CoreCharacterCombatCompiler;

    #[derive(Debug, Default)]
    struct MemoryCheckpointRepository {
        stored: Mutex<Option<StoredDangerCheckpoint>>,
        writes: Mutex<u32>,
        safe_transfer_committed: Mutex<bool>,
    }

    impl CoreDangerCheckpointRepository for MemoryCheckpointRepository {
        async fn write_checkpoint(
            &self,
            checkpoint: &StoredDangerCheckpoint,
        ) -> Result<DangerCheckpointWrite, PersistenceError> {
            let mut stored = self.stored.lock().unwrap();
            let outcome = if stored.is_some() {
                DangerCheckpointWrite::Advanced
            } else {
                DangerCheckpointWrite::Created
            };
            *stored = Some(checkpoint.clone());
            *self.writes.lock().unwrap() += 1;
            Ok(outcome)
        }

        async fn load_checkpoint(
            &self,
            _account_id: [u8; ID_BYTES],
            _character_id: [u8; ID_BYTES],
        ) -> Result<Option<StoredDangerCheckpoint>, PersistenceError> {
            Ok(self.stored.lock().unwrap().clone())
        }

        async fn delete_after_safe_transfer(
            &self,
            _account_id: [u8; ID_BYTES],
            _character_id: [u8; ID_BYTES],
            _lineage_id: [u8; ID_BYTES],
        ) -> Result<DangerCheckpointDelete, PersistenceError> {
            if !*self.safe_transfer_committed.lock().unwrap() {
                return Err(PersistenceError::DangerCheckpointFinalizationNotCommitted);
            }
            Ok(if self.stored.lock().unwrap().take().is_some() {
                DangerCheckpointDelete::Deleted
            } else {
                DangerCheckpointDelete::Absent
            })
        }
    }

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

    fn checkpoint_binding() -> CoreCheckpointBinding {
        CoreCheckpointBinding::new(StoredWorldFlowRevisionV1 {
            records_blake3: "1".repeat(64),
            assets_blake3: "2".repeat(64),
            localization_blake3: "3".repeat(64),
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
                checkpoint_binding(),
                combat(),
            )
            .unwrap();
        let world = empty_world();
        for _ in 0..65 {
            directory
                .get_mut(key)
                .unwrap()
                .with_combat_mutation(|combat| {
                    combat.state.step(
                        CombatAction {
                            primary_held: true,
                            primary_press_sequence: 1,
                            ..CombatAction::default()
                        },
                        SimulationVector::new(4.0, 7.0),
                        &world,
                    )
                })
                .unwrap()
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
                checkpoint_binding(),
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

    #[test]
    fn clean_intervals_skip_writes_and_dirty_state_coalesces_at_exact_cadence() {
        let key = CoreLifeKey::new([1; 16], [2; 16], [3; 16]).unwrap();
        let mut directory = CoreLiveDirectory::default();
        directory
            .insert(
                key,
                CoreLiveBindingId::new(10).unwrap(),
                "room.bell_approach",
                checkpoint_binding(),
                combat(),
            )
            .unwrap();
        let world = empty_world();
        for _ in 0..DANGER_CHECKPOINT_INTERVAL_TICKS {
            directory
                .get_mut(key)
                .unwrap()
                .with_combat_mutation(|combat| {
                    combat.state.step(
                        CombatAction::default(),
                        SimulationVector::new(4.0, 7.0),
                        &world,
                    )
                })
                .unwrap()
                .unwrap();
        }
        assert!(!directory.get(key).unwrap().is_checkpoint_dirty());
        assert!(
            directory
                .get(key)
                .unwrap()
                .scheduled_checkpoint()
                .unwrap()
                .is_none()
        );

        directory
            .get_mut(key)
            .unwrap()
            .with_combat_mutation(|combat| {
                combat.state.step(
                    CombatAction {
                        primary_held: true,
                        primary_press_sequence: 1,
                        ..CombatAction::default()
                    },
                    SimulationVector::new(4.0, 7.0),
                    &world,
                )
            })
            .unwrap()
            .unwrap();
        let checkpoint = directory
            .get(key)
            .unwrap()
            .scheduled_checkpoint()
            .unwrap()
            .expect("dirty state is due after the first cadence");
        assert_eq!(checkpoint.checkpoint_tick, 901);
        assert_eq!(checkpoint.checkpoint_schema_version, 1);
        assert_ne!(checkpoint.composite_digest, [0; 32]);
        directory
            .get_mut(key)
            .unwrap()
            .confirm_checkpoint(&checkpoint)
            .unwrap();
        assert!(!directory.get(key).unwrap().is_checkpoint_dirty());
        assert!(
            directory
                .get(key)
                .unwrap()
                .scheduled_checkpoint()
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn process_resume_restores_exact_pending_delay_and_rejects_binding_drift() {
        let key = CoreLifeKey::new([1; 16], [2; 16], [3; 16]).unwrap();
        let mut source = CoreLiveDirectory::default();
        source
            .insert(
                key,
                CoreLiveBindingId::new(10).unwrap(),
                "room.bell_approach",
                checkpoint_binding(),
                combat(),
            )
            .unwrap();
        let world = empty_world();
        for _ in 0..65 {
            source
                .get_mut(key)
                .unwrap()
                .with_combat_mutation(|combat| {
                    combat.state.step(
                        CombatAction {
                            primary_held: true,
                            primary_press_sequence: 1,
                            ..CombatAction::default()
                        },
                        SimulationVector::new(4.0, 7.0),
                        &world,
                    )
                })
                .unwrap()
                .unwrap();
        }
        let expected = source
            .get(key)
            .unwrap()
            .combat()
            .state
            .export_bell_debt_checkpoint()
            .unwrap();
        assert!(expected.has_pending_repeat());
        let checkpoint = source
            .get(key)
            .unwrap()
            .lifecycle_checkpoint()
            .unwrap()
            .unwrap();

        let mut resumed = CoreLiveDirectory::default();
        resumed
            .insert(
                key,
                CoreLiveBindingId::new(20).unwrap(),
                "room.bell_approach",
                checkpoint_binding(),
                combat(),
            )
            .unwrap();
        resumed
            .get_mut(key)
            .unwrap()
            .restore_checkpoint(&checkpoint)
            .unwrap();
        assert_eq!(
            resumed
                .get(key)
                .unwrap()
                .combat()
                .state
                .export_bell_debt_checkpoint()
                .unwrap(),
            expected
        );

        let mut wrong_lineage = checkpoint.clone();
        wrong_lineage.lineage_id = [9; 16];
        assert_eq!(
            CoreLiveCharacter {
                key,
                binding_id: CoreLiveBindingId::new(30).unwrap(),
                room_id: "room.bell_approach".into(),
                checkpoint_binding: checkpoint_binding(),
                checkpoint_dirty: false,
                last_checkpoint_tick: 0,
                combat: combat(),
            }
            .restore_checkpoint(&wrong_lineage)
            .unwrap_err(),
            CoreLiveError::InvalidCheckpoint
        );
    }

    #[tokio::test]
    async fn coordinator_skips_clean_state_flushes_dirty_boundary_and_resumes_latest() {
        let key = CoreLifeKey::new([1; 16], [2; 16], [3; 16]).unwrap();
        let mut directory = CoreLiveDirectory::default();
        directory
            .insert(
                key,
                CoreLiveBindingId::new(10).unwrap(),
                "room.bell_approach",
                checkpoint_binding(),
                combat(),
            )
            .unwrap();
        let service = CoreDangerCheckpointService::new(MemoryCheckpointRepository::default());
        assert_eq!(
            service
                .flush_scheduled(directory.get_mut(key).unwrap())
                .await
                .unwrap(),
            None
        );
        assert_eq!(*service.repository().writes.lock().unwrap(), 0);

        let world = empty_world();
        directory
            .get_mut(key)
            .unwrap()
            .with_combat_mutation(|combat| {
                combat.state.step(
                    CombatAction {
                        primary_held: true,
                        primary_press_sequence: 1,
                        ..CombatAction::default()
                    },
                    SimulationVector::new(4.0, 7.0),
                    &world,
                )
            })
            .unwrap()
            .unwrap();
        assert_eq!(
            service
                .flush_scheduled(directory.get_mut(key).unwrap())
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            service
                .flush_lifecycle_boundary(directory.get_mut(key).unwrap())
                .await
                .unwrap(),
            Some(DangerCheckpointWrite::Created)
        );
        assert_eq!(*service.repository().writes.lock().unwrap(), 1);

        let expected = directory
            .get(key)
            .unwrap()
            .combat()
            .state
            .export_bell_debt_checkpoint()
            .unwrap();
        let mut resumed = CoreLiveDirectory::default();
        resumed
            .insert(
                key,
                CoreLiveBindingId::new(20).unwrap(),
                "room.bell_approach",
                checkpoint_binding(),
                combat(),
            )
            .unwrap();
        assert_eq!(
            service
                .resume_latest(resumed.get_mut(key).unwrap())
                .await
                .unwrap(),
            CoreResumeOutcome::Restored { checkpoint_tick: 1 }
        );
        assert_eq!(
            resumed
                .get(key)
                .unwrap()
                .combat()
                .state
                .export_bell_debt_checkpoint()
                .unwrap(),
            expected
        );
    }

    #[tokio::test]
    async fn safe_transfer_resets_only_after_durable_cleanup_and_replays_idempotently() {
        let key = CoreLifeKey::new([1; 16], [2; 16], [3; 16]).unwrap();
        let mut directory = CoreLiveDirectory::default();
        directory
            .insert(
                key,
                CoreLiveBindingId::new(10).unwrap(),
                "room.sepulcher",
                checkpoint_binding(),
                combat(),
            )
            .unwrap();
        let world = empty_world();
        directory
            .get_mut(key)
            .unwrap()
            .with_combat_mutation(|combat| {
                combat.state.step(
                    CombatAction {
                        primary_held: true,
                        primary_press_sequence: 1,
                        ..CombatAction::default()
                    },
                    SimulationVector::new(4.0, 7.0),
                    &world,
                )
            })
            .unwrap()
            .unwrap();
        let service = CoreDangerCheckpointService::new(MemoryCheckpointRepository::default());
        service
            .flush_lifecycle_boundary(directory.get_mut(key).unwrap())
            .await
            .unwrap();
        let before = directory
            .get(key)
            .unwrap()
            .combat()
            .state
            .export_bell_debt_checkpoint()
            .unwrap();
        assert_ne!(before.primary_release_count(), 0);

        assert!(matches!(
            service
                .finalize_safe_transfer(directory.get_mut(key).unwrap())
                .await,
            Err(CoreCheckpointServiceError::Persistence(
                PersistenceError::DangerCheckpointFinalizationNotCommitted
            ))
        ));
        assert_eq!(
            directory
                .get(key)
                .unwrap()
                .combat()
                .state
                .export_bell_debt_checkpoint()
                .unwrap(),
            before
        );

        *service.repository().safe_transfer_committed.lock().unwrap() = true;
        assert_eq!(
            service
                .finalize_safe_transfer(directory.get_mut(key).unwrap())
                .await
                .unwrap(),
            DangerCheckpointDelete::Deleted
        );
        let reset = directory
            .get(key)
            .unwrap()
            .combat()
            .state
            .export_bell_debt_checkpoint()
            .unwrap();
        assert_eq!(reset.primary_release_count(), 0);
        assert!(!reset.has_pending_repeat());
        assert_eq!(
            service
                .finalize_safe_transfer(directory.get_mut(key).unwrap())
                .await
                .unwrap(),
            DangerCheckpointDelete::Absent
        );
    }
}
