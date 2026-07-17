//! Authenticated Hall service for minimum M03 `ResolutionHold` recovery.
//!
//! Authorities: `Gravebound_Production_GDD_v1_Canonical.md` `DTH-011`, `LOOT-002/050/060`,
//! and `TECH-021`-`023`; `Gravebound_Content_Production_Spec_v1.md` `CONT-HUB-001/002`;
//! `Gravebound_Development_Roadmap_v1.md` `GB-M03-03/08`; and accepted
//! `SPEC-CONFLICT-029/030`. The service projects bounded stored authority and never permits the
//! client to author destinations, item lists, post versions, or final-clear state.

use std::future::Future;

use persistence::{
    PersistenceError, PostgresPersistence, RESOLUTION_HOLD_CONTRACT_VERSION_V1,
    ResolutionHoldMutationRequestV1, ResolutionHoldMutationTransactionV1,
    StoredResolutionHoldActionV1, StoredResolutionHoldDestinationV1,
    StoredResolutionHoldDispositionV1, StoredResolutionHoldItemKindV1,
    StoredResolutionHoldMutationResultV1, StoredResolutionHoldSnapshotV1,
    StoredResolutionHoldVersionAdvanceV1, StoredResolutionHoldVersionVectorV1,
    StoredResolutionHoldVersionsV1, WIPEABLE_CORE_NAMESPACE,
};
use protocol::{
    RESOLUTION_HOLD_SCHEMA_VERSION, ResolutionHoldActionV1, ResolutionHoldDestinationV1,
    ResolutionHoldDispositionV1, ResolutionHoldItemKindV1, ResolutionHoldItemTransitionV1,
    ResolutionHoldItemV1, ResolutionHoldMutationFrameV1, ResolutionHoldMutationResultV1,
    ResolutionHoldQueryFrameV1, ResolutionHoldQueryResultV1, ResolutionHoldRejectionCodeV1,
    ResolutionHoldStackV1, ResolutionHoldVersionAdvanceV1, ResolutionHoldVersionVectorV1,
    ResolutionHoldVersionsV1, StoredResolutionHoldMutationResultV1 as WireStoredMutationResultV1,
    WireText,
};
use thiserror::Error;

use crate::{AuthenticatedAccount, AuthenticatedNamespace};

pub trait ResolutionHoldRepository: Send + Sync {
    fn load_snapshot(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> impl Future<Output = Result<StoredResolutionHoldSnapshotV1, PersistenceError>> + Send;

    fn commit_mutation(
        &self,
        request: &ResolutionHoldMutationRequestV1,
    ) -> impl Future<Output = Result<ResolutionHoldMutationTransactionV1, PersistenceError>> + Send;
}

impl ResolutionHoldRepository for PostgresPersistence {
    async fn load_snapshot(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> Result<StoredResolutionHoldSnapshotV1, PersistenceError> {
        self.load_resolution_hold_snapshot_v1(account_id, character_id)
            .await
    }

    async fn commit_mutation(
        &self,
        request: &ResolutionHoldMutationRequestV1,
    ) -> Result<ResolutionHoldMutationTransactionV1, PersistenceError> {
        self.commit_resolution_hold_mutation_v1(request).await
    }
}

#[derive(Debug, Clone)]
pub struct ResolutionHoldService<Repository> {
    repository: Repository,
}

impl<Repository> ResolutionHoldService<Repository> {
    #[must_use]
    pub const fn new(repository: Repository) -> Self {
        Self { repository }
    }
}

pub type PostgresResolutionHoldService = ResolutionHoldService<PostgresPersistence>;

#[derive(Debug, Clone)]
pub enum CoreResolutionHoldAuthority {
    Disabled,
    Persistent(PostgresResolutionHoldService),
}

pub trait CoreResolutionHoldIntentAuthority: Send + Sync {
    fn handle_resolution_hold_query<'a>(
        &'a self,
        authenticated: AuthenticatedAccount,
        frame: &'a ResolutionHoldQueryFrameV1,
    ) -> impl Future<Output = ResolutionHoldQueryResultV1> + Send + 'a;

    fn handle_resolution_hold_mutation<'a>(
        &'a self,
        authenticated: AuthenticatedAccount,
        frame: &'a ResolutionHoldMutationFrameV1,
    ) -> impl Future<Output = ResolutionHoldMutationResultV1> + Send + 'a;
}

