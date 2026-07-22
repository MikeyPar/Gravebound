//! Append-only protocol 1.19 projection for pending-at-risk Core inventory.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` `LOOT-002`, `LOOT-010`,
//! `LOOT-033`, `LOOT-060`, and `TECH-015`; `Gravebound_Content_Production_Spec_v1.md`
//! `CONT-REWARD-003` and the fixed Sir Caldus exit; and
//! `Gravebound_Development_Roadmap_v1.md` `GB-M03-03`, `GB-M03-04`, and `GB-M03-08`.
//!
//! The projection is read-only. It exposes current custody and the exact version vector a client
//! must echo when requesting extraction, but never accepts destinations or placement authority.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CHARACTER_ID_BYTES, INSTANCE_LINEAGE_ID_BYTES, ManifestHash, TERMINAL_INVENTORY_ID_BYTES,
    TerminalExpectedVersionsV1, WORLD_FLOW_ID_MAX_BYTES, WireText, WorldFlowContentRevisionV1,
};

pub const CORE_PENDING_INVENTORY_SCHEMA_VERSION: u16 = 1;
pub const CORE_PENDING_INVENTORY_FEATURE_FLAG: &str = "core_pending_inventory_v1";
pub const CORE_PENDING_BACKPACK_CAPACITY: u8 = 8;
pub const CORE_PENDING_ITEM_CAPACITY: usize = 64;
pub const CORE_PENDING_MATERIAL_CAPACITY: usize = 4;
const CORE_RED_TONIC_ID: &str = "consumable.red_tonic";
const CORE_RED_TONIC_STACK_CAP: usize = 6;
const RUN_MATERIAL_STACK_CAP: u16 = 99;

/// Server-issued terminal identity paired with the coherent pending-inventory projection. This is
/// a separate append-only event so protocol 1.19's existing inventory shape remains immutable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreExtractionReadyStateV1 {
    pub schema_version: u16,
    pub character_id: [u8; CHARACTER_ID_BYTES],
    pub instance_lineage_id: [u8; INSTANCE_LINEAGE_ID_BYTES],
    pub entry_restore_point_id: [u8; TERMINAL_INVENTORY_ID_BYTES],
    pub extraction_request_id: [u8; TERMINAL_INVENTORY_ID_BYTES],
    pub content_revision: WorldFlowContentRevisionV1,
    pub expected_versions: TerminalExpectedVersionsV1,
}

