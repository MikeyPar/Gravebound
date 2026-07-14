use std::collections::BTreeSet;

use thiserror::Error;

use crate::{DURABLE_CONSUMABLE_STACK_CAP, ItemUid, RUN_BACKPACK_CAPACITY};

pub const CHARACTER_SAFE_CAPACITY: usize = 8;
pub const VAULT_CAPACITY: usize = 160;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DurableStorageSlot {
    Empty,
    Equipment {
        item_uid: ItemUid,
    },
    Consumable {
        template_id: String,
        item_uids: Vec<ItemUid>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeStorageSnapshot {
    pub account_version: u64,
    pub inventory_version: u64,
    pub character_safe: Vec<DurableStorageSlot>,
    pub vault: Vec<DurableStorageSlot>,
    pub run_backpack: Vec<DurableStorageSlot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeStorageCommand {
    CharacterSafeToVault { source_slot: u8 },
    VaultToCharacterSafe { source_slot: u16 },
    CharacterSafeToRunBackpack { source_slot: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SafeStorageLocation {
    CharacterSafe(u8),
    Vault(u16),
    RunBackpack(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SafeStoragePlacement {
    pub item_uid: ItemUid,
    pub source: SafeStorageLocation,
    pub destination: SafeStorageLocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeStoragePlan {
    pub placements: Vec<SafeStoragePlacement>,
    pub pre_account_version: u64,
    pub post_account_version: u64,
    pub pre_inventory_version: u64,
    pub post_inventory_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SafeStorageError {
    #[error("safe-storage aggregate versions must be positive")]
    InvalidVersion,
    #[error("CharacterSafe must contain exactly eight slots")]
    InvalidCharacterSafeCapacity,
    #[error("Vault must contain exactly 160 slots")]
    InvalidVaultCapacity,
    #[error("RunBackpack must contain exactly eight slots")]
    InvalidRunBackpackCapacity,
    #[error("consumable storage stack must have a template and 1..=6 ordered units")]
    InvalidConsumableStack,
    #[error("one item UID occurs more than once in the safe-storage snapshot")]
    DuplicateItemUid,
    #[error("source slot is outside the authoritative storage capacity")]
    SourceOutOfRange,
    #[error("source slot is empty")]
    EmptySource,
    #[error("Vault lacks capacity for the complete mutation")]
    StorageFull,
    #[error("CharacterSafe lacks capacity for the complete mutation")]
    CharacterSafeFull,
    #[error("RunBackpack lacks capacity for the complete mutation")]
    RunBackpackFull,
    #[error("safe-storage aggregate version overflow")]
    VersionOverflow,
}

pub fn plan_safe_storage_transfer(
    snapshot: &SafeStorageSnapshot,
    command: SafeStorageCommand,
) -> Result<SafeStoragePlan, SafeStorageError> {
    validate_snapshot(snapshot)?;
    let (source, stack, mut destination, destination_kind, advances_account) = match command {
        SafeStorageCommand::CharacterSafeToVault { source_slot } => (
            SafeStorageLocation::CharacterSafe(source_slot),
            source_stack(&snapshot.character_safe, usize::from(source_slot))?,
            snapshot.vault.clone(),
            DestinationKind::Vault,
            true,
        ),
        SafeStorageCommand::VaultToCharacterSafe { source_slot } => (
            SafeStorageLocation::Vault(source_slot),
            source_stack(&snapshot.vault, usize::from(source_slot))?,
            snapshot.character_safe.clone(),
            DestinationKind::CharacterSafe,
            true,
        ),
        SafeStorageCommand::CharacterSafeToRunBackpack { source_slot } => (
            SafeStorageLocation::CharacterSafe(source_slot),
            source_stack(&snapshot.character_safe, usize::from(source_slot))?,
            snapshot.run_backpack.clone(),
            DestinationKind::RunBackpack,
            false,
        ),
    };
    let placements = place_stack(&stack, source, &mut destination, destination_kind)?;
    build_plan(snapshot, placements, advances_account)
}

pub fn plan_character_safe_preflight(
    snapshot: &SafeStorageSnapshot,
) -> Result<SafeStoragePlan, SafeStorageError> {
    validate_snapshot(snapshot)?;
    let mut vault = snapshot.vault.clone();
    let mut placements = Vec::new();
    for (index, stack) in snapshot.character_safe.iter().enumerate() {
        if matches!(stack, DurableStorageSlot::Empty) {
            continue;
        }
        let source = SafeStorageLocation::CharacterSafe(
            u8::try_from(index).map_err(|_| SafeStorageError::InvalidCharacterSafeCapacity)?,
        );
        placements.extend(place_stack(
            stack,
            source,
            &mut vault,
            DestinationKind::Vault,
        )?);
    }
    build_plan(snapshot, placements, true)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DestinationKind {
    CharacterSafe,
    Vault,
    RunBackpack,
}

impl DestinationKind {
    fn location(self, index: usize) -> Result<SafeStorageLocation, SafeStorageError> {
        match self {
            Self::CharacterSafe => Ok(SafeStorageLocation::CharacterSafe(
                u8::try_from(index).map_err(|_| SafeStorageError::InvalidCharacterSafeCapacity)?,
            )),
            Self::Vault => Ok(SafeStorageLocation::Vault(
                u16::try_from(index).map_err(|_| SafeStorageError::InvalidVaultCapacity)?,
            )),
            Self::RunBackpack => Ok(SafeStorageLocation::RunBackpack(
                u8::try_from(index).map_err(|_| SafeStorageError::InvalidRunBackpackCapacity)?,
            )),
        }
    }

    const fn full_error(self) -> SafeStorageError {
        match self {
            Self::CharacterSafe => SafeStorageError::CharacterSafeFull,
            Self::Vault => SafeStorageError::StorageFull,
            Self::RunBackpack => SafeStorageError::RunBackpackFull,
        }
    }
}

fn source_stack(
    slots: &[DurableStorageSlot],
    index: usize,
) -> Result<DurableStorageSlot, SafeStorageError> {
    let stack = slots.get(index).ok_or(SafeStorageError::SourceOutOfRange)?;
    if matches!(stack, DurableStorageSlot::Empty) {
        return Err(SafeStorageError::EmptySource);
    }
    Ok(stack.clone())
}

fn place_stack(
    stack: &DurableStorageSlot,
    source: SafeStorageLocation,
    destination: &mut [DurableStorageSlot],
    destination_kind: DestinationKind,
) -> Result<Vec<SafeStoragePlacement>, SafeStorageError> {
    match stack {
        DurableStorageSlot::Empty => Err(SafeStorageError::EmptySource),
        DurableStorageSlot::Equipment { item_uid } => {
            let index = destination
                .iter()
                .position(|slot| matches!(slot, DurableStorageSlot::Empty))
                .ok_or_else(|| destination_kind.full_error())?;
            destination[index] = DurableStorageSlot::Equipment {
                item_uid: *item_uid,
            };
            Ok(vec![SafeStoragePlacement {
                item_uid: *item_uid,
                source,
                destination: destination_kind.location(index)?,
            }])
        }
        DurableStorageSlot::Consumable {
            template_id,
            item_uids,
        } => place_consumables(
            template_id,
            item_uids,
            source,
            destination,
            destination_kind,
        ),
    }
}

fn place_consumables(
    template_id: &str,
    item_uids: &[ItemUid],
    source: SafeStorageLocation,
    destination: &mut [DurableStorageSlot],
    destination_kind: DestinationKind,
) -> Result<Vec<SafeStoragePlacement>, SafeStorageError> {
    let mut remaining = item_uids.iter().copied().peekable();
    let mut placements = Vec::with_capacity(item_uids.len());
    for (index, slot) in destination.iter_mut().enumerate() {
        let DurableStorageSlot::Consumable {
            template_id: stored_template,
            item_uids: stored_uids,
        } = slot
        else {
            continue;
        };
        if stored_template != template_id {
            continue;
        }
        while stored_uids.len() < usize::from(DURABLE_CONSUMABLE_STACK_CAP) {
            let Some(item_uid) = remaining.next() else {
                break;
            };
            stored_uids.push(item_uid);
            stored_uids.sort_unstable();
            placements.push(SafeStoragePlacement {
                item_uid,
                source,
                destination: destination_kind.location(index)?,
            });
        }
        if remaining.peek().is_none() {
            return Ok(placements);
        }
    }
    while remaining.peek().is_some() {
        let index = destination
            .iter()
            .position(|slot| matches!(slot, DurableStorageSlot::Empty))
            .ok_or_else(|| destination_kind.full_error())?;
        let mut placed_uids = Vec::new();
        while placed_uids.len() < usize::from(DURABLE_CONSUMABLE_STACK_CAP) {
            let Some(item_uid) = remaining.next() else {
                break;
            };
            placed_uids.push(item_uid);
            placements.push(SafeStoragePlacement {
                item_uid,
                source,
                destination: destination_kind.location(index)?,
            });
        }
        destination[index] = DurableStorageSlot::Consumable {
            template_id: template_id.to_owned(),
            item_uids: placed_uids,
        };
    }
    Ok(placements)
}

fn build_plan(
    snapshot: &SafeStorageSnapshot,
    placements: Vec<SafeStoragePlacement>,
    advances_account: bool,
) -> Result<SafeStoragePlan, SafeStorageError> {
    let changed = !placements.is_empty();
    let post_account_version = if changed && advances_account {
        snapshot
            .account_version
            .checked_add(1)
            .ok_or(SafeStorageError::VersionOverflow)?
    } else {
        snapshot.account_version
    };
    let post_inventory_version = if changed {
        snapshot
            .inventory_version
            .checked_add(1)
            .ok_or(SafeStorageError::VersionOverflow)?
    } else {
        snapshot.inventory_version
    };
    Ok(SafeStoragePlan {
        placements,
        pre_account_version: snapshot.account_version,
        post_account_version,
        pre_inventory_version: snapshot.inventory_version,
        post_inventory_version,
    })
}

fn validate_snapshot(snapshot: &SafeStorageSnapshot) -> Result<(), SafeStorageError> {
    if snapshot.account_version == 0 || snapshot.inventory_version == 0 {
        return Err(SafeStorageError::InvalidVersion);
    }
    if snapshot.character_safe.len() != CHARACTER_SAFE_CAPACITY {
        return Err(SafeStorageError::InvalidCharacterSafeCapacity);
    }
    if snapshot.vault.len() != VAULT_CAPACITY {
        return Err(SafeStorageError::InvalidVaultCapacity);
    }
    if snapshot.run_backpack.len() != RUN_BACKPACK_CAPACITY {
        return Err(SafeStorageError::InvalidRunBackpackCapacity);
    }
    let mut identities = BTreeSet::new();
    for slot in snapshot
        .character_safe
        .iter()
        .chain(&snapshot.vault)
        .chain(&snapshot.run_backpack)
    {
        match slot {
            DurableStorageSlot::Empty => {}
            DurableStorageSlot::Equipment { item_uid } => {
                if !identities.insert(*item_uid) {
                    return Err(SafeStorageError::DuplicateItemUid);
                }
            }
            DurableStorageSlot::Consumable {
                template_id,
                item_uids,
            } => {
                if template_id.is_empty()
                    || item_uids.is_empty()
                    || item_uids.len() > usize::from(DURABLE_CONSUMABLE_STACK_CAP)
                    || !item_uids.windows(2).all(|pair| pair[0] < pair[1])
                {
                    return Err(SafeStorageError::InvalidConsumableStack);
                }
                for item_uid in item_uids {
                    if !identities.insert(*item_uid) {
                        return Err(SafeStorageError::DuplicateItemUid);
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid(byte: u8) -> ItemUid {
        ItemUid::new([byte; 16]).unwrap()
    }

    fn empty_snapshot() -> SafeStorageSnapshot {
        SafeStorageSnapshot {
            account_version: 4,
            inventory_version: 7,
            character_safe: vec![DurableStorageSlot::Empty; CHARACTER_SAFE_CAPACITY],
            vault: vec![DurableStorageSlot::Empty; VAULT_CAPACITY],
            run_backpack: vec![DurableStorageSlot::Empty; RUN_BACKPACK_CAPACITY],
        }
    }

    #[test]
    fn equipment_uses_lowest_index_and_cross_scope_versions_advance_once() {
        let mut snapshot = empty_snapshot();
        snapshot.character_safe[2] = DurableStorageSlot::Equipment { item_uid: uid(1) };
        snapshot.vault[0] = DurableStorageSlot::Equipment { item_uid: uid(2) };
        snapshot.vault[2] = DurableStorageSlot::Equipment { item_uid: uid(3) };
        let plan = plan_safe_storage_transfer(
            &snapshot,
            SafeStorageCommand::CharacterSafeToVault { source_slot: 2 },
        )
        .unwrap();
        assert_eq!(
            plan.placements,
            vec![SafeStoragePlacement {
                item_uid: uid(1),
                source: SafeStorageLocation::CharacterSafe(2),
                destination: SafeStorageLocation::Vault(1),
            }]
        );
        assert_eq!(
            (
                plan.pre_account_version,
                plan.post_account_version,
                plan.pre_inventory_version,
                plan.post_inventory_version
            ),
            (4, 5, 7, 8)
        );
    }

    #[test]
    fn consumables_merge_then_split_by_slot_and_unsigned_uid() {
        let mut snapshot = empty_snapshot();
        snapshot.character_safe[0] = DurableStorageSlot::Consumable {
            template_id: "consumable.red_tonic".to_owned(),
            item_uids: vec![uid(4), uid(5), uid(6)],
        };
        snapshot.vault[1] = DurableStorageSlot::Consumable {
            template_id: "consumable.red_tonic".to_owned(),
            item_uids: vec![uid(1), uid(2), uid(3), uid(7), uid(8)],
        };
        snapshot.vault[0] = DurableStorageSlot::Consumable {
            template_id: "consumable.other".to_owned(),
            item_uids: vec![uid(9)],
        };
        let plan = plan_safe_storage_transfer(
            &snapshot,
            SafeStorageCommand::CharacterSafeToVault { source_slot: 0 },
        )
        .unwrap();
        assert_eq!(
            plan.placements
                .iter()
                .map(|placement| (placement.item_uid, placement.destination))
                .collect::<Vec<_>>(),
            [
                (uid(4), SafeStorageLocation::Vault(1)),
                (uid(5), SafeStorageLocation::Vault(2)),
                (uid(6), SafeStorageLocation::Vault(2)),
            ]
        );
    }

    #[test]
    fn full_destination_rejects_the_complete_plan_without_partial_output() {
        let mut snapshot = empty_snapshot();
        snapshot.character_safe[0] = DurableStorageSlot::Equipment { item_uid: uid(1) };
        for (index, slot) in snapshot.vault.iter_mut().enumerate() {
            *slot = DurableStorageSlot::Equipment {
                item_uid: ItemUid::new((u128::try_from(index).unwrap() + 2).to_be_bytes()).unwrap(),
            };
        }
        assert_eq!(
            plan_safe_storage_transfer(
                &snapshot,
                SafeStorageCommand::CharacterSafeToVault { source_slot: 0 }
            ),
            Err(SafeStorageError::StorageFull)
        );
    }

    #[test]
    fn preflight_is_atomic_and_empty_preflight_is_a_true_noop() {
        let empty = empty_snapshot();
        let noop = plan_character_safe_preflight(&empty).unwrap();
        assert!(noop.placements.is_empty());
        assert_eq!(
            (noop.post_account_version, noop.post_inventory_version),
            (4, 7)
        );

        let mut snapshot = empty_snapshot();
        snapshot.character_safe[0] = DurableStorageSlot::Equipment { item_uid: uid(1) };
        snapshot.character_safe[7] = DurableStorageSlot::Equipment { item_uid: uid(2) };
        for (index, slot) in snapshot.vault.iter_mut().enumerate() {
            *slot = DurableStorageSlot::Equipment {
                item_uid: ItemUid::new((u128::try_from(index).unwrap() + 100).to_be_bytes())
                    .unwrap(),
            };
        }
        assert_eq!(
            plan_character_safe_preflight(&snapshot),
            Err(SafeStorageError::StorageFull)
        );
    }

    #[test]
    fn preflight_visits_all_sources_in_slot_order_and_advances_once() {
        let mut snapshot = empty_snapshot();
        snapshot.character_safe[1] = DurableStorageSlot::Equipment { item_uid: uid(1) };
        snapshot.character_safe[7] = DurableStorageSlot::Equipment { item_uid: uid(2) };
        snapshot.vault[0] = DurableStorageSlot::Equipment { item_uid: uid(3) };
        let plan = plan_character_safe_preflight(&snapshot).unwrap();
        assert_eq!(
            plan.placements,
            [
                SafeStoragePlacement {
                    item_uid: uid(1),
                    source: SafeStorageLocation::CharacterSafe(1),
                    destination: SafeStorageLocation::Vault(1),
                },
                SafeStoragePlacement {
                    item_uid: uid(2),
                    source: SafeStorageLocation::CharacterSafe(7),
                    destination: SafeStorageLocation::Vault(2),
                },
            ]
        );
        assert_eq!(
            (plan.post_account_version, plan.post_inventory_version),
            (5, 8)
        );
    }

    #[test]
    fn deliberate_risk_changes_only_the_character_inventory_version() {
        let mut snapshot = empty_snapshot();
        snapshot.character_safe[0] = DurableStorageSlot::Equipment { item_uid: uid(1) };
        let plan = plan_safe_storage_transfer(
            &snapshot,
            SafeStorageCommand::CharacterSafeToRunBackpack { source_slot: 0 },
        )
        .unwrap();
        assert_eq!(
            plan.placements[0].destination,
            SafeStorageLocation::RunBackpack(0)
        );
        assert_eq!(
            (plan.post_account_version, plan.post_inventory_version),
            (4, 8)
        );
    }

    #[test]
    fn malformed_capacities_stacks_and_sources_fail_closed() {
        let mut snapshot = empty_snapshot();
        snapshot.vault.pop();
        assert_eq!(
            plan_character_safe_preflight(&snapshot),
            Err(SafeStorageError::InvalidVaultCapacity)
        );
        let mut snapshot = empty_snapshot();
        snapshot.character_safe[0] = DurableStorageSlot::Consumable {
            template_id: String::new(),
            item_uids: vec![uid(1)],
        };
        assert_eq!(
            plan_character_safe_preflight(&snapshot),
            Err(SafeStorageError::InvalidConsumableStack)
        );
        let snapshot = empty_snapshot();
        assert_eq!(
            plan_safe_storage_transfer(
                &snapshot,
                SafeStorageCommand::VaultToCharacterSafe { source_slot: 160 }
            ),
            Err(SafeStorageError::SourceOutOfRange)
        );
    }

    #[test]
    fn character_safe_and_run_backpack_full_errors_remain_distinct() {
        let mut safe_full = empty_snapshot();
        safe_full.vault[0] = DurableStorageSlot::Equipment { item_uid: uid(1) };
        for (index, slot) in safe_full.character_safe.iter_mut().enumerate() {
            *slot = DurableStorageSlot::Equipment {
                item_uid: uid(u8::try_from(index + 2).unwrap()),
            };
        }
        assert_eq!(
            plan_safe_storage_transfer(
                &safe_full,
                SafeStorageCommand::VaultToCharacterSafe { source_slot: 0 }
            ),
            Err(SafeStorageError::CharacterSafeFull)
        );

        let mut backpack_full = empty_snapshot();
        backpack_full.character_safe[0] = DurableStorageSlot::Equipment { item_uid: uid(1) };
        for (index, slot) in backpack_full.run_backpack.iter_mut().enumerate() {
            *slot = DurableStorageSlot::Equipment {
                item_uid: uid(u8::try_from(index + 2).unwrap()),
            };
        }
        assert_eq!(
            plan_safe_storage_transfer(
                &backpack_full,
                SafeStorageCommand::CharacterSafeToRunBackpack { source_slot: 0 }
            ),
            Err(SafeStorageError::RunBackpackFull)
        );
    }
}
