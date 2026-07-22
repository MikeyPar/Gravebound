use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{CHARACTER_ID_BYTES, NetworkChannel, WireText};

pub const SAFE_STORAGE_FEATURE_FLAG: &str = "core_safe_storage_view_v1";
pub const SAFE_STORAGE_SCHEMA_VERSION: u16 = 1;
pub const SAFE_STORAGE_MAX_STACKS_PER_PAGE: usize = 32;
pub const SAFE_STORAGE_MAX_ITEMS_PER_STACK: usize = 6;
pub const SAFE_STORAGE_MAX_CHARACTER_SAFE_STACKS: usize = 8;
pub const SAFE_STORAGE_CONTENT_ID_MAX_BYTES: usize = 96;
pub const SAFE_STORAGE_CONTENT_REVISION_MAX_BYTES: usize = 96;
pub const SAFE_STORAGE_ITEM_UID_BYTES: usize = 16;

const VAULT_CAPACITY: u16 = 160;
const OVERFLOW_CAPACITY: u16 = 20;
const CHARACTER_SAFE_CAPACITY: u16 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeStorageSurfaceV1 {
    Vault,
    Overflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeStorageLocationV1 {
    CharacterSafe,
    Vault,
    Overflow,
}

impl SafeStorageLocationV1 {
    #[must_use]
    pub const fn capacity(self) -> u16 {
        match self {
            Self::CharacterSafe => CHARACTER_SAFE_CAPACITY,
            Self::Vault => VAULT_CAPACITY,
            Self::Overflow => OVERFLOW_CAPACITY,
        }
    }
}

impl SafeStorageSurfaceV1 {
    #[must_use]
    pub const fn capacity(self) -> u16 {
        match self {
            Self::Vault => VAULT_CAPACITY,
            Self::Overflow => OVERFLOW_CAPACITY,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafeStorageQueryFrameV1 {
    pub schema_version: u16,
    pub sequence: u32,
    pub character_id: [u8; CHARACTER_ID_BYTES],
    pub surface: SafeStorageSurfaceV1,
    /// The last logical slot returned by the preceding page. `None` starts a new projection.
    pub after_slot: Option<u16>,
    /// Required on continuation pages and forbidden on a first-page request.
    pub expected_account_version: Option<u64>,
    /// Required on continuation pages and forbidden on a first-page request.
    pub expected_inventory_version: Option<u64>,
}

impl SafeStorageQueryFrameV1 {
    #[must_use]
    pub const fn channel(&self) -> NetworkChannel {
        NetworkChannel::Control
    }

    pub const fn validate(&self) -> Result<(), SafeStorageValidationError> {
        if self.schema_version != SAFE_STORAGE_SCHEMA_VERSION {
            return Err(SafeStorageValidationError::SchemaVersion);
        }
        if self.sequence == 0 {
            return Err(SafeStorageValidationError::ZeroSequence);
        }
        if all_zero(&self.character_id) {
            return Err(SafeStorageValidationError::ZeroIdentity);
        }
        match (
            self.after_slot,
            self.expected_account_version,
            self.expected_inventory_version,
        ) {
            (None, None, None) => Ok(()),
            (Some(slot), Some(account), Some(inventory))
                if slot < self.surface.capacity() && account > 0 && inventory > 0 =>
            {
                Ok(())
            }
            _ => Err(SafeStorageValidationError::CursorShape),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeStorageItemKindV1 {
    Equipment,
    Consumable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeStorageRarityV1 {
    Worn,
    Forged,
    Oathed,
    Relic,
    Sainted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeStorageSecurityV1 {
    Safe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeStorageProvenanceV1 {
    Starter,
    Drop,
    Craft,
    Gift,
    Grant,
    Migration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafeStorageItemV1 {
    pub item_uid: [u8; SAFE_STORAGE_ITEM_UID_BYTES],
    pub item_version: u64,
    pub item_level: Option<u8>,
    pub rarity: Option<SafeStorageRarityV1>,
    pub security: SafeStorageSecurityV1,
    pub provenance: SafeStorageProvenanceV1,
    pub salvage_band: u8,
    pub salvage_value: u32,
    /// Present only for Overflow and expressed as an absolute UTC Unix-millisecond deadline.
    pub overflow_expires_at_unix_millis: Option<u64>,
}

impl SafeStorageItemV1 {
    const fn validate(
        self,
        location: SafeStorageLocationV1,
        kind: SafeStorageItemKindV1,
    ) -> Result<(), SafeStorageValidationError> {
        if all_zero(&self.item_uid) {
            return Err(SafeStorageValidationError::ZeroIdentity);
        }
        if self.item_version == 0 {
            return Err(SafeStorageValidationError::ZeroVersion);
        }
        if self.salvage_band > 5 {
            return Err(SafeStorageValidationError::StackShape);
        }
        match (location, self.overflow_expires_at_unix_millis) {
            (SafeStorageLocationV1::CharacterSafe | SafeStorageLocationV1::Vault, None)
            | (SafeStorageLocationV1::Overflow, Some(1..)) => {}
            _ => return Err(SafeStorageValidationError::StackShape),
        }
        match kind {
            SafeStorageItemKindV1::Equipment
                if self.item_level.is_some() && self.rarity.is_some() => {}
            SafeStorageItemKindV1::Consumable
                if self.item_level.is_none()
                    && self.rarity.is_none()
                    && self.salvage_band == 0
                    && self.salvage_value == 0 => {}
            _ => return Err(SafeStorageValidationError::StackShape),
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafeStorageStackV1 {
    pub location: SafeStorageLocationV1,
    pub slot_index: u16,
    pub template_id: WireText<SAFE_STORAGE_CONTENT_ID_MAX_BYTES>,
    pub item_kind: SafeStorageItemKindV1,
    /// Display summary copied from the first canonical unit; exact per-unit fields remain below.
    pub item_level: Option<u8>,
    pub rarity: Option<SafeStorageRarityV1>,
    pub security: SafeStorageSecurityV1,
    pub provenance: SafeStorageProvenanceV1,
    pub salvage_band: u8,
    pub salvage_value: u32,
    /// Earliest unit deadline for an Overflow stack; exact per-unit deadlines remain below.
    pub overflow_expires_at_unix_millis: Option<u64>,
    pub items: Vec<SafeStorageItemV1>,
}

impl SafeStorageStackV1 {
    fn validate(
        &self,
        expected_location: SafeStorageLocationV1,
    ) -> Result<(), SafeStorageValidationError> {
        if self.location != expected_location
            || self.slot_index >= self.location.capacity()
            || self.items.is_empty()
            || self.items.len() > SAFE_STORAGE_MAX_ITEMS_PER_STACK
        {
            return Err(SafeStorageValidationError::StackShape);
        }
        match self.item_kind {
            SafeStorageItemKindV1::Equipment if self.items.len() == 1 => {}
            SafeStorageItemKindV1::Consumable => {}
            SafeStorageItemKindV1::Equipment => {
                return Err(SafeStorageValidationError::StackShape);
            }
        }
        let first = self.items[0];
        if self.item_level != first.item_level
            || self.rarity != first.rarity
            || self.security != first.security
            || self.provenance != first.provenance
            || self.salvage_band != first.salvage_band
            || self.salvage_value != first.salvage_value
            || self.overflow_expires_at_unix_millis
                != self
                    .items
                    .iter()
                    .filter_map(|item| item.overflow_expires_at_unix_millis)
                    .min()
        {
            return Err(SafeStorageValidationError::StackShape);
        }
        for item in &self.items {
            item.validate(self.location, self.item_kind)?;
        }
        if !self
            .items
            .windows(2)
            .all(|pair| pair[0].item_uid < pair[1].item_uid)
        {
            return Err(SafeStorageValidationError::NoncanonicalOrdering);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeStorageQueryCodeV1 {
    InvalidRequest,
    FeatureDisabled,
    ForeignAuthority,
    HallBindingRequired,
    WrongPanel,
    StaleVersions,
    ServiceUnavailable,
    CorruptStoredAuthority,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeStorageQueryResultV1 {
    Stored {
        schema_version: u16,
        sequence: u32,
        character_id: [u8; CHARACTER_ID_BYTES],
        surface: SafeStorageSurfaceV1,
        account_version: u64,
        inventory_version: u64,
        content_revision: WireText<SAFE_STORAGE_CONTENT_REVISION_MAX_BYTES>,
        /// Complete eight-slot `CharacterSafe` companion projection under the same version root.
        character_safe: Vec<SafeStorageStackV1>,
        stacks: Vec<SafeStorageStackV1>,
        /// When present, request the next page using this value as `after_slot` and echo both
        /// aggregate versions. A stale aggregate rejects the continuation and restarts paging.
        next_after_slot: Option<u16>,
    },
    Rejected {
        schema_version: u16,
        sequence: u32,
        code: SafeStorageQueryCodeV1,
    },
}

impl SafeStorageQueryResultV1 {
    pub fn validate(&self) -> Result<(), SafeStorageValidationError> {
        let (schema_version, sequence) = match self {
            Self::Stored {
                schema_version,
                sequence,
                character_id,
                surface,
                account_version,
                inventory_version,
                character_safe,
                stacks,
                next_after_slot,
                ..
            } => {
                if all_zero(character_id) || *account_version == 0 || *inventory_version == 0 {
                    return Err(SafeStorageValidationError::ZeroIdentity);
                }
                if character_safe.len() > SAFE_STORAGE_MAX_CHARACTER_SAFE_STACKS
                    || stacks.len() > SAFE_STORAGE_MAX_STACKS_PER_PAGE
                {
                    return Err(SafeStorageValidationError::PageSize);
                }
                for stack in character_safe {
                    stack.validate(SafeStorageLocationV1::CharacterSafe)?;
                }
                if !character_safe
                    .windows(2)
                    .all(|pair| pair[0].slot_index < pair[1].slot_index)
                {
                    return Err(SafeStorageValidationError::NoncanonicalOrdering);
                }
                let location = match surface {
                    SafeStorageSurfaceV1::Vault => SafeStorageLocationV1::Vault,
                    SafeStorageSurfaceV1::Overflow => SafeStorageLocationV1::Overflow,
                };
                for stack in stacks {
                    stack.validate(location)?;
                }
                if !stacks
                    .windows(2)
                    .all(|pair| pair[0].slot_index < pair[1].slot_index)
                {
                    return Err(SafeStorageValidationError::NoncanonicalOrdering);
                }
                match next_after_slot {
                    None => {}
                    Some(cursor)
                        if stacks
                            .last()
                            .is_some_and(|stack| stack.slot_index == *cursor) => {}
                    Some(_) => return Err(SafeStorageValidationError::CursorShape),
                }
                (*schema_version, *sequence)
            }
            Self::Rejected {
                schema_version,
                sequence,
                ..
            } => (*schema_version, *sequence),
        };
        if schema_version != SAFE_STORAGE_SCHEMA_VERSION {
            return Err(SafeStorageValidationError::SchemaVersion);
        }
        if sequence == 0 {
            return Err(SafeStorageValidationError::ZeroSequence);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SafeStorageValidationError {
    #[error("safe-storage schema version is unsupported")]
    SchemaVersion,
    #[error("safe-storage sequence must be positive")]
    ZeroSequence,
    #[error("safe-storage identity cannot be zero")]
    ZeroIdentity,
    #[error("safe-storage aggregate or item version must be positive")]
    ZeroVersion,
    #[error("safe-storage continuation cursor and aggregate versions have an invalid shape")]
    CursorShape,
    #[error("safe-storage stack has invalid canonical fields")]
    StackShape,
    #[error("safe-storage page exceeds its protocol bound")]
    PageSize,
    #[error("safe-storage slots or item identities are not in canonical order")]
    NoncanonicalOrdering,
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

    #[test]
    fn continuation_requires_exact_version_pair() {
        let first = SafeStorageQueryFrameV1 {
            schema_version: 1,
            sequence: 1,
            character_id: [1; 16],
            surface: SafeStorageSurfaceV1::Vault,
            after_slot: None,
            expected_account_version: None,
            expected_inventory_version: None,
        };
        first.validate().unwrap();
        let mut continuation = first;
        continuation.after_slot = Some(31);
        assert_eq!(
            continuation.validate(),
            Err(SafeStorageValidationError::CursorShape)
        );
        continuation.expected_account_version = Some(4);
        continuation.expected_inventory_version = Some(7);
        continuation.validate().unwrap();
    }

    #[test]
    fn stored_page_enforces_slot_and_uid_order() {
        let stack = SafeStorageStackV1 {
            location: SafeStorageLocationV1::Vault,
            slot_index: 4,
            template_id: WireText::new("item.weapon.test").unwrap(),
            item_kind: SafeStorageItemKindV1::Equipment,
            item_level: Some(1),
            rarity: Some(SafeStorageRarityV1::Worn),
            security: SafeStorageSecurityV1::Safe,
            provenance: SafeStorageProvenanceV1::Starter,
            salvage_band: 0,
            salvage_value: 0,
            overflow_expires_at_unix_millis: None,
            items: vec![SafeStorageItemV1 {
                item_uid: [1; 16],
                item_version: 2,
                item_level: Some(1),
                rarity: Some(SafeStorageRarityV1::Worn),
                security: SafeStorageSecurityV1::Safe,
                provenance: SafeStorageProvenanceV1::Starter,
                salvage_band: 0,
                salvage_value: 0,
                overflow_expires_at_unix_millis: None,
            }],
        };
        let result = SafeStorageQueryResultV1::Stored {
            schema_version: 1,
            sequence: 1,
            character_id: [2; 16],
            surface: SafeStorageSurfaceV1::Vault,
            account_version: 3,
            inventory_version: 4,
            content_revision: WireText::new("core-dev.blake3.test").unwrap(),
            character_safe: Vec::new(),
            stacks: vec![stack],
            next_after_slot: Some(4),
        };
        result.validate().unwrap();
    }
}
