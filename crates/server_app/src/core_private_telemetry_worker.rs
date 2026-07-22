//! Disabled-by-default telemetry-export worker ownership for the production M03 root.
//!
//! The three authorities are `Gravebound_Production_GDD_v1_Canonical.md` (`TECH-005`,
//! `TECH-123`, and `TEL-001`-`005`), `Gravebound_Content_Production_Spec_v1.md` (the exact
//! Core identities and committed lifecycle outcomes), and
//! `Gravebound_Development_Roadmap_v1.md` (`ADR-005` and `GB-M03-09`). ADR-039 additionally
//! forbids remote export before its destination/privacy review is recorded.
//!
//! This owner deliberately has no committed source, exporter, destination, pseudonymization
//! secret, or background task. Keeping the disabled [`TelemetryWorker`] in the production root
//! makes ownership and shutdown observable without creating a gameplay dependency. A later
//! approved enablement can add the committed `PostgreSQL` adapters and exporter behind this owner;
//! it must not change the private-life server's gameplay orchestration.

use telemetry::{MAX_OFFLINE_QUEUE_CAPACITY, TelemetryQueueError, TelemetryWorker};

/// The ADR-039 maximum is retained as the future enabled-mode bound. Disabled mode never fills it.
pub const CORE_PRIVATE_TELEMETRY_QUEUE_CAPACITY_V1: usize = MAX_OFFLINE_QUEUE_CAPACITY;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePrivateTelemetryWorkerModeV1 {
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePrivateTelemetryWorkerAttachmentsV1 {
    /// No source, exporter, exporter destination, or pseudonymization secret is attached.
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePrivateTelemetryWorkerLifecycleV1 {
    Bound,
    ShutdownStarted,
    ShutdownComplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateTelemetryWorkerStatusV1 {
    pub mode: CorePrivateTelemetryWorkerModeV1,
    pub attachments: CorePrivateTelemetryWorkerAttachmentsV1,
    pub queued_events: usize,
    pub spawned_tasks: usize,
    pub lifecycle: CorePrivateTelemetryWorkerLifecycleV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateTelemetryWorkerReportV1 {
    pub mode: CorePrivateTelemetryWorkerModeV1,
    pub attachments: CorePrivateTelemetryWorkerAttachmentsV1,
    pub source_poll_attempts: u64,
    pub source_acknowledgement_attempts: u64,
    pub export_attempts: u64,
    pub queued_events_on_shutdown: usize,
    pub remaining_tasks: usize,
    pub lifecycle: CorePrivateTelemetryWorkerLifecycleV1,
    pub zero_residue: bool,
}

/// Production-root lifecycle owner for the disabled telemetry export worker.
///
/// Source and exporter handles are structurally absent while disabled, so this owner cannot poll,
/// acknowledge, serialize, or export. The logical-session coordinator remains a separate,
/// explicitly configured committed-domain collector and is not an export path.
#[derive(Debug)]
pub(crate) struct CorePrivateTelemetryWorkerRuntime {
    worker: Option<TelemetryWorker>,
    shutdown_started: bool,
}

impl CorePrivateTelemetryWorkerRuntime {
    pub(crate) fn bind_disabled() -> Result<Self, TelemetryQueueError> {
        Ok(Self {
            worker: Some(TelemetryWorker::new(
                CORE_PRIVATE_TELEMETRY_QUEUE_CAPACITY_V1,
            )?),
            shutdown_started: false,
        })
    }

    pub(crate) fn status(&self) -> CorePrivateTelemetryWorkerStatusV1 {
        CorePrivateTelemetryWorkerStatusV1 {
            mode: CorePrivateTelemetryWorkerModeV1::Disabled,
            attachments: CorePrivateTelemetryWorkerAttachmentsV1::None,
            queued_events: self.worker.as_ref().map_or(0, TelemetryWorker::queued_len),
            spawned_tasks: 0,
            lifecycle: if self.shutdown_started {
                CorePrivateTelemetryWorkerLifecycleV1::ShutdownStarted
            } else {
                CorePrivateTelemetryWorkerLifecycleV1::Bound
            },
        }
    }

    pub(crate) fn begin_shutdown(&mut self) {
        self.shutdown_started = true;
    }

    pub(crate) fn finish_shutdown(&mut self) -> CorePrivateTelemetryWorkerReportV1 {
        self.begin_shutdown();
        let queued_events_on_shutdown = self.worker.take().map_or(0, |worker| worker.queued_len());
        let zero_residue = queued_events_on_shutdown == 0 && self.worker.is_none();
        CorePrivateTelemetryWorkerReportV1 {
            mode: CorePrivateTelemetryWorkerModeV1::Disabled,
            attachments: CorePrivateTelemetryWorkerAttachmentsV1::None,
            source_poll_attempts: 0,
            source_acknowledgement_attempts: 0,
            export_attempts: 0,
            queued_events_on_shutdown,
            remaining_tasks: 0,
            lifecycle: CorePrivateTelemetryWorkerLifecycleV1::ShutdownComplete,
            zero_residue,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, sync::Arc};

    use telemetry::{
        CommittedOutboxEventV1, CommittedTelemetrySource, RedactedTelemetryDocument,
        TelemetryExportAcceptance, TelemetryExporter, TelemetryId, TelemetryWorkerOutcome,
    };
    use tokio::sync::Mutex;

    use super::*;

    #[derive(Debug, Default, PartialEq, Eq)]
    struct ProbeCounts {
        polls: u64,
        acknowledgements: u64,
        exports: u64,
    }

    struct ProbeSource(Arc<Mutex<ProbeCounts>>);

    impl CommittedTelemetrySource for ProbeSource {
        type Error = Infallible;

        async fn poll_unpublished(
            &mut self,
            _limit: usize,
        ) -> Result<Vec<CommittedOutboxEventV1>, Self::Error> {
            self.0.lock().await.polls += 1;
            Ok(Vec::new())
        }

        async fn acknowledge_published(
            &mut self,
            _accepted: &[TelemetryId],
        ) -> Result<Vec<TelemetryId>, Self::Error> {
            self.0.lock().await.acknowledgements += 1;
            Ok(Vec::new())
        }
    }

    struct ProbeExporter(Arc<Mutex<ProbeCounts>>);

    impl TelemetryExporter for ProbeExporter {
        type Error = Infallible;

        async fn export(
            &mut self,
            _documents: &[RedactedTelemetryDocument],
        ) -> Result<TelemetryExportAcceptance, Self::Error> {
            self.0.lock().await.exports += 1;
            Ok(TelemetryExportAcceptance::new(Vec::new()).unwrap())
        }
    }

    #[tokio::test]
    async fn disabled_root_worker_cannot_touch_source_or_exporter() {
        let counts = Arc::new(Mutex::new(ProbeCounts::default()));
        let mut source = ProbeSource(Arc::clone(&counts));
        let mut exporter = ProbeExporter(Arc::clone(&counts));
        let mut runtime = CorePrivateTelemetryWorkerRuntime::bind_disabled().unwrap();

        let outcome = runtime
            .worker
            .as_mut()
            .unwrap()
            .run_once(&mut source, &mut exporter, 1)
            .await
            .unwrap();

        assert_eq!(outcome, TelemetryWorkerOutcome::Disabled);
        assert_eq!(*counts.lock().await, ProbeCounts::default());
        assert_eq!(
            runtime.status(),
            CorePrivateTelemetryWorkerStatusV1 {
                mode: CorePrivateTelemetryWorkerModeV1::Disabled,
                attachments: CorePrivateTelemetryWorkerAttachmentsV1::None,
                queued_events: 0,
                spawned_tasks: 0,
                lifecycle: CorePrivateTelemetryWorkerLifecycleV1::Bound,
            }
        );
    }

    #[test]
    fn disabled_root_worker_reports_clean_explicit_shutdown() {
        let mut runtime = CorePrivateTelemetryWorkerRuntime::bind_disabled().unwrap();
        runtime.begin_shutdown();
        assert_eq!(
            runtime.status().lifecycle,
            CorePrivateTelemetryWorkerLifecycleV1::ShutdownStarted
        );

        assert_eq!(
            runtime.finish_shutdown(),
            CorePrivateTelemetryWorkerReportV1 {
                mode: CorePrivateTelemetryWorkerModeV1::Disabled,
                attachments: CorePrivateTelemetryWorkerAttachmentsV1::None,
                source_poll_attempts: 0,
                source_acknowledgement_attempts: 0,
                export_attempts: 0,
                queued_events_on_shutdown: 0,
                remaining_tasks: 0,
                lifecycle: CorePrivateTelemetryWorkerLifecycleV1::ShutdownComplete,
                zero_residue: true,
            }
        );
    }
}
