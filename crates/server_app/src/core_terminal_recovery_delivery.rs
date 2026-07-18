//! Terminal-first process-restart delivery for committed Core Recall and extraction results.
//!
//! Authorities: `Gravebound_Production_GDD_v1_Canonical.md` (`DTH-010`/`011` and
//! `TECH-015`/`021`-`023`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-HUB-001`/`002` and the fixed Core content revision), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`/`08`).
//!
//! This boundary never reconstructs a terminal decision and never installs Hall state. It only
//! validates immutable repository authority and sends an append-only replay through the current
//! transport's shared reliable writer. The outer reliable frame uses the current authoritative
//! server tick; historical completion/observation ticks remain inside the stored terminal result.

use std::future::Future;

use persistence::{
    PersistenceError, PostgresPersistence, StoredCommittedExtractionTerminalV1,
    StoredCommittedRecallTerminalV1, StoredProductionExtractionIntentAcceptanceV1,
};
use protocol::{
    ExtractionCommitFrameV1, ExtractionCommitResultV1, RecallResultV1, ReliableEvent,
    ReliableEventFrame, TERMINAL_INVENTORY_SCHEMA_VERSION, TerminalInventoryValidationError,
};
use thiserror::Error;

use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CoreReliableWriter, CoreReliableWriterError,
    ProductionExtractionExecutionError, ProductionRecallExecutionError,
    protocol_extraction_terminal_result, protocol_recall_terminal_result,
};

/// Read-only seam for exact extraction response-loss recovery.
pub trait RecoveredExtractionTerminalRepository: Send + Sync {
    fn load_intent_acceptance(
        &self,
        extraction_request_id: [u8; 16],
    ) -> impl Future<
        Output = Result<Option<StoredProductionExtractionIntentAcceptanceV1>, PersistenceError>,
    > + Send;

    fn load_committed_terminal(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
        extraction_request_id: [u8; 16],
        extraction_receipt_id: [u8; 16],
    ) -> impl Future<Output = Result<Option<StoredCommittedExtractionTerminalV1>, PersistenceError>> + Send;
}

impl RecoveredExtractionTerminalRepository for PostgresPersistence {
    async fn load_intent_acceptance(
        &self,
        extraction_request_id: [u8; 16],
    ) -> Result<Option<StoredProductionExtractionIntentAcceptanceV1>, PersistenceError> {
        self.load_production_extraction_intent_acceptance_v1(extraction_request_id)
            .await
    }

    async fn load_committed_terminal(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
        extraction_request_id: [u8; 16],
        extraction_receipt_id: [u8; 16],
    ) -> Result<Option<StoredCommittedExtractionTerminalV1>, PersistenceError> {
        self.load_committed_extraction_terminal_by_identity_v1(
            account_id,
            character_id,
            extraction_request_id,
            extraction_receipt_id,
        )
        .await
    }
}

/// Opaque proof that one stored Recall was validated and delivered as a replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredRecallDeliveryProof {
    frame: ReliableEventFrame,
    terminal_id: [u8; 16],
    result_hash: [u8; 32],
}

impl RecoveredRecallDeliveryProof {
    #[must_use]
    pub const fn frame(&self) -> &ReliableEventFrame {
        &self.frame
    }

    #[must_use]
    pub const fn terminal_id(&self) -> [u8; 16] {
        self.terminal_id
    }

    #[must_use]
    pub const fn result_hash(&self) -> [u8; 32] {
        self.result_hash
    }
}

/// Opaque proof that an exact extraction retry was validated against both immutable records and
/// delivered using the retry frame's current request sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredExtractionDeliveryProof {
    frame: ReliableEventFrame,
    extraction_request_id: [u8; 16],
    terminal_id: [u8; 16],
    result_hash: [u8; 32],
}

impl RecoveredExtractionDeliveryProof {
    #[must_use]
    pub const fn frame(&self) -> &ReliableEventFrame {
        &self.frame
    }

    #[must_use]
    pub const fn extraction_request_id(&self) -> [u8; 16] {
        self.extraction_request_id
    }

