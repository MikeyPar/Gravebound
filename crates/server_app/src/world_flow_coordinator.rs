//! Dormant disposable transfer coordinator for approved `SPEC-CONFLICT-010`.
//!
//! This authority is deliberately not wired into the normal Core endpoint. It proves safe-route
//! transaction semantics while the player route remains fail-closed in [`crate::WorldFlowGateService`].

use persistence::{
    PersistenceError, PostgresPersistence, StoredDangerEntryRootV1, StoredSafeArrival,
    StoredWorldFlowRevisionV1, StoredWorldLocation, StoredWorldTransferReceipt, WorldFlowBegin,
    WorldFlowTransaction, WorldFlowTransactionState, stage_world_flow_danger_entry,
};
use protocol::{
    CharacterLocationSnapshot, WorldFlowContentRevisionV1, WorldFlowFrame, WorldFlowRequest,
    WorldFlowResult, WorldTransferCommand, WorldTransferMutation, WorldTransferResultCode,
};
use serde::{Deserialize, Serialize};

use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, EntryCaptureContext, EntryRestoreProvider,
    IdentityClock, InventorySecurityRestoreV1, OathBargainRestoreV1,
    PostgresProgressionRestoreProvider, RestorePointError, RestorePointProviders,
    WorldFlowRepositoryError, world_flow_gate::stored_location_snapshot,
};

const HALL_ID: &str = "hub.lantern_halls_01";
const CHARACTER_SELECT_RETURN_SPAWN_ID: &str = "spawn.hub.character_select_return";
const REALM_GATE_ID: &str = "station.realm_gate";
const CORE_MICROREALM_ID: &str = "world.core_microrealm_01";
const CORE_PRIVATE_LIFE_LAYOUT_ID: &str = "layout.core_private_life_01";

pub trait WorldFlowIdGenerator: Send + Sync {
    fn next_transfer_id(&self) -> [u8; 16];
    fn next_lineage_id(&self) -> [u8; 16];
    fn next_restore_point_id(&self) -> [u8; 16];
}

#[derive(Debug, Clone)]
pub struct DormantWorldFlowPlanner<Generator, Clock> {
    generator: Generator,
    clock: Clock,
    required_content_revision: WorldFlowContentRevisionV1,
}

