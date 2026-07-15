//! Pure Core orchestration for all authoritative terminal producers on one character.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` `DTH-001`, `DTH-010`,
//! `DTH-011`, and `TECH-015`/`021`-`023`; `Gravebound_Content_Production_Spec_v1.md`
//! `CONT-BOSS-001`, `CONT-ECHO-009`, and `CONT-HUB-002`; and
//! `Gravebound_Development_Roadmap_v1.md` `GB-M03-06`/`08` plus the M03 atomicity,
//! restart, and nonduplication gates.
//!
//! This is an internal server boundary. It accepts no protocol frame and performs no database
//! work. Each producer reports either a complete server-planned candidate or an explicit absence
//! for the same authenticated tick. Only a complete five-producer barrier may seal the arbiter,
//! which preserves lethal-death precedence without trusting producer call order.

use thiserror::Error;

use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CommitError, CommitResult, NonTerminalAdmission,
    PrepareError, PreparedTerminal, StoredTerminalReceipt, SubmitRejection, SubmitResult,
    TerminalArbiter, TerminalBinding, TerminalCandidate, TerminalKind, TerminalValidationError,
};

const ALL_PRODUCERS_MASK: u8 = 0b1_1111;

/// The five server systems that must evaluate every authoritative danger tick.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CoreTerminalProducer {
    LethalHealth,
    SuccessfulExtraction,
    EmergencyRecall,
    DisconnectRecovery,
    VerifiedFaultRestoration,
}

impl CoreTerminalProducer {
    pub const ALL: [Self; 5] = [
        Self::LethalHealth,
        Self::SuccessfulExtraction,
        Self::EmergencyRecall,
        Self::DisconnectRecovery,
        Self::VerifiedFaultRestoration,
    ];

    pub const fn terminal_kind(self) -> TerminalKind {
        match self {
            Self::LethalHealth => TerminalKind::LethalDeath,
            Self::SuccessfulExtraction => TerminalKind::SuccessfulExtraction,
            Self::EmergencyRecall => TerminalKind::EmergencyRecall,
            Self::DisconnectRecovery => TerminalKind::DisconnectRecovery,
            Self::VerifiedFaultRestoration => TerminalKind::VerifiedServerFaultRestoration,
        }
    }

    const fn mask(self) -> u8 {
        match self {
            Self::LethalHealth => 1 << 0,
            Self::SuccessfulExtraction => 1 << 1,
            Self::EmergencyRecall => 1 << 2,
            Self::DisconnectRecovery => 1 << 3,
            Self::VerifiedFaultRestoration => 1 << 4,
        }
    }
}

/// One producer's complete evaluation of one authenticated authoritative tick.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreTerminalEvaluation {
    producer: CoreTerminalProducer,
    binding: TerminalBinding,
    observed_tick: u64,
    expected_state_version: u64,
    candidate: Option<TerminalCandidate>,
}

impl CoreTerminalEvaluation {
    #[must_use]
    pub const fn absent(
        producer: CoreTerminalProducer,
        binding: TerminalBinding,
        observed_tick: u64,
        expected_state_version: u64,
    ) -> Self {
        Self {
            producer,
            binding,
            observed_tick,
            expected_state_version,
            candidate: None,
        }
    }

    #[must_use]
    pub const fn candidate(
        producer: CoreTerminalProducer,
        binding: TerminalBinding,
        observed_tick: u64,
        expected_state_version: u64,
        candidate: TerminalCandidate,
    ) -> Self {
        Self {
            producer,
            binding,
            observed_tick,
            expected_state_version,
            candidate: Some(candidate),
        }
    }

    pub const fn producer(&self) -> CoreTerminalProducer {
        self.producer
    }

    pub const fn observed_tick(&self) -> u64 {
        self.observed_tick
    }

    pub const fn expected_state_version(&self) -> u64 {
        self.expected_state_version
    }

