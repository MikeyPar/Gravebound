//! Read-only authoritative progression projection for `GB-M03-04A`.

use std::future::Future;

use persistence::{PersistenceError, PostgresPersistence, StoredProgressionContract};
use protocol::{ProgressionQueryFrame, ProgressionResult, ProgressionResultCode};
use sim_core::CoreProgressionState;
use thiserror::Error;

use crate::{
    AccountId, AuthenticatedAccount, AuthenticatedNamespace, CoreProgressionRules,
    ProgressionAwardContext, ProgressionAwardError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressionQuerySnapshot {
    pub progression: CoreProgressionState,
    pub current_health: u32,
    pub life_state: i16,
    pub security_state: i16,
}

pub trait ProgressionQueryRepository: Send + Sync {
    fn character_owner(
        &self,
        character_id: [u8; 16],
    ) -> impl Future<Output = Result<Option<AccountId>, ProgressionQueryRepositoryError>> + Send;

    fn snapshot(
        &self,
        account_id: AccountId,
        character_id: [u8; 16],
    ) -> impl Future<Output = Result<ProgressionQuerySnapshot, ProgressionQueryRepositoryError>> + Send;
}

#[derive(Debug, Clone)]
pub struct PostgresProgressionQueryRepository {
    persistence: PostgresPersistence,
    contract: StoredProgressionContract,
}

impl PostgresProgressionQueryRepository {
    pub fn new(
        persistence: PostgresPersistence,
        content: &sim_content::CoreDevelopmentProgression,
    ) -> Result<Self, ProgressionAwardError> {
        let rules = CoreProgressionRules::from_content(content)?;
        Ok(Self {
            persistence,
            contract: stored_contract(&rules)?,
        })
    }
}

impl ProgressionQueryRepository for PostgresProgressionQueryRepository {
    async fn character_owner(
        &self,
        character_id: [u8; 16],
    ) -> Result<Option<AccountId>, ProgressionQueryRepositoryError> {
        self.persistence
            .identity_character_owner(character_id)
            .await
            .map(|owner| owner.and_then(AccountId::new))
            .map_err(|_| ProgressionQueryRepositoryError::Unavailable)
    }

    async fn snapshot(
        &self,
        account_id: AccountId,
        character_id: [u8; 16],
    ) -> Result<ProgressionQuerySnapshot, ProgressionQueryRepositoryError> {
        let stored = self
            .persistence
            .progression_snapshot(account_id.as_bytes(), character_id, &self.contract)
            .await
            .map_err(|error| match error {
                PersistenceError::CorruptStoredProgression => {
                    ProgressionQueryRepositoryError::Corrupt
                }
                _ => ProgressionQueryRepositoryError::Unavailable,
            })?
            .ok_or(ProgressionQueryRepositoryError::Corrupt)?;
        Ok(ProgressionQuerySnapshot {
            progression: CoreProgressionState {
                total_xp: stored
                    .progression
                    .total_xp
                    .try_into()
                    .map_err(|_| ProgressionQueryRepositoryError::Corrupt)?,
                level: stored
                    .progression
                    .level
                    .try_into()
                    .map_err(|_| ProgressionQueryRepositoryError::Corrupt)?,
                progression_version: stored
                    .progression
                    .progression_version
                    .try_into()
                    .map_err(|_| ProgressionQueryRepositoryError::Corrupt)?,
            },
            current_health: stored
                .progression
                .current_health
                .try_into()
                .map_err(|_| ProgressionQueryRepositoryError::Corrupt)?,
            life_state: stored.character.life_state,
            security_state: stored.character.security_state,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ProgressionQueryService<Repository> {
    repository: Repository,
    rules: CoreProgressionRules,
}

impl<Repository> ProgressionQueryService<Repository>
where
    Repository: ProgressionQueryRepository,
{
    pub fn new(
        repository: Repository,
        content: &sim_content::CoreDevelopmentProgression,
    ) -> Result<Self, ProgressionAwardError> {
        Ok(Self {
            repository,
            rules: CoreProgressionRules::from_content(content)?,
        })
    }

    pub async fn handle(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &ProgressionQueryFrame,
    ) -> ProgressionResult {
        if frame.validate().is_err()
            || authenticated.namespace != AuthenticatedNamespace::WipeableTest
        {
            return error(frame.sequence, ProgressionResultCode::ServiceUnavailable);
        }
        if frame.progression_content_revision != *self.rules.records_revision() {
            return error(frame.sequence, ProgressionResultCode::ContentMismatch);
        }
        let owner = match self.repository.character_owner(frame.character_id).await {
            Ok(Some(owner)) => owner,
            Ok(None) => return error(frame.sequence, ProgressionResultCode::CharacterNotFound),
            Err(_) => return error(frame.sequence, ProgressionResultCode::ServiceUnavailable),
        };
        if owner != authenticated.account_id {
            return error(frame.sequence, ProgressionResultCode::CharacterNotOwned);
        }
        let Ok(snapshot) = self
            .repository
            .snapshot(authenticated.account_id, frame.character_id)
            .await
        else {
            return error(frame.sequence, ProgressionResultCode::ServiceUnavailable);
        };
        if snapshot.life_state != 0 {
            return error(frame.sequence, ProgressionResultCode::CharacterDead);
        }
        if snapshot.security_state != 0 {
            return error(frame.sequence, ProgressionResultCode::ServiceUnavailable);
        }
        let Ok(projection) = self.rules.project(
            frame.character_id,
            ProgressionAwardContext {
                selected_character_id: Some(frame.character_id),
                life_state: snapshot.life_state,
                security_state: snapshot.security_state,
                progression: snapshot.progression,
                current_health: snapshot.current_health,
                first_clear_available: false,
            },
        ) else {
            return error(frame.sequence, ProgressionResultCode::ServiceUnavailable);
        };
        ProgressionResult::Snapshot {
            request_sequence: frame.sequence,
            projection,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProgressionQueryRepositoryError {
    #[error("progression repository is unavailable")]
    Unavailable,
    #[error("stored progression projection is corrupt")]
    Corrupt,
}

fn stored_contract(
    rules: &CoreProgressionRules,
) -> Result<StoredProgressionContract, ProgressionAwardError> {
    let values = rules
        .curve()
        .cumulative_xp
        .map(i32::try_from)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ProgressionAwardError::InvalidContent)?;
    Ok(StoredProgressionContract {
        cumulative_xp: values
            .try_into()
            .map_err(|_| ProgressionAwardError::InvalidContent)?,
    })
}

const fn error(request_sequence: u32, code: ProgressionResultCode) -> ProgressionResult {
    ProgressionResult::Error {
        request_sequence,
        code,
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    #[derive(Debug, Clone, Copy)]
    struct StaticRepository {
        owner: Option<AccountId>,
        snapshot: Result<ProgressionQuerySnapshot, ProgressionQueryRepositoryError>,
    }

    impl ProgressionQueryRepository for StaticRepository {
        async fn character_owner(
            &self,
            _character_id: [u8; 16],
        ) -> Result<Option<AccountId>, ProgressionQueryRepositoryError> {
            Ok(self.owner)
        }

        async fn snapshot(
            &self,
            _account_id: AccountId,
            _character_id: [u8; 16],
        ) -> Result<ProgressionQuerySnapshot, ProgressionQueryRepositoryError> {
            self.snapshot
        }
    }

    fn content_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn content() -> sim_content::CoreDevelopmentProgression {
        sim_content::load_core_development_progression(&content_root()).unwrap()
    }

    fn account(value: u8) -> AccountId {
        AccountId::new([value; 16]).unwrap()
    }

    fn frame(content: &sim_content::CoreDevelopmentProgression) -> ProgressionQueryFrame {
        let rules = CoreProgressionRules::from_content(content).unwrap();
        ProgressionQueryFrame {
            sequence: 7,
            character_id: [2; 16],
            progression_content_revision: rules.records_revision().clone(),
        }
    }

    fn snapshot() -> ProgressionQuerySnapshot {
        ProgressionQuerySnapshot {
            progression: CoreProgressionState {
                total_xp: 450,
                level: 4,
                progression_version: 3,
            },
            current_health: 91,
            life_state: 0,
            security_state: 0,
        }
    }

    #[tokio::test]
    async fn owned_living_character_returns_exact_projection() {
        let content = content();
        let service = ProgressionQueryService::new(
            StaticRepository {
                owner: Some(account(1)),
                snapshot: Ok(snapshot()),
            },
            &content,
        )
        .unwrap();
        let result = service
            .handle(
                AuthenticatedAccount {
                    account_id: account(1),
                    namespace: AuthenticatedNamespace::WipeableTest,
                },
                &frame(&content),
            )
            .await;
        assert!(matches!(
            result,
            ProgressionResult::Snapshot {
                request_sequence: 7,
                ..
            }
        ));
        assert_eq!(result.validate(), Ok(()));
    }

    #[tokio::test]
    async fn foreign_dead_and_mismatched_queries_fail_closed() {
        let content = content();
        let authenticated = AuthenticatedAccount {
            account_id: account(1),
            namespace: AuthenticatedNamespace::WipeableTest,
        };
        let foreign = ProgressionQueryService::new(
            StaticRepository {
                owner: Some(account(9)),
                snapshot: Ok(snapshot()),
            },
            &content,
        )
        .unwrap();
        assert!(matches!(
            foreign.handle(authenticated, &frame(&content)).await,
            ProgressionResult::Error {
                code: ProgressionResultCode::CharacterNotOwned,
                ..
            }
        ));

        let mut dead_snapshot = snapshot();
        dead_snapshot.life_state = 1;
        let dead = ProgressionQueryService::new(
            StaticRepository {
                owner: Some(account(1)),
                snapshot: Ok(dead_snapshot),
            },
            &content,
        )
        .unwrap();
        assert!(matches!(
            dead.handle(authenticated, &frame(&content)).await,
            ProgressionResult::Error {
                code: ProgressionResultCode::CharacterDead,
                ..
            }
        ));

        let service = ProgressionQueryService::new(
            StaticRepository {
                owner: Some(account(1)),
                snapshot: Ok(snapshot()),
            },
            &content,
        )
        .unwrap();
        let mut mismatch = frame(&content);
        mismatch.progression_content_revision =
            protocol::ManifestHash::new("f".repeat(64)).unwrap();
        assert!(matches!(
            service.handle(authenticated, &mismatch).await,
            ProgressionResult::Error {
                code: ProgressionResultCode::ContentMismatch,
                ..
            }
        ));
    }
}
