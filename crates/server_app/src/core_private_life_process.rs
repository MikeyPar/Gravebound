//! Dormant process owner graph for the ordinary Core private-life route.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`LOOP-001`-`003`,
//! `TECH-015`, and `TECH-021`-`023`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-WORLD-001`, `CONT-HUB-001`/`002`, and `CONT-BOSS-001`/`002`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`, `GB-M03-08`, and the M03
//! exit gate). ADR-037 requires this all-or-nothing owner graph before a normal socket may exist.
//!
//! Construction intentionally does not imply admission. The authoritative Hall owner is present,
//! but normal transport dispatch and the native route client are not yet composed, so capability
//! publication remains fail-closed.

use std::{path::Path, sync::Arc};

use persistence::PostgresPersistence;
use thiserror::Error;

use crate::core_private_hall_runtime::{CorePrivateHallDirectory, CorePrivateHallError};
use crate::core_private_life_foundation::{
    CorePrivateLifeFoundationError, CorePrivateLifePersistentFoundation, PersistentWorldFlow,
    SystemIdentityClock, route_revision,
};
use crate::core_private_world_flow::CorePrivateHallWorldFlow;
use crate::{
    CaldusVictoryCompositionError, CoreB3RewardCompositionError, CoreCharacterCombatFactory,
    CoreCombatFactoryError, CoreExtractionActorDirectory, CorePrivateHallActorLease,
    CorePrivateLifeSessionDirectory, CorePrivateLifeSessionReport, CorePrivateLifeTickDirectory,
    CorePrivateLifeTickDirectoryReport, CorePrivateLifeTransportLease,
    CorePrivateMicrorealmBinding, CorePrivateMicrorealmRuntime, CorePrivateRouteRuntimeReport,
    CoreRecallActorDirectory, CoreReliableWriter, ProductionRecallIntentActor,
    ProductionRecallPendingAuthorityV1, SecretRewardEpoch,
};

type PersistentRecallDirectory =
    CoreRecallActorDirectory<SystemIdentityClock, CorePrivateLifeTickDirectory>;
type PersistentExtractionDirectory = CoreExtractionActorDirectory<
    PostgresPersistence,
    SystemIdentityClock,
    CorePrivateLifeTickDirectory,
>;
type PersistentSessionDirectory =
    CorePrivateLifeSessionDirectory<SystemIdentityClock, CorePrivateLifeTickDirectory>;
type PersistentHallWorldFlow = CorePrivateHallWorldFlow<PersistentWorldFlow>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CorePrivateLifeAdmission {
    authoritative_hall: bool,
    transport_dispatch: bool,
    native_client: bool,
}

impl CorePrivateLifeAdmission {
    const HALL_COMPOSED: Self = Self {
        authoritative_hall: true,
        transport_dispatch: false,
        native_client: false,
    };

    const fn ready(self) -> bool {
        self.authoritative_hall && self.transport_dispatch && self.native_client
    }
}

/// One process-owned graph for shared ticks, Recall, extraction, rewards, terminal resolution,
/// combat construction, and generation-safe transport sessions.
pub(crate) struct CorePrivateLifeProcess {
    foundation: Arc<CorePrivateLifePersistentFoundation>,
    sessions: Arc<PersistentSessionDirectory>,
    recall: Arc<PersistentRecallDirectory>,
    ticks: Arc<CorePrivateLifeTickDirectory>,
    hall: Arc<CorePrivateHallDirectory>,
    combat: Arc<CoreCharacterCombatFactory>,
    admission: CorePrivateLifeAdmission,
}

impl std::fmt::Debug for CorePrivateLifeProcess {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CorePrivateLifeProcess")
            .field("admission_ready", &self.admission_ready())
            .finish_non_exhaustive()
    }
}

