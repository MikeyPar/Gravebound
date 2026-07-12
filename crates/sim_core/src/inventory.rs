use std::{array, num::NonZeroU64};

use thiserror::Error;

use crate::{BeltSlot, RED_TONIC_STACK_CAP, SimulationVector, Tick, TonicBelt};

pub const EQUIPMENT_SLOT_COUNT: usize = 4;
pub const PROTOTYPE_BACKPACK_CAPACITY: usize = 8;
pub const FIELD_PICKUP_LIFETIME_TICKS: u64 = 60 * 30;
pub const AUTOMATIC_PICKUP_RADIUS_TILES: f32 = 0.75;
pub const INTERACT_PICKUP_RADIUS_TILES: f32 = 1.25;

/// Stable item-instance identity within one local prototype run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemInstanceId(NonZeroU64);

impl ItemInstanceId {
    pub fn new(value: u64) -> Result<Self, InventoryError> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(InventoryError::ZeroItemInstanceId)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Stable field-pickup identity within one local prototype run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldPickupId(NonZeroU64);

impl FieldPickupId {
    pub fn new(value: u64) -> Result<Self, InventoryError> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(InventoryError::ZeroFieldPickupId)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Nonempty stable content identity. Content validation owns the allowlist and payload.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemContentId(String);

impl ItemContentId {
    pub fn new(value: impl Into<String>) -> Result<Self, InventoryError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(InventoryError::EmptyItemContentId);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The exact four First Playable equipment slots in deterministic index order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum EquipmentSlot {
    Weapon = 0,
    Relic = 1,
    Armor = 2,
    Charm = 3,
}

impl EquipmentSlot {
    pub const ALL: [Self; EQUIPMENT_SLOT_COUNT] =
        [Self::Weapon, Self::Relic, Self::Armor, Self::Charm];

    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquipmentItem {
    instance_id: ItemInstanceId,
    content_id: ItemContentId,
    legal_slot: EquipmentSlot,
}

impl EquipmentItem {
    #[must_use]
    pub const fn new(
        instance_id: ItemInstanceId,
        content_id: ItemContentId,
        legal_slot: EquipmentSlot,
    ) -> Self {
        Self {
            instance_id,
            content_id,
            legal_slot,
        }
    }

    #[must_use]
    pub const fn instance_id(&self) -> ItemInstanceId {
        self.instance_id
    }

    #[must_use]
    pub const fn content_id(&self) -> &ItemContentId {
        &self.content_id
    }

    #[must_use]
    pub const fn legal_slot(&self) -> EquipmentSlot {
        self.legal_slot
    }
}

/// One of the eight nonbelt prototype backpack stacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InventoryStack {
    Equipment(EquipmentItem),
    RedTonic {
        instance_id: ItemInstanceId,
        quantity: u8,
    },
}

impl InventoryStack {
    pub fn red_tonic(instance_id: ItemInstanceId, quantity: u8) -> Result<Self, InventoryError> {
        if quantity == 0 || quantity > RED_TONIC_STACK_CAP {
            return Err(InventoryError::InvalidTonicStackQuantity(quantity));
        }
        Ok(Self::RedTonic {
            instance_id,
            quantity,
        })
    }

    #[must_use]
    pub const fn instance_id(&self) -> ItemInstanceId {
        match self {
            Self::Equipment(item) => item.instance_id(),
            Self::RedTonic { instance_id, .. } => *instance_id,
        }
    }

    #[must_use]
    pub const fn equipment(&self) -> Option<&EquipmentItem> {
        match self {
            Self::Equipment(item) => Some(item),
            Self::RedTonic { .. } => None,
        }
    }

    #[must_use]
    pub const fn tonic_quantity(&self) -> Option<u8> {
        match self {
            Self::RedTonic { quantity, .. } => Some(*quantity),
            Self::Equipment(_) => None,
        }
    }
}

/// Local personal ground pickup with the exact capacity-blocked lifetime.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldPickup {
    pickup_id: FieldPickupId,
    stack: InventoryStack,
    position: SimulationVector,
    spawned_at: Tick,
    expires_at: Tick,
    collected: bool,
}

impl FieldPickup {
    pub fn new(
        pickup_id: FieldPickupId,
        stack: InventoryStack,
        position: SimulationVector,
        spawned_at: Tick,
    ) -> Result<Self, InventoryError> {
        validate_stack(&stack)?;
        if !position.is_finite() {
            return Err(InventoryError::NonFinitePickupPosition);
        }
        let expiry = spawned_at
            .0
            .checked_add(FIELD_PICKUP_LIFETIME_TICKS)
            .ok_or(InventoryError::PickupExpiryOverflow)?;
        Ok(Self {
            pickup_id,
            stack,
            position,
            spawned_at,
            expires_at: Tick(expiry),
            collected: false,
        })
    }

    #[must_use]
    pub const fn pickup_id(&self) -> FieldPickupId {
        self.pickup_id
    }

    #[must_use]
    pub const fn stack(&self) -> &InventoryStack {
        &self.stack
    }

    #[must_use]
    pub const fn position(&self) -> SimulationVector {
        self.position
    }

    #[must_use]
    pub const fn spawned_at(&self) -> Tick {
        self.spawned_at
    }

    #[must_use]
    pub const fn expires_at(&self) -> Tick {
        self.expires_at
    }

    #[must_use]
    pub const fn is_collected(&self) -> bool {
        self.collected
    }

    #[must_use]
    pub const fn is_expired_at(&self, now: Tick) -> bool {
        now.0 >= self.expires_at.0
    }