impl<Generator, Clock> DormantWorldFlowPlanner<Generator, Clock>
where
    Generator: WorldFlowIdGenerator,
    Clock: IdentityClock,
{
    pub const fn new(
        generator: Generator,
        clock: Clock,
        required_content_revision: WorldFlowContentRevisionV1,
    ) -> Self {
        Self {
            generator,
            clock,
            required_content_revision,
        }
    }

    fn plan_fresh(
        &self,
        authenticated: AuthenticatedAccount,
        request_sequence: u32,
        mutation: &WorldTransferMutation,
        state: &mut WorldFlowTransactionState,
    ) -> Result<WorldFlowResult, PersistenceError> {
        let reject = |code| staged_result(request_sequence, mutation, code, None, None);
        let planned = if mutation.payload.content_revision != self.required_content_revision {
            reject(WorldTransferResultCode::ContentMismatch)
        } else if mutation.issued_at_unix_millis > self.clock.unix_millis() {
            reject(WorldTransferResultCode::IssuedAtInvalid)
        } else if state.selected_character_id.is_none() {
            reject(WorldTransferResultCode::NoSelectedCharacter)
        } else if state.selected_character_id != Some(mutation.character_id) {
            reject(WorldTransferResultCode::InvalidSource)
        } else if state.character.life_state != 0 {
            reject(WorldTransferResultCode::CharacterDead)
        } else if state.character.security_state != 0 {
            reject(WorldTransferResultCode::StorageResolutionRequired)
        } else if state.location.character_version()
            != i64::try_from(mutation.expected_character_version)
                .map_err(|_| PersistenceError::CorruptStoredWorldFlow)?
        {
            let snapshot = protocol_snapshot(mutation.character_id, &state.location)?;
            staged_result(
                request_sequence,
                mutation,
                WorldTransferResultCode::StateVersionMismatch,
                Some(snapshot),
                None,
            )
        } else {
            self.plan_route(request_sequence, mutation, state)?
        };
        stage_receipt(authenticated, mutation, state, &planned)?;
        Ok(planned)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the dormant route matrix is intentionally kept together for fail-closed auditability"
    )]
    fn plan_route(
        &self,
        request_sequence: u32,
        mutation: &WorldTransferMutation,
        state: &mut WorldFlowTransactionState,
    ) -> Result<WorldFlowResult, PersistenceError> {
        let next_version = state
            .location
            .character_version()
            .checked_add(1)
            .ok_or(PersistenceError::CorruptStoredWorldFlow)?;
        let next_location = match (&mutation.payload.command, &state.location) {
            (
                WorldTransferCommand::EnterHallFromCharacterSelect,
                StoredWorldLocation::CharacterSelect {
                    next_hall_arrival, ..
                },
            ) => StoredWorldLocation::Safe {
                character_version: next_version,
                location_content_id: HALL_ID.to_owned(),
                arrival: next_hall_arrival.clone(),
            },
            (
                WorldTransferCommand::ReturnToCharacterSelect,
                StoredWorldLocation::Safe {
                    location_content_id,
                    ..
                },
            ) if location_content_id == HALL_ID => StoredWorldLocation::CharacterSelect {
                character_version: next_version,
                next_hall_arrival: StoredSafeArrival::SpawnAnchor(
                    CHARACTER_SELECT_RETURN_SPAWN_ID.to_owned(),
                ),
            },
            (
                WorldTransferCommand::UsePortal { portal_id },
                StoredWorldLocation::Safe {
                    location_content_id,
                    ..
                },
            ) if portal_id.as_str() == REALM_GATE_ID && location_content_id == HALL_ID => {
                let transfer_id = self.generator.next_transfer_id();
                let lineage_id = self.generator.next_lineage_id();
                let restore_point_id = self.generator.next_restore_point_id();
                if [transfer_id, lineage_id, restore_point_id]
                    .into_iter()
                    .any(|identity| identity.iter().all(|byte| *byte == 0))
                    || transfer_id == lineage_id
                    || transfer_id == restore_point_id
                    || lineage_id == restore_point_id
                {
                    return Err(PersistenceError::CorruptStoredWorldFlow);
                }
                let next_location = StoredWorldLocation::Danger {
                    character_version: next_version,
                    location_content_id: CORE_MICROREALM_ID.to_owned(),
                    instance_lineage_id: lineage_id,
                    entry_restore_point_id: restore_point_id,
                };
                let snapshot = protocol_snapshot(mutation.character_id, &next_location)?;
                state.location = next_location;
                state.location_changed = true;
                return Ok(staged_result(
                    request_sequence,
                    mutation,
                    WorldTransferResultCode::Accepted,
                    Some(snapshot),
                    Some(transfer_id),
                ));
            }
            (
                WorldTransferCommand::UsePortal { .. }
                | WorldTransferCommand::UseCommittedExtraction { .. },
                _,
            ) => {
                return Ok(staged_result(
                    request_sequence,
                    mutation,
                    WorldTransferResultCode::DestinationDisabled,
                    Some(protocol_snapshot(mutation.character_id, &state.location)?),
                    None,
                ));
            }
            _ => {
                return Ok(staged_result(
                    request_sequence,
                    mutation,
                    WorldTransferResultCode::InvalidSource,
                    Some(protocol_snapshot(mutation.character_id, &state.location)?),
                    None,
                ));
            }
        };
        let transfer_id = self.generator.next_transfer_id();
        if transfer_id.iter().all(|byte| *byte == 0) {
            return Err(PersistenceError::CorruptStoredWorldFlow);
        }
        let snapshot = protocol_snapshot(mutation.character_id, &next_location)?;
        state.location = next_location;
        state.location_changed = true;
        Ok(staged_result(
            request_sequence,
            mutation,
            WorldTransferResultCode::Accepted,
            Some(snapshot),
            Some(transfer_id),
        ))
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
            || receipt.command_kind != command_kind(&mutation.payload.command)
        {
            return staged_result(
                request_sequence,
                mutation,
                WorldTransferResultCode::IdempotencyConflict,
                None,
                None,
            );
        }
        postcard::from_bytes::<StoredWorldFlowOutcome>(&receipt.result_payload).map_or_else(
            |_| {
                staged_result(
                    request_sequence,
                    mutation,
                    WorldTransferResultCode::ServiceUnavailable,
                    None,
                    None,
                )
            },
            |outcome| outcome.into_result(request_sequence, mutation.mutation_id),
        )
    }
}

