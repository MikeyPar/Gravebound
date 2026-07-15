//! Pure, server-owned terminal-outcome arbitration for one selected character.
//!
//! Authority: GDD `DTH-001`, `DTH-010`, `DTH-011`, `TECH-015`, `TECH-021`,
//! `TECH-022`, and `TECH-023`; Content Production Spec `CONT-BOSS-001`,
//! `CONT-BOSS-005`, `CONT-HUB-001`, and `CONT-HUB-002`; Roadmap `GB-M03-06`
//! and `GB-M03-08`; and the `GB-M03-06C` plus planned `GB-M03-08` task
//! contracts.
//!
//! All candidates are already authenticated, character-bound, and planned by
//! the server. The arbiter deliberately carries only an opaque server-plan
//! digest: clients cannot choose an outcome, item destination, destruction
//! list, placement map, or authoritative version through this seam.
//!
//! Submission and durable commit are separate phases. Every producer submits
//! during the authoritative simulation tick, then the server seals that tick
//! exactly once. This guarantees that lethal death wins a same-tick race while
//! preserving the stronger rule that an already committed outcome is immutable.

use std::cmp::Ordering;

pub const TERMINAL_ID_BYTES: usize = 16;
pub const TERMINAL_HASH_BYTES: usize = 32;
pub const STORED_TERMINAL_RECEIPT_SCHEMA_V1: u16 = 1;

/// Defensive bound for duplicate/concurrent producers on one simulation tick.
pub const MAX_TERMINAL_CANDIDATES_PER_TICK: usize = 32;

/// Stable terminal categories, ordered by authoritative same-tick precedence.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TerminalKind {
    LethalDeath,
    SuccessfulExtraction,
    EmergencyRecall,
    DisconnectRecovery,
    VerifiedServerFaultRestoration,
}

impl TerminalKind {
    const fn priority(self) -> u8 {
        match self {
            Self::LethalDeath => 0,
            Self::SuccessfulExtraction => 1,
            Self::EmergencyRecall => 2,
            Self::DisconnectRecovery => 3,
            Self::VerifiedServerFaultRestoration => 4,
        }
    }

    /// Append-only storage discriminant. Existing values must never change.
    pub const fn stable_code(self) -> u8 {
        match self {
            Self::LethalDeath => 1,
            Self::SuccessfulExtraction => 2,
            Self::EmergencyRecall => 3,
            Self::DisconnectRecovery => 4,
            Self::VerifiedServerFaultRestoration => 5,
        }
    }

    pub const fn from_stable_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::LethalDeath),
            2 => Some(Self::SuccessfulExtraction),
            3 => Some(Self::EmergencyRecall),
            4 => Some(Self::DisconnectRecovery),
            5 => Some(Self::VerifiedServerFaultRestoration),
            _ => None,
        }
    }
}

/// Authenticated aggregate ownership. Display names and transport identities
/// are intentionally absent because neither is durable authority.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TerminalBinding {
    account: [u8; TERMINAL_ID_BYTES],
    character: [u8; TERMINAL_ID_BYTES],
    lineage: [u8; TERMINAL_ID_BYTES],
    restore_point: [u8; TERMINAL_ID_BYTES],
}

impl TerminalBinding {
    pub fn new(
        account_id: [u8; TERMINAL_ID_BYTES],
        character_id: [u8; TERMINAL_ID_BYTES],
        lineage_id: [u8; TERMINAL_ID_BYTES],
        restore_point_id: [u8; TERMINAL_ID_BYTES],
    ) -> Result<Self, TerminalValidationError> {
        if is_zero(&account_id) {
            return Err(TerminalValidationError::MissingAccountId);
        }
        if is_zero(&character_id) {
            return Err(TerminalValidationError::MissingCharacterId);
        }
        if is_zero(&lineage_id) {
            return Err(TerminalValidationError::MissingLineageId);
        }
        if is_zero(&restore_point_id) {
            return Err(TerminalValidationError::MissingRestorePointId);
        }
        Ok(Self {
            account: account_id,
            character: character_id,
            lineage: lineage_id,
            restore_point: restore_point_id,
        })
    }

    pub const fn account_id(&self) -> &[u8; TERMINAL_ID_BYTES] {
        &self.account
    }

    pub const fn character_id(&self) -> &[u8; TERMINAL_ID_BYTES] {
        &self.character
    }

    pub const fn lineage_id(&self) -> &[u8; TERMINAL_ID_BYTES] {
        &self.lineage
    }

    pub const fn restore_point_id(&self) -> &[u8; TERMINAL_ID_BYTES] {
        &self.restore_point
    }
}

/// A server-validated terminal proposal. `server_plan_hash` binds the complete
/// authoritative repository plan without exposing or accepting destinations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalCandidate {
    binding: TerminalBinding,
    terminal_id: [u8; TERMINAL_ID_BYTES],
    mutation_id: [u8; TERMINAL_ID_BYTES],
    payload_hash: [u8; TERMINAL_HASH_BYTES],
    server_plan_hash: [u8; TERMINAL_HASH_BYTES],
    expected_state_version: u64,
    observed_tick: u64,
    kind: TerminalKind,
}

impl TerminalCandidate {
    #[allow(clippy::too_many_arguments)]
    pub fn from_server_plan(
        binding: TerminalBinding,
        terminal_id: [u8; TERMINAL_ID_BYTES],
        mutation_id: [u8; TERMINAL_ID_BYTES],
        payload_hash: [u8; TERMINAL_HASH_BYTES],
        server_plan_hash: [u8; TERMINAL_HASH_BYTES],
        expected_state_version: u64,
        observed_tick: u64,
        kind: TerminalKind,
    ) -> Result<Self, TerminalValidationError> {
        let candidate = Self {
            binding,
            terminal_id,
            mutation_id,
            payload_hash,
            server_plan_hash,
            expected_state_version,
            observed_tick,
            kind,
        };
        candidate.validate()?;
        Ok(candidate)
    }