    #[must_use]
    pub const fn remaining_ticks_at(&self, now: Tick) -> u64 {
        self.expires_at.0.saturating_sub(now.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldPickupAccess {
    Automatic,
    Interact,
}

impl FieldPickupAccess {
    #[must_use]
    pub const fn radius_tiles(self) -> f32 {
        match self {
            Self::Automatic => AUTOMATIC_PICKUP_RADIUS_TILES,
            Self::Interact => INTERACT_PICKUP_RADIUS_TILES,
        }
    }
}

/// Explicit player intent for a field pickup or reward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacementChoice {
    /// Default behavior: place in the first available backpack index.
    Take,
    /// Explicit behavior: equip into the item's authored legal slot.
    Equip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnedItemLocation {
    Equipped(EquipmentSlot),
    Backpack(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewardChoice {
    LeaveReward,
    Take,
    Equip,
    DropExisting {
        location: OwnedItemLocation,
        dropped_pickup_id: FieldPickupId,
        then: PlacementChoice,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrototypeInventory {
    equipped: [Option<EquipmentItem>; EQUIPMENT_SLOT_COUNT],
    backpack: [Option<InventoryStack>; PROTOTYPE_BACKPACK_CAPACITY],
    belt: TonicBelt,
}

impl PrototypeInventory {
    #[must_use]
    pub fn new(belt: TonicBelt) -> Self {
        Self {
            equipped: array::from_fn(|_| None),
            backpack: array::from_fn(|_| None),
            belt,
        }
    }

    #[must_use]
    pub fn first_playable_belt() -> Self {
        Self::new(TonicBelt::first_playable())
    }

    /// Constructs the exact three equipped starter items and two-Tonic belt for one qualified run.
    pub fn first_playable_loadout(run_ordinal: u32) -> Result<Self, InventoryError> {
        if run_ordinal == 0 {
            return Err(InventoryError::ZeroRunOrdinal);
        }
        let instance = |ordinal: u32| {
            let value = u64::from(run_ordinal)
                .checked_shl(32)
                .and_then(|prefix| prefix.checked_add(u64::from(ordinal)))
                .ok_or(InventoryError::ItemInstanceIdOverflow)?;
            ItemInstanceId::new(value)
        };
        let equipped = [
            Some(EquipmentItem::new(
                instance(1)?,
                ItemContentId::new("item.prototype.weapon.pine_crossbow")?,
                EquipmentSlot::Weapon,
            )),
            Some(EquipmentItem::new(
                instance(2)?,
                ItemContentId::new("item.prototype.relic.dented_scope")?,
                EquipmentSlot::Relic,
            )),
            Some(EquipmentItem::new(
                instance(3)?,
                ItemContentId::new("item.prototype.armor.reedcloth_wraps")?,
                EquipmentSlot::Armor,
            )),
            None,
        ];
        let inventory = Self {
            equipped,
            backpack: array::from_fn(|_| None),
            belt: TonicBelt::first_playable(),
        };
        inventory.validate()?;
        Ok(inventory)
    }

    #[must_use]
    pub const fn equipped(&self) -> &[Option<EquipmentItem>; EQUIPMENT_SLOT_COUNT] {
        &self.equipped
    }

    #[must_use]
    pub const fn backpack(&self) -> &[Option<InventoryStack>; PROTOTYPE_BACKPACK_CAPACITY] {
        &self.backpack
    }

    #[must_use]
    pub const fn belt(&self) -> &TonicBelt {
        &self.belt
    }

    #[must_use]
    pub fn equipped_item(&self, slot: EquipmentSlot) -> Option<&EquipmentItem> {
        self.equipped[slot.index()].as_ref()
    }

    #[must_use]
    pub fn backpack_stack(&self, index: usize) -> Option<&InventoryStack> {
        self.backpack.get(index).and_then(Option::as_ref)
    }

    /// Attempts a local field pickup with clone-then-commit transactional behavior.
    pub fn apply_field_pickup(
        &mut self,
        pickup: &mut FieldPickup,
        choice: PlacementChoice,
        player_position: SimulationVector,
        access: FieldPickupAccess,
        now: Tick,
    ) -> Result<PickupOutcome, InventoryError> {
        if !player_position.is_finite() {
            return Err(InventoryError::NonFinitePickupActorPosition);
        }
        let offset = pickup.position - player_position;
        let radius = access.radius_tiles();
        if offset.length_squared() > radius * radius {
            return Err(InventoryError::PickupOutOfReach { access });
        }
        if pickup.is_collected() {
            return Err(InventoryError::PickupAlreadyCollected(pickup.pickup_id()));
        }
        if pickup.is_expired_at(now) {
            return Err(InventoryError::PickupExpired {
                pickup_id: pickup.pickup_id(),
                expired_at: pickup.expires_at(),
                now,
            });
        }
        let mut next_inventory = self.clone();
        let mut next_pickup = pickup.clone();
        next_inventory.validate()?;
        next_inventory.ensure_incoming_unique(next_pickup.stack())?;
        let mut outcome = next_inventory.place_stack(&mut next_pickup.stack, choice, now)?;
        if let PickupOutcome::CapacityBlocked {
            remaining_lifetime_ticks,
        } = &mut outcome
        {
            *remaining_lifetime_ticks = next_pickup.remaining_ticks_at(now);
        }
        if outcome.is_fully_collected() {
            next_pickup.collected = true;
        }
        next_inventory.validate()?;
        if outcome.mutates_inventory() {
            *self = next_inventory;
            *pickup = next_pickup;
        }
        Ok(outcome)
    }

    /// Applies a reward-panel choice using the same placement rules as a field pickup.
    pub fn apply_reward_choice(
        &mut self,
        reward: &mut FieldPickup,
        choice: RewardChoice,
        now: Tick,
    ) -> Result<RewardOutcome, InventoryError> {
        if reward.is_collected() {
            return Err(InventoryError::PickupAlreadyCollected(reward.pickup_id()));
        }
        if reward.is_expired_at(now) {
            return Err(InventoryError::PickupExpired {
                pickup_id: reward.pickup_id(),
                expired_at: reward.expires_at(),
                now,
            });
        }
        if choice == RewardChoice::LeaveReward {
            return Ok(RewardOutcome::LeftReward {
                pickup_id: reward.pickup_id(),
            });
        }

        let mut next_inventory = self.clone();
        let mut next_reward = reward.clone();
        next_inventory.validate()?;
        let mut dropped = None;
        let placement = match choice {
            RewardChoice::LeaveReward => unreachable!("handled above"),
            RewardChoice::Take => PlacementChoice::Take,
            RewardChoice::Equip => PlacementChoice::Equip,
            RewardChoice::DropExisting {
                location,
                dropped_pickup_id,
                then,
            } => {
                let removed = next_inventory.remove_owned(location)?;
                dropped = Some(FieldPickup::new(
                    dropped_pickup_id,
                    removed,
                    reward.position,
                    now,
                )?);
                then
            }
        };
        next_inventory.ensure_incoming_unique(next_reward.stack())?;
        let mut pickup_outcome =
            next_inventory.place_stack(&mut next_reward.stack, placement, now)?;
        if let PickupOutcome::CapacityBlocked {
            remaining_lifetime_ticks,
        } = &mut pickup_outcome
        {
            *remaining_lifetime_ticks = next_reward.remaining_ticks_at(now);
            if dropped.is_none() {
                return Ok(RewardOutcome::CapacityBlocked {
                    pickup_id: next_reward.pickup_id(),
                    remaining_lifetime_ticks: *remaining_lifetime_ticks,
                });
            }
        }
        if dropped.is_some() && !pickup_outcome.is_fully_collected() {
            return Err(InventoryError::DropExistingDidNotMakeCapacity);
        }
        if pickup_outcome.is_fully_collected() {
            next_reward.collected = true;
        }
        next_inventory.validate()?;
        *self = next_inventory;
        *reward = next_reward;
        Ok(RewardOutcome::Collected {
            pickup: pickup_outcome,
            dropped,
        })
    }

    /// Removes every prototype item and Tonic. The caller may then grant a fresh run loadout.
    pub fn clear_for_restart(&mut self) -> RestartCleanup {
        let mut removed_stacks = Vec::new();
        for item in &mut self.equipped {
            if let Some(item) = item.take() {
                removed_stacks.push(InventoryStack::Equipment(item));
            }
        }
        for stack in &mut self.backpack {
            if let Some(stack) = stack.take() {
                removed_stacks.push(stack);
            }
        }
        let cleared_belt_tonics = self
            .belt
            .slots()
            .iter()
            .copied()
            .map(BeltSlot::tonic_count)
            .map(u32::from)
            .sum();
        self.belt = TonicBelt::from_slots([BeltSlot::Empty, BeltSlot::Empty])
            .expect("empty two-slot belt is valid");
        RestartCleanup {
            removed_stacks,
            cleared_belt_tonics,
        }
    }

    /// Applies the inventory portion of Emergency Recall: equipped gear and the belt remain,
    /// while every unsecured backpack stack is destroyed.
    pub fn clear_pending_for_recall(&mut self) -> RecallCleanup {
        let removed_backpack_stacks = self.backpack.iter_mut().filter_map(Option::take).collect();
        RecallCleanup {
            removed_backpack_stacks,
        }
    }

    fn place_stack(
        &mut self,
        incoming: &mut InventoryStack,
        choice: PlacementChoice,
        now: Tick,
    ) -> Result<PickupOutcome, InventoryError> {
        match incoming {
            InventoryStack::Equipment(item) => self.place_equipment(item.clone(), choice, now),
            InventoryStack::RedTonic {
                instance_id,
                quantity,
            } => {
                if choice == PlacementChoice::Equip {
                    return Err(InventoryError::TonicCannotEquip);
                }
                let outcome = self.place_tonics(*instance_id, *quantity, now)?;
                if let PickupOutcome::PartiallyCollected { remaining, .. } = outcome {
                    *quantity = remaining;
                }
                Ok(outcome)
            }
        }
    }

    fn place_equipment(
        &mut self,
        item: EquipmentItem,
        choice: PlacementChoice,
        now: Tick,
    ) -> Result<PickupOutcome, InventoryError> {
        match choice {
            PlacementChoice::Take => {
                let Some(index) = self.first_empty_backpack_index() else {
                    return Ok(PickupOutcome::CapacityBlocked {
                        remaining_lifetime_ticks: FIELD_PICKUP_LIFETIME_TICKS,
                    });
                };
                let instance_id = item.instance_id();
                self.backpack[index] = Some(InventoryStack::Equipment(item));
                Ok(PickupOutcome::TakenToBackpack {
                    instance_id,
                    backpack_index: index,
                })
            }
            PlacementChoice::Equip => {
                let slot = item.legal_slot();
                let previous = self.equipped[slot.index()].take();
                let swapped_to = if let Some(previous) = previous {
                    let Some(index) = self.first_empty_backpack_index() else {
                        return Err(InventoryError::BackpackFullForSwap { slot });
                    };
                    self.backpack[index] = Some(InventoryStack::Equipment(previous));
                    Some(index)
                } else {
                    None
                };
                let instance_id = item.instance_id();
                self.equipped[slot.index()] = Some(item);
                Ok(PickupOutcome::Equipped {
                    instance_id,
                    slot,
                    swapped_to_backpack: swapped_to,
                    at: now,
                })
            }
        }
    }

    fn place_tonics(
        &mut self,
        instance_id: ItemInstanceId,
        quantity: u8,
        _now: Tick,
    ) -> Result<PickupOutcome, InventoryError> {
        let merge = self.belt.merge_red_tonics(u32::from(quantity));
        let mut remaining =
            u8::try_from(merge.remainder).map_err(|_| InventoryError::TonicQuantityOverflow)?;
        let belt_added = merge.slot_one_added + merge.slot_two_added;
        let mut backpack_index = None;
        let mut backpack_added = 0;

        if remaining > 0 {
            if let Some((index, existing)) = self.first_nonfull_backpack_tonic() {
                let capacity = RED_TONIC_STACK_CAP - existing;
                backpack_added = remaining.min(capacity);
                if let Some(InventoryStack::RedTonic { quantity, .. }) = &mut self.backpack[index] {
                    *quantity += backpack_added;
                }
                remaining -= backpack_added;
                backpack_index = Some(index);
            } else if let Some(index) = self.first_empty_backpack_index() {
                backpack_added = remaining;
                self.backpack[index] = Some(InventoryStack::red_tonic(instance_id, remaining)?);
                remaining = 0;
                backpack_index = Some(index);
            }
        }

        if remaining == 0 {
            Ok(PickupOutcome::TonicsCollected {
                instance_id,
                belt_added,
                backpack_added,
                backpack_index,
            })
        } else if belt_added == 0 && backpack_added == 0 {
            Ok(PickupOutcome::CapacityBlocked {
                remaining_lifetime_ticks: FIELD_PICKUP_LIFETIME_TICKS,
            })
        } else {
            Ok(PickupOutcome::PartiallyCollected {
                instance_id,
                belt_added,
                backpack_added,
                remaining,
            })
        }
    }

    fn remove_owned(
        &mut self,
        location: OwnedItemLocation,
    ) -> Result<InventoryStack, InventoryError> {
        match location {
            OwnedItemLocation::Equipped(slot) => self.equipped[slot.index()]
                .take()
                .map(InventoryStack::Equipment)
                .ok_or(InventoryError::EmptyOwnedLocation(location)),
            OwnedItemLocation::Backpack(index) => {
                if index >= PROTOTYPE_BACKPACK_CAPACITY {
                    return Err(InventoryError::BackpackIndexOutOfRange(index));
                }
                self.backpack[index]
                    .take()
                    .ok_or(InventoryError::EmptyOwnedLocation(location))
            }
        }
    }

    fn first_empty_backpack_index(&self) -> Option<usize> {
        self.backpack.iter().position(Option::is_none)
    }

    fn first_nonfull_backpack_tonic(&self) -> Option<(usize, u8)> {
        self.backpack
            .iter()
            .enumerate()
            .find_map(|(index, stack)| match stack {
                Some(InventoryStack::RedTonic { quantity, .. })
                    if *quantity < RED_TONIC_STACK_CAP =>
                {
                    Some((index, *quantity))
                }
                _ => None,
            })
    }

    fn ensure_incoming_unique(&self, incoming: &InventoryStack) -> Result<(), InventoryError> {
        let incoming_id = incoming.instance_id();
        if self
            .equipped
            .iter()
            .flatten()
            .any(|item| item.instance_id() == incoming_id)
            || self
                .backpack
                .iter()
                .flatten()
                .any(|stack| stack.instance_id() == incoming_id)
        {
            return Err(InventoryError::DuplicateItemInstanceId(incoming_id));
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), InventoryError> {
        let mut ids = Vec::new();
        for (index, item) in self.equipped.iter().enumerate() {
            if let Some(item) = item {
                let actual = EquipmentSlot::ALL[index];
                if item.legal_slot() != actual {
                    return Err(InventoryError::IllegalEquippedSlot {
                        instance_id: item.instance_id(),
                        expected: item.legal_slot(),
                        actual,
                    });
                }
                ids.push(item.instance_id());
            }
        }
        for stack in self.backpack.iter().flatten() {
            validate_stack(stack)?;
            ids.push(stack.instance_id());
        }
        ids.sort_unstable();
        if let Some(duplicate) = ids.windows(2).find(|pair| pair[0] == pair[1]) {
            return Err(InventoryError::DuplicateItemInstanceId(duplicate[0]));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickupOutcome {
    TakenToBackpack {
        instance_id: ItemInstanceId,
        backpack_index: usize,
    },
    Equipped {
        instance_id: ItemInstanceId,
        slot: EquipmentSlot,
        swapped_to_backpack: Option<usize>,
        at: Tick,
    },
    TonicsCollected {
        instance_id: ItemInstanceId,
        belt_added: u8,
        backpack_added: u8,
        backpack_index: Option<usize>,
    },
    PartiallyCollected {
        instance_id: ItemInstanceId,
        belt_added: u8,
        backpack_added: u8,
        remaining: u8,
    },
    CapacityBlocked {
        remaining_lifetime_ticks: u64,
    },
}

impl PickupOutcome {
    #[must_use]
    pub const fn mutates_inventory(self) -> bool {
        !matches!(self, Self::CapacityBlocked { .. })
    }

    #[must_use]
    pub const fn is_fully_collected(self) -> bool {
        !matches!(
            self,
            Self::PartiallyCollected { .. } | Self::CapacityBlocked { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RewardOutcome {
    LeftReward {
        pickup_id: FieldPickupId,
    },
    CapacityBlocked {
        pickup_id: FieldPickupId,
        remaining_lifetime_ticks: u64,
    },
    Collected {
        pickup: PickupOutcome,
        dropped: Option<FieldPickup>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartCleanup {
    pub removed_stacks: Vec<InventoryStack>,
    pub cleared_belt_tonics: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallCleanup {
    pub removed_backpack_stacks: Vec<InventoryStack>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InventoryError {
    #[error("item instance ID must be nonzero")]
    ZeroItemInstanceId,
    #[error("run ordinal must be nonzero")]
    ZeroRunOrdinal,
    #[error("run-qualified item instance ID overflowed")]
    ItemInstanceIdOverflow,
    #[error("field pickup ID must be nonzero")]
    ZeroFieldPickupId,
    #[error("item content ID must not be empty")]
    EmptyItemContentId,
    #[error("Red Tonic backpack quantity must be 1..=6, received {0}")]
    InvalidTonicStackQuantity(u8),
    #[error("field-pickup expiry overflowed u64")]
    PickupExpiryOverflow,
    #[error("field pickup position must be finite")]
    NonFinitePickupPosition,
    #[error("field pickup actor position must be finite")]
    NonFinitePickupActorPosition,
    #[error("field pickup is outside the {access:?} reach radius")]
    PickupOutOfReach { access: FieldPickupAccess },
    #[error("pickup {pickup_id:?} expired at {expired_at} before operation tick {now}")]
    PickupExpired {
        pickup_id: FieldPickupId,
        expired_at: Tick,
        now: Tick,
    },
    #[error("pickup {0:?} was already collected")]
    PickupAlreadyCollected(FieldPickupId),
    #[error("item instance ID {0:?} already exists in inventory")]
    DuplicateItemInstanceId(ItemInstanceId),
    #[error("item {instance_id:?} belongs in {expected:?}, not {actual:?}")]
    IllegalEquippedSlot {
        instance_id: ItemInstanceId,
        expected: EquipmentSlot,
        actual: EquipmentSlot,
    },
    #[error("cannot field-swap {slot:?}: all eight backpack stacks are full")]
    BackpackFullForSwap { slot: EquipmentSlot },
    #[error("Red Tonic cannot be placed in an equipment slot")]
    TonicCannotEquip,
    #[error("backpack index {0} is outside 0..8")]
    BackpackIndexOutOfRange(usize),
    #[error("owned location {0:?} is empty")]
    EmptyOwnedLocation(OwnedItemLocation),
    #[error("dropping the selected item did not create enough capacity for the reward")]
    DropExistingDidNotMakeCapacity,
    #[error("Red Tonic quantity conversion overflowed")]
    TonicQuantityOverflow,
    #[error("shared belt failed validation: {0}")]
    Belt(#[from] crate::BeltError),
}

fn validate_stack(stack: &InventoryStack) -> Result<(), InventoryError> {
    if let InventoryStack::RedTonic { quantity, .. } = stack
        && (*quantity == 0 || *quantity > RED_TONIC_STACK_CAP)
    {
        return Err(InventoryError::InvalidTonicStackQuantity(*quantity));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iid(value: u64) -> ItemInstanceId {
        ItemInstanceId::new(value).expect("nonzero item ID")
    }

    fn pid(value: u64) -> FieldPickupId {
        FieldPickupId::new(value).expect("nonzero pickup ID")
    }

    fn equipment(value: u64, slot: EquipmentSlot) -> InventoryStack {
        InventoryStack::Equipment(EquipmentItem::new(
            iid(value),
            ItemContentId::new(format!("item.prototype.{slot:?}.{value}")).expect("content ID"),
            slot,
        ))
    }

    fn pickup(value: u64, stack: InventoryStack, tick: u64) -> FieldPickup {
        FieldPickup::new(pid(value), stack, SimulationVector::default(), Tick(tick))
            .expect("pickup")
    }

    fn positioned_pickup(value: u64, x: f32) -> FieldPickup {
        FieldPickup::new(
            pid(value),
            equipment(value, EquipmentSlot::Charm),
            SimulationVector::new(x, 0.0),
            Tick(0),
        )
        .expect("positioned pickup")
    }

    fn fill_backpack(inventory: &mut PrototypeInventory, first_id: u64) {
        for index in 0..PROTOTYPE_BACKPACK_CAPACITY {
            inventory.backpack[index] = Some(equipment(
                first_id + u64::try_from(index).expect("index"),
                EquipmentSlot::Charm,
            ));
        }
    }

    #[test]
    fn typed_ids_and_content_ids_fail_closed() {
        assert_eq!(
            ItemInstanceId::new(0),
            Err(InventoryError::ZeroItemInstanceId)
        );
        assert_eq!(
            FieldPickupId::new(0),
            Err(InventoryError::ZeroFieldPickupId)
        );
        assert_eq!(
            ItemContentId::new("  "),
            Err(InventoryError::EmptyItemContentId)
        );
    }

    #[test]
    fn automatic_and_interact_reach_are_inclusive_and_transactional() {
        let actor = SimulationVector::default();
        let mut automatic_inventory = PrototypeInventory::first_playable_belt();
        let mut automatic = positioned_pickup(80, AUTOMATIC_PICKUP_RADIUS_TILES);
        assert!(
            automatic_inventory
                .apply_field_pickup(
                    &mut automatic,
                    PlacementChoice::Take,
                    actor,
                    FieldPickupAccess::Automatic,
                    Tick(1),
                )
                .expect("automatic tangent")
                .is_fully_collected()
        );

        let mut interact_inventory = PrototypeInventory::first_playable_belt();
        let mut interact = positioned_pickup(81, INTERACT_PICKUP_RADIUS_TILES);
        assert!(
            interact_inventory
                .apply_field_pickup(
                    &mut interact,
                    PlacementChoice::Take,
                    actor,
                    FieldPickupAccess::Interact,
                    Tick(1),
                )
                .expect("interact tangent")
                .is_fully_collected()
        );

        for (id, x, access) in [
            (
                82,
                AUTOMATIC_PICKUP_RADIUS_TILES + 0.001,
                FieldPickupAccess::Automatic,
            ),
            (
                83,
                INTERACT_PICKUP_RADIUS_TILES + 0.001,
                FieldPickupAccess::Interact,
            ),
        ] {
            let mut inventory = PrototypeInventory::first_playable_belt();
            let mut field = positioned_pickup(id, x);
            let inventory_before = inventory.clone();
            let field_before = field.clone();
            assert_eq!(
                inventory.apply_field_pickup(
                    &mut field,
                    PlacementChoice::Take,
                    actor,
                    access,
                    Tick(1),
                ),
                Err(InventoryError::PickupOutOfReach { access })
            );
            assert_eq!(inventory, inventory_before);
            assert_eq!(field, field_before);
        }
    }

    #[test]
    fn nonfinite_pickup_and_actor_positions_fail_without_mutation() {
        assert_eq!(
            FieldPickup::new(
                pid(90),
                equipment(90, EquipmentSlot::Charm),
                SimulationVector::new(f32::NAN, 0.0),
                Tick(0),
            ),
            Err(InventoryError::NonFinitePickupPosition)
        );
        let mut inventory = PrototypeInventory::first_playable_belt();
        let mut field = positioned_pickup(91, 0.0);
        let before = (inventory.clone(), field.clone());
        assert_eq!(
            inventory.apply_field_pickup(
                &mut field,
                PlacementChoice::Take,
                SimulationVector::new(f32::INFINITY, 0.0),
                FieldPickupAccess::Automatic,
                Tick(1),
            ),
            Err(InventoryError::NonFinitePickupActorPosition)
        );
        assert_eq!((inventory, field), before);
    }

    #[test]
    fn inventory_has_exact_slots_backpack_and_shared_belt() {
        let inventory = PrototypeInventory::first_playable_belt();
        assert_eq!(inventory.equipped().len(), 4);
        assert_eq!(inventory.backpack().len(), 8);
        assert_eq!(inventory.belt().slots().len(), 2);
        assert_eq!(inventory.belt().slot(0), Some(BeltSlot::RedTonic(2)));
    }

    #[test]
    fn first_playable_loadout_is_exact_and_run_qualified() {
        let first = PrototypeInventory::first_playable_loadout(1).expect("first run");
        let second = PrototypeInventory::first_playable_loadout(2).expect("second run");
        let expected = [
            (EquipmentSlot::Weapon, "item.prototype.weapon.pine_crossbow"),
            (EquipmentSlot::Relic, "item.prototype.relic.dented_scope"),
            (EquipmentSlot::Armor, "item.prototype.armor.reedcloth_wraps"),
        ];
        for (slot, content_id) in expected {
            assert_eq!(
                first
                    .equipped_item(slot)
                    .expect("starter item")
                    .content_id()
                    .as_str(),
                content_id
            );
            assert_ne!(
                first.equipped_item(slot).map(EquipmentItem::instance_id),
                second.equipped_item(slot).map(EquipmentItem::instance_id)
            );
        }
        assert!(first.equipped_item(EquipmentSlot::Charm).is_none());
        assert!(first.backpack().iter().all(Option::is_none));
        assert_eq!(first.belt(), &TonicBelt::first_playable());
        assert_eq!(
            PrototypeInventory::first_playable_loadout(0),
            Err(InventoryError::ZeroRunOrdinal)
        );
    }

    #[test]
    fn default_take_uses_first_empty_backpack_index() {
        let mut inventory = PrototypeInventory::first_playable_belt();
        inventory.backpack[0] = Some(equipment(1, EquipmentSlot::Charm));
        inventory.backpack[2] = Some(equipment(2, EquipmentSlot::Armor));
        let mut incoming = pickup(100, equipment(3, EquipmentSlot::Weapon), 5);
        assert_eq!(
            inventory
                .apply_field_pickup(
                    &mut incoming,
                    PlacementChoice::Take,
                    SimulationVector::default(),
                    FieldPickupAccess::Automatic,
                    Tick(10)
                )
                .expect("take"),
            PickupOutcome::TakenToBackpack {
                instance_id: iid(3),
                backpack_index: 1,
            }
        );
        assert_eq!(
            inventory.backpack_stack(1).map(InventoryStack::instance_id),
            Some(iid(3))
        );
    }

    #[test]
    fn explicit_equip_uses_authored_slot() {
        let mut inventory = PrototypeInventory::first_playable_belt();
        let mut incoming = pickup(100, equipment(3, EquipmentSlot::Relic), 5);
        let outcome = inventory
            .apply_field_pickup(
                &mut incoming,
                PlacementChoice::Equip,
                SimulationVector::default(),
                FieldPickupAccess::Automatic,
                Tick(10),
            )
            .expect("equip");
        assert!(matches!(
            outcome,
            PickupOutcome::Equipped {
                slot: EquipmentSlot::Relic,
                swapped_to_backpack: None,
                ..
            }
        ));
        assert_eq!(
            inventory
                .equipped_item(EquipmentSlot::Relic)
                .map(EquipmentItem::instance_id),
            Some(iid(3))
        );
    }

    #[test]
    fn field_swap_moves_old_item_to_first_empty_backpack() {
        let mut inventory = PrototypeInventory::first_playable_belt();
        inventory.equipped[EquipmentSlot::Weapon.index()] =
            equipment(1, EquipmentSlot::Weapon).equipment().cloned();
        inventory.backpack[0] = Some(equipment(2, EquipmentSlot::Charm));
        let mut incoming = pickup(100, equipment(3, EquipmentSlot::Weapon), 5);
        assert!(matches!(
            inventory
                .apply_field_pickup(
                    &mut incoming,
                    PlacementChoice::Equip,
                    SimulationVector::default(),
                    FieldPickupAccess::Automatic,
                    Tick(10)
                )
                .expect("swap"),
            PickupOutcome::Equipped {
                swapped_to_backpack: Some(1),
                ..
            }
        ));
        assert_eq!(
            inventory.backpack_stack(1).map(InventoryStack::instance_id),
            Some(iid(1))
        );
        assert_eq!(
            inventory
                .equipped_item(EquipmentSlot::Weapon)
                .map(EquipmentItem::instance_id),
            Some(iid(3))
        );
    }

    #[test]
    fn full_backpack_swap_rejects_without_mutation() {
        let mut inventory = PrototypeInventory::first_playable_belt();
        inventory.equipped[EquipmentSlot::Weapon.index()] =
            equipment(1, EquipmentSlot::Weapon).equipment().cloned();
        fill_backpack(&mut inventory, 10);
        let mut incoming = pickup(100, equipment(30, EquipmentSlot::Weapon), 5);
        let before_inventory = inventory.clone();
        let before_pickup = incoming.clone();
        assert_eq!(
            inventory.apply_field_pickup(
                &mut incoming,
                PlacementChoice::Equip,
                SimulationVector::default(),
                FieldPickupAccess::Automatic,
                Tick(10)
            ),
            Err(InventoryError::BackpackFullForSwap {
                slot: EquipmentSlot::Weapon
            })
        );
        assert_eq!(inventory, before_inventory);
        assert_eq!(incoming, before_pickup);
    }

    #[test]
    fn capacity_blocked_take_remains_on_ground_for_exact_lifetime() {
        let mut inventory = PrototypeInventory::first_playable_belt();
        fill_backpack(&mut inventory, 10);
        let mut incoming = pickup(100, equipment(30, EquipmentSlot::Weapon), 5);
        let before = incoming.clone();
        assert_eq!(
            inventory
                .apply_field_pickup(
                    &mut incoming,
                    PlacementChoice::Take,
                    SimulationVector::default(),
                    FieldPickupAccess::Automatic,
                    Tick(10)
                )
                .expect("blocked outcome"),
            PickupOutcome::CapacityBlocked {
                remaining_lifetime_ticks: FIELD_PICKUP_LIFETIME_TICKS - 5,
            }
        );
        assert_eq!(incoming, before);
        assert!(!incoming.is_expired_at(Tick(1_804)));
        assert!(incoming.is_expired_at(Tick(1_805)));
        assert!(matches!(
            inventory.apply_field_pickup(
                &mut incoming,
                PlacementChoice::Take,
                SimulationVector::default(),
                FieldPickupAccess::Automatic,
                Tick(1_805)
            ),
            Err(InventoryError::PickupExpired { .. })
        ));
    }

    #[test]
    fn tonic_pickup_reuses_belt_order_then_one_backpack_stack() {
        let belt =
            TonicBelt::from_slots([BeltSlot::RedTonic(5), BeltSlot::RedTonic(5)]).expect("belt");
        let mut inventory = PrototypeInventory::new(belt);
        let mut incoming = pickup(
            100,
            InventoryStack::red_tonic(iid(1), 5).expect("tonics"),
            0,
        );
        assert_eq!(
            inventory
                .apply_field_pickup(
                    &mut incoming,
                    PlacementChoice::Take,
                    SimulationVector::default(),
                    FieldPickupAccess::Automatic,
                    Tick(1)
                )
                .expect("pickup"),
            PickupOutcome::TonicsCollected {
                instance_id: iid(1),
                belt_added: 2,
                backpack_added: 3,
                backpack_index: Some(0),
            }
        );
        assert_eq!(
            inventory.belt().slots(),
            &[BeltSlot::RedTonic(6), BeltSlot::RedTonic(6)]
        );
        assert_eq!(
            inventory
                .backpack_stack(0)
                .and_then(InventoryStack::tonic_quantity),
            Some(3)
        );
    }

    #[test]
    fn tonic_cannot_use_equipment_disposition_transactionally() {
        let mut inventory = PrototypeInventory::first_playable_belt();
        let mut incoming = pickup(100, InventoryStack::red_tonic(iid(1), 1).expect("tonic"), 0);
        let before_inventory = inventory.clone();
        let before_pickup = incoming.clone();
        assert_eq!(
            inventory.apply_field_pickup(
                &mut incoming,
                PlacementChoice::Equip,
                SimulationVector::default(),
                FieldPickupAccess::Automatic,
                Tick(1)
            ),
            Err(InventoryError::TonicCannotEquip)
        );
        assert_eq!(inventory, before_inventory);
        assert_eq!(incoming, before_pickup);
    }

    #[test]
    fn fully_collected_pickup_cannot_be_replayed() {
        let mut inventory = PrototypeInventory::first_playable_belt();
        let mut incoming = pickup(100, InventoryStack::red_tonic(iid(1), 1).expect("tonic"), 0);
        inventory
            .apply_field_pickup(
                &mut incoming,
                PlacementChoice::Take,
                SimulationVector::default(),
                FieldPickupAccess::Automatic,
                Tick(1),
            )
            .expect("first pickup");
        assert!(incoming.is_collected());
        let before = inventory.clone();
        assert_eq!(
            inventory.apply_field_pickup(
                &mut incoming,
                PlacementChoice::Take,
                SimulationVector::default(),
                FieldPickupAccess::Automatic,
                Tick(2)
            ),
            Err(InventoryError::PickupAlreadyCollected(pid(100)))
        );
        assert_eq!(inventory, before);
    }

    #[test]
    fn duplicate_instance_id_is_rejected_without_mutation() {
        let mut inventory = PrototypeInventory::first_playable_belt();
        inventory.backpack[0] = Some(equipment(1, EquipmentSlot::Weapon));
        let mut incoming = pickup(100, equipment(1, EquipmentSlot::Weapon), 0);
        let before = inventory.clone();
        assert_eq!(
            inventory.apply_field_pickup(
                &mut incoming,
                PlacementChoice::Take,
                SimulationVector::default(),
                FieldPickupAccess::Automatic,
                Tick(1)
            ),
            Err(InventoryError::DuplicateItemInstanceId(iid(1)))
        );
        assert_eq!(inventory, before);
    }

    #[test]
    fn reward_leave_changes_nothing() {
        let mut inventory = PrototypeInventory::first_playable_belt();
        let mut reward = pickup(100, equipment(1, EquipmentSlot::Weapon), 0);
        let before_inventory = inventory.clone();
        let before_reward = reward.clone();
        assert_eq!(
            inventory
                .apply_reward_choice(&mut reward, RewardChoice::LeaveReward, Tick(1))
                .expect("leave"),
            RewardOutcome::LeftReward {
                pickup_id: pid(100)
            }
        );
        assert_eq!(inventory, before_inventory);
        assert_eq!(reward, before_reward);
    }

    #[test]
    fn reward_take_and_equip_share_field_capacity_rules() {
        let mut take_inventory = PrototypeInventory::first_playable_belt();
        let mut field_inventory = take_inventory.clone();
        let mut reward = pickup(100, equipment(1, EquipmentSlot::Weapon), 0);
        let mut field = reward.clone();
        let reward_outcome = take_inventory
            .apply_reward_choice(&mut reward, RewardChoice::Take, Tick(1))
            .expect("reward take");
        let field_outcome = field_inventory
            .apply_field_pickup(
                &mut field,
                PlacementChoice::Take,
                SimulationVector::default(),
                FieldPickupAccess::Automatic,
                Tick(1),
            )
            .expect("field take");
        assert_eq!(
            reward_outcome,
            RewardOutcome::Collected {
                pickup: field_outcome,
                dropped: None,
            }
        );
        assert_eq!(take_inventory, field_inventory);
    }

    #[test]
    fn reward_equip_uses_the_same_authored_slot_and_swap_rule() {
        let mut inventory = PrototypeInventory::first_playable_belt();
        inventory.equipped[EquipmentSlot::Armor.index()] =
            equipment(1, EquipmentSlot::Armor).equipment().cloned();
        let mut reward = pickup(100, equipment(2, EquipmentSlot::Armor), 0);
        let outcome = inventory
            .apply_reward_choice(&mut reward, RewardChoice::Equip, Tick(1))
            .expect("reward equip");
        assert!(matches!(
            outcome,
            RewardOutcome::Collected {
                pickup: PickupOutcome::Equipped {
                    slot: EquipmentSlot::Armor,
                    swapped_to_backpack: Some(0),
                    ..
                },
                dropped: None,
            }
        ));
        assert!(reward.is_collected());
        assert_eq!(
            inventory
                .equipped_item(EquipmentSlot::Armor)
                .map(EquipmentItem::instance_id),
            Some(iid(2))
        );
        assert_eq!(
            inventory.backpack_stack(0).map(InventoryStack::instance_id),
            Some(iid(1))
        );
    }

    #[test]
    fn capacity_blocked_reward_take_leaves_reward_and_inventory_unchanged() {
        let mut inventory = PrototypeInventory::first_playable_belt();
        fill_backpack(&mut inventory, 10);
        let mut reward = pickup(100, equipment(30, EquipmentSlot::Weapon), 5);
        let before_inventory = inventory.clone();
        let before_reward = reward.clone();
        assert_eq!(
            inventory
                .apply_reward_choice(&mut reward, RewardChoice::Take, Tick(10))
                .expect("blocked reward"),
            RewardOutcome::CapacityBlocked {
                pickup_id: pid(100),
                remaining_lifetime_ticks: FIELD_PICKUP_LIFETIME_TICKS - 5,
            }
        );
        assert_eq!(inventory, before_inventory);
        assert_eq!(reward, before_reward);
    }

    #[test]
    fn reward_drop_existing_returns_ground_pickup_and_never_destroys() {
        let mut inventory = PrototypeInventory::first_playable_belt();
        fill_backpack(&mut inventory, 10);
        let displaced = inventory.backpack_stack(3).cloned().expect("owned stack");
        let mut reward = pickup(100, equipment(30, EquipmentSlot::Weapon), 0);
        let outcome = inventory
            .apply_reward_choice(
                &mut reward,
                RewardChoice::DropExisting {
                    location: OwnedItemLocation::Backpack(3),
                    dropped_pickup_id: pid(200),
                    then: PlacementChoice::Take,
                },
                Tick(10),
            )
            .expect("drop and take");
        let RewardOutcome::Collected {
            dropped: Some(dropped),
            ..
        } = outcome
        else {
            panic!("expected returned drop");
        };
        assert_eq!(dropped.pickup_id(), pid(200));
        assert_eq!(dropped.stack(), &displaced);
        assert_eq!(dropped.expires_at(), Tick(1_810));
        assert_eq!(
            inventory.backpack_stack(3).map(InventoryStack::instance_id),
            Some(iid(30))
        );
    }

    #[test]
    fn reward_can_drop_equipped_item_and_returns_it_as_ground_pickup() {
        let mut inventory = PrototypeInventory::first_playable_belt();
        inventory.equipped[EquipmentSlot::Relic.index()] =
            equipment(1, EquipmentSlot::Relic).equipment().cloned();
        let mut reward = pickup(100, equipment(2, EquipmentSlot::Relic), 0);
        let outcome = inventory
            .apply_reward_choice(
                &mut reward,
                RewardChoice::DropExisting {
                    location: OwnedItemLocation::Equipped(EquipmentSlot::Relic),
                    dropped_pickup_id: pid(200),
                    then: PlacementChoice::Equip,
                },
                Tick(1),
            )
            .expect("drop equipped and equip reward");
        let RewardOutcome::Collected {
            dropped: Some(dropped),
            ..
        } = outcome
        else {
            panic!("expected displaced ground pickup");
        };
        assert_eq!(dropped.stack().instance_id(), iid(1));
        assert_eq!(
            inventory
                .equipped_item(EquipmentSlot::Relic)
                .map(EquipmentItem::instance_id),
            Some(iid(2))
        );
    }

    #[test]
    fn illegal_equipped_slot_and_duplicate_owned_ids_fail_validation() {
        let mut wrong_slot = PrototypeInventory::first_playable_belt();
        wrong_slot.equipped[EquipmentSlot::Armor.index()] =
            equipment(1, EquipmentSlot::Weapon).equipment().cloned();
        assert_eq!(
            wrong_slot.validate(),
            Err(InventoryError::IllegalEquippedSlot {
                instance_id: iid(1),
                expected: EquipmentSlot::Weapon,
                actual: EquipmentSlot::Armor,
            })
        );

        let mut duplicate = PrototypeInventory::first_playable_belt();
        duplicate.backpack[0] = Some(equipment(1, EquipmentSlot::Weapon));
        duplicate.backpack[1] = Some(equipment(1, EquipmentSlot::Relic));
        assert_eq!(
            duplicate.validate(),
            Err(InventoryError::DuplicateItemInstanceId(iid(1)))
        );
    }

    #[test]
    fn invalid_drop_location_or_insufficient_capacity_rolls_back_everything() {
        let mut inventory = PrototypeInventory::first_playable_belt();
        fill_backpack(&mut inventory, 10);
        let mut reward = pickup(100, equipment(30, EquipmentSlot::Weapon), 0);
        let before_inventory = inventory.clone();
        let before_reward = reward.clone();
        assert_eq!(
            inventory.apply_reward_choice(
                &mut reward,
                RewardChoice::DropExisting {
                    location: OwnedItemLocation::Backpack(9),
                    dropped_pickup_id: pid(200),
                    then: PlacementChoice::Take,
                },
                Tick(1),
            ),
            Err(InventoryError::BackpackIndexOutOfRange(9))
        );
        assert_eq!(inventory, before_inventory);
        assert_eq!(reward, before_reward);
    }

    #[test]
    fn restart_cleanup_removes_every_owned_stack_and_belt_tonic() {
        let mut inventory = PrototypeInventory::first_playable_belt();
        inventory.equipped[EquipmentSlot::Weapon.index()] =
            equipment(1, EquipmentSlot::Weapon).equipment().cloned();
        inventory.backpack[0] = Some(equipment(2, EquipmentSlot::Charm));
        let cleanup = inventory.clear_for_restart();
        assert_eq!(cleanup.removed_stacks.len(), 2);
        assert_eq!(cleanup.cleared_belt_tonics, 2);
        assert!(inventory.equipped().iter().all(Option::is_none));
        assert!(inventory.backpack().iter().all(Option::is_none));
        assert_eq!(
            inventory.belt().slots(),
            &[BeltSlot::Empty, BeltSlot::Empty]
        );
    }

    #[test]
    fn emergency_recall_preserves_equipped_and_belt_but_destroys_backpack() {
        let mut inventory = PrototypeInventory::first_playable_belt();
        inventory.equipped[EquipmentSlot::Weapon.index()] =
            equipment(1, EquipmentSlot::Weapon).equipment().cloned();
        inventory.backpack[0] = Some(equipment(2, EquipmentSlot::Charm));
        let equipped_before = inventory.equipped().clone();
        let belt_before = *inventory.belt();
        let cleanup = inventory.clear_pending_for_recall();
        assert_eq!(cleanup.removed_backpack_stacks.len(), 1);
        assert_eq!(inventory.equipped(), &equipped_before);
        assert_eq!(inventory.belt(), &belt_before);
        assert!(inventory.backpack().iter().all(Option::is_none));
    }

    #[test]
    fn deterministic_operation_replay_is_identical() {
        fn replay() -> (PrototypeInventory, Vec<PickupOutcome>, RestartCleanup) {
            let mut inventory = PrototypeInventory::first_playable_belt();
            let mut outcomes = Vec::new();
            for (pickup_id, item_id, slot) in [
                (100, 1, EquipmentSlot::Weapon),
                (101, 2, EquipmentSlot::Relic),
                (102, 3, EquipmentSlot::Weapon),
            ] {
                let mut incoming = pickup(pickup_id, equipment(item_id, slot), 0);
                let choice = if item_id == 2 {
                    PlacementChoice::Take
                } else {
                    PlacementChoice::Equip
                };
                outcomes.push(
                    inventory
                        .apply_field_pickup(
                            &mut incoming,
                            choice,
                            SimulationVector::default(),
                            FieldPickupAccess::Automatic,
                            Tick(item_id),
                        )
                        .expect("operation"),
                );
            }
            let cleanup = inventory.clear_for_restart();
            (inventory, outcomes, cleanup)
        }
        assert_eq!(replay(), replay());
    }
}
