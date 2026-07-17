//! Append-only protocol 1.16 contracts for minimum M03 `ResolutionHold` recovery.
//!
//! Authorities: `Gravebound_Production_GDD_v1_Canonical.md` `DTH-011`,
//! `LOOT-002`, `LOOT-050`, and `TECH-021`-`023`;
//! `Gravebound_Content_Production_Spec_v1.md` `CONT-HUB-001`/`002`;
//! `Gravebound_Development_Roadmap_v1.md` `GB-M03-03`/`08`; and accepted
//! `SPEC-CONFLICT-029`/`030`.
//!
//! The client may select one server-published logical stack and a final Move or
//! `DestroyConfirmed` action. It cannot author an item UID list, destination,
//! merge target, post version, remaining-Hold state, or result hash.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CHARACTER_ID_BYTES, CORE_RESOLUTION_HOLD_FEATURE_FLAG, MUTATION_ID_BYTES, NetworkChannel,
    PAYLOAD_HASH_BYTES, WireText,
};

pub const RESOLUTION_HOLD_SCHEMA_VERSION: u16 = 1;
pub const RESOLUTION_HOLD_MAX_STACKS: usize = 8;
pub const RESOLUTION_HOLD_MAX_ITEMS: usize = 64;
pub const RESOLUTION_HOLD_ID_BYTES: usize = 16;
pub const RESOLUTION_HOLD_DIGEST_BYTES: usize = 32;
pub const RESOLUTION_HOLD_ID_MAX_BYTES: usize = 96;

const CHARACTER_SAFE_CAPACITY: u8 = 8;
const VAULT_CAPACITY: u16 = 160;
const OVERFLOW_CAPACITY: u8 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionHoldVersionsV1 {
    pub account: u64,
    pub character: u64,
    pub world: u64,
    pub inventory: u64,
}

