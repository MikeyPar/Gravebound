//! Dormant process owner graph for the ordinary Core private-life route.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`LOOP-001`-`003`,
//! `TECH-015`, and `TECH-021`-`023`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-WORLD-001`, `CONT-HUB-001`/`002`, and `CONT-BOSS-001`/`002`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`, `GB-M03-08`, and the M03
//! exit gate). ADR-037 requires this all-or-nothing owner graph before a normal socket may exist.
//!
//! Construction intentionally does not imply admission. The authoritative Hall movement and
//! interaction-range owner is not part of this graph yet, so capability publication remains
//! fail-closed even though every danger-route process owner is available.

use std::{path::Path, sync::Arc};

use persistence::PostgresPersistence;
use thiserror::Error;

use crate::core_private_life_foundation::{
    CorePrivateLifeFoundationError, CorePrivateLifePersistentFoundation, SystemIdentityClock,
};
use crate::{
    CaldusVictoryCompositionError, CoreB3RewardCompositionError, CoreCharacterCombatFactory,
    CoreCombatFactoryError, CoreExtractionActorDirectory, CorePrivateLifeSessionDirectory,
    CorePrivateLifeSessionReport, CorePrivateLifeTickDirectory, CorePrivateLifeTickDirectoryReport,
    CorePrivateRouteRuntimeReport, CoreRecallActorDirectory, SecretRewardEpoch,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CorePrivateLifeAdmission {
    authoritative_hall: bool,
    transport_dispatch: bool,
    native_client: bool,
}

impl CorePrivateLifeAdmission {
    const DORMANT: Self = Self {
        authoritative_hall: false,
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
    ticks: Arc<CorePrivateLifeTickDirectory>,
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
            recall,
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
            ticks,
            combat: Arc::new(CoreCharacterCombatFactory::load(persistence, content_root)?),
            admission: CorePrivateLifeAdmission::DORMANT,
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
        assert!(!CorePrivateLifeAdmission::DORMANT.ready());
    }
}
