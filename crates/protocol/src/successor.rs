//! Append-only protocol 1.17 contract for M03 successor recovery.
//!
//! Authorities: `Gravebound_Production_GDD_v1_Canonical.md` `DTH-020`/`021`,
//! `UI-007`-`009`, and `TECH-021`-`023`; `Gravebound_Content_Production_Spec_v1.md`
//! `CONT-CATALOG-003`; `Gravebound_Development_Roadmap_v1.md` `GB-M03-07`; and
//! accepted `SPEC-CONFLICT-031`.
//!
//! The client supplies only the durable death identity and one mutation identity. It cannot
//! author the successor, roster slot, class, appearance, starter items, selection, or versions.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CHARACTER_ID_BYTES, CLASS_ID_MAX_BYTES, CORE_CHARACTER_SLOT_CAPACITY,
    CORE_SUCCESSOR_FEATURE_FLAG, GRAVE_ARBALIST_CLASS_ID, MUTATION_ID_BYTES, NetworkChannel,
    PAYLOAD_HASH_BYTES, WireText,
};

pub const SUCCESSOR_SCHEMA_VERSION: u16 = 1;
pub const SUCCESSOR_ID_BYTES: usize = 16;
pub const SUCCESSOR_RESULT_HASH_BYTES: usize = 32;
pub const SUCCESSOR_CONTENT_ID_MAX_BYTES: usize = 96;
pub const SUCCESSOR_STARTER_ITEM_COUNT: usize = 4;
pub const CORE_SUCCESSOR_BASE_SILHOUETTE_ID: &str = "sprite.class.grave_arbalist";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuccessorCreatePayloadV1 {
    pub death_id: [u8; SUCCESSOR_ID_BYTES],
    pub content_revision: WireText<SUCCESSOR_CONTENT_ID_MAX_BYTES>,
}

impl SuccessorCreatePayloadV1 {
    #[must_use]
    pub fn canonical_hash(&self) -> [u8; PAYLOAD_HASH_BYTES] {
        let bytes = postcard::to_stdvec(self).expect("bounded successor payload serializes");
        *blake3::hash(&bytes).as_bytes()
    }