impl ResolutionHoldVersionsV1 {
    fn validate(self) -> Result<(), ResolutionHoldValidationError> {
        if [self.account, self.character, self.world, self.inventory].contains(&0) {
            return Err(ResolutionHoldValidationError::ZeroVersion);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionHoldVersionAdvanceV1 {
    pub before: u64,
    pub after: u64,
}

impl ResolutionHoldVersionAdvanceV1 {
    fn validate(self, required: bool) -> Result<(), ResolutionHoldValidationError> {
        if self.before == 0 || self.after == 0 {
            return Err(ResolutionHoldValidationError::ZeroVersion);
        }
        let unchanged = self.before == self.after;
        let advanced = self.before.checked_add(1) == Some(self.after);
        if (!unchanged || required) && !advanced {
            return Err(ResolutionHoldValidationError::InvalidVersionAdvance);
        }
        Ok(())
    }

    fn advanced(self) -> bool {
        self.before.checked_add(1) == Some(self.after)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionHoldVersionVectorV1 {
    pub account: ResolutionHoldVersionAdvanceV1,
    pub character: ResolutionHoldVersionAdvanceV1,
    pub world: ResolutionHoldVersionAdvanceV1,
    pub inventory: ResolutionHoldVersionAdvanceV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionHoldItemKindV1 {
    Equipment,
    Consumable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionHoldItemV1 {
    pub item_uid: [u8; RESOLUTION_HOLD_ID_BYTES],
    pub item_version: u64,
}

impl ResolutionHoldItemV1 {
    fn validate(self) -> Result<(), ResolutionHoldValidationError> {
        if all_zero(&self.item_uid) {
            return Err(ResolutionHoldValidationError::ZeroIdentity);
        }
        if self.item_version == 0 {
            return Err(ResolutionHoldValidationError::ZeroVersion);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionHoldDestinationV1 {
    CharacterSafe { slot_index: u8 },
    Vault { slot_index: u16 },
    Overflow { slot_index: u8 },
}

impl ResolutionHoldDestinationV1 {
    fn validate(self) -> Result<(), ResolutionHoldValidationError> {
        let valid = match self {
            Self::CharacterSafe { slot_index } => slot_index < CHARACTER_SAFE_CAPACITY,
            Self::Vault { slot_index } => slot_index < VAULT_CAPACITY,
            Self::Overflow { slot_index } => slot_index < OVERFLOW_CAPACITY,
        };
        if valid {
            Ok(())
        } else {
            Err(ResolutionHoldValidationError::InvalidDestination)
        }
    }

    const fn account_owned(self) -> bool {
        matches!(self, Self::Vault { .. } | Self::Overflow { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionHoldStackV1 {
    pub extraction_id: [u8; RESOLUTION_HOLD_ID_BYTES],
    pub stack_index: u8,
    pub template_id: WireText<RESOLUTION_HOLD_ID_MAX_BYTES>,
    pub content_revision: WireText<RESOLUTION_HOLD_ID_MAX_BYTES>,
    pub item_kind: ResolutionHoldItemKindV1,
    pub items: Vec<ResolutionHoldItemV1>,
    pub stack_digest: [u8; RESOLUTION_HOLD_DIGEST_BYTES],
    pub extracted_at_unix_millis: u64,
    pub overflow_deadline_unix_millis: u64,
    pub planned_destination: Option<ResolutionHoldDestinationV1>,
}

impl ResolutionHoldStackV1 {
    fn validate(&self) -> Result<(), ResolutionHoldValidationError> {
        if all_zero(&self.extraction_id)
            || all_zero(&self.stack_digest)
            || usize::from(self.stack_index) >= RESOLUTION_HOLD_MAX_STACKS
            || self.items.is_empty()
            || self.items.len() > RESOLUTION_HOLD_MAX_ITEMS
            || self.extracted_at_unix_millis == 0
            || self.overflow_deadline_unix_millis <= self.extracted_at_unix_millis
        {
            return Err(ResolutionHoldValidationError::InvalidStack);
        }
        let mut previous_uid = None;
        for item in self.items.iter().copied() {
            item.validate()?;
            if previous_uid.is_some_and(|previous| previous >= item.item_uid) {
                return Err(ResolutionHoldValidationError::NoncanonicalOrdering);
            }
            previous_uid = Some(item.item_uid);
        }
        if self.item_kind == ResolutionHoldItemKindV1::Equipment && self.items.len() != 1 {
            return Err(ResolutionHoldValidationError::InvalidStack);
        }
        self.planned_destination
            .map(ResolutionHoldDestinationV1::validate)
            .transpose()?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionHoldQueryFrameV1 {
    pub schema_version: u16,
    pub sequence: u32,
    pub character_id: [u8; CHARACTER_ID_BYTES],
}

impl ResolutionHoldQueryFrameV1 {
    #[must_use]
    pub const fn channel(&self) -> NetworkChannel {
        NetworkChannel::Control
    }

    #[must_use]
    pub const fn required_feature_flag(&self) -> &'static str {
        CORE_RESOLUTION_HOLD_FEATURE_FLAG
    }

    pub fn validate(&self) -> Result<(), ResolutionHoldValidationError> {
        validate_schema_sequence_character(self.schema_version, self.sequence, self.character_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionHoldRejectionCodeV1 {
    FeatureDisabled,
    InvalidRequest,
    IssuedAtInvalid,
    ContentMismatch,
    StaleAuthority,
    ForeignAuthority,
    HallBindingRequired,
    StorageFull,
    NoHeldStack,
    ConfirmationRequired,
    IdempotencyConflict,
    DatabaseUnavailable,
    CorruptStoredAuthority,
    UnresolvedMutation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionHoldQueryResultV1 {
    Stored {
        schema_version: u16,
        request_sequence: u32,
        character_id: [u8; CHARACTER_ID_BYTES],
        versions: ResolutionHoldVersionsV1,
        storage_resolution_required: bool,
        stacks: Vec<ResolutionHoldStackV1>,
    },
    Rejected {
        schema_version: u16,
        request_sequence: u32,
        character_id: [u8; CHARACTER_ID_BYTES],
        code: ResolutionHoldRejectionCodeV1,
    },
}

impl ResolutionHoldQueryResultV1 {
    pub fn validate(&self) -> Result<(), ResolutionHoldValidationError> {
        let (schema_version, request_sequence, character_id) = match self {
            Self::Stored {
                schema_version,
                request_sequence,
                character_id,
                ..
            }
            | Self::Rejected {
                schema_version,
                request_sequence,
                character_id,
                ..
            } => (*schema_version, *request_sequence, *character_id),
        };
        validate_schema_sequence_character(schema_version, request_sequence, character_id)?;
        let Self::Stored {
            versions,
            storage_resolution_required,
            stacks,
            ..
        } = self
        else {
            return Ok(());
        };
        versions.validate()?;
        if stacks.len() > RESOLUTION_HOLD_MAX_STACKS
            || *storage_resolution_required == stacks.is_empty()
        {
            return Err(ResolutionHoldValidationError::InvalidResult);
        }
        let mut previous_key = None;
        let mut item_count = 0_usize;
        for stack in stacks {
            stack.validate()?;
            let key = (stack.extraction_id, stack.stack_index);
            if previous_key.is_some_and(|previous| previous >= key) {
                return Err(ResolutionHoldValidationError::NoncanonicalOrdering);
            }
            previous_key = Some(key);
            item_count = item_count
                .checked_add(stack.items.len())
                .ok_or(ResolutionHoldValidationError::ItemCount)?;
        }
        if item_count > RESOLUTION_HOLD_MAX_ITEMS {
            return Err(ResolutionHoldValidationError::ItemCount);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionHoldActionV1 {
    Move,
    DestroyConfirmed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionHoldMutationPayloadV1 {
    pub extraction_id: [u8; RESOLUTION_HOLD_ID_BYTES],
    pub stack_index: u8,
    pub action: ResolutionHoldActionV1,
    pub expected_versions: ResolutionHoldVersionsV1,
    pub content_revision: WireText<RESOLUTION_HOLD_ID_MAX_BYTES>,
    pub expected_stack_digest: [u8; RESOLUTION_HOLD_DIGEST_BYTES],
}

impl ResolutionHoldMutationPayloadV1 {
    #[must_use]
    pub fn canonical_hash(&self) -> [u8; PAYLOAD_HASH_BYTES] {
        let bytes = postcard::to_stdvec(self).expect("bounded Hold payload serializes");
        *blake3::hash(&bytes).as_bytes()
    }

    fn validate(&self) -> Result<(), ResolutionHoldValidationError> {
        if all_zero(&self.extraction_id)
            || all_zero(&self.expected_stack_digest)
            || usize::from(self.stack_index) >= RESOLUTION_HOLD_MAX_STACKS
        {
            return Err(ResolutionHoldValidationError::ZeroIdentity);
        }
        self.expected_versions.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionHoldMutationFrameV1 {
    pub schema_version: u16,
    pub sequence: u32,
    pub mutation_id: [u8; MUTATION_ID_BYTES],
    pub character_id: [u8; CHARACTER_ID_BYTES],
    pub issued_at_unix_millis: u64,
    pub payload_hash: [u8; PAYLOAD_HASH_BYTES],
    pub payload: ResolutionHoldMutationPayloadV1,
}

impl ResolutionHoldMutationFrameV1 {
    #[must_use]
    pub const fn channel(&self) -> NetworkChannel {
        NetworkChannel::Mutation
    }

    #[must_use]
    pub const fn required_feature_flag(&self) -> &'static str {
        CORE_RESOLUTION_HOLD_FEATURE_FLAG
    }

    pub fn validate(&self) -> Result<(), ResolutionHoldValidationError> {
        validate_schema_sequence_character(self.schema_version, self.sequence, self.character_id)?;
        if all_zero(&self.mutation_id)
            || all_zero(&self.payload_hash)
            || self.issued_at_unix_millis == 0
        {
            return Err(ResolutionHoldValidationError::ZeroIdentity);
        }
        self.payload.validate()?;
        if self.payload_hash != self.payload.canonical_hash() {
            return Err(ResolutionHoldValidationError::PayloadHashMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionHoldDispositionV1 {
    Moved {
        destination: ResolutionHoldDestinationV1,
    },
    Destroyed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionHoldItemTransitionV1 {
    pub ordinal: u8,
    pub item_uid: [u8; RESOLUTION_HOLD_ID_BYTES],
    pub item_version: u64,
    pub disposition: ResolutionHoldDispositionV1,
}

impl ResolutionHoldItemTransitionV1 {
    fn validate(self, expected_ordinal: u8) -> Result<(), ResolutionHoldValidationError> {
        if self.ordinal != expected_ordinal || all_zero(&self.item_uid) {
            return Err(ResolutionHoldValidationError::NoncanonicalOrdering);
        }
        if self.item_version == 0 {
            return Err(ResolutionHoldValidationError::ZeroVersion);
        }
        if let ResolutionHoldDispositionV1::Moved { destination } = self.disposition {
            destination.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredResolutionHoldMutationResultV1 {
    pub mutation_id: [u8; MUTATION_ID_BYTES],
    pub character_id: [u8; CHARACTER_ID_BYTES],
    pub extraction_id: [u8; RESOLUTION_HOLD_ID_BYTES],
    pub stack_index: u8,
    pub action: ResolutionHoldActionV1,
    pub result_hash: [u8; RESOLUTION_HOLD_DIGEST_BYTES],
    pub committed_at_unix_millis: u64,
    pub versions: ResolutionHoldVersionVectorV1,
    pub transitions: Vec<ResolutionHoldItemTransitionV1>,
    pub remaining_hold_stack_count: u8,
    pub storage_resolution_required: bool,
}

impl StoredResolutionHoldMutationResultV1 {
    pub fn validate(&self) -> Result<(), ResolutionHoldValidationError> {
        if all_zero(&self.mutation_id)
            || all_zero(&self.character_id)
            || all_zero(&self.extraction_id)
            || all_zero(&self.result_hash)
            || usize::from(self.stack_index) >= RESOLUTION_HOLD_MAX_STACKS
            || self.committed_at_unix_millis == 0
            || self.transitions.is_empty()
            || self.transitions.len() > RESOLUTION_HOLD_MAX_ITEMS
            || usize::from(self.remaining_hold_stack_count) > RESOLUTION_HOLD_MAX_STACKS
            || self.storage_resolution_required != (self.remaining_hold_stack_count != 0)
        {
            return Err(ResolutionHoldValidationError::InvalidResult);
        }
        self.versions.inventory.validate(true)?;
        self.versions.account.validate(false)?;
        let final_clear = !self.storage_resolution_required;
        self.versions.character.validate(final_clear)?;
        self.versions.world.validate(final_clear)?;
        if !final_clear && (self.versions.character.advanced() || self.versions.world.advanced()) {
            return Err(ResolutionHoldValidationError::InvalidVersionAdvance);
        }
        let mut previous_uid = None;
        let mut move_destination = None;
        for (index, transition) in self.transitions.iter().copied().enumerate() {
            transition.validate(
                u8::try_from(index).map_err(|_| ResolutionHoldValidationError::ItemCount)?,
            )?;
            if previous_uid.is_some_and(|previous| previous >= transition.item_uid) {
                return Err(ResolutionHoldValidationError::NoncanonicalOrdering);
            }
            previous_uid = Some(transition.item_uid);
            match (self.action, transition.disposition) {
                (
                    ResolutionHoldActionV1::Move,
                    ResolutionHoldDispositionV1::Moved { destination },
                ) => {
                    if move_destination.is_some_and(|existing| existing != destination) {
                        return Err(ResolutionHoldValidationError::InvalidResult);
                    }
                    move_destination = Some(destination);
                }
                (
                    ResolutionHoldActionV1::DestroyConfirmed,
                    ResolutionHoldDispositionV1::Destroyed,
                ) => {}
                _ => return Err(ResolutionHoldValidationError::InvalidResult),
            }
        }
        let account_should_advance =
            move_destination.is_some_and(ResolutionHoldDestinationV1::account_owned);
        if self.versions.account.advanced() != account_should_advance {
            return Err(ResolutionHoldValidationError::InvalidVersionAdvance);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionHoldMutationResultV1 {
    Stored {
        schema_version: u16,
        request_sequence: u32,
        replayed: bool,
        result: Box<StoredResolutionHoldMutationResultV1>,
    },
    Rejected {
        schema_version: u16,
        request_sequence: u32,
        mutation_id: [u8; MUTATION_ID_BYTES],
        character_id: [u8; CHARACTER_ID_BYTES],
        extraction_id: [u8; RESOLUTION_HOLD_ID_BYTES],
        stack_index: u8,
        code: ResolutionHoldRejectionCodeV1,
    },
}

impl ResolutionHoldMutationResultV1 {
    pub fn validate(&self) -> Result<(), ResolutionHoldValidationError> {
        match self {
            Self::Stored {
                schema_version,
                request_sequence,
                result,
                ..
            } => {
                validate_schema_sequence_character(
                    *schema_version,
                    *request_sequence,
                    result.character_id,
                )?;
                result.validate()
            }
            Self::Rejected {
                schema_version,
                request_sequence,
                mutation_id,
                character_id,
                extraction_id,
                stack_index,
                ..
            } => {
                validate_schema_sequence_character(
                    *schema_version,
                    *request_sequence,
                    *character_id,
                )?;
                if all_zero(mutation_id)
                    || all_zero(extraction_id)
                    || usize::from(*stack_index) >= RESOLUTION_HOLD_MAX_STACKS
                {
                    return Err(ResolutionHoldValidationError::ZeroIdentity);
                }
                Ok(())
            }
        }
    }
}

fn validate_schema_sequence_character(
    schema_version: u16,
    sequence: u32,
    character_id: [u8; CHARACTER_ID_BYTES],
) -> Result<(), ResolutionHoldValidationError> {
    if schema_version != RESOLUTION_HOLD_SCHEMA_VERSION {
        return Err(ResolutionHoldValidationError::SchemaVersion);
    }
    if sequence == 0 {
        return Err(ResolutionHoldValidationError::ZeroSequence);
    }
    if all_zero(&character_id) {
        return Err(ResolutionHoldValidationError::ZeroIdentity);
    }
    Ok(())
}

fn all_zero<const N: usize>(value: &[u8; N]) -> bool {
    value.iter().all(|byte| *byte == 0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ResolutionHoldValidationError {
    #[error("ResolutionHold schema version is unsupported")]
    SchemaVersion,
    #[error("ResolutionHold sequence must be positive")]
    ZeroSequence,
    #[error("ResolutionHold identity or digest cannot be zero")]
    ZeroIdentity,
    #[error("ResolutionHold aggregate or item version must be positive")]
    ZeroVersion,
    #[error("ResolutionHold version transition is invalid")]
    InvalidVersionAdvance,
    #[error("ResolutionHold stack shape is invalid")]
    InvalidStack,
    #[error("ResolutionHold item count exceeds protocol bounds")]
    ItemCount,
    #[error("ResolutionHold ordering is not canonical")]
    NoncanonicalOrdering,
    #[error("ResolutionHold destination is out of bounds")]
    InvalidDestination,
    #[error("ResolutionHold payload hash does not match its payload")]
    PayloadHashMismatch,
    #[error("ResolutionHold result shape is invalid")]
    InvalidResult,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn versions() -> ResolutionHoldVersionsV1 {
        ResolutionHoldVersionsV1 {
            account: 4,
            character: 5,
            world: 5,
            inventory: 6,
        }
    }

    fn stack() -> ResolutionHoldStackV1 {
        ResolutionHoldStackV1 {
            extraction_id: [3; 16],
            stack_index: 0,
            template_id: WireText::new("consumable.red_tonic").unwrap(),
            content_revision: WireText::new("core-items-v1").unwrap(),
            item_kind: ResolutionHoldItemKindV1::Consumable,
            items: vec![
                ResolutionHoldItemV1 {
                    item_uid: [4; 16],
                    item_version: 2,
                },
                ResolutionHoldItemV1 {
                    item_uid: [5; 16],
                    item_version: 2,
                },
            ],
            stack_digest: [6; 32],
            extracted_at_unix_millis: 100,
            overflow_deadline_unix_millis: 259_200_100,
            planned_destination: Some(ResolutionHoldDestinationV1::Vault { slot_index: 12 }),
        }
    }

    fn mutation(action: ResolutionHoldActionV1) -> ResolutionHoldMutationFrameV1 {
        let payload = ResolutionHoldMutationPayloadV1 {
            extraction_id: [3; 16],
            stack_index: 0,
            action,
            expected_versions: versions(),
            content_revision: WireText::new("core-items-v1").unwrap(),
            expected_stack_digest: [6; 32],
        };
        ResolutionHoldMutationFrameV1 {
            schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
            sequence: 8,
            mutation_id: [1; 16],
            character_id: [2; 16],
            issued_at_unix_millis: 200,
            payload_hash: payload.canonical_hash(),
            payload,
        }
    }

    #[test]
    fn query_is_bounded_ordered_and_reports_the_server_plan() {
        let result = ResolutionHoldQueryResultV1::Stored {
            schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
            request_sequence: 7,
            character_id: [2; 16],
            versions: versions(),
            storage_resolution_required: true,
            stacks: vec![stack()],
        };
        result.validate().unwrap();
        let mut invalid = result;
        let ResolutionHoldQueryResultV1::Stored { stacks, .. } = &mut invalid else {
            unreachable!();
        };
        stacks[0].items.reverse();
        assert_eq!(
            invalid.validate(),
            Err(ResolutionHoldValidationError::NoncanonicalOrdering)
        );
    }

    #[test]
    fn mutation_hash_binds_action_stack_versions_and_content() {
        let frame = mutation(ResolutionHoldActionV1::Move);
        frame.validate().unwrap();
        let mut changed = frame;
        changed.payload.action = ResolutionHoldActionV1::DestroyConfirmed;
        assert_eq!(
            changed.validate(),
            Err(ResolutionHoldValidationError::PayloadHashMismatch)
        );
    }

    #[test]
    fn stored_move_requires_one_destination_and_exact_version_axes() {
        let transition = |ordinal, uid| ResolutionHoldItemTransitionV1 {
            ordinal,
            item_uid: [uid; 16],
            item_version: 3,
            disposition: ResolutionHoldDispositionV1::Moved {
                destination: ResolutionHoldDestinationV1::Vault { slot_index: 12 },
            },
        };
        let mut result = StoredResolutionHoldMutationResultV1 {
            mutation_id: [1; 16],
            character_id: [2; 16],
            extraction_id: [3; 16],
            stack_index: 0,
            action: ResolutionHoldActionV1::Move,
            result_hash: [7; 32],
            committed_at_unix_millis: 300,
            versions: ResolutionHoldVersionVectorV1 {
                account: advance(4, 5),
                character: advance(5, 6),
                world: advance(5, 6),
                inventory: advance(6, 7),
            },
            transitions: vec![transition(0, 4), transition(1, 5)],
            remaining_hold_stack_count: 0,
            storage_resolution_required: false,
        };
        result.validate().unwrap();
        result.transitions[1].disposition = ResolutionHoldDispositionV1::Moved {
            destination: ResolutionHoldDestinationV1::Vault { slot_index: 13 },
        };
        assert_eq!(
            result.validate(),
            Err(ResolutionHoldValidationError::InvalidResult)
        );
    }

    #[test]
    fn confirmed_destroy_cannot_grant_account_version_or_move_items() {
        let mut result = StoredResolutionHoldMutationResultV1 {
            mutation_id: [1; 16],
            character_id: [2; 16],
            extraction_id: [3; 16],
            stack_index: 0,
            action: ResolutionHoldActionV1::DestroyConfirmed,
            result_hash: [7; 32],
            committed_at_unix_millis: 300,
            versions: ResolutionHoldVersionVectorV1 {
                account: advance(4, 4),
                character: advance(5, 6),
                world: advance(5, 6),
                inventory: advance(6, 7),
            },
            transitions: vec![ResolutionHoldItemTransitionV1 {
                ordinal: 0,
                item_uid: [4; 16],
                item_version: 3,
                disposition: ResolutionHoldDispositionV1::Destroyed,
            }],
            remaining_hold_stack_count: 0,
            storage_resolution_required: false,
        };
        result.validate().unwrap();
        result.versions.account.after = 5;
        assert_eq!(
            result.validate(),
            Err(ResolutionHoldValidationError::InvalidVersionAdvance)
        );
    }

    #[test]
    fn rejection_codes_preserve_existing_bytes_and_append_unresolved_mutation() {
        let codes = [
            ResolutionHoldRejectionCodeV1::FeatureDisabled,
            ResolutionHoldRejectionCodeV1::InvalidRequest,
            ResolutionHoldRejectionCodeV1::IssuedAtInvalid,
            ResolutionHoldRejectionCodeV1::ContentMismatch,
            ResolutionHoldRejectionCodeV1::StaleAuthority,
            ResolutionHoldRejectionCodeV1::ForeignAuthority,
            ResolutionHoldRejectionCodeV1::HallBindingRequired,
            ResolutionHoldRejectionCodeV1::StorageFull,
            ResolutionHoldRejectionCodeV1::NoHeldStack,
            ResolutionHoldRejectionCodeV1::ConfirmationRequired,
            ResolutionHoldRejectionCodeV1::IdempotencyConflict,
            ResolutionHoldRejectionCodeV1::DatabaseUnavailable,
            ResolutionHoldRejectionCodeV1::CorruptStoredAuthority,
            ResolutionHoldRejectionCodeV1::UnresolvedMutation,
        ];
        for (index, code) in codes.into_iter().enumerate() {
            assert_eq!(
                postcard::to_stdvec(&code).unwrap(),
                vec![u8::try_from(index).unwrap()]
            );
        }
    }

    const fn advance(before: u64, after: u64) -> ResolutionHoldVersionAdvanceV1 {
        ResolutionHoldVersionAdvanceV1 { before, after }
    }
}
