//! Renderer-independent native successor recovery for `GB-M03-07`.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`DTH-020`, `DTH-021`, `UI-007`-
//! `009`, `TECH-021`-`023`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-CATALOG-003`), and `Gravebound_Development_Roadmap_v1.md` (`GB-M03-07` and the M03
//! recovery gates). The model accepts only an opaque durable terminal-summary authority, retains
//! one exact create frame across response loss, and delegates Play-to-Hall authority to the
//! existing world-transition model.

use protocol::{
    CHARACTER_ID_BYTES, CLASS_ID_MAX_BYTES, CORE_SUCCESSOR_FEATURE_FLAG, CharacterLocation,
    CharacterLocationSnapshot, MUTATION_ID_BYTES, SUCCESSOR_CONTENT_ID_MAX_BYTES,
    SUCCESSOR_SCHEMA_VERSION, SafeArrival, ServerHello, StoredSuccessorResultV1,
    SuccessorAppearanceSnapshotV1, SuccessorCreateFrameV1, SuccessorCreatePayloadV1,
    SuccessorCreateResultV1, SuccessorRejectionCodeV1, WireText, WorldFlowContentRevisionV1,
    WorldFlowFrame, WorldFlowRequest, WorldFlowResult, WorldTransferCommand, WorldTransferMutation,
    WorldTransferPayload,
};
use thiserror::Error;