    pub const fn has_candidate(&self) -> bool {
        self.candidate.is_some()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ProducerBarrier {
    observed_tick: u64,
    expected_state_version: u64,
    evaluated_mask: u8,
    candidate_count: u8,
}

impl ProducerBarrier {
    const fn new(observed_tick: u64, expected_state_version: u64) -> Self {
        Self {
            observed_tick,
            expected_state_version,
            evaluated_mask: 0,
            candidate_count: 0,
        }
    }

    const fn is_complete(self) -> bool {
        self.evaluated_mask == ALL_PRODUCERS_MASK
    }

    fn evaluated_count(self) -> u8 {
        u8::try_from(self.evaluated_mask.count_ones())
            .expect("a five-bit producer mask count always fits u8")
    }

    const fn missing_mask(self) -> u8 {
        ALL_PRODUCERS_MASK & !self.evaluated_mask
    }
}

/// Read-only progress for diagnostics and deterministic tick orchestration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CoreTerminalBarrierProgress {
    pub observed_tick: u64,
    pub expected_state_version: u64,
    pub evaluated_count: u8,
    pub candidate_count: u8,
    pub complete: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CoreTerminalEvaluationAccepted {
    pub producer: CoreTerminalProducer,
    pub observed_tick: u64,
    pub evaluated_count: u8,
    pub candidate_count: u8,
    pub barrier_complete: bool,
}

/// Result of sealing one fully evaluated authoritative tick.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreTerminalTickSeal {
    NoTerminal {
        observed_tick: u64,
        expected_state_version: u64,
    },
    Prepared(PreparedTerminal),
}

/// Coordinator-level admission includes a barrier state not visible to the lower-level arbiter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreNonTerminalAdmission {
    Allowed,
    BlockedByProducerBarrier { observed_tick: u64 },
    BlockedByUnresolvedTerminal,
    BlockedByCommittedTerminal,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CoreTerminalCoordinatorError {
    #[error("Core terminal coordination requires a wipeable authenticated account")]
    UnsupportedAuthenticatedNamespace,
    #[error("authenticated account does not own the terminal binding")]
    AuthenticatedBindingMismatch,
    #[error("authoritative terminal tick must be nonzero")]
    InvalidObservedTick,
    #[error("authoritative aggregate version must be positive and unexhausted")]
    InvalidStateVersion,
    #[error("terminal producer evaluation uses a foreign binding")]
    EvaluationBindingMismatch,
    #[error("terminal candidate kind does not match its producer")]
    ProducerKindMismatch {
        producer: CoreTerminalProducer,
        candidate_kind: TerminalKind,
    },
    #[error("terminal candidate binding differs from its producer evaluation")]
    CandidateBindingMismatch,
    #[error("terminal candidate tick differs from its producer evaluation")]
    CandidateTickMismatch {
        evaluation_tick: u64,
        candidate_tick: u64,
    },
    #[error("terminal candidate version differs from its producer evaluation")]
    CandidateVersionMismatch {
        evaluation_version: u64,
        candidate_version: u64,
    },
    #[error("terminal producer already evaluated this tick")]
    DuplicateProducer {
        producer: CoreTerminalProducer,
        observed_tick: u64,
    },
    #[error("producer evaluation tick drifted within the active barrier")]
    TickDrift { expected: u64, actual: u64 },
    #[error("producer evaluation version drifted within the active barrier")]
    VersionDrift { expected: u64, actual: u64 },
    #[error("next producer barrier must evaluate the next authoritative tick")]
    NonSequentialTick { expected: u64, actual: u64 },
    #[error("all five terminal producers must evaluate before tick sealing")]
    IncompleteProducerBarrier {
        observed_tick: u64,
        missing_mask: u8,
    },
    #[error("terminal outcome is already prepared or committed")]
    TerminalClosed,
    #[error("terminal arbiter rejected a server producer candidate: {0:?}")]
    ArbiterRejected(SubmitRejection),
    #[error("terminal arbiter could not seal the complete producer barrier: {0:?}")]
    Prepare(PrepareError),
    #[error("terminal receipt could not be recorded: {0:?}")]
    Commit(CommitError),
    #[error("stored terminal receipt is invalid: {0:?}")]
    InvalidStoredReceipt(TerminalValidationError),
}

/// Pure owner of one terminal arbiter and its five-producer tick barrier.
#[derive(Clone, Debug)]
pub struct CoreTerminalCoordinator {
    authenticated_account: AuthenticatedAccount,
    arbiter: TerminalArbiter,
    barrier: Option<ProducerBarrier>,
    last_sealed_tick: Option<u64>,
}

impl CoreTerminalCoordinator {
    pub fn new(
        authenticated_account: AuthenticatedAccount,
        binding: TerminalBinding,
    ) -> Result<Self, CoreTerminalCoordinatorError> {
        validate_authenticated_binding(authenticated_account, binding)?;
        Ok(Self {
            authenticated_account,
            arbiter: TerminalArbiter::new(binding),
            barrier: None,
            last_sealed_tick: None,
        })
    }

