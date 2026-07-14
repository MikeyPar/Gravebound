use std::collections::BTreeMap;

use persistence::{
    PersistenceError, PostgresPersistence, StoredSafeInventoryCommand,
    StoredSafeInventoryCommandKind, StoredSafeInventoryItem, StoredSafeInventoryLocation,
    StoredSafeInventoryPlacement, StoredSafeInventoryResult, StoredSafeInventorySnapshot,
};
use sim_core::{
    CHARACTER_SAFE_CAPACITY, DurableStorageSlot, ItemUid, RUN_BACKPACK_CAPACITY,
    SafeStorageCommand, SafeStorageError, SafeStorageLocation, SafeStorageSnapshot, VAULT_CAPACITY,
    plan_safe_storage_transfer,
};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeInventoryTransferKind {
    CharacterSafeToVault,
    VaultToCharacterSafe,
    CharacterSafeToRunBackpack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SafeInventoryTransferCommand {
    pub mutation_id: [u8; 16],
    pub kind: SafeInventoryTransferKind,
    pub source_slot_index: u16,
    pub expected_account_version: u64,
    pub expected_inventory_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoritativeSafeInventoryTransfer {
    pub result: StoredSafeInventoryResult,
}

#[derive(Debug, Clone)]
pub struct PostgresSafeInventoryService {
    persistence: PostgresPersistence,
}

impl PostgresSafeInventoryService {
    #[must_use]
    pub const fn new(persistence: PostgresPersistence) -> Self {
        Self { persistence }
    }

    pub async fn transfer(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
        command: SafeInventoryTransferCommand,
    ) -> Result<AuthoritativeSafeInventoryTransfer, SafeInventoryServiceError> {
        validate_command(command)?;
        let request_hash = request_hash(account_id, character_id, command);
        if let Some(result) = self
            .persistence
            .load_safe_inventory_replay(account_id, character_id, command.mutation_id, request_hash)
            .await
            .map_err(|error| map_persistence(&error))?
        {
            return Ok(AuthoritativeSafeInventoryTransfer { result });
        }
        let stored = self
            .persistence
            .load_safe_inventory_snapshot(account_id, character_id)
            .await
            .map_err(|error| map_persistence(&error))?;
        if stored.account_version != command.expected_account_version
            || stored.inventory_version != command.expected_inventory_version
        {
            return Err(SafeInventoryServiceError::StaleVersion);
        }
        let (snapshot, versions) = project_snapshot(&stored)?;
        let planned = plan_safe_storage_transfer(&snapshot, simulation_command(command))
            .map_err(|error| map_plan(&error))?;
        let placements = planned
            .placements
            .iter()
            .map(|placement| {
                let item_uid = placement.item_uid.bytes();
                let expected_item_version = versions
                    .get(&item_uid)
                    .copied()
                    .ok_or(SafeInventoryServiceError::CorruptSnapshot)?;
                Ok(StoredSafeInventoryPlacement {
                    item_uid,
                    source: stored_location(placement.source),
                    destination: stored_location(placement.destination),
                    expected_item_version,
                })
            })
            .collect::<Result<Vec<_>, SafeInventoryServiceError>>()?;
        let result_hash = result_hash(request_hash, &placements);
        let stored_command = StoredSafeInventoryCommand {
            mutation_id: command.mutation_id,
            canonical_request_hash: request_hash,
            result_hash,
            kind: stored_kind(command.kind),
            source_slot_index: command.source_slot_index,
            expected_account_version: command.expected_account_version,
            expected_inventory_version: command.expected_inventory_version,
            placements,
        };
        let result = self
            .persistence
            .commit_safe_inventory_transfer(account_id, character_id, &stored_command)
            .await
            .map_err(|error| map_persistence(&error))?;
        Ok(AuthoritativeSafeInventoryTransfer { result })
    }
}

#[derive(Debug, Error)]
pub enum SafeInventoryServiceError {
    #[error("safe-inventory command is malformed")]
    InvalidCommand,
    #[error("safe-inventory aggregate version is stale")]
    StaleVersion,
    #[error("safe-inventory operation requires the selected living character in Lantern Halls")]
    HallBinding,
    #[error("safe-inventory operation is blocked by unresolved inventory state")]
    UnresolvedMutation,
    #[error("safe-inventory mutation identity conflicts with a prior request")]
    IdempotencyConflict,
    #[error("safe-inventory source or placement changed")]
    BindingMismatch,
    #[error("safe-inventory destination lacks capacity")]
    StorageFull,
    #[error("safe-inventory snapshot is corrupt")]
    CorruptSnapshot,
    #[error("safe-inventory persistence failed")]
    Persistence,
}

fn validate_command(
    command: SafeInventoryTransferCommand,
) -> Result<(), SafeInventoryServiceError> {
    let source_valid = match command.kind {
        SafeInventoryTransferKind::CharacterSafeToVault
        | SafeInventoryTransferKind::CharacterSafeToRunBackpack => {
            usize::from(command.source_slot_index) < CHARACTER_SAFE_CAPACITY
        }
        SafeInventoryTransferKind::VaultToCharacterSafe => {
            usize::from(command.source_slot_index) < VAULT_CAPACITY
        }
    };
    if command.mutation_id == [0; 16]
        || command.expected_account_version == 0
        || command.expected_inventory_version == 0
        || !source_valid
    {
        return Err(SafeInventoryServiceError::InvalidCommand);
    }
    Ok(())
}

fn simulation_command(command: SafeInventoryTransferCommand) -> SafeStorageCommand {
    match command.kind {
        SafeInventoryTransferKind::CharacterSafeToVault => {
            SafeStorageCommand::CharacterSafeToVault {
                source_slot: u8::try_from(command.source_slot_index)
                    .expect("validated CharacterSafe slot fits u8"),
            }
        }
        SafeInventoryTransferKind::VaultToCharacterSafe => {
            SafeStorageCommand::VaultToCharacterSafe {
                source_slot: command.source_slot_index,
            }
        }
        SafeInventoryTransferKind::CharacterSafeToRunBackpack => {
            SafeStorageCommand::CharacterSafeToRunBackpack {
                source_slot: u8::try_from(command.source_slot_index)
                    .expect("validated CharacterSafe slot fits u8"),
            }
        }
    }
}

fn stored_kind(kind: SafeInventoryTransferKind) -> StoredSafeInventoryCommandKind {
    match kind {
        SafeInventoryTransferKind::CharacterSafeToVault => {
            StoredSafeInventoryCommandKind::CharacterSafeToVault
        }
        SafeInventoryTransferKind::VaultToCharacterSafe => {
            StoredSafeInventoryCommandKind::VaultToCharacterSafe
        }
        SafeInventoryTransferKind::CharacterSafeToRunBackpack => {
            StoredSafeInventoryCommandKind::CharacterSafeToRunBackpack
        }
    }
}

fn stored_location(location: SafeStorageLocation) -> StoredSafeInventoryLocation {
    match location {
        SafeStorageLocation::RunBackpack(slot) => StoredSafeInventoryLocation::RunBackpack(slot),
        SafeStorageLocation::CharacterSafe(slot) => {
            StoredSafeInventoryLocation::CharacterSafe(slot)
        }
        SafeStorageLocation::Vault(slot) => StoredSafeInventoryLocation::Vault(slot),
    }
}

fn project_snapshot(
    stored: &StoredSafeInventorySnapshot,
) -> Result<(SafeStorageSnapshot, BTreeMap<[u8; 16], u64>), SafeInventoryServiceError> {
    let mut character_safe = vec![DurableStorageSlot::Empty; CHARACTER_SAFE_CAPACITY];
    let mut vault = vec![DurableStorageSlot::Empty; VAULT_CAPACITY];
    let mut run_backpack = vec![DurableStorageSlot::Empty; RUN_BACKPACK_CAPACITY];
    let mut versions = BTreeMap::new();
    for item in stored
        .character_safe
        .iter()
        .chain(&stored.vault)
        .chain(&stored.run_backpack)
    {
        if versions.insert(item.item_uid, item.item_version).is_some() {
            return Err(SafeInventoryServiceError::CorruptSnapshot);
        }
        let (slots, index) = match item.location {
            StoredSafeInventoryLocation::CharacterSafe(slot) => {
                (&mut character_safe, usize::from(slot))
            }
            StoredSafeInventoryLocation::Vault(slot) => (&mut vault, usize::from(slot)),
            StoredSafeInventoryLocation::RunBackpack(slot) => {
                (&mut run_backpack, usize::from(slot))
            }
        };
        project_item(slots, index, item)?;
    }
    Ok((
        SafeStorageSnapshot {
            account_version: stored.account_version,
            inventory_version: stored.inventory_version,
            character_safe,
            vault,
            run_backpack,
        },
        versions,
    ))
}

fn project_item(
    slots: &mut [DurableStorageSlot],
    index: usize,
    item: &StoredSafeInventoryItem,
) -> Result<(), SafeInventoryServiceError> {
    let slot = slots
        .get_mut(index)
        .ok_or(SafeInventoryServiceError::CorruptSnapshot)?;
    let item_uid =
        ItemUid::new(item.item_uid).map_err(|_| SafeInventoryServiceError::CorruptSnapshot)?;
    match item.item_kind {
        0 if matches!(slot, DurableStorageSlot::Empty) => {
            *slot = DurableStorageSlot::Equipment { item_uid };
        }
        1 => match slot {
            DurableStorageSlot::Empty => {
                *slot = DurableStorageSlot::Consumable {
                    template_id: item.template_id.clone(),
                    item_uids: vec![item_uid],
                };
            }
            DurableStorageSlot::Consumable {
                template_id,
                item_uids,
            } if template_id == &item.template_id => item_uids.push(item_uid),
            _ => return Err(SafeInventoryServiceError::CorruptSnapshot),
        },
        _ => return Err(SafeInventoryServiceError::CorruptSnapshot),
    }
    Ok(())
}

fn request_hash(
    account_id: [u8; 16],
    character_id: [u8; 16],
    command: SafeInventoryTransferCommand,
) -> [u8; 32] {
    let mut material = Vec::with_capacity(75);
    material.extend_from_slice(&account_id);
    material.extend_from_slice(&character_id);
    material.extend_from_slice(&command.mutation_id);
    material.push(match command.kind {
        SafeInventoryTransferKind::CharacterSafeToVault => 0,
        SafeInventoryTransferKind::VaultToCharacterSafe => 1,
        SafeInventoryTransferKind::CharacterSafeToRunBackpack => 2,
    });
    material.extend_from_slice(&command.source_slot_index.to_le_bytes());
    material.extend_from_slice(&command.expected_account_version.to_le_bytes());
    material.extend_from_slice(&command.expected_inventory_version.to_le_bytes());
    blake3::derive_key("gravebound.safe-inventory-request.v1", &material)
}

fn result_hash(request_hash: [u8; 32], placements: &[StoredSafeInventoryPlacement]) -> [u8; 32] {
    let mut material = Vec::with_capacity(32 + placements.len() * 43);
    material.extend_from_slice(&request_hash);
    for placement in placements {
        material.extend_from_slice(&placement.item_uid);
        let (kind, slot) = match placement.destination {
            StoredSafeInventoryLocation::RunBackpack(slot) => (2_u8, u16::from(slot)),
            StoredSafeInventoryLocation::CharacterSafe(slot) => (5, u16::from(slot)),
            StoredSafeInventoryLocation::Vault(slot) => (6, slot),
        };
        material.push(kind);
        material.extend_from_slice(&slot.to_le_bytes());
        material.extend_from_slice(&placement.expected_item_version.to_le_bytes());
    }
    blake3::derive_key("gravebound.safe-inventory-result.v1", &material)
}

fn map_persistence(error: &PersistenceError) -> SafeInventoryServiceError {
    match error {
        PersistenceError::SafeInventoryVersionMismatch => SafeInventoryServiceError::StaleVersion,
        PersistenceError::SafeInventoryHallBindingMismatch
        | PersistenceError::SafeInventoryAccountNotFound => SafeInventoryServiceError::HallBinding,
        PersistenceError::SafeInventoryUnresolvedMutation => {
            SafeInventoryServiceError::UnresolvedMutation
        }
        PersistenceError::SafeInventoryIdempotencyConflict => {
            SafeInventoryServiceError::IdempotencyConflict
        }
        PersistenceError::SafeInventoryBindingMismatch => {
            SafeInventoryServiceError::BindingMismatch
        }
        PersistenceError::SafeInventoryStorageFull => SafeInventoryServiceError::StorageFull,
        PersistenceError::CorruptStoredSafeInventory => SafeInventoryServiceError::CorruptSnapshot,
        _ => SafeInventoryServiceError::Persistence,
    }
}

const fn map_plan(error: &SafeStorageError) -> SafeInventoryServiceError {
    match error {
        SafeStorageError::StorageFull
        | SafeStorageError::CharacterSafeFull
        | SafeStorageError::RunBackpackFull => SafeInventoryServiceError::StorageFull,
        SafeStorageError::SourceOutOfRange | SafeStorageError::EmptySource => {
            SafeInventoryServiceError::BindingMismatch
        }
        SafeStorageError::InvalidVersion | SafeStorageError::VersionOverflow => {
            SafeInventoryServiceError::StaleVersion
        }
        SafeStorageError::InvalidCharacterSafeCapacity
        | SafeStorageError::InvalidVaultCapacity
        | SafeStorageError::InvalidRunBackpackCapacity
        | SafeStorageError::InvalidConsumableStack
        | SafeStorageError::DuplicateItemUid => SafeInventoryServiceError::CorruptSnapshot,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(
        byte: u8,
        template: &str,
        kind: i16,
        location: StoredSafeInventoryLocation,
    ) -> StoredSafeInventoryItem {
        StoredSafeInventoryItem {
            item_uid: [byte; 16],
            template_id: template.to_owned(),
            item_kind: kind,
            item_version: u64::from(byte),
            security_state: if matches!(location, StoredSafeInventoryLocation::RunBackpack(_)) {
                2
            } else {
                0
            },
            location,
        }
    }

    #[test]
    fn projection_preserves_exact_slots_units_and_versions() {
        let stored = StoredSafeInventorySnapshot {
            account_version: 4,
            inventory_version: 7,
            character_safe: vec![
                item(
                    1,
                    "consumable.red",
                    1,
                    StoredSafeInventoryLocation::CharacterSafe(2),
                ),
                item(
                    2,
                    "consumable.red",
                    1,
                    StoredSafeInventoryLocation::CharacterSafe(2),
                ),
            ],
            vault: vec![item(
                3,
                "equipment",
                0,
                StoredSafeInventoryLocation::Vault(159),
            )],
            run_backpack: Vec::new(),
        };
        let (snapshot, versions) = project_snapshot(&stored).unwrap();
        assert_eq!(
            (snapshot.account_version, snapshot.inventory_version),
            (4, 7)
        );
        assert_eq!(versions.get(&[3; 16]), Some(&3));
        assert!(matches!(
            snapshot.vault[159],
            DurableStorageSlot::Equipment { .. }
        ));
        assert!(matches!(
            &snapshot.character_safe[2],
            DurableStorageSlot::Consumable { item_uids, .. } if item_uids.len() == 2
        ));
    }

    #[test]
    fn hashes_bind_identity_versions_command_and_server_placements() {
        let command = SafeInventoryTransferCommand {
            mutation_id: [3; 16],
            kind: SafeInventoryTransferKind::CharacterSafeToVault,
            source_slot_index: 2,
            expected_account_version: 4,
            expected_inventory_version: 7,
        };
        let hash = request_hash([1; 16], [2; 16], command);
        let mut changed = command;
        changed.expected_inventory_version = 8;
        assert_ne!(hash, request_hash([1; 16], [2; 16], changed));
        let placement = StoredSafeInventoryPlacement {
            item_uid: [4; 16],
            source: StoredSafeInventoryLocation::CharacterSafe(2),
            destination: StoredSafeInventoryLocation::Vault(0),
            expected_item_version: 1,
        };
        assert_ne!(result_hash(hash, &[placement]), result_hash(hash, &[]));
    }

    #[test]
    fn caller_can_name_only_source_and_versions() {
        assert!(
            validate_command(SafeInventoryTransferCommand {
                mutation_id: [1; 16],
                kind: SafeInventoryTransferKind::VaultToCharacterSafe,
                source_slot_index: 159,
                expected_account_version: 1,
                expected_inventory_version: 1,
            })
            .is_ok()
        );
        assert!(matches!(
            validate_command(SafeInventoryTransferCommand {
                mutation_id: [1; 16],
                kind: SafeInventoryTransferKind::VaultToCharacterSafe,
                source_slot_index: 160,
                expected_account_version: 1,
                expected_inventory_version: 1,
            }),
            Err(SafeInventoryServiceError::InvalidCommand)
        ));
    }
}
