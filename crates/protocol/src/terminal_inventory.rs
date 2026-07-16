//! Append-only protocol 1.15 contracts for successful extraction and Emergency Recall.
//!
//! The client may echo a server-issued extraction request and may start or cancel Recall. It
//! cannot author a terminal winner, destination, placement/destruction plan, post-mutation
//! version, completion tick, or stored result. Capability negotiation remains explicit so
//! disabled routes can reject these frames before any repository access.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CHARACTER_ID_BYTES, CORE_EXTRACTION_TERMINAL_FEATURE_FLAG, CORE_RECALL_TERMINAL_FEATURE_FLAG,
    MUTATION_ID_BYTES, NetworkChannel, PAYLOAD_HASH_BYTES, ServerHello, WireText,
    WorldFlowContentRevisionV1,
};

pub const TERMINAL_INVENTORY_SCHEMA_VERSION: u16 = 1;
pub const TERMINAL_INVENTORY_ID_BYTES: usize = 16;
pub const TERMINAL_INVENTORY_DIGEST_BYTES: usize = 32;
pub const EXTRACTION_PLACEMENT_CAPACITY: usize = 64;
pub const TERMINAL_MATERIAL_CAPACITY: usize = 4;
pub const TERMINAL_PENDING_ITEM_CAPACITY: u16 = 4_096;
pub const TERMINAL_STABILIZED_ITEM_CAPACITY: u16 = 16;
pub const TERMINAL_HALL_CONTENT_ID: &str = "hub.lantern_halls_01";
pub const RECALL_CHANNEL_TICKS: u64 = 12;

const EQUIPMENT_SLOT_CAPACITY: u8 = 4;
const BELT_SLOT_CAPACITY: u8 = 2;
const CHARACTER_SAFE_CAPACITY: u8 = 8;
const VAULT_CAPACITY: u16 = 160;
const OVERFLOW_CAPACITY: u8 = 20;
const RESOLUTION_HOLD_STACK_CAPACITY: u8 = 8;
const TERMINAL_ID_MAX_BYTES: usize = 96;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalInventoryCapabilityV1 {
    ExtractionCommit,
    EmergencyRecall,
}