    /// Reconstructs immutable terminal authority after response loss or process restart.
    pub fn from_stored_receipt(
        authenticated_account: AuthenticatedAccount,
        receipt: StoredTerminalReceipt,
    ) -> Result<Self, CoreTerminalCoordinatorError> {
        receipt
            .validate()
            .map_err(CoreTerminalCoordinatorError::InvalidStoredReceipt)?;
        validate_authenticated_binding(authenticated_account, receipt.binding())?;
        let last_sealed_tick = Some(receipt.observed_tick());
        let arbiter = TerminalArbiter::from_stored_receipt(receipt)
            .map_err(CoreTerminalCoordinatorError::InvalidStoredReceipt)?;
        Ok(Self {
            authenticated_account,
            arbiter,
            barrier: None,
            last_sealed_tick,
        })
    }

    pub const fn authenticated_account(&self) -> AuthenticatedAccount {
        self.authenticated_account
    }

    pub const fn binding(&self) -> TerminalBinding {
        self.arbiter.binding()
    }

    pub const fn non_terminal_admission(&self) -> CoreNonTerminalAdmission {
        match self.arbiter.non_terminal_admission() {
            NonTerminalAdmission::BlockedByUnresolvedTerminal => {
                CoreNonTerminalAdmission::BlockedByUnresolvedTerminal
            }
            NonTerminalAdmission::BlockedByCommittedTerminal => {
                CoreNonTerminalAdmission::BlockedByCommittedTerminal
            }
            NonTerminalAdmission::Allowed if self.barrier.is_some() => {
                let observed_tick = match self.barrier {
                    Some(barrier) => barrier.observed_tick,
                    None => unreachable!(),
                };
                CoreNonTerminalAdmission::BlockedByProducerBarrier { observed_tick }
            }
            NonTerminalAdmission::Allowed => CoreNonTerminalAdmission::Allowed,
        }
    }

    pub fn barrier_progress(&self) -> Option<CoreTerminalBarrierProgress> {
        self.barrier.map(|barrier| CoreTerminalBarrierProgress {
            observed_tick: barrier.observed_tick,
            expected_state_version: barrier.expected_state_version,
            evaluated_count: barrier.evaluated_count(),
            candidate_count: barrier.candidate_count,
            complete: barrier.is_complete(),
        })
    }

    pub fn prepared_terminal(&self) -> Option<&PreparedTerminal> {
        self.arbiter.prepared_terminal()
    }

    pub fn committed_receipt(&self) -> Option<&StoredTerminalReceipt> {
        self.arbiter.committed_receipt()
    }

    /// Narrow crate-owned bridge for terminal executors. Callers outside the server cannot bypass
    /// the five-producer barrier or gain direct mutable access to the underlying arbiter.
    pub(crate) fn terminal_arbiter_mut(&mut self) -> &mut TerminalArbiter {
        &mut self.arbiter
    }

    /// Records one producer's evaluation. Rejected evaluations never advance the barrier.
    pub fn evaluate(
        &mut self,
        evaluation: CoreTerminalEvaluation,
    ) -> Result<CoreTerminalEvaluationAccepted, CoreTerminalCoordinatorError> {
        self.validate_evaluation(&evaluation)?;
        let CoreTerminalEvaluation {
            producer,
            binding: _,
            observed_tick,
            expected_state_version,
            candidate,
        } = evaluation;
        let has_candidate = candidate.is_some();
        if let Some(candidate) = candidate {
            match self.arbiter.submit(candidate) {
                SubmitResult::Accepted { .. } => {}
                SubmitResult::Rejected(rejection) => {
                    return Err(CoreTerminalCoordinatorError::ArbiterRejected(rejection));
                }
                SubmitResult::ReplayedPending { .. }
                | SubmitResult::ReplayedPrepared { .. }
                | SubmitResult::ReplayedCommitted { .. } => {
                    return Err(CoreTerminalCoordinatorError::TerminalClosed);
                }
            }
        }

        let barrier = self
            .barrier
            .get_or_insert_with(|| ProducerBarrier::new(observed_tick, expected_state_version));
        barrier.evaluated_mask |= producer.mask();
        barrier.candidate_count += u8::from(has_candidate);
        Ok(CoreTerminalEvaluationAccepted {
            producer,
            observed_tick,
            evaluated_count: barrier.evaluated_count(),
            candidate_count: barrier.candidate_count,
            barrier_complete: barrier.is_complete(),
        })
    }

