//! Arbiter-gated execution of one sealed `GB-M03-08` successful extraction.
//!
//! Authorities: `Gravebound_Production_GDD_v1_Canonical.md` (`DTH-011`, `LOOT-002`,
//! `TECH-015`, and `TECH-021`-`023`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-BOSS-001`, `CONT-HUB-001`/`002`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`/`08`). Accepted
//! `SPEC-CONFLICT-029` requires a complete server-planned placement hash before the shared
//! five-producer terminal arbiter seals its winner. This service is the only bridge from that
//! frozen winner to the production extraction writer.

use std::future::Future;

use persistence::{
    PersistenceError, PostgresPersistence, PreparedProductionExtractionV1,
    ProductionExtractionCommitRequestV1, ProductionExtractionTransactionV1,
    StoredProductionExtractionResultV1,
};
use thiserror::Error;

use crate::{
    CommitError, CommitResult, CoreTerminalCoordinator, PreparedTerminal,
    STORED_TERMINAL_RECEIPT_SCHEMA_V1, StoredTerminalReceipt, StoredTerminalReceiptV1,
    TerminalArbiter, TerminalBinding, TerminalCandidate, TerminalKind, TerminalValidationError,
};

/// Narrow writer seam so terminal arbitration and result validation remain independently testable.
pub trait ProductionExtractionWriter: Send + Sync {
    fn commit(
        &self,
        request: &ProductionExtractionCommitRequestV1,
        expected_plan_hash: [u8; 32],
    ) -> impl Future<Output = Result<ProductionExtractionTransactionV1, PersistenceError>> + Send;
}

impl ProductionExtractionWriter for PostgresPersistence {
    async fn commit(
        &self,
        request: &ProductionExtractionCommitRequestV1,
        expected_plan_hash: [u8; 32],
    ) -> Result<ProductionExtractionTransactionV1, PersistenceError> {
        self.commit_production_extraction_v1(request, expected_plan_hash)
            .await
    }
}

#[derive(Debug, Clone)]
pub struct ProductionExtractionExecutionService<Writer> {
    writer: Writer,
}

impl<Writer> ProductionExtractionExecutionService<Writer> {
    #[must_use]
    pub const fn new(writer: Writer) -> Self {
        Self { writer }
    }
}

