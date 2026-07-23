//! Bounded, analytics-only client reconciliation diagnostics.
//!
//! The server remains authoritative for RTT, jitter, and packet loss. This frame reports only
//! whether the native client has a reconciliation counter and, when it does, its cumulative value.
//! It is never admitted as gameplay authority.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`TEL-003`, `SIM-012`,
//! `TECH-120`), `Gravebound_Content_Production_Spec_v1.md` (Core capacity-one
//! constraints), and `Gravebound_Development_Roadmap_v1.md` (`GB-M03-09`).

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const NETWORK_DIAGNOSTICS_SCHEMA_VERSION: u16 = 1;
pub const NETWORK_DIAGNOSTICS_FEATURE_FLAG: &str = "telemetry.network-health.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientCorrectionDiagnosticsV1 {
    /// This runtime does not currently own a prediction/reconciliation counter.
    Unavailable,
    /// Cumulative corrections in the current authenticated transport generation.
    Observed { cumulative_count: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientNetworkDiagnosticsFrameV1 {
    pub schema_version: u16,
    pub sample_sequence: u32,
    pub corrections: ClientCorrectionDiagnosticsV1,
}

impl ClientNetworkDiagnosticsFrameV1 {
    pub const fn validate(&self) -> Result<(), NetworkDiagnosticsValidationError> {
        if self.schema_version != NETWORK_DIAGNOSTICS_SCHEMA_VERSION {
            return Err(NetworkDiagnosticsValidationError::SchemaVersion);
        }
        if self.sample_sequence == 0 {
            return Err(NetworkDiagnosticsValidationError::SampleSequence);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientNetworkDiagnosticsResultCodeV1 {
    Accepted,
    Stale,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientNetworkDiagnosticsResultV1 {
    pub schema_version: u16,
    pub sample_sequence: u32,
    pub code: ClientNetworkDiagnosticsResultCodeV1,
}

impl ClientNetworkDiagnosticsResultV1 {
    pub const fn validate(&self) -> Result<(), NetworkDiagnosticsValidationError> {
        if self.schema_version != NETWORK_DIAGNOSTICS_SCHEMA_VERSION {
            return Err(NetworkDiagnosticsValidationError::SchemaVersion);
        }
        if self.sample_sequence == 0 {
            return Err(NetworkDiagnosticsValidationError::SampleSequence);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NetworkDiagnosticsValidationError {
    #[error("network diagnostics schema version is unsupported")]
    SchemaVersion,
    #[error("network diagnostics sample sequence must be nonzero")]
    SampleSequence,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correction_availability_is_explicit_and_sequence_is_bounded() {
        let frame = ClientNetworkDiagnosticsFrameV1 {
            schema_version: NETWORK_DIAGNOSTICS_SCHEMA_VERSION,
            sample_sequence: 1,
            corrections: ClientCorrectionDiagnosticsV1::Unavailable,
        };
        assert_eq!(frame.validate(), Ok(()));
        assert_eq!(
            ClientNetworkDiagnosticsFrameV1 {
                sample_sequence: 0,
                ..frame
            }
            .validate(),
            Err(NetworkDiagnosticsValidationError::SampleSequence)
        );
    }
}
