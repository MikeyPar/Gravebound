//! Terminal-first process bootstrap for the ordinary Core private-life runtime.
//!
//! The canonical GDD owns process-crash recovery and terminal precedence, the Content Production
//! Specification owns the exact Hall/private-route content, and the Development Roadmap requires
//! reconnect/restart without duplicate authority. Process restart and within-process reconnect are
//! therefore deliberately separate entry points: only restart may invoke durable crash restore.

use std::{
    collections::BTreeMap,
    future::Future,
    sync::{Arc, Mutex as StdMutex},
};

use persistence::{
    PersistenceError, PostgresPersistence, ResolvedPrivateLifeProcessRestartV1,
    StoredCommittedDeathTerminalV1, StoredCommittedExtractionTerminalV1,
    StoredCommittedRecallTerminalV1, StoredPrivateLifeBootstrapStateV1,
    StoredPrivateLifeBootstrapV1, StoredPrivateLifeHallV1, StoredPrivateLifeSelectedCharacterV1,
    StoredPrivateRouteGenerationV1, StoredSafeArrival,
};
use protocol::{
    CharacterLocation, CorePrivateRouteContentRevisionV1, CorePrivateRoutePhaseV1,
    CorePrivateRouteSceneV1, WorldFlowContentRevisionV1, WorldFlowResult, WorldTransferCommand,
    WorldTransferMutation, WorldTransferResultCode,
};
use thiserror::Error;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

use crate::core_private_route_actor::{
    CorePrivateRouteEnterMicrorealmTransition, CorePrivateRouteReturnToCharacterSelectTransition,
};
use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CorePrivateLifeSessionDirectory,
    CorePrivateLifeSessionError, CorePrivateLifeTransportLease, CorePrivateRouteActorDirectory,
    CorePrivateRouteActorLease, CorePrivateRouteActorPosition, CorePrivateRouteActorSeed,
    CorePrivateRouteRuntimeError, CoreReliableWriter, ProductionRecallClock,
};

/// Durable operations needed by the composition adapter. Keeping this seam narrow makes restart
/// policy independently testable without introducing another gameplay writer.
pub trait CorePrivateLifeBootstrapRepository: Send + Sync {
    fn load(
        &self,
        account_id: [u8; 16],
    ) -> impl Future<Output = Result<StoredPrivateLifeBootstrapV1, PersistenceError>> + Send;

    fn resolve_process_restart(
        &self,
        account_id: [u8; 16],
    ) -> impl Future<Output = Result<ResolvedPrivateLifeProcessRestartV1, PersistenceError>> + Send;

    fn allocate_route_generation(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> impl Future<Output = Result<StoredPrivateRouteGenerationV1, PersistenceError>> + Send;
}

impl CorePrivateLifeBootstrapRepository for PostgresPersistence {
    async fn load(
        &self,
        account_id: [u8; 16],
    ) -> Result<StoredPrivateLifeBootstrapV1, PersistenceError> {
        self.load_private_life_bootstrap_v1(account_id).await
    }

    async fn resolve_process_restart(
        &self,
        account_id: [u8; 16],
    ) -> Result<ResolvedPrivateLifeProcessRestartV1, PersistenceError> {
        self.resolve_private_life_process_restart_v1(account_id)
            .await
    }

