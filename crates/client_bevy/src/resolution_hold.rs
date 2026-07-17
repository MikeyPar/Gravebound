//! Pure native client authority for the blocking Resolution Hold recovery flow.
//!
//! The model never predicts storage placement or terminal success. It retains the exact mutation
//! frame across response loss and releases player input only after a correlated empty snapshot.

use protocol::{
    CORE_RESOLUTION_HOLD_FEATURE_FLAG, RESOLUTION_HOLD_ID_MAX_BYTES,
    RESOLUTION_HOLD_SCHEMA_VERSION, ResolutionHoldActionV1, ResolutionHoldMutationFrameV1,
    ResolutionHoldMutationPayloadV1, ResolutionHoldMutationResultV1, ResolutionHoldQueryFrameV1,
    ResolutionHoldQueryResultV1, ResolutionHoldRejectionCodeV1, ResolutionHoldStackV1,
    ResolutionHoldVersionsV1, ServerHello, StoredResolutionHoldMutationResultV1, WireText,
};
use thiserror::Error;

type ResolutionHoldStackKey = ([u8; 16], u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionHoldClientPhase {
    Dormant,
    Querying,
    Ready,
    ConfirmDestroy,
    Submitting,
    Refreshing,
    Resolved,
    RecoverableError,
    FatalError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionHoldRetryDirective {
    Unavailable,
    RetryExactMutation,
    RefreshAuthority,
    WaitForHall,
    CorrectClock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionHoldClientFailure {
    ResponseLost,
    FeatureNotNegotiated,
    InvalidResponse,
    ContentProjectionMismatch,
    Rejected(ResolutionHoldRejectionCodeV1),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionHoldApplyOutcome {
    Ready {
        stack_count: u8,
    },
    Resolved,
    MutationStored {
        replayed: bool,
        remaining_hold_stack_count: u8,
    },
    Rejected(ResolutionHoldRejectionCodeV1),
}

/// Single sequence-space owner for one Resolution Hold reliable dispatcher.
///
/// This type is intentionally not `Clone`. A second owner could issue a new mutation before an
/// unresolved exact frame has been replayed, violating the durable idempotency contract.
#[derive(Debug)]
pub struct ResolutionHoldClientModel {
    expected_content_revision: WireText<RESOLUTION_HOLD_ID_MAX_BYTES>,
    phase: ResolutionHoldClientPhase,
    selected_character_id: Option<[u8; 16]>,
    feature_authorized: bool,
    last_sequence: u32,
    pending_query_sequence: Option<u32>,
    versions: Option<ResolutionHoldVersionsV1>,
    stacks: Vec<ResolutionHoldStackV1>,
    selected_stack: Option<ResolutionHoldStackKey>,
    destroy_review: Option<ResolutionHoldStackKey>,
    in_flight_mutation: Option<ResolutionHoldMutationFrameV1>,
    last_stored_result: Option<StoredResolutionHoldMutationResultV1>,
    failure: Option<ResolutionHoldClientFailure>,
    retry: ResolutionHoldRetryDirective,
}

impl ResolutionHoldClientModel {
    #[must_use]
    pub fn new(expected_content_revision: WireText<RESOLUTION_HOLD_ID_MAX_BYTES>) -> Self {
        Self {
            expected_content_revision,
            phase: ResolutionHoldClientPhase::Dormant,
            selected_character_id: None,
            feature_authorized: false,
            last_sequence: 0,
            pending_query_sequence: None,
            versions: None,
            stacks: Vec::new(),
            selected_stack: None,
            destroy_review: None,
            in_flight_mutation: None,
            last_stored_result: None,
            failure: None,
            retry: ResolutionHoldRetryDirective::Unavailable,
        }
    }

    #[must_use]
    pub const fn phase(&self) -> ResolutionHoldClientPhase {
        self.phase
    }

    #[must_use]
    pub const fn retry_directive(&self) -> ResolutionHoldRetryDirective {
        self.retry
    }

    #[must_use]
    pub const fn failure(&self) -> Option<ResolutionHoldClientFailure> {
        self.failure
    }

    #[must_use]
    pub fn stacks(&self) -> &[ResolutionHoldStackV1] {
        &self.stacks
    }

    #[must_use]
    pub fn selected_stack(&self) -> Option<&ResolutionHoldStackV1> {
        let key = self.selected_stack?;
        self.stack(key)
    }

    #[must_use]
    pub const fn in_flight_mutation(&self) -> Option<&ResolutionHoldMutationFrameV1> {
        self.in_flight_mutation.as_ref()
    }

    #[must_use]
    pub const fn last_stored_result(&self) -> Option<&StoredResolutionHoldMutationResultV1> {
        self.last_stored_result.as_ref()
    }

    #[must_use]
    pub const fn captures_input(&self) -> bool {
        !matches!(
            self.phase,
            ResolutionHoldClientPhase::Dormant | ResolutionHoldClientPhase::Resolved
        )
    }

    #[must_use]
    pub fn can_move_selected_stack(&self) -> bool {
        self.phase == ResolutionHoldClientPhase::Ready
            && self
                .selected_stack()
                .is_some_and(|stack| stack.planned_destination.is_some())
    }

    /// Starts the mandatory Hall query using the capabilities from the current handshake.
    pub fn begin_hall_query(
        &mut self,
        server_hello: &ServerHello,
        character_id: [u8; 16],
        sequence: u32,
    ) -> Result<ResolutionHoldQueryFrameV1, ResolutionHoldClientError> {
        if self.retry == ResolutionHoldRetryDirective::RetryExactMutation
            && self.in_flight_mutation.is_some()
        {
            return Err(ResolutionHoldClientError::ExactMutationRetryRequired);
        }
        self.accept_server_hello(server_hello)?;
        if self.selected_character_id != Some(character_id) {
            self.versions = None;
            self.stacks.clear();
            self.selected_stack = None;
            self.destroy_review = None;
            self.last_stored_result = None;
        }
        self.selected_character_id = Some(character_id);
        self.begin_query(character_id, sequence, ResolutionHoldClientPhase::Querying)
    }

    /// Revalidates capability authority after a transport reconnect before any exact replay.
    pub fn accept_server_hello(
        &mut self,
        server_hello: &ServerHello,
    ) -> Result<(), ResolutionHoldClientError> {
        if server_hello.validate().is_err() {
            self.enter_fatal(ResolutionHoldClientFailure::InvalidResponse);
            return Err(ResolutionHoldClientError::InvalidServerHello);
        }
        self.feature_authorized = server_hello
            .feature_flags
            .iter()
            .any(|flag| flag.as_str() == CORE_RESOLUTION_HOLD_FEATURE_FLAG);
        if !self.feature_authorized {
            self.enter_fatal(ResolutionHoldClientFailure::FeatureNotNegotiated);
            return Err(ResolutionHoldClientError::FeatureNotNegotiated);
        }
        Ok(())
    }

    /// Refreshes the authoritative Hold projection after a stored mutation or recoverable error.
    pub fn begin_refresh_query(
        &mut self,
        sequence: u32,
    ) -> Result<ResolutionHoldQueryFrameV1, ResolutionHoldClientError> {
        if !self.feature_authorized {
            return Err(ResolutionHoldClientError::FeatureNotNegotiated);
        }
        if !matches!(
            self.phase,
            ResolutionHoldClientPhase::Refreshing | ResolutionHoldClientPhase::RecoverableError
        ) || matches!(self.retry, ResolutionHoldRetryDirective::RetryExactMutation)
        {
            return Err(ResolutionHoldClientError::InvalidPhase);
        }
        let character_id = self
            .selected_character_id
            .ok_or(ResolutionHoldClientError::MissingCharacter)?;
        self.begin_query(
            character_id,
            sequence,
            ResolutionHoldClientPhase::Refreshing,
        )
    }

    pub fn apply_query_result(
        &mut self,
        result: &ResolutionHoldQueryResultV1,
    ) -> Result<ResolutionHoldApplyOutcome, ResolutionHoldClientError> {
        if result.validate().is_err() {
            self.enter_fatal(ResolutionHoldClientFailure::InvalidResponse);
            return Err(ResolutionHoldClientError::InvalidResponse);
        }
        let (request_sequence, character_id) = query_identity(result);
        if Some(request_sequence) != self.pending_query_sequence
            || Some(character_id) != self.selected_character_id
            || !matches!(
                self.phase,
                ResolutionHoldClientPhase::Querying | ResolutionHoldClientPhase::Refreshing
            )
        {
            return Err(ResolutionHoldClientError::StaleOrForeignResult);
        }
        self.pending_query_sequence = None;
        match result {
            ResolutionHoldQueryResultV1::Stored {
                versions,
                storage_resolution_required,
                stacks,
                ..
            } => {
                if stacks.iter().any(|stack| {
                    stack.content_revision.as_str() != self.expected_content_revision.as_str()
                }) {
                    self.enter_fatal(ResolutionHoldClientFailure::ContentProjectionMismatch);
                    return Err(ResolutionHoldClientError::ContentProjectionMismatch);
                }
                self.versions = Some(*versions);
                self.stacks.clone_from(stacks);
                self.destroy_review = None;
                self.failure = None;
                self.retry = ResolutionHoldRetryDirective::Unavailable;
                if *storage_resolution_required {
                    let prior_selection = self.selected_stack;
                    self.selected_stack = prior_selection
                        .filter(|key| self.stack(*key).is_some())
                        .or_else(|| self.stacks.first().map(stack_key));
                    self.phase = ResolutionHoldClientPhase::Ready;
                    Ok(ResolutionHoldApplyOutcome::Ready {
                        stack_count: u8::try_from(self.stacks.len())
                            .expect("validated Hold stack count fits u8"),
                    })
                } else {
                    self.selected_stack = None;
                    self.phase = ResolutionHoldClientPhase::Resolved;
                    Ok(ResolutionHoldApplyOutcome::Resolved)
                }
            }
            ResolutionHoldQueryResultV1::Rejected { code, .. } => {
                self.apply_query_rejection(*code);
                Ok(ResolutionHoldApplyOutcome::Rejected(*code))
            }
        }
    }

    pub fn select_stack(
        &mut self,
        extraction_id: [u8; 16],
        stack_index: u8,
    ) -> Result<(), ResolutionHoldClientError> {
        if self.phase != ResolutionHoldClientPhase::Ready {
            return Err(ResolutionHoldClientError::InvalidPhase);
        }
        let key = (extraction_id, stack_index);
        if self.stack(key).is_none() {
            return Err(ResolutionHoldClientError::MissingStack);
        }
        self.selected_stack = Some(key);
        Ok(())
    }

    /// Opens the destructive review. This first action deliberately creates no wire frame.
    pub fn request_destroy_confirmation(&mut self) -> Result<(), ResolutionHoldClientError> {
        if self.phase != ResolutionHoldClientPhase::Ready {
            return Err(ResolutionHoldClientError::InvalidPhase);
        }
        let selected = self
            .selected_stack
            .ok_or(ResolutionHoldClientError::MissingStack)?;
        self.destroy_review = Some(selected);
        self.phase = ResolutionHoldClientPhase::ConfirmDestroy;
        Ok(())
    }

    pub fn cancel_destroy_confirmation(&mut self) -> Result<(), ResolutionHoldClientError> {
        if self.phase != ResolutionHoldClientPhase::ConfirmDestroy {
            return Err(ResolutionHoldClientError::InvalidPhase);
        }
        self.destroy_review = None;
        self.phase = ResolutionHoldClientPhase::Ready;
        Ok(())
    }

    pub fn begin_move(
        &mut self,
        sequence: u32,
        mutation_id: [u8; 16],
        issued_at_unix_millis: u64,
    ) -> Result<ResolutionHoldMutationFrameV1, ResolutionHoldClientError> {
        if !self.can_move_selected_stack() {
            return Err(ResolutionHoldClientError::MoveUnavailable);
        }
        self.build_mutation(
            ResolutionHoldActionV1::Move,
            sequence,
            mutation_id,
            issued_at_unix_millis,
        )
    }

    pub fn confirm_destroy(
        &mut self,
        sequence: u32,
        mutation_id: [u8; 16],
        issued_at_unix_millis: u64,
    ) -> Result<ResolutionHoldMutationFrameV1, ResolutionHoldClientError> {
        if self.phase != ResolutionHoldClientPhase::ConfirmDestroy
            || self.destroy_review != self.selected_stack
        {
            return Err(ResolutionHoldClientError::InvalidPhase);
        }
        self.build_mutation(
            ResolutionHoldActionV1::DestroyConfirmed,
            sequence,
            mutation_id,
            issued_at_unix_millis,
        )
    }

    pub fn apply_mutation_result(
        &mut self,
        response: &ResolutionHoldMutationResultV1,
    ) -> Result<ResolutionHoldApplyOutcome, ResolutionHoldClientError> {
        if response.validate().is_err() {
            self.enter_fatal(ResolutionHoldClientFailure::InvalidResponse);
            return Err(ResolutionHoldClientError::InvalidResponse);
        }
        let frame = self
            .in_flight_mutation
            .as_ref()
            .ok_or(ResolutionHoldClientError::MissingInFlightMutation)?;
        if self.phase != ResolutionHoldClientPhase::Submitting
            || !mutation_response_matches(frame, response)
        {
            return Err(ResolutionHoldClientError::StaleOrForeignResult);
        }
        match response {
            ResolutionHoldMutationResultV1::Stored {
                replayed, result, ..
            } => {
                let remaining_hold_stack_count = result.remaining_hold_stack_count;
                self.last_stored_result = Some((**result).clone());
                self.in_flight_mutation = None;
                self.destroy_review = None;
                self.failure = None;
                self.retry = ResolutionHoldRetryDirective::Unavailable;
                self.phase = ResolutionHoldClientPhase::Refreshing;
                Ok(ResolutionHoldApplyOutcome::MutationStored {
                    replayed: *replayed,
                    remaining_hold_stack_count,
                })
            }
            ResolutionHoldMutationResultV1::Rejected { code, .. } => {
                self.apply_mutation_rejection(*code);
                Ok(ResolutionHoldApplyOutcome::Rejected(*code))
            }
        }
    }

    /// Marks transport response loss without replacing the unresolved mutation identity.
    pub fn transport_lost(&mut self) {
        self.feature_authorized = false;
        self.pending_query_sequence = None;
        self.destroy_review = None;
        self.failure = Some(ResolutionHoldClientFailure::ResponseLost);
        self.phase = ResolutionHoldClientPhase::RecoverableError;
        self.retry = if self.in_flight_mutation.is_some() {
            ResolutionHoldRetryDirective::RetryExactMutation
        } else {
            ResolutionHoldRetryDirective::RefreshAuthority
        };
    }

    /// Returns the byte-equivalent unresolved frame; no field or sequence is regenerated.
    pub fn retry_exact_mutation(
        &mut self,
    ) -> Result<ResolutionHoldMutationFrameV1, ResolutionHoldClientError> {
        if self.phase != ResolutionHoldClientPhase::RecoverableError
            || self.retry != ResolutionHoldRetryDirective::RetryExactMutation
        {
            return Err(ResolutionHoldClientError::ExactMutationRetryUnavailable);
        }
        if !self.feature_authorized {
            return Err(ResolutionHoldClientError::FeatureNotNegotiated);
        }
        let frame = self
            .in_flight_mutation
            .clone()
            .ok_or(ResolutionHoldClientError::MissingInFlightMutation)?;
        self.failure = None;
        self.retry = ResolutionHoldRetryDirective::Unavailable;
        self.phase = ResolutionHoldClientPhase::Submitting;
        Ok(frame)
    }

    fn begin_query(
        &mut self,
        character_id: [u8; 16],
        sequence: u32,
        phase: ResolutionHoldClientPhase,
    ) -> Result<ResolutionHoldQueryFrameV1, ResolutionHoldClientError> {
        if self.pending_query_sequence.is_some() {
            return Err(ResolutionHoldClientError::QueryInFlight);
        }
        self.validate_new_sequence(sequence)?;
        let frame = ResolutionHoldQueryFrameV1 {
            schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
            sequence,
            character_id,
        };
        frame
            .validate()
            .map_err(|_| ResolutionHoldClientError::InvalidQuery)?;
        self.last_sequence = sequence;
        self.pending_query_sequence = Some(sequence);
        self.failure = None;
        self.retry = ResolutionHoldRetryDirective::Unavailable;
        self.phase = phase;
        Ok(frame)
    }

    fn build_mutation(
        &mut self,
        action: ResolutionHoldActionV1,
        sequence: u32,
        mutation_id: [u8; 16],
        issued_at_unix_millis: u64,
    ) -> Result<ResolutionHoldMutationFrameV1, ResolutionHoldClientError> {
        self.validate_new_sequence(sequence)?;
        let character_id = self
            .selected_character_id
            .ok_or(ResolutionHoldClientError::MissingCharacter)?;
        let versions = self
            .versions
            .ok_or(ResolutionHoldClientError::MissingAuthority)?;
        let selected = self
            .selected_stack
            .ok_or(ResolutionHoldClientError::MissingStack)?;
        let stack = self
            .stack(selected)
            .ok_or(ResolutionHoldClientError::MissingStack)?;
        let payload = ResolutionHoldMutationPayloadV1 {
            extraction_id: stack.extraction_id,
            stack_index: stack.stack_index,
            action,
            expected_versions: versions,
            content_revision: stack.content_revision.clone(),
            expected_stack_digest: stack.stack_digest,
        };
        let frame = ResolutionHoldMutationFrameV1 {
            schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
            sequence,
            mutation_id,
            character_id,
            issued_at_unix_millis,
            payload_hash: payload.canonical_hash(),
            payload,
        };
        frame
            .validate()
            .map_err(|_| ResolutionHoldClientError::InvalidMutation)?;
        self.last_sequence = sequence;
        self.in_flight_mutation = Some(frame.clone());
        self.failure = None;
        self.retry = ResolutionHoldRetryDirective::Unavailable;
        self.phase = ResolutionHoldClientPhase::Submitting;
        Ok(frame)
    }

    fn apply_query_rejection(&mut self, code: ResolutionHoldRejectionCodeV1) {
        self.failure = Some(ResolutionHoldClientFailure::Rejected(code));
        self.retry = query_retry_policy(code);
        self.phase = if self.retry == ResolutionHoldRetryDirective::Unavailable {
            ResolutionHoldClientPhase::FatalError
        } else {
            ResolutionHoldClientPhase::RecoverableError
        };
    }

    fn apply_mutation_rejection(&mut self, code: ResolutionHoldRejectionCodeV1) {
        self.failure = Some(ResolutionHoldClientFailure::Rejected(code));
        self.retry = mutation_retry_policy(code);
        match code {
            ResolutionHoldRejectionCodeV1::DatabaseUnavailable => {
                self.phase = ResolutionHoldClientPhase::RecoverableError;
            }
            ResolutionHoldRejectionCodeV1::ConfirmationRequired => {
                self.in_flight_mutation = None;
                self.destroy_review = self.selected_stack;
                self.phase = ResolutionHoldClientPhase::ConfirmDestroy;
            }
            _ if self.retry == ResolutionHoldRetryDirective::Unavailable => {
                self.in_flight_mutation = None;
                self.phase = ResolutionHoldClientPhase::FatalError;
            }
            _ => {
                self.in_flight_mutation = None;
                self.destroy_review = None;
                self.phase = ResolutionHoldClientPhase::RecoverableError;
            }
        }
    }

    fn enter_fatal(&mut self, failure: ResolutionHoldClientFailure) {
        self.pending_query_sequence = None;
        self.destroy_review = None;
        self.in_flight_mutation = None;
        self.failure = Some(failure);
        self.retry = ResolutionHoldRetryDirective::Unavailable;
        self.phase = ResolutionHoldClientPhase::FatalError;
    }

    fn validate_new_sequence(&self, sequence: u32) -> Result<(), ResolutionHoldClientError> {
        if sequence == 0 || sequence <= self.last_sequence {
            Err(ResolutionHoldClientError::StaleSequence)
        } else {
            Ok(())
        }
    }

    fn stack(&self, key: ResolutionHoldStackKey) -> Option<&ResolutionHoldStackV1> {
        self.stacks.iter().find(|stack| stack_key(stack) == key)
    }
}

const fn query_retry_policy(code: ResolutionHoldRejectionCodeV1) -> ResolutionHoldRetryDirective {
    match code {
        ResolutionHoldRejectionCodeV1::DatabaseUnavailable
        | ResolutionHoldRejectionCodeV1::StaleAuthority
        | ResolutionHoldRejectionCodeV1::StorageFull
        | ResolutionHoldRejectionCodeV1::NoHeldStack
        | ResolutionHoldRejectionCodeV1::UnresolvedMutation => {
            ResolutionHoldRetryDirective::RefreshAuthority
        }
        ResolutionHoldRejectionCodeV1::HallBindingRequired => {
            ResolutionHoldRetryDirective::WaitForHall
        }
        _ => ResolutionHoldRetryDirective::Unavailable,
    }
}

const fn mutation_retry_policy(
    code: ResolutionHoldRejectionCodeV1,
) -> ResolutionHoldRetryDirective {
    match code {
        ResolutionHoldRejectionCodeV1::DatabaseUnavailable => {
            ResolutionHoldRetryDirective::RetryExactMutation
        }
        ResolutionHoldRejectionCodeV1::StaleAuthority
        | ResolutionHoldRejectionCodeV1::StorageFull
        | ResolutionHoldRejectionCodeV1::NoHeldStack
        | ResolutionHoldRejectionCodeV1::UnresolvedMutation => {
            ResolutionHoldRetryDirective::RefreshAuthority
        }
        ResolutionHoldRejectionCodeV1::HallBindingRequired => {
            ResolutionHoldRetryDirective::WaitForHall
        }
        ResolutionHoldRejectionCodeV1::IssuedAtInvalid => {
            ResolutionHoldRetryDirective::CorrectClock
        }
        _ => ResolutionHoldRetryDirective::Unavailable,
    }
}

fn stack_key(stack: &ResolutionHoldStackV1) -> ResolutionHoldStackKey {
    (stack.extraction_id, stack.stack_index)
}

fn query_identity(result: &ResolutionHoldQueryResultV1) -> (u32, [u8; 16]) {
    match result {
        ResolutionHoldQueryResultV1::Stored {
            request_sequence,
            character_id,
            ..
        }
        | ResolutionHoldQueryResultV1::Rejected {
            request_sequence,
            character_id,
            ..
        } => (*request_sequence, *character_id),
    }
}

fn mutation_response_matches(
    frame: &ResolutionHoldMutationFrameV1,
    response: &ResolutionHoldMutationResultV1,
) -> bool {
    match response {
        ResolutionHoldMutationResultV1::Stored {
            request_sequence,
            result,
            ..
        } => {
            *request_sequence == frame.sequence
                && result.mutation_id == frame.mutation_id
                && result.character_id == frame.character_id
                && result.extraction_id == frame.payload.extraction_id
                && result.stack_index == frame.payload.stack_index
                && result.action == frame.payload.action
        }
        ResolutionHoldMutationResultV1::Rejected {
            request_sequence,
            mutation_id,
            character_id,
            extraction_id,
            stack_index,
            ..
        } => {
            *request_sequence == frame.sequence
                && *mutation_id == frame.mutation_id
                && *character_id == frame.character_id
                && *extraction_id == frame.payload.extraction_id
                && *stack_index == frame.payload.stack_index
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ResolutionHoldClientError {
    #[error("Resolution Hold is invalid in the current client phase")]
    InvalidPhase,
    #[error("the current server did not negotiate Resolution Hold recovery")]
    FeatureNotNegotiated,
    #[error("the negotiated server hello is malformed")]
    InvalidServerHello,
    #[error("Resolution Hold requires a selected Hall character")]
    MissingCharacter,
    #[error("Resolution Hold query construction failed validation")]
    InvalidQuery,
    #[error("a Resolution Hold query is already in flight")]
    QueryInFlight,
    #[error("Resolution Hold mutation construction failed validation")]
    InvalidMutation,
    #[error("Resolution Hold response failed protocol validation")]
    InvalidResponse,
    #[error("Resolution Hold response is stale or belongs to another request")]
    StaleOrForeignResult,
    #[error("Resolution Hold sequence must be positive and strictly increasing")]
    StaleSequence,
    #[error("a queried Resolution Hold stack is required")]
    MissingStack,
    #[error("authoritative Resolution Hold versions are unavailable")]
    MissingAuthority,
    #[error("the selected stack has no complete server-planned destination")]
    MoveUnavailable,
    #[error("an unresolved Resolution Hold mutation is not present")]
    MissingInFlightMutation,
    #[error("the unresolved mutation must be retried before querying new authority")]
    ExactMutationRetryRequired,
    #[error("the current failure does not permit an exact mutation retry")]
    ExactMutationRetryUnavailable,
    #[error("the server Hold projection does not match compiled content authority")]
    ContentProjectionMismatch,
}

#[cfg(test)]
mod tests {
    use protocol::{
        M03_CORE_DEV_BUILD_ID, ProtocolVersion, ResolutionHoldDestinationV1,
        ResolutionHoldDispositionV1, ResolutionHoldItemKindV1, ResolutionHoldItemTransitionV1,
        ResolutionHoldItemV1, ResolutionHoldMutationResultV1, ResolutionHoldVersionAdvanceV1,
        ResolutionHoldVersionVectorV1, SIMULATION_HZ, SNAPSHOT_HZ,
    };

    use super::*;

    const CHARACTER_ID: [u8; 16] = [1; 16];
    const EXTRACTION_ID: [u8; 16] = [2; 16];
    const ITEM_UID: [u8; 16] = [3; 16];
    const MUTATION_ID: [u8; 16] = [4; 16];
    const STACK_DIGEST: [u8; 32] = [5; 32];

    fn hello(feature_enabled: bool) -> ServerHello {
        let version = ProtocolVersion::current();
        ServerHello {
            session_id: WireText::new("hold-client-model").unwrap(),
            protocol_major: version.major,
            protocol_minor: version.minor,
            required_client_build: WireText::new(M03_CORE_DEV_BUILD_ID).unwrap(),
            content_bundle_version: WireText::new("core-test").unwrap(),
            server_tick_rate: SIMULATION_HZ,
            snapshot_rate: SNAPSHOT_HZ,
            region_id: WireText::new("local").unwrap(),
            feature_flags: feature_enabled
                .then(|| WireText::new(CORE_RESOLUTION_HOLD_FEATURE_FLAG).unwrap())
                .into_iter()
                .collect(),
        }
    }

    const fn versions() -> ResolutionHoldVersionsV1 {
        ResolutionHoldVersionsV1 {
            account: 10,
            character: 20,
            world: 30,
            inventory: 40,
        }
    }

    fn stack(
        content_revision: &str,
        planned_destination: Option<ResolutionHoldDestinationV1>,
    ) -> ResolutionHoldStackV1 {
        ResolutionHoldStackV1 {
            extraction_id: EXTRACTION_ID,
            stack_index: 0,
            template_id: WireText::new("equipment.rustbound_repeater").unwrap(),
            content_revision: WireText::new(content_revision).unwrap(),
            item_kind: ResolutionHoldItemKindV1::Equipment,
            items: vec![ResolutionHoldItemV1 {
                item_uid: ITEM_UID,
                item_version: 8,
            }],
            stack_digest: STACK_DIGEST,
            extracted_at_unix_millis: 1_000,
            overflow_deadline_unix_millis: 259_201_000,
            planned_destination,
        }
    }

    fn stored_query(
        sequence: u32,
        stacks: Vec<ResolutionHoldStackV1>,
    ) -> ResolutionHoldQueryResultV1 {
        ResolutionHoldQueryResultV1::Stored {
            schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
            request_sequence: sequence,
            character_id: CHARACTER_ID,
            versions: versions(),
            storage_resolution_required: !stacks.is_empty(),
            stacks,
        }
    }

    fn ready_model() -> ResolutionHoldClientModel {
        let mut model = ResolutionHoldClientModel::new(WireText::new("core-r1").unwrap());
        model
            .begin_hall_query(&hello(true), CHARACTER_ID, 1)
            .unwrap();
        model
            .apply_query_result(&stored_query(
                1,
                vec![stack(
                    "core-r1",
                    Some(ResolutionHoldDestinationV1::CharacterSafe { slot_index: 0 }),
                )],
            ))
            .unwrap();
        model
    }

    fn stored_move_result(request_sequence: u32, replayed: bool) -> ResolutionHoldMutationResultV1 {
        ResolutionHoldMutationResultV1::Stored {
            schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
            request_sequence,
            replayed,
            result: Box::new(StoredResolutionHoldMutationResultV1 {
                mutation_id: MUTATION_ID,
                character_id: CHARACTER_ID,
                extraction_id: EXTRACTION_ID,
                stack_index: 0,
                action: ResolutionHoldActionV1::Move,
                result_hash: [6; 32],
                committed_at_unix_millis: 2_000,
                versions: ResolutionHoldVersionVectorV1 {
                    account: ResolutionHoldVersionAdvanceV1 {
                        before: 10,
                        after: 10,
                    },
                    character: ResolutionHoldVersionAdvanceV1 {
                        before: 20,
                        after: 21,
                    },
                    world: ResolutionHoldVersionAdvanceV1 {
                        before: 30,
                        after: 31,
                    },
                    inventory: ResolutionHoldVersionAdvanceV1 {
                        before: 40,
                        after: 41,
                    },
                },
                transitions: vec![ResolutionHoldItemTransitionV1 {
                    ordinal: 0,
                    item_uid: ITEM_UID,
                    item_version: 9,
                    disposition: ResolutionHoldDispositionV1::Moved {
                        destination: ResolutionHoldDestinationV1::CharacterSafe { slot_index: 0 },
                    },
                }],
                remaining_hold_stack_count: 0,
                storage_resolution_required: false,
            }),
        }
    }

    fn rejected_mutation(
        request_sequence: u32,
        code: ResolutionHoldRejectionCodeV1,
    ) -> ResolutionHoldMutationResultV1 {
        ResolutionHoldMutationResultV1::Rejected {
            schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
            request_sequence,
            mutation_id: MUTATION_ID,
            character_id: CHARACTER_ID,
            extraction_id: EXTRACTION_ID,
            stack_index: 0,
            code,
        }
    }

    #[test]
    fn hall_query_requires_current_feature_and_correlates_content() {
        let mut disabled = ResolutionHoldClientModel::new(WireText::new("core-r1").unwrap());
        assert_eq!(
            disabled.begin_hall_query(&hello(false), CHARACTER_ID, 1),
            Err(ResolutionHoldClientError::FeatureNotNegotiated)
        );
        assert_eq!(disabled.phase(), ResolutionHoldClientPhase::FatalError);
        assert!(disabled.captures_input());

        let mut model = ResolutionHoldClientModel::new(WireText::new("core-r1").unwrap());
        let query = model
            .begin_hall_query(&hello(true), CHARACTER_ID, 1)
            .unwrap();
        assert_eq!(query.character_id, CHARACTER_ID);
        assert_eq!(model.phase(), ResolutionHoldClientPhase::Querying);
        assert_eq!(
            model.begin_hall_query(&hello(true), CHARACTER_ID, 2),
            Err(ResolutionHoldClientError::QueryInFlight)
        );
        assert_eq!(
            model.apply_query_result(&stored_query(
                2,
                vec![stack(
                    "core-r1",
                    Some(ResolutionHoldDestinationV1::Vault { slot_index: 7 }),
                )],
            )),
            Err(ResolutionHoldClientError::StaleOrForeignResult)
        );
        assert_eq!(
            model
                .apply_query_result(&stored_query(
                    1,
                    vec![stack(
                        "core-r1",
                        Some(ResolutionHoldDestinationV1::Vault { slot_index: 7 }),
                    )],
                ))
                .unwrap(),
            ResolutionHoldApplyOutcome::Ready { stack_count: 1 }
        );
        assert_eq!(model.phase(), ResolutionHoldClientPhase::Ready);
        assert!(model.captures_input());
        assert_eq!(model.selected_stack().unwrap().extraction_id, EXTRACTION_ID);

        let mut mismatch = ResolutionHoldClientModel::new(WireText::new("core-r1").unwrap());
        mismatch
            .begin_hall_query(&hello(true), CHARACTER_ID, 1)
            .unwrap();
        assert_eq!(
            mismatch.apply_query_result(&stored_query(1, vec![stack("foreign-r9", None)])),
            Err(ResolutionHoldClientError::ContentProjectionMismatch)
        );
        assert_eq!(mismatch.phase(), ResolutionHoldClientPhase::FatalError);
    }

    #[test]
    fn move_binds_queried_authority_and_retries_the_exact_frame() {
        let mut model = ready_model();
        let frame = model.begin_move(2, MUTATION_ID, 1_500).unwrap();
        assert_eq!(frame.payload.action, ResolutionHoldActionV1::Move);
        assert_eq!(frame.payload.expected_versions, versions());
        assert_eq!(frame.payload.content_revision.as_str(), "core-r1");
        assert_eq!(frame.payload.expected_stack_digest, STACK_DIGEST);
        assert_eq!(frame.payload_hash, frame.payload.canonical_hash());
        assert_eq!(model.phase(), ResolutionHoldClientPhase::Submitting);

        model.transport_lost();
        assert_eq!(
            model.retry_directive(),
            ResolutionHoldRetryDirective::RetryExactMutation
        );
        assert_eq!(
            model.retry_exact_mutation(),
            Err(ResolutionHoldClientError::FeatureNotNegotiated)
        );
        model.accept_server_hello(&hello(true)).unwrap();
        assert_eq!(model.retry_exact_mutation().unwrap(), frame);

        model
            .apply_mutation_result(&rejected_mutation(
                2,
                ResolutionHoldRejectionCodeV1::DatabaseUnavailable,
            ))
            .unwrap();
        assert_eq!(model.phase(), ResolutionHoldClientPhase::RecoverableError);
        assert_eq!(model.in_flight_mutation(), Some(&frame));
        assert_eq!(model.retry_exact_mutation().unwrap(), frame);
    }

    #[test]
    fn destructive_action_requires_a_separate_explicit_confirmation() {
        let mut model = ready_model();
        model.request_destroy_confirmation().unwrap();
        assert_eq!(model.phase(), ResolutionHoldClientPhase::ConfirmDestroy);
        assert!(model.in_flight_mutation().is_none());
        model.cancel_destroy_confirmation().unwrap();
        assert_eq!(model.phase(), ResolutionHoldClientPhase::Ready);

        model.request_destroy_confirmation().unwrap();
        let frame = model.confirm_destroy(2, MUTATION_ID, 1_500).unwrap();
        assert_eq!(
            frame.payload.action,
            ResolutionHoldActionV1::DestroyConfirmed
        );
        assert_eq!(model.phase(), ResolutionHoldClientPhase::Submitting);
    }

    #[test]
    fn stored_replay_refreshes_before_final_input_release() {
        let mut model = ready_model();
        model.begin_move(2, MUTATION_ID, 1_500).unwrap();
        assert_eq!(
            model
                .apply_mutation_result(&stored_move_result(2, true))
                .unwrap(),
            ResolutionHoldApplyOutcome::MutationStored {
                replayed: true,
                remaining_hold_stack_count: 0,
            }
        );
        assert_eq!(model.phase(), ResolutionHoldClientPhase::Refreshing);
        assert!(model.captures_input());
        assert!(model.last_stored_result().is_some());

        let refresh = model.begin_refresh_query(3).unwrap();
        assert_eq!(refresh.character_id, CHARACTER_ID);
        assert_eq!(
            model
                .apply_query_result(&stored_query(3, Vec::new()))
                .unwrap(),
            ResolutionHoldApplyOutcome::Resolved
        );
        assert_eq!(model.phase(), ResolutionHoldClientPhase::Resolved);
        assert!(!model.captures_input());
    }

    #[test]
    fn every_rejection_has_an_explicit_fail_closed_retry_policy() {
        let expected = [
            (
                ResolutionHoldRejectionCodeV1::FeatureDisabled,
                ResolutionHoldRetryDirective::Unavailable,
            ),
            (
                ResolutionHoldRejectionCodeV1::InvalidRequest,
                ResolutionHoldRetryDirective::Unavailable,
            ),
            (
                ResolutionHoldRejectionCodeV1::IssuedAtInvalid,
                ResolutionHoldRetryDirective::CorrectClock,
            ),
            (
                ResolutionHoldRejectionCodeV1::ContentMismatch,
                ResolutionHoldRetryDirective::Unavailable,
            ),
            (
                ResolutionHoldRejectionCodeV1::StaleAuthority,
                ResolutionHoldRetryDirective::RefreshAuthority,
            ),
            (
                ResolutionHoldRejectionCodeV1::ForeignAuthority,
                ResolutionHoldRetryDirective::Unavailable,
            ),
            (
                ResolutionHoldRejectionCodeV1::HallBindingRequired,
                ResolutionHoldRetryDirective::WaitForHall,
            ),
            (
                ResolutionHoldRejectionCodeV1::StorageFull,
                ResolutionHoldRetryDirective::RefreshAuthority,
            ),
            (
                ResolutionHoldRejectionCodeV1::NoHeldStack,
                ResolutionHoldRetryDirective::RefreshAuthority,
            ),
            (
                ResolutionHoldRejectionCodeV1::ConfirmationRequired,
                ResolutionHoldRetryDirective::Unavailable,
            ),
            (
                ResolutionHoldRejectionCodeV1::IdempotencyConflict,
                ResolutionHoldRetryDirective::Unavailable,
            ),
            (
                ResolutionHoldRejectionCodeV1::DatabaseUnavailable,
                ResolutionHoldRetryDirective::RetryExactMutation,
            ),
            (
                ResolutionHoldRejectionCodeV1::CorruptStoredAuthority,
                ResolutionHoldRetryDirective::Unavailable,
            ),
            (
                ResolutionHoldRejectionCodeV1::UnresolvedMutation,
                ResolutionHoldRetryDirective::RefreshAuthority,
            ),
        ];
        for (code, directive) in expected {
            assert_eq!(mutation_retry_policy(code), directive, "{code:?}");
        }
    }
}
