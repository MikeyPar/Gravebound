//! Production owner for the lossless ordinary Core private-route terminal feed.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`DTH-001`, `DTH-010`,
//! `DTH-020`, and `TECH-021..023`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-WORLD-001`, `CONT-BOSS-001`, and `CONT-ECHO-001`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`, `GB-M03-06`, `GB-M03-08`,
//! and `GB-M03-13`).
//!
//! The simulation driver cannot advance past a delivered frame until this owner acknowledges it.
//! Nonlethal frames are acknowledged only after their clock and damage evidence is durable. A
//! lethal frame is acknowledged only after the five-producer barrier selects death and the single
//! durable death transaction (including memorial and Echo projection) returns a stored receipt.

use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use persistence::{
    DangerCrashRestoreCode, DangerCrashRestoreTransaction, LifeClockCheckpointCommandV1,
    LifeClockCheckpointRequestV1, LifeClockContentAuthorityV1, LifeClockStateV1,
    LifeDeedCompletionCommandV2, LifeDeedCompletionRequestV2, LifeDeedContentAuthorityV2,
    PersistenceError, PostgresPersistence, StoredActiveDangerAuthorityV1, StoredLifeClockHeadV1,
    StoredWorldFlowRevisionV1,
};
use sim_core::{DamageTraceObservation, DeathTraceNetworkState};
use thiserror::Error;
use tokio::task::JoinHandle;

use crate::{
    AuthenticatedAccount, CoreExtractionActorDirectory, CoreExtractionActorLease,
    CoreExtractionAuthoritativeTick, CoreExtractionRuntimeError, CorePrivateDangerEntryAuthority,
    CorePrivateLifeBootstrapError, CorePrivateLifeRuntimeBootstrapAdapter,
    CorePrivatePlayerDamageFactV1, CorePrivateRouteActorLease,
    CorePrivateTerminalAcknowledgementError, CorePrivateTerminalDeliveryV1,
    CorePrivateTerminalFrameDelivery, CorePrivateTerminalFrameReceiver, CorePrivateTerminalFrameV1,
    CorePrivateTerminalOwner, CorePrivateTerminalOwnerError, CorePrivateTerminalOwnerFactory,
    CorePrivateTerminalOwnerStartFuture, CorePrivateTerminalRouteControlAuthorityV1,
    CorePrivateTerminalRouteControlV1, CorePrivateTerminalVerifiedFaultV1,
    CoreRecallTerminalDriverError, CoreRecallTerminalTickOutcome, CoreTerminalCoordinator,
    CoreTerminalCoordinatorError, CoreTerminalEvaluation, CoreTerminalOtherEvaluationsV1,
    CoreTerminalProducer, DeathEntityIdentityAuthority, DurableDeathExecutionError, IdentityClock,
    LiveDamageTraceIngestOutcome, LiveDamageTraceMutationAuthority, LiveDamageTraceService,
    LiveDamageTraceServiceError, PostgresDurableDeathExecutionService,
    PostgresPrivateDeathContextPlanner, PostgresProductionExtractionExecutionService,
    PostgresProductionRecallExecutionService, PreparedDurableDeathCommit,
    PrivateDeathPlanningAuthority, PrivateDeathPlanningError, ProductionExtractionExecutionError,
    ProductionExtractionPlanner, ProductionExtractionPreparedIntentV1,
    ProductionExtractionPublicationProof, ProductionRecallClock,
    ProductionRecallCompletionAuthorityV1, ProductionRecallIntentActor,
    ProductionRecallPendingAuthorityV1, StoredTerminalReceipt, SystemDurableDeathIdentitySource,
    TerminalKind, drive_recall_terminal_tick, durable_death_terminal_candidate,
    private_route_damage_entity_identities, production_extraction_terminal_candidate,
};

use crate::verified_fault_restoration::{
    PreparedVerifiedFaultRestoration, VerifiedFaultRestorationError,
    prepare_verified_fault_restoration, validate_fault_winner,
};

type PersistentDeathPlanner =
    PostgresPrivateDeathContextPlanner<PostgresPersistence, SystemDurableDeathIdentitySource>;

const RETRY_DELAY: Duration = Duration::from_millis(100);

#[derive(Clone)]
pub struct PostgresCorePrivateTerminalOwnerFactory {
    persistence: PostgresPersistence,
    planner: Arc<PersistentDeathPlanner>,
    death_execution: Arc<PostgresDurableDeathExecutionService>,
    extraction_execution: Arc<PostgresProductionExtractionExecutionService>,
    recall_execution: Arc<PostgresProductionRecallExecutionService>,
    route_reconciler: Arc<CorePrivateLifeRuntimeBootstrapAdapter<PostgresPersistence>>,
    death_view: Arc<sim_content::CoreDevelopmentDeathView>,
}

impl fmt::Debug for PostgresCorePrivateTerminalOwnerFactory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PostgresCorePrivateTerminalOwnerFactory")
            .finish_non_exhaustive()
    }
}

impl PostgresCorePrivateTerminalOwnerFactory {
    #[must_use]
    pub fn new(
        persistence: PostgresPersistence,
        planner: Arc<PersistentDeathPlanner>,
        death_execution: Arc<PostgresDurableDeathExecutionService>,
        death_view: Arc<sim_content::CoreDevelopmentDeathView>,
        route_reconciler: Arc<CorePrivateLifeRuntimeBootstrapAdapter<PostgresPersistence>>,
    ) -> Self {
        Self {
            extraction_execution: Arc::new(PostgresProductionExtractionExecutionService::new(
                persistence.clone(),
            )),
            recall_execution: Arc::new(PostgresProductionRecallExecutionService::new(
                persistence.clone(),
            )),
            route_reconciler,
            persistence,
            planner,
            death_execution,
            death_view,
        }
    }
}