pub type PostgresProductionExtractionExecutionService =
    ProductionExtractionExecutionService<PostgresPersistence>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductionExtractionExecutionOutcome {
    pub transaction: ProductionExtractionTransactionV1,
    pub terminal_commit: CommitResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductionExtractionReplayOutcome {
    pub transaction: ProductionExtractionTransactionV1,
    pub receipt: StoredTerminalReceipt,
}

#[derive(Debug, Error)]
pub enum ProductionExtractionExecutionError {
    #[error("sealed extraction could not form terminal authority: {0:?}")]
    InvalidTerminalAuthority(TerminalValidationError),
    #[error("prepared extraction authority is invalid")]
    InvalidPreparedAuthority(#[source] PersistenceError),
    #[error("an already committed extraction must use durable replay recovery")]
    CommittedReplayRequiresRecovery,
    #[error("durable replay expected the repository to return the stored extraction")]
    ExpectedStoredReplay,
    #[error("prepared terminal winner is not the sealed successful extraction")]
    PreparedWinnerMismatch,
    #[error("production extraction repository rejected the transaction")]
    Persistence(#[source] PersistenceError),
    #[error("production extraction identity conflicted with a stored terminal")]
    RepositoryConflict,
    #[error("stored extraction result is corrupt or does not match the sealed request")]
    StoredResultMismatch,
    #[error("terminal receipt could not be published: {0:?}")]
    TerminalCommit(CommitError),
}

impl<Writer> ProductionExtractionExecutionService<Writer>
where
    Writer: ProductionExtractionWriter,
{
    /// Production-facing composition through the complete five-producer coordinator.
    pub async fn execute_coordinated(
        &self,
        coordinator: &mut CoreTerminalCoordinator,
        prepared_terminal: &PreparedTerminal,
        extraction: &PreparedProductionExtractionV1,
    ) -> Result<ProductionExtractionExecutionOutcome, ProductionExtractionExecutionError> {
        self.execute_prepared(
            coordinator.terminal_arbiter_mut(),
            prepared_terminal,
            extraction,
        )
        .await
    }

    /// Executes only the frozen successful-extraction winner.
    ///
    /// Repository failure or plan drift leaves the arbiter prepared, so departure remains blocked
    /// and the exact intent can be retried or recovered without publishing a Hall arrival.
    pub async fn execute_prepared(
        &self,
        arbiter: &mut TerminalArbiter,
        prepared_terminal: &PreparedTerminal,
        extraction: &PreparedProductionExtractionV1,
    ) -> Result<ProductionExtractionExecutionOutcome, ProductionExtractionExecutionError> {
        let candidate = production_extraction_terminal_candidate(extraction)?;
        if arbiter.prepared_terminal() != Some(prepared_terminal)
            || prepared_terminal.winner() != &candidate
            || candidate.kind() != TerminalKind::SuccessfulExtraction
        {
            return Err(ProductionExtractionExecutionError::PreparedWinnerMismatch);
        }

        let transaction = self
            .writer
            .commit(extraction.request(), extraction.canonical_plan_hash())
            .await
            .map_err(ProductionExtractionExecutionError::Persistence)?;
        let result = transaction
            .result()
            .ok_or(ProductionExtractionExecutionError::RepositoryConflict)?;
        validate_stored_result_intent(extraction, result)?;
        let receipt = committed_extraction_terminal_receipt(extraction, result)?;
        let terminal_commit = arbiter
            .record_commit(receipt)
            .map_err(ProductionExtractionExecutionError::TerminalCommit)?;
        Ok(ProductionExtractionExecutionOutcome {
            transaction,
            terminal_commit,
        })
    }

    /// Returns an already committed extraction without submitting its historical tick to a new
    /// producer barrier. Callers reconstruct the coordinator from the returned durable receipt.
    pub async fn replay_committed(
        &self,
        extraction: &PreparedProductionExtractionV1,
    ) -> Result<ProductionExtractionReplayOutcome, ProductionExtractionExecutionError> {
        extraction
            .validate()
            .map_err(ProductionExtractionExecutionError::InvalidPreparedAuthority)?;
        if !extraction.replayed() {
            return Err(ProductionExtractionExecutionError::ExpectedStoredReplay);
        }
        let transaction = self
            .writer
            .commit(extraction.request(), extraction.canonical_plan_hash())
            .await
            .map_err(ProductionExtractionExecutionError::Persistence)?;
        if !transaction.is_replay() {
            return Err(ProductionExtractionExecutionError::ExpectedStoredReplay);
        }
        let receipt = {
            let result = transaction
                .result()
                .ok_or(ProductionExtractionExecutionError::RepositoryConflict)?;
            committed_extraction_terminal_receipt(extraction, result)?
        };
        Ok(ProductionExtractionReplayOutcome {
            transaction,
            receipt,
        })
    }
}

/// Converts one repository-prepared extraction into the opaque shared terminal candidate.
pub fn production_extraction_terminal_candidate(
    extraction: &PreparedProductionExtractionV1,
) -> Result<TerminalCandidate, ProductionExtractionExecutionError> {
    extraction
        .validate()
        .map_err(ProductionExtractionExecutionError::InvalidPreparedAuthority)?;
    if extraction.replayed() {
        return Err(ProductionExtractionExecutionError::CommittedReplayRequiresRecovery);
    }
    let request = extraction.request();
    let binding = TerminalBinding::new(
        request.account_id,
        request.character_id,
        request.instance_lineage_id,
        request.entry_restore_point_id,
    )
    .map_err(ProductionExtractionExecutionError::InvalidTerminalAuthority)?;
    TerminalCandidate::from_server_plan(
        binding,
        request.terminal_id,
        request.mutation_id,
        extraction.canonical_request_hash(),
        extraction.canonical_plan_hash(),
        request.expected_versions.character,
        request.observed_tick,
        TerminalKind::SuccessfulExtraction,
    )
    .map_err(ProductionExtractionExecutionError::InvalidTerminalAuthority)
}

/// Reconstructs the exact shared-terminal receipt from the committed extraction graph.
pub fn committed_extraction_terminal_receipt(
    extraction: &PreparedProductionExtractionV1,
    result: &StoredProductionExtractionResultV1,
) -> Result<StoredTerminalReceipt, ProductionExtractionExecutionError> {
    validate_stored_result_intent(extraction, result)?;
    let request = extraction.request();
    let result_hash = result
        .digest()
        .map_err(|_| ProductionExtractionExecutionError::StoredResultMismatch)?;
    StoredTerminalReceipt::from_storage(&StoredTerminalReceiptV1 {
        schema_version: STORED_TERMINAL_RECEIPT_SCHEMA_V1,
        account_id: result.account_id,
        character_id: result.character_id,
        lineage_id: request.instance_lineage_id,
        restore_point_id: request.entry_restore_point_id,
        terminal_id: result.terminal_id,
        mutation_id: result.mutation_id,
        payload_hash: result.canonical_request_hash,
        server_plan_hash: result.canonical_plan_hash,
        result_hash,
        expected_state_version: result.versions.character.pre,
        post_state_version: result.versions.character.post,
        observed_tick: result.observed_tick,
        committed_tick: result.observed_tick,
        terminal_kind_code: TerminalKind::SuccessfulExtraction.stable_code(),
    })
    .map_err(|_| ProductionExtractionExecutionError::StoredResultMismatch)
}

fn validate_stored_result_intent(
    extraction: &PreparedProductionExtractionV1,
    result: &StoredProductionExtractionResultV1,
) -> Result<(), ProductionExtractionExecutionError> {
    extraction
        .validate()
        .map_err(ProductionExtractionExecutionError::InvalidPreparedAuthority)?;
    result
        .validate()
        .map_err(|_| ProductionExtractionExecutionError::StoredResultMismatch)?;
    let request = extraction.request();
    if result.namespace_id != request.namespace_id
        || result.account_id != request.account_id
        || result.character_id != request.character_id
        || result.mutation_id != request.mutation_id
        || result.terminal_id != request.terminal_id
        || result.extraction_request_id != request.extraction_request_id
        || result.extraction_receipt_id != request.extraction_receipt_id
        || result.canonical_request_hash != extraction.canonical_request_hash()
        || result.canonical_plan_hash != extraction.canonical_plan_hash()
        || result.issued_at_unix_ms != request.issued_at_unix_ms
        || result.observed_tick != request.observed_tick
        || result.versions.account.pre != request.expected_versions.account
        || result.versions.character.pre != request.expected_versions.character
        || result.versions.world.pre != request.expected_versions.world
        || result.versions.inventory.pre != request.expected_versions.inventory
        || result.versions.life_metrics.pre != request.expected_versions.life_metrics
    {
        return Err(ProductionExtractionExecutionError::StoredResultMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use persistence::{
        PRODUCTION_EXTRACTION_CONTRACT_VERSION_V1, ProductionExtractionExpectedVersionsV1,
        ProductionExtractionVersionAdvanceV1, ProductionExtractionVersionsV1,
        StoredExtractionLocationV1, StoredProductionExtractionPlacementV1,
        StoredWorldFlowRevisionV1, WIPEABLE_CORE_NAMESPACE,
        canonical_production_extraction_plan_hash_v1,
    };

    use super::*;
    use crate::{NonTerminalAdmission, SubmitResult};

    #[derive(Debug, Clone, Copy)]
    enum FakeMode {
        Fresh,
        Replay,
        Conflict,
        ForeignResult,
        Unavailable,
    }

    #[derive(Debug, Clone)]
    struct FakeWriter {
        mode: FakeMode,
        calls: Arc<AtomicUsize>,
        expected_plan_hashes: Arc<Mutex<Vec<[u8; 32]>>>,
    }

    impl FakeWriter {
        fn new(mode: FakeMode) -> Self {
            Self {
                mode,
                calls: Arc::new(AtomicUsize::new(0)),
                expected_plan_hashes: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl ProductionExtractionWriter for FakeWriter {
        async fn commit(
            &self,
            request: &ProductionExtractionCommitRequestV1,
            expected_plan_hash: [u8; 32],
        ) -> Result<ProductionExtractionTransactionV1, PersistenceError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.expected_plan_hashes
                .lock()
                .expect("plan hash recorder")
                .push(expected_plan_hash);
            if matches!(self.mode, FakeMode::Unavailable) {
                return Err(PersistenceError::ProductionExtractionTerminalSuperseded);
            }
            if matches!(self.mode, FakeMode::Conflict) {
                return Ok(ProductionExtractionTransactionV1::Conflict {
                    extraction_request_id: request.extraction_request_id,
                    terminal_id: request.terminal_id,
                });
            }
            let extraction = prepared_extraction();
            let mut result = stored_result(&extraction);
            if matches!(self.mode, FakeMode::ForeignResult) {
                result.account_id = [99; 16];
            }
            Ok(if matches!(self.mode, FakeMode::Replay) {
                ProductionExtractionTransactionV1::Replayed(result)
            } else {
                ProductionExtractionTransactionV1::Fresh(result)
            })
        }
    }

    fn request() -> ProductionExtractionCommitRequestV1 {
        ProductionExtractionCommitRequestV1 {
            contract_version: PRODUCTION_EXTRACTION_CONTRACT_VERSION_V1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            account_id: [1; 16],
            character_id: [2; 16],
            mutation_id: [3; 16],
            terminal_id: [4; 16],
            extraction_request_id: [5; 16],
            extraction_receipt_id: [6; 16],
            encounter_id: [7; 16],
            instance_lineage_id: [8; 16],
            entry_restore_point_id: [9; 16],
            exit_instance_id: [10; 16],
            expected_versions: ProductionExtractionExpectedVersionsV1 {
                account: 11,
                character: 12,
                world: 12,
                inventory: 13,
                life_metrics: 14,
            },
            content_revision: StoredWorldFlowRevisionV1 {
                records_blake3: "a".repeat(64),
                assets_blake3: "b".repeat(64),
                localization_blake3: "c".repeat(64),
            },
            issued_at_unix_ms: 15,
            observed_tick: 16,
        }
    }

    fn placements() -> Vec<StoredProductionExtractionPlacementV1> {
        vec![StoredProductionExtractionPlacementV1 {
            ordinal: 0,
            item_uid: [17; 16],
            template_id: "item.weapon.test".into(),
            item_kind: 0,
            source: StoredExtractionLocationV1::Equipped(0),
            destination: StoredExtractionLocationV1::Equipped(0),
            pre_item_version: 1,
            post_item_version: 2,
            ledger_event_id: [18; 16],
        }]
    }

    fn prepared_extraction_with_replay(replayed: bool) -> PreparedProductionExtractionV1 {
        let request = request();
        let request_hash = request.canonical_hash().unwrap();
        let plan_hash = canonical_production_extraction_plan_hash_v1(&placements(), &[]).unwrap();
        PreparedProductionExtractionV1::seal(request, request_hash, plan_hash, replayed).unwrap()
    }

    fn prepared_extraction() -> PreparedProductionExtractionV1 {
        prepared_extraction_with_replay(false)
    }

    fn stored_result(
        extraction: &PreparedProductionExtractionV1,
    ) -> StoredProductionExtractionResultV1 {
        let request = extraction.request();
        StoredProductionExtractionResultV1 {
            contract_version: request.contract_version,
            namespace_id: request.namespace_id.clone(),
            account_id: request.account_id,
            character_id: request.character_id,
            mutation_id: request.mutation_id,
            terminal_id: request.terminal_id,
            extraction_request_id: request.extraction_request_id,
            extraction_receipt_id: request.extraction_receipt_id,
            canonical_request_hash: extraction.canonical_request_hash(),
            canonical_plan_hash: extraction.canonical_plan_hash(),
            result_code: 1,
            issued_at_unix_ms: request.issued_at_unix_ms,
            observed_tick: request.observed_tick,
            committed_at_unix_ms: request.issued_at_unix_ms + 1,
            destination_content_id: persistence::PRODUCTION_EXTRACTION_HALL_ID.into(),
            versions: ProductionExtractionVersionsV1 {
                account: ProductionExtractionVersionAdvanceV1 { pre: 11, post: 11 },
                character: ProductionExtractionVersionAdvanceV1 { pre: 12, post: 13 },
                world: ProductionExtractionVersionAdvanceV1 { pre: 12, post: 13 },
                inventory: ProductionExtractionVersionAdvanceV1 { pre: 13, post: 14 },
                life_metrics: ProductionExtractionVersionAdvanceV1 { pre: 14, post: 15 },
            },
            placements: placements(),
            material_credits: Vec::new(),
            storage_resolution_required: false,
        }
    }

    fn prepared_arbiter(
        extraction: &PreparedProductionExtractionV1,
    ) -> (TerminalArbiter, PreparedTerminal) {
        let candidate =
            production_extraction_terminal_candidate(extraction).expect("terminal candidate");
        let mut arbiter = TerminalArbiter::new(candidate.binding());
        assert!(matches!(
            arbiter.submit(candidate),
            SubmitResult::Accepted { .. }
        ));
        let prepared = arbiter
            .prepare(extraction.request().observed_tick)
            .expect("sealed extraction tick");
        (arbiter, prepared)
    }

    #[test]
    fn candidate_binds_complete_repository_preparation() {
        let extraction = prepared_extraction();
        let candidate = production_extraction_terminal_candidate(&extraction).unwrap();
        let request = extraction.request();
        assert_eq!(candidate.binding().account_id(), &request.account_id);
        assert_eq!(candidate.binding().character_id(), &request.character_id);
        assert_eq!(
            candidate.binding().lineage_id(),
            &request.instance_lineage_id
        );
        assert_eq!(
            candidate.binding().restore_point_id(),
            &request.entry_restore_point_id
        );
        assert_eq!(candidate.terminal_id(), &request.terminal_id);
        assert_eq!(candidate.mutation_id(), &request.mutation_id);
        assert_eq!(
            candidate.payload_hash(),
            &extraction.canonical_request_hash()
        );
        assert_eq!(
            candidate.server_plan_hash(),
            &extraction.canonical_plan_hash()
        );
        assert_eq!(
            candidate.expected_state_version(),
            request.expected_versions.character
        );
        assert_eq!(candidate.observed_tick(), request.observed_tick);
        assert_eq!(candidate.kind(), TerminalKind::SuccessfulExtraction);
    }

    #[test]
    fn committed_replay_cannot_reenter_a_new_terminal_tick() {
        let extraction = prepared_extraction_with_replay(true);
        assert!(matches!(
            production_extraction_terminal_candidate(&extraction),
            Err(ProductionExtractionExecutionError::CommittedReplayRequiresRecovery)
        ));
    }

    #[tokio::test]
    async fn fresh_and_replayed_commits_publish_restart_safe_receipts() {
        for mode in [FakeMode::Fresh, FakeMode::Replay] {
            let extraction = prepared_extraction();
            let (mut arbiter, prepared) = prepared_arbiter(&extraction);
            let writer = FakeWriter::new(mode);
            let service = ProductionExtractionExecutionService::new(writer.clone());
            let outcome = service
                .execute_prepared(&mut arbiter, &prepared, &extraction)
                .await
                .unwrap();

            assert_eq!(
                outcome.transaction.is_replay(),
                matches!(mode, FakeMode::Replay)
            );
            assert_eq!(writer.calls(), 1);
            assert_eq!(
                writer
                    .expected_plan_hashes
                    .lock()
                    .expect("plan hashes")
                    .as_slice(),
                &[extraction.canonical_plan_hash()]
            );
            assert!(matches!(
                outcome.terminal_commit,
                CommitResult::Committed(_)
            ));
            assert_eq!(
                arbiter.non_terminal_admission(),
                NonTerminalAdmission::BlockedByCommittedTerminal
            );
            let receipt = arbiter.committed_receipt().unwrap();
            assert_eq!(
                receipt.expected_state_version(),
                extraction.request().expected_versions.character
            );
            assert_eq!(
                receipt.post_state_version(),
                extraction.request().expected_versions.character + 1
            );
            assert_eq!(receipt.committed_tick(), extraction.request().observed_tick);
            assert_eq!(
                StoredTerminalReceipt::from_storage(&receipt.to_storage_v1()).unwrap(),
                *receipt
            );
        }
    }

    #[tokio::test]
    async fn committed_replay_returns_a_receipt_without_an_arbiter_submission() {
        let extraction = prepared_extraction_with_replay(true);
        let writer = FakeWriter::new(FakeMode::Replay);
        let service = ProductionExtractionExecutionService::new(writer.clone());
        let replay = service.replay_committed(&extraction).await.unwrap();

        assert!(replay.transaction.is_replay());
        assert_eq!(writer.calls(), 1);
        assert_eq!(
            replay.receipt.expected_state_version(),
            extraction.request().expected_versions.character
        );
        assert_eq!(
            replay.receipt.post_state_version(),
            extraction.request().expected_versions.character + 1
        );
        assert_eq!(replay.receipt.kind(), TerminalKind::SuccessfulExtraction);
        let recovered = TerminalArbiter::from_stored_receipt(replay.receipt.clone())
            .expect("recovered arbiter");
        assert_eq!(
            recovered.non_terminal_admission(),
            NonTerminalAdmission::BlockedByCommittedTerminal
        );
    }

    #[tokio::test]
    async fn lethal_winner_and_foreign_prepared_state_never_reach_the_writer() {
        let extraction = prepared_extraction();
        let extraction_candidate =
            production_extraction_terminal_candidate(&extraction).expect("candidate");
        let lethal = TerminalCandidate::from_server_plan(
            extraction_candidate.binding(),
            [30; 16],
            [31; 16],
            [32; 32],
            [33; 32],
            extraction_candidate.expected_state_version(),
            extraction_candidate.observed_tick(),
            TerminalKind::LethalDeath,
        )
        .unwrap();
        let mut arbiter = TerminalArbiter::new(extraction_candidate.binding());
        assert!(matches!(
            arbiter.submit(extraction_candidate.clone()),
            SubmitResult::Accepted { .. }
        ));
        assert!(matches!(
            arbiter.submit(lethal),
            SubmitResult::Accepted { .. }
        ));
        let prepared = arbiter.prepare(extraction.request().observed_tick).unwrap();
        assert_eq!(prepared.winner().kind(), TerminalKind::LethalDeath);
        let writer = FakeWriter::new(FakeMode::Fresh);
        let service = ProductionExtractionExecutionService::new(writer.clone());
        assert!(matches!(
            service
                .execute_prepared(&mut arbiter, &prepared, &extraction)
                .await,
            Err(ProductionExtractionExecutionError::PreparedWinnerMismatch)
        ));
        assert_eq!(writer.calls(), 0);

        let (_, matching_prepared) = prepared_arbiter(&extraction);
        let mut open_arbiter = TerminalArbiter::new(extraction_candidate.binding());
        assert!(matches!(
            service
                .execute_prepared(&mut open_arbiter, &matching_prepared, &extraction)
                .await,
            Err(ProductionExtractionExecutionError::PreparedWinnerMismatch)
        ));
        assert_eq!(writer.calls(), 0);
    }

    #[tokio::test]
    async fn repository_conflict_corruption_and_outage_leave_terminal_unresolved() {
        for (mode, expected) in [
            (FakeMode::Conflict, "conflict"),
            (FakeMode::ForeignResult, "stored"),
            (FakeMode::Unavailable, "repository"),
        ] {
            let extraction = prepared_extraction();
            let (mut arbiter, prepared) = prepared_arbiter(&extraction);
            let service = ProductionExtractionExecutionService::new(FakeWriter::new(mode));
            let outcome = service
                .execute_prepared(&mut arbiter, &prepared, &extraction)
                .await;
            assert!(matches!(
                (expected, outcome),
                (
                    "conflict",
                    Err(ProductionExtractionExecutionError::RepositoryConflict)
                ) | (
                    "stored",
                    Err(ProductionExtractionExecutionError::StoredResultMismatch)
                ) | (
                    "repository",
                    Err(ProductionExtractionExecutionError::Persistence(_))
                )
            ));
            assert_eq!(
                arbiter.non_terminal_admission(),
                NonTerminalAdmission::BlockedByUnresolvedTerminal
            );
            assert!(arbiter.committed_receipt().is_none());
        }
    }
}
