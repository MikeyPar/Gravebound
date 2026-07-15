//! Renderer-independent durable-death client authority for `GB-M03-06D`.
//!
//! Local health prediction may enter an awaiting state, but only authenticated durable-death
//! responses can reveal losses or recovery actions. One global coordinator owns sequence and
//! request-kind attribution because protocol error responses intentionally contain no query kind.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`DTH-001`, `DTH-020`,
//! `TECH-020`-`023`), `Gravebound_Content_Production_Spec_v1.md` (`CONT-HUB-001`,
//! `CONT-HUB-002`, `CONT-LOC-001`), and `Gravebound_Development_Roadmap_v1.md`
//! (`GB-M03-02`, `GB-M03-06`, `GB-M03-07`).

mod projection;
mod summary;

pub use projection::{
    DEATH_SUMMARY_SECTION_ORDER, DeathDamageEventPresentation, DeathFixedProjectionPresentation,
    DeathHeroPresentation, DeathLethalCausePresentation, DeathLocalizedValue,
    DeathLossPresentation, DeathNetworkPresentation, DeathStatusPresentation, DeathSummaryAction,
    DeathSummaryActionPresentation, DeathSummaryActionState, DeathSummaryActionsPresentation,
    DeathSummaryPresentation, DeathSummarySection, DeathTimelinePresentation,
    DeathViewProjectionError,
};
pub use summary::{TerminalDeathModel, TerminalDeathPhase};

use content_schema::CoreDeathViewCopyKind;
use protocol::{
    DEATH_VIEW_MAX_LOST_PROJECTIONS_PER_PAGE, DEATH_VIEW_SCHEMA_VERSION,
    DeathViewContentRevisionV1, DeathViewFrameV1, DeathViewRequestV1, DeathViewResultCodeV1,
    DeathViewResultV1, ManifestHash,
};
use sim_content::CoreDevelopmentDeathView;
use thiserror::Error;

use self::projection::{project_summary, project_summary_continuation, validate_latest};
use crate::core_world_transition::{
    CoreWorldTransitionModel, CoreWorldTransitionPhase, CoreWorldTransitionResolution,
};

