//! Pure terminal-death state retained behind durable acknowledgement.

use protocol::{DeathSummaryViewV1, LatestCommittedDeathV1};

use super::projection::DeathSummaryLossContinuation;
use super::{
    DeathSummaryAction, DeathSummaryActionState, DeathSummaryPresentation, DeathViewClientError,
    DeathViewFailure, PendingDeathViewQuery, TerminalQueryIntent,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalDeathPhase {
    Inactive,
    PossibleDeathObserved,
    AwaitingDurableAcknowledgement,
    LoadingLatest,
    LoadingSummary,
    SummaryReady,
    RecoverableError,
    SurfaceDisabled,
    FatalContentError,
    FatalRecordError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalDeathModel {
    phase: TerminalDeathPhase,
    captured_character_id: Option<[u8; 16]>,
    latest: Option<LatestCommittedDeathV1>,
    refresh_latest: Option<LatestCommittedDeathV1>,
    summary_anchor: Option<DeathSummaryViewV1>,
    summary: Option<DeathSummaryPresentation>,
    failure: Option<DeathViewFailure>,
    retry_query: Option<PendingDeathViewQuery>,
}

impl Default for TerminalDeathModel {
    fn default() -> Self {
        Self {
            phase: TerminalDeathPhase::Inactive,
            captured_character_id: None,
            latest: None,
            refresh_latest: None,
            summary_anchor: None,
            summary: None,
            failure: None,
            retry_query: None,
        }
    }
}

impl TerminalDeathModel {
    #[must_use]
    pub const fn phase(&self) -> TerminalDeathPhase {
        self.phase
    }

    #[must_use]
    pub const fn captured_character_id(&self) -> Option<[u8; 16]> {
        self.captured_character_id
    }

    #[must_use]
    pub const fn latest(&self) -> Option<&LatestCommittedDeathV1> {
        self.latest.as_ref()
    }

    #[must_use]
    pub const fn summary(&self) -> Option<&DeathSummaryPresentation> {
        if matches!(self.phase, TerminalDeathPhase::SummaryReady) {
            self.summary.as_ref()
        } else {
            None
        }
    }

    #[cfg(test)]
    pub(crate) const fn retained_summary(&self) -> Option<&DeathSummaryPresentation> {
        self.summary.as_ref()
    }

    #[must_use]
    pub const fn failure(&self) -> Option<&DeathViewFailure> {
        self.failure.as_ref()
    }

    #[must_use]
    pub const fn action_state(&self, action: DeathSummaryAction) -> DeathSummaryActionState {
        let ready =
            matches!(self.phase, TerminalDeathPhase::SummaryReady) && self.summary.is_some();
        let enabled = match action {
            DeathSummaryAction::Retry => self.retry_query.is_some(),
            DeathSummaryAction::InspectTrace
            | DeathSummaryAction::Memorial
            | DeathSummaryAction::CharacterSelect => ready,
            DeathSummaryAction::CreateSuccessor => false,
        };
        if enabled {
            DeathSummaryActionState::Enabled
        } else {
            DeathSummaryActionState::Disabled
        }
    }

    pub(crate) fn observe_possible_death(
        &mut self,
        character_id: [u8; 16],
    ) -> Result<(), DeathViewClientError> {
        if character_id == [0; 16] {
            return Err(DeathViewClientError::InvalidCharacterIdentity);
        }
        match self.phase {
            TerminalDeathPhase::Inactive => {
                *self = Self {
                    phase: TerminalDeathPhase::PossibleDeathObserved,
                    captured_character_id: Some(character_id),
                    ..Self::default()
                };
                Ok(())
            }
            TerminalDeathPhase::PossibleDeathObserved
            | TerminalDeathPhase::AwaitingDurableAcknowledgement => {
                if self.captured_character_id == Some(character_id) {
                    Ok(())
                } else {
                    Err(DeathViewClientError::CharacterIdentityMismatch)
                }
            }
            _ => Err(DeathViewClientError::InvalidTerminalPhase),
        }
    }

    pub(crate) fn validate_initial_lookup(
        &self,
        character_id: [u8; 16],
    ) -> Result<(), DeathViewClientError> {
        if character_id == [0; 16] {
            return Err(DeathViewClientError::InvalidCharacterIdentity);
        }
        if !matches!(
            self.phase,
            TerminalDeathPhase::Inactive | TerminalDeathPhase::PossibleDeathObserved
        ) {
            return Err(DeathViewClientError::InvalidTerminalPhase);
        }
        if self
            .captured_character_id
            .is_some_and(|captured| captured != character_id)
        {
            return Err(DeathViewClientError::CharacterIdentityMismatch);
        }
        if self.summary.is_some() {
            return Err(DeathViewClientError::InvalidTerminalPhase);
        }
        Ok(())
    }

    pub(crate) fn validate_refresh(&self) -> Result<[u8; 16], DeathViewClientError> {
        if self.phase != TerminalDeathPhase::SummaryReady || self.summary.is_none() {
            return Err(DeathViewClientError::InvalidTerminalPhase);
        }
        self.captured_character_id
            .ok_or(DeathViewClientError::InvalidCharacterIdentity)
    }

    pub(crate) fn mark_query_issued(&mut self, query: &PendingDeathViewQuery) {
        self.failure = None;
        self.retry_query = None;
        match query {
            PendingDeathViewQuery::Latest {
                character_id,
                intent,
            } => {
                self.captured_character_id.get_or_insert(*character_id);
                if *intent == TerminalQueryIntent::Initial {
                    self.phase = TerminalDeathPhase::LoadingLatest;
                    self.latest = None;
                    self.summary_anchor = None;
                    self.summary = None;
                } else {
                    self.refresh_latest = None;
                }
            }
            PendingDeathViewQuery::Summary { intent, .. } => {
                if *intent == TerminalQueryIntent::Initial {
                    self.phase = TerminalDeathPhase::LoadingSummary;
                }
            }
        }
    }

    pub(crate) fn accept_latest(
        &mut self,
        intent: TerminalQueryIntent,
        latest: LatestCommittedDeathV1,
    ) {
        match intent {
            TerminalQueryIntent::Initial => {
                self.latest = Some(latest);
                self.phase = TerminalDeathPhase::LoadingSummary;
            }
            TerminalQueryIntent::Refresh => self.refresh_latest = Some(latest),
            TerminalQueryIntent::Continuation => unreachable!("continuations never query latest"),
        }
        self.failure = None;
        self.retry_query = None;
    }

    pub(crate) fn accept_missing_latest(
        &mut self,
        intent: TerminalQueryIntent,
        refresh_failure: Option<DeathViewFailure>,
        retry_query: PendingDeathViewQuery,
    ) {
        match intent {
            TerminalQueryIntent::Initial => {
                self.latest = None;
                self.summary_anchor = None;
                self.summary = None;
                self.phase = TerminalDeathPhase::AwaitingDurableAcknowledgement;
                self.failure = None;
            }
            TerminalQueryIntent::Refresh => {
                self.refresh_latest = None;
                self.phase = TerminalDeathPhase::SummaryReady;
                self.failure = refresh_failure;
            }
            TerminalQueryIntent::Continuation => unreachable!("continuations never query latest"),
        }
        self.retry_query = Some(retry_query);
    }

    pub(crate) fn latest_for(
        &self,
        intent: TerminalQueryIntent,
    ) -> Result<&LatestCommittedDeathV1, DeathViewClientError> {
        match intent {
            TerminalQueryIntent::Initial | TerminalQueryIntent::Continuation => self
                .latest
                .as_ref()
                .ok_or(DeathViewClientError::MissingLatestAnchor),
            TerminalQueryIntent::Refresh => self
                .refresh_latest
                .as_ref()
                .ok_or(DeathViewClientError::MissingLatestAnchor),
        }
    }

    pub(crate) fn summary_anchor(&self) -> Result<&DeathSummaryViewV1, DeathViewClientError> {
        self.summary_anchor
            .as_ref()
            .ok_or(DeathViewClientError::MissingSummaryAnchor)
    }

    pub(crate) fn accept_summary(
        &mut self,
        intent: TerminalQueryIntent,
        summary_anchor: DeathSummaryViewV1,
        presentation: DeathSummaryPresentation,
    ) {
        match intent {
            TerminalQueryIntent::Initial => {
                self.summary_anchor = Some(summary_anchor);
                self.summary = Some(presentation);
            }
            TerminalQueryIntent::Refresh => {
                self.latest = self.refresh_latest.take();
                self.summary_anchor = Some(summary_anchor);
                self.summary = Some(presentation);
            }
            TerminalQueryIntent::Continuation => unreachable!("continuations append a loss page"),
        }
        self.phase = TerminalDeathPhase::SummaryReady;
        self.failure = None;
        self.retry_query = None;
    }

    pub(crate) fn accept_summary_continuation(
        &mut self,
        continuation: DeathSummaryLossContinuation,
    ) -> Result<(), DeathViewClientError> {
        let summary = self
            .summary
            .as_mut()
            .ok_or(DeathViewClientError::MissingSummaryAnchor)?;
        summary.lost.extend(continuation.additions);
        summary.next_lost_ordinal = continuation.next_lost_ordinal;
        self.phase = TerminalDeathPhase::SummaryReady;
        self.failure = None;
        self.retry_query = None;
        Ok(())
    }

    pub(crate) fn record_failure(
        &mut self,
        phase: TerminalDeathPhase,
        failure: DeathViewFailure,
        retry_query: Option<PendingDeathViewQuery>,
    ) {
        self.refresh_latest = None;
        self.phase = if self.summary.is_some() && phase == TerminalDeathPhase::RecoverableError {
            TerminalDeathPhase::SummaryReady
        } else {
            phase
        };
        self.failure = Some(failure);
        self.retry_query = retry_query;
    }

    pub(crate) fn record_unrenderable_failure(&mut self, phase: TerminalDeathPhase) {
        self.refresh_latest = None;
        self.phase = phase;
        self.failure = None;
        self.retry_query = None;
    }

    #[must_use]
    pub(crate) fn retry_query(&self) -> Option<PendingDeathViewQuery> {
        self.retry_query.clone()
    }

    pub(crate) fn continuation_query(
        &self,
        limit: u16,
    ) -> Result<PendingDeathViewQuery, DeathViewClientError> {
        if self.phase != TerminalDeathPhase::SummaryReady {
            return Err(DeathViewClientError::InvalidTerminalPhase);
        }
        let summary = self
            .summary
            .as_ref()
            .ok_or(DeathViewClientError::InvalidTerminalPhase)?;
        let next = summary
            .next_lost_ordinal
            .ok_or(DeathViewClientError::NoAdditionalLossPage)?;
        Ok(PendingDeathViewQuery::Summary {
            death_id: summary.death_id,
            lost_start_ordinal: next,
            lost_limit: limit,
            intent: TerminalQueryIntent::Continuation,
        })
    }
}
