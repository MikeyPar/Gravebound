//! Gravebound authoritative modular-monolith boundary.
//!
//! `server_app` owns sessions, instance orchestration, routing, and authoritative execution of
//! `sim_core`. It must not own rendering, client settings, gameplay rules, or persistence logic.
//! M02 deliberately has no database dependency.

use protocol::{ProtocolVersion, SIMULATION_HZ, SNAPSHOT_HZ, UpdateRates};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerFoundation {
    pub protocol: ProtocolVersion,
    pub rates: UpdateRates,
    pub simulation_ticks_per_second: u32,
}

impl ServerFoundation {
    #[must_use]
    pub const fn m02() -> Self {
        Self {
            protocol: ProtocolVersion::current(),
            rates: UpdateRates::canonical(),
            simulation_ticks_per_second: sim_core::TICKS_PER_SECOND,
        }
    }

    pub fn validate(self) -> Result<(), ServerFoundationError> {
        self.rates
            .validate()
            .map_err(|_| ServerFoundationError::ProtocolRates)?;
        if self.simulation_ticks_per_second != u32::from(SIMULATION_HZ) {
            return Err(ServerFoundationError::SimulationRateMismatch {
                protocol_hz: SIMULATION_HZ,
                simulation_hz: self.simulation_ticks_per_second,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerDoctorReport {
    pub protocol: ProtocolVersion,
    pub simulation_hz: u32,
    pub snapshot_hz: u16,
    pub database_enabled: bool,
    pub transport_enabled: bool,
}

pub async fn run_doctor() -> Result<ServerDoctorReport, ServerFoundationError> {
    let foundation = ServerFoundation::m02();
    foundation.validate()?;
    tokio::task::yield_now().await;
    Ok(ServerDoctorReport {
        protocol: foundation.protocol,
        simulation_hz: foundation.simulation_ticks_per_second,
        snapshot_hz: SNAPSHOT_HZ,
        database_enabled: false,
        transport_enabled: false,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ServerFoundationError {
    #[error("protocol update rates failed validation")]
    ProtocolRates,
    #[error(
        "protocol and sim_core tick rates differ: protocol={protocol_hz}, sim_core={simulation_hz}"
    )]
    SimulationRateMismatch {
        protocol_hz: u16,
        simulation_hz: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authoritative_server_uses_the_shared_simulation_rate() {
        assert_eq!(ServerFoundation::m02().validate(), Ok(()));
        assert_eq!(sim_core::TICKS_PER_SECOND, 30);
    }

    #[tokio::test]
    async fn doctor_is_explicit_about_unimplemented_m02_01_transport() {
        let report = run_doctor().await.expect("M02 foundation doctor");
        assert_eq!(report.protocol, ProtocolVersion::current());
        assert_eq!(report.simulation_hz, 30);
        assert_eq!(report.snapshot_hz, 15);
        assert!(!report.database_enabled);
        assert!(!report.transport_enabled);
    }
}
