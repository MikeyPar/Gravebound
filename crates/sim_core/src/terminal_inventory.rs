use std::collections::BTreeSet;

use thiserror::Error;

use crate::{
    CHARACTER_SAFE_CAPACITY, DURABLE_CONSUMABLE_STACK_CAP, DurableStorageSlot,
    EQUIPMENT_SLOT_COUNT, ItemUid, RUN_BACKPACK_CAPACITY, VAULT_CAPACITY,
};

pub const TERMINAL_BELT_CAPACITY: usize = 2;
pub const TERMINAL_OVERFLOW_CAPACITY: usize = 20;
pub const TERMINAL_RESOLUTION_HOLD_CAPACITY: usize = RUN_BACKPACK_CAPACITY;
pub const TERMINAL_MATERIAL_CAPACITY: usize = 4;
pub const OVERFLOW_LIFETIME_MICROS: u64 = 72 * 60 * 60 * 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TerminalInventoryLocation {
    Equipped(u8),
    Belt(u8),
    RunBackpack(u8),
    CharacterSafe(u8),
    Vault(u16),
    Overflow(u8),
    ResolutionHold(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalItemPlacement {
    pub item_uid: ItemUid,
    pub source: TerminalInventoryLocation,
    pub destination: TerminalInventoryLocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalMaterialSnapshot {
    pub material_id: String,
    pub safe_quantity: u32,
    pub pending_quantity: u16,
    pub wallet_cap: u32,
    pub wallet_version: u64,
    pub pouch_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalMaterialCredit {
    pub material_id: String,
    pub credited_quantity: u16,
    pub pre_safe_quantity: u32,
    pub post_safe_quantity: u32,
    pub pre_wallet_version: u64,
    pub post_wallet_version: u64,
    pub pre_pouch_version: u64,
    pub post_pouch_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionInventorySnapshot {
    pub account_version: u64,
    pub inventory_version: u64,
    pub committed_at_unix_micros: u64,
    pub equipped: Vec<DurableStorageSlot>,
    pub belt: Vec<DurableStorageSlot>,
    pub character_safe: Vec<DurableStorageSlot>,
    pub vault: Vec<DurableStorageSlot>,
    pub overflow: Vec<DurableStorageSlot>,
    pub run_backpack: Vec<DurableStorageSlot>,
    pub resolution_hold: Vec<DurableStorageSlot>,
    pub materials: Vec<TerminalMaterialSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionInventoryPlan {
    pub placements: Vec<TerminalItemPlacement>,
    pub material_credits: Vec<TerminalMaterialCredit>,
    pub post_account_version: u64,
    pub post_inventory_version: u64,
    pub overflow_expires_at_unix_micros: u64,
    pub post_belt: Vec<DurableStorageSlot>,
    pub post_character_safe: Vec<DurableStorageSlot>,
    pub post_vault: Vec<DurableStorageSlot>,
    pub post_overflow: Vec<DurableStorageSlot>,
    pub post_run_backpack: Vec<DurableStorageSlot>,
    pub post_resolution_hold: Vec<DurableStorageSlot>,
}

impl ExtractionInventoryPlan {
    #[must_use]
    pub fn resolution_required(&self) -> bool {
        self.post_resolution_hold
            .iter()
            .any(|slot| !matches!(slot, DurableStorageSlot::Empty))
    }

    #[must_use]
    pub fn accepted_item_count(&self) -> usize {
        self.placements
            .iter()
            .filter(|placement| {
                matches!(placement.source, TerminalInventoryLocation::RunBackpack(_))
            })
            .count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TerminalInventoryError {
    #[error("terminal inventory versions and commit time must be positive")]
    InvalidAuthorityVersion,
    #[error("terminal inventory slot capacity is invalid")]
    InvalidCapacity,
    #[error("terminal inventory slot contains an illegal item kind")]
    InvalidSlotKind,
    #[error("consumable stack must have a bounded template and ordered 1..=6 unique UIDs")]
    InvalidConsumableStack,
    #[error("one durable item UID occurs more than once")]
    DuplicateItemUid,
    #[error("ResolutionHold must be empty before danger and extraction")]
    UnresolvedResolutionHold,
    #[error("ResolutionHold lacks capacity for the complete accepted extraction")]
    ResolutionHoldFull,
    #[error("terminal material snapshot is invalid")]
    InvalidMaterial,
    #[error("terminal material identity occurs more than once")]
    DuplicateMaterial,
    #[error("terminal authority version or time overflow")]
    ArithmeticOverflow,
}

pub fn plan_successful_extraction(
    snapshot: &ExtractionInventorySnapshot,
) -> Result<ExtractionInventoryPlan, TerminalInventoryError> {
    validate_snapshot(snapshot)?;
    let overflow_expires_at_unix_micros = snapshot
        .committed_at_unix_micros
        .checked_add(OVERFLOW_LIFETIME_MICROS)
        .ok_or(TerminalInventoryError::ArithmeticOverflow)?;
    let items = plan_extraction_items(snapshot)?;
    let material_credits = plan_material_credits(&snapshot.materials)?;
    let inventory_changed = !items.placements.is_empty();
    let account_changed = !material_credits.is_empty()
        || items.placements.iter().any(|placement| {
            matches!(
                placement.destination,
                TerminalInventoryLocation::Vault(_) | TerminalInventoryLocation::Overflow(_)
            ) && placement.source != placement.destination
        });
    let post_inventory_version = if inventory_changed {
        snapshot
            .inventory_version
            .checked_add(1)
            .ok_or(TerminalInventoryError::ArithmeticOverflow)?
    } else {
        snapshot.inventory_version
    };
    let post_account_version = if account_changed {
        snapshot
            .account_version
            .checked_add(1)
            .ok_or(TerminalInventoryError::ArithmeticOverflow)?
    } else {
        snapshot.account_version
    };

    let plan = ExtractionInventoryPlan {
        placements: items.placements,
        material_credits,
        post_account_version,
        post_inventory_version,
        overflow_expires_at_unix_micros,
        post_belt: items.belt,
        post_character_safe: items.character_safe,
        post_vault: items.vault,
        post_overflow: items.overflow,
        post_run_backpack: items.run_backpack,
        post_resolution_hold: items.resolution_hold,
    };
    validate_plan_conservation(snapshot, &plan)?;
    Ok(plan)
}

struct PlannedExtractionItems {
    placements: Vec<TerminalItemPlacement>,
    belt: Vec<DurableStorageSlot>,
    character_safe: Vec<DurableStorageSlot>,
    vault: Vec<DurableStorageSlot>,
    overflow: Vec<DurableStorageSlot>,
    run_backpack: Vec<DurableStorageSlot>,
    resolution_hold: Vec<DurableStorageSlot>,
}

fn plan_extraction_items(
    snapshot: &ExtractionInventorySnapshot,
) -> Result<PlannedExtractionItems, TerminalInventoryError> {
    let mut items = PlannedExtractionItems {
        placements: Vec::new(),
        belt: snapshot.belt.clone(),
        character_safe: snapshot.character_safe.clone(),
        vault: snapshot.vault.clone(),
        overflow: snapshot.overflow.clone(),
        run_backpack: snapshot.run_backpack.clone(),
        resolution_hold: snapshot.resolution_hold.clone(),
    };
    append_stabilized_placements(
        &snapshot.equipped,
        TerminalLocationKind::Equipped,
        &mut items.placements,
    )?;
    append_stabilized_placements(
        &snapshot.belt,
        TerminalLocationKind::Belt,
        &mut items.placements,
    )?;
    for (source_index, source_slot) in snapshot.run_backpack.iter().enumerate() {
        place_run_backpack_slot(source_index, source_slot, &mut items)?;
        items.run_backpack[source_index] = DurableStorageSlot::Empty;
    }
    Ok(items)
}

fn place_run_backpack_slot(
    source_index: usize,
    source_slot: &DurableStorageSlot,
    items: &mut PlannedExtractionItems,
) -> Result<(), TerminalInventoryError> {
    let source = TerminalInventoryLocation::RunBackpack(
        u8::try_from(source_index).map_err(|_| TerminalInventoryError::InvalidCapacity)?,
    );
    match source_slot {
        DurableStorageSlot::Empty => {}
        DurableStorageSlot::Equipment { item_uid } => {
            let destination = place_equipment(
                *item_uid,
                &mut items.character_safe,
                &mut items.vault,
                &mut items.overflow,
                &mut items.resolution_hold,
            )?;
            items.placements.push(TerminalItemPlacement {
                item_uid: *item_uid,
                source,
                destination,
            });
        }
        DurableStorageSlot::Consumable {
            template_id,
            item_uids,
        } => place_run_backpack_consumables(template_id, item_uids, source, items)?,
    }
    Ok(())
}

fn place_run_backpack_consumables(
    template_id: &str,
    item_uids: &[ItemUid],
    source: TerminalInventoryLocation,
    items: &mut PlannedExtractionItems,
) -> Result<(), TerminalInventoryError> {
    let mut remaining = item_uids.to_vec();
    for (destination, kind) in [
        (&mut items.belt, TerminalLocationKind::Belt),
        (
            &mut items.character_safe,
            TerminalLocationKind::CharacterSafe,
        ),
        (&mut items.vault, TerminalLocationKind::Vault),
    ] {
        merge_consumables(
            template_id,
            &mut remaining,
            source,
            destination,
            kind,
            &mut items.placements,
        )?;
    }
    if remaining.is_empty() {
        return Ok(());
    }
    let destination = place_consumable_remainder(
        template_id,
        &remaining,
        &mut items.character_safe,
        &mut items.vault,
        &mut items.overflow,
        &mut items.resolution_hold,
    )?;
    items.placements.extend(
        remaining
            .iter()
            .copied()
            .map(|item_uid| TerminalItemPlacement {
                item_uid,
                source,
                destination,
            }),
    );
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalLocationKind {
    Equipped,
    Belt,
    CharacterSafe,
    Vault,
    Overflow,
    ResolutionHold,
}

impl TerminalLocationKind {
    fn location(self, index: usize) -> Result<TerminalInventoryLocation, TerminalInventoryError> {
        match self {
            Self::Equipped => Ok(TerminalInventoryLocation::Equipped(
                u8::try_from(index).map_err(|_| TerminalInventoryError::InvalidCapacity)?,
            )),
            Self::Belt => Ok(TerminalInventoryLocation::Belt(
                u8::try_from(index).map_err(|_| TerminalInventoryError::InvalidCapacity)?,
            )),
            Self::CharacterSafe => Ok(TerminalInventoryLocation::CharacterSafe(
                u8::try_from(index).map_err(|_| TerminalInventoryError::InvalidCapacity)?,
            )),
            Self::Vault => Ok(TerminalInventoryLocation::Vault(
                u16::try_from(index).map_err(|_| TerminalInventoryError::InvalidCapacity)?,
            )),
            Self::Overflow => Ok(TerminalInventoryLocation::Overflow(
                u8::try_from(index).map_err(|_| TerminalInventoryError::InvalidCapacity)?,
            )),
            Self::ResolutionHold => Ok(TerminalInventoryLocation::ResolutionHold(
                u8::try_from(index).map_err(|_| TerminalInventoryError::InvalidCapacity)?,
            )),
        }
    }
}

fn append_stabilized_placements(
    slots: &[DurableStorageSlot],
    kind: TerminalLocationKind,
    placements: &mut Vec<TerminalItemPlacement>,
) -> Result<(), TerminalInventoryError> {
    for (index, slot) in slots.iter().enumerate() {
        let location = kind.location(index)?;
        match slot {
            DurableStorageSlot::Empty => {}
            DurableStorageSlot::Equipment { item_uid } => {
                placements.push(TerminalItemPlacement {
                    item_uid: *item_uid,
                    source: location,
                    destination: location,
                });
            }
            DurableStorageSlot::Consumable { item_uids, .. } => {
                placements.extend(item_uids.iter().copied().map(|item_uid| {
                    TerminalItemPlacement {
                        item_uid,
                        source: location,
                        destination: location,
                    }
                }));
            }
        }
    }
    Ok(())
}

fn place_equipment(
    item_uid: ItemUid,
    character_safe: &mut [DurableStorageSlot],
    vault: &mut [DurableStorageSlot],
    overflow: &mut [DurableStorageSlot],
    resolution_hold: &mut [DurableStorageSlot],
) -> Result<TerminalInventoryLocation, TerminalInventoryError> {
    for (slots, kind) in [
        (&mut *character_safe, TerminalLocationKind::CharacterSafe),
        (&mut *vault, TerminalLocationKind::Vault),
        (&mut *overflow, TerminalLocationKind::Overflow),
        (&mut *resolution_hold, TerminalLocationKind::ResolutionHold),
    ] {
        if let Some(index) = slots
            .iter()
            .position(|slot| matches!(slot, DurableStorageSlot::Empty))
        {
            slots[index] = DurableStorageSlot::Equipment { item_uid };
            return kind.location(index);
        }
    }
    Err(TerminalInventoryError::ResolutionHoldFull)
}

fn merge_consumables(
    template_id: &str,
    remaining: &mut Vec<ItemUid>,
    source: TerminalInventoryLocation,
    destination: &mut [DurableStorageSlot],
    destination_kind: TerminalLocationKind,
    placements: &mut Vec<TerminalItemPlacement>,
) -> Result<(), TerminalInventoryError> {
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
        while stored_uids.len() < usize::from(DURABLE_CONSUMABLE_STACK_CAP) && !remaining.is_empty()
        {
            let item_uid = remaining.remove(0);
            stored_uids.push(item_uid);
            stored_uids.sort_unstable();
            placements.push(TerminalItemPlacement {
                item_uid,
                source,
                destination: destination_kind.location(index)?,
            });
        }
        if remaining.is_empty() {
            break;
        }
    }
    Ok(())
}

fn place_consumable_remainder(
    template_id: &str,
    remaining: &[ItemUid],
    character_safe: &mut [DurableStorageSlot],
    vault: &mut [DurableStorageSlot],
    overflow: &mut [DurableStorageSlot],
    resolution_hold: &mut [DurableStorageSlot],
) -> Result<TerminalInventoryLocation, TerminalInventoryError> {
    for (slots, kind) in [
        (&mut *character_safe, TerminalLocationKind::CharacterSafe),
        (&mut *vault, TerminalLocationKind::Vault),
        (&mut *overflow, TerminalLocationKind::Overflow),
        (&mut *resolution_hold, TerminalLocationKind::ResolutionHold),
    ] {
        if let Some(index) = slots
            .iter()
            .position(|slot| matches!(slot, DurableStorageSlot::Empty))
        {
            slots[index] = DurableStorageSlot::Consumable {
                template_id: template_id.to_owned(),
                item_uids: remaining.to_vec(),
            };
            return kind.location(index);
        }
    }
    Err(TerminalInventoryError::ResolutionHoldFull)
}

fn plan_material_credits(
    materials: &[TerminalMaterialSnapshot],
) -> Result<Vec<TerminalMaterialCredit>, TerminalInventoryError> {
    let mut sorted = materials.to_vec();
    sorted.sort_by(|left, right| {
        left.material_id
            .as_bytes()
            .cmp(right.material_id.as_bytes())
    });
    sorted
        .into_iter()
        .map(|material| {
            let post_safe_quantity = material
                .safe_quantity
                .checked_add(u32::from(material.pending_quantity))
                .ok_or(TerminalInventoryError::ArithmeticOverflow)?;
            Ok(TerminalMaterialCredit {
                material_id: material.material_id,
                credited_quantity: material.pending_quantity,
                pre_safe_quantity: material.safe_quantity,
                post_safe_quantity,
                pre_wallet_version: material.wallet_version,
                post_wallet_version: material
                    .wallet_version
                    .checked_add(1)
                    .ok_or(TerminalInventoryError::ArithmeticOverflow)?,
                pre_pouch_version: material.pouch_version,
                post_pouch_version: material
                    .pouch_version
                    .checked_add(1)
                    .ok_or(TerminalInventoryError::ArithmeticOverflow)?,
            })
        })
        .collect()
}

fn validate_snapshot(snapshot: &ExtractionInventorySnapshot) -> Result<(), TerminalInventoryError> {
    if snapshot.account_version == 0
        || snapshot.inventory_version == 0
        || snapshot.committed_at_unix_micros == 0
    {
        return Err(TerminalInventoryError::InvalidAuthorityVersion);
    }
    if snapshot.equipped.len() != EQUIPMENT_SLOT_COUNT
        || snapshot.belt.len() != TERMINAL_BELT_CAPACITY
        || snapshot.character_safe.len() != CHARACTER_SAFE_CAPACITY
        || snapshot.vault.len() != VAULT_CAPACITY
        || snapshot.overflow.len() != TERMINAL_OVERFLOW_CAPACITY
        || snapshot.run_backpack.len() != RUN_BACKPACK_CAPACITY
        || snapshot.resolution_hold.len() != TERMINAL_RESOLUTION_HOLD_CAPACITY
    {
        return Err(TerminalInventoryError::InvalidCapacity);
    }
    if snapshot
        .resolution_hold
        .iter()
        .any(|slot| !matches!(slot, DurableStorageSlot::Empty))
    {
        return Err(TerminalInventoryError::UnresolvedResolutionHold);
    }
    validate_slot_kinds(&snapshot.equipped, SlotPolicy::EquipmentOnly)?;
    validate_slot_kinds(&snapshot.belt, SlotPolicy::ConsumableOnly)?;
    for slots in [
        &snapshot.character_safe,
        &snapshot.vault,
        &snapshot.overflow,
        &snapshot.run_backpack,
    ] {
        validate_slot_kinds(slots, SlotPolicy::Any)?;
    }

    let mut identities = BTreeSet::new();
    for slot in snapshot
        .equipped
        .iter()
        .chain(&snapshot.belt)
        .chain(&snapshot.character_safe)
        .chain(&snapshot.vault)
        .chain(&snapshot.overflow)
        .chain(&snapshot.run_backpack)
    {
        match slot {
            DurableStorageSlot::Empty => {}
            DurableStorageSlot::Equipment { item_uid } => {
                if !identities.insert(*item_uid) {
                    return Err(TerminalInventoryError::DuplicateItemUid);
                }
            }
            DurableStorageSlot::Consumable {
                template_id,
                item_uids,
            } => {
                if !valid_template_id(template_id)
                    || item_uids.is_empty()
                    || item_uids.len() > usize::from(DURABLE_CONSUMABLE_STACK_CAP)
                    || !item_uids.windows(2).all(|pair| pair[0] < pair[1])
                {
                    return Err(TerminalInventoryError::InvalidConsumableStack);
                }
                for item_uid in item_uids {
                    if !identities.insert(*item_uid) {
                        return Err(TerminalInventoryError::DuplicateItemUid);
                    }
                }
            }
        }
    }
    validate_materials(&snapshot.materials)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotPolicy {
    EquipmentOnly,
    ConsumableOnly,
    Any,
}

fn validate_slot_kinds(
    slots: &[DurableStorageSlot],
    policy: SlotPolicy,
) -> Result<(), TerminalInventoryError> {
    let valid = slots.iter().all(|slot| {
        matches!(
            (policy, slot),
            (_, DurableStorageSlot::Empty)
                | (
                    SlotPolicy::EquipmentOnly | SlotPolicy::Any,
                    DurableStorageSlot::Equipment { .. }
                )
                | (
                    SlotPolicy::ConsumableOnly | SlotPolicy::Any,
                    DurableStorageSlot::Consumable { .. }
                )
        )
    });
    if valid {
        Ok(())
    } else {
        Err(TerminalInventoryError::InvalidSlotKind)
    }
}

fn validate_materials(
    materials: &[TerminalMaterialSnapshot],
) -> Result<(), TerminalInventoryError> {
    if materials.len() > TERMINAL_MATERIAL_CAPACITY {
        return Err(TerminalInventoryError::InvalidMaterial);
    }
    let mut identities = BTreeSet::new();
    for material in materials {
        let total = material
            .safe_quantity
            .checked_add(u32::from(material.pending_quantity))
            .ok_or(TerminalInventoryError::InvalidMaterial)?;
        if !valid_template_id(&material.material_id)
            || material.pending_quantity == 0
            || material.pending_quantity > 99
            || material.wallet_cap == 0
            || total > material.wallet_cap
            || material.wallet_version == 0
            || material.pouch_version == 0
        {
            return Err(TerminalInventoryError::InvalidMaterial);
        }
        if !identities.insert(material.material_id.as_bytes()) {
            return Err(TerminalInventoryError::DuplicateMaterial);
        }
    }
    Ok(())
}

fn valid_template_id(value: &str) -> bool {
    (3..=96).contains(&value.len())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn validate_plan_conservation(
    snapshot: &ExtractionInventorySnapshot,
    plan: &ExtractionInventoryPlan,
) -> Result<(), TerminalInventoryError> {
    let accepted: BTreeSet<_> = snapshot.run_backpack.iter().flat_map(slot_uids).collect();
    let placed: Vec<_> = plan
        .placements
        .iter()
        .filter(|placement| matches!(placement.source, TerminalInventoryLocation::RunBackpack(_)))
        .map(|placement| placement.item_uid)
        .collect();
    let placed_set: BTreeSet<_> = placed.iter().copied().collect();
    if placed.len() != placed_set.len()
        || placed_set != accepted
        || plan
            .post_run_backpack
            .iter()
            .any(|slot| !matches!(slot, DurableStorageSlot::Empty))
    {
        return Err(TerminalInventoryError::DuplicateItemUid);
    }
    Ok(())
}

fn slot_uids(slot: &DurableStorageSlot) -> Vec<ItemUid> {
    match slot {
        DurableStorageSlot::Empty => Vec::new(),
        DurableStorageSlot::Equipment { item_uid } => vec![*item_uid],
        DurableStorageSlot::Consumable { item_uids, .. } => item_uids.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid(value: u128) -> ItemUid {
        ItemUid::new(value.to_be_bytes()).unwrap()
    }

    fn empty_snapshot() -> ExtractionInventorySnapshot {
        ExtractionInventorySnapshot {
            account_version: 4,
            inventory_version: 7,
            committed_at_unix_micros: 1_000_000,
            equipped: vec![DurableStorageSlot::Empty; EQUIPMENT_SLOT_COUNT],
            belt: vec![DurableStorageSlot::Empty; TERMINAL_BELT_CAPACITY],
            character_safe: vec![DurableStorageSlot::Empty; CHARACTER_SAFE_CAPACITY],
            vault: vec![DurableStorageSlot::Empty; VAULT_CAPACITY],
            overflow: vec![DurableStorageSlot::Empty; TERMINAL_OVERFLOW_CAPACITY],
            run_backpack: vec![DurableStorageSlot::Empty; RUN_BACKPACK_CAPACITY],
            resolution_hold: vec![DurableStorageSlot::Empty; TERMINAL_RESOLUTION_HOLD_CAPACITY],
            materials: Vec::new(),
        }
    }

    fn equipment(value: u128) -> DurableStorageSlot {
        DurableStorageSlot::Equipment {
            item_uid: uid(value),
        }
    }

    fn tonic(values: &[u128]) -> DurableStorageSlot {
        DurableStorageSlot::Consumable {
            template_id: "consumable.red_tonic".to_owned(),
            item_uids: values.iter().copied().map(uid).collect(),
        }
    }

    #[test]
    fn empty_extraction_is_a_true_inventory_noop_with_exact_deadline() {
        let snapshot = empty_snapshot();
        let plan = plan_successful_extraction(&snapshot).unwrap();
        assert!(plan.placements.is_empty());
        assert!(plan.material_credits.is_empty());
        assert_eq!(plan.post_account_version, 4);
        assert_eq!(plan.post_inventory_version, 7);
        assert_eq!(
            plan.overflow_expires_at_unix_micros,
            1_000_000 + OVERFLOW_LIFETIME_MICROS
        );
        assert!(!plan.resolution_required());
    }

    #[test]
    fn extraction_stabilizes_equipment_and_belt_then_places_pending_in_exact_order() {
        let mut snapshot = empty_snapshot();
        snapshot.equipped[0] = equipment(1);
        snapshot.belt[0] = tonic(&[2, 3, 4, 5, 6]);
        snapshot.character_safe[0] = tonic(&[10, 11, 12, 13, 14]);
        snapshot.vault[0] = tonic(&[20, 21, 22, 23, 24]);
        snapshot.run_backpack[0] = tonic(&[30, 31, 32, 33]);
        snapshot.run_backpack[1] = equipment(40);

        let plan = plan_successful_extraction(&snapshot).unwrap();
        assert_eq!(
            plan.placements,
            [
                TerminalItemPlacement {
                    item_uid: uid(1),
                    source: TerminalInventoryLocation::Equipped(0),
                    destination: TerminalInventoryLocation::Equipped(0),
                },
                TerminalItemPlacement {
                    item_uid: uid(2),
                    source: TerminalInventoryLocation::Belt(0),
                    destination: TerminalInventoryLocation::Belt(0),
                },
                TerminalItemPlacement {
                    item_uid: uid(3),
                    source: TerminalInventoryLocation::Belt(0),
                    destination: TerminalInventoryLocation::Belt(0),
                },
                TerminalItemPlacement {
                    item_uid: uid(4),
                    source: TerminalInventoryLocation::Belt(0),
                    destination: TerminalInventoryLocation::Belt(0),
                },
                TerminalItemPlacement {
                    item_uid: uid(5),
                    source: TerminalInventoryLocation::Belt(0),
                    destination: TerminalInventoryLocation::Belt(0),
                },
                TerminalItemPlacement {
                    item_uid: uid(6),
                    source: TerminalInventoryLocation::Belt(0),
                    destination: TerminalInventoryLocation::Belt(0),
                },
                TerminalItemPlacement {
                    item_uid: uid(30),
                    source: TerminalInventoryLocation::RunBackpack(0),
                    destination: TerminalInventoryLocation::Belt(0),
                },
                TerminalItemPlacement {
                    item_uid: uid(31),
                    source: TerminalInventoryLocation::RunBackpack(0),
                    destination: TerminalInventoryLocation::CharacterSafe(0),
                },
                TerminalItemPlacement {
                    item_uid: uid(32),
                    source: TerminalInventoryLocation::RunBackpack(0),
                    destination: TerminalInventoryLocation::Vault(0),
                },
                TerminalItemPlacement {
                    item_uid: uid(33),
                    source: TerminalInventoryLocation::RunBackpack(0),
                    destination: TerminalInventoryLocation::CharacterSafe(1),
                },
                TerminalItemPlacement {
                    item_uid: uid(40),
                    source: TerminalInventoryLocation::RunBackpack(1),
                    destination: TerminalInventoryLocation::CharacterSafe(2),
                },
            ]
        );
        assert_eq!(plan.post_inventory_version, 8);
        assert_eq!(plan.post_account_version, 5);
        assert_eq!(plan.accepted_item_count(), 5);
    }

    #[test]
    fn full_safe_and_overflow_storage_uses_hold_without_item_loss() {
        let mut snapshot = empty_snapshot();
        for (index, slot) in snapshot.character_safe.iter_mut().enumerate() {
            *slot = equipment(100 + index as u128);
        }
        for (index, slot) in snapshot.vault.iter_mut().enumerate() {
            *slot = equipment(1_000 + index as u128);
        }
        for (index, slot) in snapshot.overflow.iter_mut().enumerate() {
            *slot = equipment(2_000 + index as u128);
        }
        for index in 0..RUN_BACKPACK_CAPACITY {
            snapshot.run_backpack[index] = equipment(3_000 + index as u128);
        }

        let plan = plan_successful_extraction(&snapshot).unwrap();
        assert!(plan.resolution_required());
        assert_eq!(plan.accepted_item_count(), RUN_BACKPACK_CAPACITY);
        assert_eq!(
            plan.placements
                .iter()
                .filter(|placement| matches!(
                    placement.destination,
                    TerminalInventoryLocation::ResolutionHold(_)
                ))
                .count(),
            RUN_BACKPACK_CAPACITY
        );
        assert!(
            plan.post_resolution_hold
                .iter()
                .all(|slot| !matches!(slot, DurableStorageSlot::Empty))
        );
    }

    #[test]
    fn overflow_uses_lowest_empty_slot_and_never_merges_existing_stacks() {
        let mut snapshot = empty_snapshot();
        for (index, slot) in snapshot.character_safe.iter_mut().enumerate() {
            *slot = equipment(100 + index as u128);
        }
        for (index, slot) in snapshot.vault.iter_mut().enumerate() {
            *slot = equipment(1_000 + index as u128);
        }
        snapshot.overflow[0] = tonic(&[10, 11]);
        snapshot.run_backpack[0] = tonic(&[20, 21]);

        let plan = plan_successful_extraction(&snapshot).unwrap();
        assert_eq!(
            plan.placements
                .iter()
                .filter(|placement| matches!(
                    placement.source,
                    TerminalInventoryLocation::RunBackpack(0)
                ))
                .map(|placement| placement.destination)
                .collect::<Vec<_>>(),
            [
                TerminalInventoryLocation::Overflow(1),
                TerminalInventoryLocation::Overflow(1),
            ]
        );
        assert_eq!(plan.post_overflow[0], tonic(&[10, 11]));
        assert_eq!(plan.post_overflow[1], tonic(&[20, 21]));
    }

    #[test]
    fn material_credits_are_byte_sorted_versioned_and_conserved() {
        let mut snapshot = empty_snapshot();
        snapshot.materials = vec![
            TerminalMaterialSnapshot {
                material_id: "material.salt".to_owned(),
                safe_quantity: 4,
                pending_quantity: 2,
                wallet_cap: 99,
                wallet_version: 8,
                pouch_version: 3,
            },
            TerminalMaterialSnapshot {
                material_id: "material.brass".to_owned(),
                safe_quantity: 10,
                pending_quantity: 5,
                wallet_cap: 999,
                wallet_version: 2,
                pouch_version: 7,
            },
        ];
        let plan = plan_successful_extraction(&snapshot).unwrap();
        assert_eq!(
            plan.material_credits
                .iter()
                .map(|credit| credit.material_id.as_str())
                .collect::<Vec<_>>(),
            ["material.brass", "material.salt"]
        );
        assert_eq!(
            (
                plan.material_credits[0].pre_safe_quantity,
                plan.material_credits[0].credited_quantity,
                plan.material_credits[0].post_safe_quantity,
                plan.material_credits[0].pre_wallet_version,
                plan.material_credits[0].post_wallet_version,
                plan.material_credits[0].pre_pouch_version,
                plan.material_credits[0].post_pouch_version,
            ),
            (10, 5, 15, 2, 3, 7, 8)
        );
        assert_eq!(plan.post_account_version, 5);
        assert_eq!(plan.post_inventory_version, 7);
    }

    #[test]
    fn invalid_capacity_slot_kind_uid_order_and_duplicates_fail_closed() {
        let mut snapshot = empty_snapshot();
        snapshot.overflow.pop();
        assert_eq!(
            plan_successful_extraction(&snapshot),
            Err(TerminalInventoryError::InvalidCapacity)
        );

        let mut snapshot = empty_snapshot();
        snapshot.equipped[0] = tonic(&[1]);
        assert_eq!(
            plan_successful_extraction(&snapshot),
            Err(TerminalInventoryError::InvalidSlotKind)
        );

        let mut snapshot = empty_snapshot();
        snapshot.run_backpack[0] = tonic(&[2, 1]);
        assert_eq!(
            plan_successful_extraction(&snapshot),
            Err(TerminalInventoryError::InvalidConsumableStack)
        );

        let mut snapshot = empty_snapshot();
        snapshot.equipped[0] = equipment(1);
        snapshot.run_backpack[0] = equipment(1);
        assert_eq!(
            plan_successful_extraction(&snapshot),
            Err(TerminalInventoryError::DuplicateItemUid)
        );
    }

    #[test]
    fn unresolved_hold_and_invalid_materials_block_planning() {
        let mut snapshot = empty_snapshot();
        snapshot.resolution_hold[0] = equipment(1);
        assert_eq!(
            plan_successful_extraction(&snapshot),
            Err(TerminalInventoryError::UnresolvedResolutionHold)
        );

        let mut snapshot = empty_snapshot();
        snapshot.materials.push(TerminalMaterialSnapshot {
            material_id: "material.brass".to_owned(),
            safe_quantity: 998,
            pending_quantity: 2,
            wallet_cap: 999,
            wallet_version: 1,
            pouch_version: 1,
        });
        assert_eq!(
            plan_successful_extraction(&snapshot),
            Err(TerminalInventoryError::InvalidMaterial)
        );
    }

    #[test]
    fn arithmetic_overflow_is_rejected_before_any_plan_is_returned() {
        let mut snapshot = empty_snapshot();
        snapshot.committed_at_unix_micros = u64::MAX;
        assert_eq!(
            plan_successful_extraction(&snapshot),
            Err(TerminalInventoryError::ArithmeticOverflow)
        );

        let mut snapshot = empty_snapshot();
        snapshot.run_backpack[0] = equipment(1);
        snapshot.inventory_version = u64::MAX;
        assert_eq!(
            plan_successful_extraction(&snapshot),
            Err(TerminalInventoryError::ArithmeticOverflow)
        );
    }
}
