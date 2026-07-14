use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CHARACTER_ID_BYTES, MUTATION_ID_BYTES, ManifestHash, NetworkChannel, PAYLOAD_HASH_BYTES,
    WireText,
};

pub const WORLD_FLOW_ID_MAX_BYTES: usize = 96;
pub const INSTANCE_LINEAGE_ID_BYTES: usize = 16;
pub const TRANSFER_ID_BYTES: usize = 16;

/// Exact independently validated `GB-M03-03A` world-flow content identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldFlowContentRevisionV1 {
    pub records_blake3: ManifestHash,
    pub assets_blake3: ManifestHash,
    pub localization_blake3: ManifestHash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeArrival {
    HallDefault,
    SpawnAnchor {
        spawn_id: WireText<WORLD_FLOW_ID_MAX_BYTES>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CharacterLocation {
    CharacterSelect {
        next_hall_arrival: SafeArrival,
    },
    Safe {
        location_id: WireText<WORLD_FLOW_ID_MAX_BYTES>,
        arrival: SafeArrival,
    },
    Danger {
        location_id: WireText<WORLD_FLOW_ID_MAX_BYTES>,
        instance_lineage_id: [u8; INSTANCE_LINEAGE_ID_BYTES],
        entry_restore_point_id: [u8; TRANSFER_ID_BYTES],
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterLocationSnapshot {
    pub character_id: [u8; CHARACTER_ID_BYTES],
    pub character_version: u64,
    pub location: CharacterLocation,
}

impl CharacterLocationSnapshot {
    pub fn validate(&self) -> Result<(), WorldFlowValidationError> {
        if all_zero(&self.character_id) {
            return Err(WorldFlowValidationError::ZeroCharacterId);
        }
        if self.character_version == 0 {
            return Err(WorldFlowValidationError::ZeroCharacterVersion);
        }
        match &self.location {
            CharacterLocation::CharacterSelect { next_hall_arrival } => {
                validate_arrival(next_hall_arrival)?;
            }
            CharacterLocation::Safe {
                location_id,
                arrival,
            } => {
                if !valid_stable_id(location_id.as_str()) {
                    return Err(WorldFlowValidationError::InvalidLocationId);
                }
                validate_arrival(arrival)?;
            }
            CharacterLocation::Danger {
                location_id,
                instance_lineage_id,
                entry_restore_point_id,
            } => {
                if !valid_stable_id(location_id.as_str())
                    || all_zero(instance_lineage_id)
                    || all_zero(entry_restore_point_id)
                {
                    return Err(WorldFlowValidationError::InstanceLocationMismatch);
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldTransferCommand {
    EnterHallFromCharacterSelect,
    ReturnToCharacterSelect,
    UsePortal {
        portal_id: WireText<WORLD_FLOW_ID_MAX_BYTES>,
    },
    UseCommittedExtraction {
        portal_id: WireText<WORLD_FLOW_ID_MAX_BYTES>,
        extraction_request_id: [u8; TRANSFER_ID_BYTES],
        extraction_receipt_id: [u8; TRANSFER_ID_BYTES],
    },
}

impl WorldTransferCommand {
    pub fn validate(&self) -> Result<(), WorldFlowValidationError> {
        match self {
            Self::UsePortal { portal_id } if !valid_stable_id(portal_id.as_str()) => {
                Err(WorldFlowValidationError::UnknownTransferSource)
            }
            Self::UseCommittedExtraction {
                portal_id,
                extraction_request_id,
                extraction_receipt_id,
            } if !valid_stable_id(portal_id.as_str())
                || all_zero(extraction_request_id)
                || all_zero(extraction_receipt_id)
                || extraction_request_id == extraction_receipt_id =>
            {
                Err(WorldFlowValidationError::InvalidExtractionReceipt)
            }
            Self::EnterHallFromCharacterSelect
            | Self::ReturnToCharacterSelect
            | Self::UsePortal { .. }
            | Self::UseCommittedExtraction { .. } => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldTransferPayload {
    pub content_revision: WorldFlowContentRevisionV1,
    pub command: WorldTransferCommand,
}

impl WorldTransferPayload {
    #[must_use]
    pub fn canonical_hash(&self) -> [u8; PAYLOAD_HASH_BYTES] {
        let bytes = postcard::to_stdvec(self).expect("bounded world-transfer payload serializes");
        *blake3::hash(&bytes).as_bytes()
    }

    pub fn validate(&self) -> Result<(), WorldFlowValidationError> {
        self.command.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldTransferMutation {
    pub mutation_id: [u8; MUTATION_ID_BYTES],
    pub character_id: [u8; CHARACTER_ID_BYTES],
    pub expected_character_version: u64,
    pub issued_at_unix_millis: u64,
    pub payload_hash: [u8; PAYLOAD_HASH_BYTES],
    pub payload: WorldTransferPayload,
}

impl WorldTransferMutation {
    pub fn validate(&self) -> Result<(), WorldFlowValidationError> {
        if all_zero(&self.mutation_id) {
            return Err(WorldFlowValidationError::ZeroMutationId);
        }
        if all_zero(&self.character_id) {
            return Err(WorldFlowValidationError::ZeroCharacterId);
        }
        if self.expected_character_version == 0 {
            return Err(WorldFlowValidationError::ZeroCharacterVersion);
        }
        if self.issued_at_unix_millis == 0 {
            return Err(WorldFlowValidationError::ZeroIssuedAt);
        }
        if all_zero(&self.payload_hash) {
            return Err(WorldFlowValidationError::ZeroPayloadHash);
        }
        self.payload.validate()?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldFlowRequest {
    Location {
        character_id: [u8; CHARACTER_ID_BYTES],
        content_revision: WorldFlowContentRevisionV1,
    },
    Transfer(WorldTransferMutation),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldFlowFrame {
    pub sequence: u32,
    pub request: WorldFlowRequest,
}

impl WorldFlowFrame {
    #[must_use]
    pub const fn channel(&self) -> NetworkChannel {
        NetworkChannel::Control
    }

    pub fn validate(&self) -> Result<(), WorldFlowValidationError> {
        if self.sequence == 0 {
            return Err(WorldFlowValidationError::ZeroSequence);
        }
        match &self.request {
            WorldFlowRequest::Location { character_id, .. } if all_zero(character_id) => {
                Err(WorldFlowValidationError::ZeroCharacterId)
            }
            WorldFlowRequest::Transfer(mutation) => mutation.validate(),
            WorldFlowRequest::Location { .. } => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldTransferResultCode {
    Accepted,
    StageDisabled,
    StateVersionMismatch,
    CharacterNotFound,
    NoSelectedCharacter,
    CharacterNotOwned,
    CharacterDead,
    InvalidSource,
    OutOfRange,
    ContentDisabled,
    DestinationDisabled,
    TransferInProgress,
    ContentMismatch,
    IdempotencyConflict,
    PayloadHashMismatch,
    IssuedAtInvalid,
    IncompleteRestorePoint,
    StorageResolutionRequired,
    InstanceUnavailable,
    RateLimited,
    ServiceUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldFlowResult {
    Location {
        request_sequence: u32,
        snapshot: CharacterLocationSnapshot,
    },
    Transfer {
        request_sequence: u32,
        mutation_id: [u8; MUTATION_ID_BYTES],
        accepted: bool,
        code: WorldTransferResultCode,
        snapshot: Option<CharacterLocationSnapshot>,
        transfer_id: Option<[u8; TRANSFER_ID_BYTES]>,
    },
    Error {
        request_sequence: u32,
        code: WorldTransferResultCode,
        snapshot: Option<CharacterLocationSnapshot>,
    },
}

impl WorldFlowResult {
    pub fn validate(&self) -> Result<(), WorldFlowValidationError> {
        match self {
            Self::Location {
                request_sequence,
                snapshot,
            } => {
                if *request_sequence == 0 {
                    return Err(WorldFlowValidationError::ZeroSequence);
                }
                snapshot.validate()
            }
            Self::Transfer {
                request_sequence,
                mutation_id,
                accepted,
                code,
                snapshot,
                transfer_id,
            } => {
                if *request_sequence == 0 {
                    return Err(WorldFlowValidationError::ZeroSequence);
                }
                if all_zero(mutation_id) {
                    return Err(WorldFlowValidationError::ZeroMutationId);
                }
                if *accepted != (*code == WorldTransferResultCode::Accepted)
                    || (*accepted && snapshot.is_none())
                {
                    return Err(WorldFlowValidationError::TransferResultMismatch);
                }
                if let Some(snapshot) = snapshot {
                    snapshot.validate()?;
                }
                if transfer_id.is_some_and(|id| all_zero(&id)) || transfer_id.is_some() != *accepted
                {
                    return Err(WorldFlowValidationError::TransferIdentityMismatch);
                }
                Ok(())
            }
            Self::Error {
                request_sequence,
                code,
                snapshot,
            } => {
                if *request_sequence == 0 || *code == WorldTransferResultCode::Accepted {
                    return Err(WorldFlowValidationError::TransferResultMismatch);
                }
                snapshot
                    .as_ref()
                    .map_or(Ok(()), CharacterLocationSnapshot::validate)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum WorldFlowValidationError {
    #[error("message sequence must be nonzero")]
    ZeroSequence,
    #[error("character ID must be nonzero")]
    ZeroCharacterId,
    #[error("character version must be nonzero")]
    ZeroCharacterVersion,
    #[error("mutation ID must be nonzero")]
    ZeroMutationId,
    #[error("mutation issue time must be nonzero")]
    ZeroIssuedAt,
    #[error("mutation payload hash must be nonzero")]
    ZeroPayloadHash,
    #[error("transfer source is not part of the approved Core route")]
    UnknownTransferSource,
    #[error("committed extraction requires distinct nonzero request and receipt identities")]
    InvalidExtractionReceipt,
    #[error("danger location, instance lineage, or restore-point identity is invalid")]
    InstanceLocationMismatch,
    #[error("safe or danger location contains an invalid stable ID")]
    InvalidLocationId,
    #[error("transfer result acceptance, code, or snapshot disagree")]
    TransferResultMismatch,
    #[error("accepted state and nonsecret transfer identity disagree")]
    TransferIdentityMismatch,
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

fn validate_arrival(arrival: &SafeArrival) -> Result<(), WorldFlowValidationError> {
    if let SafeArrival::SpawnAnchor { spawn_id } = arrival
        && !valid_stable_id(spawn_id.as_str())
    {
        return Err(WorldFlowValidationError::InvalidLocationId);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn revision() -> WorldFlowContentRevisionV1 {
        WorldFlowContentRevisionV1 {
            records_blake3: ManifestHash::new("7".repeat(64)).unwrap(),
            assets_blake3: ManifestHash::new("8".repeat(64)).unwrap(),
            localization_blake3: ManifestHash::new("9".repeat(64)).unwrap(),
        }
    }

    fn transfer(command: WorldTransferCommand) -> WorldTransferMutation {
        let payload = WorldTransferPayload {
            content_revision: revision(),
            command,
        };
        WorldTransferMutation {
            mutation_id: [1; MUTATION_ID_BYTES],
            character_id: [2; CHARACTER_ID_BYTES],
            expected_character_version: 1,
            issued_at_unix_millis: 1,
            payload_hash: payload.canonical_hash(),
            payload,
        }
    }

    #[test]
    fn requests_are_bounded_control_messages_with_canonical_payload_hashes() {
        let mutation = transfer(WorldTransferCommand::UsePortal {
            portal_id: WireText::new("station.realm_gate").unwrap(),
        });
        assert_eq!(mutation.payload_hash, mutation.payload.canonical_hash());
        let frame = WorldFlowFrame {
            sequence: 1,
            request: WorldFlowRequest::Transfer(mutation),
        };
        assert_eq!(frame.channel(), NetworkChannel::Control);
        assert_eq!(frame.validate(), Ok(()));
    }

    #[test]
    fn unknown_sources_and_zero_identity_fail_closed() {
        let unknown = transfer(WorldTransferCommand::UsePortal {
            portal_id: WireText::new("portal..unknown").unwrap(),
        });
        assert_eq!(
            unknown.validate(),
            Err(WorldFlowValidationError::UnknownTransferSource)
        );
        let mut zero = transfer(WorldTransferCommand::EnterHallFromCharacterSelect);
        zero.character_id = [0; CHARACTER_ID_BYTES];
        assert_eq!(
            zero.validate(),
            Err(WorldFlowValidationError::ZeroCharacterId)
        );
    }

    #[test]
    fn committed_extraction_requires_two_distinct_stable_identities() {
        let command = WorldTransferCommand::UseCommittedExtraction {
            portal_id: WireText::new("portal.exit.dungeon.bell_sepulcher").unwrap(),
            extraction_request_id: [3; TRANSFER_ID_BYTES],
            extraction_receipt_id: [4; TRANSFER_ID_BYTES],
        };
        assert_eq!(transfer(command).validate(), Ok(()));
        let invalid = transfer(WorldTransferCommand::UseCommittedExtraction {
            portal_id: WireText::new("portal.exit.dungeon.bell_sepulcher").unwrap(),
            extraction_request_id: [3; TRANSFER_ID_BYTES],
            extraction_receipt_id: [3; TRANSFER_ID_BYTES],
        });
        assert_eq!(
            invalid.validate(),
            Err(WorldFlowValidationError::InvalidExtractionReceipt)
        );
    }

    #[test]
    fn location_lineage_spawn_and_transfer_identity_are_consistent() {
        let danger = CharacterLocationSnapshot {
            character_id: [2; CHARACTER_ID_BYTES],
            character_version: 2,
            location: CharacterLocation::Danger {
                location_id: WireText::new("world.core_microrealm_01").unwrap(),
                instance_lineage_id: [3; INSTANCE_LINEAGE_ID_BYTES],
                entry_restore_point_id: [5; TRANSFER_ID_BYTES],
            },
        };
        let accepted = WorldFlowResult::Transfer {
            request_sequence: 1,
            mutation_id: [1; MUTATION_ID_BYTES],
            accepted: true,
            code: WorldTransferResultCode::Accepted,
            snapshot: Some(danger.clone()),
            transfer_id: Some([4; TRANSFER_ID_BYTES]),
        };
        assert_eq!(accepted.validate(), Ok(()));
        let mut invalid = danger;
        invalid.location = CharacterLocation::Danger {
            location_id: WireText::new("world.core_microrealm_01").unwrap(),
            instance_lineage_id: [0; INSTANCE_LINEAGE_ID_BYTES],
            entry_restore_point_id: [5; TRANSFER_ID_BYTES],
        };
        assert_eq!(
            invalid.validate(),
            Err(WorldFlowValidationError::InstanceLocationMismatch)
        );
    }

    #[test]
    fn stage_disabled_result_is_explicit_and_nonaccepted() {
        let result = WorldFlowResult::Transfer {
            request_sequence: 1,
            mutation_id: [1; MUTATION_ID_BYTES],
            accepted: false,
            code: WorldTransferResultCode::StageDisabled,
            snapshot: Some(CharacterLocationSnapshot {
                character_id: [2; CHARACTER_ID_BYTES],
                character_version: 1,
                location: CharacterLocation::CharacterSelect {
                    next_hall_arrival: SafeArrival::HallDefault,
                },
            }),
            transfer_id: None,
        };
        assert_eq!(result.validate(), Ok(()));
    }
}