    /// Seals only after all five producers evaluated the exact tick and aggregate version.
    pub fn seal_authoritative_tick(
        &mut self,
        observed_tick: u64,
        expected_state_version: u64,
    ) -> Result<CoreTerminalTickSeal, CoreTerminalCoordinatorError> {
        if self.arbiter.prepared_terminal().is_some() || self.arbiter.committed_receipt().is_some()
        {
            return Err(CoreTerminalCoordinatorError::TerminalClosed);
        }
        let barrier =
            self.barrier
                .ok_or(CoreTerminalCoordinatorError::IncompleteProducerBarrier {
                    observed_tick,
                    missing_mask: ALL_PRODUCERS_MASK,
                })?;
        if observed_tick != barrier.observed_tick {
            return Err(CoreTerminalCoordinatorError::TickDrift {
                expected: barrier.observed_tick,
                actual: observed_tick,
            });
        }
        if expected_state_version != barrier.expected_state_version {
            return Err(CoreTerminalCoordinatorError::VersionDrift {
                expected: barrier.expected_state_version,
                actual: expected_state_version,
            });
        }
        if !barrier.is_complete() {
            return Err(CoreTerminalCoordinatorError::IncompleteProducerBarrier {
                observed_tick,
                missing_mask: barrier.missing_mask(),
            });
        }

        if barrier.candidate_count == 0 {
            self.barrier = None;
            self.last_sealed_tick = Some(observed_tick);
            return Ok(CoreTerminalTickSeal::NoTerminal {
                observed_tick,
                expected_state_version,
            });
        }
        let prepared = self
            .arbiter
            .prepare(observed_tick)
            .map_err(CoreTerminalCoordinatorError::Prepare)?;
        self.barrier = None;
        self.last_sealed_tick = Some(observed_tick);
        Ok(CoreTerminalTickSeal::Prepared(prepared))
    }

    pub fn record_commit(
        &mut self,
        receipt: StoredTerminalReceipt,
    ) -> Result<CommitResult, CoreTerminalCoordinatorError> {
        self.arbiter
            .record_commit(receipt)
            .map_err(CoreTerminalCoordinatorError::Commit)
    }

    fn validate_evaluation(
        &self,
        evaluation: &CoreTerminalEvaluation,
    ) -> Result<(), CoreTerminalCoordinatorError> {
        if self.arbiter.prepared_terminal().is_some() || self.arbiter.committed_receipt().is_some()
        {
            return Err(CoreTerminalCoordinatorError::TerminalClosed);
        }
        if evaluation.observed_tick == 0 {
            return Err(CoreTerminalCoordinatorError::InvalidObservedTick);
        }
        if evaluation.expected_state_version == 0 || evaluation.expected_state_version == u64::MAX {
            return Err(CoreTerminalCoordinatorError::InvalidStateVersion);
        }
        if evaluation.binding != self.arbiter.binding() {
            return Err(CoreTerminalCoordinatorError::EvaluationBindingMismatch);
        }
        self.validate_barrier_identity(evaluation)?;
        if let Some(candidate) = evaluation.candidate.as_ref() {
            Self::validate_candidate(evaluation, candidate)?;
        }
        Ok(())
    }