impl CorePrivateLifeProcess {
    /// Builds the complete danger-route process graph from one persistence pool and one redacted
    /// reward epoch. Callers must retain this value as a unit; no partial owner escapes on error.
    pub(crate) fn compose_dormant(
        foundation: Arc<CorePrivateLifePersistentFoundation>,
        persistence: PostgresPersistence,
        content_root: &Path,
        reward_epoch: SecretRewardEpoch,
    ) -> Result<Self, CorePrivateLifeProcessError> {
        let ticks = Arc::new(CorePrivateLifeTickDirectory::new());
        let recall = Arc::new(PersistentRecallDirectory::new(Arc::clone(&ticks)));
        let extraction = Arc::new(PersistentExtractionDirectory::new(Arc::clone(&ticks)));
        let sessions = CorePrivateLifeSessionDirectory::with_caldus_extraction_runtime(
            Arc::clone(&recall),
            extraction,
            persistence.clone(),
            SystemIdentityClock,
        )
        .with_authoritative_tick_directory(Arc::clone(&ticks))
        .with_terminal_owner_factory(foundation.terminal_owner_factory())
        .with_persistent_b3_reward_authority(
            persistence.clone(),
            content_root,
            reward_epoch.clone(),
        )?
        .with_persistent_caldus_reward_authority(
            persistence.clone(),
            content_root,
            reward_epoch,
        )?;
        let process = Self {
            foundation,
            sessions: Arc::new(sessions),
            recall,
            ticks,
            hall: Arc::new(CorePrivateHallDirectory::load(content_root)?),
            combat: Arc::new(CoreCharacterCombatFactory::load(persistence, content_root)?),
            admission: CorePrivateLifeAdmission::HALL_COMPOSED,
        };
        process.validate_dormant_composition()?;
        Ok(process)
    }

    #[must_use]
    pub(crate) const fn admission_ready(&self) -> bool {
        self.admission.ready()
    }

    #[must_use]
    pub(crate) fn sessions(&self) -> &Arc<PersistentSessionDirectory> {
        &self.sessions
    }

    #[must_use]
    pub(crate) fn combat(&self) -> &Arc<CoreCharacterCombatFactory> {
        &self.combat
    }

    #[must_use]
    pub(crate) fn hall(&self) -> &Arc<CorePrivateHallDirectory> {
        &self.hall
    }

    #[must_use]
    pub(crate) fn hall_world_flow(
        &self,
        actor: CorePrivateHallActorLease,
        transport: CorePrivateLifeTransportLease,
    ) -> PersistentHallWorldFlow {
        CorePrivateHallWorldFlow::new(
            self.foundation.world_flow(),
            Arc::clone(&self.hall),
            actor,
            transport,
        )
    }

    /// Attaches the winning authenticated transport and resolves terminal/restart state before
    /// any Hall or danger control can be published by the connection root.
    pub(crate) async fn attach_transport(
        self: &Arc<Self>,
        authenticated: crate::AuthenticatedAccount,
        connection: quinn::Connection,
        issued_at_unix_ms: u64,
    ) -> Result<CorePrivateLifeProcessAttach, CorePrivateLifeProcessError> {
        let attached = self
            .sessions
            .attach_transport(authenticated, connection, issued_at_unix_ms)
            .await?;
        if let Some(previous) = attached.invalidated_connection.as_ref() {
            crate::close_transport(
                previous,
                crate::TRANSPORT_REPLACED_CLOSE_CODE,
                b"authoritative private-life transport replaced",
            );
        }
        if let Some(microrealm) = attached.microrealm {
            return Ok(CorePrivateLifeProcessAttach {
                transport: attached.lease,
                writer: attached.writer,
                disposition: CorePrivateLifeProcessDisposition::Danger(microrealm),
            });
        }
        let bootstrap = match self
            .foundation
            .runtime_bootstrap()
            .bootstrap_process_restart(authenticated, attached.lease, self.sessions.as_ref())
            .await
        {
            Ok(bootstrap) => bootstrap,
            Err(error) => {
                let _ = self
                    .sessions
                    .detach_transport(attached.lease, issued_at_unix_ms)
                    .await;
                return Err(error.into());
            }
        };
        if !Arc::ptr_eq(&bootstrap.writer, &attached.writer) {
            let _ = self
                .sessions
                .detach_transport(attached.lease, issued_at_unix_ms)
                .await;
            return Err(CorePrivateLifeProcessError::SplitReliableWriter);
        }
        let disposition = match bootstrap.disposition {
            crate::CorePrivateLifeBootstrapDisposition::HallReady { hall, route } => {
                let actor = match self.hall.install_stored(authenticated, &hall) {
                    Ok(actor) => actor,
                    Err(error) => {
                        let _ = self
                            .sessions
                            .detach_transport(attached.lease, issued_at_unix_ms)
                            .await;
                        return Err(error.into());
                    }
                };
                if let Err(error) = self
                    .hall
                    .attach_transport(authenticated, actor, attached.lease)
                {
                    let _ = self.hall.retire(actor);
                    let _ = self
                        .sessions
                        .detach_transport(attached.lease, issued_at_unix_ms)
                        .await;
                    return Err(error.into());
                }
                CorePrivateLifeProcessDisposition::Hall { hall, route, actor }
            }
            disposition => CorePrivateLifeProcessDisposition::Bootstrap(disposition),
        };
        Ok(CorePrivateLifeProcessAttach {
            transport: attached.lease,
            writer: attached.writer,
            disposition,
        })
    }