    pub const fn binding(&self) -> TerminalBinding {
        self.binding
    }

    pub const fn terminal_id(&self) -> &[u8; TERMINAL_ID_BYTES] {
        &self.terminal_id
    }

    pub const fn mutation_id(&self) -> &[u8; TERMINAL_ID_BYTES] {
        &self.mutation_id
    }

    pub const fn payload_hash(&self) -> &[u8; TERMINAL_HASH_BYTES] {
        &self.payload_hash
    }

    pub const fn server_plan_hash(&self) -> &[u8; TERMINAL_HASH_BYTES] {
        &self.server_plan_hash
    }

    pub const fn expected_state_version(&self) -> u64 {
        self.expected_state_version
    }

    pub const fn observed_tick(&self) -> u64 {
        self.observed_tick
    }

    pub const fn kind(&self) -> TerminalKind {
        self.kind
    }

    fn validate(&self) -> Result<(), TerminalValidationError> {
        if is_zero(&self.terminal_id) {
            return Err(TerminalValidationError::MissingTerminalId);
        }
        if is_zero(&self.mutation_id) {
            return Err(TerminalValidationError::MissingMutationId);
        }
        if is_zero(&self.payload_hash) {
            return Err(TerminalValidationError::MissingPayloadHash);
        }
        if is_zero(&self.server_plan_hash) {
            return Err(TerminalValidationError::MissingServerPlanHash);
        }
        if self.expected_state_version == u64::MAX {
            return Err(TerminalValidationError::StateVersionExhausted);
        }
        Ok(())
    }

