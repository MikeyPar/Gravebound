//! Live Sir Caldus extraction-intent preparation for the ordinary Core route.
//!
//! Authorities: `Gravebound_Production_GDD_v1_Canonical.md` (`DTH-011`, `TECH-015`,
//! and `TECH-021`-`023`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-ROOM-007`, `CONT-BOSS-001`/`002`, and `CONT-HUB-001`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`/`08`). Accepted
//! `SPEC-CONFLICT-006` and ADR-037 keep normal route admission disabled until this
//! authority is composed with the complete private-life actor.
//!
//! This module accepts client intent only after the server owns a durable Sir Caldus
//! victory exit and a matching `BossExitReady` actor projection. It stages the legacy
//! request row required by the production repository, but never commits the legacy
//! evidence receipt. The resulting prepared extraction remains an opaque input to the
//! shared terminal coordinator and [`crate::ProductionExtractionExecutionService`].

use std::future::Future;

use persistence::{
    CaldusExtractionRequest, CaldusExtractionTransaction, PersistenceError, PostgresPersistence,
    PreparedProductionExtractionV1, ProductionExtractionCommitRequestV1,
    ProductionExtractionCoreRouteRevisionV1, ProductionExtractionExpectedVersionsV1,
    ProductionExtractionIntentAcceptanceTransactionV1, ProductionExtractionIntentAttemptV1,
    StoredExtractionState, StoredProductionExtractionIntentAcceptanceV1, StoredWorldFlowRevisionV1,
    WIPEABLE_CORE_NAMESPACE,
};
use protocol::{
    CorePrivateRoutePhaseV1, CorePrivateRouteRoomV1, CorePrivateRouteSceneV1,
    ExtractionCommitFrameV1, ExtractionCommitResultV1, TERMINAL_INVENTORY_SCHEMA_VERSION,
    TerminalInventoryRejectionCodeV1, TerminalInventoryValidationError, WorldFlowContentRevisionV1,
};
use sim_core::{CoreBossParticipant, CoreBossParticipantLock, CoreCaldusVictoryIdentities};
use thiserror::Error;
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CaldusExitPresentation,
    CaldusInstancePresentation, CoreExtractionIntentAuthority, CoreExtractionIntentReply,
    CorePrivateCaldusDefeatHandoff, CorePrivateCaldusRewardCommit, CorePrivateRouteActorDirectory,
    CorePrivateRouteActorLease, CorePrivateRouteExtractionBinding,
    CorePrivateRouteExtractionExitBinding, CorePrivateRouteExtractionPermit,
    CorePrivateRouteRuntimeError, IdentityClock,
};

pub const CORE_EXTRACTION_ACTOR_MAILBOX_CAPACITY: usize = 8;

const CALDUS_EXIT_ID: &str = "portal.exit.dungeon.bell_sepulcher";
const CALDUS_HALL_ID: &str = "hub.lantern_halls_01";
const TERMINAL_ID_CONTEXT: &str = "gravebound.production-extraction-intent-terminal.v1";

/// Server-owned material required before a client can request the Caldus exit.
///
/// Fields are private so callers must prove that the live route, durable exit presentation,
/// capacity-one participant lock, selected character, restore root, content, and aggregate
/// versions agree at one actor boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductionExtractionBossExitAuthorityV1 {
    account_id: [u8; 16],
    selected_character_id: [u8; 16],
    route_permit: CorePrivateRouteExtractionPermit,
    encounter_id: [u8; 16],
    instance_lineage_id: [u8; 16],
    entry_restore_point_id: [u8; 16],
    exit_instance_id: [u8; 16],
    extraction_request_id: [u8; 16],
    extraction_receipt_id: [u8; 16],
    terminal_id: [u8; 16],
    attempt_ordinal: u32,
    participant: CoreBossParticipant,
    expected_versions: ProductionExtractionExpectedVersionsV1,
    route_content_revision: ProductionExtractionCoreRouteRevisionV1,
    content_revision: StoredWorldFlowRevisionV1,
}

/// Exact route reservation held while `PostgreSQL` produces the final coherent custody snapshot.
/// Callers must consume it through [`Self::seal`] or [`Self::abort`]; dropping it would leave the
/// route intentionally fail-closed in `TerminalPending` for restart reconciliation.
#[derive(Debug)]
#[must_use = "an extraction reservation must be sealed or explicitly aborted"]
pub struct ProductionExtractionCaldusReservationV1 {
    authenticated: AuthenticatedAccount,
    route_directory: CorePrivateRouteActorDirectory,
    route_lease: CorePrivateRouteActorLease,
    permit: CorePrivateRouteExtractionPermit,
    owns_route_reservation: bool,
    handoff: CorePrivateCaldusDefeatHandoff,
    exit: CaldusExitPresentation,
    participant: CoreBossParticipant,
}

impl ProductionExtractionCaldusReservationV1 {
    /// Freezes the exact `BossExitReady` generation before the final custody read. This prevents
    /// ordinary item/world mutations from racing between snapshot and extraction construction.
    pub async fn reserve(
        authenticated: AuthenticatedAccount,
        route_directory: CorePrivateRouteActorDirectory,
        handoff: &CorePrivateCaldusDefeatHandoff,
        commit: &CorePrivateCaldusRewardCommit,
        world_flow_revision: WorldFlowContentRevisionV1,
    ) -> Result<Self, ProductionExtractionIntentError> {
        let lock = handoff.lock();
        let [participant] = lock.participants.as_slice() else {
            return Err(ProductionExtractionIntentError::InvalidRouteAuthority);
        };
        let participant = *participant;
        let route = &commit.route;
        if authenticated.namespace != AuthenticatedNamespace::WipeableTest
            || authenticated.account_id.as_bytes() != handoff.route_lease().account_id()
            || handoff.character_id() != handoff.route_lease().character_id()
            || route.character_id != handoff.character_id()
            || route.actor_generation != handoff.route_lease().actor_generation()
            || route.instance_lineage_id != Some(handoff.instance_lineage_id())
            || route.phase != CorePrivateRoutePhaseV1::BossExitReady
        {
            return Err(ProductionExtractionIntentError::InvalidRouteAuthority);
        }
        let identities = CoreCaldusVictoryIdentities::derive(handoff.instance_lineage_id(), lock)
            .map_err(|_| ProductionExtractionIntentError::InvalidExitAuthority)?;
        let extraction = identities
            .extraction_for(participant)
            .ok_or(ProductionExtractionIntentError::InvalidExitAuthority)?;
        let terminal_id = derive_production_extraction_terminal_id_v1(
            authenticated.account_id.as_bytes(),
            handoff.character_id(),
            identities.encounter_id.bytes(),
            extraction.request_id.bytes(),
            extraction.receipt_id.bytes(),
        )?;
        if commit.exit.exit_instance_id != identities.exit_instance_id.bytes() {
            return Err(ProductionExtractionIntentError::InvalidExitAuthority);
        }
        let exit_binding = CorePrivateRouteExtractionExitBinding::new(
            identities.encounter_id.bytes(),
            identities.exit_instance_id.bytes(),
            extraction.request_id.bytes(),
            extraction.receipt_id.bytes(),
            terminal_id,
        )
        .map_err(|_| ProductionExtractionIntentError::InvalidExitAuthority)?;
        let binding = CorePrivateRouteExtractionBinding::new(
            authenticated.account_id.as_bytes(),
            route.clone(),
            world_flow_revision,
            handoff.entry_restore_point_id(),
            exit_binding,
        )
        .map_err(|_| ProductionExtractionIntentError::InvalidRouteAuthority)?;
        let route_lease = handoff.route_lease();
        let reservation = route_directory
            .prepare_extraction_terminal_owned(route_lease, binding)
            .await
            .map_err(|_| ProductionExtractionIntentError::InvalidRouteAuthority)?;
        let owns_route_reservation = reservation.is_fresh();
        let permit = reservation.permit().clone();
        Ok(Self {
            authenticated,
            route_directory,
            route_lease,
            permit,
            owns_route_reservation,
            handoff: handoff.clone(),
            exit: commit.exit.clone(),
            participant,
        })
    }

    #[must_use]
    pub const fn accepted_route(&self) -> &protocol::CorePrivateRouteStateV1 {
        self.permit.binding().accepted_route()
    }

    #[must_use]
    pub const fn permit(&self) -> &CorePrivateRouteExtractionPermit {
        &self.permit
    }

    #[must_use]
    pub const fn owns_route_reservation(&self) -> bool {
        self.owns_route_reservation
    }

    /// Seals the reserved route against the post-reservation coherent storage versions. A known
    /// mismatch reopens the exact route before returning the validation error.
    pub async fn seal(
        self,
        expected_versions: ProductionExtractionExpectedVersionsV1,
    ) -> Result<ProductionExtractionBossExitAuthorityV1, ProductionExtractionIntentError> {
        let result = ProductionExtractionBossExitAuthorityV1::seal_committed_exit(
            self.authenticated,
            self.handoff.character_id(),
            self.permit.clone(),
            self.handoff.instance_lineage_id(),
            self.handoff.lock().attempt_ordinal,
            &self.exit,
            self.handoff.lock(),
            self.participant,
            expected_versions,
        );
        match result {
            Ok(authority) => Ok(authority),
            Err(error) => {
                if self.owns_route_reservation {
                    self.abort().await?;
                }
                Err(error)
            }
        }
    }

    pub async fn abort(&self) -> Result<(), ProductionExtractionIntentError> {
        if !self.owns_route_reservation {
            return Ok(());
        }
        self.route_directory
            .abort_extraction_terminal(self.route_lease, &self.permit)
            .await
            .map(|_| ())
            .map_err(|_| ProductionExtractionIntentError::RouteReservationAbortFailed)
    }
}