#[derive(Debug, Clone)]
pub struct PostgresDormantWorldFlowCoordinator<Generator, Clock, Inventory, OathBargains> {
    persistence: PostgresPersistence,
    planner: DormantWorldFlowPlanner<Generator, Clock>,
    restore_providers:
        RestorePointProviders<PostgresProgressionRestoreProvider, Inventory, OathBargains>,
}

impl<Generator, Clock, Inventory, OathBargains>
    PostgresDormantWorldFlowCoordinator<Generator, Clock, Inventory, OathBargains>
where
    Generator: WorldFlowIdGenerator,
    Clock: IdentityClock,
    Inventory: EntryRestoreProvider<Snapshot = InventorySecurityRestoreV1>,
    OathBargains: EntryRestoreProvider<Snapshot = OathBargainRestoreV1>,
{
    pub fn new(
        persistence: PostgresPersistence,
        generator: Generator,
        clock: Clock,
        required_content_revision: WorldFlowContentRevisionV1,
        progression: PostgresProgressionRestoreProvider,
        inventory: Inventory,
        oath_bargains: OathBargains,
    ) -> Self {
        let restore_providers = RestorePointProviders::new(progression, inventory, oath_bargains);
        Self {
            persistence,
            planner: DormantWorldFlowPlanner::new(generator, clock, required_content_revision),
            restore_providers,
        }
    }

    pub async fn handle(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &WorldFlowFrame,
    ) -> WorldFlowResult {
        let WorldFlowRequest::Transfer(mutation) = &frame.request else {
            return error(frame.sequence, WorldTransferResultCode::ServiceUnavailable);
        };
        if frame.validate().is_err()
            || authenticated.namespace != AuthenticatedNamespace::WipeableTest
        {
            return error(frame.sequence, WorldTransferResultCode::ServiceUnavailable);
        }
        if mutation.payload_hash != mutation.payload.canonical_hash() {
            return staged_result(
                frame.sequence,
                mutation,
                WorldTransferResultCode::PayloadHashMismatch,
                None,
                None,
            );
        }
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
                return DormantWorldFlowPlanner::<Generator, Clock>::replay(
                    frame.sequence,
                    mutation,
                    &receipt,
                );
            }
            Ok(WorldFlowBegin::Fresh(write)) => *write,
            Err(PersistenceError::WorldFlowCharacterNotFound) => {
                let code = match self
                    .persistence
                    .identity_character_owner(mutation.character_id)
                    .await
                {
                    Ok(Some(owner)) if owner != authenticated.account_id.as_bytes() => {
                        WorldTransferResultCode::CharacterNotOwned
                    }
                    Ok(_) => WorldTransferResultCode::CharacterNotFound,
                    Err(_) => WorldTransferResultCode::ServiceUnavailable,
                };
                return staged_result(frame.sequence, mutation, code, None, None);
            }
            Err(_) => {
                return staged_result(
                    frame.sequence,
                    mutation,
                    WorldTransferResultCode::ServiceUnavailable,
                    None,
                    None,
                );
            }
        };
        let Ok(result) =
            self.planner
                .plan_fresh(authenticated, frame.sequence, mutation, write.state_mut())
        else {
            return staged_result(
                frame.sequence,
                mutation,
                WorldTransferResultCode::ServiceUnavailable,
                None,
                None,
            );
        };
        if let Err(code) = self.capture_danger_entry(&mut write, mutation).await {
            return staged_result(frame.sequence, mutation, code, None, None);
        }
        match write.commit(result).await {
            Ok(WorldFlowTransaction::Committed(result)) => result,
            Ok(WorldFlowTransaction::Replayed(_)) => unreachable!("fresh write cannot replay"),
            Err(_) => staged_result(
                frame.sequence,
                mutation,
                WorldTransferResultCode::ServiceUnavailable,
                None,
                None,
            ),
        }
    }

    async fn capture_danger_entry(
        &self,
        write: &mut persistence::WorldFlowWrite<'_>,
        mutation: &WorldTransferMutation,
    ) -> Result<(), WorldTransferResultCode> {
        if !write.state().location_changed {
            return Ok(());
        }
        let (
            account_id,
            account_version,
            character_version,
            lineage_id,
            restore_point_id,
            danger_location_id,
        ) = match &write.state().location {
            StoredWorldLocation::Danger {
                character_version: post_version,
                location_content_id,
                instance_lineage_id,
                entry_restore_point_id,
            } => (
                write
                    .state()
                    .new_receipt
                    .as_ref()
                    .ok_or(WorldTransferResultCode::ServiceUnavailable)?
                    .account_id,
                write.state().account_version,
                post_version
                    .checked_sub(1)
                    .ok_or(WorldTransferResultCode::ServiceUnavailable)?,
                *instance_lineage_id,
                *entry_restore_point_id,
                location_content_id.clone(),
            ),
            _ => return Ok(()),
        };
        let account_version = u64::try_from(account_version)
            .map_err(|_| WorldTransferResultCode::ServiceUnavailable)?;
        let character_version = u64::try_from(character_version)
            .map_err(|_| WorldTransferResultCode::ServiceUnavailable)?;
        let snapshot = self
            .restore_providers
            .capture_v1(
                write.transaction_mut(),
                EntryCaptureContext {
                    account_id,
                    character_id: mutation.character_id,
                    restore_point_id,
                },
                mutation.payload.content_revision.clone(),
                account_version,
                character_version,
            )
            .await
            .map_err(restore_capture_code)?;
        let root = StoredDangerEntryRootV1 {
            account_id,
            character_id: mutation.character_id,
            lineage_id,
            restore_point_id,
            source_location_id: HALL_ID.to_owned(),
            danger_location_id,
            layout_id: CORE_PRIVATE_LIFE_LAYOUT_ID.to_owned(),
            content_revision: stored_revision(&snapshot.content_revision),
            account_version: i64::try_from(snapshot.versions.account_version)
                .map_err(|_| WorldTransferResultCode::ServiceUnavailable)?,
            character_version: i64::try_from(snapshot.versions.character_version)
                .map_err(|_| WorldTransferResultCode::ServiceUnavailable)?,
            progression_version: i64::try_from(snapshot.versions.progression_version)
                .map_err(|_| WorldTransferResultCode::ServiceUnavailable)?,
            inventory_version: i64::try_from(snapshot.versions.inventory_version)
                .map_err(|_| WorldTransferResultCode::ServiceUnavailable)?,
            oath_bargain_version: i64::try_from(snapshot.versions.oath_bargain_version)
                .map_err(|_| WorldTransferResultCode::ServiceUnavailable)?,
            composite_digest: snapshot.composite_digest().map_err(restore_capture_code)?,
        };
        stage_world_flow_danger_entry(write.transaction_mut(), &root)
            .await
            .map_err(|_| WorldTransferResultCode::ServiceUnavailable)
    }
}

