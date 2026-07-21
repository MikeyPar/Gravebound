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
    pin::Pin,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use persistence::{
    LifeClockCheckpointCommandV1, LifeClockCheckpointRequestV1, LifeClockContentAuthorityV1,
    LifeClockStateV1, LifeDeedCompletionCommandV2, LifeDeedCompletionRequestV2,
    LifeDeedContentAuthorityV2, PersistenceError, PostgresPersistence, StoredLifeClockHeadV1,
};
use sim_core::{DamageTraceObservation, DeathTraceNetworkState};
use thiserror::Error;
use tokio::task::JoinHandle;

use crate::{
    AuthenticatedAccount, CorePrivateDangerEntryAuthority, CorePrivatePlayerDamageFactV1,
    CorePrivateTerminalAcknowledgementError, CorePrivateTerminalDeliveryV1,
    CorePrivateTerminalFrameDelivery, CorePrivateTerminalFrameReceiver, CorePrivateTerminalFrameV1,
    CorePrivateTerminalOwner, CorePrivateTerminalOwnerError, CorePrivateTerminalOwnerFactory,
    CorePrivateTerminalRouteControlAuthorityV1, CorePrivateTerminalRouteControlV1,
    CoreTerminalCoordinator, CoreTerminalCoordinatorError, CoreTerminalEvaluation,
    CoreTerminalProducer, CoreTerminalTickSeal, DeathEntityIdentityAuthority,
    DurableDeathExecutionError, LiveDamageTraceIngestOutcome, LiveDamageTraceMutationAuthority,
    LiveDamageTraceService, LiveDamageTraceServiceError, PostgresDurableDeathExecutionService,
    PostgresPrivateDeathContextPlanner, PreparedDurableDeathCommit, PrivateDeathPlanningAuthority,
    PrivateDeathPlanningError, StoredTerminalReceipt, SystemDurableDeathIdentitySource,
    durable_death_terminal_candidate, private_route_damage_entity_identities,
};

type PersistentDeathPlanner =
    PostgresPrivateDeathContextPlanner<PostgresPersistence, SystemDurableDeathIdentitySource>;

const RETRY_DELAY: Duration = Duration::from_millis(100);

#[derive(Clone)]
pub struct PostgresCorePrivateTerminalOwnerFactory {
    persistence: PostgresPersistence,
    planner: Arc<PersistentDeathPlanner>,
    death_execution: Arc<PostgresDurableDeathExecutionService>,
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
    ) -> Self {
        Self {
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
        receiver: CorePrivateTerminalFrameReceiver,
    ) -> Result<Box<dyn CorePrivateTerminalOwner>, CorePrivateTerminalOwnerError> {
        if authenticated.account_id.as_bytes() != *authority.terminal().account_id()
            || receiver.binding() != authority.terminal()
        {
            return Err(CorePrivateTerminalOwnerError::StartFailed);
        }
        let runtime = ProductionTerminalOwnerRuntime {
            persistence: self.persistence.clone(),
            planner: Arc::clone(&self.planner),
            death_execution: Arc::clone(&self.death_execution),
            death_view: Arc::clone(&self.death_view),
            authenticated,
            authority,
            receiver,
        };
        Ok(Box::new(PostgresCorePrivateTerminalOwner {
            task: tokio::spawn(runtime.run()),
        }))
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
    #[error("terminal owner persistence failed")]
    Persistence(#[from] PersistenceError),
    #[error("terminal owner trace failed")]
    Trace(#[from] LiveDamageTraceServiceError),
    #[error("terminal owner death planning failed")]
    Planning(#[from] PrivateDeathPlanningError),
    #[error("terminal owner death execution failed")]
    DeathExecution(#[from] DurableDeathExecutionError),
    #[error("terminal owner coordination failed")]
    Coordinator(#[from] CoreTerminalCoordinatorError),
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
    death_view: Arc<sim_content::CoreDevelopmentDeathView>,
    authenticated: AuthenticatedAccount,
    authority: CorePrivateDangerEntryAuthority,
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
            }
        }
        Ok(())
    }

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

        let terminal = self.authority.terminal();
        let tick = frame.tick.0;
        let version = frame.route.character_version;
        for producer in CoreTerminalProducer::ALL {
            let evaluation = if producer == CoreTerminalProducer::LethalHealth {
                match death.as_ref() {
                    Some(death) => CoreTerminalEvaluation::candidate(
                        producer,
                        terminal,
                        tick,
                        version,
                        durable_death_terminal_candidate(death)?,
                    ),
                    None => CoreTerminalEvaluation::absent(producer, terminal, tick, version),
                }
            } else {
                // Recall, extraction, disconnect recovery, and verified-fault candidates are
                // admitted through their process-owned actors. Until their shared mailbox is
                // composed, an ordinary simulation frame supplies an explicit absence only.
                CoreTerminalEvaluation::absent(producer, terminal, tick, version)
            };
            coordinator.evaluate(evaluation)?;
        }
        let seal = coordinator.seal_authoritative_tick(tick, version)?;
        match (seal, death) {
            (CoreTerminalTickSeal::NoTerminal { .. }, None) => delivery.acknowledge_continue()?,
            (CoreTerminalTickSeal::Prepared(prepared), Some(death)) => {
                let receipt = self
                    .execute_death_exact(coordinator, &prepared, &death)
                    .await?;
                delivery.acknowledge_terminal_owned(&receipt)?;
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
