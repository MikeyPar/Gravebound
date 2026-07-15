//! Arbiter-gated execution of one sealed `GB-M03-06C` death transaction.
//!
//! Authorities: `Gravebound_Production_GDD_v1_Canonical.md` (`DTH-001`, `TECH-021`-`023`),
//! `Gravebound_Content_Production_Spec_v1.md` (`CONT-ECHO-009`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-06`, `GB-M03-08`, `GB-M03-13`). The
//! accepted `SPEC-CONFLICT-009` contract keeps the Echo projector inside the same `PostgreSQL`
//! transaction. This service therefore executes only the already sealed lethal winner and never
//! accepts a client-authored cause, destination, destruction list, or placement map.

use std::future::Future;

use persistence::{
    DurableDeathCommitRequestV1, DurableDeathContentAuthorityV1, DurableDeathTransactionV1,
    PersistenceError, PostgresPersistence, StoredCommittedDeathResultV1,
};
use thiserror::Error;

use crate::{
    CommitError, CommitResult, PreparedDurableDeathCommit, PreparedTerminal, StoredTerminalReceipt,
    TerminalArbiter, TerminalBinding, TerminalCandidate, TerminalKind, TerminalValidationError,
};

/// Repository seam kept narrow so arbitration and post-commit validation can be tested without a
/// second gameplay writer.
pub trait DurableDeathWriter: Send + Sync {
    fn transact(
        &self,
        request: &DurableDeathCommitRequestV1,
        content: &DurableDeathContentAuthorityV1,
    ) -> impl Future<Output = Result<DurableDeathTransactionV1, PersistenceError>> + Send;
}

impl DurableDeathWriter for PostgresPersistence {
    async fn transact(
        &self,
        request: &DurableDeathCommitRequestV1,
        content: &DurableDeathContentAuthorityV1,
    ) -> Result<DurableDeathTransactionV1, PersistenceError> {
        self.transact_durable_death(request, content).await
    }
}

#[derive(Debug, Clone)]
pub struct DurableDeathExecutionService<Writer> {
    writer: Writer,
}

impl<Writer> DurableDeathExecutionService<Writer> {
    #[must_use]
    pub const fn new(writer: Writer) -> Self {
        Self { writer }
    }
}

pub type PostgresDurableDeathExecutionService = DurableDeathExecutionService<PostgresPersistence>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableDeathExecutionOutcome {
    pub transaction: DurableDeathTransactionV1,
    pub terminal_commit: CommitResult,
}

