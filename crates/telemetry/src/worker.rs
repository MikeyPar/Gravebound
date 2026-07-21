use std::{collections::BTreeSet, error::Error, future::Future};

use crate::{
    CommittedOutboxEventV1, RedactedTelemetryDocument, TelemetryConnectivity, TelemetryId,
    TelemetryIngestOutcome, TelemetryPipeline, TelemetryPipelineMode, TelemetryQueueError,
};

/// Durable source boundary for already-committed domain outbox rows.
///
/// Implementations must return only rows whose owning transaction committed and whose durable
/// publication marker is still unset. `acknowledge_published` is the only mutation permitted and
/// may advance only those exact source rows returned by a prior poll.
pub trait CommittedTelemetrySource {
    type Error: Error + Send + Sync + 'static;

    fn poll_unpublished(
        &mut self,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<CommittedOutboxEventV1>, Self::Error>> + Send;

    fn acknowledge_published(
        &mut self,
        accepted: &[TelemetryId],
    ) -> impl Future<Output = Result<Vec<TelemetryId>, Self::Error>> + Send;
}

/// External delivery boundary. Exporters must be idempotent by document `outbox_id`.
pub trait TelemetryExporter {
    type Error: Error + Send + Sync + 'static;

    fn export(
        &mut self,
        documents: &[RedactedTelemetryDocument],
    ) -> impl Future<Output = Result<TelemetryExportAcceptance, Self::Error>> + Send;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetryExportAcceptance {
    accepted: Vec<TelemetryId>,
}

impl TelemetryExportAcceptance {
    pub fn new(mut accepted: Vec<TelemetryId>) -> Result<Self, TelemetryWorkerError> {
        accepted.sort_unstable();
        if accepted.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(TelemetryWorkerError::InvalidExporterAcceptance);
        }
        Ok(Self { accepted })
    }

    #[must_use]
    pub fn accepted(&self) -> &[TelemetryId] {
        &self.accepted
    }
}

#[derive(Debug)]
pub struct TelemetryWorker {
    pipeline: TelemetryPipeline,
}

impl TelemetryWorker {
    /// Creates a disabled worker. Enabling export is an explicit runtime configuration action.
    pub fn new(capacity: usize) -> Result<Self, TelemetryQueueError> {
        Ok(Self {
            pipeline: TelemetryPipeline::new(
                TelemetryPipelineMode::Disabled,
                TelemetryConnectivity::Offline,
                capacity,
            )?,
        })
    }

    pub fn enabled(
        capacity: usize,
        connectivity: TelemetryConnectivity,
    ) -> Result<Self, TelemetryQueueError> {
        Ok(Self {
            pipeline: TelemetryPipeline::new(
                TelemetryPipelineMode::Enabled,
                connectivity,
                capacity,
            )?,
        })
    }

    pub fn set_connectivity(&mut self, connectivity: TelemetryConnectivity) {
        self.pipeline.set_connectivity(connectivity);
    }

    #[must_use]
    pub fn queued_len(&self) -> usize {
        self.pipeline.queued_len()
    }

    pub async fn run_once<Source, Exporter>(
        &mut self,
        source: &mut Source,
        exporter: &mut Exporter,
        limit: usize,
    ) -> Result<TelemetryWorkerOutcome, TelemetryWorkerError>
    where
        Source: CommittedTelemetrySource,
        Exporter: TelemetryExporter,
    {
        if self.pipeline.mode() == TelemetryPipelineMode::Disabled {
            return Ok(TelemetryWorkerOutcome::Disabled);
        }
        if limit == 0 {
            return Err(TelemetryWorkerError::InvalidWorkerLimit);
        }

        let poll_limit = limit.min(self.pipeline.remaining_capacity());
        let mut newly_queued = 0usize;
        if poll_limit > 0 {
            let records = source
                .poll_unpublished(poll_limit)
                .await
                .map_err(|error| TelemetryWorkerError::Source(Box::new(error)))?;
            if records.len() > poll_limit {
                return Err(TelemetryWorkerError::SourceExceededLimit);
            }
            for record in records {
                match self.pipeline.ingest_committed(record) {
                    TelemetryIngestOutcome::Queued => newly_queued += 1,
                    TelemetryIngestOutcome::Duplicate => {}
                    TelemetryIngestOutcome::Disabled | TelemetryIngestOutcome::Backpressured => {
                        return Err(TelemetryWorkerError::PipelineInvariant);
                    }
                }
            }
        }

        let documents = self.pipeline.prepare_redacted_batch(limit)?;
        if documents.is_empty() {
            return Ok(TelemetryWorkerOutcome::Idle { newly_queued });
        }
        let sent_ids = documents
            .iter()
            .map(|document| document.outbox_id)
            .collect::<BTreeSet<_>>();
        let acceptance = exporter
            .export(&documents)
            .await
            .map_err(|error| TelemetryWorkerError::Exporter(Box::new(error)))?;
        if acceptance
            .accepted()
            .iter()
            .any(|accepted| !sent_ids.contains(accepted))
        {
            return Err(TelemetryWorkerError::InvalidExporterAcceptance);
        }

        let published = source
            .acknowledge_published(acceptance.accepted())
            .await
            .map_err(|error| TelemetryWorkerError::Source(Box::new(error)))?;
        let accepted = acceptance
            .accepted()
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if published
            .iter()
            .any(|event_id| !accepted.contains(event_id))
            || published.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(TelemetryWorkerError::InvalidSourceAcknowledgement);
        }
        let removed = self.pipeline.acknowledge_delivered(&published);
        if removed != published.len() {
            return Err(TelemetryWorkerError::PipelineInvariant);
        }
        Ok(TelemetryWorkerOutcome::Exported {
            newly_queued,
            offered: documents.len(),
            published: published.len(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetryWorkerOutcome {
    Disabled,
    Idle {
        newly_queued: usize,
    },
    Exported {
        newly_queued: usize,
        offered: usize,
        published: usize,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum TelemetryWorkerError {
    #[error("telemetry worker limit must be nonzero")]
    InvalidWorkerLimit,
    #[error("committed telemetry source exceeded its requested bound")]
    SourceExceededLimit,
    #[error("exporter returned an invalid acceptance set")]
    InvalidExporterAcceptance,
    #[error("committed source returned an invalid acknowledgement set")]
    InvalidSourceAcknowledgement,
    #[error("telemetry pipeline invariant failed")]
    PipelineInvariant,
    #[error("committed telemetry source failed")]
    Source(#[source] Box<dyn Error + Send + Sync>),
    #[error("telemetry exporter failed")]
    Exporter(#[source] Box<dyn Error + Send + Sync>),
    #[error(transparent)]
    Queue(#[from] TelemetryQueueError),
}
