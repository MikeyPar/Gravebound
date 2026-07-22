//! Append-only protocol 1.21 contract for durable Core Belt consumable use.
//!
//! Authority: the canonical GDD `INP-001`, `LOOT-032`, and `TECH-040`; the Content
//! Production Specification `CONT-FP-007` and `CONT-CATALOG-003`; and roadmap `GB-M03`.
//! Clients name an intent and expected authority only. The server selects the concrete lowest-ID
//! unit and owns simulation acceptance, consumption, and aggregate versions.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ManifestHash, NetworkChannel};

pub const CORE_CONSUMABLE_SCHEMA_VERSION: u16 = 1;
pub const CORE_CONSUMABLE_FEATURE_FLAG: &str = "core_consumable_use_v1";
pub const CORE_CONSUMABLE_ID_BYTES: usize = 16;
pub const CORE_CONSUMABLE_HASH_BYTES: usize = 32;
pub const CORE_CONSUMABLE_BELT_CAPACITY: u8 = 6;
pub const CORE_RED_TONIC_RESTORE_TICKS: u16 = 12;
pub const CORE_RED_TONIC_COOLDOWN_TICKS: u16 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreConsumableSlotV1 {
    BeltOne,
    BeltTwo,
}

impl CoreConsumableSlotV1 {
    #[must_use]
    pub const fn index(self) -> u8 {
        match self {
            Self::BeltOne => 0,
            Self::BeltTwo => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreConsumableUsePayloadV1 {
    pub character_id: [u8; CORE_CONSUMABLE_ID_BYTES],
    pub actor_generation: u64,
    pub instance_lineage_id: [u8; CORE_CONSUMABLE_ID_BYTES],
    pub content_revision: ManifestHash,
    pub expected_inventory_version: u64,
    pub slot: CoreConsumableSlotV1,
}

impl CoreConsumableUsePayloadV1 {
    #[must_use]
    pub fn canonical_hash(&self) -> [u8; CORE_CONSUMABLE_HASH_BYTES] {
        let bytes = postcard::to_stdvec(self).expect("bounded consumable payload serializes");
        blake3::derive_key("gravebound.core-consumable-use-payload.v1", &bytes)
    }

    pub fn validate(&self) -> Result<(), CoreConsumableValidationError> {
        if all_zero(&self.character_id) || all_zero(&self.instance_lineage_id) {
            return Err(CoreConsumableValidationError::ZeroIdentity);
        }
        if self.actor_generation == 0 || self.expected_inventory_version == 0 {
            return Err(CoreConsumableValidationError::ZeroVersion);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreConsumableUseFrameV1 {
    pub schema_version: u16,
    pub mutation_id: [u8; CORE_CONSUMABLE_ID_BYTES],
    pub payload_hash: [u8; CORE_CONSUMABLE_HASH_BYTES],
    pub payload: CoreConsumableUsePayloadV1,
}

impl CoreConsumableUseFrameV1 {
    #[must_use]
    pub const fn channel(&self) -> NetworkChannel {
        NetworkChannel::Mutation
    }

    pub fn validate(&self) -> Result<(), CoreConsumableValidationError> {
        if self.schema_version != CORE_CONSUMABLE_SCHEMA_VERSION {
            return Err(CoreConsumableValidationError::SchemaVersion);
        }
        if all_zero(&self.mutation_id) || all_zero(&self.payload_hash) {
            return Err(CoreConsumableValidationError::ZeroIdentity);
        }
        self.payload.validate()?;
        if self.payload_hash != self.payload.canonical_hash() {
            return Err(CoreConsumableValidationError::PayloadHash);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreConsumableStateV1 {
    pub schema_version: u16,
    pub character_id: [u8; CORE_CONSUMABLE_ID_BYTES],
    pub actor_generation: u64,
    pub instance_lineage_id: [u8; CORE_CONSUMABLE_ID_BYTES],
    pub content_revision: ManifestHash,
    pub inventory_version: u64,
    pub belt_quantities: [u8; 2],
}

impl CoreConsumableStateV1 {
    pub fn validate(&self) -> Result<(), CoreConsumableValidationError> {
        if self.schema_version != CORE_CONSUMABLE_SCHEMA_VERSION {
            return Err(CoreConsumableValidationError::SchemaVersion);
        }
        if all_zero(&self.character_id) || all_zero(&self.instance_lineage_id) {
            return Err(CoreConsumableValidationError::ZeroIdentity);
        }
        if self.actor_generation == 0 || self.inventory_version == 0 {
            return Err(CoreConsumableValidationError::ZeroVersion);
        }
        if self
            .belt_quantities
            .iter()
            .any(|quantity| *quantity > CORE_CONSUMABLE_BELT_CAPACITY)
        {
            return Err(CoreConsumableValidationError::BeltCapacity);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreConsumableResultCodeV1 {
    Accepted,
    EmptySlot,
    FullHealth,
    SharedCooldown,
    InactiveSlot,
    RecallBlocked,
    TerminalPending,
    AuthorityMismatch,
    ContentMismatch,
    InventoryVersionMismatch,
    IdempotencyConflict,
    ServiceUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreConsumableUseResultV1 {
    pub schema_version: u16,
    pub mutation_id: [u8; CORE_CONSUMABLE_ID_BYTES],
    pub code: CoreConsumableResultCodeV1,
    pub consumed_item_uid: Option<[u8; CORE_CONSUMABLE_ID_BYTES]>,
    pub state: Option<CoreConsumableStateV1>,
}

impl CoreConsumableUseResultV1 {
    pub fn validate(&self) -> Result<(), CoreConsumableValidationError> {
        if self.schema_version != CORE_CONSUMABLE_SCHEMA_VERSION {
            return Err(CoreConsumableValidationError::SchemaVersion);
        }
        if all_zero(&self.mutation_id) {
            return Err(CoreConsumableValidationError::ZeroIdentity);
        }
        if let Some(item_uid) = self.consumed_item_uid
            && all_zero(&item_uid)
        {
            return Err(CoreConsumableValidationError::ZeroIdentity);
        }
        if let Some(state) = &self.state {
            state.validate()?;
        }
        let accepted = self.code == CoreConsumableResultCodeV1::Accepted;
        if accepted != self.consumed_item_uid.is_some() || accepted != self.state.is_some() {
            return Err(CoreConsumableValidationError::ResultShape);
        }
        Ok(())
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CoreConsumableValidationError {
    #[error("Core consumable schema version is unsupported")]
    SchemaVersion,
    #[error("Core consumable identity must be nonzero")]
    ZeroIdentity,
    #[error("Core consumable version or generation must be nonzero")]
    ZeroVersion,
    #[error("Core consumable payload hash does not match canonical material")]
    PayloadHash,
    #[error("Core consumable Belt quantity exceeds its canonical cap")]
    BeltCapacity,
    #[error("Core consumable result has an invalid shape")]
    ResultShape,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> CoreConsumableUsePayloadV1 {
        CoreConsumableUsePayloadV1 {
            character_id: [1; 16],
            actor_generation: 7,
            instance_lineage_id: [2; 16],
            content_revision: ManifestHash::new("a".repeat(64)).unwrap(),
            expected_inventory_version: 9,
            slot: CoreConsumableSlotV1::BeltOne,
        }
    }

    #[test]
    fn frame_binds_every_authority_axis_to_its_hash() {
        let payload = payload();
        let mut frame = CoreConsumableUseFrameV1 {
            schema_version: CORE_CONSUMABLE_SCHEMA_VERSION,
            mutation_id: [3; 16],
            payload_hash: payload.canonical_hash(),
            payload,
        };
        assert_eq!(frame.validate(), Ok(()));
        frame.payload.expected_inventory_version += 1;
        assert_eq!(
            frame.validate(),
            Err(CoreConsumableValidationError::PayloadHash)
        );
    }

    #[test]
    fn slots_and_result_codes_are_append_only_pinned() {
        assert_eq!(
            postcard::to_stdvec(&CoreConsumableSlotV1::BeltOne).unwrap(),
            vec![0]
        );
        assert_eq!(
            postcard::to_stdvec(&CoreConsumableSlotV1::BeltTwo).unwrap(),
            vec![1]
        );
        for (ordinal, code) in [
            CoreConsumableResultCodeV1::Accepted,
            CoreConsumableResultCodeV1::EmptySlot,
            CoreConsumableResultCodeV1::FullHealth,
            CoreConsumableResultCodeV1::SharedCooldown,
            CoreConsumableResultCodeV1::InactiveSlot,
            CoreConsumableResultCodeV1::RecallBlocked,
            CoreConsumableResultCodeV1::TerminalPending,
            CoreConsumableResultCodeV1::AuthorityMismatch,
            CoreConsumableResultCodeV1::ContentMismatch,
            CoreConsumableResultCodeV1::InventoryVersionMismatch,
            CoreConsumableResultCodeV1::IdempotencyConflict,
            CoreConsumableResultCodeV1::ServiceUnavailable,
        ]
        .into_iter()
        .enumerate()
        {
            let ordinal = u8::try_from(ordinal).expect("consumable result ordinal fits u8");
            assert_eq!(postcard::to_stdvec(&code).unwrap(), vec![ordinal]);
        }
    }
}
