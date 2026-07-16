//! Arbiter-gated execution of one sealed `GB-M03-08` Emergency Recall.
//!
//! Authorities: `Gravebound_Production_GDD_v1_Canonical.md` (`DTH-010`,
//! `LOOT-002`, `LOOT-033`, `TECH-015`, and `TECH-021`-`023`),
//! `Gravebound_Content_Production_Spec_v1.md` (`CONT-HUB-001`/`002` and the
//! Core dangerous-route Recall contract), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`/`08`). Accepted
//! `SPEC-CONFLICT-029` requires explicit and `LinkLost` Recall to share one
//! server-planned loss writer and the five-producer terminal arbiter.

use std::future::Future;

use persistence::{
    PersistenceError, PostgresPersistence, PreparedProductionRecallV1,
    ProductionRecallCommitRequestV1, ProductionRecallTransactionV1, ProductionRecallTriggerV1,
    StoredCommittedRecallTerminalV1, StoredProductionRecallResultV1,
};
use protocol::{
    CharacterLocation, CharacterLocationSnapshot, RecallFrameV1, RecallResultV1,
    RecallTerminalTriggerV1, SafeArrival, StoredRecallTerminalResultV1,
    TERMINAL_INVENTORY_SCHEMA_VERSION, TerminalInventoryRejectionCodeV1, TerminalVersionAdvanceV1,
    TerminalVersionVectorV1, WireText,
};
use thiserror::Error;

use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CommitError, CommitResult,
    CoreTerminalCoordinator, PreparedTerminal, STORED_TERMINAL_RECEIPT_SCHEMA_V1,
    StoredTerminalReceipt, StoredTerminalReceiptV1, TerminalArbiter, TerminalBinding,
    TerminalCandidate, TerminalKind, TerminalValidationError,
};

/// Reliable transport boundary. Normal Core admission keeps this disabled until
/// the live character actor supplies channel, clock, identity, and coordinator authority.
#[derive(Debug, Clone, Copy, Default)]
pub enum CoreRecallTerminalAuthority {
    #[default]
    Disabled,
}

impl CoreRecallTerminalAuthority {
    #[must_use]
    pub const fn disabled() -> Self {
        Self::Disabled
    }

    pub fn handle(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &RecallFrameV1,
    ) -> RecallResultV1 {
        let code = match frame.validate() {
            Err(_) => TerminalInventoryRejectionCodeV1::InvalidRequest,
            Ok(()) if authenticated.namespace != AuthenticatedNamespace::WipeableTest => {
                TerminalInventoryRejectionCodeV1::ForeignAuthority
            }
            Ok(()) => TerminalInventoryRejectionCodeV1::FeatureDisabled,
        };
        RecallResultV1::Rejected {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            request_sequence: frame.sequence,
            character_id: frame.character_id,
            code,
        }
    }
}

pub trait ProductionRecallWriter: Send + Sync {
    fn commit(
        &self,
        request: &ProductionRecallCommitRequestV1,
        expected_plan_hash: [u8; 32],
    ) -> impl Future<Output = Result<ProductionRecallTransactionV1, PersistenceError>> + Send;
}