    /// Converts one already committed Hall -> microrealm receipt into the exact live danger
    /// graph. Exact replay returns the retained binding; every partial fresh bind is retired before
    /// the error escapes.
    pub(crate) async fn enter_committed_microrealm(
        self: &Arc<Self>,
        authenticated: crate::AuthenticatedAccount,
        transport: CorePrivateLifeTransportLease,
        character_id: [u8; 16],
    ) -> Result<CorePrivateMicrorealmBinding, CorePrivateLifeProcessError> {
        if let Ok(binding) = self.sessions.microrealm_authority(transport).await {
            if binding.lease.character_id() == character_id {
                return Ok(binding);
            }
            return Err(CorePrivateLifeProcessError::InvalidRouteBinding);
        }
        let reattached = self
            .foundation
            .runtime_bootstrap()
            .reattach_within_process(authenticated, transport, self.sessions.as_ref())
            .await?;
        let route_lease = reattached
            .route
            .ok_or(CorePrivateLifeProcessError::InvalidRouteBinding)?;
        if route_lease.character_id() != character_id {
            return Err(CorePrivateLifeProcessError::InvalidRouteBinding);
        }
        let combat = self
            .combat
            .build(authenticated.account_id.as_bytes(), character_id)
            .await?;
        let content = self.foundation.content();
        let runtime = CorePrivateMicrorealmRuntime::new(
            self.foundation.route_directory(),
            route_lease,
            &route_revision(content.revision())?,
            content.microrealm_scene(),
            content.encounter_rooms().clone(),
            content.world_flow().clone(),
            combat,
        )?;
        let recall_actor = Arc::new(ProductionRecallIntentActor::new(
            SystemIdentityClock,
            authenticated.account_id.as_bytes(),
            character_id,
            ProductionRecallPendingAuthorityV1 {
                pending_item_count: 0,
                pending_material_stack_count: 0,
            },
        )?);
        self.recall
            .register_actor(authenticated, route_lease, recall_actor)
            .await?;
        if let Err(error) = self.sessions.bind_recall(transport).await {
            let _ = self.recall.retire_actor(authenticated).await;
            return Err(error.into());
        }
        match self.sessions.bind_microrealm(transport, runtime).await {
            Ok(binding) => Ok(binding),
            Err(error) => {
                let _ = self.sessions.unbind_recall(transport).await;
                Err(error.into())
            }
        }
    }

    pub(crate) async fn detach_transport(
        &self,
        transport: CorePrivateLifeTransportLease,
        issued_at_unix_ms: u64,
    ) -> Result<crate::CorePrivateLifeTransportDetach, CorePrivateLifeProcessError> {
        Ok(self
            .sessions
            .detach_transport(transport, issued_at_unix_ms)
            .await?)
    }

