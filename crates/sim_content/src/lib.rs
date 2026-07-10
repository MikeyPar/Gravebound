//! Immutable validated content loading for simulation consumers.

/// Reports the schema version this loader accepts.
#[must_use]
pub const fn supported_schema_version() -> u32 {
    content_schema::SCHEMA_VERSION
}