impl CorePrivateTerminalOwnerFactory for PostgresCorePrivateTerminalOwnerFactory {
    fn start(
        &self,
        authenticated: AuthenticatedAccount,
        authority: CorePrivateDangerEntryAuthority,
        recall: CorePrivateRecallTerminalHandle,
        extraction: Option<CorePrivateExtractionTerminalHandle>,
        receiver: CorePrivateTerminalFrameReceiver,
    ) -> CorePrivateTerminalOwnerStartFuture<'_> {
        if authenticated.account_id.as_bytes() != *authority.terminal().account_id()
            || receiver.binding() != authority.terminal()
        {
            return Box::pin(async { Err(CorePrivateTerminalOwnerError::StartFailed) });
        }
        let runtime = ProductionTerminalOwnerRuntime {
            persistence: self.persistence.clone(),
            planner: Arc::clone(&self.planner),
            death_execution: Arc::clone(&self.death_execution),
            extraction_execution: Arc::clone(&self.extraction_execution),
            recall_execution: Arc::clone(&self.recall_execution),
            route_reconciler: Arc::clone(&self.route_reconciler),
            death_view: Arc::clone(&self.death_view),
            authenticated,
            authority,
            recall,
            extraction,
            receiver,
        };
        Box::pin(async move {
            runtime
                .persistence
                .activate_current_danger_lineage_v1(
                    stored_danger_authority(&runtime.authority),
                    runtime.authority.transfer_id(),
                    runtime.authority.entry_character_version(),
                    &stored_world_revision(runtime.authority.world_flow_revision()),
                )
                .await?;
            Ok(Box::new(PostgresCorePrivateTerminalOwner {
                task: tokio::spawn(async move {
                    let result = runtime.run().await;
                    if let Err(error) = &result {
                        tracing::error!(
                            error = %error,
                            error_debug = ?error,
                            "production private terminal owner stopped before receiver shutdown"
                        );
                    }
                    result
                }),
            }) as Box<dyn CorePrivateTerminalOwner>)
        })
    }
}

#[derive(Debug, Clone)]
struct PreparedPrivateExtractionTerminal {
    actor_lease: CoreExtractionActorLease,
    intent: ProductionExtractionPreparedIntentV1,
}

#[derive(Clone)]
pub struct CorePrivateExtractionTerminalHandle {
    source: Arc<dyn ErasedPrivateExtractionTerminalSource>,
}

impl fmt::Debug for CorePrivateExtractionTerminalHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CorePrivateExtractionTerminalHandle")
            .finish_non_exhaustive()
    }
}

impl CorePrivateExtractionTerminalHandle {
    pub(crate) fn new<Planner, Clock, TickSource>(
        directory: Arc<CoreExtractionActorDirectory<Planner, Clock, TickSource>>,
        authenticated: AuthenticatedAccount,
        route_lease: CorePrivateRouteActorLease,
    ) -> Self
    where
        Planner: ProductionExtractionPlanner + 'static,
        Clock: IdentityClock + 'static,
        TickSource: CoreExtractionAuthoritativeTick + 'static,
    {
        Self {
            source: Arc::new(BoundPrivateExtractionTerminalSource {
                directory,
                authenticated,
                route_lease,
            }),
        }
    }

    async fn prepare(
        &self,
    ) -> Result<Option<PreparedPrivateExtractionTerminal>, CorePrivateExtractionTerminalError> {
        self.source.prepare().await
    }

    async fn publish(
        &self,
        terminal: &PreparedPrivateExtractionTerminal,
        proof: &ProductionExtractionPublicationProof,
    ) -> Result<(), CorePrivateExtractionTerminalError> {
        self.source.publish(terminal, proof).await
    }

    async fn retire_after_commit(
        &self,
        actor_lease: CoreExtractionActorLease,
    ) -> Result<(), CorePrivateExtractionTerminalError> {
        self.source.retire_after_commit(actor_lease).await
    }

    async fn retire_after_other(
        &self,
        coordinator: &CoreTerminalCoordinator,
    ) -> Result<(), CorePrivateExtractionTerminalError> {
        self.source.retire_after_other(coordinator).await
    }
}

trait ErasedPrivateExtractionTerminalSource: Send + Sync {
    fn prepare(
        &self,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<
                        Option<PreparedPrivateExtractionTerminal>,
                        CorePrivateExtractionTerminalError,
                    >,
                > + Send
                + '_,
        >,
    >;

    fn publish<'a>(
        &'a self,
        terminal: &'a PreparedPrivateExtractionTerminal,
        proof: &'a ProductionExtractionPublicationProof,
    ) -> Pin<Box<dyn Future<Output = Result<(), CorePrivateExtractionTerminalError>> + Send + 'a>>;

    fn retire_after_commit(
        &self,
        actor_lease: CoreExtractionActorLease,
    ) -> Pin<Box<dyn Future<Output = Result<(), CorePrivateExtractionTerminalError>> + Send + '_>>;

    fn retire_after_other<'a>(
        &'a self,
        coordinator: &'a CoreTerminalCoordinator,
    ) -> Pin<Box<dyn Future<Output = Result<(), CorePrivateExtractionTerminalError>> + Send + 'a>>;
}

struct BoundPrivateExtractionTerminalSource<Planner, Clock, TickSource> {
    directory: Arc<CoreExtractionActorDirectory<Planner, Clock, TickSource>>,
    authenticated: AuthenticatedAccount,
    route_lease: CorePrivateRouteActorLease,
}

impl<Planner, Clock, TickSource> ErasedPrivateExtractionTerminalSource
    for BoundPrivateExtractionTerminalSource<Planner, Clock, TickSource>