use crate::{
    CoreRetryDirective, CoreSafeOrigin, CoreSceneReadiness, CoreWorldTransitionError,
    CoreWorldTransitionModel, CoreWorldTransitionPhase, TerminalSuccessorAuthority,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuccessorRecoveryPhase {
    Disabled,
    AwaitingTerminalSummary,
    Ready,
    Submitting,
    RecoverableError,
    CharacterSelect,
    EnteringHall,
    LoadingHall,
    HallRecoverableError,
    ControllableHall,
    FatalError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuccessorRecoveryRetryDirective {
    Unavailable,
    ExactCreateFrame,
    RefreshDeathSummary,
    RestartAfterUpdate,
    SameHallMutation,
    RefreshHallAuthority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuccessorRecoveryClientFailure {
    pub code: SuccessorRejectionCodeV1,
    pub retry: SuccessorRecoveryRetryDirective,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuccessorRecoveryApplyDisposition {
    AppliedFresh,
    AppliedReplay,
    Rejected,
    IgnoredDuplicate,
    IgnoredStale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuccessorCharacterSelectProjection {
    pub selected_character_id: [u8; CHARACTER_ID_BYTES],
    pub roster_ordinal: u8,
    pub class_id: WireText<CLASS_ID_MAX_BYTES>,
    pub appearance: SuccessorAppearanceSnapshotV1,
    pub level: u16,
    pub has_oath: bool,
    pub account_version: u64,
    pub character_version: u64,
    pub world_version: u64,
}

#[derive(Debug, Clone)]
pub struct SuccessorRecoveryClientModel {
    phase: SuccessorRecoveryPhase,
    content_revision: WireText<SUCCESSOR_CONTENT_ID_MAX_BYTES>,
    next_sequence: u32,
    death_id: Option<[u8; 16]>,
    retained_create: Option<SuccessorCreateFrameV1>,
    stored: Option<StoredSuccessorResultV1>,
    last_result: Option<SuccessorCreateResultV1>,
    failure: Option<SuccessorRecoveryClientFailure>,
    retry: SuccessorRecoveryRetryDirective,
    confirmations: u8,
    hall_transition: Option<CoreWorldTransitionModel>,
}

impl SuccessorRecoveryClientModel {
    #[must_use]
    pub fn new(
        server_hello: &ServerHello,
        content_revision: WireText<SUCCESSOR_CONTENT_ID_MAX_BYTES>,
    ) -> Self {
        let enabled = server_hello
            .feature_flags
            .iter()
            .any(|flag| flag.as_str() == CORE_SUCCESSOR_FEATURE_FLAG);
        Self {
            phase: if enabled {
                SuccessorRecoveryPhase::AwaitingTerminalSummary
            } else {
                SuccessorRecoveryPhase::Disabled
            },
            content_revision,
            next_sequence: 1,
            death_id: None,
            retained_create: None,
            stored: None,
            last_result: None,
            failure: None,
            retry: SuccessorRecoveryRetryDirective::Unavailable,
            confirmations: 0,
            hall_transition: None,
        }
    }

    #[must_use]
    pub const fn phase(&self) -> SuccessorRecoveryPhase {
        self.phase
    }

    #[must_use]
    pub const fn retry_directive(&self) -> SuccessorRecoveryRetryDirective {
        self.retry
    }

    #[must_use]
    pub const fn failure(&self) -> Option<SuccessorRecoveryClientFailure> {
        self.failure
    }

    #[must_use]
    pub const fn confirmations(&self) -> u8 {
        self.confirmations
    }

    #[must_use]
    pub const fn retained_create(&self) -> Option<&SuccessorCreateFrameV1> {
        self.retained_create.as_ref()
    }

    #[must_use]
    pub const fn stored(&self) -> Option<&StoredSuccessorResultV1> {
        self.stored.as_ref()
    }

    #[must_use]
    pub const fn hall_transition(&self) -> Option<&CoreWorldTransitionModel> {
        self.hall_transition.as_ref()
    }

    #[must_use]
    pub fn action_available(&self, authority: TerminalSuccessorAuthority) -> bool {
        self.phase == SuccessorRecoveryPhase::Ready
            && self.death_id == Some(authority.death_id())
            && self.retained_create.is_none()
    }

    pub fn observe_terminal_summary(
        &mut self,
        authority: TerminalSuccessorAuthority,
    ) -> Result<(), SuccessorRecoveryClientError> {
        match self.phase {
            SuccessorRecoveryPhase::AwaitingTerminalSummary => {
                self.death_id = Some(authority.death_id());
                self.phase = SuccessorRecoveryPhase::Ready;
                Ok(())
            }
            SuccessorRecoveryPhase::Ready if self.death_id == Some(authority.death_id()) => Ok(()),
            SuccessorRecoveryPhase::Disabled => Err(SuccessorRecoveryClientError::FeatureDisabled),
            _ => Err(SuccessorRecoveryClientError::InvalidPhase),
        }
    }

    /// Confirmation one. The resulting frame is retained byte-for-byte until a stored result is
    /// accepted or a nonretryable rejection closes the attempt.
    pub fn begin_create(
        &mut self,
        mutation_id: [u8; MUTATION_ID_BYTES],
    ) -> Result<SuccessorCreateFrameV1, SuccessorRecoveryClientError> {
        if self.phase != SuccessorRecoveryPhase::Ready || self.retained_create.is_some() {
            return Err(SuccessorRecoveryClientError::InvalidPhase);
        }
        let death_id = self
            .death_id
            .ok_or(SuccessorRecoveryClientError::MissingTerminalAuthority)?;
        let sequence = self.next_sequence;
        let payload = SuccessorCreatePayloadV1 {
            death_id,
            content_revision: self.content_revision.clone(),
        };
        let frame = SuccessorCreateFrameV1 {
            schema_version: SUCCESSOR_SCHEMA_VERSION,
            sequence,
            mutation_id,
            payload_hash: payload.canonical_hash(),
            payload,
        };
        frame
            .validate()
            .map_err(|_| SuccessorRecoveryClientError::InvalidCreateFrame)?;
        self.next_sequence = sequence
            .checked_add(1)
            .ok_or(SuccessorRecoveryClientError::SequenceExhausted)?;
        self.retained_create = Some(frame.clone());
        self.failure = None;
        self.retry = SuccessorRecoveryRetryDirective::Unavailable;
        self.phase = SuccessorRecoveryPhase::Submitting;
        self.confirmations = 1;
        Ok(frame)
    }

    pub fn handle_create_response_loss(&mut self) -> Result<(), SuccessorRecoveryClientError> {
        if self.phase != SuccessorRecoveryPhase::Submitting || self.retained_create.is_none() {
            return Err(SuccessorRecoveryClientError::InvalidPhase);
        }
        self.failure = None;
        self.retry = SuccessorRecoveryRetryDirective::ExactCreateFrame;
        self.phase = SuccessorRecoveryPhase::RecoverableError;
        Ok(())
    }

    pub fn retry_create(&mut self) -> Result<SuccessorCreateFrameV1, SuccessorRecoveryClientError> {
        if self.phase != SuccessorRecoveryPhase::RecoverableError
            || self.retry != SuccessorRecoveryRetryDirective::ExactCreateFrame
        {
            return Err(SuccessorRecoveryClientError::RetryUnavailable);
        }
        let frame = self
            .retained_create
            .clone()
            .ok_or(SuccessorRecoveryClientError::MissingCreateFrame)?;
        self.failure = None;
        self.last_result = None;
        self.retry = SuccessorRecoveryRetryDirective::Unavailable;
        self.phase = SuccessorRecoveryPhase::Submitting;
        Ok(frame)
    }

    pub fn apply_create_result(
        &mut self,
        result: &SuccessorCreateResultV1,
    ) -> Result<SuccessorRecoveryApplyDisposition, SuccessorRecoveryClientError> {
        result.validate().map_err(|_| {
            self.phase = SuccessorRecoveryPhase::FatalError;
            self.retry = SuccessorRecoveryRetryDirective::Unavailable;
            SuccessorRecoveryClientError::InvalidCreateResult
        })?;
        if self.last_result.as_ref() == Some(result) {
            return Ok(SuccessorRecoveryApplyDisposition::IgnoredDuplicate);
        }
        let Some(pending) = self.retained_create.as_ref() else {
            return Ok(SuccessorRecoveryApplyDisposition::IgnoredStale);
        };
        if self.phase != SuccessorRecoveryPhase::Submitting
            || result.request_sequence() != pending.sequence
            || result.mutation_id() != pending.mutation_id
            || result.death_id() != pending.payload.death_id
        {
            return Ok(SuccessorRecoveryApplyDisposition::IgnoredStale);
        }
        match result {
            SuccessorCreateResultV1::Stored {
                replayed,
                result: stored,
                ..
            } => {
                if stored.content_revision != pending.payload.content_revision
                    || stored.selected_character_id != stored.successor_id
                {
                    self.phase = SuccessorRecoveryPhase::FatalError;
                    self.retry = SuccessorRecoveryRetryDirective::Unavailable;
                    return Err(SuccessorRecoveryClientError::InvalidStoredAuthority);
                }
                self.stored = Some(stored.as_ref().clone());
                self.last_result = Some(result.clone());
                self.failure = None;
                self.retry = SuccessorRecoveryRetryDirective::Unavailable;
                self.phase = SuccessorRecoveryPhase::CharacterSelect;
                Ok(if *replayed {
                    SuccessorRecoveryApplyDisposition::AppliedReplay
                } else {
                    SuccessorRecoveryApplyDisposition::AppliedFresh
                })
            }
            SuccessorCreateResultV1::Rejected { code, .. } => {
                let (phase, retry) = rejection_policy(*code);
                self.failure = Some(SuccessorRecoveryClientFailure { code: *code, retry });
                self.retry = retry;
                self.phase = phase;
                self.last_result = Some(result.clone());
                if retry != SuccessorRecoveryRetryDirective::ExactCreateFrame {
                    self.retained_create = None;
                }
                Ok(SuccessorRecoveryApplyDisposition::Rejected)
            }
        }
    }

    #[must_use]
    pub fn character_select_projection(&self) -> Option<SuccessorCharacterSelectProjection> {
        if !matches!(
            self.phase,
            SuccessorRecoveryPhase::CharacterSelect
                | SuccessorRecoveryPhase::EnteringHall
                | SuccessorRecoveryPhase::LoadingHall
                | SuccessorRecoveryPhase::HallRecoverableError
                | SuccessorRecoveryPhase::ControllableHall
        ) {
            return None;
        }
        self.stored
            .as_ref()
            .map(|stored| SuccessorCharacterSelectProjection {
                selected_character_id: stored.selected_character_id,
                roster_ordinal: stored.former_roster_ordinal,
                class_id: stored.class_id.clone(),
                appearance: stored.appearance,
                level: 1,
                has_oath: false,
                account_version: stored.versions.account,
                character_version: stored.versions.character,
                world_version: stored.versions.world,
            })
    }

    /// Confirmation two. This emits only the ordinary authoritative Character Select -> Hall
    /// mutation and initializes the shared transition model from the stored successor result.
    pub fn begin_play(
        &mut self,
        request_sequence: u32,
        mutation_id: [u8; MUTATION_ID_BYTES],
        issued_at_unix_millis: u64,
        content_revision: WorldFlowContentRevisionV1,
    ) -> Result<WorldFlowFrame, SuccessorRecoveryClientError> {
        if self.phase != SuccessorRecoveryPhase::CharacterSelect {
            return Err(SuccessorRecoveryClientError::InvalidPhase);
        }
        let stored = self
            .stored
            .as_ref()
            .ok_or(SuccessorRecoveryClientError::MissingStoredResult)?;
        let snapshot = CharacterLocationSnapshot {
            character_id: stored.successor_id,
            character_version: stored.versions.character,
            location: CharacterLocation::CharacterSelect {
                next_hall_arrival: SafeArrival::HallDefault,
            },
        };
        let payload = WorldTransferPayload {
            content_revision: content_revision.clone(),
            command: WorldTransferCommand::EnterHallFromCharacterSelect,
        };
        let mutation = WorldTransferMutation {
            mutation_id,
            character_id: stored.successor_id,
            expected_character_version: stored.versions.character,
            issued_at_unix_millis,
            payload_hash: payload.canonical_hash(),
            payload,
        };
        let frame = WorldFlowFrame {
            sequence: request_sequence,
            request: WorldFlowRequest::Transfer(mutation.clone()),
        };
        frame
            .validate()
            .map_err(|_| SuccessorRecoveryClientError::InvalidHallFrame)?;
        let mut transition = CoreWorldTransitionModel::new(content_revision, snapshot)?;
        transition.begin_transfer(request_sequence, mutation)?;
        self.hall_transition = Some(transition);
        self.phase = SuccessorRecoveryPhase::EnteringHall;
        self.confirmations = 2;
        Ok(frame)
    }

    pub fn apply_hall_result(
        &mut self,
        result: &WorldFlowResult,
    ) -> Result<(), SuccessorRecoveryClientError> {
        if self.phase != SuccessorRecoveryPhase::EnteringHall {
            return Err(SuccessorRecoveryClientError::InvalidPhase);
        }
        let transition = self
            .hall_transition
            .as_mut()
            .ok_or(SuccessorRecoveryClientError::MissingHallTransition)?;
        transition.apply_world_flow_result(result)?;
        self.phase = match transition.phase() {
            CoreWorldTransitionPhase::LoadingContent => SuccessorRecoveryPhase::LoadingHall,
            CoreWorldTransitionPhase::RecoverableError => {
                self.retry = match transition.retry_directive() {
                    CoreRetryDirective::SameMutation => {
                        SuccessorRecoveryRetryDirective::SameHallMutation
                    }
                    CoreRetryDirective::RefreshAuthoritativeState => {
                        SuccessorRecoveryRetryDirective::RefreshHallAuthority
                    }
                    CoreRetryDirective::Unavailable | CoreRetryDirective::ReconnectTransport => {
                        SuccessorRecoveryRetryDirective::Unavailable
                    }
                };
                SuccessorRecoveryPhase::HallRecoverableError
            }
            CoreWorldTransitionPhase::FatalError
            | CoreWorldTransitionPhase::ResolvedToCharacterSelect
            | CoreWorldTransitionPhase::ResolvedToDeathSummary => {
                self.retry = SuccessorRecoveryRetryDirective::Unavailable;
                SuccessorRecoveryPhase::FatalError
            }
            _ => return Err(SuccessorRecoveryClientError::InvalidHallTransition),
        };
        Ok(())
    }

    pub fn retry_play(
        &mut self,
        request_sequence: u32,
    ) -> Result<WorldFlowFrame, SuccessorRecoveryClientError> {
        if self.phase != SuccessorRecoveryPhase::HallRecoverableError
            || self.retry != SuccessorRecoveryRetryDirective::SameHallMutation
        {
            return Err(SuccessorRecoveryClientError::RetryUnavailable);
        }
        let transition = self
            .hall_transition
            .as_mut()
            .ok_or(SuccessorRecoveryClientError::MissingHallTransition)?;
        let mutation = transition.retry_same_mutation(request_sequence)?;
        self.phase = SuccessorRecoveryPhase::EnteringHall;
        self.retry = SuccessorRecoveryRetryDirective::Unavailable;
        Ok(WorldFlowFrame {
            sequence: request_sequence,
            request: WorldFlowRequest::Transfer(mutation),
        })
    }

    pub fn mark_hall_content_ready(
        &mut self,
        readiness: &CoreSceneReadiness,
    ) -> Result<(), SuccessorRecoveryClientError> {
        if self.phase != SuccessorRecoveryPhase::LoadingHall {
            return Err(SuccessorRecoveryClientError::InvalidPhase);
        }
        let transition = self
            .hall_transition
            .as_mut()
            .ok_or(SuccessorRecoveryClientError::MissingHallTransition)?;
        transition.mark_content_ready(readiness)?;
        if transition.phase() != CoreWorldTransitionPhase::Ready
            || transition.safe_origin() != CoreSafeOrigin::LanternHalls
        {
            self.phase = SuccessorRecoveryPhase::FatalError;
            return Err(SuccessorRecoveryClientError::InvalidHallTransition);
        }
        self.phase = SuccessorRecoveryPhase::ControllableHall;
        self.retry = SuccessorRecoveryRetryDirective::Unavailable;
        Ok(())
    }
}

fn rejection_policy(
    code: SuccessorRejectionCodeV1,
) -> (SuccessorRecoveryPhase, SuccessorRecoveryRetryDirective) {
    match code {
        SuccessorRejectionCodeV1::DatabaseUnavailable
        | SuccessorRejectionCodeV1::UnresolvedMutation => (
            SuccessorRecoveryPhase::RecoverableError,
            SuccessorRecoveryRetryDirective::ExactCreateFrame,
        ),
        SuccessorRejectionCodeV1::DeathNotFound
        | SuccessorRejectionCodeV1::DeathNotTerminal
        | SuccessorRejectionCodeV1::DeathSuperseded
        | SuccessorRejectionCodeV1::AlreadyConsumed => (
            SuccessorRecoveryPhase::FatalError,
            SuccessorRecoveryRetryDirective::RefreshDeathSummary,
        ),
        SuccessorRejectionCodeV1::ContentMismatch => (
            SuccessorRecoveryPhase::FatalError,
            SuccessorRecoveryRetryDirective::RestartAfterUpdate,
        ),
        SuccessorRejectionCodeV1::FeatureDisabled => (
            SuccessorRecoveryPhase::Disabled,
            SuccessorRecoveryRetryDirective::Unavailable,
        ),
        SuccessorRejectionCodeV1::InvalidRequest
        | SuccessorRejectionCodeV1::ForeignAuthority
        | SuccessorRejectionCodeV1::SlotConflict
        | SuccessorRejectionCodeV1::IdempotencyConflict
        | SuccessorRejectionCodeV1::CorruptStoredAuthority => (
            SuccessorRecoveryPhase::FatalError,
            SuccessorRecoveryRetryDirective::Unavailable,
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SuccessorRecoveryClientError {
    #[error("successor capability was not negotiated")]
    FeatureDisabled,
    #[error("successor recovery action is invalid in the current phase")]
    InvalidPhase,
    #[error("durable terminal-summary authority is missing")]
    MissingTerminalAuthority,
    #[error("successor request sequence space is exhausted")]
    SequenceExhausted,
    #[error("successor create frame is invalid")]
    InvalidCreateFrame,
    #[error("successor create result is invalid")]
    InvalidCreateResult,
    #[error("stored successor authority does not match the retained request")]
    InvalidStoredAuthority,
    #[error("no exact successor create frame is retained")]
    MissingCreateFrame,
    #[error("successor retry is unavailable")]
    RetryUnavailable,
    #[error("stored successor result is missing")]
    MissingStoredResult,
    #[error("Character Select to Hall frame is invalid")]
    InvalidHallFrame,
    #[error("Character Select to Hall transition is missing")]
    MissingHallTransition,
    #[error("Character Select to Hall transition resolved to an invalid state")]
    InvalidHallTransition,
    #[error(transparent)]
    WorldTransition(#[from] CoreWorldTransitionError),
}

trait SuccessorResultBinding {
    fn request_sequence(&self) -> u32;
    fn mutation_id(&self) -> [u8; MUTATION_ID_BYTES];
    fn death_id(&self) -> [u8; 16];
}

impl SuccessorResultBinding for SuccessorCreateResultV1 {
    fn request_sequence(&self) -> u32 {
        match self {
            Self::Stored {
                request_sequence, ..
            }
            | Self::Rejected {
                request_sequence, ..
            } => *request_sequence,
        }
    }

    fn mutation_id(&self) -> [u8; MUTATION_ID_BYTES] {
        match self {
            Self::Stored { result, .. } => result.mutation_id,
            Self::Rejected { mutation_id, .. } => *mutation_id,
        }
    }

    fn death_id(&self) -> [u8; 16] {
        match self {
            Self::Stored { result, .. } => result.death_id,
            Self::Rejected { death_id, .. } => *death_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use protocol::{
        CORE_SUCCESSOR_BASE_SILHOUETTE_ID, GRAVE_ARBALIST_CLASS_ID, ManifestHash, PROTOCOL_MAJOR,
        PROTOCOL_MINOR, SIMULATION_HZ, SNAPSHOT_HZ, SUCCESSOR_RESULT_HASH_BYTES,
        SuccessorStarterItemsV1, SuccessorVersionVectorV1, WorldTransferResultCode,
    };

    use super::*;

    const DEATH_ID: [u8; 16] = [2; 16];
    const MUTATION_ID: [u8; 16] = [3; 16];
    const SUCCESSOR_ID: [u8; 16] = [4; 16];

    fn hello(successor: bool) -> ServerHello {
        ServerHello {
            session_id: WireText::new("successor-client-test").unwrap(),
            protocol_major: PROTOCOL_MAJOR,
            protocol_minor: PROTOCOL_MINOR,
            required_client_build: WireText::new("core-dev").unwrap(),
            content_bundle_version: WireText::new("core-test").unwrap(),
            server_tick_rate: SIMULATION_HZ,
            snapshot_rate: SNAPSHOT_HZ,
            region_id: WireText::new("local").unwrap(),
            feature_flags: successor
                .then(|| WireText::new(CORE_SUCCESSOR_FEATURE_FLAG).unwrap())
                .into_iter()
                .collect(),
        }
    }

    fn item_revision() -> WireText<SUCCESSOR_CONTENT_ID_MAX_BYTES> {
        WireText::new(format!("core-dev.blake3.{}", "b".repeat(64))).unwrap()
    }

    fn world_revision() -> WorldFlowContentRevisionV1 {
        WorldFlowContentRevisionV1 {
            records_blake3: ManifestHash::new("c".repeat(64)).unwrap(),
            assets_blake3: ManifestHash::new("d".repeat(64)).unwrap(),
            localization_blake3: ManifestHash::new("e".repeat(64)).unwrap(),
        }
    }

    fn authority() -> TerminalSuccessorAuthority {
        TerminalSuccessorAuthority { death_id: DEATH_ID }
    }

    fn stored_result(mutation_id: [u8; 16]) -> StoredSuccessorResultV1 {
        let mut stored = StoredSuccessorResultV1 {
            mutation_id,
            death_id: DEATH_ID,
            successor_id: SUCCESSOR_ID,
            receipt_id: [5; 16],
            former_roster_ordinal: 1,
            class_id: WireText::new(GRAVE_ARBALIST_CLASS_ID).unwrap(),
            appearance: SuccessorAppearanceSnapshotV1::CoreBaseSilhouette,
            starter_items: SuccessorStarterItemsV1 {
                weapon_uid: [6; 16],
                relic_uid: [7; 16],
                tonic_unit_uids: [[8; 16], [9; 16]],
            },
            versions: SuccessorVersionVectorV1 {
                account: 8,
                character: 1,
                progression: 1,
                world: 1,
                inventory: 1,
                life_metrics: 1,
                oath_bargain: 1,
            },
            content_revision: item_revision(),
            selected_character_id: SUCCESSOR_ID,
            result_hash: [0; SUCCESSOR_RESULT_HASH_BYTES],
        };
        stored.result_hash = stored.canonical_result_hash();
        stored
    }

    fn stored_response(replayed: bool) -> SuccessorCreateResultV1 {
        SuccessorCreateResultV1::Stored {
            schema_version: SUCCESSOR_SCHEMA_VERSION,
            request_sequence: 1,
            replayed,
            result: Box::new(stored_result(MUTATION_ID)),
        }
    }

    fn ready_model() -> SuccessorRecoveryClientModel {
        let mut model = SuccessorRecoveryClientModel::new(&hello(true), item_revision());
        model.observe_terminal_summary(authority()).unwrap();
        model
    }

    #[test]
    fn negotiated_capability_and_durable_summary_are_both_required() {
        let mut disabled = SuccessorRecoveryClientModel::new(&hello(false), item_revision());
        assert_eq!(disabled.phase(), SuccessorRecoveryPhase::Disabled);
        assert_eq!(
            disabled.observe_terminal_summary(authority()),
            Err(SuccessorRecoveryClientError::FeatureDisabled)
        );

        let mut enabled = SuccessorRecoveryClientModel::new(&hello(true), item_revision());
        assert_eq!(
            enabled.phase(),
            SuccessorRecoveryPhase::AwaitingTerminalSummary
        );
        assert!(!enabled.action_available(authority()));
        enabled.observe_terminal_summary(authority()).unwrap();
        assert_eq!(enabled.phase(), SuccessorRecoveryPhase::Ready);
        assert!(enabled.action_available(authority()));
        assert!(!enabled.action_available(TerminalSuccessorAuthority { death_id: [10; 16] }));
    }

    #[test]
    fn response_loss_retries_the_exact_create_frame_bytes() {
        let mut model = ready_model();
        let first = model.begin_create(MUTATION_ID).unwrap();
        let first_bytes =
            protocol::encode_frame(&protocol::WireMessage::SuccessorCreateFrame(first.clone()))
                .unwrap();
        assert_eq!(model.confirmations(), 1);
        assert_eq!(
            model.begin_create([11; 16]),
            Err(SuccessorRecoveryClientError::InvalidPhase)
        );
        model.handle_create_response_loss().unwrap();
        assert_eq!(
            model.retry_directive(),
            SuccessorRecoveryRetryDirective::ExactCreateFrame
        );
        let retry = model.retry_create().unwrap();
        let retry_bytes =
            protocol::encode_frame(&protocol::WireMessage::SuccessorCreateFrame(retry)).unwrap();
        assert_eq!(retry_bytes, first_bytes);
    }

    #[test]
    fn stored_replay_enters_preselected_character_select_once() {
        let mut model = ready_model();
        model.begin_create(MUTATION_ID).unwrap();
        let response = stored_response(true);
        assert_eq!(
            model.apply_create_result(&response).unwrap(),
            SuccessorRecoveryApplyDisposition::AppliedReplay
        );
        assert_eq!(model.phase(), SuccessorRecoveryPhase::CharacterSelect);
        let projection = model.character_select_projection().unwrap();
        assert_eq!(projection.selected_character_id, SUCCESSOR_ID);
        assert_eq!(projection.roster_ordinal, 1);
        assert_eq!(projection.class_id.as_str(), GRAVE_ARBALIST_CLASS_ID);
        assert_eq!(projection.level, 1);
        assert!(!projection.has_oath);
        assert_eq!(
            projection.appearance.content_id(),
            CORE_SUCCESSOR_BASE_SILHOUETTE_ID
        );
        assert_eq!(model.confirmations(), 1);
        assert_eq!(
            model.apply_create_result(&response).unwrap(),
            SuccessorRecoveryApplyDisposition::IgnoredDuplicate
        );
        assert_eq!(model.confirmations(), 1);
    }

    #[test]
    fn every_server_rejection_has_a_closed_retry_policy() {
        let cases = [
            (
                SuccessorRejectionCodeV1::FeatureDisabled,
                SuccessorRecoveryPhase::Disabled,
                SuccessorRecoveryRetryDirective::Unavailable,
            ),
            (
                SuccessorRejectionCodeV1::InvalidRequest,
                SuccessorRecoveryPhase::FatalError,
                SuccessorRecoveryRetryDirective::Unavailable,
            ),
            (
                SuccessorRejectionCodeV1::ContentMismatch,
                SuccessorRecoveryPhase::FatalError,
                SuccessorRecoveryRetryDirective::RestartAfterUpdate,
            ),
            (
                SuccessorRejectionCodeV1::ForeignAuthority,
                SuccessorRecoveryPhase::FatalError,
                SuccessorRecoveryRetryDirective::Unavailable,
            ),
            (
                SuccessorRejectionCodeV1::DeathNotFound,
                SuccessorRecoveryPhase::FatalError,
                SuccessorRecoveryRetryDirective::RefreshDeathSummary,
            ),
            (
                SuccessorRejectionCodeV1::DeathNotTerminal,
                SuccessorRecoveryPhase::FatalError,
                SuccessorRecoveryRetryDirective::RefreshDeathSummary,
            ),
            (
                SuccessorRejectionCodeV1::DeathSuperseded,
                SuccessorRecoveryPhase::FatalError,
                SuccessorRecoveryRetryDirective::RefreshDeathSummary,
            ),
            (
                SuccessorRejectionCodeV1::AlreadyConsumed,
                SuccessorRecoveryPhase::FatalError,
                SuccessorRecoveryRetryDirective::RefreshDeathSummary,
            ),
            (
                SuccessorRejectionCodeV1::SlotConflict,
                SuccessorRecoveryPhase::FatalError,
                SuccessorRecoveryRetryDirective::Unavailable,
            ),
            (
                SuccessorRejectionCodeV1::IdempotencyConflict,
                SuccessorRecoveryPhase::FatalError,
                SuccessorRecoveryRetryDirective::Unavailable,
            ),
            (
                SuccessorRejectionCodeV1::DatabaseUnavailable,
                SuccessorRecoveryPhase::RecoverableError,
                SuccessorRecoveryRetryDirective::ExactCreateFrame,
            ),
            (
                SuccessorRejectionCodeV1::CorruptStoredAuthority,
                SuccessorRecoveryPhase::FatalError,
                SuccessorRecoveryRetryDirective::Unavailable,
            ),
            (
                SuccessorRejectionCodeV1::UnresolvedMutation,
                SuccessorRecoveryPhase::RecoverableError,
                SuccessorRecoveryRetryDirective::ExactCreateFrame,
            ),
        ];
        for (code, phase, retry) in cases {
            let mut model = ready_model();
            model.begin_create(MUTATION_ID).unwrap();
            let rejected = SuccessorCreateResultV1::Rejected {
                schema_version: SUCCESSOR_SCHEMA_VERSION,
                request_sequence: 1,
                mutation_id: MUTATION_ID,
                death_id: DEATH_ID,
                code,
            };
            assert_eq!(
                model.apply_create_result(&rejected).unwrap(),
                SuccessorRecoveryApplyDisposition::Rejected
            );
            assert_eq!(model.phase(), phase, "{code:?}");
            assert_eq!(model.retry_directive(), retry, "{code:?}");
            assert_eq!(model.failure().unwrap().code, code);
        }
    }

    #[test]
    fn play_is_confirmation_two_and_only_authoritative_hall_readiness_grants_control() {
        let mut model = ready_model();
        model.begin_create(MUTATION_ID).unwrap();
        model.apply_create_result(&stored_response(false)).unwrap();
        let revision = world_revision();
        let frame = model
            .begin_play(11, [12; 16], 50_000, revision.clone())
            .unwrap();
        assert_eq!(model.confirmations(), 2);
        let WorldFlowRequest::Transfer(mutation) = &frame.request else {
            panic!("Play must emit a transfer mutation");
        };
        assert_eq!(mutation.character_id, SUCCESSOR_ID);
        assert_eq!(mutation.expected_character_version, 1);
        assert_eq!(
            mutation.payload.command,
            WorldTransferCommand::EnterHallFromCharacterSelect
        );
        let hall = CharacterLocationSnapshot {
            character_id: SUCCESSOR_ID,
            character_version: 2,
            location: CharacterLocation::Safe {
                location_id: WireText::new("hub.lantern_halls_01").unwrap(),
                arrival: SafeArrival::HallDefault,
            },
        };
        model
            .apply_hall_result(&WorldFlowResult::Transfer {
                request_sequence: 11,
                mutation_id: [12; 16],
                accepted: true,
                code: WorldTransferResultCode::Accepted,
                snapshot: Some(hall),
                transfer_id: Some([13; 16]),
            })
            .unwrap();
        assert_eq!(model.phase(), SuccessorRecoveryPhase::LoadingHall);
        model
            .mark_hall_content_ready(&CoreSceneReadiness {
                location_id: WireText::new("hub.lantern_halls_01").unwrap(),
                character_version: 2,
                content_revision: revision,
            })
            .unwrap();
        assert_eq!(model.phase(), SuccessorRecoveryPhase::ControllableHall);
        assert_eq!(
            model.hall_transition().unwrap().safe_origin(),
            CoreSafeOrigin::LanternHalls
        );
    }

    #[test]
    fn foreign_results_do_not_consume_or_redirect_the_pending_attempt() {
        let mut model = ready_model();
        let original = model.begin_create(MUTATION_ID).unwrap();
        let mut foreign = stored_result([14; 16]);
        foreign.result_hash = foreign.canonical_result_hash();
        let response = SuccessorCreateResultV1::Stored {
            schema_version: SUCCESSOR_SCHEMA_VERSION,
            request_sequence: 1,
            replayed: false,
            result: Box::new(foreign),
        };
        assert_eq!(
            model.apply_create_result(&response).unwrap(),
            SuccessorRecoveryApplyDisposition::IgnoredStale
        );
        assert_eq!(model.phase(), SuccessorRecoveryPhase::Submitting);
        assert_eq!(model.retained_create(), Some(&original));
        assert!(model.stored().is_none());
    }
}