    fn validate_dormant_composition(&self) -> Result<(), CorePrivateLifeProcessError> {
        if self.admission_ready() || self.foundation.normal_route_enabled() {
            return Err(CorePrivateLifeProcessError::AdmissionEscaped);
        }
        Ok(())
    }

    /// Retires session writers before tick and route owners so no task can publish through a
    /// partially dismantled graph.
    pub(crate) async fn begin_shutdown(&self) {
        for connection in self.sessions.begin_shutdown().await {
            connection.close(
                crate::SERVER_SHUTDOWN_CLOSE_CODE.into(),
                b"private-life process shutdown",
            );
        }
        self.ticks.begin_shutdown();
        self.foundation.begin_shutdown();
    }

    pub(crate) async fn finish_shutdown(
        &self,
    ) -> Result<CorePrivateLifeProcessReport, CorePrivateLifeProcessError> {
        let sessions = self.sessions.finish_shutdown().await?;
        let ticks = self.ticks.finish_shutdown()?;
        let routes = self.foundation.finish_shutdown().await?;
        Ok(CorePrivateLifeProcessReport {
            zero_residue: sessions.zero_residue && ticks.zero_residue && routes.zero_residue,
            sessions,
            ticks,
            routes,
        })
    }
}

#[derive(Debug)]
pub(crate) struct CorePrivateLifeProcessReport {
    pub sessions: CorePrivateLifeSessionReport,
    pub ticks: CorePrivateLifeTickDirectoryReport,
    pub routes: CorePrivateRouteRuntimeReport,
    pub zero_residue: bool,
}

#[derive(Debug)]
pub(crate) struct CorePrivateLifeProcessAttach {
    pub transport: CorePrivateLifeTransportLease,
    pub writer: Arc<CoreReliableWriter>,
    pub disposition: CorePrivateLifeProcessDisposition,
}

#[derive(Debug)]
pub(crate) enum CorePrivateLifeProcessDisposition {
    Bootstrap(crate::CorePrivateLifeBootstrapDisposition),
    Hall {
        hall: persistence::StoredPrivateLifeHallV1,
        route: crate::CorePrivateRouteActorLease,
        actor: CorePrivateHallActorLease,
    },
    Danger(CorePrivateMicrorealmBinding),
}

#[derive(Debug, Error)]
pub(crate) enum CorePrivateLifeProcessError {
    #[error("private-life process admission escaped before the normal composition was complete")]
    AdmissionEscaped,
    #[error("private-life session composition failed: {0}")]
    B3(#[from] CoreB3RewardCompositionError),
    #[error("private-life Caldus composition failed: {0}")]
    Caldus(#[from] CaldusVictoryCompositionError),
    #[error("private-life combat composition failed: {0}")]
    Combat(#[from] CoreCombatFactoryError),
    #[error("private-life Hall composition failed: {0}")]
    Hall(#[from] CorePrivateHallError),
    #[error("private-life route binding is invalid")]
    InvalidRouteBinding,
    #[error("private-life microrealm composition failed: {0}")]
    Microrealm(#[from] crate::CorePrivateMicrorealmRuntimeError),
    #[error("private-life Recall composition failed: {0}")]
    Recall(#[from] crate::CoreRecallRuntimeError),
    #[error("private-life Recall actor is invalid: {0}")]
    RecallActor(#[from] crate::ProductionRecallChannelError),
    #[error("private-life runtime bootstrap failed: {0}")]
    Bootstrap(#[from] crate::CorePrivateLifeBootstrapError),
    #[error("private-life connection resolved more than one reliable writer")]
    SplitReliableWriter,
    #[error("private-life session runtime failed: {0}")]
    Session(#[from] crate::CorePrivateLifeSessionError),
    #[error("private-life tick runtime failed: {0}")]
    Tick(#[from] crate::CorePrivateLifeTickError),
    #[error("private-life foundation failed: {0}")]
    Foundation(#[from] CorePrivateLifeFoundationError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn danger_owner_graph_does_not_imply_normal_admission() {
        assert!(!CorePrivateLifeAdmission::HALL_COMPOSED.ready());
    }
}
