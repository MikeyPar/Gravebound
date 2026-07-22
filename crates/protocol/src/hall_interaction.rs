//! Append-only protocol 1.20 contract for authoritative Lantern Halls interactions.
//!
//! Clients publish bounded intent only. The server owns the nearest station, exact range,
//! 30 Hz hold clock, cancellation, and the single open panel.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::NetworkChannel;

pub const HALL_INTERACTION_SCHEMA_VERSION: u16 = 1;
pub const HALL_INTERACTION_FEATURE_FLAG: &str = "core_hall_interaction_v1";
pub const HALL_INTERACTION_HOLD_TICKS: u16 = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HallStationV1 {
    RealmGate,
    Vault,
    Overflow,
    MemorialWall,
    OathShrine,
}

impl HallStationV1 {
    #[must_use]
    pub const fn content_id(self) -> &'static str {
        match self {
            Self::RealmGate => "station.realm_gate",
            Self::Vault => "station.vault",
            Self::Overflow => "station.overflow",
            Self::MemorialWall => "station.memorial_wall",
            Self::OathShrine => "station.oath_shrine",
        }
    }

    #[must_use]
    pub const fn from_content_id(value: &str) -> Option<Self> {
        match value.as_bytes() {
            b"station.realm_gate" => Some(Self::RealmGate),
            b"station.vault" => Some(Self::Vault),
            b"station.overflow" => Some(Self::Overflow),
            b"station.memorial_wall" => Some(Self::MemorialWall),
            b"station.oath_shrine" => Some(Self::OathShrine),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HallInteractionIntentV1 {
    BeginHold,
    Release,
    ClosePanel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HallInteractionFrameV1 {
    pub schema_version: u16,
    pub sequence: u32,
    pub intent: HallInteractionIntentV1,
}

impl HallInteractionFrameV1 {
    #[must_use]
    pub const fn channel(&self) -> NetworkChannel {
        NetworkChannel::Action
    }

    pub const fn validate(&self) -> Result<(), HallInteractionValidationError> {
        if self.schema_version != HALL_INTERACTION_SCHEMA_VERSION {
            return Err(HallInteractionValidationError::SchemaVersion);
        }
        if self.sequence == 0 {
            return Err(HallInteractionValidationError::ZeroSequence);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HallInteractionResultCodeV1 {
    Holding,
    Opened,
    Closed,
    CancelledReleased,
    CancelledOutOfRange,
    OutOfRange,
    PanelAlreadyOpen,
    NoActiveHold,
    NoOpenPanel,
    StaleSequence,
    InvalidState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HallInteractionResultV1 {
    pub schema_version: u16,
    /// Correlates progress/open/cancellation events to the `BeginHold` that owns the hold.
    pub request_sequence: u32,
    pub code: HallInteractionResultCodeV1,
    pub station: Option<HallStationV1>,
    pub held_ticks: u16,
    pub required_ticks: u16,
}

impl HallInteractionResultV1 {
    pub const fn validate(&self) -> Result<(), HallInteractionValidationError> {
        if self.schema_version != HALL_INTERACTION_SCHEMA_VERSION {
            return Err(HallInteractionValidationError::SchemaVersion);
        }
        if self.request_sequence == 0 {
            return Err(HallInteractionValidationError::ZeroSequence);
        }
        let station_required = matches!(
            self.code,
            HallInteractionResultCodeV1::Holding
                | HallInteractionResultCodeV1::Opened
                | HallInteractionResultCodeV1::Closed
                | HallInteractionResultCodeV1::CancelledReleased
                | HallInteractionResultCodeV1::CancelledOutOfRange
                | HallInteractionResultCodeV1::PanelAlreadyOpen
        );
        if station_required != self.station.is_some() {
            return Err(HallInteractionValidationError::ResultShape);
        }
        match self.code {
            HallInteractionResultCodeV1::Holding
            | HallInteractionResultCodeV1::CancelledReleased
            | HallInteractionResultCodeV1::CancelledOutOfRange => {
                if self.required_ticks != HALL_INTERACTION_HOLD_TICKS
                    || self.held_ticks >= self.required_ticks
                {
                    return Err(HallInteractionValidationError::ResultShape);
                }
            }
            HallInteractionResultCodeV1::Opened => {
                let valid_instant = self.required_ticks == 0 && self.held_ticks == 0;
                let valid_hold = self.required_ticks == HALL_INTERACTION_HOLD_TICKS
                    && self.held_ticks == self.required_ticks;
                if !valid_instant && !valid_hold {
                    return Err(HallInteractionValidationError::ResultShape);
                }
            }
            HallInteractionResultCodeV1::Closed
            | HallInteractionResultCodeV1::PanelAlreadyOpen
            | HallInteractionResultCodeV1::OutOfRange
            | HallInteractionResultCodeV1::NoActiveHold
            | HallInteractionResultCodeV1::NoOpenPanel
            | HallInteractionResultCodeV1::StaleSequence
            | HallInteractionResultCodeV1::InvalidState => {
                if self.held_ticks != 0 || self.required_ticks != 0 {
                    return Err(HallInteractionValidationError::ResultShape);
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HallInteractionValidationError {
    #[error("Hall interaction schema version is unsupported")]
    SchemaVersion,
    #[error("Hall interaction sequence must be nonzero")]
    ZeroSequence,
    #[error("Hall interaction result has an invalid shape")]
    ResultShape,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_hold_and_instant_shapes_are_bounded() {
        for result in [
            HallInteractionResultV1 {
                schema_version: HALL_INTERACTION_SCHEMA_VERSION,
                request_sequence: 1,
                code: HallInteractionResultCodeV1::Holding,
                station: Some(HallStationV1::MemorialWall),
                held_ticks: 14,
                required_ticks: HALL_INTERACTION_HOLD_TICKS,
            },
            HallInteractionResultV1 {
                schema_version: HALL_INTERACTION_SCHEMA_VERSION,
                request_sequence: 2,
                code: HallInteractionResultCodeV1::Opened,
                station: Some(HallStationV1::Vault),
                held_ticks: 0,
                required_ticks: 0,
            },
        ] {
            assert_eq!(result.validate(), Ok(()));
        }
    }

    #[test]
    fn station_discriminants_and_ids_are_pinned_append_only() {
        let stations = [
            HallStationV1::RealmGate,
            HallStationV1::Vault,
            HallStationV1::Overflow,
            HallStationV1::MemorialWall,
            HallStationV1::OathShrine,
        ];
        for (ordinal, station) in stations.into_iter().enumerate() {
            assert_eq!(
                postcard::to_stdvec(&station).unwrap(),
                vec![u8::try_from(ordinal).unwrap()]
            );
            assert_eq!(
                HallStationV1::from_content_id(station.content_id()),
                Some(station)
            );
        }
    }
}