impl CoreExtractionReadyStateV1 {
    pub fn validate(&self) -> Result<(), CorePendingInventoryValidationError> {
        if self.schema_version != CORE_PENDING_INVENTORY_SCHEMA_VERSION {
            return Err(CorePendingInventoryValidationError::SchemaVersion);
        }
        if [
            self.character_id,
            self.instance_lineage_id,
            self.entry_restore_point_id,
            self.extraction_request_id,
        ]
        .contains(&[0; 16])
        {
            return Err(CorePendingInventoryValidationError::ZeroIdentity);
        }
        if zero_revision(&self.content_revision) {
            return Err(CorePendingInventoryValidationError::InvalidContentRevision);
        }
        self.expected_versions
            .validate()
            .map_err(|_| CorePendingInventoryValidationError::InvalidVersions)?;
        if self.expected_versions.character != self.expected_versions.world {
            return Err(CorePendingInventoryValidationError::InvalidVersions);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorePendingItemKindV1 {
    Equipment,
    Consumable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorePendingItemLocationV1 {
    RunBackpack {
        index: u8,
    },
    PersonalGround {
        instance_id: [u8; TERMINAL_INVENTORY_ID_BYTES],
        pickup_id: [u8; TERMINAL_INVENTORY_ID_BYTES],
        expires_at_tick: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorePendingItemV1 {
    pub item_uid: [u8; TERMINAL_INVENTORY_ID_BYTES],
    pub template_id: WireText<WORLD_FLOW_ID_MAX_BYTES>,
    pub kind: CorePendingItemKindV1,
    pub item_version: u64,
    pub location: CorePendingItemLocationV1,
}

impl CorePendingItemV1 {
    fn order_key(
        &self,
    ) -> Result<(u8, [u8; 16], [u8; 16], u64), CorePendingInventoryValidationError> {
        if all_zero(&self.item_uid) || self.item_version == 0 {
            return Err(CorePendingInventoryValidationError::InvalidItem);
        }
        match self.location {
            CorePendingItemLocationV1::RunBackpack { index }
                if index < CORE_PENDING_BACKPACK_CAPACITY =>
            {
                Ok((0, [0; 16], [0; 16], u64::from(index)))
            }
            CorePendingItemLocationV1::PersonalGround {
                instance_id,
                pickup_id,
                expires_at_tick,
            } if !all_zero(&instance_id) && !all_zero(&pickup_id) && expires_at_tick > 0 => {
                Ok((1, instance_id, pickup_id, expires_at_tick))
            }
            _ => Err(CorePendingInventoryValidationError::InvalidItemLocation),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorePendingMaterialV1 {
    pub material_id: WireText<WORLD_FLOW_ID_MAX_BYTES>,
    pub quantity: u16,
    pub material_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorePendingInventoryStateV1 {
    pub schema_version: u16,
    pub character_id: [u8; CHARACTER_ID_BYTES],
    pub instance_lineage_id: [u8; INSTANCE_LINEAGE_ID_BYTES],
    pub entry_restore_point_id: [u8; TERMINAL_INVENTORY_ID_BYTES],
    pub location_content_id: WireText<WORLD_FLOW_ID_MAX_BYTES>,
    pub content_revision: WorldFlowContentRevisionV1,
    pub expected_extraction_versions: TerminalExpectedVersionsV1,
    pub items: Vec<CorePendingItemV1>,
    pub materials: Vec<CorePendingMaterialV1>,
}

impl CorePendingInventoryStateV1 {
    #[must_use]
    pub const fn required_feature_flag() -> &'static str {
        CORE_PENDING_INVENTORY_FEATURE_FLAG
    }

    pub fn validate(&self) -> Result<(), CorePendingInventoryValidationError> {
        if self.schema_version != CORE_PENDING_INVENTORY_SCHEMA_VERSION {
            return Err(CorePendingInventoryValidationError::SchemaVersion);
        }
        if all_zero(&self.character_id)
            || all_zero(&self.instance_lineage_id)
            || all_zero(&self.entry_restore_point_id)
        {
            return Err(CorePendingInventoryValidationError::ZeroIdentity);
        }
        if zero_revision(&self.content_revision) {
            return Err(CorePendingInventoryValidationError::InvalidContentRevision);
        }
        self.expected_extraction_versions
            .validate()
            .map_err(|_| CorePendingInventoryValidationError::InvalidVersions)?;
        if self.expected_extraction_versions.character != self.expected_extraction_versions.world {
            return Err(CorePendingInventoryValidationError::InvalidVersions);
        }
        if self.items.len() > CORE_PENDING_ITEM_CAPACITY
            || self.materials.len() > CORE_PENDING_MATERIAL_CAPACITY
        {
            return Err(CorePendingInventoryValidationError::Capacity);
        }

        let mut item_uids = BTreeSet::new();
        let mut stacks: BTreeMap<CorePendingItemLocationV1, (CorePendingItemKindV1, &str, usize)> =
            BTreeMap::new();
        let mut previous_item_key = None;
        for item in &self.items {
            let key = (item.order_key()?, item.item_uid);
            if previous_item_key.is_some_and(|previous| previous >= key)
                || !item_uids.insert(item.item_uid)
            {
                return Err(CorePendingInventoryValidationError::NonCanonicalOrder);
            }
            let stack =
                stacks
                    .entry(item.location)
                    .or_insert((item.kind, item.template_id.as_str(), 0));
            stack.2 = stack.2.saturating_add(1);
            let valid_stack = match item.kind {
                CorePendingItemKindV1::Equipment => {
                    stack.0 == CorePendingItemKindV1::Equipment
                        && stack.1 == item.template_id.as_str()
                        && stack.2 == 1
                }
                CorePendingItemKindV1::Consumable => {
                    stack.0 == CorePendingItemKindV1::Consumable
                        && stack.1 == item.template_id.as_str()
                        && stack.1 == CORE_RED_TONIC_ID
                        && stack.2 <= CORE_RED_TONIC_STACK_CAP
                }
            };
            if !valid_stack {
                return Err(CorePendingInventoryValidationError::InvalidStack);
            }
            previous_item_key = Some(key);
        }

        let mut previous_material = None;
        for material in &self.materials {
            if material.quantity == 0
                || material.quantity > RUN_MATERIAL_STACK_CAP
                || material.material_version == 0
                || previous_material
                    .is_some_and(|previous: &str| previous >= material.material_id.as_str())
            {
                return Err(CorePendingInventoryValidationError::InvalidMaterial);
            }
            previous_material = Some(material.material_id.as_str());
        }
        Ok(())
    }
}

fn zero_revision(revision: &WorldFlowContentRevisionV1) -> bool {
    [
        &revision.records_blake3,
        &revision.assets_blake3,
        &revision.localization_blake3,
    ]
    .into_iter()
    .any(|hash: &ManifestHash| hash.as_str().bytes().all(|byte| byte == b'0'))
}

const fn all_zero<const N: usize>(value: &[u8; N]) -> bool {
    let mut index = 0;
    while index < N {
        if value[index] != 0 {
            return false;
        }
        index += 1;
    }
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CorePendingInventoryValidationError {
    #[error("pending-inventory schema version is unsupported")]
    SchemaVersion,
    #[error("pending-inventory identity must be nonzero")]
    ZeroIdentity,
    #[error("pending-inventory content revision is invalid")]
    InvalidContentRevision,
    #[error("pending-inventory extraction versions are invalid")]
    InvalidVersions,
    #[error("pending-inventory capacity is exceeded")]
    Capacity,
    #[error("pending item is invalid")]
    InvalidItem,
    #[error("pending item location is invalid")]
    InvalidItemLocation,
    #[error("pending item stack shape is invalid")]
    InvalidStack,
    #[error("pending items or materials are not in canonical order")]
    NonCanonicalOrder,
    #[error("pending material is invalid")]
    InvalidMaterial,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(byte: char) -> ManifestHash {
        ManifestHash::new(byte.to_string().repeat(64)).expect("hash")
    }

    fn state() -> CorePendingInventoryStateV1 {
        CorePendingInventoryStateV1 {
            schema_version: CORE_PENDING_INVENTORY_SCHEMA_VERSION,
            character_id: [1; 16],
            instance_lineage_id: [2; 16],
            entry_restore_point_id: [3; 16],
            location_content_id: WireText::new("dungeon.bell_sepulcher").expect("location"),
            content_revision: WorldFlowContentRevisionV1 {
                records_blake3: hash('1'),
                assets_blake3: hash('2'),
                localization_blake3: hash('3'),
            },
            expected_extraction_versions: TerminalExpectedVersionsV1 {
                account: 4,
                character: 5,
                world: 5,
                inventory: 6,
                life_clock: 7,
            },
            items: vec![
                CorePendingItemV1 {
                    item_uid: [4; 16],
                    template_id: WireText::new("item.weapon.sword.bell_cleaver_caldus")
                        .expect("template"),
                    kind: CorePendingItemKindV1::Equipment,
                    item_version: 1,
                    location: CorePendingItemLocationV1::RunBackpack { index: 0 },
                },
                CorePendingItemV1 {
                    item_uid: [5; 16],
                    template_id: WireText::new("consumable.red_tonic").expect("template"),
                    kind: CorePendingItemKindV1::Consumable,
                    item_version: 1,
                    location: CorePendingItemLocationV1::PersonalGround {
                        instance_id: [6; 16],
                        pickup_id: [7; 16],
                        expires_at_tick: 1_800,
                    },
                },
            ],
            materials: Vec::new(),
        }
    }

    #[test]
    fn extraction_ready_authority_requires_complete_correlated_identity() {
        let state = CoreExtractionReadyStateV1 {
            schema_version: CORE_PENDING_INVENTORY_SCHEMA_VERSION,
            character_id: [1; 16],
            instance_lineage_id: [2; 16],
            entry_restore_point_id: [3; 16],
            extraction_request_id: [4; 16],
            content_revision: WorldFlowContentRevisionV1 {
                records_blake3: hash('1'),
                assets_blake3: hash('2'),
                localization_blake3: hash('3'),
            },
            expected_versions: TerminalExpectedVersionsV1 {
                account: 1,
                character: 2,
                world: 2,
                inventory: 3,
                life_clock: 4,
            },
        };
        assert!(state.validate().is_ok());
        let mut invalid = state;
        invalid.extraction_request_id = [0; 16];
        assert_eq!(
            invalid.validate(),
            Err(CorePendingInventoryValidationError::ZeroIdentity)
        );
    }

    #[test]
    fn canonical_projection_is_bounded_and_exposes_exact_terminal_versions() {
        let state = state();
        state.validate().expect("valid state");
        assert_eq!(state.expected_extraction_versions.inventory, 6);
        assert_eq!(state.items.len(), 2);
    }

    #[test]
    fn order_capacity_identity_and_version_drift_fail_closed() {
        let mut changed = state();
        changed.items.reverse();
        assert_eq!(
            changed.validate(),
            Err(CorePendingInventoryValidationError::NonCanonicalOrder)
        );

        let mut changed = state();
        changed.expected_extraction_versions.world = 8;
        assert_eq!(
            changed.validate(),
            Err(CorePendingInventoryValidationError::InvalidVersions)
        );

        let mut changed = state();
        changed.items[0].location = CorePendingItemLocationV1::RunBackpack { index: 8 };
        assert_eq!(
            changed.validate(),
            Err(CorePendingInventoryValidationError::InvalidItemLocation)
        );
    }

    #[test]
    fn impossible_equipment_consumable_and_material_stacks_fail_closed() {
        let mut changed = state();
        changed.items.insert(
            1,
            CorePendingItemV1 {
                item_uid: [8; 16],
                template_id: WireText::new("item.weapon.sword.bell_cleaver_caldus")
                    .expect("template"),
                kind: CorePendingItemKindV1::Equipment,
                item_version: 1,
                location: CorePendingItemLocationV1::RunBackpack { index: 0 },
            },
        );
        assert_eq!(
            changed.validate(),
            Err(CorePendingInventoryValidationError::InvalidStack)
        );

        let mut changed = state();
        changed.items = (0_u8..7)
            .map(|uid| CorePendingItemV1 {
                item_uid: [uid.saturating_add(1); 16],
                template_id: WireText::new("consumable.red_tonic").expect("template"),
                kind: CorePendingItemKindV1::Consumable,
                item_version: 1,
                location: CorePendingItemLocationV1::RunBackpack { index: 0 },
            })
            .collect();
        assert_eq!(
            changed.validate(),
            Err(CorePendingInventoryValidationError::InvalidStack)
        );

        let mut changed = state();
        changed.materials.push(CorePendingMaterialV1 {
            material_id: WireText::new("material.bell_brass").expect("material"),
            quantity: 100,
            material_version: 1,
        });
        assert_eq!(
            changed.validate(),
            Err(CorePendingInventoryValidationError::InvalidMaterial)
        );
    }

    #[test]
    fn reliable_event_remains_append_only_after_protocol_nineteen() {
        let event = crate::WireMessage::ReliableEvent(crate::ReliableEventFrame {
            sequence: 1,
            server_tick: 900,
            event: crate::ReliableEvent::CorePendingInventoryState(Box::new(state())),
        });
        let event_bytes = postcard::to_stdvec(match &event {
            crate::WireMessage::ReliableEvent(frame) => &frame.event,
            _ => unreachable!("reliable event"),
        })
        .expect("event bytes");
        assert_eq!(event_bytes[0], 21, "append-only reliable-event tail");
        let encoded = crate::encode_frame(&event).expect("current protocol frame");
        assert_eq!(
            u16::from_le_bytes([encoded[6], encoded[7]]),
            crate::PROTOCOL_MINOR
        );
        assert_eq!(
            crate::encode_protocol_1_18_compatibility_frame(&event),
            Err(crate::WireCodecError::MessageUnavailableAtVersion)
        );
        assert_eq!(crate::decode_frame(&encoded).expect("decode"), event);
    }
}
