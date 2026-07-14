//! Wipeable GB-M03-03E extraction receipt and Hall transfer authority.
//!
//! This module cannot stabilize inventory. It consumes the schema-26 receipt seam and reuses the
//! existing serializable world-flow write only after the receipt is durably committed.

use persistence::{
    CaldusExtractionCommit, CaldusExtractionRequest, CaldusExtractionTransaction,
    CaldusExtractionTransfer, PersistenceError, PostgresPersistence, StoredExtractionAuthority,
    StoredExtractionResult, StoredSafeArrival, StoredWorldFlowRevisionV1, StoredWorldLocation,
    StoredWorldTransferReceipt, WorldFlowBegin, WorldFlowTransaction,
    stage_caldus_extraction_transfer, stage_danger_checkpoint_cleanup,
};
use protocol::{
    CharacterLocationSnapshot, WorldFlowContentRevisionV1, WorldFlowResult, WorldTransferCommand,
    WorldTransferMutation, WorldTransferResultCode,
};
use serde::{Deserialize, Serialize};
use sim_core::{CoreBossParticipant, CoreBossParticipantLock, CoreCaldusVictoryIdentities};
use thiserror::Error;

use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CaldusInstancePresentation, IdentityClock,
    WorldFlowRepositoryError, world_flow_gate::stored_location_snapshot,
};

