//! Authenticated M03 successor recovery service.
//!
//! Authorities: `Gravebound_Production_GDD_v1_Canonical.md` `DTH-020`/`021`,
//! `UI-007`-`009`, and `TECH-021`-`023`; `Gravebound_Content_Production_Spec_v1.md`
//! `CONT-CATALOG-003`; `Gravebound_Development_Roadmap_v1.md` `GB-M03-07`; and accepted
//! `SPEC-CONFLICT-031`. The authenticated client contributes only one death identity, mutation
//! identity, and negotiated content revision. Every successor aggregate identity, starter grant,
//! version, and selected-character result remains server-owned.

use std::future::Future;

use persistence::{
    CORE_SUCCESSOR_BASE_SILHOUETTE_ID, PersistenceError, PostgresPersistence,
    SUCCESSOR_CONTRACT_VERSION_V1, StoredSuccessorAppearanceV1,
    StoredSuccessorResultV1 as PersistenceStoredSuccessorResultV1, SuccessorCreateRequestV1,
    SuccessorCreateTransactionV1, WIPEABLE_CORE_NAMESPACE, derive_successor_character_id_v1,
    derive_successor_receipt_id_v1,
};
use protocol::{
    SUCCESSOR_SCHEMA_VERSION, StoredSuccessorResultV1 as WireStoredSuccessorResultV1,
    SuccessorAppearanceSnapshotV1, SuccessorCreateFrameV1, SuccessorCreateResultV1,
    SuccessorRejectionCodeV1, SuccessorStarterItemsV1, SuccessorVersionVectorV1, WireText,
};
use thiserror::Error;

use crate::{AuthenticatedAccount, AuthenticatedNamespace, starter_items::StarterItemPlan};

pub trait SuccessorRepository: Send + Sync {
    fn create_successor(
        &self,
        request: &SuccessorCreateRequestV1,
    ) -> impl Future<Output = Result<SuccessorCreateTransactionV1, PersistenceError>> + Send;
}

impl SuccessorRepository for PostgresPersistence {
    async fn create_successor(
        &self,
        request: &SuccessorCreateRequestV1,
    ) -> Result<SuccessorCreateTransactionV1, PersistenceError> {
        self.create_successor_v1(request).await
    }
}

#[derive(Debug, Clone)]
pub struct SuccessorService<Repository> {
    repository: Repository,
}

impl<Repository> SuccessorService<Repository> {
    #[must_use]
    pub const fn new(repository: Repository) -> Self {
        Self { repository }
    }
}

pub type PostgresSuccessorService = SuccessorService<PostgresPersistence>;

#[derive(Debug, Clone)]
pub enum CoreSuccessorAuthority {
    Disabled,
    Persistent(PostgresSuccessorService),
}

pub trait CoreSuccessorIntentAuthority: Send + Sync {
    fn handle_successor_create<'a>(
        &'a self,
        authenticated: AuthenticatedAccount,
        frame: &'a SuccessorCreateFrameV1,
    ) -> impl Future<Output = SuccessorCreateResultV1> + Send + 'a;
}

impl CoreSuccessorIntentAuthority for CoreSuccessorAuthority {
    #[allow(
        clippy::manual_async_fn,
        reason = "the desugared public trait contract guarantees Send futures for spawned QUIC workers"
    )]
    fn handle_successor_create<'a>(
        &'a self,
        authenticated: AuthenticatedAccount,
        frame: &'a SuccessorCreateFrameV1,
    ) -> impl Future<Output = SuccessorCreateResultV1> + Send + 'a {
        async move { self.create(authenticated, frame).await }
    }
}

impl CoreSuccessorAuthority {
    #[must_use]
    pub const fn disabled() -> Self {
        Self::Disabled
    }

    #[must_use]
    pub const fn persistent(service: PostgresSuccessorService) -> Self {
        Self::Persistent(service)
    }

