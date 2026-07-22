use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const NATIVE_CRASH_SCHEMA_VERSION: u16 = 1;
pub const NATIVE_CRASH_FEATURE_FLAG: &str = "telemetry.native-crash.v1";
pub const NATIVE_CRASH_ID_BYTES: usize = 16;
pub const NATIVE_CRASH_SIGNATURE_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeCrashKindV1 {
    Panic,
    AccessViolation,
    OutOfMemory,
    Watchdog,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeCrashReportFrameV1 {
    pub schema_version: u16,
    pub crash_id: [u8; NATIVE_CRASH_ID_BYTES],
    pub kind: NativeCrashKindV1,
    pub signature: [u8; NATIVE_CRASH_SIGNATURE_BYTES],
    pub uptime_millis: u64,
    pub occurred_at_utc_millis: u64,
}

impl NativeCrashReportFrameV1 {
    pub fn validate(&self) -> Result<(), NativeCrashValidationError> {
        if self.schema_version != NATIVE_CRASH_SCHEMA_VERSION {
            return Err(NativeCrashValidationError::SchemaVersion);
        }
        if self.crash_id.iter().all(|byte| *byte == 0) {
            return Err(NativeCrashValidationError::CrashId);
        }
        if self.signature.iter().all(|byte| *byte == 0) {
            return Err(NativeCrashValidationError::Signature);
        }
        if self.occurred_at_utc_millis == 0 {
            return Err(NativeCrashValidationError::OccurredAt);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeCrashReportResultCodeV1 {
    Accepted,
    Disabled,
    Unavailable,
    IdempotencyConflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeCrashReportResultV1 {
    pub schema_version: u16,
    pub crash_id: [u8; NATIVE_CRASH_ID_BYTES],
    pub code: NativeCrashReportResultCodeV1,
}

impl NativeCrashReportResultV1 {
    pub fn validate(&self) -> Result<(), NativeCrashValidationError> {
        if self.schema_version != NATIVE_CRASH_SCHEMA_VERSION {
            return Err(NativeCrashValidationError::SchemaVersion);
        }
        if self.crash_id.iter().all(|byte| *byte == 0) {
            return Err(NativeCrashValidationError::CrashId);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NativeCrashValidationError {
    #[error("native crash schema version is unsupported")]
    SchemaVersion,
    #[error("native crash ID must be nonzero")]
    CrashId,
    #[error("native crash signature must be nonzero")]
    Signature,
    #[error("native crash occurrence time must be nonzero")]
    OccurredAt,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_is_fixed_typed_and_rejects_zero_sensitive_surrogates() {
        let report = NativeCrashReportFrameV1 {
            schema_version: NATIVE_CRASH_SCHEMA_VERSION,
            crash_id: [1; NATIVE_CRASH_ID_BYTES],
            kind: NativeCrashKindV1::Panic,
            signature: [2; NATIVE_CRASH_SIGNATURE_BYTES],
            uptime_millis: 31,
            occurred_at_utc_millis: 44,
        };
        assert_eq!(report.validate(), Ok(()));
        let mut invalid = report;
        invalid.signature = [0; NATIVE_CRASH_SIGNATURE_BYTES];
        assert_eq!(
            invalid.validate(),
            Err(NativeCrashValidationError::Signature)
        );
    }
}