    fn validate_barrier_identity(
        &self,
        evaluation: &CoreTerminalEvaluation,
    ) -> Result<(), CoreTerminalCoordinatorError> {
        if let Some(barrier) = self.barrier {
            if barrier.evaluated_mask & evaluation.producer.mask() != 0 {
                return Err(CoreTerminalCoordinatorError::DuplicateProducer {
                    producer: evaluation.producer,
                    observed_tick: barrier.observed_tick,
                });
            }
            if evaluation.observed_tick != barrier.observed_tick {
                return Err(CoreTerminalCoordinatorError::TickDrift {
                    expected: barrier.observed_tick,
                    actual: evaluation.observed_tick,
                });
            }
            if evaluation.expected_state_version != barrier.expected_state_version {
                return Err(CoreTerminalCoordinatorError::VersionDrift {
                    expected: barrier.expected_state_version,
                    actual: evaluation.expected_state_version,
                });
            }
        } else if let Some(last_tick) = self.last_sealed_tick {
            let expected = last_tick
                .checked_add(1)
                .ok_or(CoreTerminalCoordinatorError::InvalidObservedTick)?;
            if evaluation.observed_tick != expected {
                return Err(CoreTerminalCoordinatorError::NonSequentialTick {
                    expected,
                    actual: evaluation.observed_tick,
                });
            }
        }
        Ok(())
    }

