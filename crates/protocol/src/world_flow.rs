use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CHARACTER_ID_BYTES, MUTATION_ID_BYTES, ManifestHash, NetworkChannel, PAYLOAD_HASH_BYTES,
    WireText,
};

pub const WORLD_FLOW_ID_MAX_BYTES: usize = 96;
pub const INSTANCE_LINEAGE_ID_BYTES: usize = 16;
pub const TRANSFER_ID_BYTES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CharacterWorldLocation {
    CharacterSelect,
    LanternHalls,
    CoreMicrorealm,
    BellSepulcher,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HallSpawnKind {
    Default,
    CharacterSelectReturn,
}

impl CharacterWorldLocation {
    #[must_use]
    pub const fn is_danger(self) -> bool {
        matches!(self, Self::CoreMicrorealm | Self::BellSepulcher)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterLocationSnapshot {
    pub character_id: [u8; CHARACTER_ID_BYTES],
    pub character_version: u64,
    pub location: CharacterWorldLocation,
    pub instance_lineage_id: Option<[u8; INSTANCE_LINEAGE_ID_BYTES]>,
    pub hall_spawn: Option<HallSpawnKind>,
}

impl CharacterLocationSnapshot {
    pub fn validate(&self) -> Result<(), WorldFlowValidationError> {
        if all_zero(&self.character_id) {
            return Err(WorldFlowValidationError::ZeroCharacterId);
        }
        if self.character_version == 0 {
            return Err(WorldFlowValidationError::ZeroCharacterVersion);
        }
        if self.location.is_danger() != self.instance_lineage_id.is_some()
            || self.instance_lineage_id.is_some_and(|id| all_zero(&id))
        {
            return Err(WorldFlowValidationError::InstanceLocationMismatch);
        }
        if (self.location == CharacterWorldLocation::LanternHalls) != self.hall_spawn.is_some() {
            return Err(WorldFlowValidationError::HallSpawnMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldTransferCommand {
    EnterHallFromCharacterSelect,
    UsePortal {
        portal_id: WireText<WORLD_FLOW_ID_MAX_BYTES>,
    },
}

impl WorldTransferCommand {
    pub fn validate(&self) -> Result<(), WorldFlowValidationError> {
        match self {
            Self::UsePortal { portal_id } if !valid_stable_id(portal_id.as_str()) => {
                Err(WorldFlowValidationError::UnknownTransferSource)
            }
            Self::EnterHallFromCharacterSelect | Self::UsePortal { .. } => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldTransferPayload {
    pub content_manifest_hash: ManifestHash,
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
        if self.payload_hash != self.payload.canonical_hash() {
            return Err(WorldFlowValidationError::PayloadHashMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldFlowRequest {
    Location {
        character_id: [u8; CHARACTER_ID_BYTES],
        content_manifest_hash: ManifestHash,
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
pub struct WorldTransferResult {
    pub mutation_id: [u8; MUTATION_ID_BYTES],
    pub accepted: bool,
    pub code: WorldTransferResultCode,
    pub snapshot: Option<CharacterLocationSnapshot>,
    pub transfer_id: Option<[u8; TRANSFER_ID_BYTES]>,
}

impl WorldTransferResult {
    pub fn validate(&self) -> Result<(), WorldFlowValidationError> {
        if all_zero(&self.mutation_id) {
            return Err(WorldFlowValidationError::ZeroMutationId);
        }
        if self.accepted != (self.code == WorldTransferResultCode::Accepted) {
            return Err(WorldFlowValidationError::TransferResultMismatch);
        }
        if self.accepted && self.snapshot.is_none() {
            return Err(WorldFlowValidationError::TransferResultMismatch);
        }
        if let Some(snapshot) = &self.snapshot {
            snapshot.validate()?;
        }
        if self.transfer_id.is_some_and(|id| all_zero(&id))
            || self.transfer_id.is_some() != self.accepted
        {
            return Err(WorldFlowValidationError::TransferIdentityMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldFlowResult {
    Location(CharacterLocationSnapshot),
    Transfer(WorldTransferResult),
    Error {
        request_sequence: u32,
        code: WorldTransferResultCode,
        snapshot: Option<CharacterLocationSnapshot>,
    },
}

impl WorldFlowResult {
    pub fn validate(&self) -> Result<(), WorldFlowValidationError> {
        match self {
            Self::Location(snapshot) => snapshot.validate(),
            Self::Transfer(result) => result.validate(),
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
    #[error("danger location and instance lineage disagree")]
    InstanceLocationMismatch,
    #[error("Hall location and spawn projection disagree")]
    HallSpawnMismatch,
    #[error("mutation payload hash does not match its canonical payload")]
    PayloadHashMismatch,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> ManifestHash {
        ManifestHash::new("9".repeat(64)).unwrap()
    }

    fn transfer(command: WorldTransferCommand) -> WorldTransferMutation {
        let payload = WorldTransferPayload {
            content_manifest_hash: manifest(),
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
    fn location_lineage_spawn_and_transfer_identity_are_consistent() {
        let danger = CharacterLocationSnapshot {
            character_id: [2; CHARACTER_ID_BYTES],
            character_version: 2,
            location: CharacterWorldLocation::CoreMicrorealm,
            instance_lineage_id: Some([3; INSTANCE_LINEAGE_ID_BYTES]),
            hall_spawn: None,
        };
        let accepted = WorldTransferResult {
            mutation_id: [1; MUTATION_ID_BYTES],
            accepted: true,
            code: WorldTransferResultCode::Accepted,
            snapshot: Some(danger.clone()),
            transfer_id: Some([4; TRANSFER_ID_BYTES]),
        };
        assert_eq!(accepted.validate(), Ok(()));
        let mut invalid = danger;
        invalid.instance_lineage_id = None;
        assert_eq!(
            invalid.validate(),
            Err(WorldFlowValidationError::InstanceLocationMismatch)
        );
    }

    #[test]
    fn stage_disabled_result_is_explicit_and_nonaccepted() {
        let result = WorldTransferResult {
            mutation_id: [1; MUTATION_ID_BYTES],
            accepted: false,
            code: WorldTransferResultCode::StageDisabled,
            snapshot: Some(CharacterLocationSnapshot {
                character_id: [2; CHARACTER_ID_BYTES],
                character_version: 1,
                location: CharacterWorldLocation::CharacterSelect,
                instance_lineage_id: None,
                hall_spawn: None,
            }),
            transfer_id: None,
        };
        assert_eq!(result.validate(), Ok(()));
    }
}