impl ProductionExtractionBossExitAuthorityV1 {
    #[allow(
        clippy::too_many_arguments,
        reason = "the constructor deliberately makes every cross-domain authority explicit"
    )]
    pub fn seal(
        authenticated: AuthenticatedAccount,
        selected_character_id: [u8; 16],
        route_permit: CorePrivateRouteExtractionPermit,
        presentation: &CaldusInstancePresentation,
        lock: &CoreBossParticipantLock,
        participant: CoreBossParticipant,
        expected_versions: ProductionExtractionExpectedVersionsV1,
    ) -> Result<Self, ProductionExtractionIntentError> {
        let exit = presentation
            .exit()
            .ok_or(ProductionExtractionIntentError::ExitNotCommitted)?;
        Self::seal_committed_exit(
            authenticated,
            selected_character_id,
            route_permit,
            presentation.instance_lineage_id(),
            presentation.attempt_ordinal(),
            exit,
            lock,
            participant,
            expected_versions,
        )
    }

    /// Reserves and seals one production extraction authority directly from the frozen Caldus
    /// defeat, its committed `BossExitReady` result, and the coherent post-reward version vector.
    /// The canonical GDD (`TECH-021`-`023`), Content Production Specification
    /// (`CONT-BOSS-001`, `CONT-REWARD-003`), and roadmap (`GB-M03-03`, `GB-M03-08`) require these
    /// authorities to share one server-owned identity. Any failure after reservation invokes the
    /// exact permit abort before returning, so construction cannot strand `TerminalPending`.
    pub async fn prepare_from_caldus_commit(
        authenticated: AuthenticatedAccount,
        route_directory: &CorePrivateRouteActorDirectory,
        handoff: &CorePrivateCaldusDefeatHandoff,
        commit: &CorePrivateCaldusRewardCommit,
        world_flow_revision: WorldFlowContentRevisionV1,
        expected_versions: ProductionExtractionExpectedVersionsV1,
    ) -> Result<Self, ProductionExtractionIntentError> {
        ProductionExtractionCaldusReservationV1::reserve(
            authenticated,
            route_directory.clone(),
            handoff,
            commit,
            world_flow_revision,
        )
        .await?
        .seal(expected_versions)
        .await
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the private seal binds every cross-domain authority without an override object"
    )]
    fn seal_committed_exit(
        authenticated: AuthenticatedAccount,
        selected_character_id: [u8; 16],
        route_permit: CorePrivateRouteExtractionPermit,
        instance_lineage_id: [u8; 16],
        attempt_ordinal: u32,
        exit: &CaldusExitPresentation,
        lock: &CoreBossParticipantLock,
        participant: CoreBossParticipant,
        expected_versions: ProductionExtractionExpectedVersionsV1,
    ) -> Result<Self, ProductionExtractionIntentError> {
        let binding = route_permit.binding();
        let route = binding.accepted_route();
        route
            .validate()
            .map_err(|_| ProductionExtractionIntentError::InvalidRouteAuthority)?;
        if authenticated.namespace != AuthenticatedNamespace::WipeableTest
            || selected_character_id == [0; 16]
            || binding.account_id() != authenticated.account_id.as_bytes()
            || binding.entry_restore_point_id() == [0; 16]
            || route.character_id != selected_character_id
            || route.character_version != expected_versions.character
            || expected_versions.character != expected_versions.world
            || route.actor_generation != route_permit.actor_generation()
            || route.state_version != route_permit.accepted_route_state_version()
            || route.state_version.checked_add(1)
                != Some(route_permit.terminal_pending_route_state_version())
            || route.scene != CorePrivateRouteSceneV1::BellSepulcher
            || route.room != Some(CorePrivateRouteRoomV1::CaldusArenaB6)
            || route.phase != CorePrivateRoutePhaseV1::BossExitReady
            || !route.readiness.extraction_available.is_available()
            || route.instance_lineage_id != Some(instance_lineage_id)
            || lock.attempt_ordinal != attempt_ordinal
            || lock.participants.as_slice() != [participant]
            || participant.party_slot != 0
        {
            return Err(ProductionExtractionIntentError::InvalidRouteAuthority);
        }
        if exit.content_id != CALDUS_EXIT_ID
            || exit.destination_content_id != CALDUS_HALL_ID
            || !exit.requires_committed_extraction_receipt
        {
            return Err(ProductionExtractionIntentError::InvalidExitAuthority);
        }
        let identities = CoreCaldusVictoryIdentities::derive(instance_lineage_id, lock)
            .map_err(|_| ProductionExtractionIntentError::InvalidExitAuthority)?;
        let extraction = identities
            .extraction_for(participant)
            .ok_or(ProductionExtractionIntentError::InvalidExitAuthority)?;
        if exit.exit_instance_id != identities.exit_instance_id.bytes() {
            return Err(ProductionExtractionIntentError::InvalidExitAuthority);
        }
        let exit_binding = binding.exit();
        let terminal_id = derive_production_extraction_terminal_id_v1(
            authenticated.account_id.as_bytes(),
            selected_character_id,
            identities.encounter_id.bytes(),
            extraction.request_id.bytes(),
            extraction.receipt_id.bytes(),
        )?;
        if exit_binding.encounter_id() != identities.encounter_id.bytes()
            || exit_binding.exit_instance_id() != identities.exit_instance_id.bytes()
            || exit_binding.extraction_request_id() != extraction.request_id.bytes()
            || exit_binding.extraction_receipt_id() != extraction.receipt_id.bytes()
            || exit_binding.terminal_id() != terminal_id
        {
            return Err(ProductionExtractionIntentError::InvalidExitAuthority);
        }
        let route_content_revision =
            stored_core_route_revision(route_permit.route_content_revision());
        let content_revision = stored_revision(route_permit.world_flow_revision());
        if !valid_core_route_revision(&route_content_revision) || !valid_revision(&content_revision)
        {
            return Err(ProductionExtractionIntentError::InvalidContentAuthority);
        }
        let entry_restore_point_id = binding.entry_restore_point_id();
        let authority = Self {
            account_id: authenticated.account_id.as_bytes(),
            selected_character_id,
            route_permit,
            encounter_id: identities.encounter_id.bytes(),
            instance_lineage_id,
            entry_restore_point_id,
            exit_instance_id: identities.exit_instance_id.bytes(),
            extraction_request_id: extraction.request_id.bytes(),
            extraction_receipt_id: extraction.receipt_id.bytes(),
            terminal_id,
            attempt_ordinal: lock.attempt_ordinal,
            participant,
            expected_versions,
            route_content_revision,
            content_revision,
        };
        authority.validate()?;
        Ok(authority)
    }

    #[must_use]
    pub const fn account_id(&self) -> [u8; 16] {
        self.account_id
    }

    #[must_use]
    pub const fn selected_character_id(&self) -> [u8; 16] {
        self.selected_character_id
    }

    #[must_use]
    pub const fn actor_generation(&self) -> u64 {
        self.route_permit.actor_generation()
    }

    #[must_use]
    pub const fn instance_lineage_id(&self) -> [u8; 16] {
        self.instance_lineage_id
    }

    #[must_use]
    pub const fn entry_restore_point_id(&self) -> [u8; 16] {
        self.entry_restore_point_id
    }

    #[must_use]
    pub const fn route_state_version(&self) -> u64 {
        self.route_permit.accepted_route_state_version()
    }

    #[must_use]
    pub const fn route_permit(&self) -> &CorePrivateRouteExtractionPermit {
        &self.route_permit
    }

    /// Reopens only this uncommitted exact reservation. Construction/session composition uses
    /// this after removing an actor that never accepted client intent or retained a terminal.
    pub async fn abort_uncommitted_route(
        &self,
        route_directory: &CorePrivateRouteActorDirectory,
        route_lease: CorePrivateRouteActorLease,
    ) -> Result<(), CorePrivateRouteRuntimeError> {
        if route_lease.account_id() != self.account_id
            || route_lease.character_id() != self.selected_character_id
            || route_lease.actor_generation() != self.actor_generation()
        {
            return Err(CorePrivateRouteRuntimeError::InvalidExtractionBinding);
        }
        route_directory
            .abort_extraction_terminal(route_lease, &self.route_permit)
            .await
            .map(|_| ())
    }

    #[must_use]
    pub const fn extraction_request_id(&self) -> [u8; 16] {
        self.extraction_request_id
    }

    fn validate(&self) -> Result<(), ProductionExtractionIntentError> {
        if [
            self.account_id,
            self.selected_character_id,
            self.encounter_id,
            self.instance_lineage_id,
            self.entry_restore_point_id,
            self.exit_instance_id,
            self.extraction_request_id,
            self.extraction_receipt_id,
            self.terminal_id,
        ]
        .contains(&[0; 16])
            || self.route_permit.actor_generation() == 0
            || self.route_permit.accepted_route_state_version() == 0
            || self
                .route_permit
                .accepted_route_state_version()
                .checked_add(1)
                != Some(self.route_permit.terminal_pending_route_state_version())
            || self.attempt_ordinal == 0
            || self.participant.party_slot != 0
            || [
                self.expected_versions.account,
                self.expected_versions.character,
                self.expected_versions.world,
                self.expected_versions.inventory,
                self.expected_versions.life_metrics,
            ]
            .contains(&0)
            || self.expected_versions.character != self.expected_versions.world
            || !valid_core_route_revision(&self.route_content_revision)
            || !valid_revision(&self.content_revision)
            || self.route_permit.binding().account_id() != self.account_id
            || self.route_permit.binding().accepted_route().character_id
                != self.selected_character_id
            || self.route_permit.binding().entry_restore_point_id() != self.entry_restore_point_id
            || self.route_permit.binding().exit().encounter_id() != self.encounter_id
            || self.route_permit.binding().exit().exit_instance_id() != self.exit_instance_id
            || self.route_permit.binding().exit().extraction_request_id()
                != self.extraction_request_id
            || self.route_permit.binding().exit().extraction_receipt_id()
                != self.extraction_receipt_id
            || self.route_permit.binding().exit().terminal_id() != self.terminal_id
        {
            return Err(ProductionExtractionIntentError::InvalidRouteAuthority);
        }
        Ok(())
    }

    fn planner_input(
        &self,
        frame: &ExtractionCommitFrameV1,
        server_tick: u64,
    ) -> Result<ProductionExtractionPlannerInputV1, TerminalInventoryRejectionCodeV1> {
        if server_tick == 0 {
            return Err(TerminalInventoryRejectionCodeV1::SourceUnavailable);
        }
        if frame.character_id != self.selected_character_id
            || frame.payload.extraction_request_id != self.extraction_request_id
        {
            return Err(TerminalInventoryRejectionCodeV1::ForeignAuthority);
        }
        let expected = frame.payload.expected_versions;
        if expected.account != self.expected_versions.account
            || expected.character != self.expected_versions.character
            || expected.world != self.expected_versions.world
            || expected.inventory != self.expected_versions.inventory
            || expected.life_clock != self.expected_versions.life_metrics
        {
            return Err(TerminalInventoryRejectionCodeV1::StaleAuthority);
        }
        let content_revision = stored_revision(&frame.payload.content_revision);
        if content_revision != self.content_revision {
            return Err(TerminalInventoryRejectionCodeV1::ContentMismatch);
        }
        let staged_request = CaldusExtractionRequest {
            account_id: self.account_id,
            character_id: self.selected_character_id,
            extraction_request_id: self.extraction_request_id,
            encounter_id: self.encounter_id,
            instance_lineage_id: self.instance_lineage_id,
            entry_restore_point_id: self.entry_restore_point_id,
            exit_instance_id: self.exit_instance_id,
            attempt_ordinal: self.attempt_ordinal,
            party_slot: self.participant.party_slot,
            participant_entity_id: self.participant.entity_id.get(),
            expected_character_version: self.expected_versions.character,
            content_revision: self.content_revision.clone(),
        };
        let commit_request = self.commit_request(frame, server_tick);
        let input = ProductionExtractionPlannerInputV1 {
            staged_request,
            commit_request,
        };
        input
            .validate()
            .map_err(|_| TerminalInventoryRejectionCodeV1::InvalidRequest)?;
        Ok(input)
    }

    fn commit_request(
        &self,
        frame: &ExtractionCommitFrameV1,
        server_tick: u64,
    ) -> ProductionExtractionCommitRequestV1 {
        ProductionExtractionCommitRequestV1 {
            contract_version: persistence::PRODUCTION_EXTRACTION_CONTRACT_VERSION_V1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.to_owned(),
            account_id: self.account_id,
            character_id: frame.character_id,
            mutation_id: frame.mutation_id,
            terminal_id: self.terminal_id,
            extraction_request_id: frame.payload.extraction_request_id,
            extraction_receipt_id: self.extraction_receipt_id,
            encounter_id: self.encounter_id,
            instance_lineage_id: self.instance_lineage_id,
            entry_restore_point_id: self.entry_restore_point_id,
            exit_instance_id: self.exit_instance_id,
            expected_versions: ProductionExtractionExpectedVersionsV1 {
                account: frame.payload.expected_versions.account,
                character: frame.payload.expected_versions.character,
                world: frame.payload.expected_versions.world,
                inventory: frame.payload.expected_versions.inventory,
                life_metrics: frame.payload.expected_versions.life_clock,
            },
            content_revision: stored_revision(&frame.payload.content_revision),
            issued_at_unix_ms: frame.issued_at_unix_millis,
            observed_tick: server_tick,
        }
    }

    fn intent_attempt(
        &self,
        frame: &ExtractionCommitFrameV1,
        server_tick: u64,
    ) -> Result<ProductionExtractionIntentAttemptV1, PersistenceError> {
        let attempt = ProductionExtractionIntentAttemptV1 {
            contract_version: persistence::PRODUCTION_EXTRACTION_INTENT_CONTRACT_VERSION_V1,
            namespace_id: WIPEABLE_CORE_NAMESPACE.to_owned(),
            authenticated_account_id: self.account_id,
            attempted_character_id: frame.character_id,
            attempted_mutation_id: frame.mutation_id,
            attempted_frame_schema_version: frame.schema_version,
            attempted_frame_payload_hash: frame.payload_hash,
            extraction_request_id: frame.payload.extraction_request_id,
            extraction_receipt_id: self.extraction_receipt_id,
            terminal_id: self.terminal_id,
            actor_generation: self.route_permit.actor_generation(),
            accepted_pre_route_state_version: self.route_permit.accepted_route_state_version(),
            accepted_post_route_state_version: self
                .route_permit
                .terminal_pending_route_state_version(),
            core_route_revision: self.route_content_revision.clone(),
            world_flow_revision: stored_revision(&frame.payload.content_revision),
            commit_request: self.commit_request(frame, server_tick),
            issued_at_unix_ms: frame.issued_at_unix_millis,
            observed_tick: server_tick,
        };
        attempt.validate()?;
        Ok(attempt)
    }
}

/// Complete deterministic input to the request-staging and atomic extraction planners.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductionExtractionPlannerInputV1 {
    staged_request: CaldusExtractionRequest,
    commit_request: ProductionExtractionCommitRequestV1,
}

impl ProductionExtractionPlannerInputV1 {
    #[must_use]
    pub const fn staged_request(&self) -> &CaldusExtractionRequest {
        &self.staged_request
    }

    #[must_use]
    pub const fn commit_request(&self) -> &ProductionExtractionCommitRequestV1 {
        &self.commit_request
    }

    fn validate(&self) -> Result<(), PersistenceError> {
        self.commit_request.validate()?;
        if self.staged_request.account_id != self.commit_request.account_id
            || self.staged_request.character_id != self.commit_request.character_id
            || self.staged_request.extraction_request_id
                != self.commit_request.extraction_request_id
            || self.staged_request.encounter_id != self.commit_request.encounter_id
            || self.staged_request.instance_lineage_id != self.commit_request.instance_lineage_id
            || self.staged_request.entry_restore_point_id
                != self.commit_request.entry_restore_point_id
            || self.staged_request.exit_instance_id != self.commit_request.exit_instance_id
            || self.staged_request.expected_character_version
                != self.commit_request.expected_versions.character
            || self.staged_request.content_revision != self.commit_request.content_revision
        {
            return Err(PersistenceError::CorruptStoredExtraction);
        }
        Ok(())
    }
}

/// Repository planner seam. The `PostgreSQL` implementation stages only the idempotent Caldus
/// request row, verifies it remains uncommitted evidence authority, then invokes the existing
/// read-only production extraction planner.
pub trait ProductionExtractionPlanner: Send + Sync {
    fn load_intent_acceptance(
        &self,
        extraction_request_id: [u8; 16],
    ) -> impl Future<
        Output = Result<Option<StoredProductionExtractionIntentAcceptanceV1>, PersistenceError>,
    > + Send;

    fn accept_intent(
        &self,
        attempt: &ProductionExtractionIntentAttemptV1,
    ) -> impl Future<
        Output = Result<ProductionExtractionIntentAcceptanceTransactionV1, PersistenceError>,
    > + Send;

    fn prepare(
        &self,
        input: &ProductionExtractionPlannerInputV1,
    ) -> impl Future<Output = Result<PreparedProductionExtractionV1, PersistenceError>> + Send;
}

impl ProductionExtractionPlanner for PostgresPersistence {
    async fn load_intent_acceptance(
        &self,
        extraction_request_id: [u8; 16],
    ) -> Result<Option<StoredProductionExtractionIntentAcceptanceV1>, PersistenceError> {
        self.load_production_extraction_intent_acceptance_v1(extraction_request_id)
            .await
    }

    async fn accept_intent(
        &self,
        attempt: &ProductionExtractionIntentAttemptV1,
    ) -> Result<ProductionExtractionIntentAcceptanceTransactionV1, PersistenceError> {
        self.accept_production_extraction_intent_v1(attempt).await
    }