where
    Planner: ProductionExtractionPlanner + 'static,
    Clock: IdentityClock + 'static,
    TickSource: CoreExtractionAuthoritativeTick + 'static,
{
    fn prepare(
        &self,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<
                        Option<PreparedPrivateExtractionTerminal>,
                        CorePrivateExtractionTerminalError,
                    >,
                > + Send
                + '_,
        >,
    > {
        Box::pin(async move {
            Ok(self
                .directory
                .terminal_intent_for_route(self.authenticated, self.route_lease)
                .await?
                .map(|(actor_lease, intent)| PreparedPrivateExtractionTerminal {
                    actor_lease,
                    intent,
                }))
        })
    }

    fn publish<'a>(
        &'a self,
        terminal: &'a PreparedPrivateExtractionTerminal,
        proof: &'a ProductionExtractionPublicationProof,
    ) -> Pin<Box<dyn Future<Output = Result<(), CorePrivateExtractionTerminalError>> + Send + 'a>>
    {
        Box::pin(async move {
            self.directory
                .publish_coordinated(terminal.actor_lease, &terminal.intent, proof)
                .await?;
            Ok(())
        })
    }

    fn retire_after_commit(
        &self,
        actor_lease: CoreExtractionActorLease,
    ) -> Pin<Box<dyn Future<Output = Result<(), CorePrivateExtractionTerminalError>> + Send + '_>>
    {
        Box::pin(async move {
            self.directory
                .retire_actor_after_commit(actor_lease)
                .await?;
            Ok(())
        })
    }

    fn retire_after_other<'a>(
        &'a self,
        coordinator: &'a CoreTerminalCoordinator,
    ) -> Pin<Box<dyn Future<Output = Result<(), CorePrivateExtractionTerminalError>> + Send + 'a>>
    {
        Box::pin(async move {
            let actor_lease = match self
                .directory
                .registered_actor_lease(self.authenticated)
                .await
            {
                Ok(lease) => lease,
                Err(CoreExtractionRuntimeError::ActorUnavailable) => return Ok(()),
                Err(error) => return Err(error.into()),
            };
            self.directory
                .retire_actor_after_other_terminal(actor_lease, coordinator)
                .await?;
            Ok(())
        })
    }
}

#[derive(Debug, Error)]
enum CorePrivateExtractionTerminalError {
    #[error("private extraction terminal source failed")]
    Source(#[from] crate::core_extraction_runtime::CoreExtractionTerminalSourceError),
    #[error("private extraction runtime failed")]
    Runtime(#[from] CoreExtractionRuntimeError),
}

#[derive(Clone)]
pub struct CorePrivateRecallTerminalHandle {
    actor: Arc<dyn ErasedPrivateRecallTerminalActor>,
}

impl fmt::Debug for CorePrivateRecallTerminalHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CorePrivateRecallTerminalHandle")
            .finish_non_exhaustive()
    }
}

impl CorePrivateRecallTerminalHandle {
    pub(crate) fn new<Clock>(actor: Arc<ProductionRecallIntentActor<Clock>>) -> Self
    where
        Clock: ProductionRecallClock + 'static,
    {
        Self { actor }
    }

    async fn drive_tick(
        &self,
        coordinator: &mut CoreTerminalCoordinator,
        persistence: &PostgresPersistence,
        executor: &PostgresProductionRecallExecutionService,
        completion: &ProductionRecallCompletionAuthorityV1,
        pending: ProductionRecallPendingAuthorityV1,
        others: CoreTerminalOtherEvaluationsV1,
    ) -> Result<CoreRecallTerminalTickOutcome, CoreRecallTerminalDriverError> {
        self.actor
            .drive_tick(
                coordinator,
                persistence,
                executor,
                completion,
                pending,
                others,
            )
            .await
    }
}

trait ErasedPrivateRecallTerminalActor: Send + Sync {
    fn drive_tick<'a>(
        &'a self,
        coordinator: &'a mut CoreTerminalCoordinator,
        persistence: &'a PostgresPersistence,
        executor: &'a PostgresProductionRecallExecutionService,
        completion: &'a ProductionRecallCompletionAuthorityV1,
        pending: ProductionRecallPendingAuthorityV1,
        others: CoreTerminalOtherEvaluationsV1,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<CoreRecallTerminalTickOutcome, CoreRecallTerminalDriverError>,
                > + Send
                + 'a,
        >,
    >;
}

impl<Clock> ErasedPrivateRecallTerminalActor for ProductionRecallIntentActor<Clock>
where
    Clock: ProductionRecallClock + 'static,
{
    fn drive_tick<'a>(
        &'a self,
        coordinator: &'a mut CoreTerminalCoordinator,
        persistence: &'a PostgresPersistence,
        executor: &'a PostgresProductionRecallExecutionService,
        completion: &'a ProductionRecallCompletionAuthorityV1,
        pending: ProductionRecallPendingAuthorityV1,
        others: CoreTerminalOtherEvaluationsV1,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<CoreRecallTerminalTickOutcome, CoreRecallTerminalDriverError>,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            match self.refresh_pending_authority(pending).await {
                Ok(()) => {}
                Err(crate::ProductionRecallChannelError::TerminalTickPinned { pinned_tick })
                    if pinned_tick == completion.server_tick => {}
                Err(error) => return Err(CoreRecallTerminalDriverError::Channel(error)),
            }
            drive_recall_terminal_tick(self, coordinator, persistence, executor, completion, others)
                .await
        })
    }
}

#[derive(Debug)]
struct PostgresCorePrivateTerminalOwner {
    task: JoinHandle<Result<(), ProductionTerminalOwnerError>>,
}

impl CorePrivateTerminalOwner for PostgresCorePrivateTerminalOwner {
    fn finish(
        self: Box<Self>,
    ) -> Pin<Box<dyn Future<Output = Result<(), CorePrivateTerminalOwnerError>> + Send>> {
        Box::pin(async move {
            self.task
                .await
                .map_err(|_| CorePrivateTerminalOwnerError::RuntimeFailed)?
                .map_err(|error| {
                    tracing::error!(%error, "production private terminal owner failed");
                    CorePrivateTerminalOwnerError::RuntimeFailed
                })
        })
    }
}

