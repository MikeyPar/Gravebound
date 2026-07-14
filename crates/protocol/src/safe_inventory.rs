use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{CHARACTER_ID_BYTES, MUTATION_ID_BYTES, NetworkChannel, PAYLOAD_HASH_BYTES};

pub const SAFE_INVENTORY_ITEM_UID_BYTES: usize = 16;
pub const SAFE_INVENTORY_RESULT_HASH_BYTES: usize = 32;
pub const SAFE_INVENTORY_PLACEMENT_CAPACITY: usize = 6;

const CHARACTER_SAFE_CAPACITY: u16 = 8;
const VAULT_CAPACITY: u16 = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeInventoryTransferKindV1 {
    CharacterSafeToVault,
    VaultToCharacterSafe,
    CharacterSafeToRunBackpack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafeInventoryTransferPayloadV1 {
    pub kind: SafeInventoryTransferKindV1,
    pub source_slot_index: u16,
    pub expected_account_version: u64,
    pub expected_inventory_version: u64,
}

impl SafeInventoryTransferPayloadV1 {
    #[must_use]
    pub fn canonical_hash(&self) -> [u8; PAYLOAD_HASH_BYTES] {
        let bytes = postcard::to_stdvec(self).expect("bounded safe-inventory payload serializes");
        *blake3::hash(&bytes).as_bytes()
    }

    fn validate(self) -> Result<(), SafeInventoryValidationError> {
        if self.expected_account_version == 0 || self.expected_inventory_version == 0 {
            return Err(SafeInventoryValidationError::ZeroVersion);
        }
        let source_capacity = match self.kind {
            SafeInventoryTransferKindV1::VaultToCharacterSafe => VAULT_CAPACITY,
            SafeInventoryTransferKindV1::CharacterSafeToVault
            | SafeInventoryTransferKindV1::CharacterSafeToRunBackpack => CHARACTER_SAFE_CAPACITY,
        };
        if self.source_slot_index >= source_capacity {
            return Err(SafeInventoryValidationError::SourceIndex);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafeInventoryTransferFrameV1 {
    pub mutation_id: [u8; MUTATION_ID_BYTES],
    pub character_id: [u8; CHARACTER_ID_BYTES],
    pub issued_at_unix_millis: u64,
    pub payload_hash: [u8; PAYLOAD_HASH_BYTES],
    pub payload: SafeInventoryTransferPayloadV1,
}

impl SafeInventoryTransferFrameV1 {
    #[must_use]
    pub const fn channel(&self) -> NetworkChannel {
        NetworkChannel::Mutation
    }

    pub fn validate(&self) -> Result<(), SafeInventoryValidationError> {
        if all_zero(&self.mutation_id)
            || all_zero(&self.character_id)
            || all_zero(&self.payload_hash)
            || self.issued_at_unix_millis == 0
        {
            return Err(SafeInventoryValidationError::ZeroIdentity);
        }
        self.payload.validate()?;
        if self.payload_hash != self.payload.canonical_hash() {
            return Err(SafeInventoryValidationError::PayloadHashMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeInventoryDestinationV1 {
    CharacterSafe { slot_index: u8 },
    Vault { slot_index: u16 },
    RunBackpack { slot_index: u8 },
}

impl SafeInventoryDestinationV1 {
    const fn validate(self) -> Result<(), SafeInventoryValidationError> {
        match self {
            Self::CharacterSafe { slot_index } if slot_index < 8 => Ok(()),
            Self::Vault { slot_index } if slot_index < VAULT_CAPACITY => Ok(()),
            Self::RunBackpack { slot_index } if slot_index < 8 => Ok(()),
            _ => Err(SafeInventoryValidationError::DestinationIndex),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafeInventoryPlacementV1 {
    pub item_uid: [u8; SAFE_INVENTORY_ITEM_UID_BYTES],
    pub destination: SafeInventoryDestinationV1,
    pub item_version: u64,
}

impl SafeInventoryPlacementV1 {
    fn validate(self) -> Result<(), SafeInventoryValidationError> {
        if all_zero(&self.item_uid) {
            return Err(SafeInventoryValidationError::ZeroIdentity);
        }
        if self.item_version == 0 {
            return Err(SafeInventoryValidationError::ZeroVersion);
        }
        self.destination.validate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeInventoryResultCodeV1 {
    Accepted,
    InvalidCommand,
    VersionMismatch,
    HallBindingRequired,
    UnresolvedMutation,
    SourceUnavailable,
    StorageFull,
    CharacterSafeFull,
    RunBackpackFull,
    IdempotencyConflict,
    ServiceUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafeInventoryTransferResultV1 {
    pub mutation_id: [u8; MUTATION_ID_BYTES],
    pub character_id: [u8; CHARACTER_ID_BYTES],
    pub code: SafeInventoryResultCodeV1,
    pub replayed: bool,
    pub result_hash: [u8; SAFE_INVENTORY_RESULT_HASH_BYTES],
    pub account_version: u64,
    pub inventory_version: u64,
    pub placements: Vec<SafeInventoryPlacementV1>,
}

impl SafeInventoryTransferResultV1 {
    pub fn validate(&self) -> Result<(), SafeInventoryValidationError> {
        if all_zero(&self.mutation_id) || all_zero(&self.character_id) {
            return Err(SafeInventoryValidationError::ZeroIdentity);
        }
        if self.code == SafeInventoryResultCodeV1::Accepted {
            if all_zero(&self.result_hash) {
                return Err(SafeInventoryValidationError::ZeroIdentity);
            }
            if self.account_version == 0 || self.inventory_version == 0 {
                return Err(SafeInventoryValidationError::ZeroVersion);
            }
            if self.placements.is_empty()
                || self.placements.len() > SAFE_INVENTORY_PLACEMENT_CAPACITY
            {
                return Err(SafeInventoryValidationError::PlacementCount);
            }
            for placement in &self.placements {
                placement.validate()?;
            }
        } else if self.replayed
            || !all_zero(&self.result_hash)
            || self.account_version != 0
            || self.inventory_version != 0
            || !self.placements.is_empty()
        {
            return Err(SafeInventoryValidationError::RejectedResultShape);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SafeInventoryValidationError {
    #[error("safe-inventory identity or correlation value cannot be zero")]
    ZeroIdentity,
    #[error("safe-inventory version must be positive")]
    ZeroVersion,
    #[error("safe-inventory source index exceeds its exact capacity")]
    SourceIndex,
    #[error("safe-inventory destination index exceeds its exact capacity")]
    DestinationIndex,
    #[error("safe-inventory payload hash does not match its canonical payload")]
    PayloadHashMismatch,
    #[error("safe-inventory placement count must be in 1..=6")]
    PlacementCount,
    #[error("rejected safe-inventory results cannot carry authoritative mutation state")]
    RejectedResultShape,
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

    fn frame(
        kind: SafeInventoryTransferKindV1,
        source_slot_index: u16,
    ) -> SafeInventoryTransferFrameV1 {
        let payload = SafeInventoryTransferPayloadV1 {
            kind,
            source_slot_index,
            expected_account_version: 4,
            expected_inventory_version: 7,
        };
        SafeInventoryTransferFrameV1 {
            mutation_id: [1; 16],
            character_id: [2; 16],
            issued_at_unix_millis: 99,
            payload_hash: payload.canonical_hash(),
            payload,
        }
    }

    #[test]
    fn request_hash_binds_every_caller_owned_field() {
        let request = frame(SafeInventoryTransferKindV1::CharacterSafeToVault, 7);
        request.validate().unwrap();
        let mut altered = request;
        altered.payload.expected_inventory_version += 1;
        assert_eq!(
            altered.validate(),
            Err(SafeInventoryValidationError::PayloadHashMismatch)
        );
    }

    #[test]
    fn source_indices_enforce_exact_storage_capacities() {
        frame(SafeInventoryTransferKindV1::VaultToCharacterSafe, 159)
            .validate()
            .unwrap();
        assert_eq!(
            frame(SafeInventoryTransferKindV1::VaultToCharacterSafe, 160).validate(),
            Err(SafeInventoryValidationError::SourceIndex)
        );
        assert_eq!(
            frame(SafeInventoryTransferKindV1::CharacterSafeToVault, 8).validate(),
            Err(SafeInventoryValidationError::SourceIndex)
        );
    }

    #[test]
    fn authoritative_result_is_bounded_and_validates_destinations() {
        let placement = SafeInventoryPlacementV1 {
            item_uid: [3; 16],
            destination: SafeInventoryDestinationV1::Vault { slot_index: 159 },
            item_version: 2,
        };
        let mut result = SafeInventoryTransferResultV1 {
            mutation_id: [1; 16],
            character_id: [2; 16],
            code: SafeInventoryResultCodeV1::Accepted,
            replayed: false,
            result_hash: [4; 32],
            account_version: 5,
            inventory_version: 8,
            placements: vec![placement; SAFE_INVENTORY_PLACEMENT_CAPACITY],
        };
        result.validate().unwrap();
        result.placements.push(placement);
        assert_eq!(
            result.validate(),
            Err(SafeInventoryValidationError::PlacementCount)
        );
        result.placements.truncate(1);
        result.placements[0].destination =
            SafeInventoryDestinationV1::CharacterSafe { slot_index: 8 };
        assert_eq!(
            result.validate(),
            Err(SafeInventoryValidationError::DestinationIndex)
        );

        let rejected = SafeInventoryTransferResultV1 {
            mutation_id: [1; 16],
            character_id: [2; 16],
            code: SafeInventoryResultCodeV1::StorageFull,
            replayed: false,
            result_hash: [0; 32],
            account_version: 0,
            inventory_version: 0,
            placements: Vec::new(),
        };
        rejected.validate().unwrap();
        let mut invalid = rejected;
        invalid.replayed = true;
        assert_eq!(
            invalid.validate(),
            Err(SafeInventoryValidationError::RejectedResultShape)
        );
    }
}
