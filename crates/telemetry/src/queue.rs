use std::collections::{BTreeSet, VecDeque};

use thiserror::Error;

use crate::{CommittedOutboxEventV1, TelemetryId};

pub const MAX_OFFLINE_QUEUE_CAPACITY: usize = 4_096;
pub const MAX_TELEMETRY_EXPORT_BATCH: usize = 256;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TelemetryPipelineMode {
    #[default]
    Disabled,
    Enabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetryConnectivity {
    Offline,
    Online,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetryIngestOutcome {
    Queued,
    Duplicate,
    Disabled,
    Backpressured,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactedTelemetryDocument {
    pub outbox_id: TelemetryId,
    pub commit_sequence: u64,
    pub committed_at_utc_millis: u64,
    pub json: String,
}

#[derive(Debug)]
pub struct TelemetryPipeline {
    mode: TelemetryPipelineMode,
    connectivity: TelemetryConnectivity,
    capacity: usize,
    queued_ids: BTreeSet<TelemetryId>,
    queue: VecDeque<CommittedOutboxEventV1>,
}

impl TelemetryPipeline {
    pub fn new(
        mode: TelemetryPipelineMode,
        connectivity: TelemetryConnectivity,
        capacity: usize,
    ) -> Result<Self, TelemetryQueueError> {
        if capacity == 0 || capacity > MAX_OFFLINE_QUEUE_CAPACITY {
            return Err(TelemetryQueueError::InvalidCapacity);
        }
        Ok(Self {
            mode,
            connectivity,
            capacity,
            queued_ids: BTreeSet::new(),
            queue: VecDeque::with_capacity(capacity),
        })
    }

    pub fn set_connectivity(&mut self, connectivity: TelemetryConnectivity) {
        self.connectivity = connectivity;
    }

    #[must_use]
    pub const fn mode(&self) -> TelemetryPipelineMode {
        self.mode
    }

    #[must_use]
    pub fn remaining_capacity(&self) -> usize {
        self.capacity.saturating_sub(self.queue.len())
    }

    pub fn ingest_committed(&mut self, event: CommittedOutboxEventV1) -> TelemetryIngestOutcome {
        if self.mode == TelemetryPipelineMode::Disabled {
            return TelemetryIngestOutcome::Disabled;
        }
        if self.queued_ids.contains(&event.outbox_id()) {
            return TelemetryIngestOutcome::Duplicate;
        }
        if self.queue.len() == self.capacity {
            return TelemetryIngestOutcome::Backpressured;
        }
        self.queued_ids.insert(event.outbox_id());
        self.queue.push_back(event);
        TelemetryIngestOutcome::Queued
    }

    #[must_use]
    pub fn queued_len(&self) -> usize {
        self.queue.len()
    }

    pub fn prepare_redacted_batch(
        &self,
        limit: usize,
    ) -> Result<Vec<RedactedTelemetryDocument>, TelemetryQueueError> {
        if limit == 0 || limit > MAX_TELEMETRY_EXPORT_BATCH {
            return Err(TelemetryQueueError::InvalidBatchSize);
        }
        if self.mode == TelemetryPipelineMode::Disabled
            || self.connectivity == TelemetryConnectivity::Offline
        {
            return Ok(Vec::new());
        }
        self.queue
            .iter()
            .take(limit)
            .map(|record| {
                Ok(RedactedTelemetryDocument {
                    outbox_id: record.outbox_id(),
                    commit_sequence: record.commit_sequence(),
                    committed_at_utc_millis: record.committed_at_utc_millis(),
                    json: record.envelope().to_redacted_json()?,
                })
            })
            .collect::<Result<Vec<_>, serde_json::Error>>()
            .map_err(TelemetryQueueError::Serialization)
    }

    /// Acknowledges only documents the exporter durably accepted. Failed or absent IDs remain in
    /// memory and their source outbox rows must remain unpublished for later polling.
    pub fn acknowledge_delivered(&mut self, delivered: &[TelemetryId]) -> usize {
        let delivered = delivered.iter().copied().collect::<BTreeSet<_>>();
        let before = self.queue.len();
        let queued_ids = &mut self.queued_ids;
        self.queue.retain(|record| {
            if delivered.contains(&record.outbox_id()) {
                queued_ids.remove(&record.outbox_id());
                false
            } else {
                true
            }
        });
        before.saturating_sub(self.queue.len())
    }
}

#[derive(Debug, Error)]
pub enum TelemetryQueueError {
    #[error("telemetry queue capacity is outside the supported bound")]
    InvalidCapacity,
    #[error("telemetry export batch size is outside the supported bound")]
    InvalidBatchSize,
    #[error("privacy-safe telemetry serialization failed")]
    Serialization(#[source] serde_json::Error),
}
