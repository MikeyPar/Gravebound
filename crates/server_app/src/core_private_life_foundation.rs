//! Dormant persistent composition foundation for the ordinary Core private-life runtime.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`LOOP-001`-`003`, `DTH-010`-
//! `011`, and `TECH-010`-`023`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-WORLD-001`, `CONT-ROOM-007`, `CONT-BOSS-001`-`002`, and `CONT-HUB-001`-
//! `002`), and `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03` and the M03 exit
//! gate). ADR-037 requires all-or-nothing construction before the normal endpoint exists.
//!
//! This module deliberately constructs only reusable process-wide owners. It does not expose a
//! socket, advertise a capability, or inject the latent normal world-flow router into transport.
//! Per-account bootstrap, live movement/combat/reward ownership, dynamic extraction/Recall, and
//! one shared connection writer remain mandatory before `BoundCorePrivateLifeServer` is legal.

use std::{
    fmt,
    path::Path,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use persistence::{DurableDeathPresentationAuthorityV1, PostgresPersistence};
use protocol::{DeathViewContentRevisionV1, ManifestHash, WorldFlowContentRevisionV1};
use thiserror::Error;

use crate::{
    Blake3CharacterIds, Blake3WorldFlowIds, CoreBargainAuthority, CoreOathSelectionAuthority,
    CorePrivateLifeRuntimeBootstrapAdapter, CorePrivateRouteActorDirectory,
    CorePrivateRouteRuntimeError, CorePrivateRouteRuntimeReport, CorePrivateWorldFlowRouter,
    CoreResolutionHoldAuthority, CoreSafeInventoryAuthority, CoreSuccessorAuthority,
    DeathViewService, IdentityClock, IdentityService, NoopIdentityEventSink,
    PostgresAccountRepository, PostgresBargainService, PostgresCorePrivateTerminalOwnerFactory,
    PostgresDangerEntryAshWalletProviderV3, PostgresDangerEntryInventoryProviderV3,
    PostgresDangerEntryLifeMetricsProviderV3, PostgresDangerEntryOathBargainProviderV3,
    PostgresDeathViewRepository, PostgresDurableDeathExecutionService,
    PostgresOathSelectionService, PostgresPrivateDeathContextPlanner,
    PostgresProductionExtractionExecutionService, PostgresProductionRecallExecutionService,
    PostgresProgressionQueryRepository, PostgresProgressionRestoreProvider,
    PostgresSafeInventoryService, PostgresWorldFlowLocationRepository, ProgressionQueryService,
    ResolutionHoldService, SuccessorService, SystemDurableDeathIdentitySource,
    WorldFlowGateService, world_flow_coordinator::PostgresCorePrivateWorldFlowCoordinator,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct SystemIdentityClock;

impl IdentityClock for SystemIdentityClock {
    fn unix_millis(&self) -> u64 {
        u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        )
        .unwrap_or(u64::MAX)
    }
}

type PersistentIdentity = IdentityService<
    PostgresAccountRepository,
    SystemIdentityClock,
    Blake3CharacterIds,
    NoopIdentityEventSink,
>;

type PersistentWorldFlowCoordinator = PostgresCorePrivateWorldFlowCoordinator<
    Blake3WorldFlowIds,
    SystemIdentityClock,
    PostgresDangerEntryInventoryProviderV3,
    PostgresDangerEntryOathBargainProviderV3,
    PostgresDangerEntryLifeMetricsProviderV3,
    PostgresDangerEntryAshWalletProviderV3,
    CorePrivateRouteActorDirectory,
>;

type PersistentWorldFlow = CorePrivateWorldFlowRouter<
    WorldFlowGateService<PostgresWorldFlowLocationRepository, SystemIdentityClock>,
    PersistentWorldFlowCoordinator,
>;

type PersistentRuntimeBootstrap = CorePrivateLifeRuntimeBootstrapAdapter<PostgresPersistence>;
type PersistentDeathPlanner =
    PostgresPrivateDeathContextPlanner<PostgresPersistence, SystemDurableDeathIdentitySource>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DormantNormalAdmission {
    world_flow: bool,
    extraction: bool,
    recall: bool,
}

impl DormantNormalAdmission {
    const DISABLED: Self = Self {
        world_flow: false,
        extraction: false,
        recall: false,
    };

    const fn normal_route_enabled(self) -> bool {
        self.world_flow || self.extraction || self.recall
    }
}

/// Reusable process-wide authorities that are safe to construct before normal route admission.
pub(crate) struct CorePrivateLifePersistentFoundation {
    content: Arc<sim_content::CorePrivateLifeContent>,
    identity: Arc<PersistentIdentity>,
    world_flow: Arc<PersistentWorldFlow>,
    progression: Arc<ProgressionQueryService<PostgresProgressionQueryRepository>>,
    death_views: Arc<DeathViewService<PostgresDeathViewRepository>>,
    death_execution: Arc<PostgresDurableDeathExecutionService>,
    death_planner: Arc<PersistentDeathPlanner>,
    terminal_owner_factory: Arc<PostgresCorePrivateTerminalOwnerFactory>,
    oath: Arc<CoreOathSelectionAuthority<SystemIdentityClock>>,
    bargain: Arc<CoreBargainAuthority<SystemIdentityClock>>,
    safe_inventory: Arc<CoreSafeInventoryAuthority>,
    resolution_hold: Arc<CoreResolutionHoldAuthority>,
    successor: Arc<CoreSuccessorAuthority>,
    extraction_execution: Arc<PostgresProductionExtractionExecutionService>,
    recall_execution: Arc<PostgresProductionRecallExecutionService>,
    runtime_bootstrap: Arc<PersistentRuntimeBootstrap>,
    route_directory: CorePrivateRouteActorDirectory,
    admission: DormantNormalAdmission,
}

impl fmt::Debug for CorePrivateLifePersistentFoundation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CorePrivateLifePersistentFoundation")
            .field("content_revision", self.content.revision())
            .field("normal_route_enabled", &self.normal_route_enabled())
            .finish_non_exhaustive()
    }
}

