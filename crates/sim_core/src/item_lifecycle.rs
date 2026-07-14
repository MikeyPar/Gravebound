use crate::{EQUIPMENT_SLOT_COUNT, EquipmentRarity, EquipmentSlot};
use thiserror::Error;

pub const ITEM_UID_BYTES: usize = 16;
pub const ITEM_UID_CONTEXT: &str = "gravebound.item-uid.v1";
pub const STARTER_UID_CONTEXT: &str = "gravebound.starter-init.v1";
pub const RUN_BACKPACK_CAPACITY: usize = 8;
pub const DURABLE_CONSUMABLE_STACK_CAP: u16 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemUid([u8; ITEM_UID_BYTES]);

impl ItemUid {
    pub fn new(bytes: [u8; ITEM_UID_BYTES]) -> Result<Self, ItemLifecycleError> {
        if bytes == [0; ITEM_UID_BYTES] {
            return Err(ItemLifecycleError::ZeroItemUid);
        }
        Ok(Self(bytes))
    }

    #[must_use]
    pub const fn bytes(self) -> [u8; ITEM_UID_BYTES] {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunBackpackSlot {
    Empty,
    Equipment,
    Consumable { template_id: String, quantity: u16 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StackPlacement {
    pub slot_index: u8,
    pub quantity: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumablePlacementPlan {
    pub backpack: Vec<StackPlacement>,
    pub personal_ground_quantity: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipmentPlacementPlan {
    RunBackpack { slot_index: u8 },
    PersonalGround,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableEquipmentItem {
    pub item_uid: ItemUid,
    pub template_id: String,
    pub legal_slot: EquipmentSlot,
    pub item_level: u8,
    pub rarity: EquipmentRarity,
    pub item_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DurableRunBackpackSlot {
    Empty,
    Equipment(DurableEquipmentItem),
    Consumable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldEquipmentSnapshot {
    pub inventory_version: u64,
    pub equipped: [Option<DurableEquipmentItem>; EQUIPMENT_SLOT_COUNT],
    pub backpack: [DurableRunBackpackSlot; RUN_BACKPACK_CAPACITY],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldEquipmentSource {
    RunBackpack {
        slot_index: u8,
    },
    PersonalGround {
        item: DurableEquipmentItem,
        pickup_id: [u8; 16],
        expires_at_tick: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplacementDestination {
    None,
    RunBackpack { slot_index: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldEquipmentPreview {
    pub inventory_version: u64,
    pub content_revision: String,
    pub source: FieldEquipmentSource,
    pub incoming: DurableEquipmentItem,
    pub replaced: Option<DurableEquipmentItem>,
    pub replacement_destination: ReplacementDestination,
    pub preview_hash: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ItemLifecycleError {
    #[error("item UID cannot be all zero")]
    ZeroItemUid,
    #[error("UID derivation field exceeds the canonical u32 byte length")]
    DerivationFieldTooLong,
    #[error("item template ID cannot be empty")]
    EmptyTemplateId,
    #[error("consumable reward quantity must be nonzero")]
    ZeroRewardQuantity,
    #[error("RunBackpack must contain exactly eight slots")]
    InvalidRunBackpackCapacity,
    #[error("stored consumable stack quantity is outside 1..=6")]
    InvalidConsumableStack,
    #[error("field equipment inventory version must be positive")]
    InvalidInventoryVersion,
    #[error("field equipment content revision cannot be empty")]
    EmptyContentRevision,
    #[error("field equipment template ID cannot be empty")]
    EmptyEquipmentTemplateId,
    #[error("field equipment item level is outside 1..=20")]
    InvalidEquipmentItemLevel,
    #[error("field equipment item version must be positive")]
    InvalidEquipmentItemVersion,
    #[error("RunBackpack source index is outside 0..=7")]
    RunBackpackSourceOutOfRange,
    #[error("RunBackpack source does not contain equipment")]
    RunBackpackSourceNotEquipment,
    #[error("personal-ground pickup identity cannot be all zero")]
    ZeroPersonalGroundPickupId,
    #[error("personal-ground item has already expired")]
    PersonalGroundExpired,
    #[error("PersonalGround equipment swap requires an empty RunBackpack index")]
    BackpackFullForPersonalGroundSwap,
    #[error("equipment item is stored in an illegal equipped slot")]
    IllegalEquippedSlot,
    #[error("equipment item identity occurs more than once in the snapshot")]
    DuplicateEquipmentItem,
    #[error("field equipment preview no longer matches authoritative state")]
    StaleEquipmentPreview,
}

pub fn plan_field_equipment_swap(
    snapshot: &FieldEquipmentSnapshot,
    source: FieldEquipmentSource,
    content_revision: &str,
    now_tick: u64,
) -> Result<FieldEquipmentPreview, ItemLifecycleError> {
    validate_field_snapshot(snapshot)?;
    if content_revision.is_empty() {
        return Err(ItemLifecycleError::EmptyContentRevision);
    }
    let incoming = match &source {
        FieldEquipmentSource::RunBackpack { slot_index } => {
            let slot = snapshot
                .backpack
                .get(usize::from(*slot_index))
                .ok_or(ItemLifecycleError::RunBackpackSourceOutOfRange)?;
            let DurableRunBackpackSlot::Equipment(item) = slot else {
                return Err(ItemLifecycleError::RunBackpackSourceNotEquipment);
            };
            item.clone()
        }
        FieldEquipmentSource::PersonalGround {
            item,
            pickup_id,
            expires_at_tick,
        } => {
            validate_equipment_item(item)?;
            if *pickup_id == [0; 16] {
                return Err(ItemLifecycleError::ZeroPersonalGroundPickupId);
            }
            if now_tick >= *expires_at_tick {
                return Err(ItemLifecycleError::PersonalGroundExpired);
            }
            if snapshot_contains(snapshot, item.item_uid) {
                return Err(ItemLifecycleError::DuplicateEquipmentItem);
            }
            item.clone()
        }
    };
    let replaced = snapshot.equipped[incoming.legal_slot.index()].clone();
    let replacement_destination = match (&source, &replaced) {
        (_, None) => ReplacementDestination::None,
        (FieldEquipmentSource::RunBackpack { slot_index }, Some(_)) => {
            ReplacementDestination::RunBackpack {
                slot_index: *slot_index,
            }
        }
        (FieldEquipmentSource::PersonalGround { .. }, Some(_)) => {
            let index = snapshot
                .backpack
                .iter()
                .position(|slot| matches!(slot, DurableRunBackpackSlot::Empty))
                .ok_or(ItemLifecycleError::BackpackFullForPersonalGroundSwap)?;
            ReplacementDestination::RunBackpack {
                slot_index: u8::try_from(index)
                    .map_err(|_| ItemLifecycleError::InvalidRunBackpackCapacity)?,
            }
        }
    };
    let preview_hash = field_equipment_preview_hash(
        snapshot.inventory_version,
        content_revision,
        &source,
        &incoming,
        replaced.as_ref(),
        replacement_destination,
    )?;
    Ok(FieldEquipmentPreview {
        inventory_version: snapshot.inventory_version,
        content_revision: content_revision.to_owned(),
        source,
        incoming,
        replaced,
        replacement_destination,
        preview_hash,
    })
}

pub fn apply_field_equipment_preview(
    snapshot: &FieldEquipmentSnapshot,
    preview: &FieldEquipmentPreview,
    now_tick: u64,
) -> Result<FieldEquipmentSnapshot, ItemLifecycleError> {
    let expected = plan_field_equipment_swap(
        snapshot,
        preview.source.clone(),
        &preview.content_revision,
        now_tick,
    )?;
    if &expected != preview {
        return Err(ItemLifecycleError::StaleEquipmentPreview);
    }
    let mut next = snapshot.clone();
    match preview.source {
        FieldEquipmentSource::RunBackpack { slot_index } => {
            next.backpack[usize::from(slot_index)] = preview.replaced.clone().map_or(
                DurableRunBackpackSlot::Empty,
                DurableRunBackpackSlot::Equipment,
            );
        }
        FieldEquipmentSource::PersonalGround { .. } => {
            if let ReplacementDestination::RunBackpack { slot_index } =
                preview.replacement_destination
            {
                next.backpack[usize::from(slot_index)] = DurableRunBackpackSlot::Equipment(
                    preview
                        .replaced
                        .clone()
                        .ok_or(ItemLifecycleError::StaleEquipmentPreview)?,
                );
            }
        }
    }
    next.equipped[preview.incoming.legal_slot.index()] = Some(preview.incoming.clone());
    next.inventory_version = next
        .inventory_version
        .checked_add(1)
        .ok_or(ItemLifecycleError::InvalidInventoryVersion)?;
    validate_field_snapshot(&next)?;
    Ok(next)
}

fn validate_field_snapshot(snapshot: &FieldEquipmentSnapshot) -> Result<(), ItemLifecycleError> {
    if snapshot.inventory_version == 0 {
        return Err(ItemLifecycleError::InvalidInventoryVersion);
    }
    let mut identities = Vec::new();
    for (index, item) in snapshot.equipped.iter().enumerate() {
        if let Some(item) = item {
            validate_equipment_item(item)?;
            if item.legal_slot.index() != index {
                return Err(ItemLifecycleError::IllegalEquippedSlot);
            }
            identities.push(item.item_uid);
        }
    }
    for slot in &snapshot.backpack {
        if let DurableRunBackpackSlot::Equipment(item) = slot {
            validate_equipment_item(item)?;
            identities.push(item.item_uid);
        }
    }
    identities.sort_unstable();
    if identities.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(ItemLifecycleError::DuplicateEquipmentItem);
    }
    Ok(())
}

fn validate_equipment_item(item: &DurableEquipmentItem) -> Result<(), ItemLifecycleError> {
    if item.template_id.is_empty() {
        return Err(ItemLifecycleError::EmptyEquipmentTemplateId);
    }
    if !(1..=20).contains(&item.item_level) {
        return Err(ItemLifecycleError::InvalidEquipmentItemLevel);
    }
    if item.item_version == 0 {
        return Err(ItemLifecycleError::InvalidEquipmentItemVersion);
    }
    Ok(())
}

fn snapshot_contains(snapshot: &FieldEquipmentSnapshot, uid: ItemUid) -> bool {
    snapshot
        .equipped
        .iter()
        .flatten()
        .any(|item| item.item_uid == uid)
        || snapshot.backpack.iter().any(
            |slot| matches!(slot, DurableRunBackpackSlot::Equipment(item) if item.item_uid == uid),
        )
}

fn field_equipment_preview_hash(
    inventory_version: u64,
    content_revision: &str,
    source: &FieldEquipmentSource,
    incoming: &DurableEquipmentItem,
    replaced: Option<&DurableEquipmentItem>,
    destination: ReplacementDestination,
) -> Result<[u8; 32], ItemLifecycleError> {
    let mut material = Vec::new();
    push_hash_field(&mut material, &inventory_version.to_le_bytes())?;
    push_hash_field(&mut material, content_revision.as_bytes())?;
    match source {
        FieldEquipmentSource::RunBackpack { slot_index } => {
            push_hash_field(&mut material, &[0, *slot_index])?;
        }
        FieldEquipmentSource::PersonalGround {
            pickup_id,
            expires_at_tick,
            ..
        } => {
            push_hash_field(&mut material, &[1])?;
            push_hash_field(&mut material, pickup_id)?;
            push_hash_field(&mut material, &expires_at_tick.to_le_bytes())?;
        }
    }
    push_equipment_hash_fields(&mut material, incoming)?;
    if let Some(item) = replaced {
        push_hash_field(&mut material, &[1])?;
        push_equipment_hash_fields(&mut material, item)?;
    } else {
        push_hash_field(&mut material, &[0])?;
    }
    match destination {
        ReplacementDestination::None => push_hash_field(&mut material, &[0])?,
        ReplacementDestination::RunBackpack { slot_index } => {
            push_hash_field(&mut material, &[1, slot_index])?;
        }
    }
    Ok(blake3::derive_key(
        "gravebound.field-equipment-preview.v1",
        &material,
    ))
}

fn push_equipment_hash_fields(
    material: &mut Vec<u8>,
    item: &DurableEquipmentItem,
) -> Result<(), ItemLifecycleError> {
    push_hash_field(material, &item.item_uid.bytes())?;
    push_hash_field(material, item.template_id.as_bytes())?;
    let rarity = match item.rarity {
        EquipmentRarity::Worn => 0,
        EquipmentRarity::Forged => 1,
        EquipmentRarity::Oathed => 2,
        EquipmentRarity::Relic => 3,
        EquipmentRarity::Sainted => 4,
        EquipmentRarity::BlackUnique => 5,
    };
    push_hash_field(material, &[item.legal_slot as u8, item.item_level, rarity])?;
    push_hash_field(material, &item.item_version.to_le_bytes())
}

fn push_hash_field(material: &mut Vec<u8>, field: &[u8]) -> Result<(), ItemLifecycleError> {
    let length =
        u32::try_from(field.len()).map_err(|_| ItemLifecycleError::DerivationFieldTooLong)?;
    material.extend_from_slice(&length.to_le_bytes());
    material.extend_from_slice(field);
    Ok(())
}

pub fn derive_reward_item_uid(
    reward_request_id: [u8; 16],
    roll_index: u16,
    unit_ordinal: u16,
) -> Result<ItemUid, ItemLifecycleError> {
    derive_uid(
        ITEM_UID_CONTEXT,
        &[
            reward_request_id.as_slice(),
            roll_index.to_le_bytes().as_slice(),
            unit_ordinal.to_le_bytes().as_slice(),
        ],
    )
}

pub fn derive_starter_item_uid(
    character_id: [u8; 16],
    initializer_revision: &str,
    template_id: &str,
    unit_ordinal: u16,
) -> Result<ItemUid, ItemLifecycleError> {
    if initializer_revision.is_empty() || template_id.is_empty() {
        return Err(ItemLifecycleError::EmptyTemplateId);
    }
    derive_uid(
        STARTER_UID_CONTEXT,
        &[
            character_id.as_slice(),
            initializer_revision.as_bytes(),
            template_id.as_bytes(),
            unit_ordinal.to_le_bytes().as_slice(),
        ],
    )
}

fn derive_uid(context: &str, fields: &[&[u8]]) -> Result<ItemUid, ItemLifecycleError> {
    let mut material = Vec::new();
    for field in fields {
        let length =
            u32::try_from(field.len()).map_err(|_| ItemLifecycleError::DerivationFieldTooLong)?;
        material.extend_from_slice(&length.to_le_bytes());
        material.extend_from_slice(field);
    }
    let derived = blake3::derive_key(context, &material);
    let mut uid = [0; ITEM_UID_BYTES];
    uid.copy_from_slice(&derived[..ITEM_UID_BYTES]);
    ItemUid::new(uid)
}

pub fn plan_consumable_reward_placement(
    slots: &[RunBackpackSlot],
    template_id: &str,
    quantity: u16,
) -> Result<ConsumablePlacementPlan, ItemLifecycleError> {
    validate_slots(slots)?;
    if template_id.is_empty() {
        return Err(ItemLifecycleError::EmptyTemplateId);
    }
    if quantity == 0 {
        return Err(ItemLifecycleError::ZeroRewardQuantity);
    }

    let mut remaining = quantity;
    let mut backpack = Vec::new();
    for (index, slot) in slots.iter().enumerate() {
        let RunBackpackSlot::Consumable {
            template_id: stored_template,
            quantity: stored_quantity,
        } = slot
        else {
            continue;
        };
        if stored_template == template_id && *stored_quantity < DURABLE_CONSUMABLE_STACK_CAP {
            let placed = remaining.min(DURABLE_CONSUMABLE_STACK_CAP - stored_quantity);
            backpack.push(StackPlacement {
                slot_index: u8::try_from(index)
                    .map_err(|_| ItemLifecycleError::InvalidRunBackpackCapacity)?,
                quantity: placed,
            });
            remaining -= placed;
            if remaining == 0 {
                break;
            }
        }
    }
    if remaining > 0 {
        for (index, slot) in slots.iter().enumerate() {
            if !matches!(slot, RunBackpackSlot::Empty) {
                continue;
            }
            let placed = remaining.min(DURABLE_CONSUMABLE_STACK_CAP);
            backpack.push(StackPlacement {
                slot_index: u8::try_from(index)
                    .map_err(|_| ItemLifecycleError::InvalidRunBackpackCapacity)?,
                quantity: placed,
            });
            remaining -= placed;
            if remaining == 0 {
                break;
            }
        }
    }
    Ok(ConsumablePlacementPlan {
        backpack,
        personal_ground_quantity: remaining,
    })
}

pub fn plan_equipment_reward_placement(
    slots: &[RunBackpackSlot],
) -> Result<EquipmentPlacementPlan, ItemLifecycleError> {
    validate_slots(slots)?;
    let Some(index) = slots
        .iter()
        .position(|slot| matches!(slot, RunBackpackSlot::Empty))
    else {
        return Ok(EquipmentPlacementPlan::PersonalGround);
    };
    Ok(EquipmentPlacementPlan::RunBackpack {
        slot_index: u8::try_from(index)
            .map_err(|_| ItemLifecycleError::InvalidRunBackpackCapacity)?,
    })
}

fn validate_slots(slots: &[RunBackpackSlot]) -> Result<(), ItemLifecycleError> {
    if slots.len() != RUN_BACKPACK_CAPACITY {
        return Err(ItemLifecycleError::InvalidRunBackpackCapacity);
    }
    for slot in slots {
        if let RunBackpackSlot::Consumable {
            template_id,
            quantity,
        } = slot
        {
            if template_id.is_empty() {
                return Err(ItemLifecycleError::EmptyTemplateId);
            }
            if !(1..=DURABLE_CONSUMABLE_STACK_CAP).contains(quantity) {
                return Err(ItemLifecycleError::InvalidConsumableStack);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_backpack() -> Vec<RunBackpackSlot> {
        vec![RunBackpackSlot::Empty; RUN_BACKPACK_CAPACITY]
    }

    fn equipment(byte: u8, template: &str, legal_slot: EquipmentSlot) -> DurableEquipmentItem {
        DurableEquipmentItem {
            item_uid: ItemUid::new([byte; 16]).unwrap(),
            template_id: template.to_owned(),
            legal_slot,
            item_level: 1,
            rarity: EquipmentRarity::Forged,
            item_version: 1,
        }
    }

    fn field_snapshot() -> FieldEquipmentSnapshot {
        FieldEquipmentSnapshot {
            inventory_version: 7,
            equipped: [
                Some(equipment(
                    1,
                    "item.weapon.crossbow.pine_crossbow",
                    EquipmentSlot::Weapon,
                )),
                Some(equipment(
                    2,
                    "item.relic.arbalist.cracked_mark_lens",
                    EquipmentSlot::Relic,
                )),
                None,
                None,
            ],
            backpack: std::array::from_fn(|_| DurableRunBackpackSlot::Empty),
        }
    }

    #[test]
    fn uid_derivation_is_domain_separated_framed_and_stable() {
        let request = [0x11; 16];
        let reward = derive_reward_item_uid(request, 0x2233, 0x4455).unwrap();
        let starter = derive_starter_item_uid(
            request,
            "starter.core-dev.v1",
            "consumable.red_tonic",
            0x4455,
        )
        .unwrap();
        assert_eq!(
            reward.bytes(),
            [
                0xd3, 0xb3, 0x33, 0xf5, 0xb9, 0xa7, 0xfb, 0x91, 0x48, 0x7f, 0xd2, 0x45, 0xd8, 0x9f,
                0x88, 0x2d,
            ]
        );
        assert_ne!(reward, starter);
        assert_ne!(
            reward,
            derive_reward_item_uid(request, 0x2233, 0x4454).unwrap()
        );
    }

    #[test]
    fn tonic_rewards_merge_then_fill_lowest_empty_slots() {
        let mut slots = empty_backpack();
        slots[0] = RunBackpackSlot::Consumable {
            template_id: "consumable.red_tonic".to_owned(),
            quantity: 5,
        };
        slots[1] = RunBackpackSlot::Equipment;
        slots[2] = RunBackpackSlot::Consumable {
            template_id: "consumable.red_tonic".to_owned(),
            quantity: 3,
        };
        assert_eq!(
            plan_consumable_reward_placement(&slots, "consumable.red_tonic", 6).unwrap(),
            ConsumablePlacementPlan {
                backpack: vec![
                    StackPlacement {
                        slot_index: 0,
                        quantity: 1,
                    },
                    StackPlacement {
                        slot_index: 2,
                        quantity: 3,
                    },
                    StackPlacement {
                        slot_index: 3,
                        quantity: 2,
                    },
                ],
                personal_ground_quantity: 0,
            }
        );
    }

    #[test]
    fn overflow_remains_whole_and_personal_ground_without_using_belt() {
        let slots = vec![
            RunBackpackSlot::Consumable {
                template_id: "consumable.red_tonic".to_owned(),
                quantity: DURABLE_CONSUMABLE_STACK_CAP,
            };
            RUN_BACKPACK_CAPACITY
        ];
        assert_eq!(
            plan_consumable_reward_placement(&slots, "consumable.red_tonic", 2).unwrap(),
            ConsumablePlacementPlan {
                backpack: Vec::new(),
                personal_ground_quantity: 2,
            }
        );
        assert_eq!(
            plan_equipment_reward_placement(&slots).unwrap(),
            EquipmentPlacementPlan::PersonalGround
        );
    }

    #[test]
    fn planners_reject_corrupt_projections() {
        let mut slots = empty_backpack();
        slots[0] = RunBackpackSlot::Consumable {
            template_id: "consumable.red_tonic".to_owned(),
            quantity: 7,
        };
        assert_eq!(
            plan_consumable_reward_placement(&slots, "consumable.red_tonic", 1),
            Err(ItemLifecycleError::InvalidConsumableStack)
        );
        assert_eq!(
            plan_equipment_reward_placement(&slots[..7]),
            Err(ItemLifecycleError::InvalidRunBackpackCapacity)
        );
    }

    #[test]
    fn backpack_swap_uses_the_exact_vacated_source_even_when_every_slot_is_full() {
        let mut snapshot = field_snapshot();
        for index in 0..RUN_BACKPACK_CAPACITY {
            snapshot.backpack[index] = DurableRunBackpackSlot::Consumable;
        }
        let incoming = equipment(
            3,
            "item.weapon.crossbow.grave_repeater",
            EquipmentSlot::Weapon,
        );
        snapshot.backpack[5] = DurableRunBackpackSlot::Equipment(incoming.clone());
        let preview = plan_field_equipment_swap(
            &snapshot,
            FieldEquipmentSource::RunBackpack { slot_index: 5 },
            "core-dev.blake3.test",
            100,
        )
        .unwrap();
        assert_eq!(preview.incoming, incoming);
        assert_eq!(
            preview.replacement_destination,
            ReplacementDestination::RunBackpack { slot_index: 5 }
        );
        let next = apply_field_equipment_preview(&snapshot, &preview, 100).unwrap();
        assert_eq!(next.inventory_version, 8);
        assert_eq!(next.equipped[0], Some(incoming));
        assert!(matches!(
            &next.backpack[5],
            DurableRunBackpackSlot::Equipment(item) if item.item_uid == ItemUid::new([1; 16]).unwrap()
        ));
    }

    #[test]
    fn personal_ground_swap_uses_lowest_empty_slot_and_rejects_a_full_backpack_atomically() {
        let mut snapshot = field_snapshot();
        snapshot.backpack[0] = DurableRunBackpackSlot::Consumable;
        snapshot.backpack[1] = DurableRunBackpackSlot::Consumable;
        let source = FieldEquipmentSource::PersonalGround {
            item: equipment(4, "item.relic.arbalist.long_lens", EquipmentSlot::Relic),
            pickup_id: [9; 16],
            expires_at_tick: 200,
        };
        let preview =
            plan_field_equipment_swap(&snapshot, source.clone(), "core-dev.blake3.test", 100)
                .unwrap();
        assert_eq!(
            preview.replacement_destination,
            ReplacementDestination::RunBackpack { slot_index: 2 }
        );
        let next = apply_field_equipment_preview(&snapshot, &preview, 100).unwrap();
        assert!(
            matches!(&next.backpack[2], DurableRunBackpackSlot::Equipment(item) if item.item_uid == ItemUid::new([2; 16]).unwrap())
        );

        let mut full = snapshot.clone();
        full.backpack.fill(DurableRunBackpackSlot::Consumable);
        assert_eq!(
            plan_field_equipment_swap(&full, source, "core-dev.blake3.test", 100),
            Err(ItemLifecycleError::BackpackFullForPersonalGroundSwap)
        );
        assert_eq!(full, {
            let mut unchanged = snapshot;
            unchanged.backpack.fill(DurableRunBackpackSlot::Consumable);
            unchanged
        });
    }

    #[test]
    fn preview_hash_binds_version_source_item_destination_and_content() {
        let mut snapshot = field_snapshot();
        snapshot.backpack[3] = DurableRunBackpackSlot::Equipment(equipment(
            5,
            "item.charm.ember_tooth.t1",
            EquipmentSlot::Charm,
        ));
        let preview = plan_field_equipment_swap(
            &snapshot,
            FieldEquipmentSource::RunBackpack { slot_index: 3 },
            "core-dev.blake3.test",
            0,
        )
        .unwrap();
        assert_eq!(
            preview.preview_hash,
            plan_field_equipment_swap(
                &snapshot,
                FieldEquipmentSource::RunBackpack { slot_index: 3 },
                "core-dev.blake3.test",
                999,
            )
            .unwrap()
            .preview_hash
        );
        let mut tampered = preview.clone();
        tampered.preview_hash[0] ^= 1;
        assert_eq!(
            apply_field_equipment_preview(&snapshot, &tampered, 0),
            Err(ItemLifecycleError::StaleEquipmentPreview)
        );
        let mut stale = snapshot;
        stale.inventory_version += 1;
        assert_eq!(
            apply_field_equipment_preview(&stale, &preview, 0),
            Err(ItemLifecycleError::StaleEquipmentPreview)
        );
    }

    #[test]
    fn corrupt_and_expired_sources_fail_closed() {
        let snapshot = field_snapshot();
        assert_eq!(
            plan_field_equipment_swap(
                &snapshot,
                FieldEquipmentSource::RunBackpack { slot_index: 8 },
                "core-dev.blake3.test",
                0,
            ),
            Err(ItemLifecycleError::RunBackpackSourceOutOfRange)
        );
        assert_eq!(
            plan_field_equipment_swap(
                &snapshot,
                FieldEquipmentSource::PersonalGround {
                    item: equipment(8, "item.armor.pilgrim.t1", EquipmentSlot::Armor),
                    pickup_id: [8; 16],
                    expires_at_tick: 90,
                },
                "core-dev.blake3.test",
                90,
            ),
            Err(ItemLifecycleError::PersonalGroundExpired)
        );
    }
}