    async fn prepare(
        &self,
        input: &ProductionExtractionPlannerInputV1,
    ) -> Result<PreparedProductionExtractionV1, PersistenceError> {
        input.validate()?;
        let staged = match self
            .request_caldus_extraction(input.staged_request())
            .await?
        {
            CaldusExtractionTransaction::Fresh(staged)
            | CaldusExtractionTransaction::Replay(staged) => staged,
        };
        if staged.request != *input.staged_request()
            || staged.state != StoredExtractionState::Requested
            || staged.extraction_receipt_id.is_some()
            || staged.authority.is_some()
            || staged.transfer_mutation_id.is_some()
            || staged.post_character_version.is_some()
        {
            return Err(PersistenceError::CorruptStoredExtraction);
        }
        self.prepare_production_extraction_v1(input.commit_request())
            .await
    }
}

/// Actor-owned preparation pinned to the first accepted frame and authoritative tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductionExtractionPreparedIntentV1 {
    frame: ExtractionCommitFrameV1,
    server_tick: u64,
    acceptance: StoredProductionExtractionIntentAcceptanceV1,
    input: ProductionExtractionPlannerInputV1,
    prepared: Option<PreparedProductionExtractionV1>,
}

impl ProductionExtractionPreparedIntentV1 {
    #[must_use]
    pub const fn server_tick(&self) -> u64 {
        self.server_tick
    }

    #[must_use]
    pub const fn input(&self) -> &ProductionExtractionPlannerInputV1 {
        &self.input
    }

    #[must_use]
    pub const fn acceptance(&self) -> &StoredProductionExtractionIntentAcceptanceV1 {
        &self.acceptance
    }

    #[must_use]
    pub const fn prepared(&self) -> Option<&PreparedProductionExtractionV1> {
        self.prepared.as_ref()
    }

    #[must_use]
    pub(crate) const fn frame(&self) -> &ExtractionCommitFrameV1 {
        &self.frame
    }

    fn exact_frame(&self, frame: &ExtractionCommitFrameV1) -> bool {
        self.frame.schema_version == frame.schema_version
            && self.frame.mutation_id == frame.mutation_id
            && self.frame.character_id == frame.character_id
            && self.frame.issued_at_unix_millis == frame.issued_at_unix_millis
            && self.frame.payload_hash == frame.payload_hash
            && self.frame.payload == frame.payload
    }
}

#[derive(Debug)]
pub struct ProductionExtractionIntentActor<Planner, Clock> {
    authority: ProductionExtractionBossExitAuthorityV1,
    route_directory: CorePrivateRouteActorDirectory,
    route_lease: CorePrivateRouteActorLease,
    planner: Planner,
    clock: Clock,
    intent: Mutex<Option<ProductionExtractionPreparedIntentV1>>,
}

impl<Planner, Clock> ProductionExtractionIntentActor<Planner, Clock> {
    pub fn new(
        authority: ProductionExtractionBossExitAuthorityV1,
        route_directory: CorePrivateRouteActorDirectory,
        route_lease: CorePrivateRouteActorLease,
        planner: Planner,
        clock: Clock,
    ) -> Result<Self, ProductionExtractionIntentError> {
        authority.validate()?;
        if route_lease.account_id() != authority.account_id
            || route_lease.character_id() != authority.selected_character_id
            || route_lease.actor_generation() != authority.actor_generation()
        {
            return Err(ProductionExtractionIntentError::InvalidRouteAuthority);
        }
        Ok(Self {
            authority,
            route_directory,
            route_lease,
            planner,
            clock,
            intent: Mutex::new(None),
        })
    }

    #[must_use]
    pub const fn authority(&self) -> &ProductionExtractionBossExitAuthorityV1 {
        &self.authority
    }

    #[must_use]
    pub async fn prepared_intent(&self) -> Option<ProductionExtractionPreparedIntentV1> {
        self.intent.lock().await.clone()
    }

    /// Retires the route generation only after the extraction runtime has retained a committed
    /// terminal result. Durable bootstrap owns recovery if the process ends before this cleanup.
    pub async fn retire_route_after_terminal(&self) -> Result<(), CorePrivateRouteRuntimeError> {
        self.route_directory.retire_actor(self.route_lease).await
    }

    /// Reopens the exact reserved route when actor registration/session activation fails before
    /// any client intent or terminal result exists.
    pub async fn abort_route_after_failed_construction(
        &self,
    ) -> Result<(), CorePrivateRouteRuntimeError> {
        self.authority
            .abort_uncommitted_route(&self.route_directory, self.route_lease)
            .await
    }
}

impl<Planner, Clock> ProductionExtractionIntentActor<Planner, Clock>
where
    Planner: ProductionExtractionPlanner,
    Clock: IdentityClock,
{
    pub async fn handle(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &ExtractionCommitFrameV1,
        server_tick: u64,
    ) -> CoreExtractionIntentReply {
        if let Err(error) = frame.validate() {
            let code = if error == TerminalInventoryValidationError::PayloadHashMismatch {
                TerminalInventoryRejectionCodeV1::PayloadHashMismatch
            } else {
                TerminalInventoryRejectionCodeV1::InvalidRequest
            };
            return rejection_reply(frame, server_tick, code);
        }
        if authenticated.namespace != AuthenticatedNamespace::WipeableTest
            || authenticated.account_id.as_bytes() != self.authority.account_id
        {
            return rejection_reply(
                frame,
                server_tick,
                TerminalInventoryRejectionCodeV1::ForeignAuthority,
            );
        }

        let mut intent = self.intent.lock().await;
        if let Some(existing) = intent.as_ref() {
            if !existing.exact_frame(frame) {
                return self.audit_changed_replay(frame, existing.server_tick).await;
            }
            if let Err(code) = self.revalidate_route_permit().await {
                return rejection_reply(frame, existing.server_tick, code);
            }
            if existing.prepared.is_some() {
                return pending_reply(frame, existing.server_tick);
            }
        } else {
            if frame.payload.extraction_request_id != self.authority.extraction_request_id {
                return rejection_reply(
                    frame,
                    server_tick,
                    TerminalInventoryRejectionCodeV1::ForeignAuthority,
                );
            }
            match self.recover_persisted_intent(frame, server_tick).await {
                Ok(Some(recovered)) => *intent = Some(recovered),
                Ok(None) => {
                    if frame.character_id != self.authority.selected_character_id {
                        return rejection_reply(
                            frame,
                            server_tick,
                            TerminalInventoryRejectionCodeV1::ForeignAuthority,
                        );
                    }
                    let accepted = match self.accept_new_intent(frame, server_tick).await {
                        Ok(accepted) => accepted,
                        Err(code) => return rejection_reply(frame, server_tick, code),
                    };
                    *intent = Some(accepted);
                }
                Err(reply) => return reply,
            }
        }

        let Some(pinned) = intent.as_mut() else {
            return rejection_reply(
                frame,
                server_tick,
                TerminalInventoryRejectionCodeV1::SourceUnavailable,
            );
        };
        self.prepare_pinned_intent(frame, pinned).await
    }

    async fn recover_persisted_intent(
        &self,
        frame: &ExtractionCommitFrameV1,
        fallback_server_tick: u64,
    ) -> Result<Option<ProductionExtractionPreparedIntentV1>, CoreExtractionIntentReply> {
        let acceptance = self
            .planner
            .load_intent_acceptance(frame.payload.extraction_request_id)
            .await
            .map_err(|error| {
                rejection_reply(frame, fallback_server_tick, planner_error_code(&error))
            })?;
        let Some(acceptance) = acceptance else {
            return Ok(None);
        };
        let pinned_server_tick = acceptance.attempt.observed_tick;
        if !exact_persisted_frame(&acceptance, frame) {
            return Err(self.audit_changed_replay(frame, pinned_server_tick).await);
        }
        if acceptance.attempt.core_route_revision != self.authority.route_content_revision {
            return Err(rejection_reply(
                frame,
                pinned_server_tick,
                TerminalInventoryRejectionCodeV1::ContentMismatch,
            ));
        }
        if let Err(code) = self.revalidate_route_permit().await {
            return Err(rejection_reply(frame, pinned_server_tick, code));
        }
        let input = self
            .authority
            .planner_input(frame, pinned_server_tick)
            .map_err(|code| rejection_reply(frame, pinned_server_tick, code))?;
        if acceptance.attempt.commit_request != *input.commit_request() {
            return Err(rejection_reply(
                frame,
                pinned_server_tick,
                TerminalInventoryRejectionCodeV1::CorruptStoredAuthority,
            ));
        }
        Ok(Some(ProductionExtractionPreparedIntentV1 {
            frame: frame.clone(),
            server_tick: pinned_server_tick,
            acceptance,
            input,
            prepared: None,
        }))
    }

    async fn audit_changed_replay(
        &self,
        frame: &ExtractionCommitFrameV1,
        pinned_server_tick: u64,
    ) -> CoreExtractionIntentReply {
        if frame.payload.extraction_request_id != self.authority.extraction_request_id {
            return rejection_reply(
                frame,
                pinned_server_tick,
                TerminalInventoryRejectionCodeV1::ForeignAuthority,
            );
        }
        if frame.issued_at_unix_millis > self.clock.unix_millis() {
            return rejection_reply(
                frame,
                pinned_server_tick,
                TerminalInventoryRejectionCodeV1::IssuedAtInvalid,
            );
        }
        let Ok(attempt) = self.authority.intent_attempt(frame, pinned_server_tick) else {
            return rejection_reply(
                frame,
                pinned_server_tick,
                TerminalInventoryRejectionCodeV1::InvalidRequest,
            );
        };
        match self.planner.accept_intent(&attempt).await {
            Ok(ProductionExtractionIntentAcceptanceTransactionV1::Conflict { .. }) => {
                rejection_reply(
                    frame,
                    pinned_server_tick,
                    TerminalInventoryRejectionCodeV1::IdempotencyConflict,
                )
            }
            Ok(
                ProductionExtractionIntentAcceptanceTransactionV1::Fresh(_)
                | ProductionExtractionIntentAcceptanceTransactionV1::Replayed(_),
            ) => rejection_reply(
                frame,
                pinned_server_tick,
                TerminalInventoryRejectionCodeV1::CorruptStoredAuthority,
            ),
            Err(error) => rejection_reply(frame, pinned_server_tick, planner_error_code(&error)),
        }
    }

    async fn accept_new_intent(
        &self,
        frame: &ExtractionCommitFrameV1,
        server_tick: u64,
    ) -> Result<ProductionExtractionPreparedIntentV1, TerminalInventoryRejectionCodeV1> {
        if frame.issued_at_unix_millis > self.clock.unix_millis() {
            return Err(TerminalInventoryRejectionCodeV1::IssuedAtInvalid);
        }
        self.revalidate_route_permit().await?;
        let input = self.authority.planner_input(frame, server_tick)?;
        let attempt = match self.authority.intent_attempt(frame, server_tick) {
            Ok(attempt) if attempt.commit_request == *input.commit_request() => attempt,
            Ok(_) => return Err(TerminalInventoryRejectionCodeV1::CorruptStoredAuthority),
            Err(_) => return Err(TerminalInventoryRejectionCodeV1::InvalidRequest),
        };
        let acceptance = match self.planner.accept_intent(&attempt).await {
            Ok(
                ProductionExtractionIntentAcceptanceTransactionV1::Fresh(acceptance)
                | ProductionExtractionIntentAcceptanceTransactionV1::Replayed(acceptance),
            ) if acceptance.attempt == attempt => acceptance,
            Ok(ProductionExtractionIntentAcceptanceTransactionV1::Conflict { .. }) => {
                return Err(TerminalInventoryRejectionCodeV1::IdempotencyConflict);
            }
            Ok(_) => return Err(TerminalInventoryRejectionCodeV1::CorruptStoredAuthority),
            Err(error) => return Err(planner_error_code(&error)),
        };
        self.revalidate_route_permit().await?;
        Ok(ProductionExtractionPreparedIntentV1 {
            frame: frame.clone(),
            server_tick,
            acceptance,
            input,
            prepared: None,
        })
    }

    async fn prepare_pinned_intent(
        &self,
        frame: &ExtractionCommitFrameV1,
        pinned: &mut ProductionExtractionPreparedIntentV1,
    ) -> CoreExtractionIntentReply {
        match self.planner.prepare(&pinned.input).await {
            Ok(prepared)
                if prepared.validate().is_ok()
                    && prepared.request() == pinned.input.commit_request() =>
            {
                if let Err(code) = self.revalidate_route_permit().await {
                    return rejection_reply(frame, pinned.server_tick, code);
                }
                pinned.prepared = Some(prepared);
                pending_reply(frame, pinned.server_tick)
            }
            Ok(_) => rejection_reply(
                frame,
                pinned.server_tick,
                TerminalInventoryRejectionCodeV1::CorruptStoredAuthority,
            ),
            Err(error) => rejection_reply(frame, pinned.server_tick, planner_error_code(&error)),
        }
    }

    async fn revalidate_route_permit(&self) -> Result<(), TerminalInventoryRejectionCodeV1> {
        self.route_directory
            .revalidate_extraction_terminal(self.route_lease, self.authority.route_permit())
            .await
            .map_err(|error| route_error_code(&error))
    }
}

#[derive(Debug, Clone)]
pub struct CoreExtractionActorHandle {
    sender: mpsc::Sender<CoreExtractionActorCommand>,
}

#[derive(Debug)]
pub struct CoreExtractionActorInbox {
    receiver: mpsc::Receiver<CoreExtractionActorCommand>,
}

#[derive(Debug)]
struct CoreExtractionActorCommand {
    authenticated: AuthenticatedAccount,
    frame: ExtractionCommitFrameV1,
    fallback_server_tick: u64,
    reply: oneshot::Sender<CoreExtractionIntentReply>,
}

#[must_use]
pub fn production_extraction_actor_mailbox() -> (CoreExtractionActorHandle, CoreExtractionActorInbox)
{
    let (sender, receiver) = mpsc::channel(CORE_EXTRACTION_ACTOR_MAILBOX_CAPACITY);
    (
        CoreExtractionActorHandle { sender },
        CoreExtractionActorInbox { receiver },
    )
}