impl CoreResolutionHoldIntentAuthority for CoreResolutionHoldAuthority {
    #[allow(
        clippy::manual_async_fn,
        reason = "the desugared public trait contract guarantees Send futures for spawned QUIC workers"
    )]
    fn handle_resolution_hold_query<'a>(
        &'a self,
        authenticated: AuthenticatedAccount,
        frame: &'a ResolutionHoldQueryFrameV1,
    ) -> impl Future<Output = ResolutionHoldQueryResultV1> + Send + 'a {
        async move { self.query(authenticated, frame).await }
    }

    #[allow(
        clippy::manual_async_fn,
        reason = "the desugared public trait contract guarantees Send futures for spawned QUIC workers"
    )]
    fn handle_resolution_hold_mutation<'a>(
        &'a self,
        authenticated: AuthenticatedAccount,
        frame: &'a ResolutionHoldMutationFrameV1,
    ) -> impl Future<Output = ResolutionHoldMutationResultV1> + Send + 'a {
        async move { self.mutate(authenticated, frame).await }
    }
}

impl CoreResolutionHoldAuthority {
    #[must_use]
    pub const fn disabled() -> Self {
        Self::Disabled
    }

    #[must_use]
    pub const fn persistent(service: PostgresResolutionHoldService) -> Self {
        Self::Persistent(service)
    }

    pub async fn query(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &ResolutionHoldQueryFrameV1,
    ) -> ResolutionHoldQueryResultV1 {
        let result = match frame.validate() {
            Err(_) => Err(ResolutionHoldServiceError::InvalidRequest),
            Ok(()) if authenticated.namespace != AuthenticatedNamespace::WipeableTest => {
                Err(ResolutionHoldServiceError::ForeignAuthority)
            }
            Ok(()) => match self {
                Self::Disabled => Err(ResolutionHoldServiceError::FeatureDisabled),
                Self::Persistent(service) => {
                    service
                        .query_frame(authenticated.account_id.as_bytes(), frame)
                        .await
                }
            },
        };
        result.unwrap_or_else(|error| rejected_query(frame, error.rejection_code()))
    }

    pub async fn mutate(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &ResolutionHoldMutationFrameV1,
    ) -> ResolutionHoldMutationResultV1 {
        let result = match frame.validate() {
            Err(_) => Err(ResolutionHoldServiceError::InvalidRequest),
            Ok(()) if authenticated.namespace != AuthenticatedNamespace::WipeableTest => {
                Err(ResolutionHoldServiceError::ForeignAuthority)
            }
            Ok(()) => match self {
                Self::Disabled => Err(ResolutionHoldServiceError::FeatureDisabled),
                Self::Persistent(service) => {
                    service
                        .mutate_frame(authenticated.account_id.as_bytes(), frame)
                        .await
                }
            },
        };
        result.unwrap_or_else(|error| rejected_mutation(frame, error.rejection_code()))
    }
}

