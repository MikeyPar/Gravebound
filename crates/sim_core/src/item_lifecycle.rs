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
}