    fn canonical_cmp(&self, other: &Self) -> Ordering {
        self.kind
            .priority()
            .cmp(&other.kind.priority())
            .then_with(|| self.terminal_id.cmp(&other.terminal_id))
            .then_with(|| self.mutation_id.cmp(&other.mutation_id))
            .then_with(|| self.payload_hash.cmp(&other.payload_hash))
            .then_with(|| self.server_plan_hash.cmp(&other.server_plan_hash))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalValidationError {
    MissingAccountId,
    MissingCharacterId,
    MissingLineageId,
    MissingRestorePointId,
    MissingTerminalId,
    MissingMutationId,
    MissingPayloadHash,
    MissingServerPlanHash,
    MissingResultHash,
    StateVersionExhausted,
    InvalidPostStateVersion,
    CommitBeforeObservation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NonTerminalAdmission {
    Allowed,
    BlockedByUnresolvedTerminal,
    BlockedByCommittedTerminal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubmitResult {
    Accepted {
        pending_tick: u64,
        candidate_count: usize,
        current_leader: TerminalCandidate,
    },
    ReplayedPending {
        candidate: TerminalCandidate,
    },
    ReplayedPrepared {
        prepared: PreparedTerminal,
    },
    ReplayedCommitted {
        receipt: StoredTerminalReceipt,
    },
    Rejected(SubmitRejection),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubmitRejection {
    Invalid(TerminalValidationError),
    BindingMismatch,
    IdempotencyConflict,
    TerminalIdConflict,
    AggregateVersionConflict { pending_version: u64 },
    EarlierTickUnresolved { pending_tick: u64 },
    StaleTick { pending_tick: u64 },
    TickAlreadySealed { sealed_tick: u64 },
    CandidateCapacityExhausted { capacity: usize },
    TerminalAlreadyCommitted { receipt: Box<StoredTerminalReceipt> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedTerminal {
    winner: TerminalCandidate,
    candidate_count: usize,
    sealed_through_tick: u64,
}

impl PreparedTerminal {
    pub const fn winner(&self) -> &TerminalCandidate {
        &self.winner
    }

    pub const fn candidate_count(&self) -> usize {
        self.candidate_count
    }

    pub const fn sealed_through_tick(&self) -> u64 {
        self.sealed_through_tick
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrepareError {
    NothingPending,
    TickNotSealed { pending_tick: u64 },
}

/// Append-only storage DTO for reconstruction across process restart. The
/// repository persists every field exactly and passes it through `from_storage`;
/// unknown schema/kind values and corrupt bindings fail closed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredTerminalReceiptV1 {
    pub schema_version: u16,
    pub account_id: [u8; TERMINAL_ID_BYTES],
    pub character_id: [u8; TERMINAL_ID_BYTES],
    pub lineage_id: [u8; TERMINAL_ID_BYTES],
    pub restore_point_id: [u8; TERMINAL_ID_BYTES],
    pub terminal_id: [u8; TERMINAL_ID_BYTES],
    pub mutation_id: [u8; TERMINAL_ID_BYTES],
    pub payload_hash: [u8; TERMINAL_HASH_BYTES],
    pub server_plan_hash: [u8; TERMINAL_HASH_BYTES],
    pub result_hash: [u8; TERMINAL_HASH_BYTES],
    pub expected_state_version: u64,
    pub post_state_version: u64,
    pub observed_tick: u64,
    pub committed_tick: u64,
    pub terminal_kind_code: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoredReceiptError {
    UnsupportedSchemaVersion(u16),
    UnknownTerminalKind(u8),
    Invalid(TerminalValidationError),
}

/// Complete durable replay authority. A repository stores this record in the
/// same transaction as the selected terminal mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredTerminalReceipt {
    binding: TerminalBinding,
    terminal_id: [u8; TERMINAL_ID_BYTES],
    mutation_id: [u8; TERMINAL_ID_BYTES],
    payload_hash: [u8; TERMINAL_HASH_BYTES],
    server_plan_hash: [u8; TERMINAL_HASH_BYTES],
    result_hash: [u8; TERMINAL_HASH_BYTES],
    expected_state_version: u64,
    post_state_version: u64,
    observed_tick: u64,
    committed_tick: u64,
    kind: TerminalKind,
}

impl StoredTerminalReceipt {
    pub fn from_prepared(
        prepared: &PreparedTerminal,
        committed_tick: u64,
        result_hash: [u8; TERMINAL_HASH_BYTES],
    ) -> Result<Self, TerminalValidationError> {
        let winner = prepared.winner();
        let post_state_version = winner
            .expected_state_version
            .checked_add(1)
            .ok_or(TerminalValidationError::StateVersionExhausted)?;
        let receipt = Self {
            binding: winner.binding,
            terminal_id: winner.terminal_id,
            mutation_id: winner.mutation_id,
            payload_hash: winner.payload_hash,
            server_plan_hash: winner.server_plan_hash,
            result_hash,
            expected_state_version: winner.expected_state_version,
            post_state_version,
            observed_tick: winner.observed_tick,
            committed_tick,
            kind: winner.kind,
        };
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn from_storage(stored: &StoredTerminalReceiptV1) -> Result<Self, StoredReceiptError> {
        if stored.schema_version != STORED_TERMINAL_RECEIPT_SCHEMA_V1 {
            return Err(StoredReceiptError::UnsupportedSchemaVersion(
                stored.schema_version,
            ));
        }
        let kind = TerminalKind::from_stable_code(stored.terminal_kind_code).ok_or(
            StoredReceiptError::UnknownTerminalKind(stored.terminal_kind_code),
        )?;
        let binding = TerminalBinding::new(
            stored.account_id,
            stored.character_id,
            stored.lineage_id,
            stored.restore_point_id,
        )
        .map_err(StoredReceiptError::Invalid)?;
        let receipt = Self {
            binding,
            terminal_id: stored.terminal_id,
            mutation_id: stored.mutation_id,
            payload_hash: stored.payload_hash,
            server_plan_hash: stored.server_plan_hash,
            result_hash: stored.result_hash,
            expected_state_version: stored.expected_state_version,
            post_state_version: stored.post_state_version,
            observed_tick: stored.observed_tick,
            committed_tick: stored.committed_tick,
            kind,
        };
        receipt.validate().map_err(StoredReceiptError::Invalid)?;
        Ok(receipt)
    }

    pub fn to_storage_v1(&self) -> StoredTerminalReceiptV1 {
        StoredTerminalReceiptV1 {
            schema_version: STORED_TERMINAL_RECEIPT_SCHEMA_V1,
            account_id: self.binding.account,
            character_id: self.binding.character,
            lineage_id: self.binding.lineage,
            restore_point_id: self.binding.restore_point,
            terminal_id: self.terminal_id,
            mutation_id: self.mutation_id,
            payload_hash: self.payload_hash,
            server_plan_hash: self.server_plan_hash,
            result_hash: self.result_hash,
            expected_state_version: self.expected_state_version,
            post_state_version: self.post_state_version,
            observed_tick: self.observed_tick,
            committed_tick: self.committed_tick,
            terminal_kind_code: self.kind.stable_code(),
        }
    }

    pub fn validate(&self) -> Result<(), TerminalValidationError> {
        TerminalCandidate {
            binding: self.binding,
            terminal_id: self.terminal_id,
            mutation_id: self.mutation_id,
            payload_hash: self.payload_hash,
            server_plan_hash: self.server_plan_hash,
            expected_state_version: self.expected_state_version,
            observed_tick: self.observed_tick,
            kind: self.kind,
        }
        .validate()?;
        if is_zero(&self.result_hash) {
            return Err(TerminalValidationError::MissingResultHash);
        }
        let expected_post = self
            .expected_state_version
            .checked_add(1)
            .ok_or(TerminalValidationError::StateVersionExhausted)?;
        if self.post_state_version != expected_post {
            return Err(TerminalValidationError::InvalidPostStateVersion);
        }
        if self.committed_tick < self.observed_tick {
            return Err(TerminalValidationError::CommitBeforeObservation);
        }
        Ok(())
    }

    pub const fn binding(&self) -> TerminalBinding {
        self.binding
    }

    pub const fn terminal_id(&self) -> &[u8; TERMINAL_ID_BYTES] {
        &self.terminal_id
    }

    pub const fn mutation_id(&self) -> &[u8; TERMINAL_ID_BYTES] {
        &self.mutation_id
    }

    pub const fn payload_hash(&self) -> &[u8; TERMINAL_HASH_BYTES] {
        &self.payload_hash
    }

    pub const fn server_plan_hash(&self) -> &[u8; TERMINAL_HASH_BYTES] {
        &self.server_plan_hash
    }

    pub const fn result_hash(&self) -> &[u8; TERMINAL_HASH_BYTES] {
        &self.result_hash
    }

    pub const fn expected_state_version(&self) -> u64 {
        self.expected_state_version
    }

    pub const fn post_state_version(&self) -> u64 {
        self.post_state_version
    }

    pub const fn observed_tick(&self) -> u64 {
        self.observed_tick
    }

    pub const fn committed_tick(&self) -> u64 {
        self.committed_tick
    }

    pub const fn kind(&self) -> TerminalKind {
        self.kind
    }

    fn matches_candidate(&self, candidate: &TerminalCandidate) -> bool {
        self.binding == candidate.binding
            && self.terminal_id == candidate.terminal_id
            && self.mutation_id == candidate.mutation_id
            && self.payload_hash == candidate.payload_hash
            && self.server_plan_hash == candidate.server_plan_hash
            && self.expected_state_version == candidate.expected_state_version
            && self.observed_tick == candidate.observed_tick
            && self.kind == candidate.kind
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommitResult {
    Committed(StoredTerminalReceipt),
    Replayed(StoredTerminalReceipt),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitError {
    InvalidReceipt(TerminalValidationError),
    NotPrepared,
    ReceiptDoesNotMatchPreparedWinner,
    ImmutableOutcome,
}

#[derive(Clone, Debug)]
enum ArbiterState {
    Open,
    Pending {
        tick: u64,
        candidates: Vec<TerminalCandidate>,
    },
    Prepared(PreparedTerminal),
    Committed(StoredTerminalReceipt),
}

/// Single-character terminal aggregate. A server keeps one instance behind the
/// same single-writer ownership used for the durable character aggregate.
#[derive(Clone, Debug)]
pub struct TerminalArbiter {
    binding: TerminalBinding,
    state: ArbiterState,
}

impl TerminalArbiter {
    pub const fn new(binding: TerminalBinding) -> Self {
        Self {
            binding,
            state: ArbiterState::Open,
        }
    }

    /// Reconstructs final authority after response loss or process restart.
    pub fn from_stored_receipt(
        receipt: StoredTerminalReceipt,
    ) -> Result<Self, TerminalValidationError> {
        receipt.validate()?;
        Ok(Self {
            binding: receipt.binding,
            state: ArbiterState::Committed(receipt),
        })
    }

    pub const fn binding(&self) -> TerminalBinding {
        self.binding
    }

    /// Gates ordinary departures and every nonterminal character mutation.
    pub const fn non_terminal_admission(&self) -> NonTerminalAdmission {
        match self.state {
            ArbiterState::Open => NonTerminalAdmission::Allowed,
            ArbiterState::Pending { .. } | ArbiterState::Prepared(_) => {
                NonTerminalAdmission::BlockedByUnresolvedTerminal
            }
            ArbiterState::Committed(_) => NonTerminalAdmission::BlockedByCommittedTerminal,
        }
    }

    pub fn committed_receipt(&self) -> Option<&StoredTerminalReceipt> {
        match &self.state {
            ArbiterState::Committed(receipt) => Some(receipt),
            ArbiterState::Open | ArbiterState::Pending { .. } | ArbiterState::Prepared(_) => None,
        }
    }

    /// Returns the frozen winner only while repository execution is unresolved.
    pub fn prepared_terminal(&self) -> Option<&PreparedTerminal> {
        match &self.state {
            ArbiterState::Prepared(prepared) => Some(prepared),
            ArbiterState::Open | ArbiterState::Pending { .. } | ArbiterState::Committed(_) => None,
        }
    }

    pub fn submit(&mut self, candidate: TerminalCandidate) -> SubmitResult {
        if let Err(error) = candidate.validate() {
            return SubmitResult::Rejected(SubmitRejection::Invalid(error));
        }
        if candidate.binding != self.binding {
            return SubmitResult::Rejected(SubmitRejection::BindingMismatch);
        }

        match &mut self.state {
            ArbiterState::Open => {
                let tick = candidate.observed_tick;
                let leader = candidate.clone();
                self.state = ArbiterState::Pending {
                    tick,
                    candidates: vec![candidate],
                };
                SubmitResult::Accepted {
                    pending_tick: tick,
                    candidate_count: 1,
                    current_leader: leader,
                }
            }
            ArbiterState::Pending { tick, candidates } => {
                Self::submit_pending(*tick, candidates, candidate)
            }
            ArbiterState::Prepared(prepared) => Self::submit_prepared(prepared, &candidate),
            ArbiterState::Committed(receipt) => Self::submit_committed(receipt, &candidate),
        }
    }

    fn submit_pending(
        tick: u64,
        candidates: &mut Vec<TerminalCandidate>,
        candidate: TerminalCandidate,
    ) -> SubmitResult {
        if let Some(existing) = candidates
            .iter()
            .find(|existing| existing.mutation_id == candidate.mutation_id)
        {
            return if existing == &candidate {
                SubmitResult::ReplayedPending {
                    candidate: existing.clone(),
                }
            } else {
                SubmitResult::Rejected(SubmitRejection::IdempotencyConflict)
            };
        }
        if candidates
            .iter()
            .any(|existing| existing.terminal_id == candidate.terminal_id)
        {
            return SubmitResult::Rejected(SubmitRejection::TerminalIdConflict);
        }
        if candidate.observed_tick != tick {
            let rejection = if candidate.observed_tick < tick {
                SubmitRejection::StaleTick { pending_tick: tick }
            } else {
                SubmitRejection::EarlierTickUnresolved { pending_tick: tick }
            };
            return SubmitResult::Rejected(rejection);
        }
        let pending_version = candidates
            .first()
            .expect("pending terminal state must contain a candidate")
            .expected_state_version;
        if candidate.expected_state_version != pending_version {
            return SubmitResult::Rejected(SubmitRejection::AggregateVersionConflict {
                pending_version,
            });
        }
        if candidates.len() == MAX_TERMINAL_CANDIDATES_PER_TICK {
            if let Err(rejection) = Self::replace_for_lethal_overflow(candidates, candidate) {
                return SubmitResult::Rejected(rejection);
            }
        } else {
            candidates.push(candidate);
        }
        let leader = candidates
            .iter()
            .min_by(|left, right| left.canonical_cmp(right))
            .expect("a just-appended terminal candidate must exist")
            .clone();
        SubmitResult::Accepted {
            pending_tick: tick,
            candidate_count: candidates.len(),
            current_leader: leader,
        }
    }

    fn replace_for_lethal_overflow(
        candidates: &mut [TerminalCandidate],
        candidate: TerminalCandidate,
    ) -> Result<(), SubmitRejection> {
        let has_lethal = candidates
            .iter()
            .any(|existing| existing.kind == TerminalKind::LethalDeath);
        if candidate.kind != TerminalKind::LethalDeath || has_lethal {
            return Err(SubmitRejection::CandidateCapacityExhausted {
                capacity: MAX_TERMINAL_CANDIDATES_PER_TICK,
            });
        }
        // Preserve absolute lethal precedence under defensive queue exhaustion.
        // Ordinary additions stay fail-closed once full, so an evicted request
        // cannot later re-enter with altered authority.
        let (worst_index, _) = candidates
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| left.canonical_cmp(right))
            .expect("a full terminal candidate set must not be empty");
        candidates[worst_index] = candidate;
        Ok(())
    }

    fn submit_prepared(prepared: &PreparedTerminal, candidate: &TerminalCandidate) -> SubmitResult {
        if prepared.winner == *candidate {
            return SubmitResult::ReplayedPrepared {
                prepared: prepared.clone(),
            };
        }
        let rejection = if prepared.winner.mutation_id == candidate.mutation_id {
            SubmitRejection::IdempotencyConflict
        } else if prepared.winner.terminal_id == candidate.terminal_id {
            SubmitRejection::TerminalIdConflict
        } else {
            SubmitRejection::TickAlreadySealed {
                sealed_tick: prepared.sealed_through_tick,
            }
        };
        SubmitResult::Rejected(rejection)
    }

    fn submit_committed(
        receipt: &StoredTerminalReceipt,
        candidate: &TerminalCandidate,
    ) -> SubmitResult {
        if receipt.mutation_id == candidate.mutation_id {
            return if receipt.matches_candidate(candidate) {
                SubmitResult::ReplayedCommitted {
                    receipt: receipt.clone(),
                }
            } else {
                SubmitResult::Rejected(SubmitRejection::IdempotencyConflict)
            };
        }
        if receipt.terminal_id == candidate.terminal_id {
            SubmitResult::Rejected(SubmitRejection::TerminalIdConflict)
        } else {
            SubmitResult::Rejected(SubmitRejection::TerminalAlreadyCommitted {
                receipt: Box::new(receipt.clone()),
            })
        }
    }

    /// Seals every producer phase through `sealed_through_tick` and freezes the
    /// deterministic winner for repository execution. No candidate may be
    /// admitted after this call, including a late lethal proposal.
    pub fn prepare(&mut self, sealed_through_tick: u64) -> Result<PreparedTerminal, PrepareError> {
        match &self.state {
            ArbiterState::Pending { tick, .. } if *tick > sealed_through_tick => {
                Err(PrepareError::TickNotSealed {
                    pending_tick: *tick,
                })
            }
            ArbiterState::Pending { candidates, .. } => {
                let winner = candidates
                    .iter()
                    .min_by(|left, right| left.canonical_cmp(right))
                    .expect("pending terminal state must contain a candidate")
                    .clone();
                let prepared = PreparedTerminal {
                    winner,
                    candidate_count: candidates.len(),
                    sealed_through_tick,
                };
                self.state = ArbiterState::Prepared(prepared.clone());
                Ok(prepared)
            }
            ArbiterState::Prepared(prepared) => Ok(prepared.clone()),
            ArbiterState::Open | ArbiterState::Committed(_) => Err(PrepareError::NothingPending),
        }
    }

    /// Publishes only the exact receipt produced for the prepared winner.
    pub fn record_commit(
        &mut self,
        receipt: StoredTerminalReceipt,
    ) -> Result<CommitResult, CommitError> {
        if let Err(error) = receipt.validate() {
            return Err(CommitError::InvalidReceipt(error));
        }
        match &self.state {
            ArbiterState::Prepared(prepared) => {
                if !receipt.matches_candidate(&prepared.winner) {
                    return Err(CommitError::ReceiptDoesNotMatchPreparedWinner);
                }
                self.state = ArbiterState::Committed(receipt.clone());
                Ok(CommitResult::Committed(receipt))
            }
            ArbiterState::Committed(existing) if existing == &receipt => {
                Ok(CommitResult::Replayed(existing.clone()))
            }
            ArbiterState::Committed(_) => Err(CommitError::ImmutableOutcome),
            ArbiterState::Open | ArbiterState::Pending { .. } => Err(CommitError::NotPrepared),
        }
    }
}

fn is_zero<const N: usize>(bytes: &[u8; N]) -> bool {
    bytes.iter().all(|byte| *byte == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [TerminalKind; 5] = [
        TerminalKind::LethalDeath,
        TerminalKind::SuccessfulExtraction,
        TerminalKind::EmergencyRecall,
        TerminalKind::DisconnectRecovery,
        TerminalKind::VerifiedServerFaultRestoration,
    ];

    fn binding() -> TerminalBinding {
        TerminalBinding::new([1; 16], [2; 16], [3; 16], [4; 16]).expect("valid binding")
    }

    fn candidate(kind: TerminalKind, discriminator: u8, tick: u64) -> TerminalCandidate {
        TerminalCandidate::from_server_plan(
            binding(),
            [discriminator; 16],
            [discriminator.wrapping_add(40); 16],
            [discriminator.wrapping_add(80); 32],
            [discriminator.wrapping_add(120); 32],
            7,
            tick,
            kind,
        )
        .expect("valid candidate")
    }

    fn prepare_and_receipt(
        arbiter: &mut TerminalArbiter,
        sealed_tick: u64,
    ) -> (PreparedTerminal, StoredTerminalReceipt) {
        let prepared = arbiter.prepare(sealed_tick).expect("prepare succeeds");
        let receipt = StoredTerminalReceipt::from_prepared(&prepared, sealed_tick, [231; 32])
            .expect("receipt succeeds");
        (prepared, receipt)
    }

    #[test]
    fn all_pairwise_same_tick_races_are_order_independent() {
        for (left_index, left_kind) in KINDS.iter().copied().enumerate() {
            for (right_index, right_kind) in KINDS.iter().copied().enumerate() {
                let left = candidate(left_kind, (left_index + 1) as u8, 50);
                let right = candidate(right_kind, (right_index + 11) as u8, 50);
                let expected = if left.canonical_cmp(&right).is_le() {
                    left.clone()
                } else {
                    right.clone()
                };

                for proposals in [[left.clone(), right.clone()], [right.clone(), left.clone()]] {
                    let mut arbiter = TerminalArbiter::new(binding());
                    for proposal in proposals {
                        assert!(matches!(
                            arbiter.submit(proposal),
                            SubmitResult::Accepted { .. }
                        ));
                    }
                    let prepared = arbiter.prepare(50).expect("tick is sealed");
                    assert_eq!(prepared.winner(), &expected);
                    assert_eq!(prepared.candidate_count(), 2);
                }
            }
        }
    }

    #[test]
    fn lethal_death_wins_every_same_tick_terminal_race() {
        for (index, kind) in KINDS.iter().copied().enumerate().skip(1) {
            let mut arbiter = TerminalArbiter::new(binding());
            arbiter.submit(candidate(kind, (index + 10) as u8, 80));
            arbiter.submit(candidate(TerminalKind::LethalDeath, 30, 80));
            assert_eq!(
                arbiter.prepare(80).expect("prepare").winner().kind(),
                TerminalKind::LethalDeath
            );
        }
    }

    #[test]
    fn full_same_tick_set_has_stable_priority_in_every_insertion_rotation() {
        for rotation in 0..KINDS.len() {
            let mut arbiter = TerminalArbiter::new(binding());
            for offset in 0..KINDS.len() {
                let index = (rotation + offset) % KINDS.len();
                arbiter.submit(candidate(KINDS[index], (index + 1) as u8, 90));
            }
            let prepared = arbiter.prepare(90).expect("prepare");
            assert_eq!(prepared.winner().kind(), TerminalKind::LethalDeath);
            assert_eq!(prepared.candidate_count(), KINDS.len());
        }
    }

    #[test]
    fn earlier_unresolved_tick_blocks_later_tick_and_stale_arrivals() {
        let mut arbiter = TerminalArbiter::new(binding());
        arbiter.submit(candidate(TerminalKind::EmergencyRecall, 1, 10));
        assert_eq!(
            arbiter.submit(candidate(TerminalKind::LethalDeath, 2, 11)),
            SubmitResult::Rejected(SubmitRejection::EarlierTickUnresolved { pending_tick: 10 })
        );
        assert_eq!(
            arbiter.submit(candidate(TerminalKind::LethalDeath, 3, 9)),
            SubmitResult::Rejected(SubmitRejection::StaleTick { pending_tick: 10 })
        );
        assert_eq!(
            arbiter.prepare(9),
            Err(PrepareError::TickNotSealed { pending_tick: 10 })
        );
        assert_eq!(
            arbiter
                .prepare(10)
                .expect("tick ten sealed")
                .winner()
                .kind(),
            TerminalKind::EmergencyRecall
        );
    }

    #[test]
    fn same_tick_candidates_must_bind_the_same_aggregate_version() {
        let mut arbiter = TerminalArbiter::new(binding());
        arbiter.submit(candidate(TerminalKind::EmergencyRecall, 1, 10));
        let mut stale = candidate(TerminalKind::LethalDeath, 2, 10);
        stale.expected_state_version = 6;
        assert_eq!(
            arbiter.submit(stale),
            SubmitResult::Rejected(SubmitRejection::AggregateVersionConflict {
                pending_version: 7
            })
        );
    }

    #[test]
    fn exact_pending_and_committed_retries_replay_but_changed_payload_conflicts() {
        let original = candidate(TerminalKind::SuccessfulExtraction, 7, 40);
        let mut arbiter = TerminalArbiter::new(binding());
        assert!(matches!(
            arbiter.submit(original.clone()),
            SubmitResult::Accepted { .. }
        ));
        assert_eq!(
            arbiter.submit(original.clone()),
            SubmitResult::ReplayedPending {
                candidate: original.clone()
            }
        );

        let mut changed = original.clone();
        changed.payload_hash = [99; 32];
        assert_eq!(
            arbiter.submit(changed.clone()),
            SubmitResult::Rejected(SubmitRejection::IdempotencyConflict)
        );

        let (_, receipt) = prepare_and_receipt(&mut arbiter, 40);
        arbiter.record_commit(receipt.clone()).expect("commit");
        assert_eq!(
            arbiter.submit(original),
            SubmitResult::ReplayedCommitted {
                receipt: receipt.clone()
            }
        );
        assert_eq!(
            arbiter.submit(changed),
            SubmitResult::Rejected(SubmitRejection::IdempotencyConflict)
        );
    }

    #[test]
    fn unresolved_and_committed_terminal_states_block_normal_mutations() {
        let mut arbiter = TerminalArbiter::new(binding());
        assert_eq!(
            arbiter.non_terminal_admission(),
            NonTerminalAdmission::Allowed
        );
        arbiter.submit(candidate(TerminalKind::EmergencyRecall, 1, 22));
        assert_eq!(
            arbiter.non_terminal_admission(),
            NonTerminalAdmission::BlockedByUnresolvedTerminal
        );
        let (_, receipt) = prepare_and_receipt(&mut arbiter, 22);
        assert_eq!(
            arbiter.non_terminal_admission(),
            NonTerminalAdmission::BlockedByUnresolvedTerminal
        );
        arbiter.record_commit(receipt).expect("commit");
        assert_eq!(
            arbiter.non_terminal_admission(),
            NonTerminalAdmission::BlockedByCommittedTerminal
        );
    }

    #[test]
    fn response_loss_and_restart_reconstruct_exact_committed_authority() {
        let original = candidate(TerminalKind::DisconnectRecovery, 5, 100);
        let mut arbiter = TerminalArbiter::new(binding());
        arbiter.submit(original.clone());
        let (_, receipt) = prepare_and_receipt(&mut arbiter, 101);
        arbiter.record_commit(receipt.clone()).expect("commit");

        let mut restored = TerminalArbiter::from_stored_receipt(receipt.clone()).expect("restore");
        assert_eq!(restored.committed_receipt(), Some(&receipt));
        assert_eq!(
            restored.submit(original),
            SubmitResult::ReplayedCommitted { receipt }
        );
    }

    #[test]
    fn storage_v1_round_trip_preserves_every_replay_field_and_stable_kind_code() {
        for (index, kind) in KINDS.iter().copied().enumerate() {
            assert_eq!(
                TerminalKind::from_stable_code(kind.stable_code()),
                Some(kind)
            );
            assert_eq!(kind.stable_code(), (index + 1) as u8);

            let mut arbiter = TerminalArbiter::new(binding());
            arbiter.submit(candidate(kind, (index + 1) as u8, 100));
            let (_, receipt) = prepare_and_receipt(&mut arbiter, 101);
            let stored = receipt.to_storage_v1();
            assert_eq!(stored.schema_version, STORED_TERMINAL_RECEIPT_SCHEMA_V1);
            assert_eq!(StoredTerminalReceipt::from_storage(&stored), Ok(receipt));
        }
        assert_eq!(TerminalKind::from_stable_code(0), None);
        assert_eq!(TerminalKind::from_stable_code(6), None);
    }

    #[test]
    fn corrupt_or_unknown_stored_receipt_fields_fail_closed() {
        let mut arbiter = TerminalArbiter::new(binding());
        arbiter.submit(candidate(TerminalKind::LethalDeath, 1, 10));
        let (_, receipt) = prepare_and_receipt(&mut arbiter, 10);
        let valid = receipt.to_storage_v1();

        let mut corrupt = valid.clone();
        corrupt.schema_version = 2;
        assert_eq!(
            StoredTerminalReceipt::from_storage(&corrupt),
            Err(StoredReceiptError::UnsupportedSchemaVersion(2))
        );

        let mut corrupt = valid.clone();
        corrupt.terminal_kind_code = 255;
        assert_eq!(
            StoredTerminalReceipt::from_storage(&corrupt),
            Err(StoredReceiptError::UnknownTerminalKind(255))
        );

        let corruptions = [
            (0, TerminalValidationError::MissingAccountId),
            (1, TerminalValidationError::MissingCharacterId),
            (2, TerminalValidationError::MissingLineageId),
            (3, TerminalValidationError::MissingRestorePointId),
            (4, TerminalValidationError::MissingTerminalId),
            (5, TerminalValidationError::MissingMutationId),
            (6, TerminalValidationError::MissingPayloadHash),
            (7, TerminalValidationError::MissingServerPlanHash),
            (8, TerminalValidationError::MissingResultHash),
        ];
        for (field, expected) in corruptions {
            let mut corrupt = valid.clone();
            match field {
                0 => corrupt.account_id = [0; 16],
                1 => corrupt.character_id = [0; 16],
                2 => corrupt.lineage_id = [0; 16],
                3 => corrupt.restore_point_id = [0; 16],
                4 => corrupt.terminal_id = [0; 16],
                5 => corrupt.mutation_id = [0; 16],
                6 => corrupt.payload_hash = [0; 32],
                7 => corrupt.server_plan_hash = [0; 32],
                8 => corrupt.result_hash = [0; 32],
                _ => unreachable!("corruption table is exhaustive"),
            }
            assert_eq!(
                StoredTerminalReceipt::from_storage(&corrupt),
                Err(StoredReceiptError::Invalid(expected))
            );
        }

        let mut corrupt = valid.clone();
        corrupt.post_state_version += 1;
        assert_eq!(
            StoredTerminalReceipt::from_storage(&corrupt),
            Err(StoredReceiptError::Invalid(
                TerminalValidationError::InvalidPostStateVersion
            ))
        );

        let mut corrupt = valid;
        corrupt.committed_tick = corrupt.observed_tick - 1;
        assert_eq!(
            StoredTerminalReceipt::from_storage(&corrupt),
            Err(StoredReceiptError::Invalid(
                TerminalValidationError::CommitBeforeObservation
            ))
        );
    }

    #[test]
    fn committed_outcome_is_immutable_even_for_a_late_same_tick_death() {
        let mut arbiter = TerminalArbiter::new(binding());
        arbiter.submit(candidate(TerminalKind::EmergencyRecall, 1, 70));
        let (_, receipt) = prepare_and_receipt(&mut arbiter, 70);
        arbiter.record_commit(receipt.clone()).expect("commit");

        assert!(matches!(
            arbiter.submit(candidate(TerminalKind::LethalDeath, 2, 70)),
            SubmitResult::Rejected(SubmitRejection::TerminalAlreadyCommitted {
                receipt: stored
            }) if *stored == receipt
        ));
    }

    #[test]
    fn prepared_tick_cannot_accept_late_producers_or_a_different_receipt() {
        let original = candidate(TerminalKind::EmergencyRecall, 1, 70);
        let mut arbiter = TerminalArbiter::new(binding());
        arbiter.submit(original.clone());
        let (prepared, _) = prepare_and_receipt(&mut arbiter, 70);
        assert_eq!(
            arbiter.submit(original),
            SubmitResult::ReplayedPrepared {
                prepared: prepared.clone()
            }
        );
        assert!(matches!(
            arbiter.submit(candidate(TerminalKind::LethalDeath, 2, 70)),
            SubmitResult::Rejected(SubmitRejection::TickAlreadySealed { sealed_tick: 70 })
        ));

        let mut wrong = StoredTerminalReceipt::from_prepared(&prepared, 70, [9; 32])
            .expect("valid receipt shape");
        wrong.terminal_id = [88; 16];
        assert_eq!(
            arbiter.record_commit(wrong),
            Err(CommitError::ReceiptDoesNotMatchPreparedWinner)
        );
    }

    #[test]
    fn terminal_and_mutation_id_collisions_fail_closed() {
        let original = candidate(TerminalKind::EmergencyRecall, 4, 30);
        let mut arbiter = TerminalArbiter::new(binding());
        arbiter.submit(original.clone());

        let mut same_terminal = candidate(TerminalKind::LethalDeath, 5, 30);
        same_terminal.terminal_id = original.terminal_id;
        assert_eq!(
            arbiter.submit(same_terminal),
            SubmitResult::Rejected(SubmitRejection::TerminalIdConflict)
        );

        let mut same_mutation = candidate(TerminalKind::LethalDeath, 6, 30);
        same_mutation.mutation_id = original.mutation_id;
        assert_eq!(
            arbiter.submit(same_mutation),
            SubmitResult::Rejected(SubmitRejection::IdempotencyConflict)
        );
    }

    #[test]
    fn bounded_capacity_reserves_lethal_precedence_and_other_overflow_fails_closed() {
        let mut arbiter = TerminalArbiter::new(binding());
        for index in 0..MAX_TERMINAL_CANDIDATES_PER_TICK {
            let discriminator = (index + 1) as u8;
            assert!(matches!(
                arbiter.submit(candidate(
                    TerminalKind::VerifiedServerFaultRestoration,
                    discriminator,
                    12
                )),
                SubmitResult::Accepted { .. }
            ));
        }
        assert!(matches!(
            arbiter.submit(candidate(TerminalKind::LethalDeath, 200, 12)),
            SubmitResult::Accepted {
                candidate_count: MAX_TERMINAL_CANDIDATES_PER_TICK,
                current_leader,
                ..
            } if current_leader.kind() == TerminalKind::LethalDeath
        ));
        assert_eq!(
            arbiter.submit(candidate(
                TerminalKind::VerifiedServerFaultRestoration,
                201,
                12
            )),
            SubmitResult::Rejected(SubmitRejection::CandidateCapacityExhausted {
                capacity: MAX_TERMINAL_CANDIDATES_PER_TICK
            })
        );
        let prepared = arbiter.prepare(12).expect("prepare");
        assert_eq!(prepared.candidate_count(), MAX_TERMINAL_CANDIDATES_PER_TICK);
        assert_eq!(prepared.winner().kind(), TerminalKind::LethalDeath);
    }

    #[test]
    fn exhausted_state_version_and_malformed_authority_are_rejected() {
        assert_eq!(
            TerminalCandidate::from_server_plan(
                binding(),
                [3; 16],
                [4; 16],
                [5; 32],
                [6; 32],
                u64::MAX,
                u64::MAX,
                TerminalKind::LethalDeath,
            ),
            Err(TerminalValidationError::StateVersionExhausted)
        );
        assert_eq!(
            TerminalBinding::new([0; 16], [2; 16], [3; 16], [4; 16]),
            Err(TerminalValidationError::MissingAccountId)
        );
        assert_eq!(
            TerminalBinding::new([1; 16], [0; 16], [3; 16], [4; 16]),
            Err(TerminalValidationError::MissingCharacterId)
        );
        assert_eq!(
            TerminalBinding::new([1; 16], [2; 16], [0; 16], [4; 16]),
            Err(TerminalValidationError::MissingLineageId)
        );
        assert_eq!(
            TerminalBinding::new([1; 16], [2; 16], [3; 16], [0; 16]),
            Err(TerminalValidationError::MissingRestorePointId)
        );
    }

    #[test]
    fn receipt_rejects_zero_result_and_commit_before_observation() {
        let mut arbiter = TerminalArbiter::new(binding());
        arbiter.submit(candidate(TerminalKind::LethalDeath, 1, 10));
        let prepared = arbiter.prepare(10).expect("prepare");
        assert_eq!(
            StoredTerminalReceipt::from_prepared(&prepared, 10, [0; 32]),
            Err(TerminalValidationError::MissingResultHash)
        );
        assert_eq!(
            StoredTerminalReceipt::from_prepared(&prepared, 9, [1; 32]),
            Err(TerminalValidationError::CommitBeforeObservation)
        );
    }

    #[test]
    fn commit_is_idempotent_and_cannot_be_replaced() {
        let mut arbiter = TerminalArbiter::new(binding());
        arbiter.submit(candidate(TerminalKind::SuccessfulExtraction, 1, 10));
        let (prepared, receipt) = prepare_and_receipt(&mut arbiter, 10);
        assert_eq!(
            arbiter.record_commit(receipt.clone()),
            Ok(CommitResult::Committed(receipt.clone()))
        );
        assert_eq!(
            arbiter.record_commit(receipt.clone()),
            Ok(CommitResult::Replayed(receipt))
        );

        let replacement = StoredTerminalReceipt::from_prepared(&prepared, 11, [2; 32])
            .expect("other valid receipt");
        assert_eq!(
            arbiter.record_commit(replacement),
            Err(CommitError::ImmutableOutcome)
        );
    }

    #[test]
    fn foreign_binding_is_rejected_before_it_can_join_a_tick() {
        let foreign = TerminalBinding::new([9; 16], [8; 16], [7; 16], [6; 16])
            .expect("valid foreign binding");
        let foreign_candidate = TerminalCandidate::from_server_plan(
            foreign,
            [1; 16],
            [2; 16],
            [3; 32],
            [4; 32],
            1,
            1,
            TerminalKind::LethalDeath,
        )
        .expect("valid candidate shape");
        let mut arbiter = TerminalArbiter::new(binding());
        assert_eq!(
            arbiter.submit(foreign_candidate),
            SubmitResult::Rejected(SubmitRejection::BindingMismatch)
        );
        assert_eq!(
            arbiter.non_terminal_admission(),
            NonTerminalAdmission::Allowed
        );
    }

    #[test]
    fn stale_danger_lineage_or_restore_point_is_rejected_for_the_same_character() {
        for stale_binding in [
            TerminalBinding::new([1; 16], [2; 16], [9; 16], [4; 16])
                .expect("valid stale lineage binding"),
            TerminalBinding::new([1; 16], [2; 16], [3; 16], [9; 16])
                .expect("valid stale restore binding"),
        ] {
            let proposal = TerminalCandidate::from_server_plan(
                stale_binding,
                [10; 16],
                [11; 16],
                [12; 32],
                [13; 32],
                7,
                20,
                TerminalKind::LethalDeath,
            )
            .expect("valid candidate shape");
            let mut arbiter = TerminalArbiter::new(binding());
            assert_eq!(
                arbiter.submit(proposal),
                SubmitResult::Rejected(SubmitRejection::BindingMismatch)
            );
        }
    }
}