    fn validate_candidate(
        evaluation: &CoreTerminalEvaluation,
        candidate: &TerminalCandidate,
    ) -> Result<(), CoreTerminalCoordinatorError> {
        if candidate.kind() != evaluation.producer.terminal_kind() {
            return Err(CoreTerminalCoordinatorError::ProducerKindMismatch {
                producer: evaluation.producer,
                candidate_kind: candidate.kind(),
            });
        }
        if candidate.binding() != evaluation.binding {
            return Err(CoreTerminalCoordinatorError::CandidateBindingMismatch);
        }
        if candidate.observed_tick() != evaluation.observed_tick {
            return Err(CoreTerminalCoordinatorError::CandidateTickMismatch {
                evaluation_tick: evaluation.observed_tick,
                candidate_tick: candidate.observed_tick(),
            });
        }
        if candidate.expected_state_version() != evaluation.expected_state_version {
            return Err(CoreTerminalCoordinatorError::CandidateVersionMismatch {
                evaluation_version: evaluation.expected_state_version,
                candidate_version: candidate.expected_state_version(),
            });
        }
        Ok(())
    }
}

fn validate_authenticated_binding(
    authenticated_account: AuthenticatedAccount,
    binding: TerminalBinding,
) -> Result<(), CoreTerminalCoordinatorError> {
    if authenticated_account.namespace != AuthenticatedNamespace::WipeableTest {
        return Err(CoreTerminalCoordinatorError::UnsupportedAuthenticatedNamespace);
    }
    if authenticated_account.account_id.as_bytes() != *binding.account_id() {
        return Err(CoreTerminalCoordinatorError::AuthenticatedBindingMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AccountId;

    const TICK: u64 = 40;
    const VERSION: u64 = 7;

    fn account() -> AuthenticatedAccount {
        AuthenticatedAccount {
            account_id: AccountId::new([1; 16]).expect("valid account"),
            namespace: AuthenticatedNamespace::WipeableTest,
        }
    }

    fn binding() -> TerminalBinding {
        TerminalBinding::new([1; 16], [2; 16], [3; 16], [4; 16]).expect("valid binding")
    }

    fn foreign_binding() -> TerminalBinding {
        TerminalBinding::new([9; 16], [8; 16], [7; 16], [6; 16]).expect("valid foreign binding")
    }

    fn candidate(
        producer: CoreTerminalProducer,
        discriminator: u8,
        tick: u64,
        version: u64,
    ) -> TerminalCandidate {
        TerminalCandidate::from_server_plan(
            binding(),
            [discriminator; 16],
            [discriminator.wrapping_add(20); 16],
            [discriminator.wrapping_add(40); 32],
            [discriminator.wrapping_add(60); 32],
            version,
            tick,
            producer.terminal_kind(),
        )
        .expect("valid server candidate")
    }

    fn absent(producer: CoreTerminalProducer, tick: u64) -> CoreTerminalEvaluation {
        CoreTerminalEvaluation::absent(producer, binding(), tick, VERSION)
    }

    fn with_candidate(
        producer: CoreTerminalProducer,
        discriminator: u8,
        tick: u64,
    ) -> CoreTerminalEvaluation {
        CoreTerminalEvaluation::candidate(
            producer,
            binding(),
            tick,
            VERSION,
            candidate(producer, discriminator, tick, VERSION),
        )
    }

    fn evaluate_all_absent(coordinator: &mut CoreTerminalCoordinator, tick: u64) {
        for producer in CoreTerminalProducer::ALL {
            coordinator
                .evaluate(absent(producer, tick))
                .expect("producer evaluation succeeds");
        }
    }

    fn permutations() -> Vec<[CoreTerminalProducer; 5]> {
        fn visit(
            values: &mut [CoreTerminalProducer; 5],
            index: usize,
            output: &mut Vec<[CoreTerminalProducer; 5]>,
        ) {
            if index == values.len() {
                output.push(*values);
                return;
            }
            for swap_index in index..values.len() {
                values.swap(index, swap_index);
                visit(values, index + 1, output);
                values.swap(index, swap_index);
            }
        }

        let mut values = CoreTerminalProducer::ALL;
        let mut output = Vec::with_capacity(120);
        visit(&mut values, 0, &mut output);
        output
    }

    #[test]
    fn every_producer_order_permutation_seals_the_same_candidate() {
        for order in permutations() {
            let mut coordinator = CoreTerminalCoordinator::new(account(), binding()).unwrap();
            for producer in order {
                let evaluation = if producer == CoreTerminalProducer::EmergencyRecall {
                    with_candidate(producer, 10, TICK)
                } else {
                    absent(producer, TICK)
                };
                coordinator.evaluate(evaluation).unwrap();
            }
            let CoreTerminalTickSeal::Prepared(prepared) =
                coordinator.seal_authoritative_tick(TICK, VERSION).unwrap()
            else {
                panic!("one Recall candidate must prepare");
            };
            assert_eq!(prepared.winner().kind(), TerminalKind::EmergencyRecall);
            assert_eq!(prepared.candidate_count(), 1);
        }
    }

    #[test]
    fn explicit_absence_from_all_five_producers_advances_without_terminal() {
        let mut coordinator = CoreTerminalCoordinator::new(account(), binding()).unwrap();
        evaluate_all_absent(&mut coordinator, TICK);
        assert_eq!(
            coordinator.barrier_progress(),
            Some(CoreTerminalBarrierProgress {
                observed_tick: TICK,
                expected_state_version: VERSION,
                evaluated_count: 5,
                candidate_count: 0,
                complete: true,
            })
        );
        assert_eq!(
            coordinator.seal_authoritative_tick(TICK, VERSION),
            Ok(CoreTerminalTickSeal::NoTerminal {
                observed_tick: TICK,
                expected_state_version: VERSION,
            })
        );
        assert_eq!(
            coordinator.non_terminal_admission(),
            CoreNonTerminalAdmission::Allowed
        );
    }

    #[test]
    fn duplicate_producers_do_not_advance_or_replace_the_barrier() {
        let mut coordinator = CoreTerminalCoordinator::new(account(), binding()).unwrap();
        coordinator
            .evaluate(absent(CoreTerminalProducer::LethalHealth, TICK))
            .unwrap();
        assert_eq!(
            coordinator.evaluate(absent(CoreTerminalProducer::LethalHealth, TICK)),
            Err(CoreTerminalCoordinatorError::DuplicateProducer {
                producer: CoreTerminalProducer::LethalHealth,
                observed_tick: TICK,
            })
        );
        assert_eq!(coordinator.barrier_progress().unwrap().evaluated_count, 1);
        assert_eq!(coordinator.barrier_progress().unwrap().candidate_count, 0);
    }

    #[test]
    fn incomplete_barrier_cannot_seal_even_when_a_candidate_exists() {
        let mut coordinator = CoreTerminalCoordinator::new(account(), binding()).unwrap();
        coordinator
            .evaluate(with_candidate(
                CoreTerminalProducer::SuccessfulExtraction,
                11,
                TICK,
            ))
            .unwrap();
        assert!(matches!(
            coordinator.seal_authoritative_tick(TICK, VERSION),
            Err(CoreTerminalCoordinatorError::IncompleteProducerBarrier {
                observed_tick: TICK,
                missing_mask,
            }) if missing_mask != 0
        ));
        assert_eq!(
            coordinator.barrier_progress(),
            Some(CoreTerminalBarrierProgress {
                observed_tick: TICK,
                expected_state_version: VERSION,
                evaluated_count: 1,
                candidate_count: 1,
                complete: false,
            })
        );
        assert_eq!(
            coordinator.non_terminal_admission(),
            CoreNonTerminalAdmission::BlockedByUnresolvedTerminal
        );
    }

    #[test]
    fn same_tick_lethal_death_wins_after_every_producer_reports() {
        let mut coordinator = CoreTerminalCoordinator::new(account(), binding()).unwrap();
        for (index, producer) in CoreTerminalProducer::ALL.into_iter().rev().enumerate() {
            coordinator
                .evaluate(with_candidate(
                    producer,
                    u8::try_from(index + 1).unwrap(),
                    TICK,
                ))
                .unwrap();
        }
        let CoreTerminalTickSeal::Prepared(prepared) =
            coordinator.seal_authoritative_tick(TICK, VERSION).unwrap()
        else {
            panic!("five candidates must prepare");
        };
        assert_eq!(prepared.candidate_count(), 5);
        assert_eq!(prepared.winner().kind(), TerminalKind::LethalDeath);
    }

    #[test]
    fn no_terminal_tick_advances_only_to_the_immediate_next_tick() {
        let mut coordinator = CoreTerminalCoordinator::new(account(), binding()).unwrap();
        evaluate_all_absent(&mut coordinator, TICK);
        coordinator.seal_authoritative_tick(TICK, VERSION).unwrap();

        assert_eq!(
            coordinator.evaluate(absent(CoreTerminalProducer::LethalHealth, TICK + 2)),
            Err(CoreTerminalCoordinatorError::NonSequentialTick {
                expected: TICK + 1,
                actual: TICK + 2,
            })
        );
        evaluate_all_absent(&mut coordinator, TICK + 1);
        assert!(matches!(
            coordinator
                .seal_authoritative_tick(TICK + 1, VERSION)
                .unwrap(),
            CoreTerminalTickSeal::NoTerminal { .. }
        ));
    }

    #[test]
    fn committed_receipt_reconstructs_immutable_restart_authority() {
        let mut coordinator = CoreTerminalCoordinator::new(account(), binding()).unwrap();
        for producer in CoreTerminalProducer::ALL {
            let evaluation = if producer == CoreTerminalProducer::DisconnectRecovery {
                with_candidate(producer, 13, TICK)
            } else {
                absent(producer, TICK)
            };
            coordinator.evaluate(evaluation).unwrap();
        }
        let CoreTerminalTickSeal::Prepared(prepared) =
            coordinator.seal_authoritative_tick(TICK, VERSION).unwrap()
        else {
            panic!("disconnect candidate must prepare");
        };
        let receipt = StoredTerminalReceipt::from_prepared(&prepared, TICK, [99; 32]).unwrap();
        coordinator.record_commit(receipt.clone()).unwrap();

        let restored =
            CoreTerminalCoordinator::from_stored_receipt(account(), receipt.clone()).unwrap();
        assert_eq!(restored.committed_receipt(), Some(&receipt));
        assert_eq!(
            restored.non_terminal_admission(),
            CoreNonTerminalAdmission::BlockedByCommittedTerminal
        );
    }

    #[test]
    fn authentication_producer_binding_tick_and_version_drift_fail_closed() {
        let production = AuthenticatedAccount {
            account_id: account().account_id,
            namespace: AuthenticatedNamespace::Production,
        };
        assert!(matches!(
            CoreTerminalCoordinator::new(production, binding()),
            Err(CoreTerminalCoordinatorError::UnsupportedAuthenticatedNamespace)
        ));
        assert!(matches!(
            CoreTerminalCoordinator::new(account(), foreign_binding()),
            Err(CoreTerminalCoordinatorError::AuthenticatedBindingMismatch)
        ));

        let mut coordinator = CoreTerminalCoordinator::new(account(), binding()).unwrap();
        assert_eq!(
            coordinator.evaluate(CoreTerminalEvaluation::absent(
                CoreTerminalProducer::LethalHealth,
                binding(),
                0,
                VERSION,
            )),
            Err(CoreTerminalCoordinatorError::InvalidObservedTick)
        );
        assert_eq!(
            coordinator.evaluate(CoreTerminalEvaluation::absent(
                CoreTerminalProducer::LethalHealth,
                binding(),
                TICK,
                0,
            )),
            Err(CoreTerminalCoordinatorError::InvalidStateVersion)
        );
        assert_eq!(
            coordinator.evaluate(CoreTerminalEvaluation::absent(
                CoreTerminalProducer::LethalHealth,
                foreign_binding(),
                TICK,
                VERSION,
            )),
            Err(CoreTerminalCoordinatorError::EvaluationBindingMismatch)
        );

        let wrong_kind = candidate(CoreTerminalProducer::EmergencyRecall, 20, TICK, VERSION);
        assert!(matches!(
            coordinator.evaluate(CoreTerminalEvaluation::candidate(
                CoreTerminalProducer::LethalHealth,
                binding(),
                TICK,
                VERSION,
                wrong_kind,
            )),
            Err(CoreTerminalCoordinatorError::ProducerKindMismatch { .. })
        ));

        coordinator
            .evaluate(absent(CoreTerminalProducer::LethalHealth, TICK))
            .unwrap();
        assert_eq!(
            coordinator.evaluate(absent(CoreTerminalProducer::SuccessfulExtraction, TICK + 1)),
            Err(CoreTerminalCoordinatorError::TickDrift {
                expected: TICK,
                actual: TICK + 1,
            })
        );
        assert_eq!(
            coordinator.evaluate(CoreTerminalEvaluation::absent(
                CoreTerminalProducer::SuccessfulExtraction,
                binding(),
                TICK,
                VERSION + 1,
            )),
            Err(CoreTerminalCoordinatorError::VersionDrift {
                expected: VERSION,
                actual: VERSION + 1,
            })
        );
    }

    #[test]
    fn candidate_specific_binding_tick_and_version_mismatch_never_reaches_arbiter() {
        let cases = [
            CoreTerminalEvaluation::candidate(
                CoreTerminalProducer::LethalHealth,
                binding(),
                TICK,
                VERSION,
                TerminalCandidate::from_server_plan(
                    foreign_binding(),
                    [30; 16],
                    [31; 16],
                    [32; 32],
                    [33; 32],
                    VERSION,
                    TICK,
                    TerminalKind::LethalDeath,
                )
                .unwrap(),
            ),
            CoreTerminalEvaluation::candidate(
                CoreTerminalProducer::LethalHealth,
                binding(),
                TICK,
                VERSION,
                candidate(CoreTerminalProducer::LethalHealth, 34, TICK + 1, VERSION),
            ),
            CoreTerminalEvaluation::candidate(
                CoreTerminalProducer::LethalHealth,
                binding(),
                TICK,
                VERSION,
                candidate(CoreTerminalProducer::LethalHealth, 35, TICK, VERSION + 1),
            ),
        ];
        let expected = [
            CoreTerminalCoordinatorError::CandidateBindingMismatch,
            CoreTerminalCoordinatorError::CandidateTickMismatch {
                evaluation_tick: TICK,
                candidate_tick: TICK + 1,
            },
            CoreTerminalCoordinatorError::CandidateVersionMismatch {
                evaluation_version: VERSION,
                candidate_version: VERSION + 1,
            },
        ];
        for (evaluation, expected) in cases.into_iter().zip(expected) {
            let mut coordinator = CoreTerminalCoordinator::new(account(), binding()).unwrap();
            assert_eq!(coordinator.evaluate(evaluation), Err(expected));
            assert!(coordinator.barrier_progress().is_none());
            assert_eq!(
                coordinator.non_terminal_admission(),
                CoreNonTerminalAdmission::Allowed
            );
        }
    }

    #[test]
    fn producer_barrier_blocks_nonterminal_admission_even_when_reports_are_absent() {
        let mut coordinator = CoreTerminalCoordinator::new(account(), binding()).unwrap();
        assert_eq!(
            coordinator.non_terminal_admission(),
            CoreNonTerminalAdmission::Allowed
        );
        coordinator
            .evaluate(absent(CoreTerminalProducer::LethalHealth, TICK))
            .unwrap();
        assert_eq!(
            coordinator.non_terminal_admission(),
            CoreNonTerminalAdmission::BlockedByProducerBarrier {
                observed_tick: TICK,
            }
        );
    }
}
