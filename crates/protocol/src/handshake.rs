use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{AuthTicket, ManifestHash, ProtocolVersion, SIMULATION_HZ, SNAPSHOT_HZ, WireText};

pub const BUILD_ID_MAX_BYTES: usize = 96;
pub const SESSION_ID_MAX_BYTES: usize = 64;
pub const BUNDLE_ID_MAX_BYTES: usize = 32;
pub const REGION_ID_MAX_BYTES: usize = 32;
pub const LOCALE_MAX_BYTES: usize = 16;
pub const FEATURE_FLAG_MAX_BYTES: usize = 64;
pub const MAX_COMPRESSION_OPTIONS: usize = 4;
pub const MAX_FEATURE_FLAGS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    WindowsNative,
    SteamWindows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Compression {
    None,
    Zstd,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientHello {
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub client_build_id: WireText<BUILD_ID_MAX_BYTES>,
    pub platform: Platform,
    pub supported_compression: Vec<Compression>,
    pub content_manifest_hash: ManifestHash,
    pub auth_ticket: AuthTicket,
    pub locale: WireText<LOCALE_MAX_BYTES>,
}

impl ClientHello {
    #[must_use]
    pub const fn protocol_version(&self) -> ProtocolVersion {
        ProtocolVersion {
            major: self.protocol_major,
            minor: self.protocol_minor,
        }
    }

    pub fn validate(&self) -> Result<(), HandshakeValidationError> {
        if self.supported_compression.is_empty()
            || self.supported_compression.len() > MAX_COMPRESSION_OPTIONS
        {
            return Err(HandshakeValidationError::CompressionCount);
        }
        let unique = self
            .supported_compression
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if unique.len() != self.supported_compression.len() {
            return Err(HandshakeValidationError::DuplicateCompression);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerHello {
    pub session_id: WireText<SESSION_ID_MAX_BYTES>,
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub required_client_build: WireText<BUILD_ID_MAX_BYTES>,
    pub content_bundle_version: WireText<BUNDLE_ID_MAX_BYTES>,
    pub server_tick_rate: u16,
    pub snapshot_rate: u16,
    pub region_id: WireText<REGION_ID_MAX_BYTES>,
    pub feature_flags: Vec<WireText<FEATURE_FLAG_MAX_BYTES>>,
}

impl ServerHello {
    pub fn validate(&self) -> Result<(), HandshakeValidationError> {
        if self.server_tick_rate != SIMULATION_HZ || self.snapshot_rate != SNAPSHOT_HZ {
            return Err(HandshakeValidationError::ServerRates);
        }
        if self.feature_flags.len() > MAX_FEATURE_FLAGS {
            return Err(HandshakeValidationError::FeatureFlagCount);
        }
        let unique = self
            .feature_flags
            .iter()
            .map(WireText::as_str)
            .collect::<BTreeSet<_>>();
        if unique.len() != self.feature_flags.len() {
            return Err(HandshakeValidationError::DuplicateFeatureFlag);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandshakeRejection {
    Maintenance,
    UpdateRequired,
    ProtocolUnsupported,
    AuthenticationFailed,
    AccountSuspended,
    RegionFull,
    ContentMismatch,
    RateLimited,
    InternalRetryable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandshakeResponse {
    Accepted(ServerHello),
    Rejected(HandshakeRejection),
}

impl HandshakeResponse {
    pub fn validate(&self) -> Result<(), HandshakeValidationError> {
        match self {
            Self::Accepted(hello) => hello.validate(),
            Self::Rejected(_) => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HandshakeValidationError {
    #[error("client must advertise 1..={MAX_COMPRESSION_OPTIONS} compression options")]
    CompressionCount,
    #[error("client compression options must be unique")]
    DuplicateCompression,
    #[error("server hello rates must match the canonical 30/15 Hz contract")]
    ServerRates,
    #[error("server hello exceeds {MAX_FEATURE_FLAGS} feature flags")]
    FeatureFlagCount,
    #[error("server hello feature flags must be unique")]
    DuplicateFeatureFlag,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client_hello() -> ClientHello {
        ClientHello {
            protocol_major: 1,
            protocol_minor: crate::PROTOCOL_MINOR,
            client_build_id: WireText::new(format!("release-{}", "1".repeat(64))).unwrap(),
            platform: Platform::WindowsNative,
            supported_compression: vec![Compression::None],
            content_manifest_hash: ManifestHash::new("a".repeat(64)).unwrap(),
            auth_ticket: AuthTicket::new(b"test-ticket".to_vec()).unwrap(),
            locale: WireText::new("en-US").unwrap(),
        }
    }

    #[test]
    fn exact_rejection_set_and_valid_client_are_stable() {
        assert_eq!(client_hello().validate(), Ok(()));
        let rejections = [
            HandshakeRejection::Maintenance,
            HandshakeRejection::UpdateRequired,
            HandshakeRejection::ProtocolUnsupported,
            HandshakeRejection::AuthenticationFailed,
            HandshakeRejection::AccountSuspended,
            HandshakeRejection::RegionFull,
            HandshakeRejection::ContentMismatch,
            HandshakeRejection::RateLimited,
            HandshakeRejection::InternalRetryable,
        ];
        assert_eq!(rejections.len(), 9);
    }

    #[test]
    fn duplicate_and_unbounded_negotiation_values_fail_closed() {
        let mut hello = client_hello();
        hello.supported_compression = vec![Compression::None, Compression::None];
        assert_eq!(
            hello.validate(),
            Err(HandshakeValidationError::DuplicateCompression)
        );
        hello.supported_compression.clear();
        assert_eq!(
            hello.validate(),
            Err(HandshakeValidationError::CompressionCount)
        );
    }
}
