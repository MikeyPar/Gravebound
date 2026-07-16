//! Serialized five-producer terminal driving for production Emergency Recall.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` `DTH-010`,
//! `DTH-011`, and `TECH-015`/`021`-`023`;
//! `Gravebound_Content_Production_Spec_v1.md` Core danger-route and Hall
//! `(32,42)` contracts; `Gravebound_Development_Roadmap_v1.md`
//! `GB-M03-03`/`07`/`08`; and accepted `SPEC-CONFLICT-029`.
//!
//! The character actor first prepares an immutable explicit/`LinkLost` bundle.
//! A cloned coordinator then evaluates all five producers in canonical order and
//! replaces live state only after the complete barrier seals. Persistence outage
//! leaves both actor and coordinator pinned at the exact unresolved tick.

use persistence::PreparedProductionRecallV1;
use thiserror::Error;

use crate::{
    CoreTerminalCoordinator, CoreTerminalCoordinatorError, CoreTerminalEvaluation,
    CoreTerminalProducer, CoreTerminalTickSeal, PreparedTerminal, ProductionRecallChannelError,
    ProductionRecallClock, ProductionRecallCompletionAuthorityV1, ProductionRecallExecutionError,
    ProductionRecallExecutionService, ProductionRecallIntentActor, ProductionRecallPlanner,
    ProductionRecallPublishedV1, ProductionRecallTickBundle, ProductionRecallWriter, TerminalKind,
    production_recall_terminal_candidate, published_recall_from_transaction,
    validate_published_recall_receipt,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreTerminalOtherEvaluationsV1 {
    pub lethal: CoreTerminalEvaluation,
    pub extraction: CoreTerminalEvaluation,
    pub fault_restore: CoreTerminalEvaluation,
}

impl CoreTerminalOtherEvaluationsV1 {
    fn has_candidate(&self) -> bool {
        self.lethal.has_candidate()
            || self.extraction.has_candidate()
            || self.fault_restore.has_candidate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreRecallTerminalTickOutcome {
    NoTerminal,
    OtherTerminalPrepared(PreparedTerminal),
    RecallStored(ProductionRecallPublishedV1),
    RecallReplayed(ProductionRecallPublishedV1),
}

#[derive(Debug, Error)]
pub enum CoreRecallTerminalDriverError {
    #[error("Recall actor rejected terminal-tick authority")]
    Channel(#[from] ProductionRecallChannelError),
    #[error("five-producer terminal coordination failed")]
    Coordinator(#[from] CoreTerminalCoordinatorError),
    #[error("Recall terminal execution or publication failed")]
    Execution(#[from] ProductionRecallExecutionError),
    #[error("terminal producer bundle does not match its immutable actor snapshot")]
    InvalidProducerBundle,
    #[error("sealed Recall winner has no matching prepared repository plan")]
    MissingPreparedRecall,
    #[error("committed terminal coordinator has no actor publication and requires recovery")]
    CommittedCoordinatorRequiresRecovery,
    #[error("committed Recall replay belongs to a different live actor binding")]
    ReplayBindingMismatch,
}

/// Drives one immutable actor tick through all five terminal producers.
///
/// A stored replay never enters a new barrier. A fresh barrier is evaluated on
/// a clone, so rejected producer authority cannot partially mutate live state.
#[allow(
    clippy::too_many_arguments,
    reason = "the driver explicitly composes the actor, coordinator, planner, writer, immutable snapshot, and three non-Recall producers"
)]
pub async fn drive_recall_terminal_tick<Clock, Planner, Writer>(
    actor: &ProductionRecallIntentActor<Clock>,
    coordinator: &mut CoreTerminalCoordinator,
    planner: &Planner,
    executor: &ProductionRecallExecutionService<Writer>,
    completion: &ProductionRecallCompletionAuthorityV1,
    others: CoreTerminalOtherEvaluationsV1,
) -> Result<CoreRecallTerminalTickOutcome, CoreRecallTerminalDriverError>
where
    Clock: ProductionRecallClock,
    Planner: ProductionRecallPlanner,
    Writer: ProductionRecallWriter,
{
    if let Some(published) = actor.published_recall().await {
        let receipt = coordinator
            .committed_receipt()
            .ok_or(CoreRecallTerminalDriverError::CommittedCoordinatorRequiresRecovery)?;
        if receipt.binding().account_id() != &actor.account_id()
            || receipt.binding().character_id() != &actor.character_id()
        {
            return Err(CoreRecallTerminalDriverError::ReplayBindingMismatch);
        }
        validate_published_recall_receipt(&published, receipt)?;
        return Ok(CoreRecallTerminalTickOutcome::RecallReplayed(
            published.as_replayed(),
        ));
    }
    if coordinator.committed_receipt().is_some() {
        return Err(CoreRecallTerminalDriverError::CommittedCoordinatorRequiresRecovery);
    }

    validate_live_binding(actor, coordinator, completion)?;
    let snapshot_hash = terminal_snapshot_hash(completion, &others);
    if let Some(prepared_terminal) = coordinator.prepared_terminal().cloned() {
        validate_other_evaluations(completion, &others)?;
        validate_prepared_tick(completion, &prepared_terminal)?;
        actor
            .pin_terminal_snapshot(completion.server_tick, snapshot_hash)
            .await?;
        if !is_recall_kind(prepared_terminal.winner().kind()) {
            return Ok(CoreRecallTerminalTickOutcome::OtherTerminalPrepared(
                prepared_terminal,
            ));
        }
        let bundle = actor
            .evaluate_terminal_tick(planner, completion, snapshot_hash)
            .await?;
        return execute_existing_winner(actor, coordinator, executor, prepared_terminal, bundle)
            .await;
    }

    if others.has_candidate() {
        actor
            .pin_terminal_snapshot(completion.server_tick, snapshot_hash)
            .await?;
    }
    let recall_bundle = actor
        .evaluate_terminal_tick(planner, completion, snapshot_hash)
        .await?;
    if let ProductionRecallTickBundle::CommittedReplay { prepared } = recall_bundle {
        return publish_committed_replay(actor, coordinator, executor, prepared.as_ref()).await;
    }
    validate_other_evaluations(completion, &others)?;

    let ProductionRecallTickBundle::Evaluated {
        emergency,
        disconnect,
    } = recall_bundle
    else {
        unreachable!("committed replay returned above");
    };
    let mut staged = coordinator.clone();
    for producer in CoreTerminalProducer::ALL {
        let evaluation = match producer {
            CoreTerminalProducer::LethalHealth => others.lethal.clone(),
            CoreTerminalProducer::SuccessfulExtraction => others.extraction.clone(),
            CoreTerminalProducer::EmergencyRecall => emergency.evaluation.clone(),
            CoreTerminalProducer::DisconnectRecovery => disconnect.evaluation.clone(),
            CoreTerminalProducer::VerifiedFaultRestoration => others.fault_restore.clone(),
        };
        staged.evaluate(evaluation)?;
    }
    let seal = staged.seal_authoritative_tick(
        completion.server_tick,
        completion.expected_versions.character,
    )?;
    *coordinator = staged;

    let CoreTerminalTickSeal::Prepared(prepared_terminal) = seal else {
        return Ok(CoreRecallTerminalTickOutcome::NoTerminal);
    };
    if !is_recall_kind(prepared_terminal.winner().kind()) {
        return Ok(CoreRecallTerminalTickOutcome::OtherTerminalPrepared(
            prepared_terminal,
        ));
    }
    let prepared_recall = take_winning_recall(&prepared_terminal, *emergency, *disconnect)?;
    execute_fresh_winner(
        actor,
        coordinator,
        executor,
        &prepared_terminal,
        prepared_recall.as_ref(),
    )
    .await
}

async fn execute_existing_winner<Clock, Writer>(
    actor: &ProductionRecallIntentActor<Clock>,
    coordinator: &mut CoreTerminalCoordinator,
    executor: &ProductionRecallExecutionService<Writer>,
    prepared_terminal: PreparedTerminal,
    bundle: ProductionRecallTickBundle,
) -> Result<CoreRecallTerminalTickOutcome, CoreRecallTerminalDriverError>
where
    Clock: ProductionRecallClock,
    Writer: ProductionRecallWriter,
{
    match bundle {
        ProductionRecallTickBundle::CommittedReplay { prepared } => {
            publish_committed_replay(actor, coordinator, executor, prepared.as_ref()).await
        }
        ProductionRecallTickBundle::Evaluated {
            emergency,
            disconnect,
        } => {
            let prepared_recall = take_winning_recall(&prepared_terminal, *emergency, *disconnect)?;
            execute_fresh_winner(
                actor,
                coordinator,
                executor,
                &prepared_terminal,
                prepared_recall.as_ref(),
            )
            .await
        }
    }
}

async fn execute_fresh_winner<Clock, Writer>(
    actor: &ProductionRecallIntentActor<Clock>,
    coordinator: &mut CoreTerminalCoordinator,
    executor: &ProductionRecallExecutionService<Writer>,
    prepared_terminal: &PreparedTerminal,
    recall: &PreparedProductionRecallV1,
) -> Result<CoreRecallTerminalTickOutcome, CoreRecallTerminalDriverError>
where
    Clock: ProductionRecallClock,
    Writer: ProductionRecallWriter,
{
    let outcome = executor
        .execute_coordinated(coordinator, prepared_terminal, recall)
        .await?;
    let replayed = outcome.transaction.is_replay();
    let published = published_recall_from_transaction(&outcome.transaction)?;
    let receipt = coordinator
        .committed_receipt()
        .ok_or(CoreRecallTerminalDriverError::CommittedCoordinatorRequiresRecovery)?;
    validate_published_recall_receipt(&published, receipt)?;
    actor.publish_recall(published.clone()).await?;
    if replayed {
        Ok(CoreRecallTerminalTickOutcome::RecallReplayed(published))
    } else {
        Ok(CoreRecallTerminalTickOutcome::RecallStored(published))
    }
}

async fn publish_committed_replay<Clock, Writer>(
    actor: &ProductionRecallIntentActor<Clock>,
    coordinator: &mut CoreTerminalCoordinator,
    executor: &ProductionRecallExecutionService<Writer>,
    recall: &PreparedProductionRecallV1,
) -> Result<CoreRecallTerminalTickOutcome, CoreRecallTerminalDriverError>
where
    Clock: ProductionRecallClock,
    Writer: ProductionRecallWriter,
{
    let replay = executor.replay_committed(recall).await?;
    if replay.receipt.binding() != coordinator.binding()
        || replay.receipt.binding().account_id() != &actor.account_id()
        || replay.receipt.binding().character_id() != &actor.character_id()
    {
        return Err(CoreRecallTerminalDriverError::ReplayBindingMismatch);
    }
    let published = published_recall_from_transaction(&replay.transaction)?;
    validate_published_recall_receipt(&published, &replay.receipt)?;
    let restored = CoreTerminalCoordinator::from_stored_receipt(
        coordinator.authenticated_account(),
        replay.receipt.clone(),
    )?;
    actor.publish_recall(published.clone()).await?;
    *coordinator = restored;
    Ok(CoreRecallTerminalTickOutcome::RecallReplayed(published))
}

fn take_winning_recall(
    prepared_terminal: &PreparedTerminal,
    emergency: crate::ProductionRecallProducerEvaluation,
    disconnect: crate::ProductionRecallProducerEvaluation,
) -> Result<Box<PreparedProductionRecallV1>, CoreRecallTerminalDriverError> {
    let prepared = match prepared_terminal.winner().kind() {
        TerminalKind::EmergencyRecall => emergency.prepared,
        TerminalKind::DisconnectRecovery => disconnect.prepared,
        _ => return Err(CoreRecallTerminalDriverError::MissingPreparedRecall),
    }
    .ok_or(CoreRecallTerminalDriverError::MissingPreparedRecall)?;
    let candidate = production_recall_terminal_candidate(prepared.as_ref())?;
    if &candidate != prepared_terminal.winner() {
        return Err(CoreRecallTerminalDriverError::MissingPreparedRecall);
    }
    Ok(prepared)
}

fn validate_other_evaluations(
    completion: &ProductionRecallCompletionAuthorityV1,
    others: &CoreTerminalOtherEvaluationsV1,
) -> Result<(), CoreRecallTerminalDriverError> {
    for (evaluation, producer) in [
        (&others.lethal, CoreTerminalProducer::LethalHealth),
        (
            &others.extraction,
            CoreTerminalProducer::SuccessfulExtraction,
        ),
        (
            &others.fault_restore,
            CoreTerminalProducer::VerifiedFaultRestoration,
        ),
    ] {
        if evaluation.producer() != producer
            || evaluation.observed_tick() != completion.server_tick
            || evaluation.expected_state_version() != completion.expected_versions.character
        {
            return Err(CoreRecallTerminalDriverError::InvalidProducerBundle);
        }
    }
    Ok(())
}

fn validate_prepared_tick(
    completion: &ProductionRecallCompletionAuthorityV1,
    prepared: &PreparedTerminal,
) -> Result<(), CoreRecallTerminalDriverError> {
    if prepared.winner().observed_tick() != completion.server_tick
        || prepared.winner().expected_state_version() != completion.expected_versions.character
    {
        return Err(CoreRecallTerminalDriverError::InvalidProducerBundle);
    }
    Ok(())
}

fn validate_live_binding<Clock>(
    actor: &ProductionRecallIntentActor<Clock>,
    coordinator: &CoreTerminalCoordinator,
    completion: &ProductionRecallCompletionAuthorityV1,
) -> Result<(), CoreRecallTerminalDriverError> {
    let completion_binding = completion.binding()?;
    if coordinator.binding() != completion_binding
        || completion_binding.account_id() != &actor.account_id()
        || completion_binding.character_id() != &actor.character_id()
        || coordinator.authenticated_account().account_id.as_bytes() != actor.account_id()
    {
        return Err(CoreRecallTerminalDriverError::InvalidProducerBundle);
    }
    Ok(())
}

fn terminal_snapshot_hash(
    completion: &ProductionRecallCompletionAuthorityV1,
    others: &CoreTerminalOtherEvaluationsV1,
) -> [u8; 32] {
    let mut hasher =
        blake3::Hasher::new_derive_key("gravebound.production-recall-terminal-snapshot.v1");
    for part in [
        completion.account_id.as_slice(),
        completion.character_id.as_slice(),
        completion.instance_lineage_id.as_slice(),
        completion.entry_restore_point_id.as_slice(),
        completion
            .expected_versions
            .account
            .to_be_bytes()
            .as_slice(),
        completion
            .expected_versions
            .character
            .to_be_bytes()
            .as_slice(),
        completion.expected_versions.world.to_be_bytes().as_slice(),
        completion
            .expected_versions
            .inventory
            .to_be_bytes()
            .as_slice(),
        completion
            .expected_versions
            .life_metrics
            .to_be_bytes()
            .as_slice(),
        completion
            .expected_versions
            .progression
            .to_be_bytes()
            .as_slice(),
        completion
            .expected_versions
            .oath_bargain
            .to_be_bytes()
            .as_slice(),
        completion
            .expected_versions
            .ash_wallet
            .to_be_bytes()
            .as_slice(),
        completion.content_revision.records_blake3.as_bytes(),
        completion.content_revision.assets_blake3.as_bytes(),
        completion.content_revision.localization_blake3.as_bytes(),
        completion.server_tick.to_be_bytes().as_slice(),
        completion.final_lifetime_ticks.to_be_bytes().as_slice(),
        completion
            .final_permadeath_combat_ticks
            .to_be_bytes()
            .as_slice(),
        others.lethal.snapshot_hash().as_slice(),
        others.extraction.snapshot_hash().as_slice(),
        others.fault_restore.snapshot_hash().as_slice(),
    ] {
        hash_snapshot_part(&mut hasher, part);
    }
    *hasher.finalize().as_bytes()
}

fn hash_snapshot_part(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

const fn is_recall_kind(kind: TerminalKind) -> bool {
    matches!(
        kind,
        TerminalKind::EmergencyRecall | TerminalKind::DisconnectRecovery
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use persistence::{
        PersistenceError, ProductionRecallCommitRequestV1, ProductionRecallExpectedVersionsV1,
        ProductionRecallTransactionV1, ProductionRecallVersionAdvanceV1,
        ProductionRecallVersionsV1, StoredProductionRecallResultV1, StoredWorldFlowRevisionV1,
        canonical_production_recall_plan_hash_v1,
    };
    use protocol::{
        CharacterLocation, RecallFrameV1, RecallIntentV1, RecallResultV1, RecallTerminalTriggerV1,
        SafeArrival, TERMINAL_INVENTORY_SCHEMA_VERSION, TerminalInventoryRejectionCodeV1,
    };

    use crate::{
        AccountId, AuthenticatedAccount, AuthenticatedNamespace, CoreTerminalEvaluation,
        ProductionRecallPendingAuthorityV1, StoredTerminalReceipt, SubmitResult, TerminalArbiter,
        TerminalBinding, TerminalCandidate,
    };

    use super::*;

    #[derive(Debug, Clone, Copy)]
    struct FixedClock(u64);

    impl ProductionRecallClock for FixedClock {
        fn unix_millis(&self) -> u64 {
            self.0
        }
    }

    #[derive(Clone)]
    struct FakePlanner {
        replayed: bool,
        fail_first: bool,
        calls: Arc<AtomicUsize>,
    }

    impl FakePlanner {
        fn fresh() -> Self {
            Self {
                replayed: false,
                fail_first: false,
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn replayed() -> Self {
            Self {
                replayed: true,
                fail_first: false,
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn fail_once() -> Self {
            Self {
                replayed: false,
                fail_first: true,
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl ProductionRecallPlanner for FakePlanner {
        async fn prepare(
            &self,
            request: &ProductionRecallCommitRequestV1,
        ) -> Result<PreparedProductionRecallV1, PersistenceError> {
            let attempt = self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_first && attempt == 0 {
                return Err(PersistenceError::InvalidWipeableNamespace);
            }
            PreparedProductionRecallV1::seal(
                request.clone(),
                request.canonical_hash()?,
                canonical_production_recall_plan_hash_v1(&[], &[], &[])?,
                self.replayed,
            )
        }
    }

    struct AlteredReplayPlanner;

    impl ProductionRecallPlanner for AlteredReplayPlanner {
        async fn prepare(
            &self,
            request: &ProductionRecallCommitRequestV1,
        ) -> Result<PreparedProductionRecallV1, PersistenceError> {
            let mut altered = request.clone();
            altered.final_lifetime_ticks += 1;
            PreparedProductionRecallV1::seal(
                altered.clone(),
                altered.canonical_hash()?,
                canonical_production_recall_plan_hash_v1(&[], &[], &[])?,
                true,
            )
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum WriterMode {
        Fresh,
        Replay,
        FailOnce,
    }

    #[derive(Clone)]
    struct FakeWriter {
        mode: WriterMode,
        attempts: Arc<AtomicUsize>,
    }

    impl FakeWriter {
        fn new(mode: WriterMode) -> Self {
            Self {
                mode,
                attempts: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl ProductionRecallWriter for FakeWriter {
        async fn commit(
            &self,
            request: &ProductionRecallCommitRequestV1,
            expected_plan_hash: [u8; 32],
        ) -> Result<ProductionRecallTransactionV1, PersistenceError> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
            if matches!(self.mode, WriterMode::FailOnce) && attempt == 0 {
                return Err(PersistenceError::InvalidWipeableNamespace);
            }
            let result = stored_result(request, expected_plan_hash)?;
            Ok(if matches!(self.mode, WriterMode::Replay) {
                ProductionRecallTransactionV1::Replayed(result)
            } else {
                ProductionRecallTransactionV1::Fresh(result)
            })
        }
    }

    fn authenticated() -> AuthenticatedAccount {
        AuthenticatedAccount {
            account_id: AccountId::new([1; 16]).unwrap(),
            namespace: AuthenticatedNamespace::WipeableTest,
        }
    }

    fn binding() -> TerminalBinding {
        TerminalBinding::new([1; 16], [2; 16], [3; 16], [4; 16]).unwrap()
    }

    fn completion(server_tick: u64) -> ProductionRecallCompletionAuthorityV1 {
        ProductionRecallCompletionAuthorityV1 {
            account_id: [1; 16],
            character_id: [2; 16],
            instance_lineage_id: [3; 16],
            entry_restore_point_id: [4; 16],
            expected_versions: ProductionRecallExpectedVersionsV1 {
                account: 5,
                character: 6,
                world: 6,
                inventory: 7,
                life_metrics: 8,
                progression: 9,
                oath_bargain: 10,
                ash_wallet: 11,
            },
            content_revision: StoredWorldFlowRevisionV1 {
                records_blake3: "a".repeat(64),
                assets_blake3: "b".repeat(64),
                localization_blake3: "c".repeat(64),
            },
            server_tick,
            final_lifetime_ticks: 1_000 + server_tick,
            final_permadeath_combat_ticks: 800 + server_tick,
        }
    }

    fn frame(sequence: u32, client_tick: u64) -> RecallFrameV1 {
        RecallFrameV1 {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            sequence,
            character_id: [2; 16],
            client_tick,
            intent: RecallIntentV1::Start,
        }
    }

    fn actor() -> ProductionRecallIntentActor<FixedClock> {
        ProductionRecallIntentActor::new(
            FixedClock(50),
            [1; 16],
            [2; 16],
            ProductionRecallPendingAuthorityV1 {
                pending_item_count: 3,
                pending_material_stack_count: 1,
            },
        )
        .unwrap()
    }

    async fn start_explicit(actor: &ProductionRecallIntentActor<FixedClock>) {
        let pending = actor.handle(authenticated(), &frame(7, 9_999), 100).await;
        assert!(matches!(
            pending,
            RecallResultV1::Pending {
                started_tick: 100,
                completion_tick: 112,
                ..
            }
        ));
    }

    fn absent_others(
        completion: &ProductionRecallCompletionAuthorityV1,
    ) -> CoreTerminalOtherEvaluationsV1 {
        CoreTerminalOtherEvaluationsV1 {
            lethal: CoreTerminalEvaluation::absent(
                CoreTerminalProducer::LethalHealth,
                binding(),
                completion.server_tick,
                completion.expected_versions.character,
            ),
            extraction: CoreTerminalEvaluation::absent(
                CoreTerminalProducer::SuccessfulExtraction,
                binding(),
                completion.server_tick,
                completion.expected_versions.character,
            ),
            fault_restore: CoreTerminalEvaluation::absent(
                CoreTerminalProducer::VerifiedFaultRestoration,
                binding(),
                completion.server_tick,
                completion.expected_versions.character,
            ),
        }
    }

    fn lethal_others(
        completion: &ProductionRecallCompletionAuthorityV1,
    ) -> CoreTerminalOtherEvaluationsV1 {
        let lethal = TerminalCandidate::from_server_plan(
            binding(),
            [40; 16],
            [41; 16],
            [42; 32],
            [43; 32],
            completion.expected_versions.character,
            completion.server_tick,
            TerminalKind::LethalDeath,
        )
        .unwrap();
        let mut others = absent_others(completion);
        others.lethal = CoreTerminalEvaluation::candidate(
            CoreTerminalProducer::LethalHealth,
            binding(),
            completion.server_tick,
            completion.expected_versions.character,
            lethal,
        );
        others
    }

    fn stored_result(
        request: &ProductionRecallCommitRequestV1,
        plan_hash: [u8; 32],
    ) -> Result<StoredProductionRecallResultV1, PersistenceError> {
        let result = StoredProductionRecallResultV1 {
            contract_version: request.contract_version,
            namespace_id: request.namespace_id.clone(),
            account_id: request.account_id,
            character_id: request.character_id,
            mutation_id: request.mutation_id,
            terminal_id: request.terminal_id,
            canonical_request_hash: request.canonical_hash()?,
            canonical_plan_hash: plan_hash,
            result_code: 1,
            trigger: request.trigger,
            request_sequence: request.request_sequence,
            explicit_client_tick: request.explicit_client_tick,
            issued_at_unix_ms: request.issued_at_unix_ms,
            trigger_started_tick: request.trigger_started_tick,
            completion_tick: request.completion_tick,
            committed_at_unix_ms: request.issued_at_unix_ms + 1,
            source_content_id: "world.core_microrealm_01".into(),
            destination_content_id: persistence::PRODUCTION_RECALL_HALL_ID.into(),
            versions: ProductionRecallVersionsV1 {
                account: ProductionRecallVersionAdvanceV1 {
                    pre: request.expected_versions.account,
                    post: request.expected_versions.account,
                },
                character: ProductionRecallVersionAdvanceV1 {
                    pre: request.expected_versions.character,
                    post: request.expected_versions.character + 1,
                },
                world: ProductionRecallVersionAdvanceV1 {
                    pre: request.expected_versions.world,
                    post: request.expected_versions.world + 1,
                },
                inventory: ProductionRecallVersionAdvanceV1 {
                    pre: request.expected_versions.inventory,
                    post: request.expected_versions.inventory + 1,
                },
                life_metrics: ProductionRecallVersionAdvanceV1 {
                    pre: request.expected_versions.life_metrics,
                    post: request.expected_versions.life_metrics + 1,
                },
                progression: ProductionRecallVersionAdvanceV1 {
                    pre: request.expected_versions.progression,
                    post: request.expected_versions.progression,
                },
                oath_bargain: ProductionRecallVersionAdvanceV1 {
                    pre: request.expected_versions.oath_bargain,
                    post: request.expected_versions.oath_bargain,
                },
                ash_wallet: ProductionRecallVersionAdvanceV1 {
                    pre: request.expected_versions.ash_wallet,
                    post: request.expected_versions.ash_wallet,
                },
            },
            pre_lifetime_ticks: 1_000,
            post_lifetime_ticks: request.final_lifetime_ticks,
            pre_permadeath_combat_ticks: 800,
            post_permadeath_combat_ticks: request.final_permadeath_combat_ticks,
            stabilized_items: Vec::new(),
            destroyed_items: Vec::new(),
            destroyed_materials: Vec::new(),
        };
        result.validate()?;
        Ok(result)
    }

    #[test]
    fn terminal_snapshot_hash_binds_authority_clocks_content_and_other_candidates() {
        let base = completion(112);
        let absent = absent_others(&base);
        let expected = terminal_snapshot_hash(&base, &absent);

        let mut lineage = base.clone();
        lineage.instance_lineage_id = [30; 16];
        let mut version = base.clone();
        version.expected_versions.inventory += 1;
        let mut content = base.clone();
        content.content_revision.records_blake3 = "d".repeat(64);
        let mut clocks = base.clone();
        clocks.final_lifetime_ticks += 1;
        for changed in [lineage, version, content, clocks] {
            assert_ne!(terminal_snapshot_hash(&changed, &absent), expected);
        }
        assert_ne!(
            terminal_snapshot_hash(&base, &lethal_others(&base)),
            expected
        );
    }

    #[tokio::test]
    async fn explicit_tick_twelve_commits_once_and_replays_the_stored_publication() {
        let actor = actor();
        start_explicit(&actor).await;
        let completion = completion(112);
        let planner = FakePlanner::fresh();
        let writer = FakeWriter::new(WriterMode::Fresh);
        let executor = ProductionRecallExecutionService::new(writer.clone());
        let mut coordinator = CoreTerminalCoordinator::new(authenticated(), binding()).unwrap();

        let outcome = drive_recall_terminal_tick(
            &actor,
            &mut coordinator,
            &planner,
            &executor,
            &completion,
            absent_others(&completion),
        )
        .await
        .unwrap();
        let CoreRecallTerminalTickOutcome::RecallStored(published) = outcome else {
            panic!("explicit Recall must commit");
        };
        assert_eq!(writer.attempts.load(Ordering::SeqCst), 1);
        assert_eq!(planner.calls.load(Ordering::SeqCst), 1);
        assert!(coordinator.committed_receipt().is_some());
        assert_eq!(actor.pinned_terminal_tick().await, Some(112));
        assert_eq!(actor.published_recall().await, Some(published.clone()));
        assert!(matches!(
            published.result,
            RecallResultV1::Stored {
                request_sequence: Some(7),
                replayed: false,
                ref result,
                ..
            } if result.trigger == RecallTerminalTriggerV1::Explicit
        ));
        assert!(matches!(
            published.hall.location,
            CharacterLocation::Safe {
                arrival: SafeArrival::HallDefault,
                ..
            }
        ));

        let replayed = actor.handle(authenticated(), &frame(7, 9_999), 113).await;
        assert!(matches!(
            replayed,
            RecallResultV1::Stored {
                request_sequence: Some(7),
                replayed: true,
                ref result,
                ..
            } if matches!(
                &published.result,
                RecallResultV1::Stored {
                    result: original,
                    ..
                } if original == result
            )
        ));
        assert!(matches!(
            actor.handle(authenticated(), &frame(7, 10_000), 113).await,
            RecallResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::IdempotencyConflict,
                ..
            }
        ));

        assert!(matches!(
            drive_recall_terminal_tick(
                &actor,
                &mut coordinator,
                &planner,
                &executor,
                &completion,
                lethal_others(&completion),
            )
            .await
            .unwrap(),
            CoreRecallTerminalTickOutcome::RecallReplayed(ProductionRecallPublishedV1 {
                result: RecallResultV1::Stored { replayed: true, .. },
                ..
            })
        ));
    }

    #[tokio::test]
    async fn actor_publication_cannot_pair_with_a_committed_death_receipt() {
        let actor = actor();
        start_explicit(&actor).await;
        let due = completion(112);
        let writer = FakeWriter::new(WriterMode::Fresh);
        let executor = ProductionRecallExecutionService::new(writer);
        let planner = FakePlanner::fresh();
        let mut coordinator = CoreTerminalCoordinator::new(authenticated(), binding()).unwrap();
        drive_recall_terminal_tick(
            &actor,
            &mut coordinator,
            &planner,
            &executor,
            &due,
            absent_others(&due),
        )
        .await
        .unwrap();

        let lethal = TerminalCandidate::from_server_plan(
            binding(),
            [70; 16],
            [71; 16],
            [72; 32],
            [73; 32],
            due.expected_versions.character,
            due.server_tick,
            TerminalKind::LethalDeath,
        )
        .unwrap();
        let mut arbiter = TerminalArbiter::new(binding());
        assert!(matches!(
            arbiter.submit(lethal),
            SubmitResult::Accepted { .. }
        ));
        let prepared = arbiter.prepare(due.server_tick).unwrap();
        let receipt =
            StoredTerminalReceipt::from_prepared(&prepared, due.server_tick, [74; 32]).unwrap();
        let mut wrong_coordinator =
            CoreTerminalCoordinator::from_stored_receipt(authenticated(), receipt).unwrap();

        assert!(matches!(
            drive_recall_terminal_tick(
                &actor,
                &mut wrong_coordinator,
                &planner,
                &executor,
                &due,
                absent_others(&due),
            )
            .await,
            Err(CoreRecallTerminalDriverError::Execution(
                ProductionRecallExecutionError::PublishedReceiptMismatch
            ))
        ));
    }

    #[tokio::test]
    async fn lethal_death_wins_same_tick_without_touching_recall_writer() {
        let actor = actor();
        start_explicit(&actor).await;
        let completion = completion(112);
        let writer = FakeWriter::new(WriterMode::Fresh);
        let executor = ProductionRecallExecutionService::new(writer.clone());
        let mut coordinator = CoreTerminalCoordinator::new(authenticated(), binding()).unwrap();

        let outcome = drive_recall_terminal_tick(
            &actor,
            &mut coordinator,
            &FakePlanner::fresh(),
            &executor,
            &completion,
            lethal_others(&completion),
        )
        .await
        .unwrap();
        let CoreRecallTerminalTickOutcome::OtherTerminalPrepared(prepared) = outcome else {
            panic!("lethal death must own the exact completion tick");
        };
        assert_eq!(prepared.winner().kind(), TerminalKind::LethalDeath);
        assert_eq!(coordinator.prepared_terminal(), Some(&prepared));
        assert_eq!(writer.attempts.load(Ordering::SeqCst), 0);
        assert_eq!(actor.pinned_terminal_tick().await, Some(112));
        assert!(actor.published_recall().await.is_none());
    }

    #[tokio::test]
    async fn planner_outage_cannot_erase_a_same_tick_lethal_snapshot() {
        let actor = actor();
        start_explicit(&actor).await;
        let due = completion(112);
        let planner = FakePlanner::fail_once();
        let writer = FakeWriter::new(WriterMode::Fresh);
        let executor = ProductionRecallExecutionService::new(writer.clone());
        let mut coordinator = CoreTerminalCoordinator::new(authenticated(), binding()).unwrap();

        assert!(matches!(
            drive_recall_terminal_tick(
                &actor,
                &mut coordinator,
                &planner,
                &executor,
                &due,
                lethal_others(&due),
            )
            .await,
            Err(CoreRecallTerminalDriverError::Channel(
                ProductionRecallChannelError::Persistence(
                    PersistenceError::InvalidWipeableNamespace
                )
            ))
        ));
        assert!(coordinator.prepared_terminal().is_none());
        assert_eq!(actor.pinned_terminal_tick().await, Some(112));

        assert!(matches!(
            drive_recall_terminal_tick(
                &actor,
                &mut coordinator,
                &planner,
                &executor,
                &due,
                absent_others(&due),
            )
            .await,
            Err(CoreRecallTerminalDriverError::Channel(
                ProductionRecallChannelError::PinnedSnapshotMismatch
            ))
        ));
        assert_eq!(planner.calls.load(Ordering::SeqCst), 1);
        assert_eq!(writer.attempts.load(Ordering::SeqCst), 0);

        let outcome = drive_recall_terminal_tick(
            &actor,
            &mut coordinator,
            &planner,
            &executor,
            &due,
            lethal_others(&due),
        )
        .await
        .unwrap();
        let CoreRecallTerminalTickOutcome::OtherTerminalPrepared(prepared) = outcome else {
            panic!("the frozen lethal candidate must remain the winner");
        };
        assert_eq!(prepared.winner().kind(), TerminalKind::LethalDeath);
        assert_eq!(writer.attempts.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn commit_outage_retries_the_same_pinned_tick_without_replanning() {
        let actor = actor();
        start_explicit(&actor).await;
        let due_completion = completion(112);
        let planner = FakePlanner::fresh();
        let writer = FakeWriter::new(WriterMode::FailOnce);
        let executor = ProductionRecallExecutionService::new(writer.clone());
        let mut coordinator = CoreTerminalCoordinator::new(authenticated(), binding()).unwrap();

        assert!(matches!(
            drive_recall_terminal_tick(
                &actor,
                &mut coordinator,
                &planner,
                &executor,
                &due_completion,
                absent_others(&due_completion),
            )
            .await,
            Err(CoreRecallTerminalDriverError::Execution(
                ProductionRecallExecutionError::Persistence(
                    PersistenceError::InvalidWipeableNamespace
                )
            ))
        ));
        assert!(coordinator.prepared_terminal().is_some());
        assert_eq!(actor.pinned_terminal_tick().await, Some(112));
        assert_eq!(planner.calls.load(Ordering::SeqCst), 1);

        let advanced = completion(113);
        assert!(matches!(
            drive_recall_terminal_tick(
                &actor,
                &mut coordinator,
                &planner,
                &executor,
                &advanced,
                absent_others(&advanced),
            )
            .await,
            Err(CoreRecallTerminalDriverError::InvalidProducerBundle)
        ));

        assert!(matches!(
            drive_recall_terminal_tick(
                &actor,
                &mut coordinator,
                &planner,
                &executor,
                &due_completion,
                absent_others(&due_completion),
            )
            .await
            .unwrap(),
            CoreRecallTerminalTickOutcome::RecallStored(_)
        ));
        assert_eq!(writer.attempts.load(Ordering::SeqCst), 2);
        assert_eq!(planner.calls.load(Ordering::SeqCst), 1);
        assert!(coordinator.committed_receipt().is_some());
    }

    #[tokio::test]
    async fn link_lost_tick_ninety_commits_after_tick_eighty_nine_absence() {
        let actor = actor();
        actor.enter_link_lost(200, 70).await.unwrap();
        let planner = FakePlanner::fresh();
        let writer = FakeWriter::new(WriterMode::Fresh);
        let executor = ProductionRecallExecutionService::new(writer);
        let mut coordinator = CoreTerminalCoordinator::new(authenticated(), binding()).unwrap();

        let early = completion(289);
        assert_eq!(
            drive_recall_terminal_tick(
                &actor,
                &mut coordinator,
                &planner,
                &executor,
                &early,
                absent_others(&early),
            )
            .await
            .unwrap(),
            CoreRecallTerminalTickOutcome::NoTerminal
        );
        assert_eq!(actor.pinned_terminal_tick().await, None);

        let due = completion(290);
        let outcome = drive_recall_terminal_tick(
            &actor,
            &mut coordinator,
            &planner,
            &executor,
            &due,
            absent_others(&due),
        )
        .await
        .unwrap();
        let CoreRecallTerminalTickOutcome::RecallStored(published) = outcome else {
            panic!("LinkLost deadline must commit disconnect recovery");
        };
        assert!(matches!(
            published.result,
            RecallResultV1::Stored {
                request_sequence: None,
                ref result,
                ..
            } if result.trigger == RecallTerminalTriggerV1::LinkLost
        ));
        assert_eq!(actor.pinned_terminal_tick().await, Some(290));
    }

    #[tokio::test]
    async fn lethal_death_wins_the_exact_link_lost_deadline() {
        let actor = actor();
        actor.enter_link_lost(200, 70).await.unwrap();
        let due = completion(290);
        let writer = FakeWriter::new(WriterMode::Fresh);
        let executor = ProductionRecallExecutionService::new(writer.clone());
        let mut coordinator = CoreTerminalCoordinator::new(authenticated(), binding()).unwrap();

        let outcome = drive_recall_terminal_tick(
            &actor,
            &mut coordinator,
            &FakePlanner::fresh(),
            &executor,
            &due,
            lethal_others(&due),
        )
        .await
        .unwrap();
        let CoreRecallTerminalTickOutcome::OtherTerminalPrepared(prepared) = outcome else {
            panic!("lethal death must beat automatic Recall at tick ninety");
        };
        assert_eq!(prepared.winner().kind(), TerminalKind::LethalDeath);
        assert_eq!(writer.attempts.load(Ordering::SeqCst), 0);
        assert!(actor.published_recall().await.is_none());
    }

    #[tokio::test]
    async fn committed_prepare_replay_bypasses_a_new_producer_barrier() {
        let actor = actor();
        start_explicit(&actor).await;
        let completion = completion(112);
        let planner = FakePlanner::replayed();
        let writer = FakeWriter::new(WriterMode::Replay);
        let executor = ProductionRecallExecutionService::new(writer.clone());
        let mut coordinator = CoreTerminalCoordinator::new(authenticated(), binding()).unwrap();

        assert!(matches!(
            drive_recall_terminal_tick(
                &actor,
                &mut coordinator,
                &planner,
                &executor,
                &completion,
                lethal_others(&completion),
            )
            .await
            .unwrap(),
            CoreRecallTerminalTickOutcome::RecallReplayed(_)
        ));
        assert!(coordinator.committed_receipt().is_some());
        assert!(coordinator.barrier_progress().is_none());
        assert_eq!(planner.calls.load(Ordering::SeqCst), 1);
        assert_eq!(writer.attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn historical_replay_cannot_replace_a_newer_live_lineage() {
        let actor = actor();
        start_explicit(&actor).await;
        let completion = completion(112);
        let newer_binding = TerminalBinding::new([1; 16], [2; 16], [30; 16], [31; 16]).unwrap();
        let mut coordinator = CoreTerminalCoordinator::new(authenticated(), newer_binding).unwrap();
        let planner = FakePlanner::replayed();
        let writer = FakeWriter::new(WriterMode::Replay);
        let executor = ProductionRecallExecutionService::new(writer.clone());

        assert!(matches!(
            drive_recall_terminal_tick(
                &actor,
                &mut coordinator,
                &planner,
                &executor,
                &completion,
                absent_others(&completion),
            )
            .await,
            Err(CoreRecallTerminalDriverError::InvalidProducerBundle)
        ));
        assert_eq!(coordinator.binding(), newer_binding);
        assert!(coordinator.committed_receipt().is_none());
        assert!(actor.published_recall().await.is_none());
        assert_eq!(planner.calls.load(Ordering::SeqCst), 0);
        assert_eq!(writer.attempts.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn altered_replayed_planner_output_is_rejected_before_writer_or_barrier() {
        let actor = actor();
        start_explicit(&actor).await;
        let completion = completion(112);
        let writer = FakeWriter::new(WriterMode::Replay);
        let executor = ProductionRecallExecutionService::new(writer.clone());
        let mut coordinator = CoreTerminalCoordinator::new(authenticated(), binding()).unwrap();

        assert!(matches!(
            drive_recall_terminal_tick(
                &actor,
                &mut coordinator,
                &AlteredReplayPlanner,
                &executor,
                &completion,
                absent_others(&completion),
            )
            .await,
            Err(CoreRecallTerminalDriverError::Channel(
                ProductionRecallChannelError::InvalidPreparedAuthority
            ))
        ));
        assert!(coordinator.barrier_progress().is_none());
        assert!(coordinator.prepared_terminal().is_none());
        assert_eq!(writer.attempts.load(Ordering::SeqCst), 0);
    }
}
