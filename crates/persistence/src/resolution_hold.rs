//! Bounded durable domain for minimum M03 `ResolutionHold` recovery.
//!
//! The contract is derived from GDD `DTH-011`, `LOOT-002/050/060`, and `TECH-021`-`023`;
//! Content Production Spec `CONT-HUB-001/002`; Roadmap `GB-M03-03/08`; and accepted
//! `SPEC-CONFLICT-029/030`. Clients select one stored logical stack and an action. The server
//! owns the complete-stack destination plan, version advances, durable hashes, and result.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sim_core::{
    CHARACTER_SAFE_CAPACITY, DurableStorageSlot, ItemUid, ResolutionHoldRecoveryAction,
    ResolutionHoldRecoveryDestination, ResolutionHoldRecoverySnapshot, TERMINAL_OVERFLOW_CAPACITY,
    VAULT_CAPACITY, plan_resolution_hold_recovery,
};

use crate::{CORE_ITEM_CONTENT_REVISION, PersistenceError, WIPEABLE_CORE_NAMESPACE};

pub const RESOLUTION_HOLD_CONTRACT_VERSION_V1: u16 = 1;
pub const MAX_RESOLUTION_HOLD_STACKS_V1: usize = 8;
pub const MAX_RESOLUTION_HOLD_ITEMS_V1: usize = 64;
pub const RESOLUTION_HOLD_ID_BYTES: usize = 16;
pub const RESOLUTION_HOLD_HASH_BYTES: usize = 32;
pub const RESOLUTION_HOLD_OVERFLOW_LIFETIME_MILLIS: u64 = 72 * 60 * 60 * 1_000;

pub const RESOLUTION_HOLD_STACK_DIGEST_CONTEXT_V1: &str = "gravebound.resolution-hold-stack.v1";
pub const RESOLUTION_HOLD_REQUEST_HASH_CONTEXT_V1: &str = "gravebound.resolution-hold-request.v1";
pub const RESOLUTION_HOLD_RESULT_DIGEST_CONTEXT_V1: &str = "gravebound.resolution-hold-result.v1";
pub const RESOLUTION_HOLD_ITEM_LEDGER_ID_CONTEXT_V1: &str =
    "gravebound.resolution-hold-item-ledger.v1";
pub const RESOLUTION_HOLD_ACCEPTED_AUDIT_ID_CONTEXT_V1: &str =
    "gravebound.resolution-hold-audit.v1";
pub const RESOLUTION_HOLD_OUTBOX_ID_CONTEXT_V1: &str = "gravebound.resolution-hold-outbox.v1";
pub const RESOLUTION_HOLD_CONFLICT_DIGEST_CONTEXT_V1: &str =
    "gravebound.resolution-hold-conflict.v1";

const MAX_RESULT_BYTES: usize = 65_536;
const CORE_TONIC_TEMPLATE_ID: &str = "consumable.red_tonic";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredResolutionHoldItemKindV1 {
    Equipment,
    Consumable,
}

impl StoredResolutionHoldItemKindV1 {
    #[must_use]
    pub const fn durable_kind(self) -> i16 {
        match self {
            Self::Equipment => 0,
            Self::Consumable => 1,
        }
    }

