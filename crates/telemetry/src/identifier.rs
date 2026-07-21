use std::fmt;

use serde::{Serialize, Serializer};
use thiserror::Error;

pub const STABLE_TELEMETRY_ID_MAX_BYTES: usize = 96;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TelemetryId([u8; 16]);

impl TelemetryId {
    pub fn new(bytes: [u8; 16]) -> Result<Self, TelemetryIdentifierError> {
        if bytes.iter().all(|byte| *byte == 0) {
            return Err(TelemetryIdentifierError::ZeroIdentifier);
        }
        Ok(Self(bytes))
    }

    #[must_use]
    pub const fn as_bytes(self) -> [u8; 16] {
        self.0
    }
}

impl fmt::Debug for TelemetryId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TelemetryId(")?;
        write_hex(formatter, &self.0)?;
        formatter.write_str(")")
    }
}

impl Serialize for TelemetryId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex(&self.0))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PseudonymousAccountId([u8; 32]);

impl PseudonymousAccountId {
    pub fn new(bytes: [u8; 32]) -> Result<Self, TelemetryIdentifierError> {
        if bytes.iter().all(|byte| *byte == 0) {
            return Err(TelemetryIdentifierError::ZeroIdentifier);
        }
        Ok(Self(bytes))
    }
}

impl fmt::Debug for PseudonymousAccountId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PseudonymousAccountId([redacted])")
    }
}

impl Serialize for PseudonymousAccountId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex(&self.0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StableTelemetryId(String);

impl StableTelemetryId {
    pub fn new(value: impl Into<String>) -> Result<Self, TelemetryIdentifierError> {
        let value = value.into();
        let bytes = value.as_bytes();
        if bytes.is_empty() || bytes.len() > STABLE_TELEMETRY_ID_MAX_BYTES {
            return Err(TelemetryIdentifierError::StableIdLength);
        }
        if !bytes[0].is_ascii_lowercase()
            || !bytes.iter().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'_' | b'-')
            })
        {
            return Err(TelemetryIdentifierError::StableIdCharacters);
        }
        let lower = value.as_str();
        if lower.starts_with("bearer")
            || lower.starts_with("token")
            || lower.starts_with("secret")
            || lower.starts_with("password")
            || lower.starts_with("sk_")
            || lower.starts_with("eyj")
            || lower.contains("key=")
        {
            return Err(TelemetryIdentifierError::SensitiveLookingValue);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for StableTelemetryId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TelemetryIdentifierError {
    #[error("telemetry identifier must be nonzero")]
    ZeroIdentifier,
    #[error("stable telemetry identifier has an invalid length")]
    StableIdLength,
    #[error("stable telemetry identifier contains forbidden characters")]
    StableIdCharacters,
    #[error("stable telemetry identifier resembles secret material")]
    SensitiveLookingValue,
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn write_hex(formatter: &mut fmt::Formatter<'_>, bytes: &[u8]) -> fmt::Result {
    formatter.write_str(&hex(bytes))
}