impl CoreExtractionIntentAuthority for CoreExtractionActorHandle {
    #[allow(
        clippy::manual_async_fn,
        reason = "the desugared trait contract guarantees a Send future for QUIC workers"
    )]
    fn handle_extraction<'a>(
        &'a self,
        authenticated: AuthenticatedAccount,
        frame: &'a ExtractionCommitFrameV1,
        fallback_server_tick: u64,
    ) -> impl Future<Output = CoreExtractionIntentReply> + Send + 'a {
        async move {
            if let Err(error) = frame.validate() {
                let code = if error == TerminalInventoryValidationError::PayloadHashMismatch {
                    TerminalInventoryRejectionCodeV1::PayloadHashMismatch
                } else {
                    TerminalInventoryRejectionCodeV1::InvalidRequest
                };
                return CoreExtractionIntentReply {
                    server_tick: fallback_server_tick,
                    result: rejected(frame, code),
                };
            }
            if authenticated.namespace != AuthenticatedNamespace::WipeableTest {
                return CoreExtractionIntentReply {
                    server_tick: fallback_server_tick,
                    result: rejected(frame, TerminalInventoryRejectionCodeV1::ForeignAuthority),
                };
            }
            let (reply, receive) = oneshot::channel();
            if self
                .sender
                .send(CoreExtractionActorCommand {
                    authenticated,
                    frame: frame.clone(),
                    fallback_server_tick,
                    reply,
                })
                .await
                .is_err()
            {
                return CoreExtractionIntentReply {
                    server_tick: fallback_server_tick,
                    result: rejected(frame, TerminalInventoryRejectionCodeV1::SourceUnavailable),
                };
            }
            receive.await.unwrap_or_else(|_| CoreExtractionIntentReply {
                server_tick: fallback_server_tick,
                result: rejected(frame, TerminalInventoryRejectionCodeV1::SourceUnavailable),
            })
        }
    }
}

impl CoreExtractionActorInbox {
    pub fn close(&mut self) {
        self.receiver.close();
    }

    #[must_use]
    pub fn queued_command_count(&self) -> usize {
        self.receiver.len()
    }

    pub async fn serve_next<Planner, Clock>(
        &mut self,
        actor: &ProductionExtractionIntentActor<Planner, Clock>,
        authoritative_tick: u64,
    ) -> bool
    where
        Planner: ProductionExtractionPlanner,
        Clock: IdentityClock,
    {
        self.serve_next_with_tick(actor, || Some(authoritative_tick))
            .await
            .unwrap_or(false)
    }

    pub(crate) async fn serve_next_with_tick<Planner, Clock, Tick>(
        &mut self,
        actor: &ProductionExtractionIntentActor<Planner, Clock>,
        authoritative_tick: Tick,
    ) -> Result<bool, ()>
    where
        Planner: ProductionExtractionPlanner,
        Clock: IdentityClock,
        Tick: FnOnce() -> Option<u64>,
    {
        let Some(command) = self.receiver.recv().await else {
            return Ok(false);
        };
        let Some(authoritative_tick) = authoritative_tick() else {
            let _ = command.reply.send(CoreExtractionIntentReply {
                server_tick: command.fallback_server_tick,
                result: rejected(
                    &command.frame,
                    TerminalInventoryRejectionCodeV1::SourceUnavailable,
                ),
            });
            return Err(());
        };
        let reply = actor
            .handle(command.authenticated, &command.frame, authoritative_tick)
            .await;
        let _ = command.reply.send(reply);
        Ok(true)
    }
}

fn pending(frame: &ExtractionCommitFrameV1) -> ExtractionCommitResultV1 {
    ExtractionCommitResultV1::Pending {
        schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
        request_sequence: frame.sequence,
        mutation_id: frame.mutation_id,
        character_id: frame.character_id,
        extraction_request_id: frame.payload.extraction_request_id,
    }
}

fn exact_persisted_frame(
    acceptance: &StoredProductionExtractionIntentAcceptanceV1,
    frame: &ExtractionCommitFrameV1,
) -> bool {
    let attempt = &acceptance.attempt;
    attempt.attempted_character_id == frame.character_id
        && attempt.attempted_mutation_id == frame.mutation_id
        && attempt.attempted_frame_schema_version == frame.schema_version
        && attempt.attempted_frame_payload_hash == frame.payload_hash
        && attempt.extraction_request_id == frame.payload.extraction_request_id
        && attempt.issued_at_unix_ms == frame.issued_at_unix_millis
}

fn pending_reply(frame: &ExtractionCommitFrameV1, server_tick: u64) -> CoreExtractionIntentReply {
    CoreExtractionIntentReply {
        server_tick,
        result: pending(frame),
    }
}

fn rejected(
    frame: &ExtractionCommitFrameV1,
    code: TerminalInventoryRejectionCodeV1,
) -> ExtractionCommitResultV1 {
    ExtractionCommitResultV1::Rejected {
        schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
        request_sequence: frame.sequence,
        mutation_id: frame.mutation_id,
        character_id: frame.character_id,
        extraction_request_id: frame.payload.extraction_request_id,
        code,
    }
}

fn rejection_reply(
    frame: &ExtractionCommitFrameV1,
    server_tick: u64,
    code: TerminalInventoryRejectionCodeV1,
) -> CoreExtractionIntentReply {
    CoreExtractionIntentReply {
        server_tick,
        result: rejected(frame, code),
    }
}

fn planner_error_code(error: &PersistenceError) -> TerminalInventoryRejectionCodeV1 {
    match error {
        PersistenceError::ExtractionIdempotencyConflict => {
            TerminalInventoryRejectionCodeV1::IdempotencyConflict
        }
        PersistenceError::ProductionExtractionVersionMismatch { .. } => {
            TerminalInventoryRejectionCodeV1::StaleAuthority
        }
        PersistenceError::ProductionExtractionContentMismatch => {
            TerminalInventoryRejectionCodeV1::ContentMismatch
        }
        PersistenceError::ProductionExtractionUnresolvedMutation => {
            TerminalInventoryRejectionCodeV1::UnresolvedMutation
        }
        PersistenceError::ProductionExtractionOwnerNotFound
        | PersistenceError::ProductionExtractionBindingMismatch
        | PersistenceError::ProductionExtractionTerminalSuperseded
        | PersistenceError::ProductionExtractionIntentAuthorityMismatch => {
            TerminalInventoryRejectionCodeV1::TerminalLost
        }
        PersistenceError::CorruptStoredExtraction
        | PersistenceError::CorruptStoredProductionExtractionIntent
        | PersistenceError::ProductionExtractionPlanChanged
        | PersistenceError::ProductionExtractionPlanningFailed => {
            TerminalInventoryRejectionCodeV1::CorruptStoredAuthority
        }
        _ => TerminalInventoryRejectionCodeV1::DatabaseUnavailable,
    }
}

fn route_error_code(error: &CorePrivateRouteRuntimeError) -> TerminalInventoryRejectionCodeV1 {
    match error {
        CorePrivateRouteRuntimeError::Retired
        | CorePrivateRouteRuntimeError::ActorUnavailable
        | CorePrivateRouteRuntimeError::StaleGeneration
        | CorePrivateRouteRuntimeError::StaleRouteState
        | CorePrivateRouteRuntimeError::ExtractionNotReady
        | CorePrivateRouteRuntimeError::TerminalInProgress
        | CorePrivateRouteRuntimeError::TerminalReservationConflict
        | CorePrivateRouteRuntimeError::TransferInProgress => {
            TerminalInventoryRejectionCodeV1::TerminalLost
        }
        CorePrivateRouteRuntimeError::RuntimeUnavailable
        | CorePrivateRouteRuntimeError::ShutdownNotStarted
        | CorePrivateRouteRuntimeError::ActorTaskFailed(_) => {
            TerminalInventoryRejectionCodeV1::SourceUnavailable
        }
        CorePrivateRouteRuntimeError::InvalidActorBinding
        | CorePrivateRouteRuntimeError::AccountAlreadyActive
        | CorePrivateRouteRuntimeError::ActorAlreadyRegistered
        | CorePrivateRouteRuntimeError::InvalidExtractionBinding
        | CorePrivateRouteRuntimeError::ContentAuthorityMismatch
        | CorePrivateRouteRuntimeError::Actor(_) => {
            TerminalInventoryRejectionCodeV1::CorruptStoredAuthority
        }
    }
}

fn stored_revision(revision: &WorldFlowContentRevisionV1) -> StoredWorldFlowRevisionV1 {
    StoredWorldFlowRevisionV1 {
        records_blake3: revision.records_blake3.as_str().to_owned(),
        assets_blake3: revision.assets_blake3.as_str().to_owned(),
        localization_blake3: revision.localization_blake3.as_str().to_owned(),
    }
}

fn stored_core_route_revision(
    revision: &protocol::CorePrivateRouteContentRevisionV1,
) -> ProductionExtractionCoreRouteRevisionV1 {
    ProductionExtractionCoreRouteRevisionV1 {
        records_blake3: revision.records_blake3.as_str().to_owned(),
        assets_blake3: revision.assets_blake3.as_str().to_owned(),
        localization_blake3: revision.localization_blake3.as_str().to_owned(),
    }
}

fn valid_core_route_revision(revision: &ProductionExtractionCoreRouteRevisionV1) -> bool {
    valid_hashes([
        revision.records_blake3.as_str(),
        revision.assets_blake3.as_str(),
        revision.localization_blake3.as_str(),
    ])
}

fn valid_revision(revision: &StoredWorldFlowRevisionV1) -> bool {
    valid_hashes([
        revision.records_blake3.as_str(),
        revision.assets_blake3.as_str(),
        revision.localization_blake3.as_str(),
    ])
}

fn valid_hashes(hashes: [&str; 3]) -> bool {
    hashes.iter().all(|hash| {
        hash.len() == 64
            && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            && !hash.bytes().all(|byte| byte == b'0')
    })
}