    pub const fn try_from_durable_kind(value: i16) -> Result<Self, PersistenceError> {
        match value {
            0 => Ok(Self::Equipment),
            1 => Ok(Self::Consumable),
            _ => Err(corrupt()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredResolutionHoldDestinationV1 {
    CharacterSafe(u8),
    Vault(u16),
    Overflow(u8),
}

impl StoredResolutionHoldDestinationV1 {
    #[must_use]
    pub const fn durable_kind(self) -> i16 {
        match self {
            Self::CharacterSafe(_) => 5,
            Self::Vault(_) => 6,
            Self::Overflow(_) => 8,
        }
    }

    #[must_use]
    pub const fn slot_index(self) -> u16 {
        match self {
            Self::CharacterSafe(index) | Self::Overflow(index) => index as u16,
            Self::Vault(index) => index,
        }
    }

    #[must_use]
    pub const fn account_owned(self) -> bool {
        matches!(self, Self::Vault(_) | Self::Overflow(_))
    }

    pub fn try_from_durable(kind: i16, slot_index: u16) -> Result<Self, PersistenceError> {
        match kind {
            5 => match u8::try_from(slot_index) {
                Ok(index) => Ok(Self::CharacterSafe(index)),
                Err(_) => Err(corrupt()),
            },
            6 => Ok(Self::Vault(slot_index)),
            8 => match u8::try_from(slot_index) {
                Ok(index) => Ok(Self::Overflow(index)),
                Err(_) => Err(corrupt()),
            },
            _ => Err(corrupt()),
        }
    }

    fn validate(self) -> Result<(), PersistenceError> {
        let valid = match self {
            Self::CharacterSafe(index) => usize::from(index) < CHARACTER_SAFE_CAPACITY,
            Self::Vault(index) => usize::from(index) < VAULT_CAPACITY,
            Self::Overflow(index) => usize::from(index) < TERMINAL_OVERFLOW_CAPACITY,
        };
        if valid { Ok(()) } else { Err(corrupt()) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredResolutionHoldActionV1 {
    Move,
    DestroyConfirmed,
}

impl StoredResolutionHoldActionV1 {
    #[must_use]
    pub const fn durable_kind(self) -> i16 {
        match self {
            Self::Move => 0,
            Self::DestroyConfirmed => 1,
        }
    }

    pub const fn try_from_durable_kind(value: i16) -> Result<Self, PersistenceError> {
        match value {
            0 => Ok(Self::Move),
            1 => Ok(Self::DestroyConfirmed),
            _ => Err(corrupt()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredResolutionHoldVersionsV1 {
    pub account: u64,
    pub character: u64,
    pub world: u64,
    pub inventory: u64,
}

impl StoredResolutionHoldVersionsV1 {
    fn validate(self) -> Result<(), PersistenceError> {
        if [self.account, self.character, self.world, self.inventory].contains(&0)
            || self.character != self.world
        {
            return Err(corrupt());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredResolutionHoldVersionAdvanceV1 {
    pub pre: u64,
    pub post: u64,
}

impl StoredResolutionHoldVersionAdvanceV1 {
    fn validate(self, may_remain: bool) -> Result<(), PersistenceError> {
        if self.pre == 0
            || (self.pre != self.post && self.pre.checked_add(1) != Some(self.post))
            || (!may_remain && self.pre == self.post)
        {
            return Err(corrupt());
        }
        Ok(())
    }

    #[must_use]
    pub fn advanced(self) -> bool {
        self.pre.checked_add(1) == Some(self.post)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredResolutionHoldVersionVectorV1 {
    pub account: StoredResolutionHoldVersionAdvanceV1,
    pub character: StoredResolutionHoldVersionAdvanceV1,
    pub world: StoredResolutionHoldVersionAdvanceV1,
    pub inventory: StoredResolutionHoldVersionAdvanceV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredResolutionHoldItemV1 {
    pub item_uid: [u8; RESOLUTION_HOLD_ID_BYTES],
    pub item_version: u64,
}

impl StoredResolutionHoldItemV1 {
    fn validate(self) -> Result<(), PersistenceError> {
        if self.item_uid == [0; RESOLUTION_HOLD_ID_BYTES] || self.item_version == 0 {
            return Err(corrupt());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredResolutionHoldStackV1 {
    pub extraction_id: [u8; RESOLUTION_HOLD_ID_BYTES],
    pub stack_index: u8,
    pub template_id: String,
    pub content_revision: String,
    pub item_kind: StoredResolutionHoldItemKindV1,
    pub items: Vec<StoredResolutionHoldItemV1>,
    pub stack_digest: [u8; RESOLUTION_HOLD_HASH_BYTES],
    pub extracted_at_unix_millis: u64,
    pub overflow_deadline_unix_millis: u64,
    pub planned_destination: Option<StoredResolutionHoldDestinationV1>,
}

impl StoredResolutionHoldStackV1 {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        if self.extraction_id == [0; RESOLUTION_HOLD_ID_BYTES]
            || usize::from(self.stack_index) >= MAX_RESOLUTION_HOLD_STACKS_V1
            || self.stack_digest == [0; RESOLUTION_HOLD_HASH_BYTES]
            || self.extracted_at_unix_millis == 0
            || self.overflow_deadline_unix_millis
                != self
                    .extracted_at_unix_millis
                    .checked_add(RESOLUTION_HOLD_OVERFLOW_LIFETIME_MILLIS)
                    .ok_or_else(corrupt)?
        {
            return Err(corrupt());
        }
        validate_logical_stack(
            &self.template_id,
            &self.content_revision,
            self.item_kind,
            &self.items,
        )?;
        self.planned_destination
            .map(StoredResolutionHoldDestinationV1::validate)
            .transpose()?;
        if self.stack_digest != canonical_resolution_hold_stack_digest_v1(self)? {
            return Err(corrupt());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredResolutionHoldSnapshotV1 {
    pub account_id: [u8; RESOLUTION_HOLD_ID_BYTES],
    pub character_id: [u8; RESOLUTION_HOLD_ID_BYTES],
    pub versions: StoredResolutionHoldVersionsV1,
    pub storage_resolution_required: bool,
    pub stacks: Vec<StoredResolutionHoldStackV1>,
}

impl StoredResolutionHoldSnapshotV1 {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        if self.account_id == [0; RESOLUTION_HOLD_ID_BYTES]
            || self.character_id == [0; RESOLUTION_HOLD_ID_BYTES]
            || self.account_id == self.character_id
            || self.stacks.len() > MAX_RESOLUTION_HOLD_STACKS_V1
            || self.storage_resolution_required == self.stacks.is_empty()
        {
            return Err(corrupt());
        }
        self.versions.validate()?;
        let mut previous_key = None;
        let mut item_uids = BTreeSet::new();
        for stack in &self.stacks {
            stack.validate()?;
            let key = (stack.extraction_id, stack.stack_index);
            if previous_key.is_some_and(|previous| previous >= key) {
                return Err(corrupt());
            }
            previous_key = Some(key);
            for item in &stack.items {
                if !item_uids.insert(item.item_uid) {
                    return Err(corrupt());
                }
            }
        }
        if item_uids.len() > MAX_RESOLUTION_HOLD_ITEMS_V1 {
            return Err(corrupt());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionHoldStorageStackV1 {
    pub template_id: String,
    pub content_revision: String,
    pub item_kind: StoredResolutionHoldItemKindV1,
    pub items: Vec<StoredResolutionHoldItemV1>,
}

impl ResolutionHoldStorageStackV1 {
    fn validate(&self) -> Result<(), PersistenceError> {
        validate_logical_stack(
            &self.template_id,
            &self.content_revision,
            self.item_kind,
            &self.items,
        )
    }

    fn durable_slot(&self) -> Result<DurableStorageSlot, PersistenceError> {
        self.validate()?;
        match self.item_kind {
            StoredResolutionHoldItemKindV1::Equipment => Ok(DurableStorageSlot::Equipment {
                item_uid: item_uid(self.items[0].item_uid)?,
            }),
            StoredResolutionHoldItemKindV1::Consumable => Ok(DurableStorageSlot::Consumable {
                template_id: self.template_id.clone(),
                item_uids: self
                    .items
                    .iter()
                    .map(|item| item_uid(item.item_uid))
                    .collect::<Result<Vec<_>, _>>()?,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionHoldStorageSnapshotV1 {
    pub character_safe: Vec<Option<ResolutionHoldStorageStackV1>>,
    pub vault: Vec<Option<ResolutionHoldStorageStackV1>>,
    pub overflow: Vec<Option<ResolutionHoldStorageStackV1>>,
}

impl ResolutionHoldStorageSnapshotV1 {
    pub fn empty() -> Self {
        Self {
            character_safe: vec![None; CHARACTER_SAFE_CAPACITY],
            vault: vec![None; VAULT_CAPACITY],
            overflow: vec![None; TERMINAL_OVERFLOW_CAPACITY],
        }
    }

    fn durable_slots(
        slots: &[Option<ResolutionHoldStorageStackV1>],
    ) -> Result<Vec<DurableStorageSlot>, PersistenceError> {
        slots
            .iter()
            .map(|slot| {
                slot.as_ref()
                    .map(ResolutionHoldStorageStackV1::durable_slot)
                    .transpose()
                    .map(|slot| slot.unwrap_or(DurableStorageSlot::Empty))
            })
            .collect()
    }

    fn validate(&self) -> Result<(), PersistenceError> {
        if self.character_safe.len() != CHARACTER_SAFE_CAPACITY
            || self.vault.len() != VAULT_CAPACITY
            || self.overflow.len() != TERMINAL_OVERFLOW_CAPACITY
        {
            return Err(corrupt());
        }
        let mut item_uids = BTreeSet::new();
        for stack in self
            .character_safe
            .iter()
            .chain(&self.vault)
            .chain(&self.overflow)
            .flatten()
        {
            stack.validate()?;
            for item in &stack.items {
                if !item_uids.insert(item.item_uid) {
                    return Err(corrupt());
                }
            }
        }
        Ok(())
    }
}

pub fn plan_resolution_hold_destination_v1(
    selected: &StoredResolutionHoldStackV1,
    storage: &ResolutionHoldStorageSnapshotV1,
    authoritative_time_unix_millis: u64,
) -> Result<StoredResolutionHoldDestinationV1, PersistenceError> {
    selected.validate()?;
    storage.validate()?;
    if authoritative_time_unix_millis == 0 {
        return Err(corrupt());
    }
    let mut known_uids = BTreeSet::new();
    for item in &selected.items {
        known_uids.insert(item.item_uid);
    }
    for stack in storage
        .character_safe
        .iter()
        .chain(&storage.vault)
        .chain(&storage.overflow)
        .flatten()
    {
        for item in &stack.items {
            if !known_uids.insert(item.item_uid) {
                return Err(corrupt());
            }
        }
    }
    let selected_storage = ResolutionHoldStorageStackV1 {
        template_id: selected.template_id.clone(),
        content_revision: selected.content_revision.clone(),
        item_kind: selected.item_kind,
        items: selected.items.clone(),
    };
    let plan = plan_resolution_hold_recovery(
        &ResolutionHoldRecoverySnapshot {
            account_version: 1,
            inventory_version: 1,
            authoritative_time_unix_micros: authoritative_time_unix_millis
                .checked_mul(1_000)
                .ok_or_else(corrupt)?,
            extracted_at_unix_micros: selected
                .extracted_at_unix_millis
                .checked_mul(1_000)
                .ok_or_else(corrupt)?,
            held_stack: selected_storage.durable_slot()?,
            character_safe: ResolutionHoldStorageSnapshotV1::durable_slots(
                &storage.character_safe,
            )?,
            vault: ResolutionHoldStorageSnapshotV1::durable_slots(&storage.vault)?,
            overflow: ResolutionHoldStorageSnapshotV1::durable_slots(&storage.overflow)?,
        },
        ResolutionHoldRecoveryAction::Move,
    )
    .map_err(|error| match error {
        sim_core::TerminalInventoryError::ResolutionHoldStorageFull => {
            PersistenceError::ResolutionHoldStorageFull
        }
        _ => corrupt(),
    })?;
    match plan.destination.ok_or_else(corrupt)? {
        ResolutionHoldRecoveryDestination::CharacterSafe(index) => {
            Ok(StoredResolutionHoldDestinationV1::CharacterSafe(index))
        }
        ResolutionHoldRecoveryDestination::Vault(index) => {
            Ok(StoredResolutionHoldDestinationV1::Vault(index))
        }
        ResolutionHoldRecoveryDestination::Overflow {
            slot_index,
            expires_at_unix_micros,
        } => {
            if expires_at_unix_micros / 1_000 != selected.overflow_deadline_unix_millis
                || expires_at_unix_micros % 1_000 != 0
            {
                return Err(corrupt());
            }
            Ok(StoredResolutionHoldDestinationV1::Overflow(slot_index))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionHoldMutationRequestV1 {
    pub contract_version: u16,
    pub namespace_id: String,
    pub account_id: [u8; RESOLUTION_HOLD_ID_BYTES],
    pub character_id: [u8; RESOLUTION_HOLD_ID_BYTES],
    pub mutation_id: [u8; RESOLUTION_HOLD_ID_BYTES],
    pub extraction_id: [u8; RESOLUTION_HOLD_ID_BYTES],
    pub stack_index: u8,
    pub action: StoredResolutionHoldActionV1,
    pub expected_versions: StoredResolutionHoldVersionsV1,
    pub content_revision: String,
    pub expected_stack_digest: [u8; RESOLUTION_HOLD_HASH_BYTES],
    pub issued_at_unix_millis: u64,
}

impl ResolutionHoldMutationRequestV1 {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        if self.contract_version != RESOLUTION_HOLD_CONTRACT_VERSION_V1
            || self.namespace_id != WIPEABLE_CORE_NAMESPACE
            || [
                self.account_id,
                self.character_id,
                self.mutation_id,
                self.extraction_id,
            ]
            .contains(&[0; RESOLUTION_HOLD_ID_BYTES])
            || self.account_id == self.character_id
            || self.mutation_id == self.extraction_id
            || usize::from(self.stack_index) >= MAX_RESOLUTION_HOLD_STACKS_V1
            || self.content_revision != CORE_ITEM_CONTENT_REVISION
            || self.expected_stack_digest == [0; RESOLUTION_HOLD_HASH_BYTES]
            || self.issued_at_unix_millis == 0
        {
            return Err(corrupt());
        }
        self.expected_versions.validate()
    }

    pub fn canonical_hash(&self) -> Result<[u8; RESOLUTION_HOLD_HASH_BYTES], PersistenceError> {
        self.validate()?;
        canonical_hash(RESOLUTION_HOLD_REQUEST_HASH_CONTEXT_V1, self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredResolutionHoldDispositionV1 {
    Moved(StoredResolutionHoldDestinationV1),
    Destroyed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredResolutionHoldItemTransitionV1 {
    pub ordinal: u8,
    pub item_uid: [u8; RESOLUTION_HOLD_ID_BYTES],
    pub template_id: String,
    pub content_revision: String,
    pub item_kind: StoredResolutionHoldItemKindV1,
    pub disposition: StoredResolutionHoldDispositionV1,
    pub pre_item_version: u64,
    pub post_item_version: u64,
    pub ledger_event_id: [u8; RESOLUTION_HOLD_ID_BYTES],
}

impl StoredResolutionHoldItemTransitionV1 {
    fn validate(
        &self,
        expected_ordinal: u8,
        action: StoredResolutionHoldActionV1,
        expected_destination: Option<StoredResolutionHoldDestinationV1>,
    ) -> Result<(), PersistenceError> {
        if self.ordinal != expected_ordinal
            || self.item_uid == [0; RESOLUTION_HOLD_ID_BYTES]
            || self.ledger_event_id == [0; RESOLUTION_HOLD_ID_BYTES]
            || !valid_stable_id(&self.template_id)
            || self.content_revision != CORE_ITEM_CONTENT_REVISION
            || self.pre_item_version == 0
            || self.pre_item_version.checked_add(1) != Some(self.post_item_version)
        {
            return Err(corrupt());
        }
        match (action, self.disposition, expected_destination) {
            (
                StoredResolutionHoldActionV1::Move,
                StoredResolutionHoldDispositionV1::Moved(destination),
                Some(expected),
            ) if destination == expected => destination.validate(),
            (
                StoredResolutionHoldActionV1::DestroyConfirmed,
                StoredResolutionHoldDispositionV1::Destroyed,
                None,
            ) => Ok(()),
            _ => Err(corrupt()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ResolutionHoldResultHashMaterialV1 {
    contract_version: u16,
    namespace_id: String,
    account_id: [u8; RESOLUTION_HOLD_ID_BYTES],
    character_id: [u8; RESOLUTION_HOLD_ID_BYTES],
    mutation_id: [u8; RESOLUTION_HOLD_ID_BYTES],
    extraction_id: [u8; RESOLUTION_HOLD_ID_BYTES],
    stack_index: u8,
    action: StoredResolutionHoldActionV1,
    canonical_request_hash: [u8; RESOLUTION_HOLD_HASH_BYTES],
    expected_stack_digest: [u8; RESOLUTION_HOLD_HASH_BYTES],
    issued_at_unix_millis: u64,
    committed_at_unix_millis: u64,
    versions: StoredResolutionHoldVersionVectorV1,
    destination: Option<StoredResolutionHoldDestinationV1>,
    transitions: Vec<StoredResolutionHoldItemTransitionV1>,
    remaining_hold_stack_count: u8,
    storage_resolution_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredResolutionHoldMutationResultV1 {
    pub contract_version: u16,
    pub namespace_id: String,
    pub account_id: [u8; RESOLUTION_HOLD_ID_BYTES],
    pub character_id: [u8; RESOLUTION_HOLD_ID_BYTES],
    pub mutation_id: [u8; RESOLUTION_HOLD_ID_BYTES],
    pub extraction_id: [u8; RESOLUTION_HOLD_ID_BYTES],
    pub stack_index: u8,
    pub action: StoredResolutionHoldActionV1,
    pub canonical_request_hash: [u8; RESOLUTION_HOLD_HASH_BYTES],
    pub expected_stack_digest: [u8; RESOLUTION_HOLD_HASH_BYTES],
    pub result_hash: [u8; RESOLUTION_HOLD_HASH_BYTES],
    pub issued_at_unix_millis: u64,
    pub committed_at_unix_millis: u64,
    pub versions: StoredResolutionHoldVersionVectorV1,
    pub destination: Option<StoredResolutionHoldDestinationV1>,
    pub transitions: Vec<StoredResolutionHoldItemTransitionV1>,
    pub remaining_hold_stack_count: u8,
    pub storage_resolution_required: bool,
}

impl StoredResolutionHoldMutationResultV1 {
    pub fn seal(mut self) -> Result<Self, PersistenceError> {
        self.result_hash = self.digest()?;
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), PersistenceError> {
        if self.contract_version != RESOLUTION_HOLD_CONTRACT_VERSION_V1
            || self.namespace_id != WIPEABLE_CORE_NAMESPACE
            || [
                self.account_id,
                self.character_id,
                self.mutation_id,
                self.extraction_id,
            ]
            .contains(&[0; RESOLUTION_HOLD_ID_BYTES])
            || self.account_id == self.character_id
            || self.mutation_id == self.extraction_id
            || usize::from(self.stack_index) >= MAX_RESOLUTION_HOLD_STACKS_V1
            || self.canonical_request_hash == [0; RESOLUTION_HOLD_HASH_BYTES]
            || self.expected_stack_digest == [0; RESOLUTION_HOLD_HASH_BYTES]
            || self.result_hash == [0; RESOLUTION_HOLD_HASH_BYTES]
            || self.issued_at_unix_millis == 0
            || self.committed_at_unix_millis < self.issued_at_unix_millis
            || self.transitions.is_empty()
            || self.transitions.len() > MAX_RESOLUTION_HOLD_ITEMS_V1
            || usize::from(self.remaining_hold_stack_count) > MAX_RESOLUTION_HOLD_STACKS_V1
            || self.storage_resolution_required != (self.remaining_hold_stack_count != 0)
        {
            return Err(corrupt());
        }
        let final_clear = !self.storage_resolution_required;
        self.versions.account.validate(true)?;
        self.versions.character.validate(!final_clear)?;
        self.versions.world.validate(!final_clear)?;
        self.versions.inventory.validate(false)?;
        if self.versions.character != self.versions.world
            || (!final_clear
                && (self.versions.character.advanced() || self.versions.world.advanced()))
            || self.versions.account.advanced()
                != self
                    .destination
                    .is_some_and(StoredResolutionHoldDestinationV1::account_owned)
            || self.destination.is_some() != (self.action == StoredResolutionHoldActionV1::Move)
        {
            return Err(corrupt());
        }
        self.destination
            .map(StoredResolutionHoldDestinationV1::validate)
            .transpose()?;
        let mut previous_uid = None;
        let mut ledger_ids = BTreeSet::new();
        let mut logical_items = Vec::with_capacity(self.transitions.len());
        let expected_template_id = self.transitions[0].template_id.as_str();
        let expected_content_revision = self.transitions[0].content_revision.as_str();
        let expected_item_kind = self.transitions[0].item_kind;
        for (index, transition) in self.transitions.iter().enumerate() {
            transition.validate(
                u8::try_from(index).map_err(|_| corrupt())?,
                self.action,
                self.destination,
            )?;
            if previous_uid.is_some_and(|previous| previous >= transition.item_uid)
                || transition.template_id != expected_template_id
                || transition.content_revision != expected_content_revision
                || transition.item_kind != expected_item_kind
                || !ledger_ids.insert(transition.ledger_event_id)
            {
                return Err(corrupt());
            }
            previous_uid = Some(transition.item_uid);
            logical_items.push(StoredResolutionHoldItemV1 {
                item_uid: transition.item_uid,
                item_version: transition.pre_item_version,
            });
        }
        validate_logical_stack(
            expected_template_id,
            expected_content_revision,
            expected_item_kind,
            &logical_items,
        )?;
        if self.result_hash != self.digest()? {
            return Err(corrupt());
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, PersistenceError> {
        self.validate()?;
        let payload = postcard::to_stdvec(self).map_err(|_| corrupt())?;
        if payload.is_empty() || payload.len() > MAX_RESULT_BYTES {
            return Err(corrupt());
        }
        Ok(payload)
    }

    pub fn decode(payload: &[u8]) -> Result<Self, PersistenceError> {
        if payload.is_empty() || payload.len() > MAX_RESULT_BYTES {
            return Err(corrupt());
        }
        let result: Self = postcard::from_bytes(payload).map_err(|_| corrupt())?;
        result.validate()?;
        Ok(result)
    }

    pub fn digest(&self) -> Result<[u8; RESOLUTION_HOLD_HASH_BYTES], PersistenceError> {
        canonical_hash(
            RESOLUTION_HOLD_RESULT_DIGEST_CONTEXT_V1,
            &ResolutionHoldResultHashMaterialV1 {
                contract_version: self.contract_version,
                namespace_id: self.namespace_id.clone(),
                account_id: self.account_id,
                character_id: self.character_id,
                mutation_id: self.mutation_id,
                extraction_id: self.extraction_id,
                stack_index: self.stack_index,
                action: self.action,
                canonical_request_hash: self.canonical_request_hash,
                expected_stack_digest: self.expected_stack_digest,
                issued_at_unix_millis: self.issued_at_unix_millis,
                committed_at_unix_millis: self.committed_at_unix_millis,
                versions: self.versions,
                destination: self.destination,
                transitions: self.transitions.clone(),
                remaining_hold_stack_count: self.remaining_hold_stack_count,
                storage_resolution_required: self.storage_resolution_required,
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionHoldMutationTransactionV1 {
    Fresh(StoredResolutionHoldMutationResultV1),
    Replayed(StoredResolutionHoldMutationResultV1),
    Conflict {
        mutation_id: [u8; RESOLUTION_HOLD_ID_BYTES],
        character_id: [u8; RESOLUTION_HOLD_ID_BYTES],
    },
}

impl ResolutionHoldMutationTransactionV1 {
    #[must_use]
    pub const fn result(&self) -> Option<&StoredResolutionHoldMutationResultV1> {
        match self {
            Self::Fresh(result) | Self::Replayed(result) => Some(result),
            Self::Conflict { .. } => None,
        }
    }

    #[must_use]
    pub const fn is_replay(&self) -> bool {
        matches!(self, Self::Replayed(_))
    }
}

pub fn canonical_resolution_hold_stack_digest_v1(
    stack: &StoredResolutionHoldStackV1,
) -> Result<[u8; RESOLUTION_HOLD_HASH_BYTES], PersistenceError> {
    canonical_hash(
        RESOLUTION_HOLD_STACK_DIGEST_CONTEXT_V1,
        &(
            stack.extraction_id,
            stack.stack_index,
            &stack.template_id,
            &stack.content_revision,
            stack.item_kind,
            stack.extracted_at_unix_millis,
            stack.overflow_deadline_unix_millis,
            &stack.items,
        ),
    )
}

pub fn derive_resolution_hold_id_v1(
    context: &str,
    parts: &[&[u8]],
) -> [u8; RESOLUTION_HOLD_ID_BYTES] {
    let mut hasher = blake3::Hasher::new_derive_key(context);
    for part in parts {
        hasher.update(&(part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    let mut id = [0_u8; RESOLUTION_HOLD_ID_BYTES];
    id.copy_from_slice(&hasher.finalize().as_bytes()[..RESOLUTION_HOLD_ID_BYTES]);
    if id == [0; RESOLUTION_HOLD_ID_BYTES] {
        id[RESOLUTION_HOLD_ID_BYTES - 1] = 1;
    }
    id
}

pub fn canonical_resolution_hold_conflict_digest_v1(
    account_id: [u8; RESOLUTION_HOLD_ID_BYTES],
    mutation_id: [u8; RESOLUTION_HOLD_ID_BYTES],
    stored_request_hash: [u8; RESOLUTION_HOLD_HASH_BYTES],
    incoming_request_hash: [u8; RESOLUTION_HOLD_HASH_BYTES],
) -> Result<[u8; RESOLUTION_HOLD_HASH_BYTES], PersistenceError> {
    canonical_hash(
        RESOLUTION_HOLD_CONFLICT_DIGEST_CONTEXT_V1,
        &(
            account_id,
            mutation_id,
            stored_request_hash,
            incoming_request_hash,
        ),
    )
}

fn validate_logical_stack(
    template_id: &str,
    content_revision: &str,
    item_kind: StoredResolutionHoldItemKindV1,
    items: &[StoredResolutionHoldItemV1],
) -> Result<(), PersistenceError> {
    let count_valid = match item_kind {
        StoredResolutionHoldItemKindV1::Equipment => items.len() == 1,
        StoredResolutionHoldItemKindV1::Consumable => {
            template_id == CORE_TONIC_TEMPLATE_ID && (1..=6).contains(&items.len())
        }
    };
    if !valid_stable_id(template_id)
        || content_revision != CORE_ITEM_CONTENT_REVISION
        || !count_valid
    {
        return Err(corrupt());
    }
    let mut previous_uid = None;
    for item in items.iter().copied() {
        item.validate()?;
        if previous_uid.is_some_and(|previous| previous >= item.item_uid) {
            return Err(corrupt());
        }
        previous_uid = Some(item.item_uid);
    }
    Ok(())
}

fn item_uid(bytes: [u8; RESOLUTION_HOLD_ID_BYTES]) -> Result<ItemUid, PersistenceError> {
    ItemUid::new(bytes).map_err(|_| corrupt())
}

fn canonical_hash<T: Serialize>(
    context: &str,
    value: &T,
) -> Result<[u8; RESOLUTION_HOLD_HASH_BYTES], PersistenceError> {
    let payload = postcard::to_stdvec(value).map_err(|_| corrupt())?;
    if payload.is_empty() || payload.len() > MAX_RESULT_BYTES {
        return Err(corrupt());
    }
    let mut hasher = blake3::Hasher::new_derive_key(context);
    hasher.update(&(payload.len() as u64).to_be_bytes());
    hasher.update(&payload);
    Ok(*hasher.finalize().as_bytes())
}

fn valid_stable_id(value: &str) -> bool {
    (3..=96).contains(&value.len())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

const fn corrupt() -> PersistenceError {
    PersistenceError::CorruptStoredResolutionHold
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(value: u8, version: u64) -> StoredResolutionHoldItemV1 {
        StoredResolutionHoldItemV1 {
            item_uid: [value; 16],
            item_version: version,
        }
    }

    fn stack() -> StoredResolutionHoldStackV1 {
        let mut stack = StoredResolutionHoldStackV1 {
            extraction_id: [4; 16],
            stack_index: 0,
            template_id: CORE_TONIC_TEMPLATE_ID.into(),
            content_revision: CORE_ITEM_CONTENT_REVISION.into(),
            item_kind: StoredResolutionHoldItemKindV1::Consumable,
            items: vec![item(10, 1), item(11, 2)],
            stack_digest: [0; 32],
            extracted_at_unix_millis: 1_000,
            overflow_deadline_unix_millis: 1_000 + RESOLUTION_HOLD_OVERFLOW_LIFETIME_MILLIS,
            planned_destination: None,
        };
        stack.stack_digest = canonical_resolution_hold_stack_digest_v1(&stack).unwrap();
        stack
    }

    fn request() -> ResolutionHoldMutationRequestV1 {
        ResolutionHoldMutationRequestV1 {
            contract_version: RESOLUTION_HOLD_CONTRACT_VERSION_V1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            account_id: [1; 16],
            character_id: [2; 16],
            mutation_id: [3; 16],
            extraction_id: [4; 16],
            stack_index: 0,
            action: StoredResolutionHoldActionV1::Move,
            expected_versions: StoredResolutionHoldVersionsV1 {
                account: 5,
                character: 6,
                world: 6,
                inventory: 7,
            },
            content_revision: CORE_ITEM_CONTENT_REVISION.into(),
            expected_stack_digest: stack().stack_digest,
            issued_at_unix_millis: 10,
        }
    }

    fn transition(
        ordinal: u8,
        uid: u8,
        destination: StoredResolutionHoldDestinationV1,
    ) -> StoredResolutionHoldItemTransitionV1 {
        StoredResolutionHoldItemTransitionV1 {
            ordinal,
            item_uid: [uid; 16],
            template_id: CORE_TONIC_TEMPLATE_ID.into(),
            content_revision: CORE_ITEM_CONTENT_REVISION.into(),
            item_kind: StoredResolutionHoldItemKindV1::Consumable,
            disposition: StoredResolutionHoldDispositionV1::Moved(destination),
            pre_item_version: u64::from(ordinal) + 1,
            post_item_version: u64::from(ordinal) + 2,
            ledger_event_id: [uid + 20; 16],
        }
    }

    fn result() -> StoredResolutionHoldMutationResultV1 {
        let request = request();
        let destination = StoredResolutionHoldDestinationV1::Vault(0);
        StoredResolutionHoldMutationResultV1 {
            contract_version: RESOLUTION_HOLD_CONTRACT_VERSION_V1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            account_id: request.account_id,
            character_id: request.character_id,
            mutation_id: request.mutation_id,
            extraction_id: request.extraction_id,
            stack_index: request.stack_index,
            action: request.action,
            canonical_request_hash: request.canonical_hash().unwrap(),
            expected_stack_digest: request.expected_stack_digest,
            result_hash: [0; 32],
            issued_at_unix_millis: request.issued_at_unix_millis,
            committed_at_unix_millis: 20,
            versions: StoredResolutionHoldVersionVectorV1 {
                account: StoredResolutionHoldVersionAdvanceV1 { pre: 5, post: 6 },
                character: StoredResolutionHoldVersionAdvanceV1 { pre: 6, post: 7 },
                world: StoredResolutionHoldVersionAdvanceV1 { pre: 6, post: 7 },
                inventory: StoredResolutionHoldVersionAdvanceV1 { pre: 7, post: 8 },
            },
            destination: Some(destination),
            transitions: vec![
                transition(0, 10, destination),
                transition(1, 11, destination),
            ],
            remaining_hold_stack_count: 0,
            storage_resolution_required: false,
        }
        .seal()
        .unwrap()
    }

    #[test]
    fn stack_digest_binds_every_authoritative_axis_but_not_preview() {
        let stack = stack();
        stack.validate().unwrap();
        let digest = stack.stack_digest;

        let mut changed = stack.clone();
        changed.extraction_id = [5; 16];
        assert_ne!(
            canonical_resolution_hold_stack_digest_v1(&changed).unwrap(),
            digest
        );
        let mut changed = stack.clone();
        changed.stack_index = 1;
        assert_ne!(
            canonical_resolution_hold_stack_digest_v1(&changed).unwrap(),
            digest
        );
        let mut changed = stack.clone();
        changed.template_id = "consumable.changed".into();
        assert_ne!(
            canonical_resolution_hold_stack_digest_v1(&changed).unwrap(),
            digest
        );
        let mut changed = stack.clone();
        changed.content_revision.push('a');
        assert_ne!(
            canonical_resolution_hold_stack_digest_v1(&changed).unwrap(),
            digest
        );
        let mut changed = stack.clone();
        changed.item_kind = StoredResolutionHoldItemKindV1::Equipment;
        assert_ne!(
            canonical_resolution_hold_stack_digest_v1(&changed).unwrap(),
            digest
        );
        let mut changed = stack.clone();
        changed.items[0].item_uid = [9; 16];
        assert_ne!(
            canonical_resolution_hold_stack_digest_v1(&changed).unwrap(),
            digest
        );
        let mut changed = stack.clone();
        changed.items[0].item_version += 1;
        assert_ne!(
            canonical_resolution_hold_stack_digest_v1(&changed).unwrap(),
            digest
        );
        let mut changed = stack.clone();
        changed.overflow_deadline_unix_millis += 1;
        assert_ne!(
            canonical_resolution_hold_stack_digest_v1(&changed).unwrap(),
            digest
        );

        let mut changed = stack.clone();
        changed.extracted_at_unix_millis += 1;
        assert_ne!(
            canonical_resolution_hold_stack_digest_v1(&changed).unwrap(),
            digest
        );

        let mut preview = stack;
        preview.planned_destination = Some(StoredResolutionHoldDestinationV1::Vault(9));
        assert_eq!(
            canonical_resolution_hold_stack_digest_v1(&preview).unwrap(),
            digest
        );
    }

    #[test]
    fn malformed_logical_stacks_fail_closed() {
        let mut malformed = stack();
        malformed.items.swap(0, 1);
        malformed.stack_digest = canonical_resolution_hold_stack_digest_v1(&malformed).unwrap();
        assert!(matches!(
            malformed.validate(),
            Err(PersistenceError::CorruptStoredResolutionHold)
        ));

        let mut malformed = stack();
        malformed.template_id = "consumable.unknown".into();
        malformed.stack_digest = canonical_resolution_hold_stack_digest_v1(&malformed).unwrap();
        assert!(matches!(
            malformed.validate(),
            Err(PersistenceError::CorruptStoredResolutionHold)
        ));

        let mut malformed = stack();
        malformed.item_kind = StoredResolutionHoldItemKindV1::Equipment;
        malformed.stack_digest = canonical_resolution_hold_stack_digest_v1(&malformed).unwrap();
        assert!(matches!(
            malformed.validate(),
            Err(PersistenceError::CorruptStoredResolutionHold)
        ));
    }

    #[test]
    fn planner_reuses_complete_stack_order_without_splitting() {
        let selected = stack();
        let mut storage = ResolutionHoldStorageSnapshotV1::empty();
        storage.character_safe[0] = Some(ResolutionHoldStorageStackV1 {
            template_id: CORE_TONIC_TEMPLATE_ID.into(),
            content_revision: CORE_ITEM_CONTENT_REVISION.into(),
            item_kind: StoredResolutionHoldItemKindV1::Consumable,
            items: vec![item(1, 1), item(2, 1), item(3, 1), item(4, 1), item(5, 1)],
        });
        storage.vault[0] = Some(ResolutionHoldStorageStackV1 {
            template_id: CORE_TONIC_TEMPLATE_ID.into(),
            content_revision: CORE_ITEM_CONTENT_REVISION.into(),
            item_kind: StoredResolutionHoldItemKindV1::Consumable,
            items: vec![item(6, 1), item(7, 1), item(8, 1), item(9, 1)],
        });
        assert_eq!(
            plan_resolution_hold_destination_v1(&selected, &storage, 2_000).unwrap(),
            StoredResolutionHoldDestinationV1::Vault(0)
        );

        storage.vault[0].as_mut().unwrap().items.push(item(12, 1));
        assert_eq!(
            plan_resolution_hold_destination_v1(&selected, &storage, 2_000).unwrap(),
            StoredResolutionHoldDestinationV1::CharacterSafe(1)
        );
    }

    #[test]
    fn planner_uses_only_future_original_overflow_deadline() {
        let selected = stack();
        let mut storage = ResolutionHoldStorageSnapshotV1::empty();
        for (index, slot) in storage.character_safe.iter_mut().enumerate() {
            *slot = Some(ResolutionHoldStorageStackV1 {
                template_id: format!("equipment.safe_{index}"),
                content_revision: CORE_ITEM_CONTENT_REVISION.into(),
                item_kind: StoredResolutionHoldItemKindV1::Equipment,
                items: vec![item(30 + u8::try_from(index).unwrap(), 1)],
            });
        }
        for (index, slot) in storage.vault.iter_mut().enumerate() {
            let uid = u8::try_from(index).unwrap().wrapping_add(40);
            *slot = Some(ResolutionHoldStorageStackV1 {
                template_id: format!("equipment.vault_{index}"),
                content_revision: CORE_ITEM_CONTENT_REVISION.into(),
                item_kind: StoredResolutionHoldItemKindV1::Equipment,
                items: vec![item(uid, 1)],
            });
        }
        assert_eq!(
            plan_resolution_hold_destination_v1(&selected, &storage, 2_000).unwrap(),
            StoredResolutionHoldDestinationV1::Overflow(0)
        );
        assert!(matches!(
            plan_resolution_hold_destination_v1(
                &selected,
                &storage,
                selected.overflow_deadline_unix_millis,
            ),
            Err(PersistenceError::ResolutionHoldStorageFull)
        ));
    }

    #[test]
    fn request_hash_binds_every_durable_field() {
        let request = request();
        let digest = request.canonical_hash().unwrap();
        let mut changed = request.clone();
        changed.action = StoredResolutionHoldActionV1::DestroyConfirmed;
        assert_ne!(changed.canonical_hash().unwrap(), digest);
        let mut changed = request.clone();
        changed.expected_versions.inventory += 1;
        assert_ne!(changed.canonical_hash().unwrap(), digest);
        let mut changed = request;
        changed.issued_at_unix_millis += 1;
        assert_ne!(changed.canonical_hash().unwrap(), digest);
    }

    #[test]
    fn result_hash_excludes_itself_and_round_trips_canonically() {
        let stored = result();
        let payload = stored.encode().unwrap();
        assert_eq!(
            StoredResolutionHoldMutationResultV1::decode(&payload).unwrap(),
            stored
        );
        assert_eq!(stored.digest().unwrap(), stored.result_hash);

        let mut corrupt = stored;
        corrupt.transitions[0].post_item_version += 1;
        assert!(matches!(
            corrupt.validate(),
            Err(PersistenceError::CorruptStoredResolutionHold)
        ));

        let mut corrupt = result();
        corrupt.transitions[1].template_id = "consumable.changed".into();
        corrupt.result_hash = corrupt.digest().unwrap();
        assert!(matches!(
            corrupt.validate(),
            Err(PersistenceError::CorruptStoredResolutionHold)
        ));

        let mut corrupt = result();
        corrupt.transitions[1].ledger_event_id = corrupt.transitions[0].ledger_event_id;
        corrupt.result_hash = corrupt.digest().unwrap();
        assert!(matches!(
            corrupt.validate(),
            Err(PersistenceError::CorruptStoredResolutionHold)
        ));
    }

    #[test]
    fn final_clear_and_account_version_rules_are_exact() {
        let result = result();
        result.validate().unwrap();

        let mut invalid = result.clone();
        invalid.versions.account.post = invalid.versions.account.pre;
        invalid.result_hash = invalid.digest().unwrap();
        assert!(matches!(
            invalid.validate(),
            Err(PersistenceError::CorruptStoredResolutionHold)
        ));

        let mut invalid = result;
        invalid.storage_resolution_required = true;
        invalid.remaining_hold_stack_count = 1;
        invalid.result_hash = invalid.digest().unwrap();
        assert!(matches!(
            invalid.validate(),
            Err(PersistenceError::CorruptStoredResolutionHold)
        ));
    }
}