    async fn allocate_route_generation(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> Result<StoredPrivateRouteGenerationV1, PersistenceError> {
        self.allocate_private_route_generation_v1(account_id, character_id)
            .await
    }
}

/// Ordered bootstrap projection. Terminal variants intentionally contain the stored terminal
/// before the Hall projection so callers cannot publish destination control first.
#[derive(Debug)]
pub enum CorePrivateLifeBootstrapDisposition {
    AwaitIdentityBootstrap,
    CharacterSelect {
        selected_character: Option<StoredPrivateLifeSelectedCharacterV1>,
        next_hall_arrival: Option<StoredSafeArrival>,
    },
    HallReady {
        hall: StoredPrivateLifeHallV1,
        route: CorePrivateRouteActorLease,
    },
    StorageResolutionRequired {
        hall: StoredPrivateLifeHallV1,
    },
    DeathCommitted {
        terminal: Box<StoredCommittedDeathTerminalV1>,
    },
    ExtractionCommitted {
        terminal: Box<StoredCommittedExtractionTerminalV1>,
        hall: StoredPrivateLifeHallV1,
        route: Option<CorePrivateRouteActorLease>,
    },
    RecallCommitted {
        terminal: Box<StoredCommittedRecallTerminalV1>,
        hall: StoredPrivateLifeHallV1,
        route: CorePrivateRouteActorLease,
    },
}

#[derive(Debug)]
pub struct CorePrivateLifeBootstrapOutcome {
    pub writer: Arc<CoreReliableWriter>,
    pub disposition: CorePrivateLifeBootstrapDisposition,
}

#[derive(Debug)]
pub struct CorePrivateLifeReattachOutcome {
    pub writer: Arc<CoreReliableWriter>,
    pub route: Option<CorePrivateRouteActorLease>,
}

#[derive(Debug, Error)]
pub enum CorePrivateLifeBootstrapError {
    #[error("private-life bootstrap authentication or transport binding is invalid")]
    InvalidBinding,
    #[error("private-life bootstrap runtime is shutting down")]
    Retired,
    #[error("retained private-life actor disagrees with durable Hall authority")]
    RetainedActorMismatch,
    #[error("private-life persistence failed: {0}")]
    Persistence(#[source] PersistenceError),
    #[error("private-life session failed: {0}")]
    Session(#[from] CorePrivateLifeSessionError),
    #[error("private-life route runtime failed: {0}")]
    Route(#[from] CorePrivateRouteRuntimeError),
}

/// Owns the account-to-route lease association for process bootstrap and reconnect. The durable
/// repository remains the source of truth; the map only prevents duplicate in-process actors.
#[derive(Debug)]
pub struct CorePrivateLifeRuntimeBootstrapAdapter<Repository> {
    repository: Repository,
    route_directory: CorePrivateRouteActorDirectory,
    route_revision: CorePrivateRouteContentRevisionV1,
    world_flow_revision: WorldFlowContentRevisionV1,
    account_locks: StdMutex<BTreeMap<[u8; 16], Arc<AsyncMutex<()>>>>,
    retained_routes: StdMutex<BTreeMap<[u8; 16], CorePrivateRouteActorLease>>,
    retired_routes: StdMutex<BTreeMap<[u8; 16], CorePrivateRouteActorLease>>,
    hall_reconciliations: StdMutex<BTreeMap<[u8; 16], CommittedHallTransition>>,
    accepting: StdMutex<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommittedHallTransition {
    mutation_id: [u8; 16],
    transfer_id: [u8; 16],
    character_id: [u8; 16],
    source_character_version: u64,
    destination_character_version: u64,
    content_revision: WorldFlowContentRevisionV1,
}

impl<Repository> CorePrivateLifeRuntimeBootstrapAdapter<Repository>
where
    Repository: CorePrivateLifeBootstrapRepository,
{
    pub fn new(
        repository: Repository,
        route_directory: CorePrivateRouteActorDirectory,
        route_revision: CorePrivateRouteContentRevisionV1,
        world_flow_revision: WorldFlowContentRevisionV1,
    ) -> Result<Self, CorePrivateLifeBootstrapError> {
        route_revision
            .validate()
            .map_err(|_| CorePrivateLifeBootstrapError::InvalidBinding)?;
        if zero_world_revision(&world_flow_revision) {
            return Err(CorePrivateLifeBootstrapError::InvalidBinding);
        }
        Ok(Self {
            repository,
            route_directory,
            route_revision,
            world_flow_revision,
            account_locks: StdMutex::new(BTreeMap::new()),
            retained_routes: StdMutex::new(BTreeMap::new()),
            retired_routes: StdMutex::new(BTreeMap::new()),
            hall_reconciliations: StdMutex::new(BTreeMap::new()),
            accepting: StdMutex::new(true),
        })
    }

    /// Process-start-only entry point. A danger snapshot is atomically restored to Hall (or loses
    /// to a committed terminal) before any runtime actor is created.
    pub async fn bootstrap_process_restart<Clock, TickSource>(
        &self,
        authenticated: AuthenticatedAccount,
        transport: CorePrivateLifeTransportLease,
        sessions: &CorePrivateLifeSessionDirectory<Clock, TickSource>,
    ) -> Result<CorePrivateLifeBootstrapOutcome, CorePrivateLifeBootstrapError>
    where
        Clock: ProductionRecallClock + 'static,
        TickSource: crate::CoreRecallAuthoritativeTick + 'static,
    {
        let (account_id, writer, _guard) = self.begin(authenticated, transport, sessions).await?;
        let bootstrap = match self.repository.resolve_process_restart(account_id).await {
            Ok(resolved) => resolved.bootstrap,
            Err(PersistenceError::PrivateLifeBootstrapAccountNotFound) => {
                return Ok(CorePrivateLifeBootstrapOutcome {
                    writer,
                    disposition: CorePrivateLifeBootstrapDisposition::AwaitIdentityBootstrap,
                });
            }
            Err(error) => return Err(CorePrivateLifeBootstrapError::Persistence(error)),
        };
        let disposition = self.map_bootstrap(authenticated, bootstrap).await?;
        Ok(CorePrivateLifeBootstrapOutcome {
            writer,
            disposition,
        })
    }

    /// Explicit refresh after identity or a committed non-Bell world transition. It observes
    /// durable state but never invokes process-crash restoration.
    pub async fn refresh_after_identity_or_transition<Clock, TickSource>(
        &self,
        authenticated: AuthenticatedAccount,
        transport: CorePrivateLifeTransportLease,
        sessions: &CorePrivateLifeSessionDirectory<Clock, TickSource>,
    ) -> Result<CorePrivateLifeBootstrapOutcome, CorePrivateLifeBootstrapError>
    where
        Clock: ProductionRecallClock + 'static,
        TickSource: crate::CoreRecallAuthoritativeTick + 'static,
    {
        let (account_id, writer, _guard) = self.begin(authenticated, transport, sessions).await?;
        let bootstrap = match self.repository.load(account_id).await {
            Ok(bootstrap) => bootstrap,
            Err(PersistenceError::PrivateLifeBootstrapAccountNotFound) => {
                return Ok(CorePrivateLifeBootstrapOutcome {
                    writer,
                    disposition: CorePrivateLifeBootstrapDisposition::AwaitIdentityBootstrap,
                });
            }
            Err(error) => return Err(CorePrivateLifeBootstrapError::Persistence(error)),
        };
        let disposition = self.map_bootstrap(authenticated, bootstrap).await?;
        Ok(CorePrivateLifeBootstrapOutcome {
            writer,
            disposition,
        })
    }

    /// Within-process reconnect reuses retained authority and cannot access the restart resolver.
    pub async fn reattach_within_process<Clock, TickSource>(
        &self,
        authenticated: AuthenticatedAccount,
        transport: CorePrivateLifeTransportLease,
        sessions: &CorePrivateLifeSessionDirectory<Clock, TickSource>,
    ) -> Result<CorePrivateLifeReattachOutcome, CorePrivateLifeBootstrapError>
    where
        Clock: ProductionRecallClock + 'static,
        TickSource: crate::CoreRecallAuthoritativeTick + 'static,
    {
        let (account_id, writer, _guard) = self.begin(authenticated, transport, sessions).await?;
        let route = self.retained_route(account_id);
        if let Some(lease) = route {
            self.route_directory.snapshot(lease)?;
        }
        Ok(CorePrivateLifeReattachOutcome { writer, route })
    }

    pub fn begin_shutdown(&self) {
        *lock(&self.accepting) = false;
        lock(&self.retained_routes).clear();
        lock(&self.retired_routes).clear();
        lock(&self.hall_reconciliations).clear();
        lock(&self.account_locks).clear();
    }

    /// Converges in-memory route ownership after an accepted non-Bell world-flow receipt. The
    /// database result remains authoritative; callback failure is retried by exact receipt replay.
    #[allow(
        clippy::too_many_lines,
        reason = "the accepted receipt validation and three closed route-transition mappings remain contiguous for authority review"
    )]
    pub(crate) async fn reconcile_committed_world_transition(
        &self,
        authenticated: AuthenticatedAccount,
        mutation: &WorldTransferMutation,
        result: &WorldFlowResult,
    ) -> Result<(), CorePrivateLifeBootstrapError> {
        if authenticated.namespace != AuthenticatedNamespace::WipeableTest {
            return Err(CorePrivateLifeBootstrapError::InvalidBinding);
        }
        let WorldFlowResult::Transfer {
            mutation_id,
            accepted,
            code,
            snapshot,
            transfer_id,
            ..
        } = result
        else {
            return Ok(());
        };
        if !accepted {
            return Ok(());
        }
        let (Some(snapshot), Some(transfer_id)) = (snapshot, transfer_id) else {
            return Err(CorePrivateLifeBootstrapError::InvalidBinding);
        };
        if *code != WorldTransferResultCode::Accepted
            || *mutation_id != mutation.mutation_id
            || snapshot.character_id != mutation.character_id
        {
            return Err(CorePrivateLifeBootstrapError::InvalidBinding);
        }
        let account_id = authenticated.account_id.as_bytes();
        let account_lock = {
            let mut locks = lock(&self.account_locks);
            Arc::clone(
                locks
                    .entry(account_id)
                    .or_insert_with(|| Arc::new(AsyncMutex::new(()))),
            )
        };
        let _guard = account_lock.lock_owned().await;
        if !*lock(&self.accepting) {
            return Err(CorePrivateLifeBootstrapError::Retired);
        }
        match &mutation.payload.command {
            WorldTransferCommand::EnterHallFromCharacterSelect => {
                let transition = CommittedHallTransition {
                    mutation_id: mutation.mutation_id,
                    transfer_id: *transfer_id,
                    character_id: mutation.character_id,
                    source_character_version: mutation.expected_character_version,
                    destination_character_version: snapshot.character_version,
                    content_revision: mutation.payload.content_revision.clone(),
                };
                if let Some(stored) = lock(&self.hall_reconciliations).get(&account_id) {
                    if stored == &transition {
                        return Ok(());
                    }
                    if self.retained_route(account_id).is_some() {
                        return Err(CorePrivateLifeBootstrapError::RetainedActorMismatch);
                    }
                }
                let CharacterLocation::Safe { location_id, .. } = &snapshot.location else {
                    return Err(CorePrivateLifeBootstrapError::RetainedActorMismatch);
                };
                if location_id.as_str() != "hub.lantern_halls_01" {
                    return Err(CorePrivateLifeBootstrapError::RetainedActorMismatch);
                }
                let bootstrap = self
                    .repository
                    .load(account_id)
                    .await
                    .map_err(CorePrivateLifeBootstrapError::Persistence)?;
                let StoredPrivateLifeBootstrapStateV1::HallReady(hall) = bootstrap.state else {
                    return Err(CorePrivateLifeBootstrapError::RetainedActorMismatch);
                };
                if snapshot.character_version != hall.character.versions.world {
                    return Err(CorePrivateLifeBootstrapError::RetainedActorMismatch);
                }
                self.ensure_hall_actor(authenticated, &hall).await?;
                lock(&self.retired_routes).remove(&account_id);
                lock(&self.hall_reconciliations).insert(account_id, transition);
                Ok(())
            }
            WorldTransferCommand::ReturnToCharacterSelect => {
                if !matches!(snapshot.location, CharacterLocation::CharacterSelect { .. }) {
                    return Err(CorePrivateLifeBootstrapError::RetainedActorMismatch);
                }
                let lease = self
                    .retained_route(account_id)
                    .or_else(|| lock(&self.retired_routes).get(&account_id).copied());
                let lease = lease.ok_or(CorePrivateLifeBootstrapError::RetainedActorMismatch)?;
                self.route_directory
                    .reconcile_return_to_character_select(
                        lease,
                        CorePrivateRouteReturnToCharacterSelectTransition {
                            transfer_id: *transfer_id,
                            source_character_version: mutation.expected_character_version,
                            destination_character_version: snapshot.character_version,
                            content_revision: mutation.payload.content_revision.clone(),
                        },
                    )
                    .await?;
                lock(&self.retained_routes).remove(&account_id);
                lock(&self.retired_routes).insert(account_id, lease);
                Ok(())
            }
            WorldTransferCommand::UsePortal { portal_id }
                if portal_id.as_str() == "station.realm_gate" =>
            {
                let CharacterLocation::Danger {
                    location_id,
                    instance_lineage_id,
                    ..
                } = &snapshot.location
                else {
                    return Err(CorePrivateLifeBootstrapError::RetainedActorMismatch);
                };
                if location_id.as_str() != "world.core_microrealm_01" {
                    return Err(CorePrivateLifeBootstrapError::RetainedActorMismatch);
                }
                let lease = self
                    .retained_route(account_id)
                    .ok_or(CorePrivateLifeBootstrapError::RetainedActorMismatch)?;
                self.route_directory
                    .reconcile_enter_microrealm(
                        lease,
                        CorePrivateRouteEnterMicrorealmTransition {
                            transfer_id: *transfer_id,
                            source_character_version: mutation.expected_character_version,
                            destination_character_version: snapshot.character_version,
                            instance_lineage_id: *instance_lineage_id,
                            content_revision: mutation.payload.content_revision.clone(),
                        },
                    )
                    .await?;
                Ok(())
            }
            WorldTransferCommand::UsePortal { .. }
            | WorldTransferCommand::UseCommittedExtraction { .. } => Ok(()),
        }
    }

    async fn begin<Clock, TickSource>(
        &self,
        authenticated: AuthenticatedAccount,
        transport: CorePrivateLifeTransportLease,
        sessions: &CorePrivateLifeSessionDirectory<Clock, TickSource>,
    ) -> Result<
        ([u8; 16], Arc<CoreReliableWriter>, OwnedMutexGuard<()>),
        CorePrivateLifeBootstrapError,
    >
    where
        Clock: ProductionRecallClock + 'static,
        TickSource: crate::CoreRecallAuthoritativeTick + 'static,
    {
        let account_id = authenticated.account_id.as_bytes();
        if authenticated.namespace != AuthenticatedNamespace::WipeableTest
            || transport.account_id() != account_id
        {
            return Err(CorePrivateLifeBootstrapError::InvalidBinding);
        }
        if !*lock(&self.accepting) {
            return Err(CorePrivateLifeBootstrapError::Retired);
        }
        let writer = sessions.writer(transport).await?;
        let account_lock = {
            let mut locks = lock(&self.account_locks);
            Arc::clone(
                locks
                    .entry(account_id)
                    .or_insert_with(|| Arc::new(AsyncMutex::new(()))),
            )
        };
        let guard = account_lock.lock_owned().await;
        if !*lock(&self.accepting) {
            return Err(CorePrivateLifeBootstrapError::Retired);
        }
        Ok((account_id, writer, guard))
    }

    async fn map_bootstrap(
        &self,
        authenticated: AuthenticatedAccount,
        bootstrap: StoredPrivateLifeBootstrapV1,
    ) -> Result<CorePrivateLifeBootstrapDisposition, CorePrivateLifeBootstrapError> {
        bootstrap
            .validate()
            .map_err(CorePrivateLifeBootstrapError::Persistence)?;
        if bootstrap.account_id != authenticated.account_id.as_bytes() {
            return Err(CorePrivateLifeBootstrapError::InvalidBinding);
        }
        match bootstrap.state {
            StoredPrivateLifeBootstrapStateV1::CharacterSelect {
                selected_character,
                next_hall_arrival,
            } => Ok(CorePrivateLifeBootstrapDisposition::CharacterSelect {
                selected_character,
                next_hall_arrival,
            }),
            StoredPrivateLifeBootstrapStateV1::HallReady(hall) => {
                let route = self.ensure_hall_actor(authenticated, &hall).await?;
                Ok(CorePrivateLifeBootstrapDisposition::HallReady { hall, route })
            }
            StoredPrivateLifeBootstrapStateV1::HallStorageResolutionRequired(hall) => {
                self.require_no_retained_route(authenticated.account_id.as_bytes())?;
                Ok(CorePrivateLifeBootstrapDisposition::StorageResolutionRequired { hall })
            }
            StoredPrivateLifeBootstrapStateV1::DangerRequiresCrashRestore { .. } => {
                // This can only reach refresh, never restart (the resolver consumes it). Rebuilding
                // danger would violate the crash contract, so retain an exact actor or fail closed.
                Err(CorePrivateLifeBootstrapError::RetainedActorMismatch)
            }
            StoredPrivateLifeBootstrapStateV1::DeathCommitted(terminal) => {
                self.require_no_retained_route(authenticated.account_id.as_bytes())?;
                Ok(CorePrivateLifeBootstrapDisposition::DeathCommitted { terminal })
            }
            StoredPrivateLifeBootstrapStateV1::ExtractionCommitted { hall, terminal } => {
                let route = if hall.resolution_hold.storage_resolution_required {
                    self.require_no_retained_route(authenticated.account_id.as_bytes())?;
                    None
                } else {
                    Some(self.ensure_hall_actor(authenticated, &hall).await?)
                };
                Ok(CorePrivateLifeBootstrapDisposition::ExtractionCommitted {
                    terminal,
                    hall,
                    route,
                })
            }
            StoredPrivateLifeBootstrapStateV1::RecallCommitted { hall, terminal } => {
                let route = self.ensure_hall_actor(authenticated, &hall).await?;
                Ok(CorePrivateLifeBootstrapDisposition::RecallCommitted {
                    terminal,
                    hall,
                    route,
                })
            }
        }
    }

    async fn ensure_hall_actor(
        &self,
        authenticated: AuthenticatedAccount,
        hall: &StoredPrivateLifeHallV1,
    ) -> Result<CorePrivateRouteActorLease, CorePrivateLifeBootstrapError> {
        let account_id = authenticated.account_id.as_bytes();
        if let Some(lease) = self.retained_route(account_id) {
            let projection = self.route_directory.snapshot(lease)?;
            if projection.character_id != hall.character.character_id
                || projection.character_version != hall.character.versions.world
                || projection.content_revision != self.route_revision
                || projection.scene != CorePrivateRouteSceneV1::LanternHalls
                || projection.phase != CorePrivateRoutePhaseV1::Hall
                || projection.instance_lineage_id.is_some()
            {
                return Err(CorePrivateLifeBootstrapError::RetainedActorMismatch);
            }
            return Ok(lease);
        }
        let allocation = self
            .repository
            .allocate_route_generation(account_id, hall.character.character_id)
            .await
            .map_err(CorePrivateLifeBootstrapError::Persistence)?;
        if allocation.account_id != account_id
            || allocation.character_id != hall.character.character_id
        {
            return Err(CorePrivateLifeBootstrapError::InvalidBinding);
        }
        let lease = self.route_directory.register_actor(
            authenticated,
            CorePrivateRouteActorSeed {
                character_id: hall.character.character_id,
                character_version: hall.character.versions.world,
                content_revision: self.route_revision.clone(),
                world_flow_revision: self.world_flow_revision.clone(),
                position: CorePrivateRouteActorPosition::hall(),
            },
            allocation.actor_generation,
        )?;
        lock(&self.retained_routes).insert(account_id, lease);
        Ok(lease)
    }

    fn retained_route(&self, account_id: [u8; 16]) -> Option<CorePrivateRouteActorLease> {
        lock(&self.retained_routes).get(&account_id).copied()
    }

    fn require_no_retained_route(
        &self,
        account_id: [u8; 16],
    ) -> Result<(), CorePrivateLifeBootstrapError> {
        if self.retained_route(account_id).is_some() {
            return Err(CorePrivateLifeBootstrapError::RetainedActorMismatch);
        }
        Ok(())
    }
}

fn zero_world_revision(revision: &WorldFlowContentRevisionV1) -> bool {
    [
        &revision.records_blake3,
        &revision.assets_blake3,
        &revision.localization_blake3,
    ]
    .into_iter()
    .any(|hash| hash.as_str().bytes().all(|byte| byte == b'0'))
}

fn lock<T>(mutex: &StdMutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use persistence::{
        PRIVATE_LIFE_BOOTSTRAP_SCHEMA_VERSION_V1, StoredPrivateLifeLifeStateV1,
        StoredPrivateLifeSecurityStateV1, StoredPrivateLifeVersionsV1,
        StoredResolutionHoldSnapshotV1, StoredResolutionHoldVersionsV1,
    };
    use protocol::{
        CharacterLocationSnapshot, ManifestHash, SafeArrival, WireText, WorldTransferPayload,
    };

    use super::*;
    use crate::AccountId;

    #[derive(Debug)]
    struct FakeRepository {
        bootstrap: StoredPrivateLifeBootstrapV1,
        allocations: AtomicUsize,
    }

    impl CorePrivateLifeBootstrapRepository for FakeRepository {
        async fn load(
            &self,
            _account_id: [u8; 16],
        ) -> Result<StoredPrivateLifeBootstrapV1, PersistenceError> {
            Ok(self.bootstrap.clone())
        }

        async fn resolve_process_restart(
            &self,
            _account_id: [u8; 16],
        ) -> Result<ResolvedPrivateLifeProcessRestartV1, PersistenceError> {
            Ok(ResolvedPrivateLifeProcessRestartV1 {
                bootstrap: self.bootstrap.clone(),
                crash_restore: None,
            })
        }

        async fn allocate_route_generation(
            &self,
            account_id: [u8; 16],
            character_id: [u8; 16],
        ) -> Result<StoredPrivateRouteGenerationV1, PersistenceError> {
            let generation = self.allocations.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(StoredPrivateRouteGenerationV1 {
                account_id,
                character_id,
                actor_generation: u64::try_from(generation).unwrap(),
            })
        }
    }

    fn authenticated() -> AuthenticatedAccount {
        AuthenticatedAccount {
            account_id: AccountId::new([1; 16]).unwrap(),
            namespace: AuthenticatedNamespace::WipeableTest,
        }
    }

    fn revisions() -> (
        CorePrivateRouteContentRevisionV1,
        WorldFlowContentRevisionV1,
    ) {
        let hash = || ManifestHash::new("a".repeat(64)).unwrap();
        (
            CorePrivateRouteContentRevisionV1 {
                records_blake3: hash(),
                assets_blake3: hash(),
                localization_blake3: hash(),
            },
            WorldFlowContentRevisionV1 {
                records_blake3: hash(),
                assets_blake3: hash(),
                localization_blake3: hash(),
            },
        )
    }

    fn hall() -> StoredPrivateLifeHallV1 {
        let versions = StoredPrivateLifeVersionsV1 {
            account: 7,
            character: 11,
            world: 11,
            inventory: 13,
            progression: 17,
            oath_bargain: 19,
            life_metrics: 23,
            ash_wallet: 29,
        };
        StoredPrivateLifeHallV1 {
            character: StoredPrivateLifeSelectedCharacterV1 {
                character_id: [2; 16],
                class_id: persistence::PRIVATE_LIFE_CLASS_ID_V1.to_owned(),
                level: 1,
                life_state: StoredPrivateLifeLifeStateV1::Living,
                security_state: StoredPrivateLifeSecurityStateV1::Normal,
                versions,
            },
            arrival: StoredSafeArrival::HallDefault,
            resolution_hold: StoredResolutionHoldSnapshotV1 {
                account_id: [1; 16],
                character_id: [2; 16],
                versions: StoredResolutionHoldVersionsV1 {
                    account: versions.account,
                    character: versions.character,
                    world: versions.world,
                    inventory: versions.inventory,
                },
                storage_resolution_required: false,
                stacks: Vec::new(),
            },
        }
    }

    fn bootstrap(state: StoredPrivateLifeBootstrapStateV1) -> StoredPrivateLifeBootstrapV1 {
        StoredPrivateLifeBootstrapV1 {
            schema_version: PRIVATE_LIFE_BOOTSTRAP_SCHEMA_VERSION_V1,
            account_id: [1; 16],
            account_version: 7,
            state,
        }
    }

    fn mutation(command: WorldTransferCommand) -> WorldTransferMutation {
        let (_, world_revision) = revisions();
        let payload = WorldTransferPayload {
            content_revision: world_revision,
            command,
        };
        WorldTransferMutation {
            mutation_id: [7; 16],
            character_id: [2; 16],
            expected_character_version: 11,
            issued_at_unix_millis: 1,
            payload_hash: payload.canonical_hash(),
            payload,
        }
    }

    fn accepted_result(location: CharacterLocation, transfer_id: [u8; 16]) -> WorldFlowResult {
        WorldFlowResult::Transfer {
            request_sequence: 1,
            mutation_id: [7; 16],
            accepted: true,
            code: WorldTransferResultCode::Accepted,
            snapshot: Some(CharacterLocationSnapshot {
                character_id: [2; 16],
                character_version: 12,
                location,
            }),
            transfer_id: Some(transfer_id),
        }
    }

    #[tokio::test]
    async fn hall_refresh_allocates_once_and_exact_replay_reuses_the_actor() {
        let stored = bootstrap(StoredPrivateLifeBootstrapStateV1::HallReady(hall()));
        let (route_revision, world_revision) = revisions();
        let directory = CorePrivateRouteActorDirectory::new();
        let adapter = CorePrivateLifeRuntimeBootstrapAdapter::new(
            FakeRepository {
                bootstrap: stored.clone(),
                allocations: AtomicUsize::new(0),
            },
            directory.clone(),
            route_revision,
            world_revision,
        )
        .unwrap();

        let first = adapter
            .map_bootstrap(authenticated(), stored.clone())
            .await
            .unwrap();
        let second = adapter
            .map_bootstrap(authenticated(), stored)
            .await
            .unwrap();
        let first_lease = match first {
            CorePrivateLifeBootstrapDisposition::HallReady { route, .. } => route,
            other => panic!("unexpected bootstrap disposition: {other:?}"),
        };
        let second_lease = match second {
            CorePrivateLifeBootstrapDisposition::HallReady { route, .. } => route,
            other => panic!("unexpected bootstrap disposition: {other:?}"),
        };
        assert_eq!(first_lease, second_lease);
        assert_eq!(adapter.repository.allocations.load(Ordering::SeqCst), 1);
        let projection = directory.snapshot(first_lease).unwrap();
        assert_eq!(projection.scene, CorePrivateRouteSceneV1::LanternHalls);
        assert_eq!(projection.phase, CorePrivateRoutePhaseV1::Hall);
        assert_eq!(projection.character_version, 11);

        directory.begin_shutdown();
        assert!(directory.finish_shutdown().await.unwrap().zero_residue);
    }

    #[tokio::test]
    async fn character_select_never_allocates_a_route_generation() {
        let stored = bootstrap(StoredPrivateLifeBootstrapStateV1::CharacterSelect {
            selected_character: None,
            next_hall_arrival: None,
        });
        let (route_revision, world_revision) = revisions();
        let adapter = CorePrivateLifeRuntimeBootstrapAdapter::new(
            FakeRepository {
                bootstrap: stored.clone(),
                allocations: AtomicUsize::new(0),
            },
            CorePrivateRouteActorDirectory::new(),
            route_revision,
            world_revision,
        )
        .unwrap();
        let disposition = adapter
            .map_bootstrap(authenticated(), stored)
            .await
            .unwrap();
        assert!(matches!(
            disposition,
            CorePrivateLifeBootstrapDisposition::CharacterSelect {
                selected_character: None,
                next_hall_arrival: None
            }
        ));
        assert_eq!(adapter.repository.allocations.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn unresolved_danger_cannot_be_reconstructed_by_refresh() {
        let mut stored = bootstrap(StoredPrivateLifeBootstrapStateV1::CharacterSelect {
            selected_character: None,
            next_hall_arrival: None,
        });
        let hall = hall();
        stored.state = StoredPrivateLifeBootstrapStateV1::DangerRequiresCrashRestore {
            character: hall.character,
            danger: persistence::StoredPrivateLifeDangerRootV1 {
                location_content_id: "world.core_microrealm_01".to_owned(),
                lineage_id: [3; 16],
                restore_point_id: [4; 16],
                source_location_id: persistence::PRIVATE_LIFE_HALL_ID_V1.to_owned(),
                restore_location_id: persistence::PRIVATE_LIFE_HALL_ID_V1.to_owned(),
                layout_id: persistence::PRIVATE_LIFE_LAYOUT_ID_V1.to_owned(),
                lineage_state: 1,
                entry_versions: StoredPrivateLifeVersionsV1 {
                    account: 7,
                    character: 11,
                    world: 11,
                    inventory: 13,
                    progression: 17,
                    oath_bargain: 19,
                    life_metrics: 23,
                    ash_wallet: 29,
                },
                content_revision: persistence::StoredWorldFlowRevisionV1 {
                    records_blake3: "b".repeat(64),
                    assets_blake3: "c".repeat(64),
                    localization_blake3: "d".repeat(64),
                },
                composite_digest: [5; 32],
            },
        };
        let (route_revision, world_revision) = revisions();
        let adapter = CorePrivateLifeRuntimeBootstrapAdapter::new(
            FakeRepository {
                bootstrap: stored.clone(),
                allocations: AtomicUsize::new(0),
            },
            CorePrivateRouteActorDirectory::new(),
            route_revision,
            world_revision,
        )
        .unwrap();
        assert!(matches!(
            adapter.map_bootstrap(authenticated(), stored).await,
            Err(CorePrivateLifeBootstrapError::RetainedActorMismatch)
        ));
        assert_eq!(adapter.repository.allocations.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn committed_microrealm_transition_is_exactly_replayable_and_changed_material_fails() {
        let stored = bootstrap(StoredPrivateLifeBootstrapStateV1::HallReady(hall()));
        let (route_revision, world_revision) = revisions();
        let directory = CorePrivateRouteActorDirectory::new();
        let adapter = CorePrivateLifeRuntimeBootstrapAdapter::new(
            FakeRepository {
                bootstrap: stored.clone(),
                allocations: AtomicUsize::new(0),
            },
            directory.clone(),
            route_revision,
            world_revision,
        )
        .unwrap();
        adapter
            .map_bootstrap(authenticated(), stored)
            .await
            .unwrap();
        let mutation = mutation(WorldTransferCommand::UsePortal {
            portal_id: WireText::new("station.realm_gate").unwrap(),
        });
        let result = accepted_result(
            CharacterLocation::Danger {
                location_id: WireText::new("world.core_microrealm_01").unwrap(),
                instance_lineage_id: [8; 16],
                entry_restore_point_id: [9; 16],
            },
            [10; 16],
        );
        adapter
            .reconcile_committed_world_transition(authenticated(), &mutation, &result)
            .await
            .unwrap();
        adapter
            .reconcile_committed_world_transition(authenticated(), &mutation, &result)
            .await
            .unwrap();

        let changed = accepted_result(
            CharacterLocation::Danger {
                location_id: WireText::new("world.core_microrealm_01").unwrap(),
                instance_lineage_id: [8; 16],
                entry_restore_point_id: [9; 16],
            },
            [11; 16],
        );
        assert!(matches!(
            adapter
                .reconcile_committed_world_transition(authenticated(), &mutation, &changed)
                .await,
            Err(CorePrivateLifeBootstrapError::Route(
                CorePrivateRouteRuntimeError::StaleRouteState
            ))
        ));
        directory.begin_shutdown();
        assert!(directory.finish_shutdown().await.unwrap().zero_residue);
    }

    #[tokio::test]
    async fn committed_character_select_return_retires_once_and_exact_replay_is_safe() {
        let stored = bootstrap(StoredPrivateLifeBootstrapStateV1::HallReady(hall()));
        let (route_revision, world_revision) = revisions();
        let directory = CorePrivateRouteActorDirectory::new();
        let adapter = CorePrivateLifeRuntimeBootstrapAdapter::new(
            FakeRepository {
                bootstrap: stored.clone(),
                allocations: AtomicUsize::new(0),
            },
            directory.clone(),
            route_revision,
            world_revision,
        )
        .unwrap();
        adapter
            .map_bootstrap(authenticated(), stored)
            .await
            .unwrap();
        let mutation = mutation(WorldTransferCommand::ReturnToCharacterSelect);
        let result = accepted_result(
            CharacterLocation::CharacterSelect {
                next_hall_arrival: SafeArrival::SpawnAnchor {
                    spawn_id: WireText::new("spawn.hub.character_select_return").unwrap(),
                },
            },
            [12; 16],
        );
        adapter
            .reconcile_committed_world_transition(authenticated(), &mutation, &result)
            .await
            .unwrap();
        adapter
            .reconcile_committed_world_transition(authenticated(), &mutation, &result)
            .await
            .unwrap();
        assert!(adapter.retained_route([1; 16]).is_none());

        directory.begin_shutdown();
        assert!(directory.finish_shutdown().await.unwrap().zero_residue);
    }
}