const fn restore_capture_code(error: RestorePointError) -> WorldTransferResultCode {
    match error {
        RestorePointError::Persistence => WorldTransferResultCode::ServiceUnavailable,
        RestorePointError::ZeroItemUid
        | RestorePointError::ZeroCharacterId
        | RestorePointError::ZeroContextIdentity
        | RestorePointError::InvalidProgression
        | RestorePointError::InvalidInventory
        | RestorePointError::InvalidBeltStack
        | RestorePointError::DuplicateItemUid
        | RestorePointError::InvalidOathBargains
        | RestorePointError::ZeroAggregateVersion
        | RestorePointError::AggregateVersionMismatch
        | RestorePointError::Encoding
        | RestorePointError::IncompleteRestorePoint
        | RestorePointError::RestoreSuperseded => WorldTransferResultCode::IncompleteRestorePoint,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredWorldFlowOutcome {
    code: WorldTransferResultCode,
    snapshot: Option<CharacterLocationSnapshot>,
    transfer_id: Option<[u8; 16]>,
}

impl StoredWorldFlowOutcome {
    fn into_result(self, request_sequence: u32, mutation_id: [u8; 16]) -> WorldFlowResult {
        WorldFlowResult::Transfer {
            request_sequence,
            mutation_id,
            accepted: self.code == WorldTransferResultCode::Accepted,
            code: self.code,
            snapshot: self.snapshot,
            transfer_id: self.transfer_id,
        }
    }
}

fn stage_receipt(
    authenticated: AuthenticatedAccount,
    mutation: &WorldTransferMutation,
    state: &mut WorldFlowTransactionState,
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
    let result_payload = postcard::to_stdvec(&StoredWorldFlowOutcome {
        code: *code,
        snapshot: snapshot.clone(),
        transfer_id: *transfer_id,
    })
    .map_err(|_| PersistenceError::CorruptStoredWorldFlow)?;
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
        command_kind: command_kind(&mutation.payload.command),
        transfer_id: *transfer_id,
        pre_character_version: state.character.character_version,
        post_character_version: state.location.character_version(),
        result_code: result_code(*code),
        result_payload,
    });
    Ok(())
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

const fn command_kind(command: &WorldTransferCommand) -> i16 {
    match command {
        WorldTransferCommand::EnterHallFromCharacterSelect => 0,
        WorldTransferCommand::ReturnToCharacterSelect => 1,
        WorldTransferCommand::UsePortal { .. } => 2,
        WorldTransferCommand::UseCommittedExtraction { .. } => 3,
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

fn staged_result(
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

const fn error(request_sequence: u32, code: WorldTransferResultCode) -> WorldFlowResult {
    WorldFlowResult::Error {
        request_sequence,
        code,
        snapshot: None,
    }
}

#[cfg(test)]
mod tests {
    use protocol::{CharacterLocation, ManifestHash, SafeArrival, WireText, WorldTransferPayload};

    use super::*;
    use crate::AccountId;

    #[derive(Debug, Clone, Copy)]
    struct FixedIds;

    impl WorldFlowIdGenerator for FixedIds {
        fn next_transfer_id(&self) -> [u8; 16] {
            [8; 16]
        }

        fn next_lineage_id(&self) -> [u8; 16] {
            [9; 16]
        }

        fn next_restore_point_id(&self) -> [u8; 16] {
            [10; 16]
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct FixedClock;

    impl IdentityClock for FixedClock {
        fn unix_millis(&self) -> u64 {
            10_000
        }
    }

    fn revision() -> WorldFlowContentRevisionV1 {
        WorldFlowContentRevisionV1 {
            records_blake3: ManifestHash::new("a".repeat(64)).unwrap(),
            assets_blake3: ManifestHash::new("b".repeat(64)).unwrap(),
            localization_blake3: ManifestHash::new("c".repeat(64)).unwrap(),
        }
    }

    fn authenticated() -> AuthenticatedAccount {
        AuthenticatedAccount {
            account_id: AccountId::new([1; 16]).unwrap(),
            namespace: AuthenticatedNamespace::WipeableTest,
        }
    }

    fn mutation(command: WorldTransferCommand, version: u64) -> WorldTransferMutation {
        let payload = WorldTransferPayload {
            content_revision: revision(),
            command,
        };
        WorldTransferMutation {
            mutation_id: [3; 16],
            character_id: [2; 16],
            expected_character_version: version,
            issued_at_unix_millis: 9_000,
            payload_hash: payload.canonical_hash(),
            payload,
        }
    }

    fn state(location: StoredWorldLocation) -> WorldFlowTransactionState {
        WorldFlowTransactionState {
            account_version: 1,
            selected_character_id: Some([2; 16]),
            character: persistence::StoredWorldFlowCharacter {
                life_state: 0,
                security_state: 0,
                character_version: location.character_version(),
            },
            location,
            new_receipt: None,
            location_changed: false,
        }
    }

    #[test]
    fn safe_route_consumes_default_then_preserves_character_select_return_arrival() {
        let planner = DormantWorldFlowPlanner::new(FixedIds, FixedClock, revision());
        let mut initial = state(StoredWorldLocation::CharacterSelect {
            character_version: 1,
            next_hall_arrival: StoredSafeArrival::HallDefault,
        });
        let enter = mutation(WorldTransferCommand::EnterHallFromCharacterSelect, 1);
        let result = planner
            .plan_fresh(authenticated(), 1, &enter, &mut initial)
            .unwrap();
        assert!(matches!(
            result,
            WorldFlowResult::Transfer {
                code: WorldTransferResultCode::Accepted,
                snapshot: Some(CharacterLocationSnapshot {
                    location: CharacterLocation::Safe {
                        arrival: SafeArrival::HallDefault,
                        ..
                    },
                    ..
                }),
                ..
            }
        ));
        assert_eq!(initial.location.character_version(), 2);

        initial.character.character_version = 2;
        initial.new_receipt = None;
        initial.location_changed = false;
        let return_to_select = mutation(WorldTransferCommand::ReturnToCharacterSelect, 2);
        planner
            .plan_fresh(authenticated(), 2, &return_to_select, &mut initial)
            .unwrap();
        assert!(matches!(
            &initial.location,
            StoredWorldLocation::CharacterSelect {
                next_hall_arrival: StoredSafeArrival::SpawnAnchor(spawn),
                ..
            } if spawn == CHARACTER_SELECT_RETURN_SPAWN_ID
        ));

        initial.character.character_version = 3;
        initial.new_receipt = None;
        initial.location_changed = false;
        let reenter = mutation(WorldTransferCommand::EnterHallFromCharacterSelect, 3);
        let result = planner
            .plan_fresh(authenticated(), 3, &reenter, &mut initial)
            .unwrap();
        assert!(matches!(
            result,
            WorldFlowResult::Transfer {
                snapshot: Some(CharacterLocationSnapshot {
                    location: CharacterLocation::Safe {
                        arrival: SafeArrival::SpawnAnchor { ref spawn_id },
                        ..
                    },
                    ..
                }),
                ..
            } if spawn_id.as_str() == CHARACTER_SELECT_RETURN_SPAWN_ID
        ));
    }

    #[test]
    fn stale_dead_unselected_and_disabled_portal_results_are_stored_without_mutation() {
        let planner = DormantWorldFlowPlanner::new(FixedIds, FixedClock, revision());
        let base = StoredWorldLocation::CharacterSelect {
            character_version: 1,
            next_hall_arrival: StoredSafeArrival::HallDefault,
        };
        let mut stale = state(base.clone());
        let stale_mutation = mutation(WorldTransferCommand::EnterHallFromCharacterSelect, 2);
        let result = planner
            .plan_fresh(authenticated(), 1, &stale_mutation, &mut stale)
            .unwrap();
        assert!(matches!(
            result,
            WorldFlowResult::Transfer {
                code: WorldTransferResultCode::StateVersionMismatch,
                ..
            }
        ));
        assert!(!stale.location_changed);
        assert!(stale.new_receipt.is_some());

        let mut dead = state(base.clone());
        dead.character.life_state = 1;
        let result = planner
            .plan_fresh(
                authenticated(),
                1,
                &mutation(WorldTransferCommand::EnterHallFromCharacterSelect, 1),
                &mut dead,
            )
            .unwrap();
        assert!(matches!(
            result,
            WorldFlowResult::Transfer {
                code: WorldTransferResultCode::CharacterDead,
                ..
            }
        ));

        let mut unselected = state(base.clone());
        unselected.selected_character_id = None;
        let result = planner
            .plan_fresh(
                authenticated(),
                1,
                &mutation(WorldTransferCommand::EnterHallFromCharacterSelect, 1),
                &mut unselected,
            )
            .unwrap();
        assert!(matches!(
            result,
            WorldFlowResult::Transfer {
                code: WorldTransferResultCode::NoSelectedCharacter,
                ..
            }
        ));

        let mut portal = state(base);
        let result = planner
            .plan_fresh(
                authenticated(),
                1,
                &mutation(
                    WorldTransferCommand::UsePortal {
                        portal_id: WireText::new("station.realm_gate").unwrap(),
                    },
                    1,
                ),
                &mut portal,
            )
            .unwrap();
        assert!(matches!(
            result,
            WorldFlowResult::Transfer {
                code: WorldTransferResultCode::DestinationDisabled,
                ..
            }
        ));
        assert!(!portal.location_changed);
    }

    #[test]
    fn exact_realm_gate_stages_distinct_danger_identities_after_safe_preflight() {
        let planner = DormantWorldFlowPlanner::new(FixedIds, FixedClock, revision());
        let mut state = state(StoredWorldLocation::Safe {
            character_version: 4,
            location_content_id: HALL_ID.to_owned(),
            arrival: StoredSafeArrival::HallDefault,
        });
        state.character.character_version = 4;
        let mutation = mutation(
            WorldTransferCommand::UsePortal {
                portal_id: WireText::new(REALM_GATE_ID).unwrap(),
            },
            4,
        );
        let result = planner
            .plan_fresh(authenticated(), 7, &mutation, &mut state)
            .unwrap();
        assert!(matches!(
            result,
            WorldFlowResult::Transfer {
                code: WorldTransferResultCode::Accepted,
                transfer_id: Some(transfer_id),
                snapshot: Some(CharacterLocationSnapshot {
                    location: CharacterLocation::Danger {
                        ref location_id,
                        instance_lineage_id,
                        entry_restore_point_id,
                    },
                    ..
                }),
                ..
            } if transfer_id == [8; 16]
                && instance_lineage_id == [9; 16]
                && entry_restore_point_id == [10; 16]
                && location_id.as_str() == CORE_MICROREALM_ID
        ));
        assert!(state.location_changed);
    }

    #[test]
    fn exact_replay_resequences_and_changed_binding_conflicts() {
        let planner = DormantWorldFlowPlanner::new(FixedIds, FixedClock, revision());
        let mut state = state(StoredWorldLocation::CharacterSelect {
            character_version: 1,
            next_hall_arrival: StoredSafeArrival::HallDefault,
        });
        let mutation = mutation(WorldTransferCommand::EnterHallFromCharacterSelect, 1);
        planner
            .plan_fresh(authenticated(), 1, &mutation, &mut state)
            .unwrap();
        let receipt = state.new_receipt.unwrap();
        assert!(matches!(
            DormantWorldFlowPlanner::<FixedIds, FixedClock>::replay(9, &mutation, &receipt),
            WorldFlowResult::Transfer {
                request_sequence: 9,
                code: WorldTransferResultCode::Accepted,
                ..
            }
        ));
        let mut changed = mutation.clone();
        changed.payload.content_revision.records_blake3 =
            ManifestHash::new("f".repeat(64)).unwrap();
        changed.payload_hash = changed.payload.canonical_hash();
        assert!(matches!(
            DormantWorldFlowPlanner::<FixedIds, FixedClock>::replay(10, &changed, &receipt),
            WorldFlowResult::Transfer {
                code: WorldTransferResultCode::IdempotencyConflict,
                ..
            }
        ));
    }
}