impl CorePrivateLifePersistentFoundation {
    #[allow(
        clippy::too_many_lines,
        reason = "all process-wide owners are constructed contiguously before dormant admission can exist"
    )]
    pub(crate) fn new(
        content_root: &Path,
        persistence: PostgresPersistence,
    ) -> Result<Self, CorePrivateLifeFoundationError> {
        sim_content::load_core_development_identity(content_root)
            .map_err(content_error("identity"))?;
        let (_, source_report) = sim_content::load_and_validate(content_root)
            .map_err(content_error("source package"))?;
        let required_manifest_hash = ManifestHash::new(source_report.package_hash_blake3)?;
        let content = Arc::new(
            sim_content::load_core_private_life_content(content_root)
                .map_err(content_error("private-life route"))?,
        );
        let progression_content = sim_content::load_core_development_progression(content_root)
            .map_err(content_error("progression"))?;
        let oath_content = sim_content::load_core_development_oaths_bargains(content_root)
            .map_err(content_error("Oath/Bargain"))?;
        let death_view = Arc::new(
            sim_content::load_core_development_death_view(content_root)
                .map_err(content_error("death presentation"))?,
        );
        let death_view_revision = death_view_revision(&death_view)?;
        let items = Arc::new(
            sim_content::load_core_development_items(content_root)
                .map_err(content_error("death item authority"))?,
        );
        let death_planner = Arc::new(PostgresPrivateDeathContextPlanner::new(
            persistence.clone(),
            SystemDurableDeathIdentitySource,
            Arc::clone(&death_view),
            items,
            Arc::clone(&content),
        )?);
        let death_execution = Arc::new(PostgresDurableDeathExecutionService::new(
            persistence.clone(),
        ));
        let world_flow_revision = world_flow_revision(content.world_flow())?;
        let route_revision = route_revision(content.revision())?;

        let route_directory = CorePrivateRouteActorDirectory::new();
        let runtime_bootstrap = Arc::new(CorePrivateLifeRuntimeBootstrapAdapter::new(
            persistence.clone(),
            route_directory.clone(),
            route_revision,
            world_flow_revision.clone(),
        )?);
        let terminal_owner_factory = Arc::new(PostgresCorePrivateTerminalOwnerFactory::new(
            persistence.clone(),
            Arc::clone(&death_planner),
            Arc::clone(&death_execution),
            death_view,
            Arc::clone(&runtime_bootstrap),
        ));
        let world_flow_coordinator =
            PostgresCorePrivateWorldFlowCoordinator::with_runtime_authorities(
                persistence.clone(),
                Blake3WorldFlowIds,
                SystemIdentityClock,
                world_flow_revision.clone(),
                PostgresProgressionRestoreProvider::new(&progression_content)
                    .map_err(content_error("progression restore"))?,
                PostgresDangerEntryInventoryProviderV3,
                PostgresDangerEntryOathBargainProviderV3,
                PostgresDangerEntryLifeMetricsProviderV3,
                PostgresDangerEntryAshWalletProviderV3,
                route_directory.clone(),
                Arc::clone(&runtime_bootstrap),
            );
        let world_flow = CorePrivateWorldFlowRouter::new(
            WorldFlowGateService::new(
                PostgresWorldFlowLocationRepository::new(persistence.clone()),
                SystemIdentityClock,
                world_flow_revision,
            ),
            world_flow_coordinator,
        );
        let progression_repository =
            PostgresProgressionQueryRepository::new(persistence.clone(), &progression_content)
                .map_err(content_error("progression query"))?;
        let progression =
            ProgressionQueryService::new(progression_repository, &progression_content)
                .map_err(content_error("progression service"))?;
        let oath = PostgresOathSelectionService::new(
            persistence.clone(),
            SystemIdentityClock,
            &oath_content,
        )?;
        let bargain =
            PostgresBargainService::new(persistence.clone(), SystemIdentityClock, &oath_content)?;

        let foundation = Self {
            content,
            identity: Arc::new(IdentityService::new(
                PostgresAccountRepository::new(persistence.clone()),
                SystemIdentityClock,
                Blake3CharacterIds,
                NoopIdentityEventSink,
                required_manifest_hash,
            )),
            world_flow: Arc::new(world_flow),
            progression: Arc::new(progression),
            death_views: Arc::new(DeathViewService::new(
                PostgresDeathViewRepository::new(persistence.clone()),
                death_view_revision,
            )),
            death_execution,
            death_planner,
            terminal_owner_factory,
            oath: Arc::new(CoreOathSelectionAuthority::persistent(oath)),
            bargain: Arc::new(CoreBargainAuthority::persistent(bargain)),
            safe_inventory: Arc::new(CoreSafeInventoryAuthority::persistent(
                PostgresSafeInventoryService::new(persistence.clone()),
            )),
            resolution_hold: Arc::new(CoreResolutionHoldAuthority::persistent(
                ResolutionHoldService::new(persistence.clone()),
            )),
            successor: Arc::new(CoreSuccessorAuthority::persistent(SuccessorService::new(
                persistence.clone(),
            ))),
            extraction_execution: Arc::new(PostgresProductionExtractionExecutionService::new(
                persistence.clone(),
            )),
            recall_execution: Arc::new(PostgresProductionRecallExecutionService::new(persistence)),
            runtime_bootstrap,
            route_directory,
            admission: DormantNormalAdmission::DISABLED,
        };
        foundation.validate_dormant()?;
        Ok(foundation)
    }

    pub(crate) const fn normal_route_enabled(&self) -> bool {
        self.admission.normal_route_enabled()
    }

    pub(crate) fn begin_shutdown(&self) {
        self.runtime_bootstrap.begin_shutdown();
        self.route_directory.begin_shutdown();
    }

    pub(crate) async fn finish_shutdown(
        &self,
    ) -> Result<CorePrivateRouteRuntimeReport, CorePrivateLifeFoundationError> {
        self.route_directory
            .finish_shutdown()
            .await
            .map_err(CorePrivateLifeFoundationError::RouteRuntime)
    }

    fn validate_dormant(&self) -> Result<(), CorePrivateLifeFoundationError> {
        let revision = self.content.revision();
        if self.normal_route_enabled()
            || !valid_hash(&revision.records_blake3)
            || !valid_hash(&revision.assets_blake3)
            || !valid_hash(&revision.localization_blake3)
        {
            return Err(CorePrivateLifeFoundationError::InvalidComposition);
        }
        // Construction must finish before any process-wide owner can escape into a session.
        // These counts make that all-or-nothing boundary explicit and keep partial composition
        // from becoming externally observable.
        let single_owner_counts = [
            Arc::strong_count(&self.identity),
            Arc::strong_count(&self.world_flow),
            Arc::strong_count(&self.progression),
            Arc::strong_count(&self.death_views),
            Arc::strong_count(&self.terminal_owner_factory),
            Arc::strong_count(&self.oath),
            Arc::strong_count(&self.bargain),
            Arc::strong_count(&self.safe_inventory),
            Arc::strong_count(&self.resolution_hold),
            Arc::strong_count(&self.successor),
            Arc::strong_count(&self.extraction_execution),
            Arc::strong_count(&self.recall_execution),
        ];
        if single_owner_counts.into_iter().any(|count| count != 1)
            || Arc::strong_count(&self.death_execution) != 2
            || Arc::strong_count(&self.death_planner) != 2
            || Arc::strong_count(&self.runtime_bootstrap) != 2
        {
            return Err(CorePrivateLifeFoundationError::InvalidComposition);
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub(crate) enum CorePrivateLifeFoundationError {
    #[error("private-life foundation content validation failed: {0}")]
    Content(String),
    #[error("private-life foundation constructed an invalid or enabled composition")]
    InvalidComposition,
    #[error("private-life foundation route runtime failed: {0}")]
    RouteRuntime(#[source] CorePrivateRouteRuntimeError),
    #[error("private-life runtime bootstrap failed: {0}")]
    RuntimeBootstrap(#[from] crate::CorePrivateLifeBootstrapError),
    #[error("private-life death planner failed: {0}")]
    DeathPlanner(#[from] crate::PrivateDeathPlanningError),
    #[error(transparent)]
    Bounded(#[from] protocol::BoundedValueError),
}

pub(crate) fn load_death_view_revision(
    content_root: &Path,
) -> Result<DeathViewContentRevisionV1, CorePrivateLifeFoundationError> {
    let content = sim_content::load_core_development_death_view(content_root)
        .map_err(content_error("death presentation"))?;
    death_view_revision(&content)
}

fn death_view_revision(
    content: &sim_content::CoreDevelopmentDeathView,
) -> Result<DeathViewContentRevisionV1, CorePrivateLifeFoundationError> {
    let hashes = content.hashes();
    let persistence_authority = DurableDeathPresentationAuthorityV1::core();
    if hashes.records_blake3 != persistence_authority.records_blake3
        || hashes.assets_blake3 != persistence_authority.assets_blake3
        || hashes.localization_blake3 != persistence_authority.localization_blake3
        || content.item_content_revision() != persistence::CORE_ITEM_CONTENT_REVISION
    {
        return Err(CorePrivateLifeFoundationError::Content(
            "compiled Core death presentation does not match durable death authority".to_owned(),
        ));
    }
    Ok(DeathViewContentRevisionV1 {
        records_blake3: ManifestHash::new(hashes.records_blake3.clone())?,
        assets_blake3: ManifestHash::new(hashes.assets_blake3.clone())?,
        localization_blake3: ManifestHash::new(hashes.localization_blake3.clone())?,
    })
}

fn world_flow_revision(
    content: &sim_content::CoreDevelopmentWorldFlow,
) -> Result<WorldFlowContentRevisionV1, CorePrivateLifeFoundationError> {
    let hashes = content.hashes();
    Ok(WorldFlowContentRevisionV1 {
        records_blake3: ManifestHash::new(hashes.records_blake3.clone())?,
        assets_blake3: ManifestHash::new(hashes.assets_blake3.clone())?,
        localization_blake3: ManifestHash::new(hashes.localization_blake3.clone())?,
    })
}

fn route_revision(
    content: &sim_content::CorePrivateLifeContentRevision,
) -> Result<protocol::CorePrivateRouteContentRevisionV1, CorePrivateLifeFoundationError> {
    Ok(protocol::CorePrivateRouteContentRevisionV1 {
        records_blake3: ManifestHash::new(content.records_blake3.clone())?,
        assets_blake3: ManifestHash::new(content.assets_blake3.clone())?,
        localization_blake3: ManifestHash::new(content.localization_blake3.clone())?,
    })
}

fn content_error<Error>(
    component: &'static str,
) -> impl FnOnce(Error) -> CorePrivateLifeFoundationError
where
    Error: fmt::Display,
{
    move |error| CorePrivateLifeFoundationError::Content(format!("{component}: {error}"))
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn checked_in_content_builds_a_dormant_normal_admission_contract() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let content = sim_content::load_core_private_life_content(&root).unwrap();
        assert!(valid_hash(&content.revision().records_blake3));
        assert!(valid_hash(&content.revision().assets_blake3));
        assert!(valid_hash(&content.revision().localization_blake3));
        assert!(!DormantNormalAdmission::DISABLED.normal_route_enabled());
    }

    #[test]
    fn malformed_content_fails_before_any_runtime_owner_or_socket_can_exist() {
        let missing = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/missing-private-life-content");
        assert!(sim_content::load_core_private_life_content(&missing).is_err());
    }
}