#[derive(Debug, Error)]
pub enum DurableDeathExecutionError {
    #[error("sealed death could not form terminal authority: {0:?}")]
    InvalidTerminalAuthority(TerminalValidationError),
    #[error("prepared terminal winner is not the sealed lethal-death request")]
    PreparedWinnerMismatch,
    #[error("durable death repository rejected the transaction")]
    Persistence(#[source] PersistenceError),
    #[error("stored death result is corrupt or does not match the sealed request")]
    StoredResultMismatch,
    #[error("terminal receipt could not be published: {0:?}")]
    TerminalCommit(CommitError),
}

impl<Writer> DurableDeathExecutionService<Writer>
where
    Writer: DurableDeathWriter,
{
    /// Executes only the arbiter's sealed lethal winner. A repository error leaves the arbiter in
    /// `Prepared`, so ordinary departure remains blocked and the exact request can be retried.
    pub async fn execute_prepared(
        &self,
        arbiter: &mut TerminalArbiter,
        prepared_terminal: &PreparedTerminal,
        death: &PreparedDurableDeathCommit,
    ) -> Result<DurableDeathExecutionOutcome, DurableDeathExecutionError> {
        let candidate = durable_death_terminal_candidate(death)?;
        if arbiter.prepared_terminal() != Some(prepared_terminal)
            || prepared_terminal.winner() != &candidate
            || candidate.kind() != TerminalKind::LethalDeath
        {
            return Err(DurableDeathExecutionError::PreparedWinnerMismatch);
        }

        let transaction = self
            .writer
            .transact(&death.request, &death.content)
            .await
            .map_err(DurableDeathExecutionError::Persistence)?;
        let result = transaction.result();
        validate_stored_result_intent(death, result)?;
        let result_hash = result
            .digest()
            .map_err(|_| DurableDeathExecutionError::StoredResultMismatch)?;
        // The durable simulation tick is stable across response loss and process restart. Using
        // wall-clock acknowledgement time here would make the reconstructed receipt diverge.
        let receipt = StoredTerminalReceipt::from_prepared(
            prepared_terminal,
            death.request.plan.event.death_tick,
            result_hash,
        )
        .map_err(DurableDeathExecutionError::InvalidTerminalAuthority)?;
        let terminal_commit = arbiter
            .record_commit(receipt)
            .map_err(DurableDeathExecutionError::TerminalCommit)?;
        Ok(DurableDeathExecutionOutcome {
            transaction,
            terminal_commit,
        })
    }
}

/// Converts one sealed server death plan into the opaque candidate shared with extraction/Recall.
pub fn durable_death_terminal_candidate(
    death: &PreparedDurableDeathCommit,
) -> Result<TerminalCandidate, DurableDeathExecutionError> {
    let event = &death.request.plan.event;
    let binding = TerminalBinding::new(
        event.account_id,
        event.character_id,
        event.lineage_id,
        event.restore_point_id,
    )
    .map_err(DurableDeathExecutionError::InvalidTerminalAuthority)?;
    TerminalCandidate::from_server_plan(
        binding,
        event.death_id,
        event.mutation_id,
        death.request.canonical_request_hash,
        death.request.canonical_plan_hash,
        event.versions.account.pre,
        event.death_tick,
        TerminalKind::LethalDeath,
    )
    .map_err(DurableDeathExecutionError::InvalidTerminalAuthority)
}

fn validate_stored_result_intent(
    death: &PreparedDurableDeathCommit,
    result: &StoredCommittedDeathResultV1,
) -> Result<(), DurableDeathExecutionError> {
    result
        .validate()
        .map_err(|_| DurableDeathExecutionError::StoredResultMismatch)?;
    let mut committed_request = death.request.clone();
    committed_request
        .bind_commit_time(result.committed_at_unix_ms)
        .map_err(|_| DurableDeathExecutionError::StoredResultMismatch)?;
    result
        .validate_against(&committed_request)
        .map_err(|_| DurableDeathExecutionError::StoredResultMismatch)
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use persistence::{DurableDeathTransactionV1, StoredCommittedDeathResultV1};

    use super::*;
    use crate::{
        NonTerminalAdmission, SubmitResult, durable_death_service::tests::prepared_commit,
    };

    #[derive(Debug, Clone, Copy)]
    enum FakeMode {
        Fresh,
        Replay,
        ForeignResult,
        Unavailable,
    }

    #[derive(Debug, Clone)]
    struct FakeWriter {
        mode: FakeMode,
        calls: Arc<AtomicUsize>,
    }

    impl FakeWriter {
        fn new(mode: FakeMode) -> Self {
            Self {
                mode,
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl DurableDeathWriter for FakeWriter {
        async fn transact(
            &self,
            request: &DurableDeathCommitRequestV1,
            _content: &DurableDeathContentAuthorityV1,
        ) -> Result<DurableDeathTransactionV1, PersistenceError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if matches!(self.mode, FakeMode::Unavailable) {
                return Err(PersistenceError::DurableDeathTerminalSuperseded);
            }
            let mut committed = request.clone();
            committed.bind_commit_time(
                request
                    .plan
                    .event
                    .committed_at_unix_ms
                    .checked_add(10)
                    .expect("fixture commit time"),
            )?;
            let mut result = StoredCommittedDeathResultV1::from_request(&committed)?;
            if matches!(self.mode, FakeMode::ForeignResult) {
                result.account_id = [201; 16];
            }
            Ok(if matches!(self.mode, FakeMode::Replay) {
                DurableDeathTransactionV1::Replayed(result)
            } else {
                DurableDeathTransactionV1::Fresh(result)
            })
        }
    }

    fn prepared_arbiter(death: &PreparedDurableDeathCommit) -> (TerminalArbiter, PreparedTerminal) {
        let candidate = durable_death_terminal_candidate(death).expect("terminal candidate");
        let mut arbiter = TerminalArbiter::new(candidate.binding());
        assert!(matches!(
            arbiter.submit(candidate),
            SubmitResult::Accepted { .. }
        ));
        let prepared = arbiter
            .prepare(death.request.plan.event.death_tick)
            .expect("sealed lethal tick");
        (arbiter, prepared)
    }

    #[test]
    fn candidate_binds_the_complete_sealed_death_authority() {
        let death = prepared_commit();
        let candidate = durable_death_terminal_candidate(&death).unwrap();
        let event = &death.request.plan.event;
        assert_eq!(candidate.binding().account_id(), &event.account_id);
        assert_eq!(candidate.binding().character_id(), &event.character_id);
        assert_eq!(candidate.binding().lineage_id(), &event.lineage_id);
        assert_eq!(
            candidate.binding().restore_point_id(),
            &event.restore_point_id
        );
        assert_eq!(candidate.terminal_id(), &event.death_id);
        assert_eq!(candidate.mutation_id(), &event.mutation_id);
        assert_eq!(
            candidate.payload_hash(),
            &death.request.canonical_request_hash
        );
        assert_eq!(
            candidate.server_plan_hash(),
            &death.request.canonical_plan_hash
        );
        assert_eq!(
            candidate.expected_state_version(),
            event.versions.account.pre
        );
        assert_eq!(candidate.observed_tick(), event.death_tick);
        assert_eq!(candidate.kind(), TerminalKind::LethalDeath);
    }

    #[tokio::test]
    async fn fresh_commit_publishes_one_restart_safe_terminal_receipt() {
        let death = prepared_commit();
        let (mut arbiter, prepared) = prepared_arbiter(&death);
        let writer = FakeWriter::new(FakeMode::Fresh);
        let service = DurableDeathExecutionService::new(writer.clone());

        let outcome = service
            .execute_prepared(&mut arbiter, &prepared, &death)
            .await
            .unwrap();

        assert!(!outcome.transaction.is_replay());
        assert!(matches!(
            outcome.terminal_commit,
            CommitResult::Committed(_)
        ));
        assert_eq!(writer.calls(), 1);
        assert_eq!(
            arbiter.non_terminal_admission(),
            NonTerminalAdmission::BlockedByCommittedTerminal
        );
        let receipt = arbiter.committed_receipt().unwrap();
        assert_eq!(
            receipt.committed_tick(),
            death.request.plan.event.death_tick
        );
        let stored = receipt.to_storage_v1();
        assert_eq!(
            StoredTerminalReceipt::from_storage(&stored).unwrap(),
            *receipt
        );
    }

    #[tokio::test]
    async fn database_replay_reconstructs_the_same_terminal_after_response_loss() {
        let death = prepared_commit();
        let (mut arbiter, prepared) = prepared_arbiter(&death);
        let service = DurableDeathExecutionService::new(FakeWriter::new(FakeMode::Replay));

        let outcome = service
            .execute_prepared(&mut arbiter, &prepared, &death)
            .await
            .unwrap();
        assert!(outcome.transaction.is_replay());
        let receipt = arbiter.committed_receipt().unwrap().clone();

        let mut restarted = TerminalArbiter::from_stored_receipt(
            StoredTerminalReceipt::from_storage(&receipt.to_storage_v1()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            restarted.submit(durable_death_terminal_candidate(&death).unwrap()),
            SubmitResult::ReplayedCommitted { receipt }
        );
    }

    #[tokio::test]
    async fn mismatched_winner_is_rejected_before_repository_access() {
        let death = prepared_commit();
        let death_candidate = durable_death_terminal_candidate(&death).unwrap();
        let recall = TerminalCandidate::from_server_plan(
            death_candidate.binding(),
            [31; 16],
            [32; 16],
            [33; 32],
            [34; 32],
            death_candidate.expected_state_version(),
            death_candidate.observed_tick(),
            TerminalKind::EmergencyRecall,
        )
        .unwrap();
        let mut arbiter = TerminalArbiter::new(recall.binding());
        arbiter.submit(recall);
        let prepared = arbiter.prepare(death_candidate.observed_tick()).unwrap();
        let writer = FakeWriter::new(FakeMode::Fresh);
        let service = DurableDeathExecutionService::new(writer.clone());

        assert!(matches!(
            service
                .execute_prepared(&mut arbiter, &prepared, &death)
                .await,
            Err(DurableDeathExecutionError::PreparedWinnerMismatch)
        ));
        assert_eq!(writer.calls(), 0);

        let (foreign_arbiter, matching_prepared) = prepared_arbiter(&death);
        let mut open_arbiter = TerminalArbiter::new(foreign_arbiter.binding());
        assert!(matches!(
            service
                .execute_prepared(&mut open_arbiter, &matching_prepared, &death)
                .await,
            Err(DurableDeathExecutionError::PreparedWinnerMismatch)
        ));
        assert_eq!(writer.calls(), 0);
    }

    #[tokio::test]
    async fn corrupt_result_and_repository_failure_leave_departure_blocked() {
        for (mode, expected) in [
            (FakeMode::ForeignResult, "stored"),
            (FakeMode::Unavailable, "repository"),
        ] {
            let death = prepared_commit();
            let (mut arbiter, prepared) = prepared_arbiter(&death);
            let service = DurableDeathExecutionService::new(FakeWriter::new(mode));
            let result = service
                .execute_prepared(&mut arbiter, &prepared, &death)
                .await;
            assert!(matches!(
                (expected, result),
                (
                    "stored",
                    Err(DurableDeathExecutionError::StoredResultMismatch)
                ) | (
                    "repository",
                    Err(DurableDeathExecutionError::Persistence(_))
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