impl<Repository> ResolutionHoldService<Repository>
where
    Repository: ResolutionHoldRepository,
{
    pub async fn query_frame(
        &self,
        account_id: [u8; 16],
        frame: &ResolutionHoldQueryFrameV1,
    ) -> Result<ResolutionHoldQueryResultV1, ResolutionHoldServiceError> {
        frame
            .validate()
            .map_err(|_| ResolutionHoldServiceError::InvalidRequest)?;
        let snapshot = self
            .repository
            .load_snapshot(account_id, frame.character_id)
            .await
            .map_err(|error| map_persistence(&error))?;
        project_query(frame.sequence, account_id, frame.character_id, &snapshot)
    }

    pub async fn mutate_frame(
        &self,
        account_id: [u8; 16],
        frame: &ResolutionHoldMutationFrameV1,
    ) -> Result<ResolutionHoldMutationResultV1, ResolutionHoldServiceError> {
        frame
            .validate()
            .map_err(|_| ResolutionHoldServiceError::InvalidRequest)?;
        let request = mutation_request(account_id, frame);
        let transaction = self
            .repository
            .commit_mutation(&request)
            .await
            .map_err(|error| map_persistence(&error))?;
        match transaction {
            ResolutionHoldMutationTransactionV1::Fresh(result) => {
                project_mutation(frame.sequence, account_id, frame, false, &result)
            }
            ResolutionHoldMutationTransactionV1::Replayed(result) => {
                project_mutation(frame.sequence, account_id, frame, true, &result)
            }
            ResolutionHoldMutationTransactionV1::Conflict { .. } => {
                Err(ResolutionHoldServiceError::IdempotencyConflict)
            }
        }
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionHoldServiceError {
    #[error("ResolutionHold request is malformed")]
    InvalidRequest,
    #[error("ResolutionHold capability is disabled")]
    FeatureDisabled,
    #[error("ResolutionHold issue time is invalid")]
    IssuedAtInvalid,
    #[error("ResolutionHold content authority changed")]
    ContentMismatch,
    #[error("ResolutionHold aggregate or stack authority is stale")]
    StaleAuthority,
    #[error("ResolutionHold account or selected-character authority is foreign")]
    ForeignAuthority,
    #[error("ResolutionHold recovery requires the selected living character in Lantern Halls")]
    HallBindingRequired,
    #[error("ResolutionHold has no complete legal destination")]
    StorageFull,
    #[error("ResolutionHold stack no longer exists")]
    NoHeldStack,
    #[error("ResolutionHold mutation identity conflicts with stored material")]
    IdempotencyConflict,
    #[error("ResolutionHold recovery is blocked by an unresolved mutation")]
    UnresolvedMutation,
    #[error("ResolutionHold persistence is unavailable")]
    DatabaseUnavailable,
    #[error("stored ResolutionHold authority is corrupt")]
    CorruptStoredAuthority,
}

impl ResolutionHoldServiceError {
    #[must_use]
    pub const fn rejection_code(self) -> ResolutionHoldRejectionCodeV1 {
        match self {
            Self::InvalidRequest => ResolutionHoldRejectionCodeV1::InvalidRequest,
            Self::FeatureDisabled => ResolutionHoldRejectionCodeV1::FeatureDisabled,
            Self::IssuedAtInvalid => ResolutionHoldRejectionCodeV1::IssuedAtInvalid,
            Self::ContentMismatch => ResolutionHoldRejectionCodeV1::ContentMismatch,
            Self::StaleAuthority => ResolutionHoldRejectionCodeV1::StaleAuthority,
            Self::ForeignAuthority => ResolutionHoldRejectionCodeV1::ForeignAuthority,
            Self::HallBindingRequired => ResolutionHoldRejectionCodeV1::HallBindingRequired,
            Self::StorageFull => ResolutionHoldRejectionCodeV1::StorageFull,
            Self::NoHeldStack => ResolutionHoldRejectionCodeV1::NoHeldStack,
            Self::IdempotencyConflict => ResolutionHoldRejectionCodeV1::IdempotencyConflict,
            Self::UnresolvedMutation => ResolutionHoldRejectionCodeV1::UnresolvedMutation,
            Self::DatabaseUnavailable => ResolutionHoldRejectionCodeV1::DatabaseUnavailable,
            Self::CorruptStoredAuthority => ResolutionHoldRejectionCodeV1::CorruptStoredAuthority,
        }
    }
}

fn mutation_request(
    account_id: [u8; 16],
    frame: &ResolutionHoldMutationFrameV1,
) -> ResolutionHoldMutationRequestV1 {
    ResolutionHoldMutationRequestV1 {
        contract_version: RESOLUTION_HOLD_CONTRACT_VERSION_V1,
        namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
        account_id,
        character_id: frame.character_id,
        mutation_id: frame.mutation_id,
        extraction_id: frame.payload.extraction_id,
        stack_index: frame.payload.stack_index,
        action: stored_action(frame.payload.action),
        expected_versions: StoredResolutionHoldVersionsV1 {
            account: frame.payload.expected_versions.account,
            character: frame.payload.expected_versions.character,
            world: frame.payload.expected_versions.world,
            inventory: frame.payload.expected_versions.inventory,
        },
        content_revision: frame.payload.content_revision.as_str().into(),
        expected_stack_digest: frame.payload.expected_stack_digest,
        issued_at_unix_millis: frame.issued_at_unix_millis,
    }
}

fn project_query(
    request_sequence: u32,
    account_id: [u8; 16],
    character_id: [u8; 16],
    snapshot: &StoredResolutionHoldSnapshotV1,
) -> Result<ResolutionHoldQueryResultV1, ResolutionHoldServiceError> {
    if snapshot.account_id != account_id || snapshot.character_id != character_id {
        return Err(ResolutionHoldServiceError::CorruptStoredAuthority);
    }
    let result = ResolutionHoldQueryResultV1::Stored {
        schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
        request_sequence,
        character_id,
        versions: wire_versions(snapshot.versions),
        storage_resolution_required: snapshot.storage_resolution_required,
        stacks: snapshot
            .stacks
            .iter()
            .map(wire_stack)
            .collect::<Result<Vec<_>, _>>()?,
    };
    result
        .validate()
        .map_err(|_| ResolutionHoldServiceError::CorruptStoredAuthority)?;
    Ok(result)
}

fn wire_stack(
    stack: &persistence::StoredResolutionHoldStackV1,
) -> Result<ResolutionHoldStackV1, ResolutionHoldServiceError> {
    Ok(ResolutionHoldStackV1 {
        extraction_id: stack.extraction_id,
        stack_index: stack.stack_index,
        template_id: WireText::new(stack.template_id.clone())
            .map_err(|_| ResolutionHoldServiceError::CorruptStoredAuthority)?,
        content_revision: WireText::new(stack.content_revision.clone())
            .map_err(|_| ResolutionHoldServiceError::CorruptStoredAuthority)?,
        item_kind: wire_item_kind(stack.item_kind),
        items: stack
            .items
            .iter()
            .map(|item| ResolutionHoldItemV1 {
                item_uid: item.item_uid,
                item_version: item.item_version,
            })
            .collect(),
        stack_digest: stack.stack_digest,
        extracted_at_unix_millis: stack.extracted_at_unix_millis,
        overflow_deadline_unix_millis: stack.overflow_deadline_unix_millis,
        planned_destination: stack.planned_destination.map(wire_destination),
    })
}

fn project_mutation(
    request_sequence: u32,
    account_id: [u8; 16],
    frame: &ResolutionHoldMutationFrameV1,
    replayed: bool,
    stored: &StoredResolutionHoldMutationResultV1,
) -> Result<ResolutionHoldMutationResultV1, ResolutionHoldServiceError> {
    if stored.account_id != account_id
        || stored.character_id != frame.character_id
        || stored.mutation_id != frame.mutation_id
        || stored.extraction_id != frame.payload.extraction_id
        || stored.stack_index != frame.payload.stack_index
        || stored.action != stored_action(frame.payload.action)
    {
        return Err(ResolutionHoldServiceError::CorruptStoredAuthority);
    }
    let result = ResolutionHoldMutationResultV1::Stored {
        schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
        request_sequence,
        replayed,
        result: Box::new(WireStoredMutationResultV1 {
            mutation_id: stored.mutation_id,
            character_id: stored.character_id,
            extraction_id: stored.extraction_id,
            stack_index: stored.stack_index,
            action: wire_action(stored.action),
            result_hash: stored.result_hash,
            committed_at_unix_millis: stored.committed_at_unix_millis,
            versions: wire_version_vector(stored.versions),
            transitions: stored
                .transitions
                .iter()
                .map(|transition| ResolutionHoldItemTransitionV1 {
                    ordinal: transition.ordinal,
                    item_uid: transition.item_uid,
                    item_version: transition.post_item_version,
                    disposition: match transition.disposition {
                        StoredResolutionHoldDispositionV1::Moved(destination) => {
                            ResolutionHoldDispositionV1::Moved {
                                destination: wire_destination(destination),
                            }
                        }
                        StoredResolutionHoldDispositionV1::Destroyed => {
                            ResolutionHoldDispositionV1::Destroyed
                        }
                    },
                })
                .collect(),
            remaining_hold_stack_count: stored.remaining_hold_stack_count,
            storage_resolution_required: stored.storage_resolution_required,
        }),
    };
    result
        .validate()
        .map_err(|_| ResolutionHoldServiceError::CorruptStoredAuthority)?;
    Ok(result)
}

const fn wire_versions(versions: StoredResolutionHoldVersionsV1) -> ResolutionHoldVersionsV1 {
    ResolutionHoldVersionsV1 {
        account: versions.account,
        character: versions.character,
        world: versions.world,
        inventory: versions.inventory,
    }
}

const fn wire_version_vector(
    versions: StoredResolutionHoldVersionVectorV1,
) -> ResolutionHoldVersionVectorV1 {
    ResolutionHoldVersionVectorV1 {
        account: wire_version_advance(versions.account),
        character: wire_version_advance(versions.character),
        world: wire_version_advance(versions.world),
        inventory: wire_version_advance(versions.inventory),
    }
}

const fn wire_version_advance(
    version: StoredResolutionHoldVersionAdvanceV1,
) -> ResolutionHoldVersionAdvanceV1 {
    ResolutionHoldVersionAdvanceV1 {
        before: version.pre,
        after: version.post,
    }
}

const fn wire_action(action: StoredResolutionHoldActionV1) -> ResolutionHoldActionV1 {
    match action {
        StoredResolutionHoldActionV1::Move => ResolutionHoldActionV1::Move,
        StoredResolutionHoldActionV1::DestroyConfirmed => ResolutionHoldActionV1::DestroyConfirmed,
    }
}

const fn stored_action(action: ResolutionHoldActionV1) -> StoredResolutionHoldActionV1 {
    match action {
        ResolutionHoldActionV1::Move => StoredResolutionHoldActionV1::Move,
        ResolutionHoldActionV1::DestroyConfirmed => StoredResolutionHoldActionV1::DestroyConfirmed,
    }
}

const fn wire_item_kind(kind: StoredResolutionHoldItemKindV1) -> ResolutionHoldItemKindV1 {
    match kind {
        StoredResolutionHoldItemKindV1::Equipment => ResolutionHoldItemKindV1::Equipment,
        StoredResolutionHoldItemKindV1::Consumable => ResolutionHoldItemKindV1::Consumable,
    }
}

const fn wire_destination(
    destination: StoredResolutionHoldDestinationV1,
) -> ResolutionHoldDestinationV1 {
    match destination {
        StoredResolutionHoldDestinationV1::CharacterSafe(slot_index) => {
            ResolutionHoldDestinationV1::CharacterSafe { slot_index }
        }
        StoredResolutionHoldDestinationV1::Vault(slot_index) => {
            ResolutionHoldDestinationV1::Vault { slot_index }
        }
        StoredResolutionHoldDestinationV1::Overflow(slot_index) => {
            ResolutionHoldDestinationV1::Overflow { slot_index }
        }
    }
}

fn map_persistence(error: &PersistenceError) -> ResolutionHoldServiceError {
    match error {
        PersistenceError::ResolutionHoldIssuedAtInvalid => {
            ResolutionHoldServiceError::IssuedAtInvalid
        }
        PersistenceError::ResolutionHoldContentMismatch => {
            ResolutionHoldServiceError::ContentMismatch
        }
        PersistenceError::ResolutionHoldVersionMismatch { .. }
        | PersistenceError::ResolutionHoldStackDigestMismatch => {
            ResolutionHoldServiceError::StaleAuthority
        }
        PersistenceError::ResolutionHoldOwnerNotFound => {
            ResolutionHoldServiceError::ForeignAuthority
        }
        PersistenceError::ResolutionHoldHallBindingMismatch => {
            ResolutionHoldServiceError::HallBindingRequired
        }
        PersistenceError::ResolutionHoldStorageFull => ResolutionHoldServiceError::StorageFull,
        PersistenceError::ResolutionHoldStackNotFound => ResolutionHoldServiceError::NoHeldStack,
        PersistenceError::ResolutionHoldIdempotencyConflict => {
            ResolutionHoldServiceError::IdempotencyConflict
        }
        PersistenceError::CorruptStoredResolutionHold => {
            ResolutionHoldServiceError::CorruptStoredAuthority
        }
        PersistenceError::ResolutionHoldUnresolvedMutation => {
            ResolutionHoldServiceError::UnresolvedMutation
        }
        _ => ResolutionHoldServiceError::DatabaseUnavailable,
    }
}

fn rejected_query(
    frame: &ResolutionHoldQueryFrameV1,
    code: ResolutionHoldRejectionCodeV1,
) -> ResolutionHoldQueryResultV1 {
    ResolutionHoldQueryResultV1::Rejected {
        schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
        request_sequence: frame.sequence.max(1),
        character_id: nonzero_character(frame.character_id),
        code,
    }
}

fn rejected_mutation(
    frame: &ResolutionHoldMutationFrameV1,
    code: ResolutionHoldRejectionCodeV1,
) -> ResolutionHoldMutationResultV1 {
    ResolutionHoldMutationResultV1::Rejected {
        schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
        request_sequence: frame.sequence.max(1),
        mutation_id: nonzero_id(frame.mutation_id),
        character_id: nonzero_character(frame.character_id),
        extraction_id: nonzero_id(frame.payload.extraction_id),
        stack_index: frame.payload.stack_index.min(7),
        code,
    }
}

fn nonzero_character(mut id: [u8; 16]) -> [u8; 16] {
    if id == [0; 16] {
        id[15] = 1;
    }
    id
}

fn nonzero_id(id: [u8; 16]) -> [u8; 16] {
    nonzero_character(id)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use persistence::{
        CORE_ITEM_CONTENT_REVISION, RESOLUTION_HOLD_OVERFLOW_LIFETIME_MILLIS,
        StoredResolutionHoldItemTransitionV1, StoredResolutionHoldItemV1,
        StoredResolutionHoldStackV1, canonical_resolution_hold_stack_digest_v1,
    };
    use protocol::ResolutionHoldMutationPayloadV1;

    const ACCOUNT: [u8; 16] = [1; 16];
    const CHARACTER: [u8; 16] = [2; 16];
    const EXTRACTION: [u8; 16] = [3; 16];
    const MUTATION: [u8; 16] = [4; 16];

    #[derive(Clone)]
    struct RecordingRepository {
        snapshot: StoredResolutionHoldSnapshotV1,
        mutation: ResolutionHoldMutationTransactionV1,
        query_account: Arc<Mutex<Option<[u8; 16]>>>,
        mutation_request: Arc<Mutex<Option<ResolutionHoldMutationRequestV1>>>,
    }

    impl ResolutionHoldRepository for RecordingRepository {
        async fn load_snapshot(
            &self,
            account_id: [u8; 16],
            _character_id: [u8; 16],
        ) -> Result<StoredResolutionHoldSnapshotV1, PersistenceError> {
            *self.query_account.lock().unwrap() = Some(account_id);
            Ok(self.snapshot.clone())
        }

        async fn commit_mutation(
            &self,
            request: &ResolutionHoldMutationRequestV1,
        ) -> Result<ResolutionHoldMutationTransactionV1, PersistenceError> {
            *self.mutation_request.lock().unwrap() = Some(request.clone());
            Ok(self.mutation.clone())
        }
    }

    fn recording_repository(mutation: ResolutionHoldMutationTransactionV1) -> RecordingRepository {
        RecordingRepository {
            snapshot: snapshot(),
            mutation,
            query_account: Arc::new(Mutex::new(None)),
            mutation_request: Arc::new(Mutex::new(None)),
        }
    }

    fn snapshot() -> StoredResolutionHoldSnapshotV1 {
        let mut stack = StoredResolutionHoldStackV1 {
            extraction_id: EXTRACTION,
            stack_index: 0,
            template_id: "consumable.red_tonic".into(),
            content_revision: CORE_ITEM_CONTENT_REVISION.into(),
            item_kind: StoredResolutionHoldItemKindV1::Consumable,
            items: vec![StoredResolutionHoldItemV1 {
                item_uid: [5; 16],
                item_version: 2,
            }],
            stack_digest: [0; 32],
            extracted_at_unix_millis: 1_000,
            overflow_deadline_unix_millis: 1_000 + RESOLUTION_HOLD_OVERFLOW_LIFETIME_MILLIS,
            planned_destination: Some(StoredResolutionHoldDestinationV1::Vault(12)),
        };
        stack.stack_digest = canonical_resolution_hold_stack_digest_v1(&stack).unwrap();
        StoredResolutionHoldSnapshotV1 {
            account_id: ACCOUNT,
            character_id: CHARACTER,
            versions: StoredResolutionHoldVersionsV1 {
                account: 7,
                character: 8,
                world: 8,
                inventory: 9,
            },
            storage_resolution_required: true,
            stacks: vec![stack],
        }
    }

    fn mutation_result() -> StoredResolutionHoldMutationResultV1 {
        let snapshot = snapshot();
        StoredResolutionHoldMutationResultV1 {
            contract_version: RESOLUTION_HOLD_CONTRACT_VERSION_V1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            account_id: ACCOUNT,
            character_id: CHARACTER,
            mutation_id: MUTATION,
            extraction_id: EXTRACTION,
            stack_index: 0,
            action: StoredResolutionHoldActionV1::Move,
            canonical_request_hash: [6; 32],
            expected_stack_digest: snapshot.stacks[0].stack_digest,
            result_hash: [0; 32],
            issued_at_unix_millis: 1_500,
            committed_at_unix_millis: 2_000,
            versions: StoredResolutionHoldVersionVectorV1 {
                account: StoredResolutionHoldVersionAdvanceV1 { pre: 7, post: 8 },
                character: StoredResolutionHoldVersionAdvanceV1 { pre: 8, post: 9 },
                world: StoredResolutionHoldVersionAdvanceV1 { pre: 8, post: 9 },
                inventory: StoredResolutionHoldVersionAdvanceV1 { pre: 9, post: 10 },
            },
            destination: Some(StoredResolutionHoldDestinationV1::Vault(12)),
            transitions: vec![StoredResolutionHoldItemTransitionV1 {
                ordinal: 0,
                item_uid: [5; 16],
                template_id: "consumable.red_tonic".into(),
                content_revision: CORE_ITEM_CONTENT_REVISION.into(),
                item_kind: StoredResolutionHoldItemKindV1::Consumable,
                disposition: StoredResolutionHoldDispositionV1::Moved(
                    StoredResolutionHoldDestinationV1::Vault(12),
                ),
                pre_item_version: 2,
                post_item_version: 3,
                ledger_event_id: [7; 16],
            }],
            remaining_hold_stack_count: 0,
            storage_resolution_required: false,
        }
        .seal()
        .unwrap()
    }

    fn query_frame() -> ResolutionHoldQueryFrameV1 {
        ResolutionHoldQueryFrameV1 {
            schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
            sequence: 10,
            character_id: CHARACTER,
        }
    }

    fn mutation_frame() -> ResolutionHoldMutationFrameV1 {
        let payload = ResolutionHoldMutationPayloadV1 {
            extraction_id: EXTRACTION,
            stack_index: 0,
            action: ResolutionHoldActionV1::Move,
            expected_versions: ResolutionHoldVersionsV1 {
                account: 7,
                character: 8,
                world: 8,
                inventory: 9,
            },
            content_revision: WireText::new(CORE_ITEM_CONTENT_REVISION).unwrap(),
            expected_stack_digest: snapshot().stacks[0].stack_digest,
        };
        ResolutionHoldMutationFrameV1 {
            schema_version: RESOLUTION_HOLD_SCHEMA_VERSION,
            sequence: 11,
            mutation_id: MUTATION,
            character_id: CHARACTER,
            issued_at_unix_millis: 1_500,
            payload_hash: payload.canonical_hash(),
            payload,
        }
    }

    #[test]
    fn query_projection_preserves_server_owned_preview_and_versions() {
        let result = project_query(10, ACCOUNT, CHARACTER, &snapshot()).unwrap();
        let ResolutionHoldQueryResultV1::Stored {
            versions, stacks, ..
        } = result
        else {
            panic!("query must project stored authority");
        };
        assert_eq!(versions.account, 7);
        assert_eq!(stacks[0].items[0].item_version, 2);
        assert_eq!(
            stacks[0].planned_destination,
            Some(ResolutionHoldDestinationV1::Vault { slot_index: 12 })
        );
    }

    #[test]
    fn mutation_projection_uses_post_item_versions_and_exact_replay_flag() {
        let result =
            project_mutation(11, ACCOUNT, &mutation_frame(), true, &mutation_result()).unwrap();
        let ResolutionHoldMutationResultV1::Stored {
            replayed, result, ..
        } = result
        else {
            panic!("mutation must project stored authority");
        };
        assert!(replayed);
        assert_eq!(result.transitions[0].item_version, 3);
        assert_eq!(result.versions.account.before, 7);
        assert_eq!(result.versions.account.after, 8);
    }

    #[test]
    fn mutation_request_never_uses_transport_payload_hash_as_durable_authority() {
        let frame = mutation_frame();
        let request = mutation_request(ACCOUNT, &frame);
        assert_eq!(request.account_id, ACCOUNT);
        assert_eq!(
            request.expected_stack_digest,
            frame.payload.expected_stack_digest
        );
        assert_ne!(request.canonical_hash().unwrap(), frame.payload_hash);
    }

    #[tokio::test]
    async fn service_binds_authenticated_account_and_preserves_fresh_replay_semantics() {
        let repository =
            recording_repository(ResolutionHoldMutationTransactionV1::Fresh(mutation_result()));
        let query_account = Arc::clone(&repository.query_account);
        let recorded_mutation = Arc::clone(&repository.mutation_request);
        let service = ResolutionHoldService::new(repository);

        let query = service.query_frame(ACCOUNT, &query_frame()).await.unwrap();
        assert!(matches!(query, ResolutionHoldQueryResultV1::Stored { .. }));
        assert_eq!(*query_account.lock().unwrap(), Some(ACCOUNT));

        let mutation = service
            .mutate_frame(ACCOUNT, &mutation_frame())
            .await
            .unwrap();
        assert!(matches!(
            mutation,
            ResolutionHoldMutationResultV1::Stored {
                replayed: false,
                ..
            }
        ));
        let request = recorded_mutation.lock().unwrap().clone().unwrap();
        assert_eq!(request.account_id, ACCOUNT);
        assert_eq!(request.character_id, CHARACTER);
        assert_eq!(request.mutation_id, MUTATION);
    }

    #[test]
    fn projection_rejects_repository_material_from_another_account_or_request() {
        let mut foreign_snapshot = snapshot();
        foreign_snapshot.account_id = [9; 16];
        assert_eq!(
            project_query(10, ACCOUNT, CHARACTER, &foreign_snapshot),
            Err(ResolutionHoldServiceError::CorruptStoredAuthority)
        );

        let mut wrong_result = mutation_result();
        wrong_result.extraction_id = [8; 16];
        assert_eq!(
            project_mutation(11, ACCOUNT, &mutation_frame(), false, &wrong_result),
            Err(ResolutionHoldServiceError::CorruptStoredAuthority)
        );
    }

    #[tokio::test]
    async fn service_returns_typed_conflict_without_projecting_stored_material() {
        let repository = recording_repository(ResolutionHoldMutationTransactionV1::Conflict {
            mutation_id: MUTATION,
            character_id: CHARACTER,
        });
        let error = ResolutionHoldService::new(repository)
            .mutate_frame(ACCOUNT, &mutation_frame())
            .await
            .unwrap_err();
        assert_eq!(error, ResolutionHoldServiceError::IdempotencyConflict);
    }

    #[tokio::test]
    async fn disabled_authority_validates_before_returning_feature_state() {
        let authenticated = AuthenticatedAccount {
            account_id: crate::AccountId::new(ACCOUNT).unwrap(),
            namespace: AuthenticatedNamespace::WipeableTest,
        };
        let result = CoreResolutionHoldAuthority::disabled()
            .query(authenticated, &query_frame())
            .await;
        assert!(matches!(
            result,
            ResolutionHoldQueryResultV1::Rejected {
                code: ResolutionHoldRejectionCodeV1::FeatureDisabled,
                ..
            }
        ));

        let mut invalid = mutation_frame();
        invalid.payload_hash[0] ^= 1;
        let result = CoreResolutionHoldAuthority::disabled()
            .mutate(authenticated, &invalid)
            .await;
        assert!(matches!(
            result,
            ResolutionHoldMutationResultV1::Rejected {
                code: ResolutionHoldRejectionCodeV1::InvalidRequest,
                ..
            }
        ));
        result.validate().unwrap();
    }

    #[test]
    fn persistence_errors_map_to_stable_fail_closed_codes() {
        assert_eq!(
            map_persistence(&PersistenceError::ResolutionHoldStackDigestMismatch),
            ResolutionHoldServiceError::StaleAuthority
        );
        assert_eq!(
            map_persistence(&PersistenceError::ResolutionHoldOwnerNotFound),
            ResolutionHoldServiceError::ForeignAuthority
        );
        assert_eq!(
            map_persistence(&PersistenceError::CorruptStoredResolutionHold),
            ResolutionHoldServiceError::CorruptStoredAuthority
        );
    }
}