    #[must_use]
    pub const fn terminal_id(&self) -> [u8; 16] {
        self.terminal_id
    }

    #[must_use]
    pub const fn result_hash(&self) -> [u8; 32] {
        self.result_hash
    }
}

#[derive(Debug, Error)]
pub enum RecoveredTerminalDeliveryError {
    #[error("current authoritative server tick is invalid")]
    InvalidServerTick,
    #[error("stored Recall terminal is corrupt")]
    InvalidRecallTerminal(#[source] PersistenceError),
    #[error("stored Recall terminal no longer owns the selected character's current Hall state")]
    RecallTerminalNotCurrent,
    #[error("stored Recall terminal does not belong to the authenticated selected character")]
    ForeignRecallAuthority,
    #[error("stored Recall terminal cannot be projected")]
    RecallProjection(#[source] ProductionRecallExecutionError),
    #[error("extraction retry frame is invalid")]
    InvalidExtractionFrame(#[source] TerminalInventoryValidationError),
    #[error("extraction retry is outside the authenticated Core namespace")]
    ForeignExtractionAuthority,
    #[error("extraction recovery repository is unavailable")]
    Persistence(#[source] PersistenceError),
    #[error("no immutable acceptance exists for the extraction retry")]
    IntentAcceptanceNotFound,
    #[error("no committed terminal exists for the accepted extraction")]
    CommittedExtractionNotFound,
    #[error("extraction retry conflicts with immutable intent authority")]
    ExtractionIntentMismatch,
    #[error("committed extraction conflicts with immutable intent authority")]
    CommittedExtractionMismatch,
    #[error("stored extraction terminal cannot be projected")]
    ExtractionProjection(#[source] ProductionExtractionExecutionError),
    #[error(transparent)]
    Writer(#[from] CoreReliableWriterError),
}

/// Sends one committed Recall after process restart. Explicit Recall retains the stored client
/// request sequence; `LinkLost` retains `None`. Neither path invents a transport-era sequence.
pub async fn send_recovered_recall_terminal(
    writer: &CoreReliableWriter,
    authenticated: AuthenticatedAccount,
    selected_character_id: [u8; 16],
    stored: &StoredCommittedRecallTerminalV1,
    current_server_tick: u64,
) -> Result<RecoveredRecallDeliveryProof, RecoveredTerminalDeliveryError> {
    if current_server_tick == 0 {
        return Err(RecoveredTerminalDeliveryError::InvalidServerTick);
    }
    let (event, terminal_id, result_hash) =
        recovered_recall_event(authenticated, selected_character_id, stored)?;
    let frame = writer.send_event(current_server_tick, event).await?;
    Ok(RecoveredRecallDeliveryProof {
        frame,
        terminal_id,
        result_hash,
    })
}

/// Recovers and sends only an exact retry of the first accepted extraction payload. The request's
/// current sequence is response correlation only and is intentionally absent from durable intent
/// matching; every domain-authority field remains exact.
pub async fn recover_and_send_extraction_retry<Repository>(
    repository: &Repository,
    writer: &CoreReliableWriter,
    authenticated: AuthenticatedAccount,
    retry: &ExtractionCommitFrameV1,
    current_server_tick: u64,
) -> Result<RecoveredExtractionDeliveryProof, RecoveredTerminalDeliveryError>
where
    Repository: RecoveredExtractionTerminalRepository,
{
    if current_server_tick == 0 {
        return Err(RecoveredTerminalDeliveryError::InvalidServerTick);
    }
    retry
        .validate()
        .map_err(RecoveredTerminalDeliveryError::InvalidExtractionFrame)?;
    if authenticated.namespace != AuthenticatedNamespace::WipeableTest {
        return Err(RecoveredTerminalDeliveryError::ForeignExtractionAuthority);
    }
    let acceptance = repository
        .load_intent_acceptance(retry.payload.extraction_request_id)
        .await
        .map_err(RecoveredTerminalDeliveryError::Persistence)?
        .ok_or(RecoveredTerminalDeliveryError::IntentAcceptanceNotFound)?;
    validate_retry_against_acceptance(authenticated, retry, &acceptance)?;
    let request = &acceptance.attempt.commit_request;
    let terminal = repository
        .load_committed_terminal(
            authenticated.account_id.as_bytes(),
            retry.character_id,
            request.extraction_request_id,
            request.extraction_receipt_id,
        )
        .await
        .map_err(RecoveredTerminalDeliveryError::Persistence)?
        .ok_or(RecoveredTerminalDeliveryError::CommittedExtractionNotFound)?;
    validate_terminal_against_acceptance(&acceptance, &terminal)?;
    let result = protocol_extraction_terminal_result(&terminal.result)
        .map_err(RecoveredTerminalDeliveryError::ExtractionProjection)?;
    let event = ReliableEvent::ExtractionCommitResult(Box::new(ExtractionCommitResultV1::Stored {
        schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
        request_sequence: retry.sequence,
        replayed: true,
        result: Box::new(result),
    }));
    let frame = writer.send_event(current_server_tick, event).await?;
    Ok(RecoveredExtractionDeliveryProof {
        frame,
        extraction_request_id: terminal.result.extraction_request_id,
        terminal_id: terminal.result.terminal_id,
        result_hash: terminal.result_hash,
    })
}

fn recovered_recall_event(
    authenticated: AuthenticatedAccount,
    selected_character_id: [u8; 16],
    stored: &StoredCommittedRecallTerminalV1,
) -> Result<(ReliableEvent, [u8; 16], [u8; 32]), RecoveredTerminalDeliveryError> {
    stored
        .validate()
        .map_err(RecoveredTerminalDeliveryError::InvalidRecallTerminal)?;
    if !stored.owns_current_hall {
        return Err(RecoveredTerminalDeliveryError::RecallTerminalNotCurrent);
    }
    if authenticated.namespace != AuthenticatedNamespace::WipeableTest
        || selected_character_id == [0; 16]
        || stored.result.account_id != authenticated.account_id.as_bytes()
        || stored.result.character_id != selected_character_id
    {
        return Err(RecoveredTerminalDeliveryError::ForeignRecallAuthority);
    }
    let projected = protocol_recall_terminal_result(&stored.result)
        .map_err(RecoveredTerminalDeliveryError::RecallProjection)?;
    let result = RecallResultV1::Stored {
        schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
        request_sequence: stored.result.request_sequence,
        replayed: true,
        result: Box::new(projected),
    };
    result.validate().map_err(|_| {
        RecoveredTerminalDeliveryError::RecallProjection(
            ProductionRecallExecutionError::StoredResultMismatch,
        )
    })?;
    Ok((
        ReliableEvent::RecallResult(Box::new(result)),
        stored.result.terminal_id,
        stored.result_hash,
    ))
}

fn validate_retry_against_acceptance(
    authenticated: AuthenticatedAccount,
    retry: &ExtractionCommitFrameV1,
    acceptance: &StoredProductionExtractionIntentAcceptanceV1,
) -> Result<(), RecoveredTerminalDeliveryError> {
    let attempt = &acceptance.attempt;
    let request = &attempt.commit_request;
    attempt
        .validate()
        .map_err(|_| RecoveredTerminalDeliveryError::ExtractionIntentMismatch)?;
    let canonical_attempt_hash = attempt
        .canonical_hash()
        .map_err(|_| RecoveredTerminalDeliveryError::ExtractionIntentMismatch)?;
    let canonical_request_hash = request
        .canonical_hash()
        .map_err(|_| RecoveredTerminalDeliveryError::ExtractionIntentMismatch)?;
    let revision = &retry.payload.content_revision;
    let versions = retry.payload.expected_versions;
    let exact = acceptance.canonical_attempt_hash == canonical_attempt_hash
        && acceptance.commit_request_hash == canonical_request_hash
        && acceptance.accepted_at_unix_ms >= attempt.issued_at_unix_ms
        && attempt.authenticated_account_id == authenticated.account_id.as_bytes()
        && attempt.attempted_character_id == retry.character_id
        && attempt.attempted_mutation_id == retry.mutation_id
        && attempt.attempted_frame_schema_version == retry.schema_version
        && attempt.attempted_frame_payload_hash == retry.payload_hash
        && attempt.extraction_request_id == retry.payload.extraction_request_id
        && attempt.issued_at_unix_ms == retry.issued_at_unix_millis
        && request.account_id == authenticated.account_id.as_bytes()
        && request.character_id == retry.character_id
        && request.mutation_id == retry.mutation_id
        && request.extraction_request_id == retry.payload.extraction_request_id
        && request.issued_at_unix_ms == retry.issued_at_unix_millis
        && request.content_revision.records_blake3 == revision.records_blake3.as_str()
        && request.content_revision.assets_blake3 == revision.assets_blake3.as_str()
        && request.content_revision.localization_blake3 == revision.localization_blake3.as_str()
        && request.expected_versions.account == versions.account
        && request.expected_versions.character == versions.character
        && request.expected_versions.world == versions.world
        && request.expected_versions.inventory == versions.inventory
        && request.expected_versions.life_metrics == versions.life_clock;
    if exact {
        Ok(())
    } else {
        Err(RecoveredTerminalDeliveryError::ExtractionIntentMismatch)
    }
}

fn validate_terminal_against_acceptance(
    acceptance: &StoredProductionExtractionIntentAcceptanceV1,
    terminal: &StoredCommittedExtractionTerminalV1,
) -> Result<(), RecoveredTerminalDeliveryError> {
    terminal
        .validate()
        .map_err(|_| RecoveredTerminalDeliveryError::CommittedExtractionMismatch)?;
    let request = &acceptance.attempt.commit_request;
    let result = &terminal.result;
    let exact = terminal.encounter_id == request.encounter_id
        && terminal.lineage_id == request.instance_lineage_id
        && terminal.restore_point_id == request.entry_restore_point_id
        && terminal.exit_instance_id == request.exit_instance_id
        && result.account_id == request.account_id
        && result.character_id == request.character_id
        && result.mutation_id == request.mutation_id
        && result.terminal_id == request.terminal_id
        && result.extraction_request_id == request.extraction_request_id
        && result.extraction_receipt_id == request.extraction_receipt_id
        && result.canonical_request_hash == acceptance.commit_request_hash
        && result.issued_at_unix_ms == request.issued_at_unix_ms
        && result.observed_tick == request.observed_tick
        && result.committed_at_unix_ms >= acceptance.accepted_at_unix_ms
        && result.versions.account.pre == request.expected_versions.account
        && result.versions.character.pre == request.expected_versions.character
        && result.versions.world.pre == request.expected_versions.world
        && result.versions.inventory.pre == request.expected_versions.inventory
        && result.versions.life_metrics.pre == request.expected_versions.life_metrics;
    if exact {
        Ok(())
    } else {
        Err(RecoveredTerminalDeliveryError::CommittedExtractionMismatch)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use persistence::{
        PRODUCTION_EXTRACTION_CONTRACT_VERSION_V1,
        PRODUCTION_EXTRACTION_INTENT_CONTRACT_VERSION_V1,
        PRODUCTION_EXTRACTION_INTENT_FRAME_SCHEMA_VERSION_V1,
        PRODUCTION_EXTRACTION_RECOVERY_SCHEMA_VERSION, PRODUCTION_RECALL_CONTRACT_VERSION_V1,
        PRODUCTION_RECALL_EXPLICIT_CHANNEL_TICKS, PRODUCTION_RECALL_RECOVERY_SCHEMA_VERSION,
        ProductionExtractionCommitRequestV1, ProductionExtractionCoreRouteRevisionV1,
        ProductionExtractionExpectedVersionsV1, ProductionExtractionIntentAttemptV1,
        ProductionExtractionVersionAdvanceV1, ProductionExtractionVersionsV1,
        ProductionRecallTriggerV1, ProductionRecallVersionAdvanceV1, ProductionRecallVersionsV1,
        StoredProductionExtractionResultV1, StoredProductionRecallResultV1,
        StoredWorldFlowRevisionV1, WIPEABLE_CORE_NAMESPACE,
        canonical_production_extraction_frame_payload_hash_v1,
        canonical_production_extraction_plan_hash_v1, canonical_production_recall_plan_hash_v1,
    };
    use protocol::{
        ExtractionCommitPayloadV1, ManifestHash, RecallTerminalTriggerV1,
        TerminalExpectedVersionsV1, WorldFlowContentRevisionV1,
    };
    use rcgen::generate_simple_self_signed;
    use rustls::pki_types::PrivatePkcs8KeyDer;

    use crate::AccountId;

    use super::*;

    #[derive(Debug, Clone)]
    struct FakeRepository {
        acceptance: Option<StoredProductionExtractionIntentAcceptanceV1>,
        terminal: Option<StoredCommittedExtractionTerminalV1>,
    }

    impl RecoveredExtractionTerminalRepository for FakeRepository {
        async fn load_intent_acceptance(
            &self,
            _extraction_request_id: [u8; 16],
        ) -> Result<Option<StoredProductionExtractionIntentAcceptanceV1>, PersistenceError>
        {
            Ok(self.acceptance.clone())
        }

        async fn load_committed_terminal(
            &self,
            _account_id: [u8; 16],
            _character_id: [u8; 16],
            _extraction_request_id: [u8; 16],
            _extraction_receipt_id: [u8; 16],
        ) -> Result<Option<StoredCommittedExtractionTerminalV1>, PersistenceError> {
            Ok(self.terminal.clone())
        }
    }

    fn revision() -> StoredWorldFlowRevisionV1 {
        StoredWorldFlowRevisionV1 {
            records_blake3: "a".repeat(64),
            assets_blake3: "b".repeat(64),
            localization_blake3: "c".repeat(64),
        }
    }

    fn protocol_revision() -> WorldFlowContentRevisionV1 {
        WorldFlowContentRevisionV1 {
            records_blake3: ManifestHash::new("a".repeat(64)).unwrap(),
            assets_blake3: ManifestHash::new("b".repeat(64)).unwrap(),
            localization_blake3: ManifestHash::new("c".repeat(64)).unwrap(),
        }
    }

    const fn extraction_versions() -> ProductionExtractionExpectedVersionsV1 {
        ProductionExtractionExpectedVersionsV1 {
            account: 1,
            character: 2,
            world: 2,
            inventory: 3,
            life_metrics: 4,
        }
    }

    fn extraction_request() -> ProductionExtractionCommitRequestV1 {
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
            expected_versions: extraction_versions(),
            content_revision: revision(),
            issued_at_unix_ms: 500,
            observed_tick: 900,
        }
    }

    fn acceptance() -> StoredProductionExtractionIntentAcceptanceV1 {
        let request = extraction_request();
        let attempt = ProductionExtractionIntentAttemptV1 {
            contract_version: PRODUCTION_EXTRACTION_INTENT_CONTRACT_VERSION_V1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            authenticated_account_id: request.account_id,
            attempted_character_id: request.character_id,
            attempted_mutation_id: request.mutation_id,
            attempted_frame_schema_version: PRODUCTION_EXTRACTION_INTENT_FRAME_SCHEMA_VERSION_V1,
            attempted_frame_payload_hash: canonical_production_extraction_frame_payload_hash_v1(
                &request,
            )
            .unwrap(),
            extraction_request_id: request.extraction_request_id,
            extraction_receipt_id: request.extraction_receipt_id,
            terminal_id: request.terminal_id,
            actor_generation: 11,
            accepted_pre_route_state_version: 12,
            accepted_post_route_state_version: 13,
            core_route_revision: ProductionExtractionCoreRouteRevisionV1 {
                records_blake3: "d".repeat(64),
                assets_blake3: "e".repeat(64),
                localization_blake3: "f".repeat(64),
            },
            world_flow_revision: request.content_revision.clone(),
            issued_at_unix_ms: request.issued_at_unix_ms,
            observed_tick: request.observed_tick,
            commit_request: request,
        };
        StoredProductionExtractionIntentAcceptanceV1 {
            canonical_attempt_hash: attempt.canonical_hash().unwrap(),
            commit_request_hash: attempt.commit_request.canonical_hash().unwrap(),
            accepted_at_unix_ms: 550,
            attempt,
        }
    }

    fn retry(sequence: u32) -> ExtractionCommitFrameV1 {
        let request = extraction_request();
        let payload = ExtractionCommitPayloadV1 {
            extraction_request_id: request.extraction_request_id,
            expected_versions: TerminalExpectedVersionsV1 {
                account: request.expected_versions.account,
                character: request.expected_versions.character,
                world: request.expected_versions.world,
                inventory: request.expected_versions.inventory,
                life_clock: request.expected_versions.life_metrics,
            },
            content_revision: protocol_revision(),
        };
        ExtractionCommitFrameV1 {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            sequence,
            mutation_id: request.mutation_id,
            character_id: request.character_id,
            issued_at_unix_millis: request.issued_at_unix_ms,
            payload_hash: payload.canonical_hash(),
            payload,
        }
    }

    const fn advanced(pre: u64) -> ProductionExtractionVersionAdvanceV1 {
        ProductionExtractionVersionAdvanceV1 { pre, post: pre + 1 }
    }

    fn extraction_terminal() -> StoredCommittedExtractionTerminalV1 {
        let request = extraction_request();
        let result = StoredProductionExtractionResultV1 {
            contract_version: PRODUCTION_EXTRACTION_CONTRACT_VERSION_V1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            account_id: request.account_id,
            character_id: request.character_id,
            mutation_id: request.mutation_id,
            terminal_id: request.terminal_id,
            extraction_request_id: request.extraction_request_id,
            extraction_receipt_id: request.extraction_receipt_id,
            canonical_request_hash: request.canonical_hash().unwrap(),
            canonical_plan_hash: canonical_production_extraction_plan_hash_v1(&[], &[]).unwrap(),
            result_code: 1,
            issued_at_unix_ms: request.issued_at_unix_ms,
            observed_tick: request.observed_tick,
            committed_at_unix_ms: 600,
            destination_content_id: persistence::PRODUCTION_EXTRACTION_HALL_ID.into(),
            versions: ProductionExtractionVersionsV1 {
                account: ProductionExtractionVersionAdvanceV1 { pre: 1, post: 1 },
                character: advanced(2),
                world: advanced(2),
                inventory: advanced(3),
                life_metrics: advanced(4),
            },
            placements: Vec::new(),
            material_credits: Vec::new(),
            storage_resolution_required: false,
        };
        StoredCommittedExtractionTerminalV1 {
            schema_version: PRODUCTION_EXTRACTION_RECOVERY_SCHEMA_VERSION,
            result_hash: result.digest().unwrap(),
            encounter_id: request.encounter_id,
            lineage_id: request.instance_lineage_id,
            restore_point_id: request.entry_restore_point_id,
            exit_instance_id: request.exit_instance_id,
            result,
        }
    }

    fn authenticated() -> AuthenticatedAccount {
        AuthenticatedAccount {
            account_id: AccountId::new([1; 16]).unwrap(),
            namespace: AuthenticatedNamespace::WipeableTest,
        }
    }

    const fn recall_advance(pre: u64, unchanged: bool) -> ProductionRecallVersionAdvanceV1 {
        ProductionRecallVersionAdvanceV1 {
            pre,
            post: if unchanged { pre } else { pre + 1 },
        }
    }

    fn recall_terminal(trigger: ProductionRecallTriggerV1) -> StoredCommittedRecallTerminalV1 {
        let explicit = trigger == ProductionRecallTriggerV1::Explicit;
        let started = 100;
        let completion = started
            + if explicit {
                PRODUCTION_RECALL_EXPLICIT_CHANNEL_TICKS
            } else {
                persistence::PRODUCTION_RECALL_LINK_LOST_TICKS
            };
        let result = StoredProductionRecallResultV1 {
            contract_version: PRODUCTION_RECALL_CONTRACT_VERSION_V1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.into(),
            account_id: [1; 16],
            character_id: [2; 16],
            mutation_id: [3; 16],
            terminal_id: [4; 16],
            canonical_request_hash: [5; 32],
            canonical_plan_hash: canonical_production_recall_plan_hash_v1(&[], &[], &[]).unwrap(),
            result_code: 1,
            trigger,
            request_sequence: explicit.then_some(77),
            explicit_client_tick: explicit.then_some(88),
            issued_at_unix_ms: 50,
            trigger_started_tick: started,
            completion_tick: completion,
            committed_at_unix_ms: 80,
            source_content_id: "dungeon.bell_sepulcher".into(),
            destination_content_id: persistence::PRODUCTION_RECALL_HALL_ID.into(),
            versions: ProductionRecallVersionsV1 {
                account: recall_advance(1, true),
                character: recall_advance(2, false),
                world: recall_advance(2, false),
                inventory: recall_advance(3, false),
                life_metrics: recall_advance(4, false),
                progression: recall_advance(5, true),
                oath_bargain: recall_advance(6, true),
                ash_wallet: recall_advance(7, true),
            },
            pre_lifetime_ticks: 10,
            post_lifetime_ticks: 11,
            pre_permadeath_combat_ticks: 8,
            post_permadeath_combat_ticks: 9,
            stabilized_items: Vec::new(),
            destroyed_items: Vec::new(),
            destroyed_materials: Vec::new(),
        };
        StoredCommittedRecallTerminalV1 {
            schema_version: PRODUCTION_RECALL_RECOVERY_SCHEMA_VERSION,
            result_hash: result.digest().unwrap(),
            lineage_id: [7; 16],
            restore_point_id: [8; 16],
            content_revision: revision(),
            owns_current_hall: true,
            result,
        }
    }

    async fn live_connection_pair() -> (
        quinn::Endpoint,
        quinn::Endpoint,
        quinn::Connection,
        quinn::Connection,
    ) {
        let rcgen::CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
        let certificate = cert.der().clone();
        let private_key = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
        let server_config =
            quinn::ServerConfig::with_single_cert(vec![certificate.clone()], private_key.into())
                .unwrap();
        let server_endpoint =
            quinn::Endpoint::server(server_config, "127.0.0.1:0".parse().unwrap()).unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate).unwrap();
        let client_config = quinn::ClientConfig::with_root_certificates(Arc::new(roots)).unwrap();
        let mut client_endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        client_endpoint.set_default_client_config(client_config);
        let connecting = client_endpoint
            .connect(server_endpoint.local_addr().unwrap(), "localhost")
            .unwrap();
        let incoming = server_endpoint.accept().await.unwrap();
        let (client, server) = tokio::join!(connecting, incoming);
        (
            server_endpoint,
            client_endpoint,
            client.unwrap(),
            server.unwrap(),
        )
    }

    #[test]
    fn recall_recovery_preserves_trigger_binding_and_forces_replay() {
        for (trigger, expected_sequence) in [
            (ProductionRecallTriggerV1::Explicit, Some(77)),
            (ProductionRecallTriggerV1::LinkLost, None),
        ] {
            let stored = recall_terminal(trigger);
            let (event, terminal_id, result_hash) =
                recovered_recall_event(authenticated(), [2; 16], &stored).unwrap();
            assert_eq!(terminal_id, stored.result.terminal_id);
            assert_eq!(result_hash, stored.result_hash);
            let ReliableEvent::RecallResult(result) = event else {
                panic!("unexpected event");
            };
            let RecallResultV1::Stored {
                request_sequence,
                replayed,
                result,
                ..
            } = *result
            else {
                panic!("unexpected result");
            };
            assert_eq!(request_sequence, expected_sequence);
            assert!(replayed);
            assert_eq!(
                result.trigger,
                if expected_sequence.is_some() {
                    RecallTerminalTriggerV1::Explicit
                } else {
                    RecallTerminalTriggerV1::LinkLost
                }
            );
        }
    }

    #[test]
    fn recall_recovery_rejects_foreign_account_character_and_namespace() {
        let stored = recall_terminal(ProductionRecallTriggerV1::Explicit);
        let foreign_account = AuthenticatedAccount {
            account_id: AccountId::new([9; 16]).unwrap(),
            namespace: AuthenticatedNamespace::WipeableTest,
        };
        for outcome in [
            recovered_recall_event(foreign_account, [2; 16], &stored),
            recovered_recall_event(authenticated(), [9; 16], &stored),
            recovered_recall_event(
                AuthenticatedAccount {
                    account_id: AccountId::new([1; 16]).unwrap(),
                    namespace: AuthenticatedNamespace::Production,
                },
                [2; 16],
                &stored,
            ),
        ] {
            assert!(matches!(
                outcome,
                Err(RecoveredTerminalDeliveryError::ForeignRecallAuthority)
            ));
        }
    }

    #[tokio::test]
    async fn recovered_recall_send_uses_current_outer_tick_and_stored_inner_binding() {
        let stored = recall_terminal(ProductionRecallTriggerV1::Explicit);
        let (server_endpoint, client_endpoint, client, server) = live_connection_pair().await;
        let writer = CoreReliableWriter::new(server);
        let proof =
            send_recovered_recall_terminal(&writer, authenticated(), [2; 16], &stored, 2_345)
                .await
                .unwrap();
        assert_eq!(proof.frame().server_tick, 2_345);
        let ReliableEvent::RecallResult(result) = &proof.frame().event else {
            panic!("unexpected event");
        };
        let RecallResultV1::Stored {
            request_sequence,
            replayed,
            result,
            ..
        } = result.as_ref()
        else {
            panic!("unexpected result");
        };
        assert_eq!(*request_sequence, Some(77));
        assert!(*replayed);
        assert_eq!(result.completion_tick, stored.result.completion_tick);

        let mut receive = client.accept_uni().await.unwrap();
        assert!(!receive.read_to_end(65_536).await.unwrap().is_empty());
        client.close(0_u32.into(), b"test complete");
        server_endpoint.wait_idle().await;
        client_endpoint.wait_idle().await;
    }

    #[test]
    fn extraction_retry_validation_accepts_only_sequence_change() {
        let acceptance = acceptance();
        let original = retry(1);
        let current_retry = retry(999);
        assert_ne!(original.sequence, current_retry.sequence);
        validate_retry_against_acceptance(authenticated(), &current_retry, &acceptance).unwrap();
        validate_terminal_against_acceptance(&acceptance, &extraction_terminal()).unwrap();

        let mut altered_time = current_retry.clone();
        altered_time.issued_at_unix_millis += 1;
        assert!(matches!(
            validate_retry_against_acceptance(authenticated(), &altered_time, &acceptance),
            Err(RecoveredTerminalDeliveryError::ExtractionIntentMismatch)
        ));

        let mut altered_payload = current_retry;
        altered_payload.payload.expected_versions.inventory += 1;
        altered_payload.payload_hash = altered_payload.payload.canonical_hash();
        assert!(matches!(
            validate_retry_against_acceptance(authenticated(), &altered_payload, &acceptance),
            Err(RecoveredTerminalDeliveryError::ExtractionIntentMismatch)
        ));
    }

    #[tokio::test]
    async fn exact_extraction_retry_uses_current_inner_sequence_and_current_outer_tick() {
        let repository = FakeRepository {
            acceptance: Some(acceptance()),
            terminal: Some(extraction_terminal()),
        };
        let (server_endpoint, client_endpoint, client, server) = live_connection_pair().await;
        let writer = CoreReliableWriter::new(server);
        let proof = recover_and_send_extraction_retry(
            &repository,
            &writer,
            authenticated(),
            &retry(444),
            1_234,
        )
        .await
        .unwrap();
        assert_eq!(proof.frame().server_tick, 1_234);
        let ReliableEvent::ExtractionCommitResult(result) = &proof.frame().event else {
            panic!("unexpected event");
        };
        let ExtractionCommitResultV1::Stored {
            request_sequence,
            replayed,
            ..
        } = result.as_ref()
        else {
            panic!("unexpected result");
        };
        assert_eq!(*request_sequence, 444);
        assert!(*replayed);

        let mut receive = client.accept_uni().await.unwrap();
        assert!(!receive.read_to_end(65_536).await.unwrap().is_empty());
        client.close(0_u32.into(), b"test complete");
        server_endpoint.wait_idle().await;
        client_endpoint.wait_idle().await;
    }
}