impl TerminalInventoryCapabilityV1 {
    #[must_use]
    pub const fn feature_flag(self) -> &'static str {
        match self {
            Self::ExtractionCommit => CORE_EXTRACTION_TERMINAL_FEATURE_FLAG,
            Self::EmergencyRecall => CORE_RECALL_TERMINAL_FEATURE_FLAG,
        }
    }

    #[must_use]
    pub fn is_advertised_by(self, hello: &ServerHello) -> bool {
        hello
            .feature_flags
            .iter()
            .any(|flag| flag.as_str() == self.feature_flag())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalExpectedVersionsV1 {
    pub account: u64,
    pub character: u64,
    pub world: u64,
    pub inventory: u64,
    pub life_clock: u64,
}

impl TerminalExpectedVersionsV1 {
    fn validate(self) -> Result<(), TerminalInventoryValidationError> {
        if [
            self.account,
            self.character,
            self.world,
            self.inventory,
            self.life_clock,
        ]
        .contains(&0)
        {
            return Err(TerminalInventoryValidationError::ZeroVersion);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionCommitPayloadV1 {
    pub extraction_request_id: [u8; TERMINAL_INVENTORY_ID_BYTES],
    pub expected_versions: TerminalExpectedVersionsV1,
    pub content_revision: WorldFlowContentRevisionV1,
}

impl ExtractionCommitPayloadV1 {
    #[must_use]
    pub fn canonical_hash(&self) -> [u8; PAYLOAD_HASH_BYTES] {
        let bytes = postcard::to_stdvec(self).expect("bounded extraction payload serializes");
        *blake3::hash(&bytes).as_bytes()
    }

    fn validate(&self) -> Result<(), TerminalInventoryValidationError> {
        if all_zero(&self.extraction_request_id) {
            return Err(TerminalInventoryValidationError::ZeroIdentity);
        }
        self.expected_versions.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionCommitFrameV1 {
    pub schema_version: u16,
    pub sequence: u32,
    pub mutation_id: [u8; MUTATION_ID_BYTES],
    pub character_id: [u8; CHARACTER_ID_BYTES],
    pub issued_at_unix_millis: u64,
    pub payload_hash: [u8; PAYLOAD_HASH_BYTES],
    pub payload: ExtractionCommitPayloadV1,
}

impl ExtractionCommitFrameV1 {
    #[must_use]
    pub const fn channel(&self) -> NetworkChannel {
        NetworkChannel::Mutation
    }

    #[must_use]
    pub const fn required_capability(&self) -> TerminalInventoryCapabilityV1 {
        TerminalInventoryCapabilityV1::ExtractionCommit
    }

    pub fn validate(&self) -> Result<(), TerminalInventoryValidationError> {
        validate_schema_and_sequence(self.schema_version, self.sequence)?;
        if all_zero(&self.payload_hash) {
            return Err(TerminalInventoryValidationError::ZeroIdentity);
        }
        if self.issued_at_unix_millis == 0 {
            return Err(TerminalInventoryValidationError::ZeroIssueTime);
        }
        self.payload.validate()?;
        validate_extraction_correlations(
            &self.mutation_id,
            &self.character_id,
            &self.payload.extraction_request_id,
        )?;
        if self.payload_hash != self.payload.canonical_hash() {
            return Err(TerminalInventoryValidationError::PayloadHashMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalVersionAdvanceV1 {
    pub before: u64,
    pub after: u64,
}

impl TerminalVersionAdvanceV1 {
    fn validate(self, may_be_unchanged: bool) -> Result<(), TerminalInventoryValidationError> {
        if self.before == 0 || self.after == 0 {
            return Err(TerminalInventoryValidationError::ZeroVersion);
        }
        let unchanged = may_be_unchanged && self.after == self.before;
        let advanced = self.before.checked_add(1) == Some(self.after);
        if !unchanged && !advanced {
            return Err(TerminalInventoryValidationError::InvalidVersionAdvance);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalVersionVectorV1 {
    pub account: TerminalVersionAdvanceV1,
    pub character: TerminalVersionAdvanceV1,
    pub world: TerminalVersionAdvanceV1,
    pub inventory: TerminalVersionAdvanceV1,
    pub life_clock: TerminalVersionAdvanceV1,
}

impl TerminalVersionVectorV1 {
    fn validate(self) -> Result<(), TerminalInventoryValidationError> {
        self.account.validate(true)?;
        self.character.validate(false)?;
        self.world.validate(false)?;
        self.inventory.validate(false)?;
        self.life_clock.validate(false)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionDestinationV1 {
    Equipped { slot_index: u8 },
    Belt { slot_index: u8 },
    CharacterSafe { slot_index: u8 },
    Vault { slot_index: u16 },
    Overflow { slot_index: u8 },
    ResolutionHold { stack_index: u8 },
}

impl ExtractionDestinationV1 {
    fn validate(self) -> Result<(), TerminalInventoryValidationError> {
        let valid = match self {
            Self::Equipped { slot_index } => slot_index < EQUIPMENT_SLOT_CAPACITY,
            Self::Belt { slot_index } => slot_index < BELT_SLOT_CAPACITY,
            Self::CharacterSafe { slot_index } => slot_index < CHARACTER_SAFE_CAPACITY,
            Self::Vault { slot_index } => slot_index < VAULT_CAPACITY,
            Self::Overflow { slot_index } => slot_index < OVERFLOW_CAPACITY,
            Self::ResolutionHold { stack_index } => stack_index < RESOLUTION_HOLD_STACK_CAPACITY,
        };
        if valid {
            Ok(())
        } else {
            Err(TerminalInventoryValidationError::InvalidDestination)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionPlacementV1 {
    pub ordinal: u16,
    pub item_uid: [u8; TERMINAL_INVENTORY_ID_BYTES],
    pub destination: ExtractionDestinationV1,
    pub item_version: u64,
}

impl ExtractionPlacementV1 {
    fn validate(self, expected_ordinal: u16) -> Result<(), TerminalInventoryValidationError> {
        if self.ordinal != expected_ordinal {
            return Err(TerminalInventoryValidationError::NoncanonicalOrdering);
        }
        if all_zero(&self.item_uid) {
            return Err(TerminalInventoryValidationError::ZeroIdentity);
        }
        if self.item_version == 0 {
            return Err(TerminalInventoryValidationError::ZeroVersion);
        }
        self.destination.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionMaterialCreditV1 {
    pub ordinal: u8,
    pub material_id: WireText<TERMINAL_ID_MAX_BYTES>,
    pub quantity: u8,
    pub wallet_balance: u32,
    pub wallet_version: u64,
}

impl ExtractionMaterialCreditV1 {
    fn validate(
        &self,
        expected_ordinal: u8,
        previous_material_id: Option<&str>,
    ) -> Result<(), TerminalInventoryValidationError> {
        if self.ordinal != expected_ordinal
            || self.quantity == 0
            || self.quantity > 99
            || self.wallet_balance == 0
            || self.wallet_version == 0
            || !valid_stable_id(self.material_id.as_str())
            || previous_material_id.is_some_and(|previous| previous >= self.material_id.as_str())
        {
            return Err(TerminalInventoryValidationError::InvalidMaterialCredit);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredExtractionTerminalResultV1 {
    pub mutation_id: [u8; MUTATION_ID_BYTES],
    pub character_id: [u8; CHARACTER_ID_BYTES],
    pub terminal_id: [u8; TERMINAL_INVENTORY_ID_BYTES],
    pub extraction_request_id: [u8; TERMINAL_INVENTORY_ID_BYTES],
    pub extraction_receipt_id: [u8; TERMINAL_INVENTORY_ID_BYTES],
    pub result_hash: [u8; TERMINAL_INVENTORY_DIGEST_BYTES],
    pub committed_at_unix_millis: u64,
    pub destination_content_id: WireText<TERMINAL_ID_MAX_BYTES>,
    pub versions: TerminalVersionVectorV1,
    pub placements: Vec<ExtractionPlacementV1>,
    pub material_credits: Vec<ExtractionMaterialCreditV1>,
    pub storage_resolution_required: bool,
}

impl StoredExtractionTerminalResultV1 {
    pub fn validate(&self) -> Result<(), TerminalInventoryValidationError> {
        if [
            self.mutation_id,
            self.character_id,
            self.terminal_id,
            self.extraction_request_id,
            self.extraction_receipt_id,
        ]
        .iter()
        .any(all_zero)
            || all_zero(&self.result_hash)
        {
            return Err(TerminalInventoryValidationError::ZeroIdentity);
        }
        let operation_ids = [
            self.mutation_id,
            self.terminal_id,
            self.extraction_request_id,
            self.extraction_receipt_id,
        ];
        if operation_ids
            .iter()
            .enumerate()
            .any(|(index, value)| operation_ids[index + 1..].contains(value))
        {
            return Err(TerminalInventoryValidationError::InvalidExtractionBinding);
        }
        if self.committed_at_unix_millis == 0 {
            return Err(TerminalInventoryValidationError::ZeroIssueTime);
        }
        if self.destination_content_id.as_str() != TERMINAL_HALL_CONTENT_ID {
            return Err(TerminalInventoryValidationError::InvalidDestination);
        }
        self.versions.validate()?;
        if self.placements.len() > EXTRACTION_PLACEMENT_CAPACITY {
            return Err(TerminalInventoryValidationError::PlacementCount);
        }
        let mut item_uids = BTreeSet::new();
        let mut has_resolution_hold = false;
        for (index, placement) in self.placements.iter().copied().enumerate() {
            let ordinal = u16::try_from(index)
                .map_err(|_| TerminalInventoryValidationError::PlacementCount)?;
            placement.validate(ordinal)?;
            if !item_uids.insert(placement.item_uid) {
                return Err(TerminalInventoryValidationError::DuplicateItem);
            }
            has_resolution_hold |= matches!(
                placement.destination,
                ExtractionDestinationV1::ResolutionHold { .. }
            );
        }
        if self.storage_resolution_required != has_resolution_hold {
            return Err(TerminalInventoryValidationError::InvalidStoredResult);
        }
        validate_material_credits(&self.material_credits)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalInventoryRejectionCodeV1 {
    FeatureDisabled,
    InvalidRequest,
    PayloadHashMismatch,
    IssuedAtInvalid,
    ContentMismatch,
    StaleAuthority,
    ForeignAuthority,
    SourceUnavailable,
    UnresolvedMutation,
    TerminalLost,
    StorageResolutionRequired,
    IdempotencyConflict,
    DatabaseUnavailable,
    CorruptStoredAuthority,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionCommitResultV1 {
    Pending {
        schema_version: u16,
        request_sequence: u32,
        mutation_id: [u8; MUTATION_ID_BYTES],
        character_id: [u8; CHARACTER_ID_BYTES],
        extraction_request_id: [u8; TERMINAL_INVENTORY_ID_BYTES],
    },
    Stored {
        schema_version: u16,
        request_sequence: u32,
        replayed: bool,
        result: Box<StoredExtractionTerminalResultV1>,
    },
    Rejected {
        schema_version: u16,
        request_sequence: u32,
        mutation_id: [u8; MUTATION_ID_BYTES],
        character_id: [u8; CHARACTER_ID_BYTES],
        extraction_request_id: [u8; TERMINAL_INVENTORY_ID_BYTES],
        code: TerminalInventoryRejectionCodeV1,
    },
}

impl ExtractionCommitResultV1 {
    pub fn validate(&self) -> Result<(), TerminalInventoryValidationError> {
        let (schema_version, request_sequence) = match self {
            Self::Pending {
                schema_version,
                request_sequence,
                ..
            }
            | Self::Stored {
                schema_version,
                request_sequence,
                ..
            }
            | Self::Rejected {
                schema_version,
                request_sequence,
                ..
            } => (*schema_version, *request_sequence),
        };
        validate_schema_and_sequence(schema_version, request_sequence)?;
        match self {
            Self::Pending {
                mutation_id,
                character_id,
                extraction_request_id,
                ..
            }
            | Self::Rejected {
                mutation_id,
                character_id,
                extraction_request_id,
                ..
            } => validate_extraction_correlations(mutation_id, character_id, extraction_request_id),
            Self::Stored { result, .. } => result.validate(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallIntentV1 {
    Start,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallFrameV1 {
    pub schema_version: u16,
    pub sequence: u32,
    pub character_id: [u8; CHARACTER_ID_BYTES],
    pub client_tick: u64,
    pub intent: RecallIntentV1,
}

impl RecallFrameV1 {
    #[must_use]
    pub const fn channel(&self) -> NetworkChannel {
        NetworkChannel::Action
    }

    #[must_use]
    pub const fn required_capability(&self) -> TerminalInventoryCapabilityV1 {
        TerminalInventoryCapabilityV1::EmergencyRecall
    }

    pub fn validate(&self) -> Result<(), TerminalInventoryValidationError> {
        validate_schema_and_sequence(self.schema_version, self.sequence)?;
        if all_zero(&self.character_id) {
            return Err(TerminalInventoryValidationError::ZeroIdentity);
        }
        if self.client_tick == 0 {
            return Err(TerminalInventoryValidationError::InvalidRecallTick);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallTerminalTriggerV1 {
    Explicit,
    LinkLost,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRecallTerminalResultV1 {
    pub character_id: [u8; CHARACTER_ID_BYTES],
    pub terminal_id: [u8; TERMINAL_INVENTORY_ID_BYTES],
    pub result_hash: [u8; TERMINAL_INVENTORY_DIGEST_BYTES],
    pub trigger: RecallTerminalTriggerV1,
    pub committed_at_unix_millis: u64,
    pub completion_tick: u64,
    pub destination_content_id: WireText<TERMINAL_ID_MAX_BYTES>,
    pub versions: TerminalVersionVectorV1,
    pub stabilized_item_count: u16,
    pub stabilized_items_digest: [u8; TERMINAL_INVENTORY_DIGEST_BYTES],
    pub destroyed_item_count: u16,
    pub destroyed_items_digest: [u8; TERMINAL_INVENTORY_DIGEST_BYTES],
    pub destroyed_material_stack_count: u8,
    pub destroyed_materials_digest: [u8; TERMINAL_INVENTORY_DIGEST_BYTES],
}

impl StoredRecallTerminalResultV1 {
    pub fn validate(&self) -> Result<(), TerminalInventoryValidationError> {
        if all_zero(&self.character_id)
            || all_zero(&self.terminal_id)
            || all_zero(&self.result_hash)
            || all_zero(&self.stabilized_items_digest)
            || all_zero(&self.destroyed_items_digest)
            || all_zero(&self.destroyed_materials_digest)
        {
            return Err(TerminalInventoryValidationError::ZeroIdentity);
        }
        if self.committed_at_unix_millis == 0 || self.completion_tick == 0 {
            return Err(TerminalInventoryValidationError::InvalidRecallTick);
        }
        if self.destination_content_id.as_str() != TERMINAL_HALL_CONTENT_ID {
            return Err(TerminalInventoryValidationError::InvalidDestination);
        }
        if self.stabilized_item_count > TERMINAL_STABILIZED_ITEM_CAPACITY
            || self.destroyed_item_count > TERMINAL_PENDING_ITEM_CAPACITY
            || usize::from(self.destroyed_material_stack_count) > TERMINAL_MATERIAL_CAPACITY
        {
            return Err(TerminalInventoryValidationError::InvalidRecallCount);
        }
        self.versions.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallResultV1 {
    Pending {
        schema_version: u16,
        request_sequence: u32,
        character_id: [u8; CHARACTER_ID_BYTES],
        started_tick: u64,
        completion_tick: u64,
        pending_item_count: u16,
        pending_material_stack_count: u8,
    },
    Cancelled {
        schema_version: u16,
        request_sequence: u32,
        character_id: [u8; CHARACTER_ID_BYTES],
        started_tick: u64,
        cancelled_tick: u64,
    },
    Stored {
        schema_version: u16,
        request_sequence: Option<u32>,
        replayed: bool,
        result: Box<StoredRecallTerminalResultV1>,
    },
    Rejected {
        schema_version: u16,
        request_sequence: u32,
        character_id: [u8; CHARACTER_ID_BYTES],
        code: TerminalInventoryRejectionCodeV1,
    },
}

impl RecallResultV1 {
    pub fn validate(&self) -> Result<(), TerminalInventoryValidationError> {
        match self {
            Self::Pending {
                schema_version,
                request_sequence,
                character_id,
                started_tick,
                completion_tick,
                pending_item_count,
                pending_material_stack_count,
            } => {
                validate_schema_and_sequence(*schema_version, *request_sequence)?;
                validate_character(character_id)?;
                if *started_tick == 0
                    || started_tick.checked_add(RECALL_CHANNEL_TICKS) != Some(*completion_tick)
                {
                    return Err(TerminalInventoryValidationError::InvalidRecallTick);
                }
                if *pending_item_count > TERMINAL_PENDING_ITEM_CAPACITY
                    || usize::from(*pending_material_stack_count) > TERMINAL_MATERIAL_CAPACITY
                {
                    return Err(TerminalInventoryValidationError::InvalidRecallCount);
                }
                Ok(())
            }
            Self::Cancelled {
                schema_version,
                request_sequence,
                character_id,
                started_tick,
                cancelled_tick,
            } => {
                validate_schema_and_sequence(*schema_version, *request_sequence)?;
                validate_character(character_id)?;
                let completion_tick = started_tick.checked_add(RECALL_CHANNEL_TICKS);
                if *started_tick == 0
                    || *cancelled_tick < *started_tick
                    || completion_tick.is_none_or(|completion| *cancelled_tick >= completion)
                {
                    return Err(TerminalInventoryValidationError::InvalidRecallTick);
                }
                Ok(())
            }
            Self::Stored {
                schema_version,
                request_sequence,
                result,
                ..
            } => {
                if *schema_version != TERMINAL_INVENTORY_SCHEMA_VERSION {
                    return Err(TerminalInventoryValidationError::UnsupportedSchemaVersion);
                }
                let request_binding_valid = match result.trigger {
                    RecallTerminalTriggerV1::Explicit => {
                        request_sequence.is_some_and(|sequence| sequence != 0)
                    }
                    RecallTerminalTriggerV1::LinkLost => request_sequence.is_none(),
                };
                if !request_binding_valid {
                    return Err(TerminalInventoryValidationError::InvalidRecallBinding);
                }
                result.validate()
            }
            Self::Rejected {
                schema_version,
                request_sequence,
                character_id,
                ..
            } => {
                validate_schema_and_sequence(*schema_version, *request_sequence)?;
                validate_character(character_id)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TerminalInventoryValidationError {
    #[error("terminal-inventory schema version is unsupported")]
    UnsupportedSchemaVersion,
    #[error("terminal-inventory sequence must be nonzero")]
    ZeroSequence,
    #[error("terminal-inventory identity or digest must be nonzero")]
    ZeroIdentity,
    #[error("terminal-inventory issue/commit time must be nonzero")]
    ZeroIssueTime,
    #[error("terminal-inventory aggregate version must be positive")]
    ZeroVersion,
    #[error("terminal-inventory aggregate versions must remain equal or advance exactly once")]
    InvalidVersionAdvance,
    #[error("extraction payload hash does not match its canonical payload")]
    PayloadHashMismatch,
    #[error("extraction operation identities are not distinct and correctly correlated")]
    InvalidExtractionBinding,
    #[error("extraction placement map exceeds its bounded capacity")]
    PlacementCount,
    #[error("terminal-inventory destination is invalid")]
    InvalidDestination,
    #[error("terminal-inventory projection ordering is not canonical")]
    NoncanonicalOrdering,
    #[error("extraction placement map contains a duplicate item UID")]
    DuplicateItem,
    #[error("extraction material credit is invalid or out of canonical order")]
    InvalidMaterialCredit,
    #[error("stored terminal result disagrees with its authoritative projections")]
    InvalidStoredResult,
    #[error("Recall tick evidence does not match the exact twelve-tick channel")]
    InvalidRecallTick,
    #[error("Recall item or material counts exceed their bounded protocol limits")]
    InvalidRecallCount,
    #[error("Recall trigger and request correlation disagree")]
    InvalidRecallBinding,
}

fn validate_schema_and_sequence(
    schema_version: u16,
    sequence: u32,
) -> Result<(), TerminalInventoryValidationError> {
    if schema_version != TERMINAL_INVENTORY_SCHEMA_VERSION {
        return Err(TerminalInventoryValidationError::UnsupportedSchemaVersion);
    }
    if sequence == 0 {
        return Err(TerminalInventoryValidationError::ZeroSequence);
    }
    Ok(())
}

fn validate_extraction_correlations(
    mutation_id: &[u8; MUTATION_ID_BYTES],
    character_id: &[u8; CHARACTER_ID_BYTES],
    extraction_request_id: &[u8; TERMINAL_INVENTORY_ID_BYTES],
) -> Result<(), TerminalInventoryValidationError> {
    if all_zero(mutation_id) || all_zero(character_id) || all_zero(extraction_request_id) {
        return Err(TerminalInventoryValidationError::ZeroIdentity);
    }
    if mutation_id == extraction_request_id {
        return Err(TerminalInventoryValidationError::InvalidExtractionBinding);
    }
    Ok(())
}

fn validate_character(
    character_id: &[u8; CHARACTER_ID_BYTES],
) -> Result<(), TerminalInventoryValidationError> {
    if all_zero(character_id) {
        Err(TerminalInventoryValidationError::ZeroIdentity)
    } else {
        Ok(())
    }
}

fn validate_material_credits(
    credits: &[ExtractionMaterialCreditV1],
) -> Result<(), TerminalInventoryValidationError> {
    if credits.len() > TERMINAL_MATERIAL_CAPACITY {
        return Err(TerminalInventoryValidationError::InvalidMaterialCredit);
    }
    let mut previous = None;
    for (index, credit) in credits.iter().enumerate() {
        let ordinal = u8::try_from(index)
            .map_err(|_| TerminalInventoryValidationError::InvalidMaterialCredit)?;
        credit.validate(ordinal, previous)?;
        previous = Some(credit.material_id.as_str());
    }
    Ok(())
}

fn all_zero<const N: usize>(bytes: &[u8; N]) -> bool {
    bytes.iter().all(|byte| *byte == 0)
}

fn valid_stable_id(value: &str) -> bool {
    let mut segments = value.split('.');
    let valid_segment = |segment: &str| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    };
    let first = segments.next().is_some_and(valid_segment);
    let second = segments.next().is_some_and(valid_segment);
    first && second && segments.all(valid_segment)
}

#[cfg(test)]
mod tests {
    use crate::{ManifestHash, ProtocolVersion, SIMULATION_HZ, SNAPSHOT_HZ, ServerHello};

    use super::*;

    fn content_revision() -> WorldFlowContentRevisionV1 {
        WorldFlowContentRevisionV1 {
            records_blake3: ManifestHash::new("a".repeat(64)).unwrap(),
            assets_blake3: ManifestHash::new("b".repeat(64)).unwrap(),
            localization_blake3: ManifestHash::new("c".repeat(64)).unwrap(),
        }
    }

    fn expected_versions() -> TerminalExpectedVersionsV1 {
        TerminalExpectedVersionsV1 {
            account: 2,
            character: 3,
            world: 3,
            inventory: 4,
            life_clock: 5,
        }
    }

    fn version_vector() -> TerminalVersionVectorV1 {
        TerminalVersionVectorV1 {
            account: TerminalVersionAdvanceV1 {
                before: 2,
                after: 3,
            },
            character: TerminalVersionAdvanceV1 {
                before: 3,
                after: 4,
            },
            world: TerminalVersionAdvanceV1 {
                before: 3,
                after: 4,
            },
            inventory: TerminalVersionAdvanceV1 {
                before: 4,
                after: 5,
            },
            life_clock: TerminalVersionAdvanceV1 {
                before: 5,
                after: 6,
            },
        }
    }

    fn extraction_frame() -> ExtractionCommitFrameV1 {
        let payload = ExtractionCommitPayloadV1 {
            extraction_request_id: [3; 16],
            expected_versions: expected_versions(),
            content_revision: content_revision(),
        };
        ExtractionCommitFrameV1 {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            sequence: 1,
            mutation_id: [1; 16],
            character_id: [2; 16],
            issued_at_unix_millis: 10,
            payload_hash: payload.canonical_hash(),
            payload,
        }
    }

    fn stored_extraction() -> StoredExtractionTerminalResultV1 {
        StoredExtractionTerminalResultV1 {
            mutation_id: [1; 16],
            character_id: [2; 16],
            terminal_id: [4; 16],
            extraction_request_id: [3; 16],
            extraction_receipt_id: [5; 16],
            result_hash: [6; 32],
            committed_at_unix_millis: 11,
            destination_content_id: WireText::new(TERMINAL_HALL_CONTENT_ID).unwrap(),
            versions: version_vector(),
            placements: vec![
                ExtractionPlacementV1 {
                    ordinal: 0,
                    item_uid: [7; 16],
                    destination: ExtractionDestinationV1::Equipped { slot_index: 0 },
                    item_version: 2,
                },
                ExtractionPlacementV1 {
                    ordinal: 1,
                    item_uid: [8; 16],
                    destination: ExtractionDestinationV1::ResolutionHold { stack_index: 0 },
                    item_version: 3,
                },
            ],
            material_credits: vec![ExtractionMaterialCreditV1 {
                ordinal: 0,
                material_id: WireText::new("material.bell_brass").unwrap(),
                quantity: 2,
                wallet_balance: 9,
                wallet_version: 4,
            }],
            storage_resolution_required: true,
        }
    }

    fn stored_recall(trigger: RecallTerminalTriggerV1) -> StoredRecallTerminalResultV1 {
        StoredRecallTerminalResultV1 {
            character_id: [2; 16],
            terminal_id: [9; 16],
            result_hash: [10; 32],
            trigger,
            committed_at_unix_millis: 12,
            completion_tick: 112,
            destination_content_id: WireText::new(TERMINAL_HALL_CONTENT_ID).unwrap(),
            versions: version_vector(),
            stabilized_item_count: 6,
            stabilized_items_digest: [11; 32],
            destroyed_item_count: 3,
            destroyed_items_digest: [12; 32],
            destroyed_material_stack_count: 1,
            destroyed_materials_digest: [13; 32],
        }
    }

    fn server_hello(feature_flags: Vec<WireText<64>>) -> ServerHello {
        ServerHello {
            session_id: WireText::new("session-1").unwrap(),
            protocol_major: ProtocolVersion::current().major,
            protocol_minor: ProtocolVersion::current().minor,
            required_client_build: WireText::new("build-1").unwrap(),
            content_bundle_version: WireText::new("core-dev").unwrap(),
            server_tick_rate: SIMULATION_HZ,
            snapshot_rate: SNAPSHOT_HZ,
            region_id: WireText::new("local").unwrap(),
            feature_flags,
        }
    }

    #[test]
    fn extraction_frame_binds_every_client_owned_field_and_channel() {
        let frame = extraction_frame();
        assert_eq!(frame.validate(), Ok(()));
        assert_eq!(frame.channel(), NetworkChannel::Mutation);
        assert_eq!(
            frame.required_capability(),
            TerminalInventoryCapabilityV1::ExtractionCommit
        );

        let mut altered = frame;
        altered.payload.expected_versions.inventory += 1;
        assert_eq!(
            altered.validate(),
            Err(TerminalInventoryValidationError::PayloadHashMismatch)
        );

        let mut aliased = extraction_frame();
        aliased.mutation_id = aliased.payload.extraction_request_id;
        assert_eq!(
            aliased.validate(),
            Err(TerminalInventoryValidationError::InvalidExtractionBinding)
        );
    }

    #[test]
    fn extraction_stored_result_is_bounded_canonical_and_hold_aware() {
        let result = stored_extraction();
        assert_eq!(result.validate(), Ok(()));

        let mut duplicate = result.clone();
        duplicate.placements[1].item_uid = duplicate.placements[0].item_uid;
        assert_eq!(
            duplicate.validate(),
            Err(TerminalInventoryValidationError::DuplicateItem)
        );

        let mut mismatched_hold = result.clone();
        mismatched_hold.storage_resolution_required = false;
        assert_eq!(
            mismatched_hold.validate(),
            Err(TerminalInventoryValidationError::InvalidStoredResult)
        );

        let mut aliased_identity = result.clone();
        aliased_identity.terminal_id = aliased_identity.extraction_receipt_id;
        assert_eq!(
            aliased_identity.validate(),
            Err(TerminalInventoryValidationError::InvalidExtractionBinding)
        );

        let mut invalid_slot = result;
        invalid_slot.placements[0].destination =
            ExtractionDestinationV1::Overflow { slot_index: 20 };
        assert_eq!(
            invalid_slot.validate(),
            Err(TerminalInventoryValidationError::InvalidDestination)
        );

        let mut oversized = stored_extraction();
        oversized.placements = (0..=EXTRACTION_PLACEMENT_CAPACITY)
            .map(|index| ExtractionPlacementV1 {
                ordinal: u16::try_from(index).unwrap(),
                item_uid: u128::try_from(index + 1).unwrap().to_be_bytes(),
                destination: ExtractionDestinationV1::Vault {
                    slot_index: u16::try_from(index).unwrap(),
                },
                item_version: 2,
            })
            .collect();
        oversized.storage_resolution_required = false;
        assert_eq!(
            oversized.validate(),
            Err(TerminalInventoryValidationError::PlacementCount)
        );
    }

    #[test]
    fn extraction_results_separate_pending_stored_replay_and_rejection() {
        let pending = ExtractionCommitResultV1::Pending {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            request_sequence: 1,
            mutation_id: [1; 16],
            character_id: [2; 16],
            extraction_request_id: [3; 16],
        };
        assert_eq!(pending.validate(), Ok(()));

        let stored = ExtractionCommitResultV1::Stored {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            request_sequence: 1,
            replayed: true,
            result: Box::new(stored_extraction()),
        };
        assert_eq!(stored.validate(), Ok(()));

        let rejected = ExtractionCommitResultV1::Rejected {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            request_sequence: 1,
            mutation_id: [1; 16],
            character_id: [2; 16],
            extraction_request_id: [3; 16],
            code: TerminalInventoryRejectionCodeV1::IdempotencyConflict,
        };
        assert_eq!(rejected.validate(), Ok(()));
    }

    #[test]
    fn recall_intent_and_results_pin_twelve_tick_authority() {
        let frame = RecallFrameV1 {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            sequence: 7,
            character_id: [2; 16],
            client_tick: 99,
            intent: RecallIntentV1::Start,
        };
        assert_eq!(frame.validate(), Ok(()));
        assert_eq!(frame.channel(), NetworkChannel::Action);
        assert_eq!(
            frame.required_capability(),
            TerminalInventoryCapabilityV1::EmergencyRecall
        );
        let mut zero_tick = frame;
        zero_tick.client_tick = 0;
        assert_eq!(
            zero_tick.validate(),
            Err(TerminalInventoryValidationError::InvalidRecallTick)
        );

        let pending = RecallResultV1::Pending {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            request_sequence: 7,
            character_id: [2; 16],
            started_tick: 100,
            completion_tick: 112,
            pending_item_count: 3,
            pending_material_stack_count: 1,
        };
        assert_eq!(pending.validate(), Ok(()));

        let invalid = RecallResultV1::Pending {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            request_sequence: 7,
            character_id: [2; 16],
            started_tick: 100,
            completion_tick: 111,
            pending_item_count: 3,
            pending_material_stack_count: 1,
        };
        assert_eq!(
            invalid.validate(),
            Err(TerminalInventoryValidationError::InvalidRecallTick)
        );

        let cancelled = RecallResultV1::Cancelled {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            request_sequence: 8,
            character_id: [2; 16],
            started_tick: 100,
            cancelled_tick: 111,
        };
        assert_eq!(cancelled.validate(), Ok(()));
    }

    #[test]
    fn stored_recall_trigger_owns_request_binding_and_loss_digests() {
        let explicit = RecallResultV1::Stored {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            request_sequence: Some(7),
            replayed: true,
            result: Box::new(stored_recall(RecallTerminalTriggerV1::Explicit)),
        };
        assert_eq!(explicit.validate(), Ok(()));

        let automatic = RecallResultV1::Stored {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            request_sequence: None,
            replayed: false,
            result: Box::new(stored_recall(RecallTerminalTriggerV1::LinkLost)),
        };
        assert_eq!(automatic.validate(), Ok(()));

        let invalid = RecallResultV1::Stored {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            request_sequence: Some(7),
            replayed: false,
            result: Box::new(stored_recall(RecallTerminalTriggerV1::LinkLost)),
        };
        assert_eq!(
            invalid.validate(),
            Err(TerminalInventoryValidationError::InvalidRecallBinding)
        );

        let mut oversized = stored_recall(RecallTerminalTriggerV1::Explicit);
        oversized.stabilized_item_count = TERMINAL_STABILIZED_ITEM_CAPACITY + 1;
        assert_eq!(
            oversized.validate(),
            Err(TerminalInventoryValidationError::InvalidRecallCount)
        );
    }

    #[test]
    fn capabilities_are_explicit_and_disabled_rejections_are_typed() {
        let disabled = server_hello(Vec::new());
        assert!(!TerminalInventoryCapabilityV1::ExtractionCommit.is_advertised_by(&disabled));
        assert!(!TerminalInventoryCapabilityV1::EmergencyRecall.is_advertised_by(&disabled));

        let enabled = server_hello(vec![
            WireText::new(CORE_EXTRACTION_TERMINAL_FEATURE_FLAG).unwrap(),
            WireText::new(CORE_RECALL_TERMINAL_FEATURE_FLAG).unwrap(),
        ]);
        assert!(TerminalInventoryCapabilityV1::ExtractionCommit.is_advertised_by(&enabled));
        assert!(TerminalInventoryCapabilityV1::EmergencyRecall.is_advertised_by(&enabled));

        let extraction_rejection = ExtractionCommitResultV1::Rejected {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            request_sequence: 1,
            mutation_id: [1; 16],
            character_id: [2; 16],
            extraction_request_id: [3; 16],
            code: TerminalInventoryRejectionCodeV1::FeatureDisabled,
        };
        let recall_rejection = RecallResultV1::Rejected {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            request_sequence: 1,
            character_id: [2; 16],
            code: TerminalInventoryRejectionCodeV1::FeatureDisabled,
        };
        assert_eq!(extraction_rejection.validate(), Ok(()));
        assert_eq!(recall_rejection.validate(), Ok(()));
    }
}
