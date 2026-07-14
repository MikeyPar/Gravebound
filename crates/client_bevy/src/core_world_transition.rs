//! Pure native projection for Core loading, transfer failure, and reconnect states.
//!
//! This module owns no widgets and advances no server authority. A destination becomes ready only
//! after an exact authoritative snapshot and matching compiled-scene readiness are both present.

use protocol::{
    CharacterLocation, CharacterLocationSnapshot, HandshakeRejection, SessionDestination, WireText,
    WorldFlowContentRevisionV1, WorldFlowResult, WorldTransferMutation, WorldTransferResultCode,
};
use sim_content::CoreWorldTransitionCopyKey;
use thiserror::Error;

const LANTERN_HALLS_ID: &str = "hub.lantern_halls_01";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreWorldTransitionPhase {
    SafeOrigin,
    RequestingTransfer,
    LoadingContent,
    AwaitingAuthoritativeState,
    Ready,
    RecoverableError,
    FatalError,
    LinkLost,
    Reconnecting,
    ResolvedToHall,
    ResolvedToCharacterSelect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreSafeOrigin {
    CharacterSelect,
    LanternHalls,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreRetryDirective {
    Unavailable,
    SameMutation,
    RefreshAuthoritativeState,
    ReconnectTransport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreWorldTransitionFailure {
    Transfer(WorldTransferResultCode),
    Handshake(HandshakeRejection),
    InvalidAuthoritativeState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreWorldTransitionResolution {
    None,
    TransferReady,
    Reattached,
    HallCommitted,
    DeathCommitted,
}

impl CoreWorldTransitionFailure {
    #[must_use]
    pub const fn localization_key(self) -> &'static str {
        match self {
            Self::Transfer(code) => transfer_failure_localization_key(code),
            Self::Handshake(HandshakeRejection::Maintenance) => "transition.handshake.maintenance",
            Self::Handshake(HandshakeRejection::UpdateRequired) => {
                "transition.handshake.update_required"
            }
            Self::Handshake(HandshakeRejection::ProtocolUnsupported) => {
                "transition.handshake.protocol_unsupported"
            }
            Self::Handshake(HandshakeRejection::AuthenticationFailed) => {
                "transition.handshake.authentication_failed"
            }
            Self::Handshake(HandshakeRejection::AccountSuspended) => {
                "transition.handshake.account_suspended"
            }
            Self::Handshake(HandshakeRejection::RegionFull) => "transition.handshake.region_full",
            Self::Handshake(HandshakeRejection::ContentMismatch) => {
                "transition.handshake.content_mismatch"
            }
            Self::Handshake(HandshakeRejection::RateLimited) => "transition.handshake.rate_limited",
            Self::Handshake(HandshakeRejection::InternalRetryable) => {
                "transition.handshake.internal_retryable"
            }
            Self::InvalidAuthoritativeState => "transition.phase.fatal_error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreSceneReadiness {
    pub location_id: WireText<96>,
    pub character_version: u64,
    pub content_revision: WorldFlowContentRevisionV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoadingCompletion {
    Ordinary,
    Reattached,
    ResolvedToHall,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreWorldTransitionModel {
    content_revision: WorldFlowContentRevisionV1,
    character_id: [u8; 16],
    phase: CoreWorldTransitionPhase,
    safe_origin: CoreSafeOrigin,
    current_snapshot: Option<CharacterLocationSnapshot>,
    last_request_sequence: u32,
    pending_request_sequence: Option<u32>,
    pending_mutation: Option<WorldTransferMutation>,
    loading_snapshot: Option<CharacterLocationSnapshot>,
    loading_completion: Option<LoadingCompletion>,
    failure: Option<CoreWorldTransitionFailure>,
    retry: CoreRetryDirective,
    reconnect_attempt: Option<u8>,
    resolution: CoreWorldTransitionResolution,
}

impl CoreWorldTransitionModel {
    pub fn new(
        content_revision: WorldFlowContentRevisionV1,
        snapshot: CharacterLocationSnapshot,
    ) -> Result<Self, CoreWorldTransitionError> {
        snapshot
            .validate()
            .map_err(|_| CoreWorldTransitionError::InvalidSnapshot)?;
        let safe_origin = safe_origin(&snapshot)?;
        Ok(Self {
            content_revision,
            character_id: snapshot.character_id,
            phase: CoreWorldTransitionPhase::SafeOrigin,
            safe_origin,
            current_snapshot: Some(snapshot),
            last_request_sequence: 0,
            pending_request_sequence: None,
            pending_mutation: None,
            loading_snapshot: None,
            loading_completion: None,
            failure: None,
            retry: CoreRetryDirective::Unavailable,
            reconnect_attempt: None,
            resolution: CoreWorldTransitionResolution::None,
        })
    }

    #[must_use]
    pub const fn phase(&self) -> CoreWorldTransitionPhase {
        self.phase
    }

    #[must_use]
    pub const fn safe_origin(&self) -> CoreSafeOrigin {
        self.safe_origin
    }

    #[must_use]
    pub const fn retry_directive(&self) -> CoreRetryDirective {
        self.retry
    }

    #[must_use]
    pub const fn failure(&self) -> Option<CoreWorldTransitionFailure> {
        self.failure
    }

    #[must_use]
    pub const fn reconnect_attempt(&self) -> Option<u8> {
        self.reconnect_attempt
    }

    #[must_use]
    pub const fn resolution(&self) -> CoreWorldTransitionResolution {
        self.resolution
    }

    #[must_use]
    pub const fn current_snapshot(&self) -> Option<&CharacterLocationSnapshot> {
        self.current_snapshot.as_ref()
    }

    #[must_use]
    pub const fn phase_copy_key(&self) -> CoreWorldTransitionCopyKey {
        match self.phase {
            CoreWorldTransitionPhase::SafeOrigin => CoreWorldTransitionCopyKey::PhaseSafeOrigin,
            CoreWorldTransitionPhase::RequestingTransfer => {
                CoreWorldTransitionCopyKey::PhaseRequestingTransfer
            }
            CoreWorldTransitionPhase::LoadingContent => {
                CoreWorldTransitionCopyKey::PhaseLoadingContent
            }
            CoreWorldTransitionPhase::AwaitingAuthoritativeState => {
                CoreWorldTransitionCopyKey::PhaseAwaitingAuthoritativeState
            }
            CoreWorldTransitionPhase::Ready => CoreWorldTransitionCopyKey::PhaseReady,
            CoreWorldTransitionPhase::RecoverableError => {
                CoreWorldTransitionCopyKey::PhaseRecoverableError
            }
            CoreWorldTransitionPhase::FatalError => CoreWorldTransitionCopyKey::PhaseFatalError,
            CoreWorldTransitionPhase::LinkLost => CoreWorldTransitionCopyKey::PhaseLinkLost,
            CoreWorldTransitionPhase::Reconnecting => CoreWorldTransitionCopyKey::PhaseReconnecting,
            CoreWorldTransitionPhase::ResolvedToHall => {
                CoreWorldTransitionCopyKey::PhaseResolvedToHall
            }
            CoreWorldTransitionPhase::ResolvedToCharacterSelect => {
                CoreWorldTransitionCopyKey::PhaseResolvedToCharacterSelect
            }
        }
    }

    pub fn begin_transfer(
        &mut self,
        request_sequence: u32,
        mutation: WorldTransferMutation,
    ) -> Result<(), CoreWorldTransitionError> {
        if !matches!(
            self.phase,
            CoreWorldTransitionPhase::SafeOrigin | CoreWorldTransitionPhase::Ready
        ) {
            return Err(CoreWorldTransitionError::InvalidPhase);
        }
        if request_sequence == 0 || request_sequence <= self.last_request_sequence {
            return Err(CoreWorldTransitionError::StaleRequestSequence);
        }
        mutation
            .validate()
            .map_err(|_| CoreWorldTransitionError::InvalidMutation)?;
        let current = self
            .current_snapshot
            .as_ref()
            .ok_or(CoreWorldTransitionError::InvalidAuthoritativeState)?;
        if mutation.character_id != self.character_id
            || mutation.expected_character_version != current.character_version
            || mutation.payload.content_revision != self.content_revision
        {
            return Err(CoreWorldTransitionError::InvalidMutation);
        }
        self.last_request_sequence = request_sequence;
        self.pending_request_sequence = Some(request_sequence);
        self.pending_mutation = Some(mutation);
        self.loading_snapshot = None;
        self.loading_completion = None;
        self.failure = None;
        self.retry = CoreRetryDirective::Unavailable;
        self.reconnect_attempt = None;
        self.resolution = CoreWorldTransitionResolution::None;
        self.phase = CoreWorldTransitionPhase::RequestingTransfer;
        Ok(())
    }

    pub fn retry_same_mutation(
        &mut self,
        request_sequence: u32,
    ) -> Result<WorldTransferMutation, CoreWorldTransitionError> {
        if self.phase != CoreWorldTransitionPhase::RecoverableError
            || self.retry != CoreRetryDirective::SameMutation
        {
            return Err(CoreWorldTransitionError::RetryUnavailable);
        }
        if request_sequence == 0 || request_sequence <= self.last_request_sequence {
            return Err(CoreWorldTransitionError::StaleRequestSequence);
        }
        let mutation = self
            .pending_mutation
            .clone()
            .ok_or(CoreWorldTransitionError::InvalidAuthoritativeState)?;
        self.failure = None;
        self.retry = CoreRetryDirective::Unavailable;
        self.last_request_sequence = request_sequence;
        self.pending_request_sequence = Some(request_sequence);
        self.phase = CoreWorldTransitionPhase::RequestingTransfer;
        Ok(mutation)
    }

    pub fn apply_world_flow_result(
        &mut self,
        result: &WorldFlowResult,
    ) -> Result<(), CoreWorldTransitionError> {
        result
            .validate()
            .map_err(|_| CoreWorldTransitionError::InvalidResult)?;
        match result {
            WorldFlowResult::Transfer {
                request_sequence,
                mutation_id,
                accepted,
                code,
                snapshot,
                ..
            } => {
                let pending = self
                    .pending_mutation
                    .as_ref()
                    .ok_or(CoreWorldTransitionError::MissingPendingMutation)?;
                if self.phase != CoreWorldTransitionPhase::RequestingTransfer
                    || Some(*request_sequence) != self.pending_request_sequence
                    || *mutation_id != pending.mutation_id
                {
                    return Err(CoreWorldTransitionError::StaleOrForeignResult);
                }
                if *accepted {
                    let snapshot = snapshot
                        .as_ref()
                        .ok_or(CoreWorldTransitionError::InvalidSnapshot)?;
                    self.accept_transfer_snapshot(snapshot.clone(), LoadingCompletion::Ordinary)
                } else {
                    self.reject_transfer(*code, snapshot.as_ref())
                }
            }
            WorldFlowResult::Error {
                request_sequence,
                code,
                snapshot,
            } => {
                if self.phase != CoreWorldTransitionPhase::RequestingTransfer
                    || Some(*request_sequence) != self.pending_request_sequence
                {
                    return Err(CoreWorldTransitionError::StaleOrForeignResult);
                }
                self.reject_transfer(*code, snapshot.as_ref())
            }
            WorldFlowResult::Location { .. } => Err(CoreWorldTransitionError::StaleOrForeignResult),
        }
    }

    pub fn mark_content_ready(
        &mut self,
        readiness: &CoreSceneReadiness,
    ) -> Result<(), CoreWorldTransitionError> {
        if self.phase != CoreWorldTransitionPhase::LoadingContent
            || readiness.content_revision != self.content_revision
        {
            return Err(CoreWorldTransitionError::InvalidSceneReadiness);
        }
        let snapshot = self
            .loading_snapshot
            .take()
            .ok_or(CoreWorldTransitionError::InvalidAuthoritativeState)?;
        let expected_location =
            scene_location_id(&snapshot).ok_or(CoreWorldTransitionError::InvalidSceneReadiness)?;
        if readiness.location_id.as_str() != expected_location
            || readiness.character_version != snapshot.character_version
        {
            self.loading_snapshot = Some(snapshot);
            return Err(CoreWorldTransitionError::InvalidSceneReadiness);
        }
        self.current_snapshot = Some(snapshot.clone());
        if is_hall(&snapshot) {
            self.safe_origin = CoreSafeOrigin::LanternHalls;
        }
        let completion = self
            .loading_completion
            .take()
            .ok_or(CoreWorldTransitionError::InvalidAuthoritativeState)?;
        self.phase = match completion {
            LoadingCompletion::Ordinary | LoadingCompletion::Reattached => {
                CoreWorldTransitionPhase::Ready
            }
            LoadingCompletion::ResolvedToHall => CoreWorldTransitionPhase::ResolvedToHall,
        };
        self.resolution = match completion {
            LoadingCompletion::Ordinary => CoreWorldTransitionResolution::TransferReady,
            LoadingCompletion::Reattached => CoreWorldTransitionResolution::Reattached,
            LoadingCompletion::ResolvedToHall => CoreWorldTransitionResolution::HallCommitted,
        };
        self.pending_mutation = None;
        self.pending_request_sequence = None;
        self.failure = None;
        self.retry = CoreRetryDirective::Unavailable;
        Ok(())
    }

    pub fn transport_lost(&mut self) -> Result<(), CoreWorldTransitionError> {
        if !matches!(
            self.phase,
            CoreWorldTransitionPhase::RequestingTransfer
                | CoreWorldTransitionPhase::LoadingContent
                | CoreWorldTransitionPhase::Ready
        ) {
            return Err(CoreWorldTransitionError::InvalidPhase);
        }
        self.phase = CoreWorldTransitionPhase::LinkLost;
        self.failure = None;
        self.retry = CoreRetryDirective::ReconnectTransport;
        self.reconnect_attempt = None;
        self.resolution = CoreWorldTransitionResolution::None;
        Ok(())
    }

    /// Changes presentation only. No local deadline may choose Recall, Hall, or death finality.
    pub fn await_authoritative_resolution(&mut self) -> Result<(), CoreWorldTransitionError> {
        if self.phase != CoreWorldTransitionPhase::LinkLost {
            return Err(CoreWorldTransitionError::InvalidPhase);
        }
        self.phase = CoreWorldTransitionPhase::AwaitingAuthoritativeState;
        Ok(())
    }

    pub fn reconnecting(&mut self, attempt: u8) -> Result<(), CoreWorldTransitionError> {
        if !matches!(
            self.phase,
            CoreWorldTransitionPhase::LinkLost
                | CoreWorldTransitionPhase::AwaitingAuthoritativeState
                | CoreWorldTransitionPhase::Reconnecting
        ) || attempt == 0
        {
            return Err(CoreWorldTransitionError::InvalidReconnectAttempt);
        }
        if self
            .reconnect_attempt
            .is_some_and(|previous| attempt <= previous)
        {
            return Err(CoreWorldTransitionError::InvalidReconnectAttempt);
        }
        self.reconnect_attempt = Some(attempt);
        self.phase = CoreWorldTransitionPhase::Reconnecting;
        Ok(())
    }

    pub fn reconnect_resolved(
        &mut self,
        destination: SessionDestination,
        snapshot: Option<CharacterLocationSnapshot>,
    ) -> Result<(), CoreWorldTransitionError> {
        if !matches!(
            self.phase,
            CoreWorldTransitionPhase::LinkLost
                | CoreWorldTransitionPhase::AwaitingAuthoritativeState
                | CoreWorldTransitionPhase::Reconnecting
        ) {
            return Err(CoreWorldTransitionError::InvalidPhase);
        }
        match destination {
            SessionDestination::CombatInstance => {
                let snapshot = snapshot.ok_or(CoreWorldTransitionError::InvalidSnapshot)?;
                if !matches!(snapshot.location, CharacterLocation::Danger { .. }) {
                    return Err(CoreWorldTransitionError::InvalidSnapshot);
                }
                self.accept_transfer_snapshot(snapshot, LoadingCompletion::Reattached)
            }
            SessionDestination::LanternHalls => {
                let snapshot = snapshot.ok_or(CoreWorldTransitionError::InvalidSnapshot)?;
                if !is_hall(&snapshot) {
                    return Err(CoreWorldTransitionError::InvalidSnapshot);
                }
                self.accept_transfer_snapshot(snapshot, LoadingCompletion::ResolvedToHall)
            }
            SessionDestination::DeathFinal => {
                if snapshot.is_some() {
                    return Err(CoreWorldTransitionError::InvalidSnapshot);
                }
                self.current_snapshot = None;
                self.pending_mutation = None;
                self.pending_request_sequence = None;
                self.loading_snapshot = None;
                self.loading_completion = None;
                self.failure = None;
                self.retry = CoreRetryDirective::Unavailable;
                self.resolution = CoreWorldTransitionResolution::DeathCommitted;
                self.phase = CoreWorldTransitionPhase::ResolvedToCharacterSelect;
                Ok(())
            }
            SessionDestination::Closed => {
                self.enter_fatal_invalid_state();
                Ok(())
            }
        }
    }

    pub fn apply_handshake_rejection(
        &mut self,
        rejection: HandshakeRejection,
    ) -> Result<(), CoreWorldTransitionError> {
        if matches!(
            self.phase,
            CoreWorldTransitionPhase::FatalError
                | CoreWorldTransitionPhase::ResolvedToCharacterSelect
                | CoreWorldTransitionPhase::ResolvedToHall
        ) {
            return Err(CoreWorldTransitionError::InvalidPhase);
        }
        self.failure = Some(CoreWorldTransitionFailure::Handshake(rejection));
        self.retry = handshake_retry(rejection);
        self.phase = if self.retry == CoreRetryDirective::ReconnectTransport {
            CoreWorldTransitionPhase::RecoverableError
        } else {
            CoreWorldTransitionPhase::FatalError
        };
        Ok(())
    }

    fn accept_transfer_snapshot(
        &mut self,
        snapshot: CharacterLocationSnapshot,
        completion: LoadingCompletion,
    ) -> Result<(), CoreWorldTransitionError> {
        validate_owned_snapshot(self.character_id, &snapshot)?;
        if self
            .current_snapshot
            .as_ref()
            .is_some_and(|current| snapshot.character_version < current.character_version)
        {
            return Err(CoreWorldTransitionError::InvalidSnapshot);
        }
        if let Some(pending) = &self.pending_mutation {
            let expected_version = pending
                .expected_character_version
                .checked_add(1)
                .ok_or(CoreWorldTransitionError::InvalidSnapshot)?;
            if snapshot.character_version != expected_version {
                return Err(CoreWorldTransitionError::InvalidSnapshot);
            }
        }
        if matches!(snapshot.location, CharacterLocation::CharacterSelect { .. }) {
            self.current_snapshot = Some(snapshot);
            self.pending_mutation = None;
            self.pending_request_sequence = None;
            self.loading_snapshot = None;
            self.loading_completion = None;
            self.failure = None;
            self.retry = CoreRetryDirective::Unavailable;
            self.safe_origin = CoreSafeOrigin::CharacterSelect;
            self.phase = CoreWorldTransitionPhase::ResolvedToCharacterSelect;
            self.resolution = CoreWorldTransitionResolution::None;
            return Ok(());
        }
        self.loading_snapshot = Some(snapshot);
        self.loading_completion = Some(completion);
        self.failure = None;
        self.retry = CoreRetryDirective::Unavailable;
        self.phase = CoreWorldTransitionPhase::LoadingContent;
        Ok(())
    }

    fn reject_transfer(
        &mut self,
        code: WorldTransferResultCode,
        snapshot: Option<&CharacterLocationSnapshot>,
    ) -> Result<(), CoreWorldTransitionError> {
        if code == WorldTransferResultCode::Accepted {
            return Err(CoreWorldTransitionError::InvalidResult);
        }
        if let Some(snapshot) = snapshot {
            validate_owned_snapshot(self.character_id, snapshot)?;
            if self
                .current_snapshot
                .as_ref()
                .is_some_and(|current| snapshot.character_version < current.character_version)
            {
                return Err(CoreWorldTransitionError::StaleOrForeignResult);
            }
            self.current_snapshot = Some(snapshot.clone());
        }
        self.failure = Some(CoreWorldTransitionFailure::Transfer(code));
        self.retry = transfer_retry(code);
        self.loading_snapshot = None;
        self.loading_completion = None;
        self.reconnect_attempt = None;
        match code {
            WorldTransferResultCode::CharacterDead
            | WorldTransferResultCode::NoSelectedCharacter => {
                self.current_snapshot = None;
                self.pending_mutation = None;
                self.pending_request_sequence = None;
                self.retry = CoreRetryDirective::Unavailable;
                self.resolution = if code == WorldTransferResultCode::CharacterDead {
                    CoreWorldTransitionResolution::DeathCommitted
                } else {
                    CoreWorldTransitionResolution::None
                };
                self.phase = CoreWorldTransitionPhase::ResolvedToCharacterSelect;
            }
            _ if transfer_is_fatal(code) => {
                self.pending_mutation = None;
                self.pending_request_sequence = None;
                self.phase = CoreWorldTransitionPhase::FatalError;
            }
            _ => {
                if self.retry != CoreRetryDirective::SameMutation {
                    self.pending_mutation = None;
                    self.pending_request_sequence = None;
                }
                self.phase = CoreWorldTransitionPhase::RecoverableError;
            }
        }
        Ok(())
    }

    fn enter_fatal_invalid_state(&mut self) {
        self.pending_mutation = None;
        self.pending_request_sequence = None;
        self.loading_snapshot = None;
        self.loading_completion = None;
        self.failure = Some(CoreWorldTransitionFailure::InvalidAuthoritativeState);
        self.retry = CoreRetryDirective::Unavailable;
        self.phase = CoreWorldTransitionPhase::FatalError;
    }
}

fn validate_owned_snapshot(
    character_id: [u8; 16],
    snapshot: &CharacterLocationSnapshot,
) -> Result<(), CoreWorldTransitionError> {
    snapshot
        .validate()
        .map_err(|_| CoreWorldTransitionError::InvalidSnapshot)?;
    if snapshot.character_id != character_id {
        return Err(CoreWorldTransitionError::InvalidSnapshot);
    }
    Ok(())
}

fn safe_origin(
    snapshot: &CharacterLocationSnapshot,
) -> Result<CoreSafeOrigin, CoreWorldTransitionError> {
    match &snapshot.location {
        CharacterLocation::CharacterSelect { .. } => Ok(CoreSafeOrigin::CharacterSelect),
        CharacterLocation::Safe { location_id, .. } if location_id.as_str() == LANTERN_HALLS_ID => {
            Ok(CoreSafeOrigin::LanternHalls)
        }
        CharacterLocation::Safe { .. } | CharacterLocation::Danger { .. } => {
            Err(CoreWorldTransitionError::UnsafeInitialState)
        }
    }
}

fn is_hall(snapshot: &CharacterLocationSnapshot) -> bool {
    matches!(
        &snapshot.location,
        CharacterLocation::Safe { location_id, .. } if location_id.as_str() == LANTERN_HALLS_ID
    )
}

fn scene_location_id(snapshot: &CharacterLocationSnapshot) -> Option<&str> {
    match &snapshot.location {
        CharacterLocation::Safe { location_id, .. }
        | CharacterLocation::Danger { location_id, .. } => Some(location_id.as_str()),
        CharacterLocation::CharacterSelect { .. } => None,
    }
}

const fn handshake_retry(rejection: HandshakeRejection) -> CoreRetryDirective {
    match rejection {
        HandshakeRejection::Maintenance
        | HandshakeRejection::RegionFull
        | HandshakeRejection::RateLimited
        | HandshakeRejection::InternalRetryable => CoreRetryDirective::ReconnectTransport,
        HandshakeRejection::UpdateRequired
        | HandshakeRejection::ProtocolUnsupported
        | HandshakeRejection::AuthenticationFailed
        | HandshakeRejection::AccountSuspended
        | HandshakeRejection::ContentMismatch => CoreRetryDirective::Unavailable,
    }
}

const fn transfer_retry(code: WorldTransferResultCode) -> CoreRetryDirective {
    match code {
        WorldTransferResultCode::TransferInProgress
        | WorldTransferResultCode::InstanceUnavailable
        | WorldTransferResultCode::RateLimited
        | WorldTransferResultCode::ServiceUnavailable => CoreRetryDirective::SameMutation,
        WorldTransferResultCode::StateVersionMismatch
        | WorldTransferResultCode::IssuedAtInvalid => CoreRetryDirective::RefreshAuthoritativeState,
        WorldTransferResultCode::Accepted
        | WorldTransferResultCode::StageDisabled
        | WorldTransferResultCode::CharacterNotFound
        | WorldTransferResultCode::NoSelectedCharacter
        | WorldTransferResultCode::CharacterNotOwned
        | WorldTransferResultCode::CharacterDead
        | WorldTransferResultCode::InvalidSource
        | WorldTransferResultCode::OutOfRange
        | WorldTransferResultCode::ContentDisabled
        | WorldTransferResultCode::DestinationDisabled
        | WorldTransferResultCode::ContentMismatch
        | WorldTransferResultCode::IdempotencyConflict
        | WorldTransferResultCode::PayloadHashMismatch
        | WorldTransferResultCode::IncompleteRestorePoint
        | WorldTransferResultCode::StorageResolutionRequired => CoreRetryDirective::Unavailable,
    }
}

const fn transfer_is_fatal(code: WorldTransferResultCode) -> bool {
    matches!(
        code,
        WorldTransferResultCode::CharacterNotFound
            | WorldTransferResultCode::CharacterNotOwned
            | WorldTransferResultCode::InvalidSource
            | WorldTransferResultCode::ContentMismatch
            | WorldTransferResultCode::IdempotencyConflict
            | WorldTransferResultCode::PayloadHashMismatch
    )
}

const fn transfer_failure_localization_key(code: WorldTransferResultCode) -> &'static str {
    match code {
        WorldTransferResultCode::Accepted => "transition.phase.ready",
        WorldTransferResultCode::StageDisabled => "transition.transfer.stage_disabled",
        WorldTransferResultCode::StateVersionMismatch => {
            "transition.transfer.state_version_mismatch"
        }
        WorldTransferResultCode::CharacterNotFound => "transition.transfer.character_not_found",
        WorldTransferResultCode::NoSelectedCharacter => "transition.transfer.no_selected_character",
        WorldTransferResultCode::CharacterNotOwned => "transition.transfer.character_not_owned",
        WorldTransferResultCode::CharacterDead => "transition.transfer.character_dead",
        WorldTransferResultCode::InvalidSource => "transition.transfer.invalid_source",
        WorldTransferResultCode::OutOfRange => "transition.transfer.out_of_range",
        WorldTransferResultCode::ContentDisabled => "transition.transfer.content_disabled",
        WorldTransferResultCode::DestinationDisabled => "transition.transfer.destination_disabled",
        WorldTransferResultCode::TransferInProgress => "transition.transfer.transfer_in_progress",
        WorldTransferResultCode::ContentMismatch => "transition.transfer.content_mismatch",
        WorldTransferResultCode::IdempotencyConflict => "transition.transfer.idempotency_conflict",
        WorldTransferResultCode::PayloadHashMismatch => "transition.transfer.payload_hash_mismatch",
        WorldTransferResultCode::IssuedAtInvalid => "transition.transfer.issued_at_invalid",
        WorldTransferResultCode::IncompleteRestorePoint => {
            "transition.transfer.incomplete_restore_point"
        }
        WorldTransferResultCode::StorageResolutionRequired => {
            "transition.transfer.storage_resolution_required"
        }
        WorldTransferResultCode::InstanceUnavailable => "transition.transfer.instance_unavailable",
        WorldTransferResultCode::RateLimited => "transition.transfer.rate_limited",
        WorldTransferResultCode::ServiceUnavailable => "transition.transfer.service_unavailable",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CoreWorldTransitionError {
    #[error("transition action is invalid in the current phase")]
    InvalidPhase,
    #[error("initial transition state must be Character Select or Lantern Halls")]
    UnsafeInitialState,
    #[error("world-transfer mutation is invalid or does not match authoritative state")]
    InvalidMutation,
    #[error("world-flow result is malformed")]
    InvalidResult,
    #[error("authoritative snapshot is missing, malformed, foreign, or inconsistent")]
    InvalidSnapshot,
    #[error("scene readiness does not match the authoritative location and content revision")]
    InvalidSceneReadiness,
    #[error("no transfer mutation is awaiting a result")]
    MissingPendingMutation,
    #[error("world-flow result is stale or belongs to another request")]
    StaleOrForeignResult,
    #[error("world-flow request sequence must be positive and strictly increasing")]
    StaleRequestSequence,
    #[error("authoritative state required by this projection is absent")]
    InvalidAuthoritativeState,
    #[error("the current failure does not permit identical mutation retry")]
    RetryUnavailable,
    #[error("reconnect attempt must be positive and strictly increasing")]
    InvalidReconnectAttempt,
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use protocol::{ManifestHash, SafeArrival, WorldTransferCommand, WorldTransferPayload};
    use sim_content::load_core_development_world_flow;

    use super::*;

    fn content_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn revision() -> WorldFlowContentRevisionV1 {
        let compiled = load_core_development_world_flow(&content_root()).unwrap();
        WorldFlowContentRevisionV1 {
            records_blake3: ManifestHash::new(compiled.hashes().records_blake3.clone()).unwrap(),
            assets_blake3: ManifestHash::new(compiled.hashes().assets_blake3.clone()).unwrap(),
            localization_blake3: ManifestHash::new(compiled.hashes().localization_blake3.clone())
                .unwrap(),
        }
    }

    fn hall(version: u64) -> CharacterLocationSnapshot {
        CharacterLocationSnapshot {
            character_id: [7; 16],
            character_version: version,
            location: CharacterLocation::Safe {
                location_id: WireText::new(LANTERN_HALLS_ID).unwrap(),
                arrival: SafeArrival::HallDefault,
            },
        }
    }

    fn danger(version: u64) -> CharacterLocationSnapshot {
        CharacterLocationSnapshot {
            character_id: [7; 16],
            character_version: version,
            location: CharacterLocation::Danger {
                location_id: WireText::new("world.core_microrealm_01").unwrap(),
                instance_lineage_id: [8; 16],
                entry_restore_point_id: [9; 16],
            },
        }
    }

    fn mutation(id: u8, expected_version: u64) -> WorldTransferMutation {
        let payload = WorldTransferPayload {
            content_revision: revision(),
            command: WorldTransferCommand::UsePortal {
                portal_id: WireText::new("portal.dungeon.bell_sepulcher").unwrap(),
            },
        };
        WorldTransferMutation {
            mutation_id: [id; 16],
            character_id: [7; 16],
            expected_character_version: expected_version,
            issued_at_unix_millis: 1,
            payload_hash: payload.canonical_hash(),
            payload,
        }
    }

    fn transfer_result(
        mutation_id: [u8; 16],
        code: WorldTransferResultCode,
        snapshot: Option<CharacterLocationSnapshot>,
    ) -> WorldFlowResult {
        let accepted = code == WorldTransferResultCode::Accepted;
        WorldFlowResult::Transfer {
            request_sequence: 1,
            mutation_id,
            accepted,
            code,
            snapshot,
            transfer_id: accepted.then_some([44; 16]),
        }
    }

    fn readiness(snapshot: &CharacterLocationSnapshot) -> CoreSceneReadiness {
        CoreSceneReadiness {
            location_id: WireText::new(scene_location_id(snapshot).unwrap()).unwrap(),
            character_version: snapshot.character_version,
            content_revision: revision(),
        }
    }

    #[test]
    fn accepted_transfer_requires_exact_snapshot_and_compiled_scene_readiness() {
        let mut model = CoreWorldTransitionModel::new(revision(), hall(1)).unwrap();
        let command = mutation(10, 1);
        model.begin_transfer(1, command.clone()).unwrap();
        assert_eq!(model.phase(), CoreWorldTransitionPhase::RequestingTransfer);
        model
            .apply_world_flow_result(&transfer_result(
                command.mutation_id,
                WorldTransferResultCode::Accepted,
                Some(danger(2)),
            ))
            .unwrap();
        assert_eq!(model.phase(), CoreWorldTransitionPhase::LoadingContent);
        let mut wrong = readiness(&danger(2));
        wrong.character_version = 3;
        assert_eq!(
            model.mark_content_ready(&wrong),
            Err(CoreWorldTransitionError::InvalidSceneReadiness)
        );
        model.mark_content_ready(&readiness(&danger(2))).unwrap();
        assert_eq!(model.phase(), CoreWorldTransitionPhase::Ready);
        assert_eq!(model.safe_origin(), CoreSafeOrigin::LanternHalls);
        assert_eq!(model.current_snapshot(), Some(&danger(2)));
    }

    #[test]
    fn every_transfer_rejection_has_closed_copy_and_retry_semantics() {
        let same_mutation = [
            WorldTransferResultCode::TransferInProgress,
            WorldTransferResultCode::InstanceUnavailable,
            WorldTransferResultCode::RateLimited,
            WorldTransferResultCode::ServiceUnavailable,
        ];
        let refresh = [
            WorldTransferResultCode::StateVersionMismatch,
            WorldTransferResultCode::IssuedAtInvalid,
        ];
        let recover_without_retry = [
            WorldTransferResultCode::StageDisabled,
            WorldTransferResultCode::OutOfRange,
            WorldTransferResultCode::ContentDisabled,
            WorldTransferResultCode::DestinationDisabled,
            WorldTransferResultCode::IncompleteRestorePoint,
            WorldTransferResultCode::StorageResolutionRequired,
        ];
        let resolved_character_select = [
            WorldTransferResultCode::NoSelectedCharacter,
            WorldTransferResultCode::CharacterDead,
        ];
        let fatal = [
            WorldTransferResultCode::CharacterNotFound,
            WorldTransferResultCode::CharacterNotOwned,
            WorldTransferResultCode::InvalidSource,
            WorldTransferResultCode::ContentMismatch,
            WorldTransferResultCode::IdempotencyConflict,
            WorldTransferResultCode::PayloadHashMismatch,
        ];
        let compiled = load_core_development_world_flow(&content_root()).unwrap();
        for (codes, phase, retry) in [
            (
                same_mutation.as_slice(),
                CoreWorldTransitionPhase::RecoverableError,
                CoreRetryDirective::SameMutation,
            ),
            (
                refresh.as_slice(),
                CoreWorldTransitionPhase::RecoverableError,
                CoreRetryDirective::RefreshAuthoritativeState,
            ),
            (
                recover_without_retry.as_slice(),
                CoreWorldTransitionPhase::RecoverableError,
                CoreRetryDirective::Unavailable,
            ),
            (
                resolved_character_select.as_slice(),
                CoreWorldTransitionPhase::ResolvedToCharacterSelect,
                CoreRetryDirective::Unavailable,
            ),
            (
                fatal.as_slice(),
                CoreWorldTransitionPhase::FatalError,
                CoreRetryDirective::Unavailable,
            ),
        ] {
            for &code in codes {
                let mut model = CoreWorldTransitionModel::new(revision(), hall(1)).unwrap();
                let command = mutation(11, 1);
                model.begin_transfer(1, command.clone()).unwrap();
                model
                    .apply_world_flow_result(&transfer_result(command.mutation_id, code, None))
                    .unwrap();
                assert_eq!(model.phase(), phase, "{code:?}");
                assert_eq!(model.retry_directive(), retry, "{code:?}");
                let failure = model.failure().unwrap();
                assert_eq!(failure, CoreWorldTransitionFailure::Transfer(code));
                assert!(compiled.localized(failure.localization_key()).is_some());
                assert!(compiled.transition_copy(model.phase_copy_key()).len() > 1);
            }
        }
    }

    #[test]
    fn identical_retry_preserves_canonical_mutation_identity_and_payload() {
        let mut model = CoreWorldTransitionModel::new(revision(), hall(1)).unwrap();
        let command = mutation(12, 1);
        model.begin_transfer(1, command.clone()).unwrap();
        model
            .apply_world_flow_result(&transfer_result(
                command.mutation_id,
                WorldTransferResultCode::ServiceUnavailable,
                Some(hall(1)),
            ))
            .unwrap();
        assert_eq!(model.retry_same_mutation(2).unwrap(), command);
        assert_eq!(model.phase(), CoreWorldTransitionPhase::RequestingTransfer);
    }

    #[test]
    fn stale_request_sequences_and_results_cannot_mutate_the_projection() {
        let mut model = CoreWorldTransitionModel::new(revision(), hall(1)).unwrap();
        let command = mutation(17, 1);
        model.begin_transfer(2, command.clone()).unwrap();
        assert_eq!(
            model.apply_world_flow_result(&transfer_result(
                command.mutation_id,
                WorldTransferResultCode::ServiceUnavailable,
                None,
            )),
            Err(CoreWorldTransitionError::StaleOrForeignResult)
        );
        assert_eq!(model.phase(), CoreWorldTransitionPhase::RequestingTransfer);

        let mut accepted = transfer_result(
            command.mutation_id,
            WorldTransferResultCode::Accepted,
            Some(danger(2)),
        );
        let WorldFlowResult::Transfer {
            request_sequence, ..
        } = &mut accepted
        else {
            unreachable!()
        };
        *request_sequence = 2;
        model.apply_world_flow_result(&accepted).unwrap();
        model.mark_content_ready(&readiness(&danger(2))).unwrap();
        assert_eq!(
            model.begin_transfer(2, mutation(18, 2)),
            Err(CoreWorldTransitionError::StaleRequestSequence)
        );
    }

    #[test]
    fn handshake_rejections_are_exhaustive_and_only_retry_transport_when_safe() {
        let retryable = [
            HandshakeRejection::Maintenance,
            HandshakeRejection::RegionFull,
            HandshakeRejection::RateLimited,
            HandshakeRejection::InternalRetryable,
        ];
        let fatal = [
            HandshakeRejection::UpdateRequired,
            HandshakeRejection::ProtocolUnsupported,
            HandshakeRejection::AuthenticationFailed,
            HandshakeRejection::AccountSuspended,
            HandshakeRejection::ContentMismatch,
        ];
        let compiled = load_core_development_world_flow(&content_root()).unwrap();
        for (codes, phase, retry) in [
            (
                retryable.as_slice(),
                CoreWorldTransitionPhase::RecoverableError,
                CoreRetryDirective::ReconnectTransport,
            ),
            (
                fatal.as_slice(),
                CoreWorldTransitionPhase::FatalError,
                CoreRetryDirective::Unavailable,
            ),
        ] {
            for &code in codes {
                let mut model = CoreWorldTransitionModel::new(revision(), hall(1)).unwrap();
                model.apply_handshake_rejection(code).unwrap();
                assert_eq!(model.phase(), phase);
                assert_eq!(model.retry_directive(), retry);
                assert!(
                    compiled
                        .localized(model.failure().unwrap().localization_key())
                        .is_some()
                );
            }
        }
    }

    #[test]
    fn link_lost_never_chooses_terminal_state_without_server_resolution() {
        let mut model = CoreWorldTransitionModel::new(revision(), hall(1)).unwrap();
        let command = mutation(13, 1);
        model.begin_transfer(1, command.clone()).unwrap();
        model
            .apply_world_flow_result(&transfer_result(
                command.mutation_id,
                WorldTransferResultCode::Accepted,
                Some(danger(2)),
            ))
            .unwrap();
        model.mark_content_ready(&readiness(&danger(2))).unwrap();
        model.transport_lost().unwrap();
        model.await_authoritative_resolution().unwrap();
        assert_eq!(
            model.phase(),
            CoreWorldTransitionPhase::AwaitingAuthoritativeState
        );
        assert_eq!(model.safe_origin(), CoreSafeOrigin::LanternHalls);
        model.reconnecting(1).unwrap();
        assert_eq!(model.phase(), CoreWorldTransitionPhase::Reconnecting);
        model
            .reconnect_resolved(SessionDestination::CombatInstance, Some(danger(2)))
            .unwrap();
        assert_eq!(model.phase(), CoreWorldTransitionPhase::LoadingContent);
        model.mark_content_ready(&readiness(&danger(2))).unwrap();
        assert_eq!(model.phase(), CoreWorldTransitionPhase::Ready);
    }

    #[test]
    fn committed_hall_and_death_resolutions_are_terminal_and_nonreversible() {
        let mut hall_model = CoreWorldTransitionModel::new(revision(), hall(1)).unwrap();
        hall_model.transport_lost().unwrap_err();
        let command = mutation(14, 1);
        hall_model.begin_transfer(1, command.clone()).unwrap();
        hall_model
            .apply_world_flow_result(&transfer_result(
                command.mutation_id,
                WorldTransferResultCode::Accepted,
                Some(danger(2)),
            ))
            .unwrap();
        hall_model
            .mark_content_ready(&readiness(&danger(2)))
            .unwrap();
        hall_model.transport_lost().unwrap();
        hall_model.reconnecting(1).unwrap();
        hall_model
            .reconnect_resolved(SessionDestination::LanternHalls, Some(hall(3)))
            .unwrap();
        hall_model.mark_content_ready(&readiness(&hall(3))).unwrap();
        assert_eq!(hall_model.phase(), CoreWorldTransitionPhase::ResolvedToHall);
        assert_eq!(
            hall_model.begin_transfer(2, mutation(15, 3)),
            Err(CoreWorldTransitionError::InvalidPhase)
        );

        let mut death_model = CoreWorldTransitionModel::new(revision(), hall(1)).unwrap();
        death_model.begin_transfer(1, mutation(16, 1)).unwrap();
        death_model.transport_lost().unwrap();
        death_model.reconnecting(1).unwrap();
        death_model
            .reconnect_resolved(SessionDestination::DeathFinal, None)
            .unwrap();
        assert_eq!(
            death_model.phase(),
            CoreWorldTransitionPhase::ResolvedToCharacterSelect
        );
        assert!(death_model.current_snapshot().is_none());
        assert_eq!(
            death_model.reconnect_resolved(SessionDestination::LanternHalls, Some(hall(2))),
            Err(CoreWorldTransitionError::InvalidPhase)
        );
    }
}
