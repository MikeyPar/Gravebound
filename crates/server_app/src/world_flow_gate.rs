//! Read-only location projection and fail-closed normal route gate for `GB-M03-03B`.

use std::future::Future;

use persistence::{PostgresPersistence, StoredSafeArrival, StoredWorldLocation};
use protocol::{
    CharacterLocation, CharacterLocationSnapshot, SafeArrival, WireText, WorldFlowFrame,
    WorldFlowRequest, WorldFlowResult, WorldTransferResultCode,
};
use thiserror::Error;

use crate::{AccountId, AuthenticatedAccount, AuthenticatedNamespace, IdentityClock};

pub trait WorldFlowLocationRepository: Send + Sync {
    fn selected_character(
        &self,
        account_id: AccountId,
    ) -> impl Future<Output = Result<Option<[u8; 16]>, WorldFlowRepositoryError>> + Send;

    fn character_owner(
        &self,
        character_id: [u8; 16],
    ) -> impl Future<Output = Result<Option<AccountId>, WorldFlowRepositoryError>> + Send;

    fn location(
        &self,
        account_id: AccountId,
        character_id: [u8; 16],
    ) -> impl Future<Output = Result<Option<CharacterLocationSnapshot>, WorldFlowRepositoryError>> + Send;
}

#[derive(Debug, Clone)]
pub struct PostgresWorldFlowLocationRepository {
    persistence: PostgresPersistence,
}

impl PostgresWorldFlowLocationRepository {
    pub const fn new(persistence: PostgresPersistence) -> Self {
        Self { persistence }
    }
}

impl WorldFlowLocationRepository for PostgresWorldFlowLocationRepository {
    async fn selected_character(
        &self,
        account_id: AccountId,
    ) -> Result<Option<[u8; 16]>, WorldFlowRepositoryError> {
        self.persistence
            .world_flow_selected_character(account_id.as_bytes())
            .await
            .map_err(|_| WorldFlowRepositoryError::Unavailable)
    }

    async fn character_owner(
        &self,
        character_id: [u8; 16],
    ) -> Result<Option<AccountId>, WorldFlowRepositoryError> {
        self.persistence
            .identity_character_owner(character_id)
            .await
            .map(|owner| owner.and_then(AccountId::new))
            .map_err(|_| WorldFlowRepositoryError::Unavailable)
    }

    async fn location(
        &self,
        account_id: AccountId,
        character_id: [u8; 16],
    ) -> Result<Option<CharacterLocationSnapshot>, WorldFlowRepositoryError> {
        self.persistence
            .world_location(account_id.as_bytes(), character_id)
            .await
            .map_err(|_| WorldFlowRepositoryError::Unavailable)?
            .map(|stored| stored_location_snapshot(character_id, stored))
            .transpose()
    }
}

#[derive(Debug, Clone)]
pub struct WorldFlowGateService<Repository, Clock> {
    repository: Repository,
    clock: Clock,
    required_manifest_hash: protocol::ManifestHash,
}

