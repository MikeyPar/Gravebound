//! Content-bound combat presentation projection for the ordinary M03 private route.
//!
//! This is presentation data only. Collision, damage, timing, and terminal authority remain on
//! the server. Exact route context lets clients discard delayed bindings and telegraphs after a
//! transfer or actor-generation change.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CorePrivateRouteContentRevisionV1, CorePrivateRouteRoomV1, CorePrivateRouteSceneV1, WireText,
    messages::CONTENT_ID_MAX_BYTES,
};

pub const CORE_COMBAT_PRESENTATION_SCHEMA_VERSION: u16 = 1;
pub const CORE_COMBAT_PRESENTATION_FEATURE_FLAG: &str = "core_combat_presentation_v1";
pub const CORE_COMBAT_PRESENTATION_MAX_ACTORS: usize = 64;
pub const CORE_COMBAT_PRESENTATION_MAX_TELEGRAPHS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreCombatActorKindV1 {
    Player,
    Enemy,
    Boss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreCombatDamageTypeV1 {
    Physical,
    Veil,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreCombatActorBindingV1 {
    pub entity_id: u64,
    pub kind: CoreCombatActorKindV1,
    pub content_id: WireText<CONTENT_ID_MAX_BYTES>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreCombatTelegraphShapeV1 {
    Fan {
        ray_count: u8,
        ray_offsets_milli_degrees: [i32; 8],
        extent_milli_tiles: u32,
        ray_width_milli_tiles: u16,
    },
    AimedLane {
        extent_milli_tiles: u32,
        width_milli_tiles: u16,
    },
    Ring {
        segment_count: u8,
        gap_start_index: u8,
        gap_count: u8,
        radius_milli_tiles: u16,
        segment_width_milli_tiles: u16,
    },
    Lanes {
        axes_degrees: [u16; 2],
        width_milli_tiles: u16,
    },
    Rotor {
        arm_count: u8,
        clockwise_milli_degrees_per_second: u32,
        extent_milli_tiles: u32,
        arm_width_milli_tiles: u16,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreCombatTelegraphV1 {
    pub source_entity_id: u64,
    pub cast_id: u64,
    pub pattern_id: WireText<CONTENT_ID_MAX_BYTES>,
    pub damage_type: CoreCombatDamageTypeV1,
    pub starts_at_tick: u64,
    pub resolves_at_tick: u64,
    pub origin_x_milli_tiles: i32,
    pub origin_y_milli_tiles: i32,
    pub target_x_milli_tiles: i32,
    pub target_y_milli_tiles: i32,
    pub shape: CoreCombatTelegraphShapeV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreCombatPresentationStateV1 {
    pub schema_version: u16,
    pub content_revision: CorePrivateRouteContentRevisionV1,
    pub actor_generation: u64,
    pub route_state_version: u64,
    pub scene: CorePrivateRouteSceneV1,
    pub room: Option<CorePrivateRouteRoomV1>,
    pub server_tick: u64,
    /// Complete actor binding set for this authoritative frame.
    pub actors: Vec<CoreCombatActorBindingV1>,
    /// Telegraphs that began on `server_tick`; clients retain them through `resolves_at_tick`.
    pub telegraphs: Vec<CoreCombatTelegraphV1>,
}

impl CoreCombatPresentationStateV1 {
    pub fn validate(&self) -> Result<(), CoreCombatPresentationValidationError> {
        if self.schema_version != CORE_COMBAT_PRESENTATION_SCHEMA_VERSION
            || self.actor_generation == 0
            || self.route_state_version == 0
            || !matches!(
                (self.scene, self.room),
                (CorePrivateRouteSceneV1::CoreMicrorealm, None)
                    | (CorePrivateRouteSceneV1::BellSepulcher, Some(_))
            )
        {
            return Err(CoreCombatPresentationValidationError::InvalidAuthority);
        }
        if self.actors.is_empty()
            || self.actors.len() > CORE_COMBAT_PRESENTATION_MAX_ACTORS
            || self.telegraphs.len() > CORE_COMBAT_PRESENTATION_MAX_TELEGRAPHS
        {
            return Err(CoreCombatPresentationValidationError::Capacity);
        }
        let mut actor_ids = BTreeSet::new();
        let mut player_count = 0_u8;
        for (index, actor) in self.actors.iter().enumerate() {
            if actor.entity_id == 0 || !actor_ids.insert(actor.entity_id) {
                return Err(CoreCombatPresentationValidationError::Actor);
            }
            if actor.kind == CoreCombatActorKindV1::Player {
                player_count = player_count.saturating_add(1);
            }
            if index > 0 && self.actors[index - 1].entity_id >= actor.entity_id {
                return Err(CoreCombatPresentationValidationError::Actor);
            }
        }
        if player_count != 1 {
            return Err(CoreCombatPresentationValidationError::Actor);
        }
        let mut casts = BTreeSet::new();
        for (index, telegraph) in self.telegraphs.iter().enumerate() {
            if telegraph.source_entity_id == 0
                || telegraph.cast_id == 0
                || telegraph.starts_at_tick != self.server_tick
                || telegraph.resolves_at_tick <= telegraph.starts_at_tick
                || !actor_ids.contains(&telegraph.source_entity_id)
                || !casts.insert((telegraph.source_entity_id, telegraph.cast_id))
                || !valid_shape(&telegraph.shape)
            {
                return Err(CoreCombatPresentationValidationError::Telegraph);
            }
            if index > 0
                && (
                    self.telegraphs[index - 1].source_entity_id,
                    self.telegraphs[index - 1].cast_id,
                ) >= (telegraph.source_entity_id, telegraph.cast_id)
            {
                return Err(CoreCombatPresentationValidationError::Telegraph);
            }
        }
        Ok(())
    }
}

fn valid_shape(shape: &CoreCombatTelegraphShapeV1) -> bool {
    match shape {
        CoreCombatTelegraphShapeV1::Fan {
            ray_count,
            ray_offsets_milli_degrees,
            extent_milli_tiles,
            ray_width_milli_tiles,
        } => {
            (1..=8).contains(ray_count)
                && ray_offsets_milli_degrees[..usize::from(*ray_count)]
                    .iter()
                    .all(|offset| (-180_000..=180_000).contains(offset))
                && ray_offsets_milli_degrees[..usize::from(*ray_count)]
                    .windows(2)
                    .all(|pair| pair[0] < pair[1])
                && ray_offsets_milli_degrees[usize::from(*ray_count)..]
                    .iter()
                    .all(|offset| *offset == 0)
                && valid_extent(*extent_milli_tiles)
                && valid_width(*ray_width_milli_tiles)
        }
        CoreCombatTelegraphShapeV1::AimedLane {
            extent_milli_tiles,
            width_milli_tiles,
        } => valid_extent(*extent_milli_tiles) && valid_width(*width_milli_tiles),
        CoreCombatTelegraphShapeV1::Ring {
            segment_count,
            gap_start_index,
            gap_count,
            radius_milli_tiles,
            segment_width_milli_tiles,
        } => {
            (4..=24).contains(segment_count)
                && *gap_count > 0
                && *gap_count < *segment_count
                && *gap_start_index < *segment_count
                && (500..=8_000).contains(radius_milli_tiles)
                && valid_width(*segment_width_milli_tiles)
        }
        CoreCombatTelegraphShapeV1::Lanes {
            axes_degrees,
            width_milli_tiles,
        } => {
            axes_degrees.iter().all(|axis| *axis < 360)
                && axes_degrees[0] != axes_degrees[1]
                && valid_width(*width_milli_tiles)
        }
        CoreCombatTelegraphShapeV1::Rotor {
            arm_count,
            clockwise_milli_degrees_per_second,
            extent_milli_tiles,
            arm_width_milli_tiles,
        } => {
            (1..=4).contains(arm_count)
                && (1..=180_000).contains(clockwise_milli_degrees_per_second)
                && valid_extent(*extent_milli_tiles)
                && valid_width(*arm_width_milli_tiles)
        }
    }
}

const fn valid_extent(extent_milli_tiles: u32) -> bool {
    extent_milli_tiles >= 500 && extent_milli_tiles <= 30_000
}

const fn valid_width(width_milli_tiles: u16) -> bool {
    width_milli_tiles >= 80 && width_milli_tiles <= 3_000
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CoreCombatPresentationValidationError {
    #[error("combat presentation authority is invalid")]
    InvalidAuthority,
    #[error("combat presentation capacity was exceeded")]
    Capacity,
    #[error("combat presentation actor binding is invalid")]
    Actor,
    #[error("combat presentation telegraph is invalid")]
    Telegraph,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ManifestHash, NetworkChannel, ReliableEvent, ReliableEventFrame, WireCodecError,
        WireMessage, decode_frame, encode_frame, encode_protocol_1_22_compatibility_frame,
    };

    fn valid_state() -> CoreCombatPresentationStateV1 {
        CoreCombatPresentationStateV1 {
            schema_version: CORE_COMBAT_PRESENTATION_SCHEMA_VERSION,
            content_revision: CorePrivateRouteContentRevisionV1 {
                records_blake3: ManifestHash::new("a".repeat(64)).unwrap(),
                assets_blake3: ManifestHash::new("b".repeat(64)).unwrap(),
                localization_blake3: ManifestHash::new("c".repeat(64)).unwrap(),
            },
            actor_generation: 7,
            route_state_version: 11,
            scene: CorePrivateRouteSceneV1::CoreMicrorealm,
            room: None,
            server_tick: 120,
            actors: vec![
                CoreCombatActorBindingV1 {
                    entity_id: 10,
                    kind: CoreCombatActorKindV1::Player,
                    content_id: WireText::new(crate::GRAVE_ARBALIST_CLASS_ID).unwrap(),
                },
                CoreCombatActorBindingV1 {
                    entity_id: 20,
                    kind: CoreCombatActorKindV1::Enemy,
                    content_id: WireText::new("enemy.drowned_pilgrim").unwrap(),
                },
            ],
            telegraphs: vec![CoreCombatTelegraphV1 {
                source_entity_id: 20,
                cast_id: 3,
                pattern_id: WireText::new("pattern.enemy.drowned_pilgrim.fan").unwrap(),
                damage_type: CoreCombatDamageTypeV1::Physical,
                starts_at_tick: 120,
                resolves_at_tick: 129,
                origin_x_milli_tiles: 1_000,
                origin_y_milli_tiles: 2_000,
                target_x_milli_tiles: 4_000,
                target_y_milli_tiles: 2_000,
                shape: CoreCombatTelegraphShapeV1::Fan {
                    ray_count: 3,
                    ray_offsets_milli_degrees: [-15_000, 0, 15_000, 0, 0, 0, 0, 0],
                    extent_milli_tiles: 12_100,
                    ray_width_milli_tiles: 240,
                },
            }],
        }
    }

    #[test]
    fn core_combat_presentation_1_23_round_trips_on_pattern_channel_only() {
        let message = WireMessage::ReliableEvent(ReliableEventFrame {
            sequence: 5,
            server_tick: 120,
            event: ReliableEvent::CoreCombatPresentationState(Box::new(valid_state())),
        });

        assert_eq!(message.channel(), NetworkChannel::Pattern);
        let encoded = encode_frame(&message).unwrap();
        assert_eq!(u16::from_le_bytes([encoded[6], encoded[7]]), 23);
        assert_eq!(decode_frame(&encoded).unwrap(), message);
    }

    #[test]
    fn core_combat_presentation_is_unavailable_to_1_22_encoder() {
        let message = WireMessage::ReliableEvent(ReliableEventFrame {
            sequence: 5,
            server_tick: 120,
            event: ReliableEvent::CoreCombatPresentationState(Box::new(valid_state())),
        });

        assert_eq!(
            encode_protocol_1_22_compatibility_frame(&message),
            Err(WireCodecError::MessageUnavailableAtVersion)
        );
    }

    #[test]
    fn combat_presentation_rejects_inactive_fan_tail() {
        let mut state = valid_state();
        let CoreCombatTelegraphShapeV1::Fan {
            ray_offsets_milli_degrees,
            ..
        } = &mut state.telegraphs[0].shape
        else {
            unreachable!();
        };
        ray_offsets_milli_degrees[3] = 1;

        assert_eq!(
            state.validate(),
            Err(CoreCombatPresentationValidationError::Telegraph)
        );
    }

    #[test]
    fn combat_presentation_rejects_nonordered_active_fan_offsets() {
        let mut state = valid_state();
        let CoreCombatTelegraphShapeV1::Fan {
            ray_offsets_milli_degrees,
            ..
        } = &mut state.telegraphs[0].shape
        else {
            unreachable!();
        };
        ray_offsets_milli_degrees[..3].copy_from_slice(&[0, -15_000, 15_000]);

        assert_eq!(
            state.validate(),
            Err(CoreCombatPresentationValidationError::Telegraph)
        );
    }

    #[test]
    fn combat_presentation_rejects_invalid_ring_gap() {
        let mut state = valid_state();
        state.telegraphs[0].shape = CoreCombatTelegraphShapeV1::Ring {
            segment_count: 8,
            gap_start_index: 8,
            gap_count: 2,
            radius_milli_tiles: 3_000,
            segment_width_milli_tiles: 240,
        };

        assert_eq!(
            state.validate(),
            Err(CoreCombatPresentationValidationError::Telegraph)
        );
    }
}
