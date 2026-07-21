use thiserror::Error;

use crate::{TelemetryId, VersionedTelemetryEnvelopeV1};

/// Post-commit projection accepted by the telemetry pipeline.
///
/// Persistence adapters construct this value only after the transaction that inserted the outbox
/// row has committed. The telemetry pipeline deliberately has no API that accepts a bare event
/// envelope or live gameplay state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedOutboxEventV1 {
    outbox_id: TelemetryId,
    commit_sequence: u64,
    committed_at_utc_millis: u64,
    envelope: VersionedTelemetryEnvelopeV1,
}

impl CommittedOutboxEventV1 {
    pub fn from_committed_row(
        outbox_id: TelemetryId,
        commit_sequence: u64,
        committed_at_utc_millis: u64,
        envelope: VersionedTelemetryEnvelopeV1,
    ) -> Result<Self, CommittedOutboxError> {
        if commit_sequence == 0 {
            return Err(CommittedOutboxError::ZeroCommitSequence);
        }
        if committed_at_utc_millis < envelope.occurred_at_utc_millis() {
            return Err(CommittedOutboxError::CommitPredatesEvent);
        }
        Ok(Self {
            outbox_id,
            commit_sequence,
            committed_at_utc_millis,
            envelope,
        })
    }

    #[must_use]
    pub const fn outbox_id(&self) -> TelemetryId {
        self.outbox_id
    }

    #[must_use]
    pub const fn commit_sequence(&self) -> u64 {
        self.commit_sequence
    }

    #[must_use]
    pub const fn committed_at_utc_millis(&self) -> u64 {
        self.committed_at_utc_millis
    }

    #[must_use]
    pub const fn envelope(&self) -> &VersionedTelemetryEnvelopeV1 {
        &self.envelope
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CommittedOutboxError {
    #[error("committed outbox sequence must be nonzero")]
    ZeroCommitSequence,
    #[error("committed outbox timestamp predates its event")]
    CommitPredatesEvent,
}