    pub async fn create(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &SuccessorCreateFrameV1,
    ) -> SuccessorCreateResultV1 {
        let result = match frame.validate() {
            Err(_) => Err(SuccessorServiceError::InvalidRequest),
            Ok(()) if authenticated.namespace != AuthenticatedNamespace::WipeableTest => {
                Err(SuccessorServiceError::ForeignAuthority)
            }
            Ok(()) => match self {
                Self::Disabled => Err(SuccessorServiceError::FeatureDisabled),
                Self::Persistent(service) => {
                    service
                        .create_frame(authenticated.account_id.as_bytes(), frame)
                        .await
                }
            },
        };
        result.unwrap_or_else(|error| rejected(frame, error.rejection_code()))
    }
}

impl<Repository> SuccessorService<Repository>
where
    Repository: SuccessorRepository,
{
    pub async fn create_frame(
        &self,
        account_id: [u8; 16],
        frame: &SuccessorCreateFrameV1,
    ) -> Result<SuccessorCreateResultV1, SuccessorServiceError> {
        frame
            .validate()
            .map_err(|_| SuccessorServiceError::InvalidRequest)?;
        if !is_well_formed_core_content_revision(frame.payload.content_revision.as_str()) {
            return Err(SuccessorServiceError::InvalidRequest);
        }
        let request = successor_request(account_id, frame)?;
        let transaction = self
            .repository
            .create_successor(&request)
            .await
            .map_err(|error| map_persistence(&error))?;
        match transaction {
            SuccessorCreateTransactionV1::Fresh(stored) => {
                project_stored(frame.sequence, account_id, frame, false, &stored)
            }
            SuccessorCreateTransactionV1::Replayed(stored) => {
                project_stored(frame.sequence, account_id, frame, true, &stored)
            }
            SuccessorCreateTransactionV1::Conflict {
                stored_mutation_id,
                stored_death_id,
            } if stored_mutation_id == frame.mutation_id && stored_death_id != [0; 16] => {
                Err(SuccessorServiceError::IdempotencyConflict)
            }
            SuccessorCreateTransactionV1::Conflict { .. } => {
                Err(SuccessorServiceError::CorruptStoredAuthority)
            }
        }
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum SuccessorServiceError {
    #[error("successor request is malformed")]
    InvalidRequest,
    #[error("successor capability is disabled")]
    FeatureDisabled,
    #[error("successor content authority changed")]
    ContentMismatch,
    #[error("successor death authority belongs to another account")]
    ForeignAuthority,
    #[error("successor death authority does not exist")]
    DeathNotFound,
    #[error("death is not legal terminal successor authority")]
    DeathNotTerminal,
    #[error("successor death authority was superseded")]
    DeathSuperseded,
    #[error("successor death authority was already consumed")]
    AlreadyConsumed,
    #[error("successor reserved roster ordinal conflicts with stored authority")]
    SlotConflict,
    #[error("successor mutation identity conflicts with stored material")]
    IdempotencyConflict,
    #[error("successor creation is blocked by unresolved recovery authority")]
    UnresolvedMutation,
    #[error("successor persistence is unavailable")]
    DatabaseUnavailable,
    #[error("stored successor authority is corrupt")]
    CorruptStoredAuthority,
}

impl SuccessorServiceError {
    #[must_use]
    pub const fn rejection_code(self) -> SuccessorRejectionCodeV1 {
        match self {
            Self::InvalidRequest => SuccessorRejectionCodeV1::InvalidRequest,
            Self::FeatureDisabled => SuccessorRejectionCodeV1::FeatureDisabled,
            Self::ContentMismatch => SuccessorRejectionCodeV1::ContentMismatch,
            Self::ForeignAuthority => SuccessorRejectionCodeV1::ForeignAuthority,
            Self::DeathNotFound => SuccessorRejectionCodeV1::DeathNotFound,
            Self::DeathNotTerminal => SuccessorRejectionCodeV1::DeathNotTerminal,
            Self::DeathSuperseded => SuccessorRejectionCodeV1::DeathSuperseded,
            Self::AlreadyConsumed => SuccessorRejectionCodeV1::AlreadyConsumed,
            Self::SlotConflict => SuccessorRejectionCodeV1::SlotConflict,
            Self::IdempotencyConflict => SuccessorRejectionCodeV1::IdempotencyConflict,
            Self::UnresolvedMutation => SuccessorRejectionCodeV1::UnresolvedMutation,
            Self::DatabaseUnavailable => SuccessorRejectionCodeV1::DatabaseUnavailable,
            Self::CorruptStoredAuthority => SuccessorRejectionCodeV1::CorruptStoredAuthority,
        }
    }
}

fn successor_request(
    account_id: [u8; 16],
    frame: &SuccessorCreateFrameV1,
) -> Result<SuccessorCreateRequestV1, SuccessorServiceError> {
    let successor_id =
        derive_successor_character_id_v1(account_id, frame.payload.death_id, frame.mutation_id);
    let receipt_id = derive_successor_receipt_id_v1(
        account_id,
        frame.payload.death_id,
        frame.mutation_id,
        successor_id,
    );
    let starter = StarterItemPlan::for_character(successor_id)
        .map_err(|_| SuccessorServiceError::CorruptStoredAuthority)?;
    let request = SuccessorCreateRequestV1 {
        contract_version: SUCCESSOR_CONTRACT_VERSION_V1,
        namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
        account_id,
        mutation_id: frame.mutation_id,
        death_id: frame.payload.death_id,
        successor_id,
        receipt_id,
        canonical_request_hash: frame.payload_hash,
        content_revision: frame.payload.content_revision.as_str().into(),
        starter_request_hash: starter.request_hash,
        starter_result_hash: starter.result_hash,
        starter_items: starter.items,
    };
    Ok(request)
}

fn project_stored(
    request_sequence: u32,
    account_id: [u8; 16],
    frame: &SuccessorCreateFrameV1,
    replayed: bool,
    stored: &PersistenceStoredSuccessorResultV1,
) -> Result<SuccessorCreateResultV1, SuccessorServiceError> {
    stored
        .validate()
        .map_err(|_| SuccessorServiceError::CorruptStoredAuthority)?;
    let expected_successor =
        derive_successor_character_id_v1(account_id, frame.payload.death_id, frame.mutation_id);
    let expected_receipt = derive_successor_receipt_id_v1(
        account_id,
        frame.payload.death_id,
        frame.mutation_id,
        expected_successor,
    );
    let expected_starter = StarterItemPlan::for_character(expected_successor)
        .map_err(|_| SuccessorServiceError::CorruptStoredAuthority)?;
    let expected_starter_uids: Vec<_> = expected_starter
        .items
        .iter()
        .map(|item| item.item_uid)
        .collect();
    if stored.namespace_id != WIPEABLE_CORE_NAMESPACE
        || stored.account_id != account_id
        || stored.mutation_id != frame.mutation_id
        || stored.death_id != frame.payload.death_id
        || stored.successor_id != expected_successor
        || stored.selected_character_id != expected_successor
        || stored.receipt_id != expected_receipt
        || stored.canonical_request_hash != frame.payload_hash
        || stored.content_revision != frame.payload.content_revision.as_str()
        || stored.starter_items.ordered_uids().as_slice() != expected_starter_uids.as_slice()
        || stored.base_silhouette_id != CORE_SUCCESSOR_BASE_SILHOUETTE_ID
    {
        return Err(SuccessorServiceError::CorruptStoredAuthority);
    }
    let wire = WireStoredSuccessorResultV1 {
        mutation_id: stored.mutation_id,
        death_id: stored.death_id,
        successor_id: stored.successor_id,
        receipt_id: stored.receipt_id,
        former_roster_ordinal: stored.former_roster_ordinal,
        class_id: WireText::new(stored.class_id.clone())
            .map_err(|_| SuccessorServiceError::CorruptStoredAuthority)?,
        appearance: match stored.appearance {
            StoredSuccessorAppearanceV1::CoreBaseSilhouette => {
                SuccessorAppearanceSnapshotV1::CoreBaseSilhouette
            }
        },
        starter_items: SuccessorStarterItemsV1 {
            weapon_uid: stored.starter_items.weapon_uid,
            relic_uid: stored.starter_items.relic_uid,
            tonic_unit_uids: stored.starter_items.tonic_unit_uids,
        },
        versions: SuccessorVersionVectorV1 {
            account: stored.versions.account,
            character: stored.versions.character,
            progression: stored.versions.progression,
            world: stored.versions.world,
            inventory: stored.versions.inventory,
            life_metrics: stored.versions.life_metrics,
            oath_bargain: stored.versions.oath_bargain,
        },
        content_revision: WireText::new(stored.content_revision.clone())
            .map_err(|_| SuccessorServiceError::CorruptStoredAuthority)?,
        selected_character_id: stored.selected_character_id,
        result_hash: stored.result_hash,
    };
    wire.validate()
        .map_err(|_| SuccessorServiceError::CorruptStoredAuthority)?;
    let result = SuccessorCreateResultV1::Stored {
        schema_version: SUCCESSOR_SCHEMA_VERSION,
        request_sequence,
        replayed,
        result: Box::new(wire),
    };
    result
        .validate()
        .map_err(|_| SuccessorServiceError::CorruptStoredAuthority)?;
    Ok(result)
}

fn map_persistence(error: &PersistenceError) -> SuccessorServiceError {
    if error.may_have_ambiguous_commit_outcome() {
        return SuccessorServiceError::UnresolvedMutation;
    }
    match error {
        PersistenceError::SuccessorContentMismatch => SuccessorServiceError::ContentMismatch,
        PersistenceError::SuccessorForeignAuthority => SuccessorServiceError::ForeignAuthority,
        PersistenceError::SuccessorDeathNotFound => SuccessorServiceError::DeathNotFound,
        PersistenceError::SuccessorDeathNotTerminal => SuccessorServiceError::DeathNotTerminal,
        PersistenceError::SuccessorDeathSuperseded => SuccessorServiceError::DeathSuperseded,
        PersistenceError::SuccessorAlreadyConsumed => SuccessorServiceError::AlreadyConsumed,
        PersistenceError::SuccessorSlotConflict => SuccessorServiceError::SlotConflict,
        PersistenceError::SuccessorIdempotencyConflict => {
            SuccessorServiceError::IdempotencyConflict
        }
        PersistenceError::SuccessorResolutionRequired => SuccessorServiceError::UnresolvedMutation,
        PersistenceError::CorruptStoredSuccessor
        | PersistenceError::CorruptStoredItems
        | PersistenceError::ItemCharacterNotFound
        | PersistenceError::ItemIdempotencyConflict => {
            SuccessorServiceError::CorruptStoredAuthority
        }
        _ => SuccessorServiceError::DatabaseUnavailable,
    }
}

fn rejected(
    frame: &SuccessorCreateFrameV1,
    code: SuccessorRejectionCodeV1,
) -> SuccessorCreateResultV1 {
    SuccessorCreateResultV1::Rejected {
        schema_version: SUCCESSOR_SCHEMA_VERSION,
        request_sequence: frame.sequence.max(1),
        mutation_id: nonzero_id(frame.mutation_id),
        death_id: nonzero_id(frame.payload.death_id),
        code,
    }
}

fn nonzero_id(mut id: [u8; 16]) -> [u8; 16] {
    if id == [0; 16] {
        id[15] = 1;
    }
    id
}

fn is_well_formed_core_content_revision(value: &str) -> bool {
    value
        .strip_prefix("core-dev.blake3.")
        .is_some_and(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use persistence::{
        CORE_ITEM_CONTENT_REVISION, CORE_SUCCESSOR_CLASS_ID, DurableSuccessorPresetV1,
        SUCCESSOR_APPEARANCE_KIND_CORE_BASE_SILHOUETTE,
    };
    use protocol::{SUCCESSOR_SCHEMA_VERSION, SuccessorCreatePayloadV1};

    use super::*;
    use crate::AccountId;

    const ACCOUNT: [u8; 16] = [1; 16];
    const MUTATION: [u8; 16] = [2; 16];
    const DEATH: [u8; 16] = [3; 16];

    #[derive(Debug, Clone, Copy)]
    enum RepositoryOutcome {
        Fresh,
        Replayed,
        Conflict,
    }

    #[derive(Clone)]
    struct RecordingRepository {
        outcome: RepositoryOutcome,
        request: Arc<Mutex<Option<SuccessorCreateRequestV1>>>,
    }

    impl SuccessorRepository for RecordingRepository {
        async fn create_successor(
            &self,
            request: &SuccessorCreateRequestV1,
        ) -> Result<SuccessorCreateTransactionV1, PersistenceError> {
            *self.request.lock().unwrap() = Some(request.clone());
            match self.outcome {
                RepositoryOutcome::Fresh => {
                    Ok(SuccessorCreateTransactionV1::Fresh(stored_result(request)))
                }
                RepositoryOutcome::Replayed => Ok(SuccessorCreateTransactionV1::Replayed(
                    stored_result(request),
                )),
                RepositoryOutcome::Conflict => Ok(SuccessorCreateTransactionV1::Conflict {
                    stored_mutation_id: request.mutation_id,
                    stored_death_id: [9; 16],
                }),
            }
        }
    }

    fn recording_repository(outcome: RepositoryOutcome) -> RecordingRepository {
        RecordingRepository {
            outcome,
            request: Arc::new(Mutex::new(None)),
        }
    }

    fn frame() -> SuccessorCreateFrameV1 {
        let payload = SuccessorCreatePayloadV1 {
            death_id: DEATH,
            content_revision: WireText::new(CORE_ITEM_CONTENT_REVISION).unwrap(),
        };
        SuccessorCreateFrameV1 {
            schema_version: SUCCESSOR_SCHEMA_VERSION,
            sequence: 17,
            mutation_id: MUTATION,
            payload_hash: payload.canonical_hash(),
            payload,
        }
    }

    fn preset(request: &SuccessorCreateRequestV1) -> DurableSuccessorPresetV1 {
        let mut preset = DurableSuccessorPresetV1 {
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            account_id: request.account_id,
            former_character_id: [8; 16],
            death_id: request.death_id,
            former_roster_ordinal: 1,
            class_id: CORE_SUCCESSOR_CLASS_ID.into(),
            appearance_kind: SUCCESSOR_APPEARANCE_KIND_CORE_BASE_SILHOUETTE,
            base_silhouette_id: CORE_SUCCESSOR_BASE_SILHOUETTE_ID.into(),
            content_revision: request.content_revision.clone(),
            created_at_unix_ms: 1,
            preset_hash: [0; 32],
        };
        preset.preset_hash = preset.expected_hash().unwrap();
        preset
    }

    fn stored_result(request: &SuccessorCreateRequestV1) -> PersistenceStoredSuccessorResultV1 {
        PersistenceStoredSuccessorResultV1::from_request(request, &preset(request), 7).unwrap()
    }

    fn authenticated(namespace: AuthenticatedNamespace) -> AuthenticatedAccount {
        AuthenticatedAccount {
            account_id: AccountId::new(ACCOUNT).unwrap(),
            namespace,
        }
    }

    #[tokio::test]
    async fn service_derives_every_identity_starter_and_hash_from_authenticated_authority() {
        let repository = recording_repository(RepositoryOutcome::Fresh);
        let recorded = Arc::clone(&repository.request);
        let result = SuccessorService::new(repository)
            .create_frame(ACCOUNT, &frame())
            .await
            .unwrap();
        let SuccessorCreateResultV1::Stored {
            replayed, result, ..
        } = result
        else {
            panic!("fresh successor must project stored authority");
        };
        assert!(!replayed);
        assert_eq!(result.mutation_id, MUTATION);
        assert_eq!(result.death_id, DEATH);
        assert_eq!(result.selected_character_id, result.successor_id);

        let request = recorded.lock().unwrap().clone().unwrap();
        assert_eq!(request.account_id, ACCOUNT);
        assert_eq!(
            request.successor_id,
            derive_successor_character_id_v1(ACCOUNT, DEATH, MUTATION)
        );
        assert_eq!(request.canonical_request_hash, frame().payload_hash);
        assert_eq!(request.starter_items.len(), 4);
        request.validate().unwrap();
    }

    #[tokio::test]
    async fn replay_and_conflict_are_distinct_typed_outcomes() {
        let replay = SuccessorService::new(recording_repository(RepositoryOutcome::Replayed))
            .create_frame(ACCOUNT, &frame())
            .await
            .unwrap();
        assert!(matches!(
            replay,
            SuccessorCreateResultV1::Stored { replayed: true, .. }
        ));

        let conflict = SuccessorService::new(recording_repository(RepositoryOutcome::Conflict))
            .create_frame(ACCOUNT, &frame())
            .await
            .unwrap_err();
        assert_eq!(conflict, SuccessorServiceError::IdempotencyConflict);
    }

    #[test]
    fn projection_rejects_repository_material_from_another_account_or_request() {
        let frame = frame();
        let request = successor_request(ACCOUNT, &frame).unwrap();
        let foreign_request = successor_request([9; 16], &frame).unwrap();
        let stored = stored_result(&foreign_request);
        assert_eq!(
            project_stored(frame.sequence, ACCOUNT, &frame, false, &stored),
            Err(SuccessorServiceError::CorruptStoredAuthority)
        );

        let mut other_mutation = frame.clone();
        other_mutation.mutation_id = [10; 16];
        let other_request = successor_request(ACCOUNT, &other_mutation).unwrap();
        let stored = stored_result(&other_request);
        assert_eq!(
            project_stored(frame.sequence, ACCOUNT, &frame, false, &stored),
            Err(SuccessorServiceError::CorruptStoredAuthority)
        );

        let mut other_death = frame.clone();
        other_death.payload.death_id = [11; 16];
        other_death.payload_hash = other_death.payload.canonical_hash();
        let other_request = successor_request(ACCOUNT, &other_death).unwrap();
        let stored = stored_result(&other_request);
        assert_eq!(
            project_stored(frame.sequence, ACCOUNT, &frame, false, &stored),
            Err(SuccessorServiceError::CorruptStoredAuthority)
        );

        let mut stored = stored_result(&request);
        stored.canonical_request_hash[0] ^= 1;
        assert_eq!(
            project_stored(frame.sequence, ACCOUNT, &frame, false, &stored),
            Err(SuccessorServiceError::CorruptStoredAuthority)
        );

        let mut stored = stored_result(&request);
        stored.selected_character_id[0] ^= 1;
        assert_eq!(
            project_stored(frame.sequence, ACCOUNT, &frame, false, &stored),
            Err(SuccessorServiceError::CorruptStoredAuthority)
        );

        let mut stored = stored_result(&request);
        stored.starter_items.tonic_unit_uids[1][0] ^= 1;
        assert_eq!(
            project_stored(frame.sequence, ACCOUNT, &frame, false, &stored),
            Err(SuccessorServiceError::CorruptStoredAuthority)
        );

        let mut stored = stored_result(&request);
        stored.versions.inventory = 1;
        assert_eq!(
            project_stored(frame.sequence, ACCOUNT, &frame, false, &stored),
            Err(SuccessorServiceError::CorruptStoredAuthority)
        );

        let mut stored = stored_result(&request);
        stored.result_hash[0] ^= 1;
        assert_eq!(
            project_stored(frame.sequence, ACCOUNT, &frame, false, &stored),
            Err(SuccessorServiceError::CorruptStoredAuthority)
        );
    }

    #[tokio::test]
    async fn malformed_content_and_frame_fail_before_repository_access() {
        let repository = recording_repository(RepositoryOutcome::Fresh);
        let recorded = Arc::clone(&repository.request);
        let mut malformed = frame();
        malformed.payload.content_revision = WireText::new("core-dev.blake3.not-a-digest").unwrap();
        malformed.payload_hash = malformed.payload.canonical_hash();
        assert_eq!(
            SuccessorService::new(repository)
                .create_frame(ACCOUNT, &malformed)
                .await,
            Err(SuccessorServiceError::InvalidRequest)
        );
        assert!(recorded.lock().unwrap().is_none());

        let repository = recording_repository(RepositoryOutcome::Fresh);
        let recorded = Arc::clone(&repository.request);
        let mut invalid = frame();
        invalid.payload_hash[0] ^= 1;
        assert_eq!(
            SuccessorService::new(repository)
                .create_frame(ACCOUNT, &invalid)
                .await,
            Err(SuccessorServiceError::InvalidRequest)
        );
        assert!(recorded.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn disabled_authority_validates_before_feature_and_namespace_state() {
        let disabled = CoreSuccessorAuthority::disabled();
        let result = disabled
            .create(
                authenticated(AuthenticatedNamespace::WipeableTest),
                &frame(),
            )
            .await;
        assert!(matches!(
            result,
            SuccessorCreateResultV1::Rejected {
                code: SuccessorRejectionCodeV1::FeatureDisabled,
                ..
            }
        ));

        let mut invalid = frame();
        invalid.payload_hash[0] ^= 1;
        let result = disabled
            .create(
                authenticated(AuthenticatedNamespace::WipeableTest),
                &invalid,
            )
            .await;
        assert!(matches!(
            result,
            SuccessorCreateResultV1::Rejected {
                code: SuccessorRejectionCodeV1::InvalidRequest,
                ..
            }
        ));
        result.validate().unwrap();

        let result = disabled
            .create(authenticated(AuthenticatedNamespace::Production), &frame())
            .await;
        assert!(matches!(
            result,
            SuccessorCreateResultV1::Rejected {
                code: SuccessorRejectionCodeV1::ForeignAuthority,
                ..
            }
        ));
    }

    #[test]
    fn persistence_errors_map_to_stable_fail_closed_codes() {
        let cases = [
            (
                PersistenceError::SuccessorContentMismatch,
                SuccessorServiceError::ContentMismatch,
            ),
            (
                PersistenceError::SuccessorForeignAuthority,
                SuccessorServiceError::ForeignAuthority,
            ),
            (
                PersistenceError::SuccessorDeathNotFound,
                SuccessorServiceError::DeathNotFound,
            ),
            (
                PersistenceError::SuccessorDeathNotTerminal,
                SuccessorServiceError::DeathNotTerminal,
            ),
            (
                PersistenceError::SuccessorDeathSuperseded,
                SuccessorServiceError::DeathSuperseded,
            ),
            (
                PersistenceError::SuccessorAlreadyConsumed,
                SuccessorServiceError::AlreadyConsumed,
            ),
            (
                PersistenceError::SuccessorSlotConflict,
                SuccessorServiceError::SlotConflict,
            ),
            (
                PersistenceError::SuccessorIdempotencyConflict,
                SuccessorServiceError::IdempotencyConflict,
            ),
            (
                PersistenceError::SuccessorResolutionRequired,
                SuccessorServiceError::UnresolvedMutation,
            ),
            (
                PersistenceError::CorruptStoredSuccessor,
                SuccessorServiceError::CorruptStoredAuthority,
            ),
            (
                PersistenceError::CorruptStoredItems,
                SuccessorServiceError::CorruptStoredAuthority,
            ),
            (
                PersistenceError::ItemCharacterNotFound,
                SuccessorServiceError::CorruptStoredAuthority,
            ),
            (
                PersistenceError::ItemIdempotencyConflict,
                SuccessorServiceError::CorruptStoredAuthority,
            ),
        ];
        for (persistence, service) in cases {
            assert_eq!(map_persistence(&persistence), service);
        }

        let ambiguous = PersistenceError::Database(sqlx::Error::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "lost successor commit acknowledgement",
        )));
        assert_eq!(
            map_persistence(&ambiguous),
            SuccessorServiceError::UnresolvedMutation
        );
        assert_eq!(
            map_persistence(&PersistenceError::Database(sqlx::Error::PoolTimedOut)),
            SuccessorServiceError::DatabaseUnavailable
        );
    }
}
