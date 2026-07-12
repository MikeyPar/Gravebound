use std::fmt;

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{Error as _, Visitor},
};
use thiserror::Error;

pub const AUTH_TICKET_MAX_BYTES: usize = 2_048;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WireText<const MAX: usize>(String);

impl<const MAX: usize> WireText<MAX> {
    pub fn new(value: impl Into<String>) -> Result<Self, BoundedValueError> {
        let value = value.into();
        if value.is_empty() {
            return Err(BoundedValueError::EmptyText);
        }
        if value.len() > MAX {
            return Err(BoundedValueError::TextTooLong {
                maximum: MAX,
                actual: value.len(),
            });
        }
        if !value.bytes().all(is_wire_text_byte) {
            return Err(BoundedValueError::InvalidTextCharacter);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<const MAX: usize> fmt::Debug for WireText<MAX> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("WireText").field(&self.0).finish()
    }
}

impl<const MAX: usize> fmt::Display for WireText<MAX> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<const MAX: usize> Serialize for WireText<MAX> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de, const MAX: usize> Deserialize<'de> for WireText<MAX> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct WireTextVisitor<const MAX: usize>;

        impl<const MAX: usize> Visitor<'_> for WireTextVisitor<MAX> {
            type Value = WireText<MAX>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "1..={MAX} safe wire-text bytes")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                WireText::new(value).map_err(E::custom)
            }

            fn visit_borrowed_str<E>(self, value: &'_ str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(value)
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                WireText::new(value).map_err(E::custom)
            }
        }

        deserializer.deserialize_str(WireTextVisitor::<MAX>)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ManifestHash(String);

impl ManifestHash {
    pub fn new(value: impl Into<String>) -> Result<Self, BoundedValueError> {
        let value = value.into();
        if value.len() != 64 {
            return Err(BoundedValueError::ManifestHashLength(value.len()));
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(BoundedValueError::ManifestHashCharacter);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for ManifestHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ManifestHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct AuthTicket(Vec<u8>);

impl AuthTicket {
    pub fn new(bytes: Vec<u8>) -> Result<Self, BoundedValueError> {
        if bytes.is_empty() || bytes.len() > AUTH_TICKET_MAX_BYTES {
            return Err(BoundedValueError::AuthTicketLength(bytes.len()));
        }
        Ok(Self(bytes))
    }

    /// The auth boundary is the only caller that should inspect ticket bytes.
    #[must_use]
    pub fn expose_for_validation(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for AuthTicket {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthTicket")
            .field("bytes", &"<redacted>")
            .field("length", &self.0.len())
            .finish()
    }
}

impl Serialize for AuthTicket {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AuthTicket {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(Vec::<u8>::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

const fn is_wire_text_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/' | b'+')
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BoundedValueError {
    #[error("wire text cannot be empty")]
    EmptyText,
    #[error("wire text exceeds {maximum} bytes: {actual}")]
    TextTooLong { maximum: usize, actual: usize },
    #[error("wire text contains an unsupported character")]
    InvalidTextCharacter,
    #[error("content manifest hash must contain exactly 64 lowercase hexadecimal bytes, got {0}")]
    ManifestHashLength(usize),
    #[error("content manifest hash must use lowercase hexadecimal characters")]
    ManifestHashCharacter,
    #[error("auth ticket length must be within 1..={AUTH_TICKET_MAX_BYTES}, got {0}")]
    AuthTicketLength(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_text_and_manifest_hash_are_strict() {
        assert_eq!(WireText::<8>::new("en-US").unwrap().as_str(), "en-US");
        assert_eq!(WireText::<8>::new(""), Err(BoundedValueError::EmptyText));
        assert_eq!(
            WireText::<4>::new("abcde"),
            Err(BoundedValueError::TextTooLong {
                maximum: 4,
                actual: 5
            })
        );
        assert_eq!(
            WireText::<16>::new("has space"),
            Err(BoundedValueError::InvalidTextCharacter)
        );
        assert!(ManifestHash::new("a".repeat(64)).is_ok());
        assert_eq!(
            ManifestHash::new("A".repeat(64)),
            Err(BoundedValueError::ManifestHashCharacter)
        );
    }

    #[test]
    fn auth_ticket_debug_is_redacted_and_length_is_bounded() {
        let ticket = AuthTicket::new(b"local-test-ticket".to_vec()).unwrap();
        let debug = format!("{ticket:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("local-test-ticket"));
        assert_eq!(
            AuthTicket::new(Vec::new()),
            Err(BoundedValueError::AuthTicketLength(0))
        );
    }
}