    fn validate(&self) -> Result<(), SuccessorValidationError> {
        if all_zero(&self.death_id) {
            return Err(SuccessorValidationError::ZeroIdentity);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuccessorCreateFrameV1 {
    pub schema_version: u16,
    pub sequence: u32,
    pub mutation_id: [u8; MUTATION_ID_BYTES],
    pub payload_hash: [u8; PAYLOAD_HASH_BYTES],
    pub payload: SuccessorCreatePayloadV1,
}

impl SuccessorCreateFrameV1 {
    #[must_use]
    pub const fn channel(&self) -> NetworkChannel {
        NetworkChannel::Mutation
    }

    #[must_use]
    pub const fn required_feature_flag(&self) -> &'static str {
        CORE_SUCCESSOR_FEATURE_FLAG
    }

    pub fn validate(&self) -> Result<(), SuccessorValidationError> {
        validate_schema_and_sequence(self.schema_version, self.sequence)?;
        if all_zero(&self.mutation_id) || all_zero(&self.payload_hash) {
            return Err(SuccessorValidationError::ZeroIdentity);
        }
        self.payload.validate()?;
        if self.mutation_id == self.payload.death_id {
            return Err(SuccessorValidationError::InvalidBinding);
        }
        if self.payload_hash != self.payload.canonical_hash() {
            return Err(SuccessorValidationError::PayloadHashMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuccessorAppearanceSnapshotV1 {
    CoreBaseSilhouette,
}

impl SuccessorAppearanceSnapshotV1 {
    #[must_use]
    pub const fn content_id(self) -> &'static str {
        match self {
            Self::CoreBaseSilhouette => CORE_SUCCESSOR_BASE_SILHOUETTE_ID,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuccessorStarterItemsV1 {
    pub weapon_uid: [u8; SUCCESSOR_ID_BYTES],
    pub relic_uid: [u8; SUCCESSOR_ID_BYTES],
    pub tonic_unit_uids: [[u8; SUCCESSOR_ID_BYTES]; 2],
}

impl SuccessorStarterItemsV1 {
    fn validate(self) -> Result<(), SuccessorValidationError> {
        let ids = [
            self.weapon_uid,
            self.relic_uid,
            self.tonic_unit_uids[0],
            self.tonic_unit_uids[1],
        ];
        if ids.iter().any(all_zero) {
            return Err(SuccessorValidationError::ZeroIdentity);
        }
        for (index, id) in ids.iter().enumerate() {
            if ids[..index].contains(id) {
                return Err(SuccessorValidationError::DuplicateStarterIdentity);
            }
        }
        Ok(())
    }

    #[must_use]
    pub const fn ordered_uids(self) -> [[u8; SUCCESSOR_ID_BYTES]; SUCCESSOR_STARTER_ITEM_COUNT] {
        [
            self.weapon_uid,
            self.relic_uid,
            self.tonic_unit_uids[0],
            self.tonic_unit_uids[1],
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuccessorVersionVectorV1 {
    pub account: u64,
    pub character: u64,
    pub progression: u64,
    pub world: u64,
    pub inventory: u64,
    pub life_metrics: u64,
    pub oath_bargain: u64,
}

impl SuccessorVersionVectorV1 {
    fn validate(self) -> Result<(), SuccessorValidationError> {
        if [
            self.account,
            self.character,
            self.progression,
            self.world,
            self.inventory,
            self.life_metrics,
            self.oath_bargain,
        ]
        .contains(&0)
        {
            return Err(SuccessorValidationError::ZeroVersion);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSuccessorResultV1 {
    pub mutation_id: [u8; MUTATION_ID_BYTES],
    pub death_id: [u8; SUCCESSOR_ID_BYTES],
    pub successor_id: [u8; CHARACTER_ID_BYTES],
    pub receipt_id: [u8; SUCCESSOR_ID_BYTES],
    pub former_roster_ordinal: u8,
    pub class_id: WireText<CLASS_ID_MAX_BYTES>,
    pub appearance: SuccessorAppearanceSnapshotV1,
    pub starter_items: SuccessorStarterItemsV1,
    pub versions: SuccessorVersionVectorV1,
    pub content_revision: WireText<SUCCESSOR_CONTENT_ID_MAX_BYTES>,
    pub selected_character_id: [u8; CHARACTER_ID_BYTES],
    pub result_hash: [u8; SUCCESSOR_RESULT_HASH_BYTES],
}

impl StoredSuccessorResultV1 {
    #[must_use]
    pub fn canonical_result_hash(&self) -> [u8; SUCCESSOR_RESULT_HASH_BYTES] {
        let bytes = postcard::to_stdvec(&(
            self.mutation_id,
            self.death_id,
            self.successor_id,
            self.receipt_id,
            self.former_roster_ordinal,
            &self.class_id,
            self.appearance,
            self.starter_items,
            self.versions,
            &self.content_revision,
            self.selected_character_id,
        ))
        .expect("bounded successor result material serializes");
        *blake3::hash(&bytes).as_bytes()
    }

    pub fn validate(&self) -> Result<(), SuccessorValidationError> {
        if all_zero(&self.mutation_id)
            || all_zero(&self.death_id)
            || all_zero(&self.successor_id)
            || all_zero(&self.receipt_id)
            || all_zero(&self.selected_character_id)
            || all_zero(&self.result_hash)
        {
            return Err(SuccessorValidationError::ZeroIdentity);
        }
        if !(1..=CORE_CHARACTER_SLOT_CAPACITY).contains(&self.former_roster_ordinal)
            || self.class_id.as_str() != GRAVE_ARBALIST_CLASS_ID
            || self.selected_character_id != self.successor_id
            || self.appearance.content_id() != CORE_SUCCESSOR_BASE_SILHOUETTE_ID
        {
            return Err(SuccessorValidationError::InvalidResult);
        }
        self.starter_items.validate()?;
        let operation_ids = [
            self.mutation_id,
            self.death_id,
            self.successor_id,
            self.receipt_id,
        ];
        for (index, id) in operation_ids.iter().enumerate() {
            if operation_ids[..index].contains(id) {
                return Err(SuccessorValidationError::InvalidBinding);
            }
        }
        if self
            .starter_items
            .ordered_uids()
            .iter()
            .any(|item_uid| operation_ids.contains(item_uid))
        {
            return Err(SuccessorValidationError::InvalidBinding);
        }
        self.versions.validate()?;
        if self.result_hash != self.canonical_result_hash() {
            return Err(SuccessorValidationError::ResultHashMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuccessorRejectionCodeV1 {
    FeatureDisabled,
    InvalidRequest,
    ContentMismatch,
    ForeignAuthority,
    DeathNotFound,
    DeathNotTerminal,
    DeathSuperseded,
    AlreadyConsumed,
    SlotConflict,
    IdempotencyConflict,
    DatabaseUnavailable,
    CorruptStoredAuthority,
    UnresolvedMutation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuccessorCreateResultV1 {
    Stored {
        schema_version: u16,
        request_sequence: u32,
        replayed: bool,
        result: Box<StoredSuccessorResultV1>,
    },
    Rejected {
        schema_version: u16,
        request_sequence: u32,
        mutation_id: [u8; MUTATION_ID_BYTES],
        death_id: [u8; SUCCESSOR_ID_BYTES],
        code: SuccessorRejectionCodeV1,
    },
}

impl SuccessorCreateResultV1 {
    pub fn validate(&self) -> Result<(), SuccessorValidationError> {
        match self {
            Self::Stored {
                schema_version,
                request_sequence,
                result,
                ..
            } => {
                validate_schema_and_sequence(*schema_version, *request_sequence)?;
                result.validate()
            }
            Self::Rejected {
                schema_version,
                request_sequence,
                mutation_id,
                death_id,
                ..
            } => {
                validate_schema_and_sequence(*schema_version, *request_sequence)?;
                if all_zero(mutation_id) || all_zero(death_id) {
                    return Err(SuccessorValidationError::ZeroIdentity);
                }
                Ok(())
            }
        }
    }
}

fn validate_schema_and_sequence(
    schema_version: u16,
    sequence: u32,
) -> Result<(), SuccessorValidationError> {
    if schema_version != SUCCESSOR_SCHEMA_VERSION {
        return Err(SuccessorValidationError::SchemaVersion);
    }
    if sequence == 0 {
        return Err(SuccessorValidationError::ZeroSequence);
    }
    Ok(())
}

fn all_zero<const N: usize>(value: &[u8; N]) -> bool {
    value.iter().all(|byte| *byte == 0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SuccessorValidationError {
    #[error("successor schema version is unsupported")]
    SchemaVersion,
    #[error("successor sequence must be positive")]
    ZeroSequence,
    #[error("successor identity or hash cannot be zero")]
    ZeroIdentity,
    #[error("successor payload hash does not match its payload")]
    PayloadHashMismatch,
    #[error("successor aggregate version must be positive")]
    ZeroVersion,
    #[error("successor starter item identities must be distinct")]
    DuplicateStarterIdentity,
    #[error("successor domain identities are not independently bound")]
    InvalidBinding,
    #[error("successor stored result is invalid")]
    InvalidResult,
    #[error("successor result hash does not match its canonical result")]
    ResultHashMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    fn payload() -> SuccessorCreatePayloadV1 {
        SuccessorCreatePayloadV1 {
            death_id: [2; SUCCESSOR_ID_BYTES],
            content_revision: WireText::new("core-dev.blake3.successor-fixture").unwrap(),
        }
    }

    fn frame() -> SuccessorCreateFrameV1 {
        let payload = payload();
        SuccessorCreateFrameV1 {
            schema_version: SUCCESSOR_SCHEMA_VERSION,
            sequence: 23,
            mutation_id: [1; MUTATION_ID_BYTES],
            payload_hash: payload.canonical_hash(),
            payload,
        }
    }

    fn stored_result() -> StoredSuccessorResultV1 {
        let mut result = StoredSuccessorResultV1 {
            mutation_id: [1; MUTATION_ID_BYTES],
            death_id: [2; SUCCESSOR_ID_BYTES],
            successor_id: [3; CHARACTER_ID_BYTES],
            receipt_id: [4; SUCCESSOR_ID_BYTES],
            former_roster_ordinal: 1,
            class_id: WireText::new(GRAVE_ARBALIST_CLASS_ID).unwrap(),
            appearance: SuccessorAppearanceSnapshotV1::CoreBaseSilhouette,
            starter_items: SuccessorStarterItemsV1 {
                weapon_uid: [5; SUCCESSOR_ID_BYTES],
                relic_uid: [6; SUCCESSOR_ID_BYTES],
                tonic_unit_uids: [[7; SUCCESSOR_ID_BYTES], [8; SUCCESSOR_ID_BYTES]],
            },
            versions: SuccessorVersionVectorV1 {
                account: 9,
                character: 1,
                progression: 1,
                world: 1,
                inventory: 1,
                life_metrics: 1,
                oath_bargain: 1,
            },
            content_revision: WireText::new("core-dev.blake3.successor-fixture").unwrap(),
            selected_character_id: [3; CHARACTER_ID_BYTES],
            result_hash: [0; SUCCESSOR_RESULT_HASH_BYTES],
        };
        result.result_hash = result.canonical_result_hash();
        result
    }

    #[test]
    fn request_is_mutation_only_bounded_and_hash_bound() {
        let frame = frame();
        assert_eq!(frame.channel(), NetworkChannel::Mutation);
        assert_eq!(frame.required_feature_flag(), CORE_SUCCESSOR_FEATURE_FLAG);
        assert_eq!(frame.validate(), Ok(()));

        let mut changed = frame;
        changed.payload.death_id[0] ^= 1;
        assert_eq!(
            changed.validate(),
            Err(SuccessorValidationError::PayloadHashMismatch)
        );
    }

    #[test]
    fn stored_result_requires_exact_preset_selection_versions_and_distinct_starters() {
        let result = stored_result();
        assert_eq!(result.validate(), Ok(()));
        assert_eq!(result.selected_character_id, result.successor_id);
        assert_eq!(
            result.starter_items.ordered_uids(),
            [[5; 16], [6; 16], [7; 16], [8; 16]]
        );

        let mut duplicate = result.clone();
        duplicate.starter_items.tonic_unit_uids[1] = duplicate.starter_items.tonic_unit_uids[0];
        duplicate.result_hash = duplicate.canonical_result_hash();
        assert_eq!(
            duplicate.validate(),
            Err(SuccessorValidationError::DuplicateStarterIdentity)
        );

        let mut wrong_selection = result;
        wrong_selection.selected_character_id = [9; CHARACTER_ID_BYTES];
        wrong_selection.result_hash = wrong_selection.canonical_result_hash();
        assert_eq!(
            wrong_selection.validate(),
            Err(SuccessorValidationError::InvalidResult)
        );
    }

    #[test]
    fn stored_and_rejected_envelopes_validate_without_unbounded_authority() {
        let stored = SuccessorCreateResultV1::Stored {
            schema_version: SUCCESSOR_SCHEMA_VERSION,
            request_sequence: 23,
            replayed: false,
            result: Box::new(stored_result()),
        };
        assert_eq!(stored.validate(), Ok(()));

        let rejected = SuccessorCreateResultV1::Rejected {
            schema_version: SUCCESSOR_SCHEMA_VERSION,
            request_sequence: 23,
            mutation_id: [1; MUTATION_ID_BYTES],
            death_id: [2; SUCCESSOR_ID_BYTES],
            code: SuccessorRejectionCodeV1::FeatureDisabled,
        };
        assert_eq!(rejected.validate(), Ok(()));
    }

    #[test]
    fn malformed_zero_schema_sequence_and_identities_fail_closed() {
        let mut invalid = frame();
        invalid.schema_version = 0;
        assert_eq!(
            invalid.validate(),
            Err(SuccessorValidationError::SchemaVersion)
        );

        let mut invalid = frame();
        invalid.sequence = 0;
        assert_eq!(
            invalid.validate(),
            Err(SuccessorValidationError::ZeroSequence)
        );

        let mut invalid = frame();
        invalid.mutation_id = [0; MUTATION_ID_BYTES];
        assert_eq!(
            invalid.validate(),
            Err(SuccessorValidationError::ZeroIdentity)
        );

        let mut invalid = frame();
        invalid.payload.death_id = [0; SUCCESSOR_ID_BYTES];
        invalid.payload_hash = invalid.payload.canonical_hash();
        assert_eq!(
            invalid.validate(),
            Err(SuccessorValidationError::ZeroIdentity)
        );

        let mut invalid = frame();
        invalid.payload.death_id = invalid.mutation_id;
        invalid.payload_hash = invalid.payload.canonical_hash();
        assert_eq!(
            invalid.validate(),
            Err(SuccessorValidationError::InvalidBinding)
        );

        let mut invalid = stored_result();
        invalid.result_hash = [0; SUCCESSOR_RESULT_HASH_BYTES];
        assert_eq!(
            invalid.validate(),
            Err(SuccessorValidationError::ZeroIdentity)
        );

        let mut invalid = stored_result();
        invalid.receipt_id = invalid.death_id;
        invalid.result_hash = invalid.canonical_result_hash();
        assert_eq!(
            invalid.validate(),
            Err(SuccessorValidationError::InvalidBinding)
        );

        let mut invalid = stored_result();
        invalid.starter_items.weapon_uid = invalid.successor_id;
        invalid.result_hash = invalid.canonical_result_hash();
        assert_eq!(
            invalid.validate(),
            Err(SuccessorValidationError::InvalidBinding)
        );
    }

    #[test]
    fn content_revision_rejects_oversized_wire_text_during_decode() {
        #[derive(Serialize)]
        struct UnboundedPayload {
            death_id: [u8; SUCCESSOR_ID_BYTES],
            content_revision: String,
        }

        let raw = postcard::to_stdvec(&UnboundedPayload {
            death_id: [2; SUCCESSOR_ID_BYTES],
            content_revision: "x".repeat(SUCCESSOR_CONTENT_ID_MAX_BYTES + 1),
        })
        .unwrap();
        assert!(postcard::from_bytes::<SuccessorCreatePayloadV1>(&raw).is_err());
    }

    #[test]
    fn rejection_code_discriminants_are_append_only() {
        let codes = [
            SuccessorRejectionCodeV1::FeatureDisabled,
            SuccessorRejectionCodeV1::InvalidRequest,
            SuccessorRejectionCodeV1::ContentMismatch,
            SuccessorRejectionCodeV1::ForeignAuthority,
            SuccessorRejectionCodeV1::DeathNotFound,
            SuccessorRejectionCodeV1::DeathNotTerminal,
            SuccessorRejectionCodeV1::DeathSuperseded,
            SuccessorRejectionCodeV1::AlreadyConsumed,
            SuccessorRejectionCodeV1::SlotConflict,
            SuccessorRejectionCodeV1::IdempotencyConflict,
            SuccessorRejectionCodeV1::DatabaseUnavailable,
            SuccessorRejectionCodeV1::CorruptStoredAuthority,
            SuccessorRejectionCodeV1::UnresolvedMutation,
        ];
        for (discriminant, code) in codes.into_iter().enumerate() {
            assert_eq!(
                postcard::to_stdvec(&code).unwrap(),
                vec![u8::try_from(discriminant).unwrap()]
            );
        }
    }
}