impl<Repository, Clock> WorldFlowGateService<Repository, Clock>
where
    Repository: WorldFlowLocationRepository,
    Clock: IdentityClock,
{
    pub const fn new(
        repository: Repository,
        clock: Clock,
        required_manifest_hash: protocol::ManifestHash,
    ) -> Self {
        Self {
            repository,
            clock,
            required_manifest_hash,
        }
    }

    pub async fn handle(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &WorldFlowFrame,
    ) -> WorldFlowResult {
        if frame.validate().is_err()
            || authenticated.namespace != AuthenticatedNamespace::WipeableTest
        {
            return error(
                frame.sequence,
                WorldTransferResultCode::ServiceUnavailable,
                None,
            );
        }
        match &frame.request {
            WorldFlowRequest::Location {
                character_id,
                content_manifest_hash,
            } => {
                self.handle_location(
                    frame.sequence,
                    authenticated.account_id,
                    *character_id,
                    content_manifest_hash,
                )
                .await
            }
            WorldFlowRequest::Transfer(mutation) => {
                self.handle_transfer(frame.sequence, authenticated.account_id, mutation)
                    .await
            }
        }
    }

    async fn handle_location(
        &self,
        request_sequence: u32,
        account_id: AccountId,
        character_id: [u8; 16],
        content_manifest_hash: &protocol::ManifestHash,
    ) -> WorldFlowResult {
        if content_manifest_hash != &self.required_manifest_hash {
            return error(
                request_sequence,
                WorldTransferResultCode::ContentMismatch,
                None,
            );
        }
        match self.owned_location(account_id, character_id).await {
            Ok(snapshot) => WorldFlowResult::Location {
                request_sequence,
                snapshot,
            },
            Err((code, snapshot)) => error(request_sequence, code, snapshot),
        }
    }

    async fn handle_transfer(
        &self,
        request_sequence: u32,
        account_id: AccountId,
        mutation: &protocol::WorldTransferMutation,
    ) -> WorldFlowResult {
        let reject = |code, snapshot| {
            transfer_rejection(request_sequence, mutation.mutation_id, code, snapshot)
        };
        if mutation.payload.content_manifest_hash != self.required_manifest_hash {
            return reject(WorldTransferResultCode::ContentMismatch, None);
        }
        if mutation.issued_at_unix_millis > self.clock.unix_millis() {
            return reject(WorldTransferResultCode::IssuedAtInvalid, None);
        }
        let selected = match self.repository.selected_character(account_id).await {
            Ok(Some(selected)) => selected,
            Ok(None) => return reject(WorldTransferResultCode::NoSelectedCharacter, None),
            Err(_) => return reject(WorldTransferResultCode::ServiceUnavailable, None),
        };
        let location = match self.owned_location(account_id, mutation.character_id).await {
            Ok(snapshot) => snapshot,
            Err((code, snapshot)) => return reject(code, snapshot),
        };
        if selected != mutation.character_id {
            return reject(WorldTransferResultCode::InvalidSource, Some(location));
        }
        if location.character_version != mutation.expected_character_version {
            return reject(
                WorldTransferResultCode::StateVersionMismatch,
                Some(location),
            );
        }
        // Approved SPEC-CONFLICT-006: no normal allocation or durable write until every owning
        // restore, item, Oath/Bargain, death, extraction, and Recall package passes.
        reject(WorldTransferResultCode::StageDisabled, Some(location))
    }

    async fn owned_location(
        &self,
        account_id: AccountId,
        character_id: [u8; 16],
    ) -> Result<
        CharacterLocationSnapshot,
        (WorldTransferResultCode, Option<CharacterLocationSnapshot>),
    > {
        let owner = self
            .repository
            .character_owner(character_id)
            .await
            .map_err(|_| (WorldTransferResultCode::ServiceUnavailable, None))?;
        match owner {
            Some(owner) if owner != account_id => {
                return Err((WorldTransferResultCode::CharacterNotOwned, None));
            }
            None => return Err((WorldTransferResultCode::CharacterNotFound, None)),
            Some(_) => {}
        }
        self.repository
            .location(account_id, character_id)
            .await
            .map_err(|_| (WorldTransferResultCode::ServiceUnavailable, None))?
            .ok_or((WorldTransferResultCode::CharacterNotFound, None))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum WorldFlowRepositoryError {
    #[error("world-flow repository is unavailable")]
    Unavailable,
    #[error("stored world-flow projection is corrupt")]
    Corrupt,
}

fn stored_location_snapshot(
    character_id: [u8; 16],
    stored: StoredWorldLocation,
) -> Result<CharacterLocationSnapshot, WorldFlowRepositoryError> {
    let character_version =
        u64::try_from(stored.character_version()).map_err(|_| WorldFlowRepositoryError::Corrupt)?;
    let location = match stored {
        StoredWorldLocation::CharacterSelect { .. } => CharacterLocation::CharacterSelect,
        StoredWorldLocation::Safe {
            location_content_id,
            arrival,
            ..
        } => CharacterLocation::Safe {
            location_id: WireText::new(location_content_id)
                .map_err(|_| WorldFlowRepositoryError::Corrupt)?,
            arrival: match arrival {
                StoredSafeArrival::HallDefault => SafeArrival::HallDefault,
                StoredSafeArrival::SpawnAnchor(spawn_id) => SafeArrival::SpawnAnchor {
                    spawn_id: WireText::new(spawn_id)
                        .map_err(|_| WorldFlowRepositoryError::Corrupt)?,
                },
            },
        },
        StoredWorldLocation::Danger {
            location_content_id,
            instance_lineage_id,
            entry_restore_point_id,
            ..
        } => CharacterLocation::Danger {
            location_id: WireText::new(location_content_id)
                .map_err(|_| WorldFlowRepositoryError::Corrupt)?,
            instance_lineage_id,
            entry_restore_point_id,
        },
    };
    let snapshot = CharacterLocationSnapshot {
        character_id,
        character_version,
        location,
    };
    snapshot
        .validate()
        .map_err(|_| WorldFlowRepositoryError::Corrupt)?;
    Ok(snapshot)
}

fn error(
    request_sequence: u32,
    code: WorldTransferResultCode,
    snapshot: Option<CharacterLocationSnapshot>,
) -> WorldFlowResult {
    WorldFlowResult::Error {
        request_sequence,
        code,
        snapshot,
    }
}

fn transfer_rejection(
    request_sequence: u32,
    mutation_id: [u8; 16],
    code: WorldTransferResultCode,
    snapshot: Option<CharacterLocationSnapshot>,
) -> WorldFlowResult {
    WorldFlowResult::Transfer {
        request_sequence,
        mutation_id,
        accepted: false,
        code,
        snapshot,
        transfer_id: None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use protocol::{
        ManifestHash, WorldTransferCommand, WorldTransferMutation, WorldTransferPayload,
    };

    use super::*;

    #[derive(Debug, Clone)]
    struct StaticRepository {
        account: AccountId,
        selected: Option<[u8; 16]>,
        locations: BTreeMap<[u8; 16], CharacterLocationSnapshot>,
    }

    impl WorldFlowLocationRepository for StaticRepository {
        async fn selected_character(
            &self,
            account_id: AccountId,
        ) -> Result<Option<[u8; 16]>, WorldFlowRepositoryError> {
            Ok((account_id == self.account)
                .then_some(self.selected)
                .flatten())
        }

        async fn character_owner(
            &self,
            character_id: [u8; 16],
        ) -> Result<Option<AccountId>, WorldFlowRepositoryError> {
            Ok(self
                .locations
                .contains_key(&character_id)
                .then_some(self.account))
        }

        async fn location(
            &self,
            account_id: AccountId,
            character_id: [u8; 16],
        ) -> Result<Option<CharacterLocationSnapshot>, WorldFlowRepositoryError> {
            Ok((account_id == self.account)
                .then(|| self.locations.get(&character_id).cloned())
                .flatten())
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct Clock;

    impl IdentityClock for Clock {
        fn unix_millis(&self) -> u64 {
            10_000
        }
    }

    fn gate(
        selected: Option<[u8; 16]>,
    ) -> (
        WorldFlowGateService<StaticRepository, Clock>,
        AuthenticatedAccount,
    ) {
        let account = AccountId::new([1; 16]).unwrap();
        let character = [2; 16];
        let repository = StaticRepository {
            account,
            selected,
            locations: BTreeMap::from([(
                character,
                CharacterLocationSnapshot {
                    character_id: character,
                    character_version: 1,
                    location: CharacterLocation::CharacterSelect,
                },
            )]),
        };
        (
            WorldFlowGateService::new(
                repository,
                Clock,
                ManifestHash::new("a".repeat(64)).unwrap(),
            ),
            AuthenticatedAccount {
                account_id: account,
                namespace: AuthenticatedNamespace::WipeableTest,
            },
        )
    }

    fn transfer() -> WorldFlowFrame {
        let payload = WorldTransferPayload {
            content_manifest_hash: ManifestHash::new("a".repeat(64)).unwrap(),
            command: WorldTransferCommand::EnterHallFromCharacterSelect,
        };
        WorldFlowFrame {
            sequence: 1,
            request: WorldFlowRequest::Transfer(WorldTransferMutation {
                mutation_id: [3; 16],
                character_id: [2; 16],
                expected_character_version: 1,
                issued_at_unix_millis: 9_000,
                payload_hash: payload.canonical_hash(),
                payload,
            }),
        }
    }

    #[tokio::test]
    async fn normal_selected_transfer_is_stage_disabled_without_a_transfer_identity() {
        let (service, account) = gate(Some([2; 16]));
        let result = service.handle(account, &transfer()).await;
        assert!(matches!(
            result,
            WorldFlowResult::Transfer {
                accepted: false,
                code: WorldTransferResultCode::StageDisabled,
                transfer_id: None,
                snapshot: Some(_),
                ..
            }
        ));
        assert_eq!(result.validate(), Ok(()));
    }

    #[tokio::test]
    async fn no_selection_stale_version_and_foreign_character_are_typed() {
        let (service, account) = gate(None);
        assert!(matches!(
            service.handle(account, &transfer()).await,
            WorldFlowResult::Transfer {
                code: WorldTransferResultCode::NoSelectedCharacter,
                ..
            }
        ));

        let (service, account) = gate(Some([2; 16]));
        let mut stale = transfer();
        let WorldFlowRequest::Transfer(mutation) = &mut stale.request else {
            unreachable!();
        };
        mutation.expected_character_version = 2;
        assert!(matches!(
            service.handle(account, &stale).await,
            WorldFlowResult::Transfer {
                code: WorldTransferResultCode::StateVersionMismatch,
                ..
            }
        ));

        let mut foreign = transfer();
        let WorldFlowRequest::Transfer(mutation) = &mut foreign.request else {
            unreachable!();
        };
        mutation.character_id = [9; 16];
        assert!(matches!(
            service.handle(account, &foreign).await,
            WorldFlowResult::Transfer {
                code: WorldTransferResultCode::CharacterNotFound,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn location_query_returns_the_durable_projection() {
        let (service, account) = gate(Some([2; 16]));
        let frame = WorldFlowFrame {
            sequence: 7,
            request: WorldFlowRequest::Location {
                character_id: [2; 16],
                content_manifest_hash: ManifestHash::new("a".repeat(64)).unwrap(),
            },
        };
        assert!(matches!(
            service.handle(account, &frame).await,
            WorldFlowResult::Location {
                request_sequence: 7,
                snapshot: CharacterLocationSnapshot {
                    location: CharacterLocation::CharacterSelect,
                    ..
                }
            }
        ));
    }
}