const CALDUS_EXIT_ID: &str = "portal.exit.dungeon.bell_sepulcher";
const HALL_ID: &str = "hub.lantern_halls_01";
const EXTRACTION_COMMAND_KIND: i16 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaldusExtractionEvidenceCommand {
    pub authenticated: AuthenticatedAccount,
    pub character_id: [u8; 16],
    pub participant: CoreBossParticipant,
    pub instance_lineage_id: [u8; 16],
    pub entry_restore_point_id: [u8; 16],
    pub expected_character_version: u64,
    pub content_revision: WorldFlowContentRevisionV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaldusExtractionEvidenceResult {
    pub request: StoredExtractionResult,
    pub receipt: StoredExtractionResult,
}

#[derive(Debug, Clone)]
pub struct PostgresCaldusExtractionAuthority {
    persistence: PostgresPersistence,
}

impl PostgresCaldusExtractionAuthority {
    #[must_use]
    pub const fn new(persistence: PostgresPersistence) -> Self {
        Self { persistence }
    }

    /// Explicit test/showcase authority allowed by `SPEC-CONFLICT-023`. No production authority
    /// constructor exists in 03E.
    pub async fn request_and_commit_wipeable_evidence(
        &self,
        presentation: &CaldusInstancePresentation,
        lock: &CoreBossParticipantLock,
        command: &CaldusExtractionEvidenceCommand,
    ) -> Result<CaldusExtractionEvidenceResult, CaldusExtractionError> {
        let request = build_request(presentation, lock, command)?;
        let identities = CoreCaldusVictoryIdentities::derive(command.instance_lineage_id, lock)?;
        let extraction = identities
            .extraction_for(command.participant)
            .ok_or(CaldusExtractionError::ParticipantNotLocked)?;
        let requested = self.persistence.request_caldus_extraction(&request).await?;
        let committed = self
            .persistence
            .commit_caldus_extraction(CaldusExtractionCommit {
                extraction_request_id: extraction.request_id.bytes(),
                extraction_receipt_id: extraction.receipt_id.bytes(),
                authority: StoredExtractionAuthority::WipeableTestEvidence,
            })
            .await?;
        Ok(CaldusExtractionEvidenceResult {
            request: transaction_result(requested),
            receipt: transaction_result(committed),
        })
    }
}

fn build_request(
    presentation: &CaldusInstancePresentation,
    lock: &CoreBossParticipantLock,
    command: &CaldusExtractionEvidenceCommand,
) -> Result<CaldusExtractionRequest, CaldusExtractionError> {
    if command.authenticated.namespace != AuthenticatedNamespace::WipeableTest
        || command.character_id == [0; 16]
        || command.entry_restore_point_id == [0; 16]
        || command.expected_character_version == 0
        || command.instance_lineage_id != presentation.instance_lineage_id()
        || lock.attempt_ordinal != presentation.attempt_ordinal()
    {
        return Err(CaldusExtractionError::InvalidEvidenceBinding);
    }
    let exit = presentation
        .exit()
        .ok_or(CaldusExtractionError::ExitNotCommitted)?;
    let identities = CoreCaldusVictoryIdentities::derive(command.instance_lineage_id, lock)?;
    if exit.exit_instance_id != identities.exit_instance_id.bytes() {
        return Err(CaldusExtractionError::InvalidEvidenceBinding);
    }
    let extraction = identities
        .extraction_for(command.participant)
        .ok_or(CaldusExtractionError::ParticipantNotLocked)?;
    Ok(CaldusExtractionRequest {
        account_id: command.authenticated.account_id.as_bytes(),
        character_id: command.character_id,
        extraction_request_id: extraction.request_id.bytes(),
        encounter_id: identities.encounter_id.bytes(),
        instance_lineage_id: command.instance_lineage_id,
        entry_restore_point_id: command.entry_restore_point_id,
        exit_instance_id: exit.exit_instance_id,
        attempt_ordinal: lock.attempt_ordinal,
        party_slot: command.participant.party_slot,
        participant_entity_id: command.participant.entity_id.get(),
        expected_character_version: command.expected_character_version,
        content_revision: stored_revision(&command.content_revision),
    })
}

fn transaction_result(transaction: CaldusExtractionTransaction) -> StoredExtractionResult {
    match transaction {
        CaldusExtractionTransaction::Fresh(result)
        | CaldusExtractionTransaction::Replay(result) => result,
    }
}

#[derive(Debug, Clone)]
pub struct PostgresCaldusHallTransferCoordinator<Clock> {
    persistence: PostgresPersistence,
    clock: Clock,
    required_content_revision: WorldFlowContentRevisionV1,
}

impl<Clock> PostgresCaldusHallTransferCoordinator<Clock>
where
    Clock: IdentityClock,
{
    #[must_use]
    pub const fn new(
        persistence: PostgresPersistence,
        clock: Clock,
        required_content_revision: WorldFlowContentRevisionV1,
    ) -> Self {
        Self {
            persistence,
            clock,
            required_content_revision,
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the ordered serializable transfer is intentionally kept as one auditable state machine"
    )]
    pub async fn transfer(
        &self,
        authenticated: AuthenticatedAccount,
        request_sequence: u32,
        mutation: &WorldTransferMutation,
    ) -> WorldFlowResult {
        if let Some(code) = validate_transfer_preflight(
            authenticated,
            request_sequence,
            mutation,
            &self.required_content_revision,
            self.clock.unix_millis(),
        ) {
            return transfer_result(request_sequence, mutation, code, None, None);
        }
        let WorldTransferCommand::UseCommittedExtraction {
            portal_id: _,
            extraction_request_id,
            extraction_receipt_id,
        } = &mutation.payload.command
        else {
            return transfer_result(
                request_sequence,
                mutation,
                WorldTransferResultCode::InvalidSource,
                None,
                None,
            );
        };
        let begin = self
            .persistence
            .begin_world_flow(
                authenticated.account_id.as_bytes(),
                mutation.character_id,
                mutation.mutation_id,
            )
            .await;
        let mut write = match begin {
            Ok(WorldFlowBegin::Replayed(receipt)) => {
                return replay(request_sequence, mutation, &receipt);
            }
            Ok(WorldFlowBegin::Fresh(write)) => write,
            Err(_) => {
                return transfer_result(
                    request_sequence,
                    mutation,
                    WorldTransferResultCode::ServiceUnavailable,
                    None,
                    None,
                );
            }
        };
        let rejection = validate_world_state(mutation, write.state());
        if let Some(code) = rejection {
            return commit_rejection(authenticated, request_sequence, mutation, write, code).await;
        }
        let Some(next_version) = write.state().location.character_version().checked_add(1) else {
            return transfer_result(
                request_sequence,
                mutation,
                WorldTransferResultCode::ServiceUnavailable,
                None,
                None,
            );
        };
        let Some((instance_lineage_id, entry_restore_point_id)) =
            danger_binding(&write.state().location)
        else {
            return commit_rejection(
                authenticated,
                request_sequence,
                mutation,
                write,
                WorldTransferResultCode::InvalidSource,
            )
            .await;
        };
        let transfer = CaldusExtractionTransfer {
            account_id: authenticated.account_id.as_bytes(),
            character_id: mutation.character_id,
            extraction_request_id: *extraction_request_id,
            extraction_receipt_id: *extraction_receipt_id,
            instance_lineage_id,
            entry_restore_point_id,
            transfer_mutation_id: mutation.mutation_id,
            expected_character_version: mutation.expected_character_version,
            post_character_version: match u64::try_from(next_version) {
                Ok(version) => version,
                Err(_) => {
                    return transfer_result(
                        request_sequence,
                        mutation,
                        WorldTransferResultCode::ServiceUnavailable,
                        None,
                        None,
                    );
                }
            },
        };
        if let Err(error) =
            stage_caldus_extraction_transfer(write.transaction_mut(), transfer).await
        {
            let code = match error {
                PersistenceError::ExtractionAlreadyTransferred => {
                    WorldTransferResultCode::IdempotencyConflict
                }
                PersistenceError::ExtractionReceiptRequired => {
                    WorldTransferResultCode::InvalidSource
                }
                _ => WorldTransferResultCode::ServiceUnavailable,
            };
            return commit_rejection(authenticated, request_sequence, mutation, write, code).await;
        }
        if stage_danger_checkpoint_cleanup(
            write.transaction_mut(),
            authenticated.account_id.as_bytes(),
            mutation.character_id,
            instance_lineage_id,
        )
        .await
        .is_err()
        {
            return transfer_result(
                request_sequence,
                mutation,
                WorldTransferResultCode::ServiceUnavailable,
                None,
                None,
            );
        }
        let next_location = StoredWorldLocation::Safe {
            character_version: next_version,
            location_content_id: HALL_ID.to_owned(),
            arrival: StoredSafeArrival::HallDefault,
        };
        let Ok(snapshot) = protocol_snapshot(mutation.character_id, &next_location) else {
            return transfer_result(
                request_sequence,
                mutation,
                WorldTransferResultCode::ServiceUnavailable,
                None,
                None,
            );
        };
        write.state_mut().location = next_location;
        write.state_mut().location_changed = true;
        let result = transfer_result(
            request_sequence,
            mutation,
            WorldTransferResultCode::Accepted,
            Some(snapshot),
            Some(*extraction_receipt_id),
        );
        if stage_transfer_receipt(authenticated, mutation, write.state_mut(), &result).is_err() {
            return transfer_result(
                request_sequence,
                mutation,
                WorldTransferResultCode::ServiceUnavailable,
                None,
                None,
            );
        }
        match write.commit(result).await {
            Ok(WorldFlowTransaction::Committed(result)) => result,
            _ => transfer_result(
                request_sequence,
                mutation,
                WorldTransferResultCode::ServiceUnavailable,
                None,
                None,
            ),
        }
    }
}

fn validate_transfer_preflight(
    authenticated: AuthenticatedAccount,
    request_sequence: u32,
    mutation: &WorldTransferMutation,
    required_content_revision: &WorldFlowContentRevisionV1,
    now_unix_millis: u64,
) -> Option<WorldTransferResultCode> {
    if request_sequence == 0
        || mutation.validate().is_err()
        || authenticated.namespace != AuthenticatedNamespace::WipeableTest
        || !matches!(
            &mutation.payload.command,
            WorldTransferCommand::UseCommittedExtraction { portal_id, .. }
                if portal_id.as_str() == CALDUS_EXIT_ID
        )
    {
        Some(WorldTransferResultCode::InvalidSource)
    } else if mutation.payload_hash != mutation.payload.canonical_hash() {
        Some(WorldTransferResultCode::PayloadHashMismatch)
    } else if mutation.payload.content_revision != *required_content_revision {
        Some(WorldTransferResultCode::ContentMismatch)
    } else if mutation.issued_at_unix_millis > now_unix_millis {
        Some(WorldTransferResultCode::IssuedAtInvalid)
    } else {
        None
    }
}

fn danger_binding(location: &StoredWorldLocation) -> Option<([u8; 16], [u8; 16])> {
    match location {
        StoredWorldLocation::Danger {
            instance_lineage_id,
            entry_restore_point_id,
            ..
        } => Some((*instance_lineage_id, *entry_restore_point_id)),
        StoredWorldLocation::CharacterSelect { .. } | StoredWorldLocation::Safe { .. } => None,
    }
}

async fn commit_rejection(
    authenticated: AuthenticatedAccount,
    request_sequence: u32,
    mutation: &WorldTransferMutation,
    mut write: Box<persistence::WorldFlowWrite<'_>>,
    code: WorldTransferResultCode,
) -> WorldFlowResult {
    let snapshot = protocol_snapshot(mutation.character_id, &write.state().location).ok();
    let result = transfer_result(request_sequence, mutation, code, snapshot, None);
    if stage_transfer_receipt(authenticated, mutation, write.state_mut(), &result).is_err() {
        return transfer_result(
            request_sequence,
            mutation,
            WorldTransferResultCode::ServiceUnavailable,
            None,
            None,
        );
    }
    match write.commit(result).await {
        Ok(WorldFlowTransaction::Committed(result)) => result,
        _ => transfer_result(
            request_sequence,
            mutation,
            WorldTransferResultCode::ServiceUnavailable,
            None,
            None,
        ),
    }
}

fn validate_world_state(
    mutation: &WorldTransferMutation,
    state: &persistence::WorldFlowTransactionState,
) -> Option<WorldTransferResultCode> {
    if state.selected_character_id != Some(mutation.character_id) {
        Some(WorldTransferResultCode::NoSelectedCharacter)
    } else if state.character.life_state != 0 {
        Some(WorldTransferResultCode::CharacterDead)
    } else if state.character.security_state != 0 {
        Some(WorldTransferResultCode::StorageResolutionRequired)
    } else if state.location.character_version()
        != i64::try_from(mutation.expected_character_version).unwrap_or(i64::MIN)
    {
        Some(WorldTransferResultCode::StateVersionMismatch)
    } else if !matches!(state.location, StoredWorldLocation::Danger { .. }) {
        Some(WorldTransferResultCode::InvalidSource)
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredCaldusHallOutcome {
    code: WorldTransferResultCode,
    snapshot: Option<CharacterLocationSnapshot>,
    transfer_id: Option<[u8; 16]>,
}

fn stage_transfer_receipt(
    authenticated: AuthenticatedAccount,
    mutation: &WorldTransferMutation,
    state: &mut persistence::WorldFlowTransactionState,
    result: &WorldFlowResult,
) -> Result<(), PersistenceError> {
    let WorldFlowResult::Transfer {
        code,
        snapshot,
        transfer_id,
        ..
    } = result
    else {
        return Err(PersistenceError::CorruptStoredWorldFlow);
    };
    state.new_receipt = Some(StoredWorldTransferReceipt {
        account_id: authenticated.account_id.as_bytes(),
        character_id: mutation.character_id,
        mutation_id: mutation.mutation_id,
        payload_hash: mutation.payload_hash,
        content_revision: stored_revision(&mutation.payload.content_revision),
        expected_character_version: i64::try_from(mutation.expected_character_version)
            .map_err(|_| PersistenceError::CorruptStoredWorldFlow)?,
        issued_at_unix_millis: i64::try_from(mutation.issued_at_unix_millis)
            .map_err(|_| PersistenceError::CorruptStoredWorldFlow)?,
        command_kind: EXTRACTION_COMMAND_KIND,
        transfer_id: *transfer_id,
        pre_character_version: state.character.character_version,
        post_character_version: state.location.character_version(),
        result_code: result_code(*code),
        result_payload: postcard::to_stdvec(&StoredCaldusHallOutcome {
            code: *code,
            snapshot: snapshot.clone(),
            transfer_id: *transfer_id,
        })
        .map_err(|_| PersistenceError::CorruptStoredWorldFlow)?,
    });
    Ok(())
}

fn replay(
    request_sequence: u32,
    mutation: &WorldTransferMutation,
    receipt: &StoredWorldTransferReceipt,
) -> WorldFlowResult {
    if receipt.character_id != mutation.character_id
        || receipt.payload_hash != mutation.payload_hash
        || receipt.content_revision != stored_revision(&mutation.payload.content_revision)
        || receipt.expected_character_version
            != i64::try_from(mutation.expected_character_version).unwrap_or(i64::MIN)
        || receipt.issued_at_unix_millis
            != i64::try_from(mutation.issued_at_unix_millis).unwrap_or(i64::MIN)
        || receipt.command_kind != EXTRACTION_COMMAND_KIND
    {
        return transfer_result(
            request_sequence,
            mutation,
            WorldTransferResultCode::IdempotencyConflict,
            None,
            None,
        );
    }
    postcard::from_bytes::<StoredCaldusHallOutcome>(&receipt.result_payload).map_or_else(
        |_| {
            transfer_result(
                request_sequence,
                mutation,
                WorldTransferResultCode::ServiceUnavailable,
                None,
                None,
            )
        },
        |outcome| {
            transfer_result(
                request_sequence,
                mutation,
                outcome.code,
                outcome.snapshot,
                outcome.transfer_id,
            )
        },
    )
}

fn protocol_snapshot(
    character_id: [u8; 16],
    location: &StoredWorldLocation,
) -> Result<CharacterLocationSnapshot, PersistenceError> {
    stored_location_snapshot(character_id, location.clone()).map_err(|error| match error {
        WorldFlowRepositoryError::Unavailable | WorldFlowRepositoryError::Corrupt => {
            PersistenceError::CorruptStoredWorldFlow
        }
    })
}

fn stored_revision(revision: &WorldFlowContentRevisionV1) -> StoredWorldFlowRevisionV1 {
    StoredWorldFlowRevisionV1 {
        records_blake3: revision.records_blake3.as_str().to_owned(),
        assets_blake3: revision.assets_blake3.as_str().to_owned(),
        localization_blake3: revision.localization_blake3.as_str().to_owned(),
    }
}

const fn result_code(code: WorldTransferResultCode) -> i16 {
    match code {
        WorldTransferResultCode::Accepted => 0,
        WorldTransferResultCode::StageDisabled => 1,
        WorldTransferResultCode::StateVersionMismatch => 2,
        WorldTransferResultCode::CharacterNotFound => 3,
        WorldTransferResultCode::NoSelectedCharacter => 4,
        WorldTransferResultCode::CharacterNotOwned => 5,
        WorldTransferResultCode::CharacterDead => 6,
        WorldTransferResultCode::InvalidSource => 7,
        WorldTransferResultCode::OutOfRange => 8,
        WorldTransferResultCode::ContentDisabled => 9,
        WorldTransferResultCode::DestinationDisabled => 10,
        WorldTransferResultCode::TransferInProgress => 11,
        WorldTransferResultCode::ContentMismatch => 12,
        WorldTransferResultCode::IdempotencyConflict => 13,
        WorldTransferResultCode::PayloadHashMismatch => 14,
        WorldTransferResultCode::IssuedAtInvalid => 15,
        WorldTransferResultCode::IncompleteRestorePoint => 16,
        WorldTransferResultCode::StorageResolutionRequired => 17,
        WorldTransferResultCode::InstanceUnavailable => 18,
        WorldTransferResultCode::RateLimited => 19,
        WorldTransferResultCode::ServiceUnavailable => 20,
    }
}

fn transfer_result(
    request_sequence: u32,
    mutation: &WorldTransferMutation,
    code: WorldTransferResultCode,
    snapshot: Option<CharacterLocationSnapshot>,
    transfer_id: Option<[u8; 16]>,
) -> WorldFlowResult {
    WorldFlowResult::Transfer {
        request_sequence,
        mutation_id: mutation.mutation_id,
        accepted: code == WorldTransferResultCode::Accepted,
        code,
        snapshot,
        transfer_id,
    }
}

#[derive(Debug, Error)]
pub enum CaldusExtractionError {
    #[error("Caldus extraction evidence binding is invalid")]
    InvalidEvidenceBinding,
    #[error("Caldus exit is not durably committed and presented")]
    ExitNotCommitted,
    #[error("Caldus extraction participant is not in the immutable lock")]
    ParticipantNotLocked,
    #[error(transparent)]
    Victory(#[from] sim_core::CoreCaldusVictoryError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
}

#[cfg(test)]
mod tests {
    use protocol::ManifestHash;
    use sim_core::EntityId;

    use super::*;
    use crate::AccountId;

    fn revision() -> WorldFlowContentRevisionV1 {
        WorldFlowContentRevisionV1 {
            records_blake3: ManifestHash::new("1".repeat(64)).unwrap(),
            assets_blake3: ManifestHash::new("2".repeat(64)).unwrap(),
            localization_blake3: ManifestHash::new("3".repeat(64)).unwrap(),
        }
    }

    fn authenticated(namespace: AuthenticatedNamespace) -> AuthenticatedAccount {
        AuthenticatedAccount {
            account_id: AccountId::new([1; 16]).unwrap(),
            namespace,
        }
    }

    fn transfer_mutation() -> WorldTransferMutation {
        let payload = protocol::WorldTransferPayload {
            content_revision: revision(),
            command: WorldTransferCommand::UseCommittedExtraction {
                portal_id: protocol::WireText::new(CALDUS_EXIT_ID).unwrap(),
                extraction_request_id: [4; 16],
                extraction_receipt_id: [5; 16],
            },
        };
        WorldTransferMutation {
            mutation_id: [6; 16],
            character_id: [7; 16],
            expected_character_version: 2,
            issued_at_unix_millis: 10,
            payload_hash: payload.canonical_hash(),
            payload,
        }
    }

    #[test]
    fn extraction_evidence_cannot_bypass_hidden_exit_or_wipeable_namespace() {
        let participant = CoreBossParticipant {
            entity_id: EntityId::new(9).unwrap(),
            party_slot: 0,
        };
        let lock = CoreBossParticipantLock {
            attempt_ordinal: 1,
            participants: vec![participant],
            maximum_health: 7_200,
        };
        let presentation = CaldusInstancePresentation::new([7; 16], 1).unwrap();
        let mut command = CaldusExtractionEvidenceCommand {
            authenticated: AuthenticatedAccount {
                account_id: AccountId::new([1; 16]).unwrap(),
                namespace: AuthenticatedNamespace::WipeableTest,
            },
            character_id: [2; 16],
            participant,
            instance_lineage_id: [7; 16],
            entry_restore_point_id: [8; 16],
            expected_character_version: 2,
            content_revision: revision(),
        };
        assert!(matches!(
            build_request(&presentation, &lock, &command),
            Err(CaldusExtractionError::ExitNotCommitted)
        ));
        command.authenticated.namespace = AuthenticatedNamespace::Production;
        assert!(matches!(
            build_request(&presentation, &lock, &command),
            Err(CaldusExtractionError::InvalidEvidenceBinding)
        ));
    }

    #[test]
    fn hall_transfer_preflight_is_content_payload_time_and_namespace_bound() {
        let required = revision();
        let auth = authenticated(AuthenticatedNamespace::WipeableTest);
        let mutation = transfer_mutation();
        assert_eq!(
            validate_transfer_preflight(auth, 1, &mutation, &required, 10),
            None
        );

        let mut bad_hash = mutation.clone();
        bad_hash.payload_hash[0] ^= 1;
        assert_eq!(
            validate_transfer_preflight(auth, 1, &bad_hash, &required, 10),
            Some(WorldTransferResultCode::PayloadHashMismatch)
        );

        let mut wrong_content = mutation.clone();
        wrong_content.payload.content_revision.records_blake3 =
            ManifestHash::new("9".repeat(64)).unwrap();
        wrong_content.payload_hash = wrong_content.payload.canonical_hash();
        assert_eq!(
            validate_transfer_preflight(auth, 1, &wrong_content, &required, 10),
            Some(WorldTransferResultCode::ContentMismatch)
        );
        assert_eq!(
            validate_transfer_preflight(auth, 1, &mutation, &required, 9),
            Some(WorldTransferResultCode::IssuedAtInvalid)
        );
        assert_eq!(
            validate_transfer_preflight(
                authenticated(AuthenticatedNamespace::Production),
                1,
                &mutation,
                &required,
                10,
            ),
            Some(WorldTransferResultCode::InvalidSource)
        );
    }
}
