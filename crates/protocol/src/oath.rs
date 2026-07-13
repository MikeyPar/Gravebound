//! Bounded reliable protocol for authoritative initial Oath selection.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ManifestHash, NetworkChannel, WireText};

pub const OATH_CHARACTER_ID_BYTES: usize = 16;
pub const OATH_MUTATION_ID_BYTES: usize = 16;
pub const OATH_PAYLOAD_HASH_BYTES: usize = 32;
pub const OATH_ID_BYTES: usize = 96;
pub const LONG_VIGIL_ID: &str = "oath.arbalist.long_vigil";
pub const NAILKEEPER_ID: &str = "oath.arbalist.nailkeeper";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OathContentRevisionV1 {
    pub records_blake3: ManifestHash,
    pub assets_blake3: ManifestHash,
    pub localization_blake3: ManifestHash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OathViewFrame {
    pub sequence: u32,
    pub character_id: [u8; OATH_CHARACTER_ID_BYTES],
    pub content_revision: OathContentRevisionV1,
}

impl OathViewFrame {
    #[must_use]
    pub const fn channel(&self) -> NetworkChannel {
        NetworkChannel::Control
    }

    pub const fn validate(&self) -> Result<(), OathValidationError> {
        if self.sequence == 0 {
            return Err(OathValidationError::ZeroSequence);
        }
        if all_zero(&self.character_id) {
            return Err(OathValidationError::ZeroCharacterId);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitialOathSelectionPayload {
    pub character_id: [u8; OATH_CHARACTER_ID_BYTES],
    pub oath_id: WireText<OATH_ID_BYTES>,
    pub content_revision: OathContentRevisionV1,
    pub confirmed: bool,
}

impl InitialOathSelectionPayload {
    pub fn canonical_hash(&self) -> [u8; OATH_PAYLOAD_HASH_BYTES] {
        let bytes = postcard::to_stdvec(self).expect("bounded Oath payload serializes");
        *blake3::hash(&bytes).as_bytes()
    }

    pub fn validate(&self) -> Result<(), OathValidationError> {
        if all_zero(&self.character_id) {
            return Err(OathValidationError::ZeroCharacterId);
        }
        if !legal_oath_id(self.oath_id.as_str()) {
            return Err(OathValidationError::IllegalOathId);
        }
        if !self.confirmed {
            return Err(OathValidationError::ConfirmationRequired);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitialOathSelectionFrame {
    pub mutation_id: [u8; OATH_MUTATION_ID_BYTES],
    pub expected_character_version: u64,
    pub payload_hash: [u8; OATH_PAYLOAD_HASH_BYTES],
    pub issued_at_unix_millis: u64,
    pub payload: InitialOathSelectionPayload,
}

impl InitialOathSelectionFrame {
    #[must_use]
    pub const fn channel(&self) -> NetworkChannel {
        NetworkChannel::Mutation
    }

    pub fn validate(&self) -> Result<(), OathValidationError> {
        if all_zero(&self.mutation_id) {
            return Err(OathValidationError::ZeroMutationId);
        }
        if self.expected_character_version == 0 {
            return Err(OathValidationError::ZeroCharacterVersion);
        }
        if all_zero(&self.payload_hash) {
            return Err(OathValidationError::ZeroPayloadHash);
        }
        if self.issued_at_unix_millis == 0 {
            return Err(OathValidationError::ZeroIssuedAt);
        }
        self.payload.validate()?;
        if self.payload_hash != self.payload.canonical_hash() {
            return Err(OathValidationError::PayloadHashMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OathSelectionState {
    Locked {
        current_level: u16,
        required_level: u16,
    },
    Eligible {
        current_level: u16,
    },
    Selected {
        current_level: u16,
        oath_id: WireText<OATH_ID_BYTES>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OathProjection {
    pub character_id: [u8; OATH_CHARACTER_ID_BYTES],
    pub character_version: u64,
    pub state: OathSelectionState,
    pub later_change_stage_disabled: bool,
}

impl OathProjection {
    pub fn validate(&self) -> Result<(), OathValidationError> {
        if all_zero(&self.character_id) {
            return Err(OathValidationError::ZeroCharacterId);
        }
        if self.character_version == 0 {
            return Err(OathValidationError::ZeroCharacterVersion);
        }
        match &self.state {
            OathSelectionState::Locked {
                current_level,
                required_level: 10,
            } if (1..10).contains(current_level) => {}
            OathSelectionState::Eligible { current_level: 10 } => {}
            OathSelectionState::Selected {
                current_level: 10,
                oath_id,
            } if legal_oath_id(oath_id.as_str()) => {}
            _ => return Err(OathValidationError::InvalidProjectionState),
        }
        if !self.later_change_stage_disabled {
            return Err(OathValidationError::LaterChangeMustBeDisabled);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OathResultCode {
    Available,
    Accepted,
    LevelRequired,
    LocationRequired,
    CharacterNotOwned,
    CharacterDead,
    CharacterNotSelected,
    ContentDisabled,
    ContentMismatch,
    InventoryNotSafe,
    UnresolvedMutation,
    StateVersionMismatch,
    IdempotencyConflict,
    PayloadHashMismatch,
    IllegalOath,
    AlreadySelected,
    StageDisabled,
    IssuedAtInvalid,
    ServiceUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OathViewResult {
    pub sequence: u32,
    pub code: OathResultCode,
    pub projection: Option<OathProjection>,
}

impl OathViewResult {
    pub fn validate(&self) -> Result<(), OathValidationError> {
        if self.sequence == 0 {
            return Err(OathValidationError::ZeroSequence);
        }
        if (self.code == OathResultCode::Available) != self.projection.is_some() {
            return Err(OathValidationError::ResultShapeMismatch);
        }
        self.projection
            .as_ref()
            .map_or(Ok(()), OathProjection::validate)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitialOathSelectionResult {
    pub mutation_id: [u8; OATH_MUTATION_ID_BYTES],
    pub code: OathResultCode,
    pub projection: Option<OathProjection>,
}

impl InitialOathSelectionResult {
    pub fn validate(&self) -> Result<(), OathValidationError> {
        if all_zero(&self.mutation_id) {
            return Err(OathValidationError::ZeroMutationId);
        }
        if self.code == OathResultCode::Available {
            return Err(OathValidationError::ResultShapeMismatch);
        }
        if self.code == OathResultCode::Accepted
            && !matches!(
                self.projection.as_ref().map(|value| &value.state),
                Some(OathSelectionState::Selected { .. })
            )
        {
            return Err(OathValidationError::ResultShapeMismatch);
        }
        self.projection
            .as_ref()
            .map_or(Ok(()), OathProjection::validate)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum OathValidationError {
    #[error("Oath message sequence must be nonzero")]
    ZeroSequence,
    #[error("Oath character ID must be nonzero")]
    ZeroCharacterId,
    #[error("Oath mutation ID must be nonzero")]
    ZeroMutationId,
    #[error("Oath character version must be nonzero")]
    ZeroCharacterVersion,
    #[error("Oath payload hash must be nonzero")]
    ZeroPayloadHash,
    #[error("Oath payload hash does not match its canonical payload")]
    PayloadHashMismatch,
    #[error("Oath mutation issue time must be nonzero")]
    ZeroIssuedAt,
    #[error("Oath ID is unavailable in Core")]
    IllegalOathId,
    #[error("explicit initial-Oath confirmation is required")]
    ConfirmationRequired,
    #[error("Oath projection state is inconsistent")]
    InvalidProjectionState,
    #[error("later Oath changes must remain stage-disabled")]
    LaterChangeMustBeDisabled,
    #[error("Oath result code and projection disagree")]
    ResultShapeMismatch,
}

fn legal_oath_id(value: &str) -> bool {
    matches!(value, LONG_VIGIL_ID | NAILKEEPER_ID)
}

const fn all_zero<const N: usize>(bytes: &[u8; N]) -> bool {
    let mut index = 0;
    while index < N {
        if bytes[index] != 0 {
            return false;
        }
        index += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn revision() -> OathContentRevisionV1 {
        OathContentRevisionV1 {
            records_blake3: ManifestHash::new("1".repeat(64)).unwrap(),
            assets_blake3: ManifestHash::new("2".repeat(64)).unwrap(),
            localization_blake3: ManifestHash::new("3".repeat(64)).unwrap(),
        }
    }

    fn payload() -> InitialOathSelectionPayload {
        InitialOathSelectionPayload {
            character_id: [1; 16],
            oath_id: WireText::new(LONG_VIGIL_ID).unwrap(),
            content_revision: revision(),
            confirmed: true,
        }
    }

    #[test]
    fn view_and_mutation_are_bounded_reliable_and_content_bound() {
        let view = OathViewFrame {
            sequence: 1,
            character_id: [1; 16],
            content_revision: revision(),
        };
        assert_eq!(view.validate(), Ok(()));
        assert_eq!(view.channel(), NetworkChannel::Control);
        let payload = payload();
        let frame = InitialOathSelectionFrame {
            mutation_id: [2; 16],
            expected_character_version: 7,
            payload_hash: payload.canonical_hash(),
            issued_at_unix_millis: 1,
            payload,
        };
        assert_eq!(frame.validate(), Ok(()));
        assert_eq!(frame.channel(), NetworkChannel::Mutation);

        let mut tampered = frame;
        tampered.payload.oath_id = WireText::new(NAILKEEPER_ID).unwrap();
        assert_eq!(
            tampered.validate(),
            Err(OathValidationError::PayloadHashMismatch)
        );
    }

    #[test]
    fn state_matrix_and_later_change_are_fail_closed() {
        for level in 1..10 {
            OathProjection {
                character_id: [1; 16],
                character_version: 1,
                state: OathSelectionState::Locked {
                    current_level: level,
                    required_level: 10,
                },
                later_change_stage_disabled: true,
            }
            .validate()
            .unwrap();
        }
        for state in [
            OathSelectionState::Eligible { current_level: 10 },
            OathSelectionState::Selected {
                current_level: 10,
                oath_id: WireText::new(NAILKEEPER_ID).unwrap(),
            },
        ] {
            OathProjection {
                character_id: [1; 16],
                character_version: 1,
                state,
                later_change_stage_disabled: true,
            }
            .validate()
            .unwrap();
        }
        let mut illegal = payload();
        illegal.confirmed = false;
        assert_eq!(
            illegal.validate(),
            Err(OathValidationError::ConfirmationRequired)
        );
    }
}