#[derive(Debug, Error)]
enum ProductionTerminalOwnerError {
    #[error("terminal owner persistence failed: {0}")]
    Persistence(#[from] PersistenceError),
    #[error("terminal owner trace failed: {0}")]
    Trace(#[from] LiveDamageTraceServiceError),
    #[error("terminal owner death planning failed")]
    Planning(#[from] PrivateDeathPlanningError),
    #[error("terminal owner death execution failed")]
    DeathExecution(#[from] DurableDeathExecutionError),
    #[error("terminal owner extraction source failed")]
    ExtractionSource(#[from] CorePrivateExtractionTerminalError),
    #[error("terminal owner extraction execution failed")]
    ExtractionExecution(#[from] ProductionExtractionExecutionError),
    #[error("terminal owner coordination failed")]
    Coordinator(#[from] CoreTerminalCoordinatorError),
    #[error("terminal owner Recall/disconnect coordination failed")]
    Recall(#[from] CoreRecallTerminalDriverError),
    #[error("terminal owner verified-fault restoration failed")]
    FaultRestoration(#[from] VerifiedFaultRestorationError),
    #[error("terminal owner route reconciliation failed")]
    RouteReconciliation(#[from] CorePrivateLifeBootstrapError),
    #[error("terminal owner acknowledgement failed")]
    Acknowledgement(#[from] CorePrivateTerminalAcknowledgementError),
    #[error("terminal owner received incoherent authority")]
    InvalidAuthority,
    #[error("terminal owner identity clock is unavailable")]
    ClockUnavailable,
}

struct ProductionTerminalOwnerRuntime {
    persistence: PostgresPersistence,
    planner: Arc<PersistentDeathPlanner>,
    death_execution: Arc<PostgresDurableDeathExecutionService>,
    extraction_execution: Arc<PostgresProductionExtractionExecutionService>,
    recall_execution: Arc<PostgresProductionRecallExecutionService>,
    route_reconciler: Arc<CorePrivateLifeRuntimeBootstrapAdapter<PostgresPersistence>>,
    death_view: Arc<sim_content::CoreDevelopmentDeathView>,
    authenticated: AuthenticatedAccount,
    authority: CorePrivateDangerEntryAuthority,
    recall: CorePrivateRecallTerminalHandle,
    extraction: Option<CorePrivateExtractionTerminalHandle>,
    receiver: CorePrivateTerminalFrameReceiver,
}

impl ProductionTerminalOwnerRuntime {
    async fn run(mut self) -> Result<(), ProductionTerminalOwnerError> {
        let terminal = self.authority.terminal();
        let mut clock = self
            .persistence
            .load_life_clock_head_v1(*terminal.account_id(), *terminal.character_id())
            .await?;
        validate_clock_binding(&clock, &self.authority)?;
        let mut trace = LiveDamageTraceService::start_or_resume_current(
            self.persistence.clone(),
            terminal,
            self.authority.entry_character_version(),
            DeathEntityIdentityAuthority::default(),
            Arc::clone(&self.death_view),
        )
        .await?;
        let mut coordinator = CoreTerminalCoordinator::new(self.authenticated, terminal)?;

        while let Some(delivery) = self.receiver.receive().await {
            match delivery.delivery().clone() {
                CorePrivateTerminalDeliveryV1::Frame(frame) => {
                    Box::pin(self.process_frame(
                        delivery,
                        *frame,
                        &mut clock,
                        &mut trace,
                        &mut coordinator,
                    ))
                    .await?;
                }
                CorePrivateTerminalDeliveryV1::RouteControl(control) => {
                    self.process_control(delivery, *control, &mut clock, &mut trace)
                        .await?;
                }
                CorePrivateTerminalDeliveryV1::VerifiedFault(fault) => {
                    Box::pin(self.process_verified_fault(
                        delivery,
                        *fault,
                        &mut clock,
                        &mut coordinator,
                    ))
                    .await?;
                }
            }
        }
        Ok(())
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one lossless frame must persist clocks/trace, build all terminal producers, commit the winner, and acknowledge exactly once"
    )]
    async fn process_frame(
        &self,
        delivery: CorePrivateTerminalFrameDelivery,
        frame: CorePrivateTerminalFrameV1,
        clock: &mut StoredLifeClockHeadV1,
        trace: &mut LiveDamageTraceService<PostgresPersistence>,
        coordinator: &mut CoreTerminalCoordinator,
    ) -> Result<(), ProductionTerminalOwnerError> {
        self.refresh_clock_character_version(clock, frame.route.character_version)
            .await?;
        self.persist_clock(frame.tick.0, frame.context.network_state, clock)
            .await?;
        let terminal_trace = self.persist_trace(&frame, trace).await?;
        let snapshot = self
            .persistence
            .load_current_danger_terminal_snapshot_v1(
                stored_danger_authority(&self.authority),
                &stored_world_revision(self.authority.world_flow_revision()),
            )
            .await?;
        if snapshot.clock.authoritative_tick != frame.tick.0
            || snapshot.extraction.expected_versions.character != frame.route.character_version
            || snapshot.clock.life_metrics_version != clock.life_metrics_version
        {
            return Err(ProductionTerminalOwnerError::InvalidAuthority);
        }

        let death = if frame.player_died {
            let terminal_trace =
                terminal_trace.ok_or(ProductionTerminalOwnerError::InvalidAuthority)?;
            Some(
                self.planner
                    .plan(PrivateDeathPlanningAuthority {
                        authenticated_account: self.authenticated,
                        danger_entry: self.authority.clone(),
                        route: frame.route.clone(),
                        terminal_trace,
                        terminal_context: frame.context.clone(),
                        issued_at_unix_ms: unix_millis()?,
                    })
                    .await?,
            )
        } else {
            if terminal_trace.is_some() {
                return Err(ProductionTerminalOwnerError::InvalidAuthority);
            }
            None
        };

        let tick = frame.tick.0;
        let version = frame.route.character_version;
        let terminal = self.authority.terminal();
        let lethal = match death.as_ref() {
            Some(death) => CoreTerminalEvaluation::candidate(
                CoreTerminalProducer::LethalHealth,
                terminal,
                tick,
                version,
                durable_death_terminal_candidate(death)?,
            ),
            None => CoreTerminalEvaluation::absent(
                CoreTerminalProducer::LethalHealth,
                terminal,
                tick,
                version,
            ),
        };
        let completion = ProductionRecallCompletionAuthorityV1 {
            account_id: *terminal.account_id(),
            character_id: *terminal.character_id(),
            instance_lineage_id: *terminal.lineage_id(),
            entry_restore_point_id: *terminal.restore_point_id(),
            expected_versions: snapshot.recall_expected_versions,
            content_revision: snapshot.extraction.content_revision.clone(),
            server_tick: tick,
            final_lifetime_ticks: snapshot.clock.lifetime_ticks,
            final_permadeath_combat_ticks: snapshot.clock.permadeath_combat_ticks,
        };
        let pending_material_stack_count = u8::try_from(snapshot.pending_material_stack_count)
            .map_err(|_| ProductionTerminalOwnerError::InvalidAuthority)?;
        let extraction = match &self.extraction {
            Some(source) => source.prepare().await?,
            None => None,
        };
        let extraction_evaluation = match extraction.as_ref() {
            Some(extraction) if extraction.intent.server_tick() < tick => {
                return Err(ProductionTerminalOwnerError::InvalidAuthority);
            }
            Some(extraction) if extraction.intent.server_tick() == tick => {
                let prepared = extraction
                    .intent
                    .prepared()
                    .ok_or(ProductionTerminalOwnerError::InvalidAuthority)?;
                CoreTerminalEvaluation::candidate(
                    CoreTerminalProducer::SuccessfulExtraction,
                    terminal,
                    tick,
                    version,
                    production_extraction_terminal_candidate(prepared)?,
                )
            }
            Some(_) | None => CoreTerminalEvaluation::absent(
                CoreTerminalProducer::SuccessfulExtraction,
                terminal,
                tick,
                version,
            ),
        };
        let outcome = self
            .recall
            .drive_tick(
                coordinator,
                &self.persistence,
                &self.recall_execution,
                &completion,
                ProductionRecallPendingAuthorityV1 {
                    pending_item_count: snapshot.pending_item_count,
                    pending_material_stack_count,
                },
                CoreTerminalOtherEvaluationsV1 {
                    lethal,
                    extraction: extraction_evaluation,
                    fault_restore: CoreTerminalEvaluation::absent(
                        CoreTerminalProducer::VerifiedFaultRestoration,
                        terminal,
                        tick,
                        version,
                    ),
                },
            )
            .await?;
        match (outcome, death, extraction) {
            (CoreRecallTerminalTickOutcome::NoTerminal, None, _) => {
                delivery.acknowledge_continue()?;
            }
            (CoreRecallTerminalTickOutcome::OtherTerminalPrepared(prepared), Some(death), _) => {
                let receipt = self
                    .execute_death_exact(coordinator, &prepared, &death)
                    .await?;
                if let Some(extraction) = &self.extraction {
                    extraction.retire_after_other(coordinator).await?;
                }
                delivery.acknowledge_terminal_owned(&receipt)?;
            }
            (
                CoreRecallTerminalTickOutcome::OtherTerminalPrepared(prepared),
                None,
                Some(extraction),
            ) if extraction.intent.server_tick() == tick => {
                let receipt = self
                    .execute_extraction_exact(coordinator, &prepared, &extraction)
                    .await?;
                delivery.acknowledge_terminal_owned(&receipt)?;
            }
            (
                CoreRecallTerminalTickOutcome::RecallStored(_)
                | CoreRecallTerminalTickOutcome::RecallReplayed(_),
                None,
                _,
            ) => {
                if let Some(extraction) = &self.extraction {
                    extraction.retire_after_other(coordinator).await?;
                }
                let receipt = coordinator
                    .committed_receipt()
                    .ok_or(ProductionTerminalOwnerError::InvalidAuthority)?;
                delivery.acknowledge_terminal_owned(receipt)?;
            }
            _ => return Err(ProductionTerminalOwnerError::InvalidAuthority),
        }
        Ok(())
    }

    async fn process_control(
        &self,
        delivery: CorePrivateTerminalFrameDelivery,
        control: CorePrivateTerminalRouteControlV1,
        clock: &mut StoredLifeClockHeadV1,
        trace: &mut LiveDamageTraceService<PostgresPersistence>,
    ) -> Result<(), ProductionTerminalOwnerError> {
        *clock = self
            .persistence
            .load_life_clock_head_v1(
                *self.authority.terminal().account_id(),
                *self.authority.terminal().character_id(),
            )
            .await?;
        validate_clock_binding(clock, &self.authority)?;
        if clock.character_version != control.route.character_version {
            return Err(ProductionTerminalOwnerError::InvalidAuthority);
        }
        if trace.danger_authority().lineage_id != *self.authority.terminal().lineage_id() {
            return Err(ProductionTerminalOwnerError::InvalidAuthority);
        }
        trace.advance_authority(
            control.route.character_version,
            trace.danger_authority().clone(),
        )?;
        if let Some((completion_id, achieved_tick)) = deed_authority(&control, self.authenticated)?
        {
            let request = LifeDeedCompletionRequestV2::seal(LifeDeedCompletionCommandV2 {
                account_id: *self.authority.terminal().account_id(),
                character_id: *self.authority.terminal().character_id(),
                completion_id,
                expected_character_version: control.route.character_version,
                expected_life_metrics_version: clock.life_metrics_version,
                lineage_id: *self.authority.terminal().lineage_id(),
                restore_point_id: *self.authority.terminal().restore_point_id(),
                achieved_tick,
                content: LifeDeedContentAuthorityV2::core(),
                issued_at_unix_ms: unix_millis()?,
            })?;
            let receipt = self.persist_deed_exact(&request).await?;
            clock.life_metrics_version = receipt.post_life_metrics_version;
        }
        delivery.acknowledge_continue()?;
        Ok(())
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the verified-fault boundary composes all five terminal producers before one durable restore"
    )]
    async fn process_verified_fault(
        &self,
        delivery: CorePrivateTerminalFrameDelivery,
        fault: CorePrivateTerminalVerifiedFaultV1,
        clock: &mut StoredLifeClockHeadV1,
        coordinator: &mut CoreTerminalCoordinator,
    ) -> Result<(), ProductionTerminalOwnerError> {
        self.refresh_clock_character_version(clock, fault.route.character_version)
            .await?;
        self.persist_clock(fault.tick.0, DeathTraceNetworkState::Connected, clock)
            .await?;
        let snapshot = self
            .persistence
            .load_current_danger_terminal_snapshot_v1(
                stored_danger_authority(&self.authority),
                &stored_world_revision(self.authority.world_flow_revision()),
            )
            .await?;
        if snapshot.clock.authoritative_tick != fault.tick.0
            || snapshot.extraction.expected_versions.character != fault.route.character_version
            || snapshot.clock.life_metrics_version != clock.life_metrics_version
        {
            return Err(ProductionTerminalOwnerError::InvalidAuthority);
        }

        let terminal = self.authority.terminal();
        let tick = fault.tick.0;
        let version = fault.route.character_version;
        let restoration = prepare_verified_fault_restoration(terminal, version, tick, fault.kind)?;
        let completion = ProductionRecallCompletionAuthorityV1 {
            account_id: *terminal.account_id(),
            character_id: *terminal.character_id(),
            instance_lineage_id: *terminal.lineage_id(),
            entry_restore_point_id: *terminal.restore_point_id(),
            expected_versions: snapshot.recall_expected_versions,
            content_revision: snapshot.extraction.content_revision.clone(),
            server_tick: tick,
            final_lifetime_ticks: snapshot.clock.lifetime_ticks,
            final_permadeath_combat_ticks: snapshot.clock.permadeath_combat_ticks,
        };
        let pending_material_stack_count = u8::try_from(snapshot.pending_material_stack_count)
            .map_err(|_| ProductionTerminalOwnerError::InvalidAuthority)?;
        let extraction = match &self.extraction {
            Some(source) => source.prepare().await?,
            None => None,
        };
        let extraction_evaluation = match extraction.as_ref() {
            Some(extraction) if extraction.intent.server_tick() < tick => {
                return Err(ProductionTerminalOwnerError::InvalidAuthority);
            }
            Some(extraction) if extraction.intent.server_tick() == tick => {
                let prepared = extraction
                    .intent
                    .prepared()
                    .ok_or(ProductionTerminalOwnerError::InvalidAuthority)?;
                CoreTerminalEvaluation::candidate(
                    CoreTerminalProducer::SuccessfulExtraction,
                    terminal,
                    tick,
                    version,
                    production_extraction_terminal_candidate(prepared)?,
                )
            }
            Some(_) | None => CoreTerminalEvaluation::absent(
                CoreTerminalProducer::SuccessfulExtraction,
                terminal,
                tick,
                version,
            ),
        };
        let outcome = self
            .recall
            .drive_tick(
                coordinator,
                &self.persistence,
                &self.recall_execution,
                &completion,
                ProductionRecallPendingAuthorityV1 {
                    pending_item_count: snapshot.pending_item_count,
                    pending_material_stack_count,
                },
                CoreTerminalOtherEvaluationsV1 {
                    lethal: CoreTerminalEvaluation::absent(
                        CoreTerminalProducer::LethalHealth,
                        terminal,
                        tick,
                        version,
                    ),
                    extraction: extraction_evaluation,
                    fault_restore: CoreTerminalEvaluation::candidate(
                        CoreTerminalProducer::VerifiedFaultRestoration,
                        terminal,
                        tick,
                        version,
                        restoration.candidate.clone(),
                    ),
                },
            )
            .await?;

        let receipt = match outcome {
            CoreRecallTerminalTickOutcome::OtherTerminalPrepared(prepared)
                if prepared.winner().kind() == TerminalKind::VerifiedServerFaultRestoration =>
            {
                if let Some(extraction) = &self.extraction {
                    extraction.retire_after_other(coordinator).await?;
                }
                self.execute_fault_restoration_exact(coordinator, &prepared, &restoration)
                    .await?
            }
            CoreRecallTerminalTickOutcome::OtherTerminalPrepared(prepared)
                if prepared.winner().kind() == TerminalKind::SuccessfulExtraction =>
            {
                let extraction = extraction
                    .as_ref()
                    .filter(|value| value.intent.server_tick() == tick)
                    .ok_or(ProductionTerminalOwnerError::InvalidAuthority)?;
                self.execute_extraction_exact(coordinator, &prepared, extraction)
                    .await?
            }
            CoreRecallTerminalTickOutcome::RecallStored(_)
            | CoreRecallTerminalTickOutcome::RecallReplayed(_) => {
                if let Some(extraction) = &self.extraction {
                    extraction.retire_after_other(coordinator).await?;
                }
                coordinator
                    .committed_receipt()
                    .cloned()
                    .ok_or(ProductionTerminalOwnerError::InvalidAuthority)?
            }
            CoreRecallTerminalTickOutcome::NoTerminal
            | CoreRecallTerminalTickOutcome::OtherTerminalPrepared(_) => {
                return Err(ProductionTerminalOwnerError::InvalidAuthority);
            }
        };
        delivery.acknowledge_terminal_owned(&receipt)?;
        Ok(())
    }

    async fn refresh_clock_character_version(
        &self,
        clock: &mut StoredLifeClockHeadV1,
        expected: u64,
    ) -> Result<(), ProductionTerminalOwnerError> {
        if clock.character_version != expected {
            *clock = self
                .persistence
                .load_life_clock_head_v1(
                    *self.authority.terminal().account_id(),
                    *self.authority.terminal().character_id(),
                )
                .await?;
            validate_clock_binding(clock, &self.authority)?;
        }
        if clock.character_version != expected {
            return Err(ProductionTerminalOwnerError::InvalidAuthority);
        }
        Ok(())
    }

    async fn persist_clock(
        &self,
        tick: u64,
        network: DeathTraceNetworkState,
        clock: &mut StoredLifeClockHeadV1,
    ) -> Result<(), ProductionTerminalOwnerError> {
        if tick <= clock.authoritative_tick {
            if tick == clock.authoritative_tick {
                return Ok(());
            }
            return Err(ProductionTerminalOwnerError::InvalidAuthority);
        }
        let advanced_ticks = u32::try_from(tick - clock.authoritative_tick)
            .map_err(|_| ProductionTerminalOwnerError::InvalidAuthority)?;
        let danger = clock
            .danger
            .clone()
            .ok_or(ProductionTerminalOwnerError::InvalidAuthority)?;
        let state = if network == DeathTraceNetworkState::LinkLost {
            LifeClockStateV1::DangerLinkLost
        } else {
            LifeClockStateV1::DangerControllable
        };
        let command = LifeClockCheckpointCommandV1 {
            account_id: *self.authority.terminal().account_id(),
            character_id: *self.authority.terminal().character_id(),
            checkpoint_id: deterministic_id(&self.authority, b"life-clock", tick),
            expected_character_version: clock.character_version,
            expected_life_metrics_version: clock.life_metrics_version,
            authoritative_tick: tick,
            state,
            advanced_ticks,
            danger: Some(danger),
            content: LifeClockContentAuthorityV1::core(),
            issued_at_unix_ms: unix_millis()?,
        };
        let request = LifeClockCheckpointRequestV1::seal(command)?;
        loop {
            match self
                .persistence
                .transact_life_clock_checkpoint_v1(&request)
                .await
            {
                Ok(transaction) => {
                    let receipt = transaction.receipt().clone();
                    clock.lifetime_ticks = receipt.post_lifetime_ticks;
                    clock.permadeath_combat_ticks = receipt.post_permadeath_combat_ticks;
                    clock.life_metrics_version = receipt.post_life_metrics_version;
                    clock.authoritative_tick = receipt.command.authoritative_tick;
                    clock.link_lost_ticks = receipt.post_link_lost_ticks;
                    clock.danger.clone_from(&receipt.command.danger);
                    clock.latest_receipt = Some(receipt);
                    return Ok(());
                }
                Err(error) if error.may_have_ambiguous_commit_outcome() => {
                    tokio::time::sleep(RETRY_DELAY).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    async fn persist_trace(
        &self,
        frame: &CorePrivateTerminalFrameV1,
        trace: &mut LiveDamageTraceService<PostgresPersistence>,
    ) -> Result<Option<crate::PreparedTerminalLiveDamageTrace>, ProductionTerminalOwnerError> {
        if frame.damage.is_empty() {
            return Ok(None);
        }
        let identities = private_route_damage_entity_identities(&self.authority, &frame.damage)
            .map_err(|_| ProductionTerminalOwnerError::InvalidAuthority)?;
        trace.register_entity_identities(&identities)?;
        if trace.danger_authority().lineage_id != *self.authority.terminal().lineage_id()
            || trace.danger_authority().restore_point_id
                != *self.authority.terminal().restore_point_id()
        {
            return Err(ProductionTerminalOwnerError::InvalidAuthority);
        }
        trace.advance_authority(
            frame.route.character_version,
            trace.danger_authority().clone(),
        )?;
        let mutation = LiveDamageTraceMutationAuthority::new(
            deterministic_id(&self.authority, b"damage-trace", frame.tick.0),
            frame.route.character_version,
            trace.danger_authority().clone(),
            unix_millis()?,
        )?;
        let observations = frame
            .damage
            .iter()
            .map(|fact| damage_observation(fact, &frame.context))
            .collect();
        match trace.ingest_tick(mutation, observations).await {
            Ok(LiveDamageTraceIngestOutcome::TerminalPrepared(prepared)) => Ok(Some(*prepared)),
            Ok(
                LiveDamageTraceIngestOutcome::EmptyTick
                | LiveDamageTraceIngestOutcome::Committed(_)
                | LiveDamageTraceIngestOutcome::Replayed(_),
            ) => Ok(None),
            Err(LiveDamageTraceServiceError::Persistence(error))
                if error.may_have_ambiguous_commit_outcome() =>
            {
                loop {
                    tokio::time::sleep(RETRY_DELAY).await;
                    match trace.retry_pending().await {
                        Ok(
                            LiveDamageTraceIngestOutcome::Committed(_)
                            | LiveDamageTraceIngestOutcome::Replayed(_),
                        ) => return Ok(None),
                        Err(LiveDamageTraceServiceError::Persistence(error))
                            if error.may_have_ambiguous_commit_outcome() => {}
                        Ok(_) => return Err(ProductionTerminalOwnerError::InvalidAuthority),
                        Err(error) => return Err(error.into()),
                    }
                }
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn persist_deed_exact(
        &self,
        request: &LifeDeedCompletionRequestV2,
    ) -> Result<persistence::StoredLifeDeedCompletionV2, ProductionTerminalOwnerError> {
        loop {
            match self
                .persistence
                .transact_life_deed_completion_v2(request)
                .await
            {
                Ok(transaction) => return Ok(transaction.receipt().clone()),
                Err(error) if error.may_have_ambiguous_commit_outcome() => {
                    tokio::time::sleep(RETRY_DELAY).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    async fn execute_death_exact(
        &self,
        coordinator: &mut CoreTerminalCoordinator,
        prepared: &crate::PreparedTerminal,
        death: &PreparedDurableDeathCommit,
    ) -> Result<StoredTerminalReceipt, ProductionTerminalOwnerError> {
        loop {
            match self
                .death_execution
                .execute_coordinated(coordinator, prepared, death)
                .await
            {
                Ok(_) => {
                    return coordinator
                        .committed_receipt()
                        .cloned()
                        .ok_or(ProductionTerminalOwnerError::InvalidAuthority);
                }
                Err(DurableDeathExecutionError::Persistence(error))
                    if error.may_have_ambiguous_commit_outcome() =>
                {
                    tokio::time::sleep(RETRY_DELAY).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    async fn execute_extraction_exact(
        &self,
        coordinator: &mut CoreTerminalCoordinator,
        prepared_terminal: &crate::PreparedTerminal,
        terminal: &PreparedPrivateExtractionTerminal,
    ) -> Result<StoredTerminalReceipt, ProductionTerminalOwnerError> {
        let prepared = terminal
            .intent
            .prepared()
            .ok_or(ProductionTerminalOwnerError::InvalidAuthority)?;
        loop {
            match self
                .extraction_execution
                .execute_coordinated(coordinator, prepared_terminal, prepared)
                .await
            {
                Ok(outcome) => {
                    let source = self
                        .extraction
                        .as_ref()
                        .ok_or(ProductionTerminalOwnerError::InvalidAuthority)?;
                    source
                        .publish(terminal, outcome.publication_proof())
                        .await?;
                    source.retire_after_commit(terminal.actor_lease).await?;
                    return coordinator
                        .committed_receipt()
                        .cloned()
                        .ok_or(ProductionTerminalOwnerError::InvalidAuthority);
                }
                Err(ProductionExtractionExecutionError::Persistence(error))
                    if error.may_have_ambiguous_commit_outcome() =>
                {
                    tokio::time::sleep(RETRY_DELAY).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    async fn execute_fault_restoration_exact(
        &self,
        coordinator: &mut CoreTerminalCoordinator,
        prepared: &crate::PreparedTerminal,
        restoration: &PreparedVerifiedFaultRestoration,
    ) -> Result<StoredTerminalReceipt, ProductionTerminalOwnerError> {
        validate_fault_winner(prepared, restoration)?;
        loop {
            match self
                .persistence
                .transact_danger_crash_restore(&restoration.request)
                .await
            {
                Ok(
                    DangerCrashRestoreTransaction::Fresh(receipt)
                    | DangerCrashRestoreTransaction::Replayed(receipt),
                ) => {
                    receipt.validate()?;
                    if receipt.code != DangerCrashRestoreCode::Restored
                        || receipt.account_id != restoration.request.account_id
                        || receipt.character_id != restoration.request.character_id
                        || receipt.restore_point_id != restoration.request.restore_point_id
                        || receipt.request_mutation_id != restoration.request.mutation_id
                        || receipt.request_hash != restoration.request.request_hash
                    {
                        return Err(ProductionTerminalOwnerError::InvalidAuthority);
                    }
                    let stored = StoredTerminalReceipt::from_prepared(
                        prepared,
                        prepared.sealed_through_tick(),
                        receipt.digest(),
                    )
                    .map_err(|_| ProductionTerminalOwnerError::InvalidAuthority)?;
                    self.route_reconciler
                        .reconcile_verified_fault_restoration(
                            self.authenticated,
                            self.authority.route_lease(),
                            stored.expected_state_version(),
                            &receipt,
                        )
                        .await?;
                    coordinator.record_commit(stored.clone())?;
                    return Ok(stored);
                }
                Ok(DangerCrashRestoreTransaction::Conflict { .. }) => {
                    return Err(ProductionTerminalOwnerError::InvalidAuthority);
                }
                Err(error) if error.may_have_ambiguous_commit_outcome() => {
                    tokio::time::sleep(RETRY_DELAY).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}

fn stored_danger_authority(
    authority: &CorePrivateDangerEntryAuthority,
) -> StoredActiveDangerAuthorityV1 {
    let terminal = authority.terminal();
    StoredActiveDangerAuthorityV1 {
        account_id: *terminal.account_id(),
        character_id: *terminal.character_id(),
        instance_lineage_id: *terminal.lineage_id(),
        entry_restore_point_id: *terminal.restore_point_id(),
    }
}

fn stored_world_revision(
    revision: &protocol::WorldFlowContentRevisionV1,
) -> StoredWorldFlowRevisionV1 {
    StoredWorldFlowRevisionV1 {
        records_blake3: revision.records_blake3.as_str().to_owned(),
        assets_blake3: revision.assets_blake3.as_str().to_owned(),
        localization_blake3: revision.localization_blake3.as_str().to_owned(),
    }
}

fn validate_clock_binding(
    clock: &StoredLifeClockHeadV1,
    authority: &CorePrivateDangerEntryAuthority,
) -> Result<(), ProductionTerminalOwnerError> {
    let danger = clock
        .danger
        .as_ref()
        .ok_or(ProductionTerminalOwnerError::InvalidAuthority)?;
    if danger.lineage_id != *authority.terminal().lineage_id()
        || danger.restore_point_id != *authority.terminal().restore_point_id()
        || clock.character_version < authority.entry_character_version()
    {
        return Err(ProductionTerminalOwnerError::InvalidAuthority);
    }
    Ok(())
}

fn damage_observation(
    fact: &CorePrivatePlayerDamageFactV1,
    context: &crate::CorePrivateTerminalTickContextV1,
) -> DamageTraceObservation {
    DamageTraceObservation {
        tick: fact.tick,
        event_ordinal: fact.event_ordinal,
        cause_kind: fact.cause_kind,
        source_content_id: fact.source_content_id.to_owned(),
        source_entity_id: Some(fact.source_entity_id),
        pattern_id: Some(fact.pattern_id.to_owned()),
        attack_id: fact.attack_id.to_owned(),
        raw_damage: fact.raw_damage,
        final_damage: fact.final_damage,
        damage_type: fact.damage_type,
        pre_health: fact.pre_health,
        post_health: fact.post_health,
        source_position: fact.source_position,
        statuses: context.statuses.to_vec(),
        network_state: context.network_state,
        recall_state: context.recall_state,
    }
}

fn deed_authority(
    control: &CorePrivateTerminalRouteControlV1,
    authenticated: AuthenticatedAccount,
) -> Result<Option<([u8; 16], u64)>, ProductionTerminalOwnerError> {
    let authority = match &control.authority {
        CorePrivateTerminalRouteControlAuthorityV1::B3RewardCommitted { durable, .. } => {
            if durable.reward_result_hash().is_some() {
                Some((durable.reward_event_id(), durable.handoff().death_tick.0))
            } else {
                None
            }
        }
        CorePrivateTerminalRouteControlAuthorityV1::CaldusRewardCommitted { durable, .. } => {
            let owner = durable
                .exit()
                .owners
                .iter()
                .find(|owner| {
                    owner.account_id == authenticated.account_id.as_bytes()
                        && owner.character_id == durable.handoff().character_id()
                })
                .ok_or(ProductionTerminalOwnerError::InvalidAuthority)?;
            Some((owner.reward_request_id, durable.handoff().defeat_tick().0))
        }
        _ => None,
    };
    Ok(authority)
}

fn deterministic_id(
    authority: &CorePrivateDangerEntryAuthority,
    domain: &[u8],
    tick: u64,
) -> [u8; 16] {
    let terminal = authority.terminal();
    let mut hasher = blake3::Hasher::new_derive_key("gravebound.private-terminal-owner-id.v1");
    hasher.update(domain);
    hasher.update(terminal.account_id());
    hasher.update(terminal.character_id());
    hasher.update(terminal.lineage_id());
    hasher.update(terminal.restore_point_id());
    hasher.update(&tick.to_be_bytes());
    let mut id = [0; 16];
    id.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
    if id == [0; 16] {
        id[15] = 1;
    }
    id
}

fn unix_millis() -> Result<u64, ProductionTerminalOwnerError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ProductionTerminalOwnerError::ClockUnavailable)?
        .as_millis();
    u64::try_from(millis).map_err(|_| ProductionTerminalOwnerError::ClockUnavailable)
}
