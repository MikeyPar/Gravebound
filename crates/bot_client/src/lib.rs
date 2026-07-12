//! Headless Gravebound network-client boundary.
//!
//! `bot_client` will exercise the real protocol and player journey without rendering. It may
//! choose inputs, but it cannot author gameplay outcomes or bypass server authority.

use protocol::{ProtocolVersion, SIMULATION_HZ};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BotFoundation {
    pub protocol: ProtocolVersion,
    pub expected_server_hz: u16,
    pub local_simulation_hz: u32,
}

impl BotFoundation {
    #[must_use]
    pub const fn m02() -> Self {
        Self {
            protocol: ProtocolVersion::current(),
            expected_server_hz: SIMULATION_HZ,
            local_simulation_hz: sim_core::TICKS_PER_SECOND,
        }
    }

    pub fn validate(self) -> Result<(), BotFoundationError> {
        if self.local_simulation_hz != u32::from(self.expected_server_hz) {
            return Err(BotFoundationError::SimulationRateMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotDoctorReport {
    pub protocol: ProtocolVersion,
    pub expected_server_hz: u16,
    pub transport_enabled: bool,
    pub journey_enabled: bool,
}

pub async fn run_doctor() -> Result<BotDoctorReport, BotFoundationError> {
    let foundation = BotFoundation::m02();
    foundation.validate()?;
    tokio::task::yield_now().await;
    Ok(BotDoctorReport {
        protocol: foundation.protocol,
        expected_server_hz: foundation.expected_server_hz,
        transport_enabled: false,
        journey_enabled: false,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BotFoundationError {
    #[error("bot and authoritative server simulation rates differ")]
    SimulationRateMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bot_foundation_matches_server_tick_contract() {
        assert_eq!(BotFoundation::m02().validate(), Ok(()));
    }

    #[tokio::test]
    async fn doctor_does_not_claim_future_transport_or_journey_work() {
        let report = run_doctor().await.expect("M02 bot foundation doctor");
        assert_eq!(report.protocol, ProtocolVersion::current());
        assert_eq!(report.expected_server_hz, 30);
        assert!(!report.transport_enabled);
        assert!(!report.journey_enabled);
    }
}