/// Derives the stable terminal identity shared by the route reservation, intent acceptance,
/// terminal arbiter, and extraction repository.
///
/// Inputs must come from authenticated server state and the committed Caldus exit. Publishing
/// this deterministic helper does not promote client material into authority; the sealed exit
/// authority still cross-checks every identity against the opaque route permit and durable
/// presentation before accepting an intent.
pub fn derive_production_extraction_terminal_id_v1(
    account_id: [u8; 16],
    character_id: [u8; 16],
    encounter_id: [u8; 16],
    extraction_request_id: [u8; 16],
    extraction_receipt_id: [u8; 16],
) -> Result<[u8; 16], ProductionExtractionIntentError> {
    let mut hasher = blake3::Hasher::new_derive_key(TERMINAL_ID_CONTEXT);
    for field in [
        account_id,
        character_id,
        encounter_id,
        extraction_request_id,
        extraction_receipt_id,
    ] {
        hasher.update(&field);
    }
    let mut terminal_id = [0; 16];
    terminal_id.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
    if terminal_id == [0; 16]
        || terminal_id == extraction_request_id
        || terminal_id == extraction_receipt_id
    {
        return Err(ProductionExtractionIntentError::TerminalIdentityUnavailable);
    }
    Ok(terminal_id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProductionExtractionIntentError {
    #[error("live extraction requires a selected capacity-one BossExitReady route authority")]
    InvalidRouteAuthority,
    #[error("live extraction remains hidden until the Sir Caldus victory exit commits")]
    ExitNotCommitted,
    #[error("live extraction exit identity does not match the durable Caldus authority")]
    InvalidExitAuthority,
    #[error("live extraction content does not match the bound Core authority")]
    InvalidContentAuthority,
    #[error("live extraction terminal identity derivation produced a reserved value")]
    TerminalIdentityUnavailable,
    #[error("live extraction could not safely abort its exact route reservation")]
    RouteReservationAbortFailed,
}

#[cfg(test)]
mod tests {
    use std::{
        num::NonZeroU64,
        path::{Path, PathBuf},
        sync::{
            Arc, Mutex as StdMutex,
            atomic::{AtomicU64, AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use persistence::{
        PreparedProductionExtractionV1, ProductionExtractionTransactionV1,
        ProductionExtractionVersionAdvanceV1, ProductionExtractionVersionsV1,
        StoredCaldusVictoryExit, StoredCaldusVictoryOwner, StoredProductionExtractionResultV1,
        canonical_production_extraction_plan_hash_v1,
    };
    use protocol::{
        ActionResultCode, CorePrivateRouteContentRevisionV1, ExtractionCommitPayloadV1,
        ExtractionCommitResultV1, ManifestHash, ReliableEvent, TerminalExpectedVersionsV1,
    };
    use sim_core::{EntityId, Tick};

    use super::*;
    use crate::{
        AccountId, CaldusExitPresentationCommit, CoreExtractionActorDirectory,
        CoreExtractionActorLease, CoreExtractionAuthoritativeTick,
        CoreExtractionPublicationOutcome, CoreExtractionRuntimeError,
        CoreExtractionTransportDetach, CorePrivateLifeSessionDirectory,
        CorePrivateLifeTransportDetach, CorePrivateRouteActorPosition, CorePrivateRouteActorSeed,
        CorePrivateRouteExtractionBinding, CorePrivateRouteExtractionExitBinding,
        CoreRecallActorDirectory, CoreRecallAuthoritativeTick, CoreReliableWriter,
        CoreTerminalCoordinator, ProductionExtractionPublicationProof, ProductionRecallClock,
        ProductionRecallIntentActor, ProductionRecallPendingAuthorityV1,
        STORED_TERMINAL_RECEIPT_SCHEMA_V1, StoredTerminalReceipt, StoredTerminalReceiptV1,
        TerminalKind,
    };

    #[derive(Debug, Clone, Copy)]
    struct FixedClock(u64);

    impl IdentityClock for FixedClock {
        fn unix_millis(&self) -> u64 {
            self.0
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct RecallClock;

    impl ProductionRecallClock for RecallClock {
        fn unix_millis(&self) -> u64 {
            10_000
        }
    }

    #[derive(Debug)]
    struct RecallTicks(AtomicU64);

    impl CoreRecallAuthoritativeTick for RecallTicks {
        fn current_tick(&self, account_id: [u8; 16], character_id: [u8; 16]) -> NonZeroU64 {
            assert_eq!(account_id, [1; 16]);
            assert_eq!(character_id, [2; 16]);
            NonZeroU64::new(self.0.load(Ordering::SeqCst)).expect("non-zero Recall tick")
        }
    }

    fn recall_actor() -> Arc<ProductionRecallIntentActor<RecallClock>> {
        Arc::new(
            ProductionRecallIntentActor::new(
                RecallClock,
                [1; 16],
                [2; 16],
                ProductionRecallPendingAuthorityV1 {
                    pending_item_count: 0,
                    pending_material_stack_count: 0,
                },
            )
            .expect("Recall actor"),
        )
    }

    #[derive(Debug, Clone)]
    struct FakePlanner {
        calls: Arc<AtomicUsize>,
        accept_calls: Arc<AtomicUsize>,
        inputs: Arc<StdMutex<Vec<ProductionExtractionPlannerInputV1>>>,
        acceptance: Arc<StdMutex<Option<StoredProductionExtractionIntentAcceptanceV1>>>,
        fail_first: bool,
    }

    impl FakePlanner {
        fn stable() -> Self {
            Self {
                calls: Arc::new(AtomicUsize::new(0)),
                accept_calls: Arc::new(AtomicUsize::new(0)),
                inputs: Arc::new(StdMutex::new(Vec::new())),
                acceptance: Arc::new(StdMutex::new(None)),
                fail_first: false,
            }
        }

        fn fail_first() -> Self {
            Self {
                fail_first: true,
                ..Self::stable()
            }
        }
    }

    impl ProductionExtractionPlanner for FakePlanner {
        async fn load_intent_acceptance(
            &self,
            extraction_request_id: [u8; 16],
        ) -> Result<Option<StoredProductionExtractionIntentAcceptanceV1>, PersistenceError>
        {
            Ok(self
                .acceptance
                .lock()
                .expect("intent acceptance")
                .as_ref()
                .filter(|acceptance| {
                    acceptance.attempt.extraction_request_id == extraction_request_id
                })
                .cloned())
        }

        async fn accept_intent(
            &self,
            attempt: &ProductionExtractionIntentAttemptV1,
        ) -> Result<ProductionExtractionIntentAcceptanceTransactionV1, PersistenceError> {
            self.accept_calls.fetch_add(1, Ordering::SeqCst);
            let attempted_hash = attempt.canonical_hash()?;
            let mut stored = self.acceptance.lock().expect("intent acceptance");
            if let Some(accepted) = stored.as_ref() {
                return if accepted.canonical_attempt_hash == attempted_hash {
                    Ok(ProductionExtractionIntentAcceptanceTransactionV1::Replayed(
                        accepted.clone(),
                    ))
                } else {
                    Ok(
                        ProductionExtractionIntentAcceptanceTransactionV1::Conflict {
                            extraction_request_id: attempt.extraction_request_id,
                            conflict_audit_id: [0x44; 16],
                            stored_attempt_hash: accepted.canonical_attempt_hash,
                            attempted_attempt_hash: attempted_hash,
                        },
                    )
                };
            }
            let accepted = StoredProductionExtractionIntentAcceptanceV1 {
                attempt: attempt.clone(),
                canonical_attempt_hash: attempted_hash,
                commit_request_hash: attempt.commit_request.canonical_hash()?,
                accepted_at_unix_ms: 10_000,
            };
            *stored = Some(accepted.clone());
            Ok(ProductionExtractionIntentAcceptanceTransactionV1::Fresh(
                accepted,
            ))
        }

        async fn prepare(
            &self,
            input: &ProductionExtractionPlannerInputV1,
        ) -> Result<PreparedProductionExtractionV1, PersistenceError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            self.inputs
                .lock()
                .expect("planner inputs")
                .push(input.clone());
            if self.fail_first && call == 0 {
                return Err(PersistenceError::ProductionExtractionTerminalSuperseded);
            }
            let request = input.commit_request().clone();
            let request_hash = request.canonical_hash()?;
            let plan_hash = canonical_production_extraction_plan_hash_v1(&[], &[])?;
            PreparedProductionExtractionV1::seal(request, request_hash, plan_hash, false)
        }
    }

    #[derive(Debug)]
    struct ExtractionTicks {
        tick: AtomicU64,
        calls: AtomicUsize,
    }

    impl ExtractionTicks {
        fn new(tick: u64) -> Self {
            Self {
                tick: AtomicU64::new(tick),
                calls: AtomicUsize::new(0),
            }
        }
    }

    impl CoreExtractionAuthoritativeTick for ExtractionTicks {
        fn current_tick(&self, lease: CoreExtractionActorLease) -> Option<NonZeroU64> {
            assert_eq!(lease.account_id(), [1; 16]);
            assert_eq!(lease.character_id(), [2; 16]);
            assert_eq!(lease.route_generation(), 7);
            self.calls.fetch_add(1, Ordering::SeqCst);
            NonZeroU64::new(self.tick.load(Ordering::SeqCst))
        }
    }

    fn content_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn authenticated() -> AuthenticatedAccount {
        AuthenticatedAccount {
            account_id: AccountId::new([1; 16]).expect("account"),
            namespace: AuthenticatedNamespace::WipeableTest,
        }
    }

    fn participant() -> CoreBossParticipant {
        CoreBossParticipant {
            entity_id: EntityId::new(41).expect("entity"),
            party_slot: 0,
        }
    }

    fn lock() -> CoreBossParticipantLock {
        CoreBossParticipantLock {
            attempt_ordinal: 1,
            participants: vec![participant()],
            maximum_health: 7_200,
        }
    }

    fn revision() -> WorldFlowContentRevisionV1 {
        WorldFlowContentRevisionV1 {
            records_blake3: ManifestHash::new("a".repeat(64)).expect("records"),
            assets_blake3: ManifestHash::new("b".repeat(64)).expect("assets"),
            localization_blake3: ManifestHash::new("c".repeat(64)).expect("localization"),
        }
    }

    fn route_revision() -> CorePrivateRouteContentRevisionV1 {
        CorePrivateRouteContentRevisionV1 {
            records_blake3: ManifestHash::new("d".repeat(64)).expect("route records"),
            assets_blake3: ManifestHash::new("e".repeat(64)).expect("route assets"),
            localization_blake3: ManifestHash::new("f".repeat(64)).expect("route localization"),
        }
    }

    fn versions() -> ProductionExtractionExpectedVersionsV1 {
        ProductionExtractionExpectedVersionsV1 {
            account: 10,
            character: 20,
            world: 20,
            inventory: 30,
            life_metrics: 40,
        }
    }

    fn route_seed() -> CorePrivateRouteActorSeed {
        CorePrivateRouteActorSeed {
            character_id: [2; 16],
            character_version: 20,
            content_revision: route_revision(),
            world_flow_revision: revision(),
            position: CorePrivateRouteActorPosition {
                instance_lineage_id: Some([3; 16]),
                scene: CorePrivateRouteSceneV1::BellSepulcher,
                room: Some(CorePrivateRouteRoomV1::CaldusArenaB6),
                phase: CorePrivateRoutePhaseV1::BossExitReady,
            },
        }
    }

    fn committed_exit() -> StoredCaldusVictoryExit {
        let lock = lock();
        let identities = CoreCaldusVictoryIdentities::derive([3; 16], &lock).expect("identities");
        StoredCaldusVictoryExit {
            replayed: false,
            encounter_id: identities.encounter_id.bytes(),
            instance_lineage_id: [3; 16],
            attempt_ordinal: 1,
            exit_instance_id: identities.exit_instance_id.bytes(),
            canonical_request_hash: [9; 32],
            owners: vec![StoredCaldusVictoryOwner {
                party_slot: 0,
                participant_entity_id: participant().entity_id.get(),
                account_id: [1; 16],
                character_id: [2; 16],
                reward_request_id: identities
                    .reward_for(participant())
                    .expect("reward identity")
                    .bytes(),
                reward_result_hash: [4; 32],
                progression_payload_hash: [5; 32],
            }],
        }
    }

    fn presentation(committed: bool) -> CaldusInstancePresentation {
        let mut presentation = CaldusInstancePresentation::new([3; 16], 1).expect("presentation");
        if committed {
            let content =
                sim_content::load_core_development_caldus(&content_root()).expect("Caldus content");
            assert_eq!(
                presentation.present_committed_exit(&content, &committed_exit()),
                Ok(CaldusExitPresentationCommit::Fresh)
            );
        }
        presentation
    }

    fn defeat_handoff(route_lease: CorePrivateRouteActorLease) -> CorePrivateCaldusDefeatHandoff {
        CorePrivateCaldusDefeatHandoff {
            route_lease,
            route_state_version: 1,
            instance_lineage_id: [3; 16],
            entry_restore_point_id: [4; 16],
            lock: lock(),
            active_duration_ticks: 300,
            defeat_tick: Tick(900),
            character_id: [2; 16],
            expected_progression_version: 19,
            eligibility: Vec::new(),
        }
    }

    fn reward_commit(
        directory: &CorePrivateRouteActorDirectory,
        route_lease: CorePrivateRouteActorLease,
    ) -> CorePrivateCaldusRewardCommit {
        CorePrivateCaldusRewardCommit {
            route: directory
                .snapshot(route_lease)
                .expect("BossExitReady route"),
            exit: presentation(true).exit().expect("committed exit").clone(),
            disposition: crate::CorePrivateCaldusRewardCommitDisposition::Committed,
        }
    }

    struct ExitIdentityFixture {
        encounter: [u8; 16],
        exit: [u8; 16],
        request: [u8; 16],
        receipt: [u8; 16],
        terminal: [u8; 16],
    }

    fn exit_identities() -> ExitIdentityFixture {
        let identities = CoreCaldusVictoryIdentities::derive([3; 16], &lock()).expect("identities");
        let extraction = identities
            .extraction_for(participant())
            .expect("extraction identity");
        let terminal_id = derive_production_extraction_terminal_id_v1(
            [1; 16],
            [2; 16],
            identities.encounter_id.bytes(),
            extraction.request_id.bytes(),
            extraction.receipt_id.bytes(),
        )
        .expect("terminal identity");
        ExitIdentityFixture {
            encounter: identities.encounter_id.bytes(),
            exit: identities.exit_instance_id.bytes(),
            request: extraction.request_id.bytes(),
            receipt: extraction.receipt_id.bytes(),
            terminal: terminal_id,
        }
    }

    async fn reserved_route() -> (
        CorePrivateRouteActorDirectory,
        CorePrivateRouteActorLease,
        CorePrivateRouteExtractionPermit,
    ) {
        let directory = CorePrivateRouteActorDirectory::new();
        let lease = directory
            .register_actor(authenticated(), route_seed(), 7)
            .expect("route actor");
        let accepted_route = directory.snapshot(lease).expect("BossExitReady route");
        let identities = exit_identities();
        let exit = CorePrivateRouteExtractionExitBinding::new(
            identities.encounter,
            identities.exit,
            identities.request,
            identities.receipt,
            identities.terminal,
        )
        .expect("exit binding");
        let binding = CorePrivateRouteExtractionBinding::new(
            [1; 16],
            accepted_route,
            revision(),
            [4; 16],
            exit,
        )
        .expect("route binding");
        let permit = directory
            .prepare_extraction_terminal(lease, binding)
            .await
            .expect("terminal permit");
        (directory, lease, permit)
    }

    async fn authority() -> (
        ProductionExtractionBossExitAuthorityV1,
        CorePrivateRouteActorDirectory,
        CorePrivateRouteActorLease,
    ) {
        let (directory, lease, permit) = reserved_route().await;
        let authority = ProductionExtractionBossExitAuthorityV1::seal(
            authenticated(),
            [2; 16],
            permit,
            &presentation(true),
            &lock(),
            participant(),
            versions(),
        )
        .expect("exit authority");
        (authority, directory, lease)
    }

    #[tokio::test]
    async fn committed_caldus_authority_reserves_exact_route_and_aborts_failed_seal() {
        let directory = CorePrivateRouteActorDirectory::new();
        let lease = directory
            .register_actor(authenticated(), route_seed(), 21)
            .expect("route actor");
        let handoff = defeat_handoff(lease);
        let commit = reward_commit(&directory, lease);
        let accepted_version = commit.route.state_version;

        let reservation = ProductionExtractionCaldusReservationV1::reserve(
            authenticated(),
            directory.clone(),
            &handoff,
            &commit,
            revision(),
        )
        .await
        .expect("exact Caldus reservation");
        assert_eq!(reservation.accepted_route(), &commit.route);
        assert_eq!(
            reservation.permit().accepted_route_state_version(),
            accepted_version
        );
        let authority = reservation
            .seal(versions())
            .await
            .expect("exact Caldus authority");
        assert_eq!(authority.selected_character_id(), [2; 16]);
        assert_eq!(authority.instance_lineage_id(), [3; 16]);
        assert_eq!(authority.route_state_version(), accepted_version);
        let terminal_pending = directory.snapshot(lease).expect("terminal projection");
        assert_eq!(
            terminal_pending.phase,
            CorePrivateRoutePhaseV1::TerminalPending
        );
        assert_eq!(terminal_pending.state_version, accepted_version + 1);
        directory
            .abort_extraction_terminal(lease, authority.route_permit())
            .await
            .expect("test cleanup");

        let mut changed = reward_commit(&directory, lease);
        let reopened_version = changed.route.state_version;
        changed.exit.content_id = "portal.exit.changed".to_owned();
        assert_eq!(
            ProductionExtractionBossExitAuthorityV1::prepare_from_caldus_commit(
                authenticated(),
                &directory,
                &handoff,
                &changed,
                revision(),
                versions(),
            )
            .await,
            Err(ProductionExtractionIntentError::InvalidExitAuthority)
        );
        let reopened = directory
            .snapshot(lease)
            .expect("failed seal reopens route");
        assert_eq!(reopened.phase, CorePrivateRoutePhaseV1::BossExitReady);
        assert_eq!(reopened.state_version, reopened_version + 2);

        let fresh_commit = reward_commit(&directory, lease);
        let retried = ProductionExtractionBossExitAuthorityV1::prepare_from_caldus_commit(
            authenticated(),
            &directory,
            &handoff,
            &fresh_commit,
            revision(),
            versions(),
        )
        .await
        .expect("failed construction leaves no reservation residue");
        directory
            .abort_extraction_terminal(lease, retried.route_permit())
            .await
            .expect("retry cleanup");
        directory.begin_shutdown();
        assert!(directory.finish_shutdown().await.unwrap().zero_residue);
    }

    #[tokio::test]
    async fn uncommitted_actor_abort_reopens_same_route_generation_for_retry() {
        let route_directory = CorePrivateRouteActorDirectory::new();
        let route_lease = route_directory
            .register_actor(authenticated(), route_seed(), 7)
            .expect("route actor");
        let extraction = Arc::new(CoreExtractionActorDirectory::new(Arc::new(
            ExtractionTicks::new(700),
        )));

        let register = |authority| {
            Arc::new(
                ProductionExtractionIntentActor::new(
                    authority,
                    route_directory.clone(),
                    route_lease,
                    FakePlanner::stable(),
                    FixedClock(10_000),
                )
                .expect("extraction actor"),
            )
        };
        let first_commit = reward_commit(&route_directory, route_lease);
        let first_authority = ProductionExtractionBossExitAuthorityV1::prepare_from_caldus_commit(
            authenticated(),
            &route_directory,
            &defeat_handoff(route_lease),
            &first_commit,
            revision(),
            versions(),
        )
        .await
        .expect("first authority");
        let first_lease = extraction
            .register_actor(authenticated(), register(first_authority))
            .await
            .expect("first registration");
        let first_report = extraction
            .abort_uncommitted_actor(first_lease)
            .await
            .expect("known construction abort");
        assert!(first_report.zero_actor_residue);
        assert!(!first_report.committed_result_retained);
        assert_eq!(
            route_directory
                .snapshot(route_lease)
                .expect("reopened route")
                .phase,
            CorePrivateRoutePhaseV1::BossExitReady
        );

        let retry_commit = reward_commit(&route_directory, route_lease);
        let retry_authority = ProductionExtractionBossExitAuthorityV1::prepare_from_caldus_commit(
            authenticated(),
            &route_directory,
            &defeat_handoff(route_lease),
            &retry_commit,
            revision(),
            versions(),
        )
        .await
        .expect("retry authority");
        let retry_lease = extraction
            .register_actor(authenticated(), register(retry_authority))
            .await
            .expect("same route generation may retry");
        extraction
            .abort_uncommitted_actor(retry_lease)
            .await
            .expect("retry cleanup");

        for connection in extraction.begin_shutdown().await {
            connection.close(0_u32.into(), b"test shutdown");
        }
        assert!(extraction.finish_shutdown().await.unwrap().zero_residue);
        route_directory.begin_shutdown();
        assert!(
            route_directory
                .finish_shutdown()
                .await
                .unwrap()
                .zero_residue
        );
    }

    fn frame(sequence: u32) -> ExtractionCommitFrameV1 {
        let extraction_request_id = exit_identities().request;
        let payload = ExtractionCommitPayloadV1 {
            extraction_request_id,
            expected_versions: TerminalExpectedVersionsV1 {
                account: 10,
                character: 20,
                world: 20,
                inventory: 30,
                life_clock: 40,
            },
            content_revision: revision(),
        };
        ExtractionCommitFrameV1 {
            schema_version: TERMINAL_INVENTORY_SCHEMA_VERSION,
            sequence,
            mutation_id: [6; 16],
            character_id: [2; 16],
            issued_at_unix_millis: 1_000,
            payload_hash: payload.canonical_hash(),
            payload,
        }
    }

    fn stored_result(
        prepared: &PreparedProductionExtractionV1,
    ) -> StoredProductionExtractionResultV1 {
        let request = prepared.request();
        let result = StoredProductionExtractionResultV1 {
            contract_version: request.contract_version,
            namespace_id: request.namespace_id.clone(),
            account_id: request.account_id,
            character_id: request.character_id,
            mutation_id: request.mutation_id,
            terminal_id: request.terminal_id,
            extraction_request_id: request.extraction_request_id,
            extraction_receipt_id: request.extraction_receipt_id,
            canonical_request_hash: prepared.canonical_request_hash(),
            canonical_plan_hash: prepared.canonical_plan_hash(),
            result_code: 1,
            issued_at_unix_ms: request.issued_at_unix_ms,
            observed_tick: request.observed_tick,
            committed_at_unix_ms: request.issued_at_unix_ms + 1,
            destination_content_id: persistence::PRODUCTION_EXTRACTION_HALL_ID.into(),
            versions: ProductionExtractionVersionsV1 {
                account: ProductionExtractionVersionAdvanceV1 { pre: 10, post: 10 },
                character: ProductionExtractionVersionAdvanceV1 { pre: 20, post: 21 },
                world: ProductionExtractionVersionAdvanceV1 { pre: 20, post: 21 },
                inventory: ProductionExtractionVersionAdvanceV1 { pre: 30, post: 31 },
                life_metrics: ProductionExtractionVersionAdvanceV1 { pre: 40, post: 41 },
            },
            placements: Vec::new(),
            material_credits: Vec::new(),
            storage_resolution_required: false,
        };
        result.validate().expect("valid stored extraction result");
        result
    }

    async fn live_connection_pair() -> (
        quinn::Endpoint,
        quinn::Endpoint,
        quinn::Connection,
        quinn::Connection,
    ) {
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
        let certificate = cert.der().clone();
        let private_key = rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der());
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

    #[tokio::test]
    #[allow(
        clippy::too_many_lines,
        reason = "one real-QUIC test keeps the complete prepare, recover, supersede, abort, commit, retry, shared detach, and shutdown contract contiguous"
    )]
    async fn extraction_writer_handoff_is_recoverable_exact_and_non_retiring() {
        let planner = FakePlanner::stable();
        let (exit_authority, route_directory, route_lease) = authority().await;
        let actor = Arc::new(
            ProductionExtractionIntentActor::new(
                exit_authority,
                route_directory,
                route_lease,
                planner,
                FixedClock(10_000),
            )
            .expect("intent actor"),
        );
        let runtime = Arc::new(CoreExtractionActorDirectory::new(Arc::new(
            ExtractionTicks::new(700),
        )));
        runtime
            .register_actor(authenticated(), actor)
            .await
            .expect("register actor");

        let (first_server_endpoint, first_client_endpoint, first_client, first_server) =
            live_connection_pair().await;
        let first_writer = Arc::new(CoreReliableWriter::new(first_server));
        let first = runtime
            .attach_reliable_writer(authenticated(), Arc::clone(&first_writer))
            .await
            .expect("first attach");
        let old_authority = Arc::clone(&runtime).authority(first.lease);

        let (second_server_endpoint, second_client_endpoint, second_client, second_server) =
            live_connection_pair().await;
        let second_writer = Arc::new(CoreReliableWriter::new(second_server));
        let abandoned = runtime
            .prepare_reliable_writer_handoff(authenticated(), Arc::clone(&second_writer))
            .await
            .expect("prepare second writer");
        assert_eq!(
            runtime
                .prepare_reliable_writer_handoff(authenticated(), Arc::clone(&second_writer))
                .await
                .expect("recover cancelled prepare"),
            abandoned
        );

        let (third_server_endpoint, third_client_endpoint, third_client, third_server) =
            live_connection_pair().await;
        let third_writer = Arc::new(CoreReliableWriter::new(third_server));
        let superseding = runtime
            .prepare_reliable_writer_handoff(authenticated(), Arc::clone(&third_writer))
            .await
            .expect("supersede abandoned prepare");
        assert!(
            superseding.lease().transport_generation().get()
                > abandoned.lease().transport_generation().get()
        );
        assert!(matches!(
            runtime
                .commit_prepared_reliable_writer_handoff(abandoned)
                .await,
            Err(CoreExtractionRuntimeError::PreparedWriterHandoffMismatch)
        ));
        assert!(matches!(
            old_authority
                .handle_extraction(authenticated(), &frame(1), 1)
                .await
                .result,
            ExtractionCommitResultV1::Pending { .. }
        ));

        runtime
            .abort_prepared_reliable_writer_handoff(superseding)
            .await
            .expect("abort exact prepare");
        let exact = runtime
            .prepare_reliable_writer_handoff(authenticated(), Arc::clone(&third_writer))
            .await
            .expect("prepare exact handoff");
        assert!(
            exact.lease().transport_generation().get()
                > superseding.lease().transport_generation().get()
        );
        let committed = runtime
            .commit_prepared_reliable_writer_handoff(exact)
            .await
            .expect("commit exact handoff");
        assert!(committed.invalidated_connection.is_some());
        assert!(first_writer.is_available());
        assert!(third_writer.is_available());
        let retried = runtime
            .commit_prepared_reliable_writer_handoff(exact)
            .await
            .expect("retry committed handoff");
        assert_eq!(retried.lease, committed.lease);
        assert!(retried.invalidated_connection.is_none());
        assert_eq!(
            runtime.detach_shared_reliable_writer(committed.lease).await,
            CoreExtractionTransportDetach::Detached
        );
        assert!(third_writer.is_available());
        runtime
            .prepare_reliable_writer_handoff(authenticated(), Arc::clone(&second_writer))
            .await
            .expect("prepare shutdown reservation");

        first_writer.retire(0, b"central session cleanup");
        third_writer.retire(0, b"central session cleanup");
        for connection in runtime.begin_shutdown().await {
            connection.close(0_u32.into(), b"test shutdown");
        }
        let report = runtime.finish_shutdown().await.unwrap();
        assert_eq!(report.retired_pending_writer_handoffs, 1);
        assert!(report.zero_residue);
        assert!(second_writer.is_available());
        second_writer.retire(0, b"central session cleanup");
        first_client.close(0_u32.into(), b"test complete");
        second_client.close(0_u32.into(), b"test complete");
        third_client.close(0_u32.into(), b"test complete");
        first_server_endpoint.wait_idle().await;
        first_client_endpoint.wait_idle().await;
        second_server_endpoint.wait_idle().await;
        second_client_endpoint.wait_idle().await;
        third_server_endpoint.wait_idle().await;
        third_client_endpoint.wait_idle().await;
    }

    #[tokio::test]
    #[allow(
        clippy::too_many_lines,
        reason = "one bounded real-QUIC composition journey proves shared sequence authority, coordinated dual-runtime reconnect, detach, and shutdown"
    )]
    async fn private_life_session_coordinates_recall_and_extraction_writer_handoffs() {
        let recall = Arc::new(CoreRecallActorDirectory::new(Arc::new(RecallTicks(
            AtomicU64::new(700),
        ))));
        recall
            .register_actor(authenticated(), recall_actor())
            .await
            .expect("register Recall actor");

        let (exit_authority, route_directory, route_lease) = authority().await;
        let extraction_actor = Arc::new(
            ProductionExtractionIntentActor::new(
                exit_authority,
                route_directory.clone(),
                route_lease,
                FakePlanner::stable(),
                FixedClock(10_000),
            )
            .expect("extraction actor"),
        );
        let extraction = Arc::new(CoreExtractionActorDirectory::new(Arc::new(
            ExtractionTicks::new(700),
        )));
        extraction
            .register_actor(authenticated(), extraction_actor)
            .await
            .expect("register extraction actor");
        let registered = extraction
            .registered_actor_lease(authenticated())
            .await
            .expect("registered extraction binding");
        assert_eq!(registered.account_id(), route_lease.account_id());
        assert_eq!(registered.character_id(), route_lease.character_id());
        assert_eq!(
            registered.route_generation(),
            route_lease.actor_generation()
        );
        let sessions = CorePrivateLifeSessionDirectory::with_extraction_runtime(
            Arc::clone(&recall),
            Arc::clone(&extraction),
        );

        let (first_server_endpoint, first_client_endpoint, first_client, first_server) =
            live_connection_pair().await;
        let first = sessions
            .attach_transport(authenticated(), first_server, 10_000)
            .await
            .expect("attach first private-life transport");
        let first_recall_lease = sessions
            .bind_recall(first.lease)
            .await
            .expect("bind Recall");
        let first_extraction_lease = sessions
            .bind_extraction(first.lease)
            .await
            .expect("bind extraction");
        let first_session_writer = sessions.writer(first.lease).await.expect("session writer");
        let first_recall_writer = recall
            .reliable_writer(first_recall_lease)
            .await
            .expect("Recall writer");
        let first_extraction_writer = extraction
            .reliable_writer(first_extraction_lease)
            .await
            .expect("extraction writer");
        assert!(Arc::ptr_eq(&first.writer, &first_session_writer));
        assert!(Arc::ptr_eq(&first.writer, &first_recall_writer));
        assert!(Arc::ptr_eq(&first.writer, &first_extraction_writer));

        for (writer, action_sequence, expected_sequence) in [
            (&first_session_writer, 11, 1),
            (&first_recall_writer, 12, 2),
            (&first_extraction_writer, 13, 3),
        ] {
            let sent = writer
                .send_event(
                    700,
                    ReliableEvent::ActionResult {
                        action_sequence,
                        code: ActionResultCode::Accepted,
                    },
                )
                .await
                .expect("shared reliable send");
            assert_eq!(sent.sequence, expected_sequence);
            assert_eq!(
                bot_client::receive_server_reliable(&first_client)
                    .await
                    .expect("shared reliable receive"),
                sent
            );
        }

        let (second_server_endpoint, second_client_endpoint, second_client, second_server) =
            live_connection_pair().await;
        let second = sessions
            .attach_transport(authenticated(), second_server, 10_500)
            .await
            .expect("coordinated reconnect");
        let second_recall_lease = second.recall_lease.expect("rebound Recall lease");
        let second_extraction_lease = second.extraction_lease.expect("rebound extraction lease");
        let second_session_writer = sessions
            .writer(second.lease)
            .await
            .expect("new session writer");
        let second_recall_writer = recall
            .reliable_writer(second_recall_lease)
            .await
            .expect("new Recall writer");
        let second_extraction_writer = extraction
            .reliable_writer(second_extraction_lease)
            .await
            .expect("new extraction writer");
        assert!(second.invalidated_connection.is_some());
        assert!(!first.writer.is_available());
        assert!(Arc::ptr_eq(&second.writer, &second_session_writer));
        assert!(Arc::ptr_eq(&second.writer, &second_recall_writer));
        assert!(Arc::ptr_eq(&second.writer, &second_extraction_writer));
        assert!(recall.reliable_writer(first_recall_lease).await.is_err());
        assert!(
            extraction
                .reliable_writer(first_extraction_lease)
                .await
                .is_err()
        );
        assert_eq!(
            sessions
                .detach_transport(first.lease, 10_500)
                .await
                .expect("stale detach"),
            CorePrivateLifeTransportDetach::StaleGenerationIgnored
        );

        for (writer, action_sequence, expected_sequence) in [
            (&second_session_writer, 21, 1),
            (&second_recall_writer, 22, 2),
            (&second_extraction_writer, 23, 3),
        ] {
            let sent = writer
                .send_event(
                    701,
                    ReliableEvent::ActionResult {
                        action_sequence,
                        code: ActionResultCode::Accepted,
                    },
                )
                .await
                .expect("rebound reliable send");
            assert_eq!(sent.sequence, expected_sequence);
            assert_eq!(
                bot_client::receive_server_reliable(&second_client)
                    .await
                    .expect("rebound reliable receive"),
                sent
            );
        }

        assert!(matches!(
            sessions
                .detach_transport(second.lease, 11_000)
                .await
                .expect("current detach"),
            CorePrivateLifeTransportDetach::Detached {
                recall: Some(_),
                extraction: Some(CoreExtractionTransportDetach::Detached),
            }
        ));
        assert!(!second.writer.is_available());
        assert!(recall.reliable_writer(second_recall_lease).await.is_err());
        assert!(
            extraction
                .reliable_writer(second_extraction_lease)
                .await
                .is_err()
        );

        for connection in sessions.begin_shutdown().await {
            connection.close(0_u32.into(), b"test shutdown");
        }
        let report = sessions.finish_shutdown().await.expect("session shutdown");
        assert!(report.recall.zero_residue);
        assert!(report.extraction.is_some_and(|report| report.zero_residue));
        assert!(report.zero_residue);
        route_directory.begin_shutdown();
        assert!(
            route_directory
                .finish_shutdown()
                .await
                .expect("route shutdown")
                .zero_residue
        );

        first_client.close(0_u32.into(), b"test complete");
        second_client.close(0_u32.into(), b"test complete");
        first_server_endpoint.wait_idle().await;
        first_client_endpoint.wait_idle().await;
        second_server_endpoint.wait_idle().await;
        second_client_endpoint.wait_idle().await;
    }

    #[tokio::test]
    #[allow(
        clippy::too_many_lines,
        reason = "one contiguous real-QUIC journey proves sequence continuity, durable publication, route retirement, handoff replay, and Hall acknowledgement"
    )]
    async fn dynamic_runtime_reuses_session_writer_replays_commit_and_retires_route() {
        let planner = FakePlanner::stable();
        let (exit_authority, route_directory, route_lease) = authority().await;
        let actor = Arc::new(
            ProductionExtractionIntentActor::new(
                exit_authority,
                route_directory.clone(),
                route_lease,
                planner,
                FixedClock(10_000),
            )
            .expect("intent actor"),
        );
        let ticks = Arc::new(ExtractionTicks::new(700));
        let runtime = Arc::new(CoreExtractionActorDirectory::new(Arc::clone(&ticks)));
        let actor_lease = runtime
            .register_actor(authenticated(), Arc::clone(&actor))
            .await
            .expect("register extraction actor");
        tokio::task::yield_now().await;
        assert_eq!(ticks.calls.load(Ordering::SeqCst), 0);
        let (first_server_endpoint, first_client_endpoint, first_client, first_server) =
            live_connection_pair().await;
        let session_writer = Arc::new(CoreReliableWriter::new(first_server));
        let preceding = session_writer
            .send_event(
                699,
                ReliableEvent::ActionResult {
                    action_sequence: 9,
                    code: ActionResultCode::Accepted,
                },
            )
            .await
            .expect("preceding session event");
        assert_eq!(preceding.sequence, 1);
        assert_eq!(
            bot_client::receive_server_reliable(&first_client)
                .await
                .unwrap(),
            preceding
        );
        let first = runtime
            .attach_reliable_writer(authenticated(), Arc::clone(&session_writer))
            .await
            .expect("attach session writer");
        assert!(!first.committed_result_replayed);
        let authority = Arc::clone(&runtime).authority(first.lease);
        let request = frame(1);
        let foreign = AuthenticatedAccount {
            account_id: AccountId::new([8; 16]).expect("foreign account"),
            namespace: AuthenticatedNamespace::WipeableTest,
        };
        assert!(matches!(
            authority
                .handle_extraction(foreign, &request, 1)
                .await
                .result,
            ExtractionCommitResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::ForeignAuthority,
                ..
            }
        ));
        let pending = authority
            .handle_extraction(authenticated(), &request, 1)
            .await;
        assert_eq!(pending.server_tick, 700);
        assert!(matches!(
            pending.result,
            ExtractionCommitResultV1::Pending { .. }
        ));
        assert_eq!(ticks.calls.load(Ordering::SeqCst), 1);

        let intent = actor.prepared_intent().await.expect("prepared intent");
        let prepared = intent.prepared().expect("prepared transaction");
        let stored = stored_result(prepared);
        let terminal_id = stored.terminal_id;
        let result_hash = stored.digest().expect("stored result hash");
        let fresh_transaction = ProductionExtractionTransactionV1::Fresh(stored.clone());
        let fresh_proof = ProductionExtractionPublicationProof::from_validated_test_transaction(
            prepared,
            &fresh_transaction,
        )
        .expect("validated fresh publication proof");
        let replay_transaction = ProductionExtractionTransactionV1::Replayed(stored);
        let replay_proof = ProductionExtractionPublicationProof::from_validated_test_transaction(
            prepared,
            &replay_transaction,
        )
        .expect("validated replay publication proof");
        let (delivery_started, release_delivery) = runtime.install_delivery_test_gate().await;
        let fresh_runtime = Arc::clone(&runtime);
        let fresh_intent = intent.clone();
        let fresh_publication = tokio::spawn(async move {
            fresh_runtime
                .publish_coordinated(actor_lease, &fresh_intent, &fresh_proof)
                .await
        });
        delivery_started.notified().await;
        fresh_publication.abort();
        assert!(
            fresh_publication
                .await
                .expect_err("outer publication cancelled")
                .is_cancelled()
        );
        assert_eq!(
            runtime
                .publish_coordinated(actor_lease, &intent, &replay_proof)
                .await
                .expect("concurrent exact repository replay"),
            CoreExtractionPublicationOutcome::ReplayedQueued
        );
        release_delivery.notify_one();
        let pushed = bot_client::receive_server_reliable(&first_client)
            .await
            .unwrap();
        assert_eq!(pushed.sequence, 2);
        assert_eq!(pushed.server_tick, 700);
        assert!(matches!(
            pushed.event,
            ReliableEvent::ExtractionCommitResult(result)
                if matches!(*result, ExtractionCommitResultV1::Stored { request_sequence: 1, replayed: false, .. })
        ));
        assert!(
            tokio::time::timeout(
                Duration::from_millis(50),
                bot_client::receive_server_reliable(&first_client),
            )
            .await
            .is_err(),
            "concurrent publication must emit one completion event"
        );

        let retry = authority
            .handle_extraction(authenticated(), &frame(2), 999)
            .await;
        assert_eq!(retry.server_tick, 700);
        assert!(matches!(
            retry.result,
            ExtractionCommitResultV1::Stored {
                request_sequence: 2,
                replayed: true,
                ..
            }
        ));
        let retired = runtime
            .retire_actor_after_commit(actor_lease)
            .await
            .expect("retire committed actor");
        assert!(retired.committed_result_retained);
        assert!(retired.zero_actor_residue);
        assert!(matches!(
            authority
                .handle_extraction(authenticated(), &frame(3), 999)
                .await
                .result,
            ExtractionCommitResultV1::Stored {
                request_sequence: 3,
                ..
            }
        ));
        let mut changed = frame(4);
        changed.issued_at_unix_millis += 1;
        assert!(matches!(
            authority
                .handle_extraction(authenticated(), &changed, 999)
                .await
                .result,
            ExtractionCommitResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::IdempotencyConflict,
                ..
            }
        ));
        let stale_hall = runtime
            .prepare_hall_projection(first.lease, terminal_id, result_hash)
            .await
            .expect("first generation Hall projection");

        let (second_server_endpoint, second_client_endpoint, second_client, second_server) =
            live_connection_pair().await;
        let second_writer = Arc::new(CoreReliableWriter::new(second_server));
        let second = runtime
            .attach_reliable_writer(authenticated(), Arc::clone(&second_writer))
            .await
            .expect("handoff committed extraction");
        assert!(second.invalidated_connection.is_some());
        assert!(second.committed_result_replayed);
        assert!(!session_writer.is_available());
        let replayed = bot_client::receive_server_reliable(&second_client)
            .await
            .unwrap();
        assert_eq!(replayed.sequence, 1);
        assert!(matches!(
            replayed.event,
            ReliableEvent::ExtractionCommitResult(result)
                if matches!(*result, ExtractionCommitResultV1::Stored { replayed: true, .. })
        ));
        assert!(matches!(
            runtime.acknowledge_hall_installed(stale_hall).await,
            Err(CoreExtractionRuntimeError::TransportUnavailable)
        ));
        assert!(matches!(
            authority
                .handle_extraction(authenticated(), &frame(4), 999)
                .await
                .result,
            ExtractionCommitResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::SourceUnavailable,
                ..
            }
        ));

        let hall = runtime
            .prepare_hall_projection(second.lease, terminal_id, result_hash)
            .await
            .expect("prepare delivered Hall projection");
        assert_eq!(hall.snapshot().character_id, [2; 16]);
        runtime
            .acknowledge_hall_installed(hall)
            .await
            .expect("release extraction binding without retiring session writer");
        assert!(second_writer.is_available());
        for connection in runtime.begin_shutdown().await {
            connection.close(0_u32.into(), b"test shutdown");
        }
        let report = runtime.finish_shutdown().await.unwrap();
        assert_eq!(report.served_commands, 2);
        assert!(report.zero_residue);
        first_client.close(0_u32.into(), b"test complete");
        second_client.close(0_u32.into(), b"test complete");
        first_server_endpoint.wait_idle().await;
        first_client_endpoint.wait_idle().await;
        second_server_endpoint.wait_idle().await;
        second_client_endpoint.wait_idle().await;
    }

    fn committed_terminal_coordinator(kind: TerminalKind) -> CoreTerminalCoordinator {
        committed_terminal_coordinator_with_route(kind, [3; 16], [4; 16])
    }

    fn committed_terminal_coordinator_with_route(
        kind: TerminalKind,
        lineage_id: [u8; 16],
        restore_point_id: [u8; 16],
    ) -> CoreTerminalCoordinator {
        let receipt = StoredTerminalReceipt::from_storage(&StoredTerminalReceiptV1 {
            schema_version: STORED_TERMINAL_RECEIPT_SCHEMA_V1,
            account_id: [1; 16],
            character_id: [2; 16],
            lineage_id,
            restore_point_id,
            terminal_id: [70 + kind.stable_code(); 16],
            mutation_id: [80 + kind.stable_code(); 16],
            payload_hash: [90 + kind.stable_code(); 32],
            server_plan_hash: [100 + kind.stable_code(); 32],
            result_hash: [110 + kind.stable_code(); 32],
            expected_state_version: 20,
            post_state_version: 21,
            observed_tick: 700,
            committed_tick: 700,
            terminal_kind_code: kind.stable_code(),
        })
        .expect("stored terminal receipt");
        CoreTerminalCoordinator::from_stored_receipt(authenticated(), receipt)
            .expect("committed terminal coordinator")
    }

    #[tokio::test]
    async fn every_other_terminal_winner_retires_extraction_without_retiring_session_authority() {
        for kind in [
            TerminalKind::LethalDeath,
            TerminalKind::EmergencyRecall,
            TerminalKind::DisconnectRecovery,
            TerminalKind::VerifiedServerFaultRestoration,
        ] {
            let planner = FakePlanner::stable();
            let (exit_authority, route_directory, route_lease) = authority().await;
            let actor = Arc::new(
                ProductionExtractionIntentActor::new(
                    exit_authority,
                    route_directory.clone(),
                    route_lease,
                    planner,
                    FixedClock(10_000),
                )
                .expect("intent actor"),
            );
            let runtime = Arc::new(CoreExtractionActorDirectory::new(Arc::new(
                ExtractionTicks::new(700),
            )));
            let actor_lease = runtime
                .register_actor(authenticated(), actor)
                .await
                .expect("register extraction actor");
            for stale in [
                committed_terminal_coordinator_with_route(kind, [9; 16], [4; 16]),
                committed_terminal_coordinator_with_route(kind, [3; 16], [9; 16]),
            ] {
                assert!(matches!(
                    runtime
                        .retire_actor_after_other_terminal(actor_lease, &stale)
                        .await,
                    Err(CoreExtractionRuntimeError::InvalidTerminalWinner)
                ));
            }
            let retired = runtime
                .retire_actor_after_other_terminal(
                    actor_lease,
                    &committed_terminal_coordinator(kind),
                )
                .await
                .expect("retire losing extraction");
            assert!(!retired.committed_result_retained);
            assert!(retired.zero_actor_residue);
            assert!(route_directory.snapshot(route_lease).is_err());
            assert!(runtime.begin_shutdown().await.is_empty());
            assert!(runtime.finish_shutdown().await.unwrap().zero_residue);
            route_directory.begin_shutdown();
            assert!(
                route_directory
                    .finish_shutdown()
                    .await
                    .unwrap()
                    .zero_residue
            );
        }
    }

    #[tokio::test]
    async fn authority_requires_the_committed_capacity_one_boss_exit_ready_projection() {
        let (directory, _lease, permit) = reserved_route().await;
        let hidden = ProductionExtractionBossExitAuthorityV1::seal(
            authenticated(),
            [2; 16],
            permit.clone(),
            &presentation(false),
            &lock(),
            participant(),
            versions(),
        );
        assert_eq!(
            hidden,
            Err(ProductionExtractionIntentError::ExitNotCommitted)
        );

        let mut group = lock();
        group.participants.push(CoreBossParticipant {
            entity_id: EntityId::new(42).expect("second entity"),
            party_slot: 1,
        });
        let group = ProductionExtractionBossExitAuthorityV1::seal(
            authenticated(),
            [2; 16],
            permit,
            &presentation(true),
            &group,
            participant(),
            versions(),
        );
        assert_eq!(
            group,
            Err(ProductionExtractionIntentError::InvalidRouteAuthority)
        );
        directory.begin_shutdown();
        assert!(directory.finish_shutdown().await.unwrap().zero_residue);
    }

    #[tokio::test]
    async fn exact_intent_replay_keeps_the_original_tick_and_preparation() {
        let planner = FakePlanner::stable();
        let (authority, directory, lease) = authority().await;
        let instance_lineage_id = authority.instance_lineage_id;
        let actor = ProductionExtractionIntentActor::new(
            authority,
            directory.clone(),
            lease,
            planner.clone(),
            FixedClock(10_000),
        )
        .expect("intent actor");
        let frame = frame(1);
        let first = actor.handle(authenticated(), &frame, 700).await;
        assert_eq!(first.server_tick, 700);
        assert!(matches!(
            first.result,
            ExtractionCommitResultV1::Pending { .. }
        ));
        let mut replay_frame = frame.clone();
        replay_frame.sequence = 2;
        let replay = actor.handle(authenticated(), &replay_frame, 999).await;
        assert_eq!(replay.server_tick, 700);
        assert!(matches!(
            replay.result,
            ExtractionCommitResultV1::Pending {
                request_sequence: 2,
                ..
            }
        ));
        assert_eq!(planner.calls.load(Ordering::SeqCst), 1);

        let intent = actor.prepared_intent().await.expect("prepared intent");
        assert_eq!(intent.acceptance().attempt.actor_generation, 7);
        assert_eq!(
            intent
                .acceptance()
                .attempt
                .accepted_post_route_state_version,
            intent.acceptance().attempt.accepted_pre_route_state_version + 1
        );
        assert_eq!(
            intent
                .acceptance()
                .attempt
                .core_route_revision
                .records_blake3,
            "d".repeat(64)
        );
        assert_eq!(
            intent
                .acceptance()
                .attempt
                .world_flow_revision
                .records_blake3,
            "a".repeat(64)
        );
        let prepared = intent.prepared().expect("repository preparation");
        assert_eq!(prepared.request().observed_tick, 700);
        assert_eq!(prepared.request().instance_lineage_id, instance_lineage_id);
        assert_eq!(
            intent.input().staged_request().extraction_request_id,
            prepared.request().extraction_request_id
        );
        assert_eq!(
            intent.input().staged_request().entry_restore_point_id,
            [4; 16]
        );

        let mut changed_mutation = frame.clone();
        changed_mutation.sequence = 3;
        changed_mutation.mutation_id = [7; 16];
        let mut changed_versions = frame.clone();
        changed_versions.sequence = 4;
        changed_versions.payload.expected_versions.inventory += 1;
        changed_versions.payload_hash = changed_versions.payload.canonical_hash();
        let mut changed_content = frame.clone();
        changed_content.sequence = 5;
        changed_content.payload.content_revision.records_blake3 =
            ManifestHash::new("9".repeat(64)).expect("changed content");
        changed_content.payload_hash = changed_content.payload.canonical_hash();
        let mut changed_character = frame;
        changed_character.sequence = 6;
        changed_character.character_id = [8; 16];
        for changed in [
            changed_mutation,
            changed_versions,
            changed_content,
            changed_character,
        ] {
            let conflict = actor.handle(authenticated(), &changed, 1_000).await;
            assert!(matches!(
                conflict.result,
                ExtractionCommitResultV1::Rejected {
                    code: TerminalInventoryRejectionCodeV1::IdempotencyConflict,
                    ..
                }
            ));
        }
        assert_eq!(planner.calls.load(Ordering::SeqCst), 1);
        assert_eq!(planner.accept_calls.load(Ordering::SeqCst), 5);
        directory.begin_shutdown();
        assert!(directory.finish_shutdown().await.unwrap().zero_residue);
    }

    #[tokio::test]
    async fn actor_replacement_recovers_the_first_accepted_tick_before_replanning() {
        let planner = FakePlanner::stable();
        let (authority, directory, lease) = authority().await;
        let actor = ProductionExtractionIntentActor::new(
            authority.clone(),
            directory.clone(),
            lease,
            planner.clone(),
            FixedClock(10_000),
        )
        .expect("intent actor");
        let frame = frame(1);
        let first = actor.handle(authenticated(), &frame, 700).await;
        assert_eq!(first.server_tick, 700);
        assert!(matches!(
            first.result,
            ExtractionCommitResultV1::Pending { .. }
        ));
        drop(actor);

        let replacement = ProductionExtractionIntentActor::new(
            authority,
            directory.clone(),
            lease,
            planner.clone(),
            FixedClock(10_000),
        )
        .expect("replacement intent actor");
        let mut replay_frame = frame;
        replay_frame.sequence = 2;
        let replay = replacement
            .handle(authenticated(), &replay_frame, 999)
            .await;
        assert_eq!(replay.server_tick, 700);
        assert!(matches!(
            replay.result,
            ExtractionCommitResultV1::Pending {
                request_sequence: 2,
                ..
            }
        ));
        assert_eq!(planner.accept_calls.load(Ordering::SeqCst), 1);
        assert_eq!(planner.calls.load(Ordering::SeqCst), 2);
        let recovered = replacement
            .prepared_intent()
            .await
            .expect("recovered intent");
        assert_eq!(recovered.server_tick(), 700);
        assert_eq!(recovered.input().commit_request().observed_tick, 700);

        directory.begin_shutdown();
        assert!(directory.finish_shutdown().await.unwrap().zero_residue);
    }

    #[tokio::test]
    async fn stale_foreign_and_content_drift_fail_before_the_planner() {
        let planner = FakePlanner::stable();
        let (authority, directory, lease) = authority().await;
        let actor = ProductionExtractionIntentActor::new(
            authority,
            directory.clone(),
            lease,
            planner.clone(),
            FixedClock(10_000),
        )
        .expect("intent actor");
        let mut stale = frame(1);
        stale.payload.expected_versions.inventory += 1;
        stale.payload_hash = stale.payload.canonical_hash();
        let result = actor.handle(authenticated(), &stale, 700).await;
        assert!(matches!(
            result.result,
            ExtractionCommitResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::StaleAuthority,
                ..
            }
        ));

        let mut drift = frame(2);
        drift.payload.content_revision.records_blake3 =
            ManifestHash::new("9".repeat(64)).expect("drift hash");
        drift.payload_hash = drift.payload.canonical_hash();
        let result = actor.handle(authenticated(), &drift, 701).await;
        assert!(matches!(
            result.result,
            ExtractionCommitResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::ContentMismatch,
                ..
            }
        ));

        let foreign = AuthenticatedAccount {
            account_id: AccountId::new([8; 16]).expect("foreign"),
            namespace: AuthenticatedNamespace::WipeableTest,
        };
        let result = actor.handle(foreign, &frame(3), 702).await;
        assert!(matches!(
            result.result,
            ExtractionCommitResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::ForeignAuthority,
                ..
            }
        ));
        assert_eq!(planner.calls.load(Ordering::SeqCst), 0);
        assert_eq!(planner.accept_calls.load(Ordering::SeqCst), 0);
        directory.begin_shutdown();
        assert!(directory.finish_shutdown().await.unwrap().zero_residue);
    }

    #[tokio::test]
    async fn future_issue_time_is_rejected_without_pinning_the_intent() {
        let planner = FakePlanner::stable();
        let (authority, directory, lease) = authority().await;
        let actor = ProductionExtractionIntentActor::new(
            authority,
            directory.clone(),
            lease,
            planner.clone(),
            FixedClock(10_000),
        )
        .expect("intent actor");
        let mut future = frame(1);
        future.issued_at_unix_millis = 10_001;
        let rejected = actor.handle(authenticated(), &future, 700).await;
        assert!(matches!(
            rejected.result,
            ExtractionCommitResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::IssuedAtInvalid,
                ..
            }
        ));
        assert!(actor.prepared_intent().await.is_none());
        assert_eq!(planner.calls.load(Ordering::SeqCst), 0);

        let mut boundary = frame(2);
        boundary.issued_at_unix_millis = 10_000;
        let accepted = actor.handle(authenticated(), &boundary, 701).await;
        assert!(matches!(
            accepted.result,
            ExtractionCommitResultV1::Pending { .. }
        ));
        assert_eq!(planner.calls.load(Ordering::SeqCst), 1);
        assert_eq!(planner.accept_calls.load(Ordering::SeqCst), 1);

        let mut changed_future = boundary;
        changed_future.sequence = 3;
        changed_future.mutation_id = [7; 16];
        changed_future.issued_at_unix_millis = 10_001;
        let rejected = actor.handle(authenticated(), &changed_future, 702).await;
        assert!(matches!(
            rejected.result,
            ExtractionCommitResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::IssuedAtInvalid,
                ..
            }
        ));
        assert_eq!(planner.calls.load(Ordering::SeqCst), 1);
        assert_eq!(planner.accept_calls.load(Ordering::SeqCst), 1);
        directory.begin_shutdown();
        assert!(directory.finish_shutdown().await.unwrap().zero_residue);
    }

    #[tokio::test]
    async fn retired_route_generation_cannot_accept_or_publish_an_intent() {
        let planner = FakePlanner::stable();
        let (authority, directory, lease) = authority().await;
        let actor = ProductionExtractionIntentActor::new(
            authority,
            directory.clone(),
            lease,
            planner.clone(),
            FixedClock(10_000),
        )
        .expect("intent actor");
        directory.retire_actor(lease).await.expect("retire route");

        let result = actor.handle(authenticated(), &frame(1), 700).await;
        assert!(matches!(
            result.result,
            ExtractionCommitResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::TerminalLost,
                ..
            }
        ));
        assert!(actor.prepared_intent().await.is_none());
        assert_eq!(planner.accept_calls.load(Ordering::SeqCst), 0);
        assert_eq!(planner.calls.load(Ordering::SeqCst), 0);
        directory.begin_shutdown();
        assert!(directory.finish_shutdown().await.unwrap().zero_residue);
    }

    #[tokio::test]
    async fn bounded_mailbox_uses_actor_tick_and_retries_one_pinned_database_attempt() {
        let planner = FakePlanner::fail_first();
        let (authority, directory, lease) = authority().await;
        let actor = ProductionExtractionIntentActor::new(
            authority,
            directory.clone(),
            lease,
            planner.clone(),
            FixedClock(10_000),
        )
        .expect("intent actor");
        let (handle, mut inbox) = production_extraction_actor_mailbox();
        let frame = frame(1);

        let first_request = handle.handle_extraction(authenticated(), &frame, 10);
        let first_serve = inbox.serve_next(&actor, 800);
        let (first, served) = tokio::join!(first_request, first_serve);
        assert!(served);
        assert_eq!(first.server_tick, 800);
        assert!(matches!(
            first.result,
            ExtractionCommitResultV1::Rejected {
                code: TerminalInventoryRejectionCodeV1::TerminalLost,
                ..
            }
        ));

        let retry_request = handle.handle_extraction(authenticated(), &frame, 11);
        let retry_serve = inbox.serve_next(&actor, 900);
        let (retry, served) = tokio::join!(retry_request, retry_serve);
        assert!(served);
        assert_eq!(retry.server_tick, 800);
        assert!(matches!(
            retry.result,
            ExtractionCommitResultV1::Pending { .. }
        ));
        assert_eq!(planner.calls.load(Ordering::SeqCst), 2);
        {
            let inputs = planner.inputs.lock().expect("planner inputs");
            assert_eq!(inputs.len(), 2);
            assert_eq!(inputs[0], inputs[1]);
            assert_eq!(inputs[0].commit_request().observed_tick, 800);
        }
        assert_eq!(planner.accept_calls.load(Ordering::SeqCst), 1);
        directory.begin_shutdown();
        assert!(directory.finish_shutdown().await.unwrap().zero_residue);
    }
}
