use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{CHARACTER_ID_BYTES, MUTATION_ID_BYTES, NetworkChannel, PAYLOAD_HASH_BYTES, WireText};

pub const FIELD_EQUIPMENT_ID_MAX_BYTES: usize = 96;
pub const FIELD_EQUIPMENT_CHANGE_CAPACITY: usize = 32;
pub const FIELD_EQUIPMENT_ITEM_UID_BYTES: usize = 16;
pub const FIELD_EQUIPMENT_PICKUP_ID_BYTES: usize = 16;
pub const FIELD_EQUIPMENT_PREVIEW_HASH_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldEquipmentSlotV1 {
    Weapon,
    Armor,
    Relic,
    Charm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldEquipmentRarityV1 {
    Worn,
    Forged,
    Oathed,
    Relic,
    Sainted,
    BlackUnique,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldEquipmentSourceV1 {
    RunBackpack {
        slot_index: u8,
    },
    PersonalGround {
        item_uid: [u8; FIELD_EQUIPMENT_ITEM_UID_BYTES],
        pickup_id: [u8; FIELD_EQUIPMENT_PICKUP_ID_BYTES],
    },
}

impl FieldEquipmentSourceV1 {
    fn validate(self) -> Result<(), FieldEquipmentValidationError> {
        match self {
            Self::RunBackpack { slot_index } if slot_index < 8 => Ok(()),
            Self::RunBackpack { .. } => Err(FieldEquipmentValidationError::BackpackIndex),
            Self::PersonalGround {
                item_uid,
                pickup_id,
            } if !all_zero(&item_uid) && !all_zero(&pickup_id) => Ok(()),
            Self::PersonalGround { .. } => Err(FieldEquipmentValidationError::ZeroIdentity),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldEquipmentReplacementDestinationV1 {
    None,
    RunBackpack { slot_index: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldEquipmentItemV1 {
    pub item_uid: [u8; FIELD_EQUIPMENT_ITEM_UID_BYTES],
    pub template_id: WireText<FIELD_EQUIPMENT_ID_MAX_BYTES>,
    pub slot: FieldEquipmentSlotV1,
    pub item_level: u8,
    pub rarity: FieldEquipmentRarityV1,
    pub item_version: u64,
    pub behavior_key: WireText<FIELD_EQUIPMENT_ID_MAX_BYTES>,
}

impl FieldEquipmentItemV1 {
    fn validate(&self) -> Result<(), FieldEquipmentValidationError> {
        if all_zero(&self.item_uid) {
            return Err(FieldEquipmentValidationError::ZeroIdentity);
        }
        if !(1..=20).contains(&self.item_level) || self.item_version == 0 {
            return Err(FieldEquipmentValidationError::InvalidItem);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldEquipmentComparisonAxisV1 {
    WeaponDamage,
    AttackIntervalMicros,
    RangeMilliTiles,
    ProjectileSpeedMilliTilesPerSecond,
    ProjectileRadiusMilliTiles,
    BoltCount,
    PierceCount,
    MaximumHealth,
    Armor,
    ResistanceBasisPoints,
    MovementBasisPoints,
    HealingReceivedBasisPoints,
    NegativeStatusReductionBasisPoints,
    DirectHitBarrierHealth,
    MarkDamageCoefficientBasisPoints,
    MarkDurationMillis,
    MarkPrimaryBonusBasisPoints,
    SlipstepDistanceMilliTiles,
    SlipstepDurationMillis,
    SlipstepDamageReductionBasisPoints,
    SlipstepCooldownMillis,
    RestedPrimaryBonusBasisPoints,
    RestedPrimaryIdleMillis,
    PotionHealingBasisPoints,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldEquipmentComparisonPreferenceV1 {
    Higher,
    Lower,
    Contextual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldEquipmentComparisonChangeV1 {
    pub axis: FieldEquipmentComparisonAxisV1,
    pub before: Option<i64>,
    pub after: Option<i64>,
    pub delta: i64,
    pub preference: FieldEquipmentComparisonPreferenceV1,
    pub advanced: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldEquipmentPreviewProjectionV1 {
    pub character_id: [u8; CHARACTER_ID_BYTES],
    pub inventory_version: u64,
    pub content_revision: WireText<FIELD_EQUIPMENT_ID_MAX_BYTES>,
    pub source: FieldEquipmentSourceV1,
    pub incoming: FieldEquipmentItemV1,
    pub current: Option<FieldEquipmentItemV1>,
    pub replacement_destination: FieldEquipmentReplacementDestinationV1,
    pub preview_hash: [u8; FIELD_EQUIPMENT_PREVIEW_HASH_BYTES],
    pub behavior_changed: bool,
    pub changes: Vec<FieldEquipmentComparisonChangeV1>,
}

impl FieldEquipmentPreviewProjectionV1 {
    pub fn validate(&self) -> Result<(), FieldEquipmentValidationError> {
        if all_zero(&self.character_id) || all_zero(&self.preview_hash) {
            return Err(FieldEquipmentValidationError::ZeroIdentity);
        }
        if self.inventory_version == 0 {
            return Err(FieldEquipmentValidationError::ZeroVersion);
        }
        self.source.validate()?;
        self.incoming.validate()?;
        if let Some(current) = &self.current {
            current.validate()?;
            if current.slot != self.incoming.slot {
                return Err(FieldEquipmentValidationError::SlotMismatch);
            }
        } else if !matches!(
            self.replacement_destination,
            FieldEquipmentReplacementDestinationV1::None
        ) {
            return Err(FieldEquipmentValidationError::DestinationMismatch);
        }
        if let FieldEquipmentReplacementDestinationV1::RunBackpack { slot_index } =
            self.replacement_destination
            && slot_index >= 8
        {
            return Err(FieldEquipmentValidationError::BackpackIndex);
        }
        if self.changes.len() > FIELD_EQUIPMENT_CHANGE_CAPACITY
            || self
                .changes
                .iter()
                .any(|change| change.before == change.after)
        {
            return Err(FieldEquipmentValidationError::InvalidComparison);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldEquipmentPreviewFrameV1 {
    pub sequence: u32,
    pub character_id: [u8; CHARACTER_ID_BYTES],
    pub source: FieldEquipmentSourceV1,
}

impl FieldEquipmentPreviewFrameV1 {
    #[must_use]
    pub const fn channel(&self) -> NetworkChannel {
        NetworkChannel::Mutation
    }

    pub fn validate(&self) -> Result<(), FieldEquipmentValidationError> {
        if self.sequence == 0 || all_zero(&self.character_id) {
            return Err(FieldEquipmentValidationError::ZeroIdentity);
        }
        self.source.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldEquipmentConfirmPayloadV1 {
    pub expected_inventory_version: u64,
    pub content_revision: WireText<FIELD_EQUIPMENT_ID_MAX_BYTES>,
    pub source: FieldEquipmentSourceV1,
    pub preview_hash: [u8; FIELD_EQUIPMENT_PREVIEW_HASH_BYTES],
}

impl FieldEquipmentConfirmPayloadV1 {
    #[must_use]
    pub fn canonical_hash(&self) -> [u8; PAYLOAD_HASH_BYTES] {
        let bytes = postcard::to_stdvec(self).expect("bounded equipment confirmation serializes");
        *blake3::hash(&bytes).as_bytes()
    }

    fn validate(&self) -> Result<(), FieldEquipmentValidationError> {
        if self.expected_inventory_version == 0 {
            return Err(FieldEquipmentValidationError::ZeroVersion);
        }
        if all_zero(&self.preview_hash) {
            return Err(FieldEquipmentValidationError::ZeroIdentity);
        }
        self.source.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldEquipmentConfirmFrameV1 {
    pub command_id: [u8; MUTATION_ID_BYTES],
    pub character_id: [u8; CHARACTER_ID_BYTES],
    pub issued_at_unix_millis: u64,
    pub payload_hash: [u8; PAYLOAD_HASH_BYTES],
    pub payload: FieldEquipmentConfirmPayloadV1,
}

impl FieldEquipmentConfirmFrameV1 {
    #[must_use]
    pub const fn channel(&self) -> NetworkChannel {
        NetworkChannel::Mutation
    }

    pub fn validate(&self) -> Result<(), FieldEquipmentValidationError> {
        if all_zero(&self.command_id)
            || all_zero(&self.character_id)
            || all_zero(&self.payload_hash)
            || self.issued_at_unix_millis == 0
        {
            return Err(FieldEquipmentValidationError::ZeroIdentity);
        }
        self.payload.validate()?;
        if self.payload_hash != self.payload.canonical_hash() {
            return Err(FieldEquipmentValidationError::PayloadHashMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldEquipmentResultCodeV1 {
    Accepted,
    CharacterNotFound,
    CharacterNotOwned,
    CharacterDead,
    InvalidLocation,
    SourceUnavailable,
    BackpackFull,
    InventoryVersionMismatch,
    StalePreview,
    ContentMismatch,
    IdempotencyConflict,
    ServiceUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FieldEquipmentValidationError {
    #[error("field-equipment identity or correlation value cannot be zero")]
    ZeroIdentity,
    #[error("field-equipment version must be positive")]
    ZeroVersion,
    #[error("RunBackpack index must be in 0..=7")]
    BackpackIndex,
    #[error("field-equipment item level or version is invalid")]
    InvalidItem,
    #[error("current and incoming equipment slots do not match")]
    SlotMismatch,
    #[error("replacement destination does not match the projection")]
    DestinationMismatch,
    #[error("field-equipment comparison is invalid or exceeds its bound")]
    InvalidComparison,
    #[error("field-equipment payload hash does not match its canonical payload")]
    PayloadHashMismatch,
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

    fn text(value: &str) -> WireText<FIELD_EQUIPMENT_ID_MAX_BYTES> {
        WireText::new(value).unwrap()
    }

    #[test]
    fn confirmation_hash_binds_source_version_revision_and_preview() {
        let payload = FieldEquipmentConfirmPayloadV1 {
            expected_inventory_version: 7,
            content_revision: text("core-dev.blake3.0123456789abcdef"),
            source: FieldEquipmentSourceV1::RunBackpack { slot_index: 3 },
            preview_hash: [4; 32],
        };
        let frame = FieldEquipmentConfirmFrameV1 {
            command_id: [1; 16],
            character_id: [2; 16],
            issued_at_unix_millis: 99,
            payload_hash: payload.canonical_hash(),
            payload,
        };
        frame.validate().unwrap();
        let mut altered = frame.clone();
        altered.payload.source = FieldEquipmentSourceV1::RunBackpack { slot_index: 4 };
        assert_eq!(
            altered.validate(),
            Err(FieldEquipmentValidationError::PayloadHashMismatch)
        );
    }

    #[test]
    fn projection_rejects_unbounded_changes_and_invented_destination() {
        let item = FieldEquipmentItemV1 {
            item_uid: [3; 16],
            template_id: text("item.weapon.crossbow.grave_repeater"),
            slot: FieldEquipmentSlotV1::Weapon,
            item_level: 4,
            rarity: FieldEquipmentRarityV1::Forged,
            item_version: 2,
            behavior_key: text("behavior.crossbow.single_bolt"),
        };
        let mut projection = FieldEquipmentPreviewProjectionV1 {
            character_id: [1; 16],
            inventory_version: 7,
            content_revision: text("core-dev.blake3.0123456789abcdef"),
            source: FieldEquipmentSourceV1::RunBackpack { slot_index: 0 },
            incoming: item,
            current: None,
            replacement_destination: FieldEquipmentReplacementDestinationV1::RunBackpack {
                slot_index: 0,
            },
            preview_hash: [5; 32],
            behavior_changed: true,
            changes: Vec::new(),
        };
        assert_eq!(
            projection.validate(),
            Err(FieldEquipmentValidationError::DestinationMismatch)
        );
        projection.replacement_destination = FieldEquipmentReplacementDestinationV1::None;
        projection.changes = (0..=FIELD_EQUIPMENT_CHANGE_CAPACITY)
            .map(|index| FieldEquipmentComparisonChangeV1 {
                axis: FieldEquipmentComparisonAxisV1::WeaponDamage,
                before: Some(i64::try_from(index).unwrap()),
                after: Some(i64::try_from(index).unwrap() + 1),
                delta: 1,
                preference: FieldEquipmentComparisonPreferenceV1::Higher,
                advanced: false,
            })
            .collect();
        assert_eq!(
            projection.validate(),
            Err(FieldEquipmentValidationError::InvalidComparison)
        );
    }
}
