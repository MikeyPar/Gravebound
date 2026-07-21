//! Privacy-safe telemetry contracts for the Gravebound product pipeline.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`TECH-123`, `TEL-001`-
//! `005`), `Gravebound_Content_Production_Spec_v1.md` (the exact Core content IDs and
//! lifecycle boundaries), and `Gravebound_Development_Roadmap_v1.md` (`ADR-005` and
//! `GB-M03-09`).
//!
//! The ingestion boundary accepts only [`CommittedOutboxEventV1`]. Event envelopes do not
//! contain raw account IDs, IP addresses, socket addresses, auth material, email addresses,
//! platform identities, free-form crash messages, or stack traces. Exportable JSON can be
//! produced only through the redacted document path.

mod event;
mod identifier;
mod outbox;
mod queue;
mod worker;

pub use event::{
    CrashEventV1, CrashKindV1, CrashSourceV1, DamageTypeV1, DeathCauseV1, DeathEventV1,
    ExtractionEventV1, LootActionV1, LootEventV1, NetworkHealthV1, OnboardingEventV1,
    RecallEventV1, RecallStateV1, RecallTriggerV1, SessionEndReasonV1, SessionEventV1,
    SuccessorEventV1, TelemetryContextV1, TelemetryEnvironmentV1, TelemetryEventError,
    TelemetryEventV1, TelemetryPlatformV1, VersionedTelemetryEnvelopeV1,
};
pub use identifier::{
    PseudonymousAccountId, StableTelemetryId, TelemetryId, TelemetryIdentifierError,
};
pub use outbox::{CommittedOutboxError, CommittedOutboxEventV1};
pub use queue::{
    MAX_OFFLINE_QUEUE_CAPACITY, MAX_TELEMETRY_EXPORT_BATCH, RedactedTelemetryDocument,
    TelemetryConnectivity, TelemetryIngestOutcome, TelemetryPipeline, TelemetryPipelineMode,
    TelemetryQueueError,
};
pub use worker::{
    CommittedTelemetrySource, TelemetryExportAcceptance, TelemetryExporter, TelemetryWorker,
    TelemetryWorkerError, TelemetryWorkerOutcome,
};