pub const TERMINAL_SUMMARY_LOSS_PAGE_LIMIT: u16 = DEATH_VIEW_MAX_LOST_PROJECTIONS_PER_PAGE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalQueryIntent {
    Initial,
    Refresh,
    Continuation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingDeathViewQuery {
    Latest {
        character_id: [u8; 16],
        intent: TerminalQueryIntent,
    },
    Summary {
        death_id: [u8; 16],
        lost_start_ordinal: u16,
        lost_limit: u16,
        intent: TerminalQueryIntent,
    },
}

impl PendingDeathViewQuery {
    fn request(&self) -> DeathViewRequestV1 {
        match self {
            Self::Latest { .. } => DeathViewRequestV1::LatestCommitted,
            Self::Summary {
                death_id,
                lost_start_ordinal,
                lost_limit,
                ..
            } => DeathViewRequestV1::Summary {
                death_id: *death_id,
                lost_start_ordinal: *lost_start_ordinal,
                lost_limit: *lost_limit,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingDeathViewRequest {
    pub sequence: u32,
    pub query: PendingDeathViewQuery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathViewRetryDirective {
    Unavailable,
    RetryIdenticalQuery,
    RefreshLatest,
    Reconnect,
    RestartAfterUpdate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathViewFailure {
    pub code: DeathViewResultCodeV1,
    pub title: String,
    pub detail: String,
    pub retry: DeathViewRetryDirective,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathViewApplyDisposition {
    Applied,
    IgnoredDuplicate,
    IgnoredStale,
    IgnoredUnexpectedKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathViewApplyOutcome {
    pub disposition: DeathViewApplyDisposition,
    pub follow_up: Option<DeathViewFrameV1>,
}

impl DeathViewApplyOutcome {
    const fn ignored(disposition: DeathViewApplyDisposition) -> Self {
        Self {
            disposition,
            follow_up: None,
        }
    }
}

#[derive(Debug, Error)]
pub enum DeathViewClientError {
    #[error("compiled death-presentation authority is invalid")]
    InvalidPresentationAuthority,
    #[error("captured character identity is invalid")]
    InvalidCharacterIdentity,
    #[error("authoritative death identity does not match the captured character")]
    CharacterIdentityMismatch,
    #[error("a death-view query is already in flight")]
    QueryInFlight,
    #[error("death-view sequence space is exhausted")]
    SequenceExhausted,
    #[error("death-view response failed protocol validation")]
    InvalidResponse,
    #[error("terminal death state does not permit this operation")]
    InvalidTerminalPhase,
    #[error("no retryable death-view query is available")]
    NoRetryAvailable,
    #[error("no death-view response is currently pending")]
    NoResponsePending,
    #[error("the summary has no additional loss page")]
    NoAdditionalLossPage,
    #[error("latest committed-death anchor is missing")]
    MissingLatestAnchor,
    #[error("summary snapshot anchor is missing")]
    MissingSummaryAnchor,
    #[error(transparent)]
    Projection(#[from] DeathViewProjectionError),
}

/// Single sequence-space owner for one reliable death-view dispatcher.
///
/// The coordinator is intentionally not `Clone`: error results have no request-kind field, so a
/// transport must never have two independently copied owners issuing overlapping sequences.
#[derive(Debug)]
pub struct DeathViewClientModel {
    presentation: CoreDevelopmentDeathView,
    presentation_revision: DeathViewContentRevisionV1,
    next_sequence: u32,
    pending: Option<PendingDeathViewRequest>,
    last_accepted_result: Option<DeathViewResultV1>,
    terminal: TerminalDeathModel,
}

impl DeathViewClientModel {
    pub fn new(presentation: CoreDevelopmentDeathView) -> Result<Self, DeathViewClientError> {
        let hashes = presentation.hashes();
        let presentation_revision = DeathViewContentRevisionV1 {
            records_blake3: ManifestHash::new(hashes.records_blake3.clone())
                .map_err(|_| DeathViewClientError::InvalidPresentationAuthority)?,
            assets_blake3: ManifestHash::new(hashes.assets_blake3.clone())
                .map_err(|_| DeathViewClientError::InvalidPresentationAuthority)?,
            localization_blake3: ManifestHash::new(hashes.localization_blake3.clone())
                .map_err(|_| DeathViewClientError::InvalidPresentationAuthority)?,
        };
        Ok(Self {
            presentation,
            presentation_revision,
            next_sequence: 1,
            pending: None,
            last_accepted_result: None,
            terminal: TerminalDeathModel::default(),
        })
    }

    #[must_use]
    pub const fn terminal(&self) -> &TerminalDeathModel {
        &self.terminal
    }

    #[must_use]
    pub const fn pending(&self) -> Option<&PendingDeathViewRequest> {
        self.pending.as_ref()
    }

    #[must_use]
    pub const fn presentation_revision(&self) -> &DeathViewContentRevisionV1 {
        &self.presentation_revision
    }

    #[must_use]
    pub fn phase_copy(&self) -> Option<&str> {
        let id = match self.terminal.phase() {
            TerminalDeathPhase::PossibleDeathObserved
            | TerminalDeathPhase::AwaitingDurableAcknowledgement => "death.state.awaiting_commit",
            TerminalDeathPhase::LoadingLatest | TerminalDeathPhase::LoadingSummary => {
                "death.state.loading_summary"
            }
            _ => return None,
        };
        self.presentation
            .resolve_copy(CoreDeathViewCopyKind::State, id)
    }

    #[must_use]
    pub fn awaiting_detail_copy(&self) -> Option<&str> {
        matches!(
            self.terminal.phase(),
            TerminalDeathPhase::PossibleDeathObserved
                | TerminalDeathPhase::AwaitingDurableAcknowledgement
        )
        .then(|| {
            self.presentation.resolve_copy(
                CoreDeathViewCopyKind::State,
                "death.state.awaiting_commit_detail",
            )
        })
        .flatten()
    }

    /// Records local health zero without fabricating a durable death, loss list, or action.
    pub fn observe_local_health_zero(
        &mut self,
        character_id: [u8; 16],
    ) -> Result<(), DeathViewClientError> {
        if self.pending.is_some() {
            return Err(DeathViewClientError::QueryInFlight);
        }
        self.terminal.observe_possible_death(character_id)?;
        self.last_accepted_result = None;
        Ok(())
    }

    /// Starts the read barrier only after the authenticated session resolves `DeathFinal`.
    pub fn begin_committed_death_lookup(
        &mut self,
        character_id: [u8; 16],
    ) -> Result<DeathViewFrameV1, DeathViewClientError> {
        self.terminal.validate_initial_lookup(character_id)?;
        self.start_query(PendingDeathViewQuery::Latest {
            character_id,
            intent: TerminalQueryIntent::Initial,
        })
    }

    /// Atomically converts the committed world-transition terminal into the first durable read.
    /// Callers invoke this before yielding the transition system so a transport loss is always
    /// owned either by the world transition or by this coordinator's pending request.
    pub fn begin_world_transition_death_handoff(
        &mut self,
        transition: &CoreWorldTransitionModel,
    ) -> Result<DeathViewFrameV1, DeathViewClientError> {
        if transition.phase() != CoreWorldTransitionPhase::ResolvedToDeathSummary
            || transition.resolution() != CoreWorldTransitionResolution::DeathCommitted
        {
            return Err(DeathViewClientError::InvalidTerminalPhase);
        }
        self.begin_committed_death_lookup(transition.character_id())
    }

    pub fn refresh_terminal_summary(&mut self) -> Result<DeathViewFrameV1, DeathViewClientError> {
        let character_id = self.terminal.validate_refresh()?;
        self.start_query(PendingDeathViewQuery::Latest {
            character_id,
            intent: TerminalQueryIntent::Refresh,
        })
    }

    pub fn load_more_losses(&mut self) -> Result<DeathViewFrameV1, DeathViewClientError> {
        let query = self
            .terminal
            .continuation_query(TERMINAL_SUMMARY_LOSS_PAGE_LIMIT)?;
        self.start_query(query)
    }

    pub fn retry(&mut self) -> Result<DeathViewFrameV1, DeathViewClientError> {
        let query = self
            .terminal
            .retry_query()
            .ok_or(DeathViewClientError::NoRetryAvailable)?;
        self.start_query(query)
    }

    /// Converts an actual response timeout/loss into a retryable state without fabricating a
    /// server result. The replacement request receives a fresh sequence and identical parameters.
    pub fn handle_response_loss(&mut self) -> Result<(), DeathViewClientError> {
        let retry_query = self
            .pending
            .as_ref()
            .map(|pending| pending.query.clone())
            .ok_or(DeathViewClientError::NoResponsePending)?;
        let mut failure = self.failure(DeathViewResultCodeV1::ServiceUnavailable)?;
        failure.retry = DeathViewRetryDirective::RetryIdenticalQuery;
        self.pending = None;
        self.last_accepted_result = None;
        self.terminal.record_failure(
            TerminalDeathPhase::RecoverableError,
            failure,
            Some(retry_query),
        );
        Ok(())
    }

    pub fn handle_result(
        &mut self,
        result: &DeathViewResultV1,
    ) -> Result<DeathViewApplyOutcome, DeathViewClientError> {
        if result.validate().is_err() {
            if self
                .pending
                .as_ref()
                .is_some_and(|pending| pending.sequence == result_sequence(result))
            {
                self.record_local_failure(
                    result,
                    TerminalDeathPhase::FatalRecordError,
                    DeathViewResultCodeV1::CorruptStoredRecord,
                    DeathViewRetryDirective::Unavailable,
                )?;
            }
            return Err(DeathViewClientError::InvalidResponse);
        }
        if self.last_accepted_result.as_ref() == Some(result) {
            return Ok(DeathViewApplyOutcome::ignored(
                DeathViewApplyDisposition::IgnoredDuplicate,
            ));
        }
        let Some(pending) = self.pending.clone() else {
            return Ok(DeathViewApplyOutcome::ignored(
                DeathViewApplyDisposition::IgnoredStale,
            ));
        };
        if result_sequence(result) != pending.sequence {
            return Ok(DeathViewApplyOutcome::ignored(
                DeathViewApplyDisposition::IgnoredStale,
            ));
        }
        if !result_matches_query(result, &pending.query) {
            return Ok(DeathViewApplyOutcome::ignored(
                DeathViewApplyDisposition::IgnoredUnexpectedKind,
            ));
        }

        let outcome = match result {
            DeathViewResultV1::Latest { death, .. } => {
                self.apply_latest(result, &pending, death.as_ref())
            }
            DeathViewResultV1::Summary { summary, .. } => {
                self.apply_summary(result, &pending, summary)
            }
            DeathViewResultV1::Error { code, .. } => self.apply_error(result, &pending, *code),
            DeathViewResultV1::MemorialPage { .. } | DeathViewResultV1::TracePage { .. } => Ok(
                DeathViewApplyOutcome::ignored(DeathViewApplyDisposition::IgnoredUnexpectedKind),
            ),
        };
        match outcome {
            Err(DeathViewClientError::Projection(error)) => {
                self.record_projection_failure(result, &error)?;
                Err(DeathViewClientError::Projection(error))
            }
            other => other,
        }
    }

    fn start_query(
        &mut self,
        query: PendingDeathViewQuery,
    ) -> Result<DeathViewFrameV1, DeathViewClientError> {
        if self.pending.is_some() {
            return Err(DeathViewClientError::QueryInFlight);
        }
        let (frame, next_sequence) = self.prepare_frame(&query)?;
        self.terminal.mark_query_issued(&query);
        self.pending = Some(PendingDeathViewRequest {
            sequence: frame.sequence,
            query,
        });
        self.next_sequence = next_sequence;
        Ok(frame)
    }

    fn prepare_frame(
        &self,
        query: &PendingDeathViewQuery,
    ) -> Result<(DeathViewFrameV1, u32), DeathViewClientError> {
        let sequence = self.next_sequence.max(1);
        let next_sequence = sequence
            .checked_add(1)
            .ok_or(DeathViewClientError::SequenceExhausted)?;
        let frame = DeathViewFrameV1 {
            schema_version: DEATH_VIEW_SCHEMA_VERSION,
            sequence,
            content_revision: self.presentation_revision.clone(),
            request: query.request(),
        };
        frame
            .validate()
            .map_err(|_| DeathViewClientError::InvalidResponse)?;
        Ok((frame, next_sequence))
    }

    fn apply_latest(
        &mut self,
        result: &DeathViewResultV1,
        pending: &PendingDeathViewRequest,
        death: Option<&protocol::LatestCommittedDeathV1>,
    ) -> Result<DeathViewApplyOutcome, DeathViewClientError> {
        let PendingDeathViewQuery::Latest {
            character_id,
            intent,
        } = &pending.query
        else {
            return Ok(DeathViewApplyOutcome::ignored(
                DeathViewApplyDisposition::IgnoredUnexpectedKind,
            ));
        };
        let retry_query = PendingDeathViewQuery::Latest {
            character_id: *character_id,
            intent: *intent,
        };
        let Some(death) = death else {
            let refresh_failure = (*intent == TerminalQueryIntent::Refresh)
                .then(|| self.failure(DeathViewResultCodeV1::DeathNotFound))
                .transpose()?;
            self.terminal
                .accept_missing_latest(*intent, refresh_failure, retry_query);
            self.pending = None;
            self.last_accepted_result = Some(result.clone());
            return Ok(DeathViewApplyOutcome {
                disposition: DeathViewApplyDisposition::Applied,
                follow_up: None,
            });
        };
        validate_latest(
            death,
            *character_id,
            &self.presentation_revision,
            &self.presentation,
        )?;
        let summary_query = PendingDeathViewQuery::Summary {
            death_id: death.death_id,
            lost_start_ordinal: 0,
            lost_limit: TERMINAL_SUMMARY_LOSS_PAGE_LIMIT,
            intent: *intent,
        };
        let (follow_up, next_sequence) = self.prepare_frame(&summary_query)?;
        self.terminal.accept_latest(*intent, death.clone());
        self.terminal.mark_query_issued(&summary_query);
        self.pending = Some(PendingDeathViewRequest {
            sequence: follow_up.sequence,
            query: summary_query,
        });
        self.next_sequence = next_sequence;
        self.last_accepted_result = Some(result.clone());
        Ok(DeathViewApplyOutcome {
            disposition: DeathViewApplyDisposition::Applied,
            follow_up: Some(follow_up),
        })
    }

    fn apply_summary(
        &mut self,
        result: &DeathViewResultV1,
        pending: &PendingDeathViewRequest,
        summary: &protocol::DeathSummaryViewV1,
    ) -> Result<DeathViewApplyOutcome, DeathViewClientError> {
        let PendingDeathViewQuery::Summary { intent, .. } = &pending.query else {
            return Ok(DeathViewApplyOutcome::ignored(
                DeathViewApplyDisposition::IgnoredUnexpectedKind,
            ));
        };
        let latest = self.terminal.latest_for(*intent)?;
        if *intent == TerminalQueryIntent::Continuation {
            let continuation = project_summary_continuation(
                latest,
                self.terminal.summary_anchor()?,
                summary,
                self.terminal
                    .summary()
                    .ok_or(DeathViewClientError::MissingSummaryAnchor)?,
                &self.presentation_revision,
                &self.presentation,
            )?;
            self.terminal.accept_summary_continuation(continuation)?;
        } else {
            let presentation = project_summary(
                latest,
                summary,
                &self.presentation_revision,
                &self.presentation,
            )?;
            self.terminal
                .accept_summary(*intent, summary.clone(), presentation);
        }
        self.pending = None;
        self.last_accepted_result = Some(result.clone());
        Ok(DeathViewApplyOutcome {
            disposition: DeathViewApplyDisposition::Applied,
            follow_up: None,
        })
    }

    fn apply_error(
        &mut self,
        result: &DeathViewResultV1,
        pending: &PendingDeathViewRequest,
        code: DeathViewResultCodeV1,
    ) -> Result<DeathViewApplyOutcome, DeathViewClientError> {
        let (phase, directive, retry_query) = self.failure_policy(&pending.query, code);
        let mut failure = self.failure(code)?;
        failure.retry = directive;
        self.terminal.record_failure(phase, failure, retry_query);
        self.pending = None;
        self.last_accepted_result = Some(result.clone());
        Ok(DeathViewApplyOutcome {
            disposition: DeathViewApplyDisposition::Applied,
            follow_up: None,
        })
    }

    fn failure(
        &self,
        code: DeathViewResultCodeV1,
    ) -> Result<DeathViewFailure, DeathViewClientError> {
        let detail_id = error_copy_id(code);
        let title = self
            .presentation
            .resolve_copy(CoreDeathViewCopyKind::Error, "death.error.title")
            .ok_or_else(|| DeathViewProjectionError::MissingCopy {
                domain: "error",
                content_id: "death.error.title".to_owned(),
            })?;
        let detail = self
            .presentation
            .resolve_copy(CoreDeathViewCopyKind::Error, detail_id)
            .ok_or_else(|| DeathViewProjectionError::MissingCopy {
                domain: "error",
                content_id: detail_id.to_owned(),
            })?;
        Ok(DeathViewFailure {
            code,
            title: title.to_owned(),
            detail: detail.to_owned(),
            retry: DeathViewRetryDirective::Unavailable,
        })
    }

    fn record_projection_failure(
        &mut self,
        result: &DeathViewResultV1,
        error: &DeathViewProjectionError,
    ) -> Result<(), DeathViewClientError> {
        let (phase, code, directive) = match error {
            DeathViewProjectionError::AuthorityMismatch(_) => (
                TerminalDeathPhase::FatalContentError,
                DeathViewResultCodeV1::ContentMismatch,
                DeathViewRetryDirective::RestartAfterUpdate,
            ),
            DeathViewProjectionError::MissingCopy { .. }
            | DeathViewProjectionError::AnchorMismatch(_)
            | DeathViewProjectionError::InvalidLossContinuation(_) => (
                TerminalDeathPhase::FatalRecordError,
                DeathViewResultCodeV1::CorruptStoredRecord,
                DeathViewRetryDirective::Unavailable,
            ),
        };
        self.record_local_failure(result, phase, code, directive)
    }

    fn record_local_failure(
        &mut self,
        result: &DeathViewResultV1,
        phase: TerminalDeathPhase,
        code: DeathViewResultCodeV1,
        directive: DeathViewRetryDirective,
    ) -> Result<(), DeathViewClientError> {
        let failure = self.failure(code);
        self.pending = None;
        self.last_accepted_result = Some(result.clone());
        match failure {
            Ok(mut failure) => {
                failure.retry = directive;
                self.terminal.record_failure(phase, failure, None);
                Ok(())
            }
            Err(copy_error) => {
                self.terminal
                    .record_unrenderable_failure(TerminalDeathPhase::FatalContentError);
                Err(copy_error)
            }
        }
    }

    fn failure_policy(
        &self,
        pending: &PendingDeathViewQuery,
        code: DeathViewResultCodeV1,
    ) -> (
        TerminalDeathPhase,
        DeathViewRetryDirective,
        Option<PendingDeathViewQuery>,
    ) {
        match code {
            DeathViewResultCodeV1::Unauthenticated => (
                TerminalDeathPhase::RecoverableError,
                DeathViewRetryDirective::Reconnect,
                Some(pending.clone()),
            ),
            DeathViewResultCodeV1::FeatureDisabled => (
                TerminalDeathPhase::SurfaceDisabled,
                DeathViewRetryDirective::Unavailable,
                None,
            ),
            DeathViewResultCodeV1::DeathNotFound | DeathViewResultCodeV1::PageOutOfRange => (
                TerminalDeathPhase::RecoverableError,
                DeathViewRetryDirective::RefreshLatest,
                self.latest_refresh_query(),
            ),
            DeathViewResultCodeV1::DeathNotOwned | DeathViewResultCodeV1::CorruptStoredRecord => (
                TerminalDeathPhase::FatalRecordError,
                DeathViewRetryDirective::Unavailable,
                None,
            ),
            DeathViewResultCodeV1::ContentMismatch => (
                TerminalDeathPhase::FatalContentError,
                DeathViewRetryDirective::RestartAfterUpdate,
                None,
            ),
            DeathViewResultCodeV1::ServiceUnavailable => (
                TerminalDeathPhase::RecoverableError,
                DeathViewRetryDirective::RetryIdenticalQuery,
                Some(pending.clone()),
            ),
        }
    }

    fn latest_refresh_query(&self) -> Option<PendingDeathViewQuery> {
        self.terminal
            .captured_character_id()
            .map(|character_id| PendingDeathViewQuery::Latest {
                character_id,
                intent: if self.terminal.summary().is_some() {
                    TerminalQueryIntent::Refresh
                } else {
                    TerminalQueryIntent::Initial
                },
            })
    }
}

fn result_sequence(result: &DeathViewResultV1) -> u32 {
    match result {
        DeathViewResultV1::Latest {
            request_sequence, ..
        }
        | DeathViewResultV1::Summary {
            request_sequence, ..
        }
        | DeathViewResultV1::MemorialPage {
            request_sequence, ..
        }
        | DeathViewResultV1::TracePage {
            request_sequence, ..
        }
        | DeathViewResultV1::Error {
            request_sequence, ..
        } => *request_sequence,
    }
}

fn result_matches_query(result: &DeathViewResultV1, query: &PendingDeathViewQuery) -> bool {
    match (result, query) {
        (DeathViewResultV1::Latest { .. }, PendingDeathViewQuery::Latest { .. })
        | (DeathViewResultV1::Error { .. }, _) => true,
        (
            DeathViewResultV1::Summary {
                requested_lost_limit,
                summary,
                ..
            },
            PendingDeathViewQuery::Summary {
                death_id,
                lost_start_ordinal,
                lost_limit,
                ..
            },
        ) => {
            requested_lost_limit == lost_limit
                && summary.death_id == *death_id
                && summary.lost_start_ordinal == *lost_start_ordinal
        }
        _ => false,
    }
}

const fn error_copy_id(code: DeathViewResultCodeV1) -> &'static str {
    match code {
        DeathViewResultCodeV1::Unauthenticated => "death.error.unauthenticated",
        DeathViewResultCodeV1::FeatureDisabled => "death.error.feature_disabled",
        DeathViewResultCodeV1::DeathNotFound => "death.error.death_not_found",
        DeathViewResultCodeV1::DeathNotOwned => "death.error.death_not_owned",
        DeathViewResultCodeV1::PageOutOfRange => "death.error.page_out_of_range",
        DeathViewResultCodeV1::ContentMismatch => "death.error.content_mismatch",
        DeathViewResultCodeV1::CorruptStoredRecord => "death.error.corrupt_record",
        DeathViewResultCodeV1::ServiceUnavailable => "death.error.service_unavailable",
    }
}

#[cfg(test)]
mod tests;