pub trait ProductionRecallTerminalReader: Send + Sync {
    fn load_committed_terminal(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> impl Future<Output = Result<Option<StoredCommittedRecallTerminalV1>, PersistenceError>> + Send;
}

impl ProductionRecallWriter for PostgresPersistence {
    async fn commit(
        &self,
        request: &ProductionRecallCommitRequestV1,
        expected_plan_hash: [u8; 32],
    ) -> Result<ProductionRecallTransactionV1, PersistenceError> {
        self.commit_production_recall_v1(request, expected_plan_hash)
            .await
    }
}

impl ProductionRecallTerminalReader for PostgresPersistence {
    async fn load_committed_terminal(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
    ) -> Result<Option<StoredCommittedRecallTerminalV1>, PersistenceError> {
        self.load_committed_recall_terminal_v1(account_id, character_id)
            .await
    }
}

#[derive(Debug, Clone)]
pub struct ProductionRecallExecutionService<Writer> {
    writer: Writer,
}

impl<Writer> ProductionRecallExecutionService<Writer> {
    #[must_use]
    pub const fn new(writer: Writer) -> Self {
        Self { writer }
    }
}

pub type PostgresProductionRecallExecutionService =
    ProductionRecallExecutionService<PostgresPersistence>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductionRecallExecutionOutcome {
    pub transaction: ProductionRecallTransactionV1,
    pub terminal_commit: CommitResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductionRecallReplayOutcome {
    pub transaction: ProductionRecallTransactionV1,
    pub receipt: StoredTerminalReceipt,
}

#[derive(Debug, Error)]
pub enum ProductionRecallExecutionError {
    #[error("sealed Recall could not form terminal authority: {0:?}")]
    InvalidTerminalAuthority(TerminalValidationError),
    #[error("prepared Recall authority is invalid")]
    InvalidPreparedAuthority(#[source] PersistenceError),
    #[error("an already committed Recall must use durable replay recovery")]
    CommittedReplayRequiresRecovery,
    #[error("durable replay expected the repository to return the stored Recall")]
    ExpectedStoredReplay,
    #[error("prepared terminal winner is not the sealed Recall")]
    PreparedWinnerMismatch,
    #[error("production Recall repository rejected the transaction")]
    Persistence(#[source] PersistenceError),
    #[error("production Recall identity conflicted with a stored terminal")]
    RepositoryConflict,
    #[error("stored Recall result is corrupt or does not match the sealed request")]
    StoredResultMismatch,
    #[error("stored committed-Recall terminal authority is corrupt")]
    StoredTerminalRecoveryMismatch,
    #[error("terminal receipt could not be published: {0:?}")]
    TerminalCommit(CommitError),
}

/// Rebuilds a committed Recall arbiter only while that result still owns the
/// exact selected live Hall aggregate. Historical results remain replayable
/// through persistence but must not revive an old actor.
pub async fn recover_committed_recall_arbiter<Reader>(
    reader: &Reader,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<Option<TerminalArbiter>, ProductionRecallExecutionError>
where
    Reader: ProductionRecallTerminalReader,
{
    let Some(stored) = reader
        .load_committed_terminal(account_id, character_id)
        .await
        .map_err(ProductionRecallExecutionError::Persistence)?
    else {
        return Ok(None);
    };
    if !stored.owns_current_hall {
        return Ok(None);
    }
    let receipt = committed_recall_terminal_receipt_from_stored(&stored)?;
    TerminalArbiter::from_stored_receipt(receipt)
        .map(Some)
        .map_err(|_| ProductionRecallExecutionError::StoredTerminalRecoveryMismatch)
}

impl<Writer> ProductionRecallExecutionService<Writer>
where
    Writer: ProductionRecallWriter,
{
    pub async fn execute_coordinated(
        &self,
        coordinator: &mut CoreTerminalCoordinator,
        prepared_terminal: &PreparedTerminal,
        recall: &PreparedProductionRecallV1,
    ) -> Result<ProductionRecallExecutionOutcome, ProductionRecallExecutionError> {
        self.execute_prepared(
            coordinator.terminal_arbiter_mut(),
            prepared_terminal,
            recall,
        )
        .await
    }

    /// Executes only the frozen explicit or disconnect-recovery winner.
    ///
    /// Repository failure or plan drift leaves the arbiter prepared. Hall
    /// publication therefore remains blocked until exact retry or recovery.
    pub async fn execute_prepared(
        &self,
        arbiter: &mut TerminalArbiter,
        prepared_terminal: &PreparedTerminal,
        recall: &PreparedProductionRecallV1,
    ) -> Result<ProductionRecallExecutionOutcome, ProductionRecallExecutionError> {
        let candidate = production_recall_terminal_candidate(recall)?;
        if arbiter.prepared_terminal() != Some(prepared_terminal)
            || prepared_terminal.winner() != &candidate
        {
            return Err(ProductionRecallExecutionError::PreparedWinnerMismatch);
        }

        let transaction = self
            .writer
            .commit(recall.request(), recall.canonical_plan_hash())
            .await
            .map_err(ProductionRecallExecutionError::Persistence)?;
        let result = transaction
            .result()
            .ok_or(ProductionRecallExecutionError::RepositoryConflict)?;
        validate_stored_result_intent(recall, result)?;
        let receipt = committed_recall_terminal_receipt(recall, result)?;
        let terminal_commit = arbiter
            .record_commit(receipt)
            .map_err(ProductionRecallExecutionError::TerminalCommit)?;
        Ok(ProductionRecallExecutionOutcome {
            transaction,
            terminal_commit,
        })
    }

    /// Returns an already committed Recall without submitting its historical
    /// completion tick to a new producer barrier.
    pub async fn replay_committed(
        &self,
        recall: &PreparedProductionRecallV1,
    ) -> Result<ProductionRecallReplayOutcome, ProductionRecallExecutionError> {
        recall
            .validate()
            .map_err(ProductionRecallExecutionError::InvalidPreparedAuthority)?;
        if !recall.replayed() {
            return Err(ProductionRecallExecutionError::ExpectedStoredReplay);
        }
        let transaction = self
            .writer
            .commit(recall.request(), recall.canonical_plan_hash())
            .await
            .map_err(ProductionRecallExecutionError::Persistence)?;
        if !transaction.is_replay() {
            return Err(ProductionRecallExecutionError::ExpectedStoredReplay);
        }
        let receipt = {
            let result = transaction
                .result()
                .ok_or(ProductionRecallExecutionError::RepositoryConflict)?;
            committed_recall_terminal_receipt(recall, result)?
        };
        Ok(ProductionRecallReplayOutcome {
            transaction,
            receipt,
        })
    }
}

pub fn production_recall_terminal_candidate(
    recall: &PreparedProductionRecallV1,
) -> Result<TerminalCandidate, ProductionRecallExecutionError> {
    recall
        .validate()
        .map_err(ProductionRecallExecutionError::InvalidPreparedAuthority)?;
    if recall.replayed() {
        return Err(ProductionRecallExecutionError::CommittedReplayRequiresRecovery);
    }
    let request = recall.request();
    let binding = TerminalBinding::new(
        request.account_id,
        request.character_id,
        request.instance_lineage_id,
        request.entry_restore_point_id,
    )
    .map_err(ProductionRecallExecutionError::InvalidTerminalAuthority)?;
    TerminalCandidate::from_server_plan(
        binding,
        request.terminal_id,
        request.mutation_id,
        recall.canonical_request_hash(),
        recall.canonical_plan_hash(),
        request.expected_versions.character,
        request.completion_tick,
        terminal_kind(request.trigger),
    )
    .map_err(ProductionRecallExecutionError::InvalidTerminalAuthority)
}

pub fn committed_recall_terminal_receipt(
    recall: &PreparedProductionRecallV1,
    result: &StoredProductionRecallResultV1,
) -> Result<StoredTerminalReceipt, ProductionRecallExecutionError> {
    validate_stored_result_intent(recall, result)?;
    let request = recall.request();
    let result_hash = result
        .digest()
        .map_err(|_| ProductionRecallExecutionError::StoredResultMismatch)?;
    recall_terminal_receipt(
        request.instance_lineage_id,
        request.entry_restore_point_id,
        result,
        result_hash,
    )
}

pub fn committed_recall_terminal_receipt_from_stored(
    stored: &StoredCommittedRecallTerminalV1,
) -> Result<StoredTerminalReceipt, ProductionRecallExecutionError> {
    stored
        .validate()
        .map_err(|_| ProductionRecallExecutionError::StoredTerminalRecoveryMismatch)?;
    recall_terminal_receipt(
        stored.lineage_id,
        stored.restore_point_id,
        &stored.result,
        stored.result_hash,
    )
    .map_err(|_| ProductionRecallExecutionError::StoredTerminalRecoveryMismatch)
}

/// Projects the already committed Hall arrival without advancing any aggregate again.
pub fn hall_snapshot_from_stored_recall(
    result: &StoredProductionRecallResultV1,
) -> Result<CharacterLocationSnapshot, ProductionRecallExecutionError> {
    result
        .validate()
        .map_err(|_| ProductionRecallExecutionError::StoredResultMismatch)?;
    Ok(CharacterLocationSnapshot {
        character_id: result.character_id,
        character_version: result.versions.world.post,
        location: CharacterLocation::Safe {
            location_id: WireText::new(persistence::PRODUCTION_RECALL_HALL_ID)
                .map_err(|_| ProductionRecallExecutionError::StoredResultMismatch)?,
            arrival: SafeArrival::HallDefault,
        },
    })
}

/// Maps the complete committed persistence result into append-only protocol `1.15`.
pub fn protocol_recall_terminal_result(
    result: &StoredProductionRecallResultV1,
) -> Result<StoredRecallTerminalResultV1, ProductionRecallExecutionError> {
    result
        .validate()
        .map_err(|_| ProductionRecallExecutionError::StoredResultMismatch)?;
    let projected = StoredRecallTerminalResultV1 {
        character_id: result.character_id,
        terminal_id: result.terminal_id,
        result_hash: result
            .digest()
            .map_err(|_| ProductionRecallExecutionError::StoredResultMismatch)?,
        trigger: match result.trigger {
            ProductionRecallTriggerV1::Explicit => RecallTerminalTriggerV1::Explicit,
            ProductionRecallTriggerV1::LinkLost => RecallTerminalTriggerV1::LinkLost,
        },
        committed_at_unix_millis: result.committed_at_unix_ms,
        completion_tick: result.completion_tick,
        destination_content_id: WireText::new(result.destination_content_id.clone())
            .map_err(|_| ProductionRecallExecutionError::StoredResultMismatch)?,
        versions: TerminalVersionVectorV1 {
            account: protocol_version(result.versions.account),
            character: protocol_version(result.versions.character),
            world: protocol_version(result.versions.world),
            inventory: protocol_version(result.versions.inventory),
            life_clock: protocol_version(result.versions.life_metrics),
        },
        stabilized_item_count: u16::try_from(result.stabilized_items.len())
            .map_err(|_| ProductionRecallExecutionError::StoredResultMismatch)?,
        stabilized_items_digest: result
            .stabilized_items_digest()
            .map_err(|_| ProductionRecallExecutionError::StoredResultMismatch)?,
        destroyed_item_count: u16::try_from(result.destroyed_items.len())
            .map_err(|_| ProductionRecallExecutionError::StoredResultMismatch)?,
        destroyed_items_digest: result
            .destroyed_items_digest()
            .map_err(|_| ProductionRecallExecutionError::StoredResultMismatch)?,
        destroyed_material_stack_count: u8::try_from(result.destroyed_materials.len())
            .map_err(|_| ProductionRecallExecutionError::StoredResultMismatch)?,
        destroyed_materials_digest: result
            .destroyed_materials_digest()
            .map_err(|_| ProductionRecallExecutionError::StoredResultMismatch)?,
    };
    projected
        .validate()
        .map_err(|_| ProductionRecallExecutionError::StoredResultMismatch)?;
    Ok(projected)
}

const fn protocol_version(
    version: persistence::ProductionRecallVersionAdvanceV1,
) -> TerminalVersionAdvanceV1 {
    TerminalVersionAdvanceV1 {
        before: version.pre,
        after: version.post,
    }
}

fn recall_terminal_receipt(
    lineage_id: [u8; 16],
    restore_point_id: [u8; 16],
    result: &StoredProductionRecallResultV1,
    result_hash: [u8; 32],
) -> Result<StoredTerminalReceipt, ProductionRecallExecutionError> {
    StoredTerminalReceipt::from_storage(&StoredTerminalReceiptV1 {
        schema_version: STORED_TERMINAL_RECEIPT_SCHEMA_V1,
        account_id: result.account_id,
        character_id: result.character_id,
        lineage_id,
        restore_point_id,
        terminal_id: result.terminal_id,
        mutation_id: result.mutation_id,
        payload_hash: result.canonical_request_hash,
        server_plan_hash: result.canonical_plan_hash,
        result_hash,
        expected_state_version: result.versions.character.pre,
        post_state_version: result.versions.character.post,
        observed_tick: result.completion_tick,
        committed_tick: result.completion_tick,
        terminal_kind_code: terminal_kind(result.trigger).stable_code(),
    })
    .map_err(|_| ProductionRecallExecutionError::StoredResultMismatch)
}

#[allow(
    clippy::too_many_lines,
    reason = "the terminal executor must bind every prepared and stored Recall axis"
)]
fn validate_stored_result_intent(
    recall: &PreparedProductionRecallV1,
    result: &StoredProductionRecallResultV1,
) -> Result<(), ProductionRecallExecutionError> {
    recall
        .validate()
        .map_err(ProductionRecallExecutionError::InvalidPreparedAuthority)?;
    result
        .validate()
        .map_err(|_| ProductionRecallExecutionError::StoredResultMismatch)?;
    let request = recall.request();
    if result.contract_version != request.contract_version
        || result.namespace_id != request.namespace_id
        || result.account_id != request.account_id
        || result.character_id != request.character_id
        || result.mutation_id != request.mutation_id
        || result.terminal_id != request.terminal_id
        || result.canonical_request_hash != recall.canonical_request_hash()
        || result.canonical_plan_hash != recall.canonical_plan_hash()
        || result.trigger != request.trigger
        || result.request_sequence != request.request_sequence
        || result.issued_at_unix_ms != request.issued_at_unix_ms
        || result.trigger_started_tick != request.trigger_started_tick
        || result.completion_tick != request.completion_tick
        || result.destination_content_id != persistence::PRODUCTION_RECALL_HALL_ID
        || result.versions.account.pre != request.expected_versions.account
        || result.versions.character.pre != request.expected_versions.character
        || result.versions.world.pre != request.expected_versions.world
        || result.versions.inventory.pre != request.expected_versions.inventory
        || result.versions.life_metrics.pre != request.expected_versions.life_metrics
        || result.versions.progression.pre != request.expected_versions.progression
        || result.versions.oath_bargain.pre != request.expected_versions.oath_bargain
        || result.versions.ash_wallet.pre != request.expected_versions.ash_wallet
        || result.post_lifetime_ticks != request.final_lifetime_ticks
        || result.post_permadeath_combat_ticks != request.final_permadeath_combat_ticks
    {
        return Err(ProductionRecallExecutionError::StoredResultMismatch);
    }
    Ok(())
}

const fn terminal_kind(trigger: ProductionRecallTriggerV1) -> TerminalKind {
    match trigger {
        ProductionRecallTriggerV1::Explicit => TerminalKind::EmergencyRecall,
        ProductionRecallTriggerV1::LinkLost => TerminalKind::DisconnectRecovery,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use persistence::{
        PRODUCTION_RECALL_CONTRACT_VERSION_V1, ProductionRecallExpectedVersionsV1,
        ProductionRecallVersionAdvanceV1, ProductionRecallVersionsV1, StoredProductionRecallItemV1,
        StoredProductionRecallMaterialDestructionV1, StoredRecallLocationV1,
        StoredWorldFlowRevisionV1, WIPEABLE_CORE_NAMESPACE,
        canonical_production_recall_plan_hash_v1,
    };
    use protocol::{RecallIntentV1, TERMINAL_HALL_CONTENT_ID};

    use crate::{AccountId, NonTerminalAdmission, SubmitResult};

    use super::*;

    #[derive(Clone, Copy)]
    enum FakeMode {
        Fresh,
        Replay,
        Conflict,
        ForeignResult,
        Unavailable,
    }

    #[derive(Clone)]
    struct FakeWriter {
        mode: FakeMode,
        calls: Arc<Mutex<usize>>,
    }

    impl FakeWriter {
        fn new(mode: FakeMode) -> Self {
            Self {
                mode,
                calls: Arc::new(Mutex::new(0)),
            }
        }

        fn calls(&self) -> usize {
            *self.calls.lock().expect("calls")
        }
    }

    impl ProductionRecallWriter for FakeWriter {
        async fn commit(
            &self,
            request: &ProductionRecallCommitRequestV1,
            expected_plan_hash: [u8; 32],
        ) -> Result<ProductionRecallTransactionV1, PersistenceError> {
            *self.calls.lock().expect("calls") += 1;
            if matches!(self.mode, FakeMode::Unavailable) {
                return Err(PersistenceError::ProductionRecallTerminalSuperseded);
            }
            if matches!(self.mode, FakeMode::Conflict) {
                return Ok(ProductionRecallTransactionV1::Conflict {
                    terminal_id: [99; 16],
                });
            }
            let prepared = PreparedProductionRecallV1::seal(
                request.clone(),
                request.canonical_hash()?,
                expected_plan_hash,
                false,
            )?;
            let mut result = stored_result(&prepared);
            if matches!(self.mode, FakeMode::ForeignResult) {
                result.account_id = [99; 16];
            }
            Ok(if matches!(self.mode, FakeMode::Replay) {
                ProductionRecallTransactionV1::Replayed(result)
            } else {
                ProductionRecallTransactionV1::Fresh(result)
            })
        }
    }

    enum FakeReaderMode {
        Stored(Box<StoredCommittedRecallTerminalV1>),
        Absent,
        Unavailable,
    }

    struct FakeReader {
        mode: FakeReaderMode,
    }

    impl ProductionRecallTerminalReader for FakeReader {
        async fn load_committed_terminal(
            &self,
            _account_id: [u8; 16],
            _character_id: [u8; 16],
        ) -> Result<Option<StoredCommittedRecallTerminalV1>, PersistenceError> {
            match &self.mode {
                FakeReaderMode::Stored(stored) => Ok(Some((**stored).clone())),
                FakeReaderMode::Absent => Ok(None),
                FakeReaderMode::Unavailable => {
                    Err(PersistenceError::ProductionRecallTerminalSuperseded)
                }
            }
        }
    }

    fn request(trigger: ProductionRecallTriggerV1) -> ProductionRecallCommitRequestV1 {
        ProductionRecallCommitRequestV1 {
            contract_version: PRODUCTION_RECALL_CONTRACT_VERSION_V1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            account_id: [1; 16],
            character_id: [2; 16],
            mutation_id: [3; 16],
            terminal_id: [4; 16],
            trigger,
            request_sequence: match trigger {
                ProductionRecallTriggerV1::Explicit => Some(7),
                ProductionRecallTriggerV1::LinkLost => None,
            },
            instance_lineage_id: [5; 16],
            entry_restore_point_id: [6; 16],
            expected_versions: ProductionRecallExpectedVersionsV1 {
                account: 11,
                character: 12,
                world: 12,
                inventory: 13,
                life_metrics: 14,
                progression: 15,
                oath_bargain: 16,
                ash_wallet: 17,
            },
            content_revision: StoredWorldFlowRevisionV1 {
                records_blake3: "a".repeat(64),
                assets_blake3: "b".repeat(64),
                localization_blake3: "c".repeat(64),
            },
            issued_at_unix_ms: 18,
            trigger_started_tick: 100,
            completion_tick: 100 + trigger.channel_ticks(),
            final_lifetime_ticks: 1_000 + trigger.channel_ticks(),
            final_permadeath_combat_ticks: 800 + trigger.channel_ticks(),
        }
    }

    fn stabilized_items() -> Vec<StoredProductionRecallItemV1> {
        vec![StoredProductionRecallItemV1 {
            ordinal: 0,
            item_uid: [20; 16],
            template_id: "item.weapon.test".into(),
            content_revision: "core.items.v1".into(),
            item_kind: 0,
            source: StoredRecallLocationV1::Equipped(0),
            pre_item_version: 1,
            post_item_version: 2,
            ledger_event_id: [21; 16],
        }]
    }

    fn destroyed_items() -> Vec<StoredProductionRecallItemV1> {
        vec![StoredProductionRecallItemV1 {
            ordinal: 0,
            item_uid: [22; 16],
            template_id: "item.armor.test".into(),
            content_revision: "core.items.v1".into(),
            item_kind: 0,
            source: StoredRecallLocationV1::RunBackpack(0),
            pre_item_version: 1,
            post_item_version: 2,
            ledger_event_id: [23; 16],
        }]
    }

    fn destroyed_materials() -> Vec<StoredProductionRecallMaterialDestructionV1> {
        vec![StoredProductionRecallMaterialDestructionV1 {
            ordinal: 0,
            material_id: "material.bell_brass".into(),
            destroyed_quantity: 2,
            pre_pouch_version: 1,
            post_pouch_version: 2,
            destruction_event_id: [24; 16],
        }]
    }

    fn prepared_recall_with_replay(
        trigger: ProductionRecallTriggerV1,
        replayed: bool,
    ) -> PreparedProductionRecallV1 {
        let request = request(trigger);
        let request_hash = request.canonical_hash().unwrap();
        let plan_hash = canonical_production_recall_plan_hash_v1(
            &stabilized_items(),
            &destroyed_items(),
            &destroyed_materials(),
        )
        .unwrap();
        PreparedProductionRecallV1::seal(request, request_hash, plan_hash, replayed).unwrap()
    }

    fn prepared_recall(trigger: ProductionRecallTriggerV1) -> PreparedProductionRecallV1 {
        prepared_recall_with_replay(trigger, false)
    }

    fn stored_result(recall: &PreparedProductionRecallV1) -> StoredProductionRecallResultV1 {
        let request = recall.request();
        StoredProductionRecallResultV1 {
            contract_version: request.contract_version,
            namespace_id: request.namespace_id.clone(),
            account_id: request.account_id,
            character_id: request.character_id,
            mutation_id: request.mutation_id,
            terminal_id: request.terminal_id,
            canonical_request_hash: recall.canonical_request_hash(),
            canonical_plan_hash: recall.canonical_plan_hash(),
            result_code: 1,
            trigger: request.trigger,
            request_sequence: request.request_sequence,
            issued_at_unix_ms: request.issued_at_unix_ms,
            trigger_started_tick: request.trigger_started_tick,
            completion_tick: request.completion_tick,
            committed_at_unix_ms: request.issued_at_unix_ms + 1,
            source_content_id: "world.core_microrealm_01".into(),
            destination_content_id: persistence::PRODUCTION_RECALL_HALL_ID.into(),
            versions: ProductionRecallVersionsV1 {
                account: ProductionRecallVersionAdvanceV1 { pre: 11, post: 11 },
                character: ProductionRecallVersionAdvanceV1 { pre: 12, post: 13 },
                world: ProductionRecallVersionAdvanceV1 { pre: 12, post: 13 },
                inventory: ProductionRecallVersionAdvanceV1 { pre: 13, post: 14 },
                life_metrics: ProductionRecallVersionAdvanceV1 { pre: 14, post: 15 },
                progression: ProductionRecallVersionAdvanceV1 { pre: 15, post: 15 },
                oath_bargain: ProductionRecallVersionAdvanceV1 { pre: 16, post: 16 },
                ash_wallet: ProductionRecallVersionAdvanceV1 { pre: 17, post: 17 },
            },
            pre_lifetime_ticks: 1_000,
            post_lifetime_ticks: request.final_lifetime_ticks,
            pre_permadeath_combat_ticks: 800,
            post_permadeath_combat_ticks: request.final_permadeath_combat_ticks,
            stabilized_items: stabilized_items(),
            destroyed_items: destroyed_items(),
            destroyed_materials: destroyed_materials(),
        }
    }

    fn committed_terminal(owns_current_hall: bool) -> StoredCommittedRecallTerminalV1 {
        let recall = prepared_recall(ProductionRecallTriggerV1::Explicit);
        let result = stored_result(&recall);
        StoredCommittedRecallTerminalV1 {
            schema_version: persistence::PRODUCTION_RECALL_RECOVERY_SCHEMA_VERSION,
            result_hash: result.digest().unwrap(),
            result,
            lineage_id: recall.request().instance_lineage_id,
            restore_point_id: recall.request().entry_restore_point_id,
            content_revision: recall.request().content_revision.clone(),
            owns_current_hall,
        }
    }

    fn prepared_arbiter(
        recall: &PreparedProductionRecallV1,
    ) -> (TerminalArbiter, PreparedTerminal) {
        let candidate = production_recall_terminal_candidate(recall).expect("terminal candidate");
        let mut arbiter = TerminalArbiter::new(candidate.binding());
        assert!(matches!(
            arbiter.submit(candidate),
            SubmitResult::Accepted { .. }
        ));
        let prepared = arbiter
            .prepare(recall.request().completion_tick)
            .expect("sealed Recall tick");
        (arbiter, prepared)
    }

    #[test]
    fn explicit_and_link_lost_candidates_bind_complete_repository_preparation() {
        for (trigger, expected_kind, expected_tick) in [
            (
                ProductionRecallTriggerV1::Explicit,
                TerminalKind::EmergencyRecall,
                112,
            ),
            (
                ProductionRecallTriggerV1::LinkLost,
                TerminalKind::DisconnectRecovery,
                190,
            ),
        ] {
            let recall = prepared_recall(trigger);
            let candidate = production_recall_terminal_candidate(&recall).unwrap();
            let request = recall.request();
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
            assert_eq!(candidate.payload_hash(), &recall.canonical_request_hash());
            assert_eq!(candidate.server_plan_hash(), &recall.canonical_plan_hash());
            assert_eq!(candidate.expected_state_version(), 12);
            assert_eq!(candidate.observed_tick(), expected_tick);
            assert_eq!(candidate.kind(), expected_kind);
        }
    }

    #[test]
    fn disabled_transport_authority_rejects_before_repository_access() {
        let authenticated = AuthenticatedAccount {
            account_id: AccountId::new([1; 16]).unwrap(),
            namespace: AuthenticatedNamespace::WipeableTest,
        };
        let frame = RecallFrameV1 {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            sequence: 1,
            character_id: [2; 16],
            client_tick: 10,
            intent: RecallIntentV1::Start,
        };
        assert!(matches!(
            CoreRecallTerminalAuthority::disabled().handle(authenticated, &frame),
            RecallResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::FeatureDisabled,
                ..
            }
        ));

        let mut malformed = frame;
        malformed.client_tick = 0;
        assert!(matches!(
            CoreRecallTerminalAuthority::disabled().handle(authenticated, &malformed),
            RecallResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::InvalidRequest,
                ..
            }
        ));
    }

    #[test]
    fn protocol_and_hall_projection_bind_every_result_axis() {
        let recall = prepared_recall(ProductionRecallTriggerV1::Explicit);
        let stored = stored_result(&recall);
        let projected = protocol_recall_terminal_result(&stored).unwrap();
        assert_eq!(projected.result_hash, stored.digest().unwrap());
        assert_eq!(projected.trigger, RecallTerminalTriggerV1::Explicit);
        assert_eq!(projected.completion_tick, 112);
        assert_eq!(projected.stabilized_item_count, 1);
        assert_eq!(
            projected.stabilized_items_digest,
            stored.stabilized_items_digest().unwrap()
        );
        assert_eq!(projected.destroyed_item_count, 1);
        assert_eq!(projected.destroyed_material_stack_count, 1);
        assert_eq!(
            projected.versions.account,
            TerminalVersionAdvanceV1 {
                before: 11,
                after: 11,
            }
        );
        projected.validate().unwrap();

        let hall = hall_snapshot_from_stored_recall(&stored).unwrap();
        assert_eq!(hall.character_id, stored.character_id);
        assert_eq!(hall.character_version, stored.versions.world.post);
        assert!(matches!(
            hall.location,
            CharacterLocation::Safe {
                location_id,
                arrival: SafeArrival::HallDefault
            } if location_id.as_str() == TERMINAL_HALL_CONTENT_ID
        ));
    }

    #[test]
    fn committed_replay_cannot_reenter_a_new_terminal_tick() {
        let recall = prepared_recall_with_replay(ProductionRecallTriggerV1::Explicit, true);
        assert!(matches!(
            production_recall_terminal_candidate(&recall),
            Err(ProductionRecallExecutionError::CommittedReplayRequiresRecovery)
        ));
    }

    #[tokio::test]
    async fn fresh_and_replayed_commits_publish_restart_safe_receipts() {
        for mode in [FakeMode::Fresh, FakeMode::Replay] {
            let recall = prepared_recall(ProductionRecallTriggerV1::Explicit);
            let (mut arbiter, prepared) = prepared_arbiter(&recall);
            let writer = FakeWriter::new(mode);
            let service = ProductionRecallExecutionService::new(writer.clone());
            let outcome = service
                .execute_prepared(&mut arbiter, &prepared, &recall)
                .await
                .unwrap();

            assert_eq!(
                outcome.transaction.is_replay(),
                matches!(mode, FakeMode::Replay)
            );
            assert_eq!(writer.calls(), 1);
            assert!(matches!(
                outcome.terminal_commit,
                CommitResult::Committed(_)
            ));
            assert_eq!(
                arbiter.non_terminal_admission(),
                NonTerminalAdmission::BlockedByCommittedTerminal
            );
            let receipt = arbiter.committed_receipt().unwrap();
            assert_eq!(receipt.expected_state_version(), 12);
            assert_eq!(receipt.post_state_version(), 13);
            assert_eq!(receipt.observed_tick(), 112);
            assert_eq!(receipt.committed_tick(), 112);
            assert_eq!(receipt.kind(), TerminalKind::EmergencyRecall);
        }
    }

    #[tokio::test]
    async fn committed_replay_returns_a_receipt_without_new_tick_submission() {
        let recall = prepared_recall_with_replay(ProductionRecallTriggerV1::LinkLost, true);
        let writer = FakeWriter::new(FakeMode::Replay);
        let service = ProductionRecallExecutionService::new(writer.clone());
        let replay = service.replay_committed(&recall).await.unwrap();

        assert!(replay.transaction.is_replay());
        assert_eq!(writer.calls(), 1);
        assert_eq!(replay.receipt.kind(), TerminalKind::DisconnectRecovery);
        assert_eq!(replay.receipt.observed_tick(), 190);
        let recovered =
            TerminalArbiter::from_stored_receipt(replay.receipt).expect("recovered arbiter");
        assert_eq!(
            recovered.non_terminal_admission(),
            NonTerminalAdmission::BlockedByCommittedTerminal
        );
    }

    #[tokio::test]
    async fn strict_recovery_reconstructs_only_the_current_recall_actor() {
        let stored = committed_terminal(true);
        let expected =
            committed_recall_terminal_receipt_from_stored(&stored).expect("stored receipt");
        let reader = FakeReader {
            mode: FakeReaderMode::Stored(Box::new(stored.clone())),
        };
        let recovered = recover_committed_recall_arbiter(
            &reader,
            stored.result.account_id,
            stored.result.character_id,
        )
        .await
        .expect("recovery")
        .expect("committed Recall");
        assert_eq!(recovered.committed_receipt(), Some(&expected));
        assert_eq!(expected.kind(), TerminalKind::EmergencyRecall);

        let historical = FakeReader {
            mode: FakeReaderMode::Stored(Box::new(committed_terminal(false))),
        };
        assert!(
            recover_committed_recall_arbiter(&historical, [1; 16], [2; 16])
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn absent_and_unavailable_recovery_are_typed() {
        let absent = FakeReader {
            mode: FakeReaderMode::Absent,
        };
        assert!(
            recover_committed_recall_arbiter(&absent, [1; 16], [2; 16])
                .await
                .unwrap()
                .is_none()
        );

        let unavailable = FakeReader {
            mode: FakeReaderMode::Unavailable,
        };
        assert!(matches!(
            recover_committed_recall_arbiter(&unavailable, [1; 16], [2; 16]).await,
            Err(ProductionRecallExecutionError::Persistence(
                PersistenceError::ProductionRecallTerminalSuperseded
            ))
        ));
    }

    #[tokio::test]
    async fn lethal_winner_and_repository_failures_never_publish_recall() {
        let recall = prepared_recall(ProductionRecallTriggerV1::Explicit);
        let recall_candidate = production_recall_terminal_candidate(&recall).unwrap();
        let lethal = TerminalCandidate::from_server_plan(
            recall_candidate.binding(),
            [30; 16],
            [31; 16],
            [32; 32],
            [33; 32],
            recall_candidate.expected_state_version(),
            recall_candidate.observed_tick(),
            TerminalKind::LethalDeath,
        )
        .unwrap();
        let mut arbiter = TerminalArbiter::new(recall_candidate.binding());
        assert!(matches!(
            arbiter.submit(recall_candidate),
            SubmitResult::Accepted { .. }
        ));
        assert!(matches!(
            arbiter.submit(lethal),
            SubmitResult::Accepted { .. }
        ));
        let prepared = arbiter.prepare(112).unwrap();
        assert_eq!(prepared.winner().kind(), TerminalKind::LethalDeath);
        let writer = FakeWriter::new(FakeMode::Fresh);
        let service = ProductionRecallExecutionService::new(writer.clone());
        assert!(matches!(
            service
                .execute_prepared(&mut arbiter, &prepared, &recall)
                .await,
            Err(ProductionRecallExecutionError::PreparedWinnerMismatch)
        ));
        assert_eq!(writer.calls(), 0);

        for (mode, expected) in [
            (FakeMode::Conflict, "conflict"),
            (FakeMode::ForeignResult, "stored"),
            (FakeMode::Unavailable, "repository"),
        ] {
            let recall = prepared_recall(ProductionRecallTriggerV1::Explicit);
            let (mut arbiter, prepared) = prepared_arbiter(&recall);
            let service = ProductionRecallExecutionService::new(FakeWriter::new(mode));
            let outcome = service
                .execute_prepared(&mut arbiter, &prepared, &recall)
                .await;
            assert!(matches!(
                (expected, outcome),
                (
                    "conflict",
                    Err(ProductionRecallExecutionError::RepositoryConflict)
                ) | (
                    "stored",
                    Err(ProductionRecallExecutionError::StoredResultMismatch)
                ) | (
                    "repository",
                    Err(ProductionRecallExecutionError::Persistence(_))
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
